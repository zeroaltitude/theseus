//! `judge.list` and `judge.get` (M5 23b; design §2.13): Jev's judgments as
//! their `judge.call` rows, each pack's history one scope (`judge:<pack
//! id>`), and one judgment, keyed by its id, with the state its blob holds.
//! Reads; nothing is written.

use theseus_protocol::error_code;
use theseus_protocol::judge::{JudgeGetParams, JudgeGetResult, JudgeListParams, JudgeListResult};
use theseus_protocol::LedgerEntry;

use super::server::RpcFailure;
use super::Core;
use crate::ledger::LedgerRow;

/// The judgments `judge.list` gives by default, and at most.
const LIST: u64 = 50;
const MAX_LIST: u64 = 500;
/// The fewest rows a page of `judge.call` rows asks for.
const PAGE: usize = 64;

/// A listing: its judgments, `matched`, `more`, and what the read cost.
type Listed = (Vec<LedgerEntry>, u64, bool, ListRead);

/// A pack's scope, from its name (`loop.v1`) or its id (`loop`).
pub fn scope_of(pack: &str) -> String {
    format!("judge:{}", pack.split('.').next().unwrap_or(pack))
}

fn entry(r: &theseus_store::Record) -> anyhow::Result<LedgerEntry> {
    let row: LedgerRow = r.decode()?;
    Ok(LedgerEntry {
        position: r.position,
        at_unix_ms: row.at_unix_ms,
        kind: row.kind,
        session_id: row.session_id,
        turn_id: row.turn_id,
        data: row.data,
    })
}

/// What a `judge.list` read cost: the records it decoded (theseus-wse2).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListRead {
    pub decoded: usize,
    /// Whether it paged back through the index, or scanned the scopes.
    pub paged: bool,
}

/// The listing's question, settled from its params.
struct Ask<'a> {
    p: &'a JudgeListParams,
    scopes: Vec<String>,
    /// A name with its version (`loop.v1`) is that version's alone.
    version: Option<&'a str>,
    limit: usize,
}

impl Ask<'_> {
    fn keeps(&self, e: &LedgerEntry) -> bool {
        e.kind == theseus_protocol::LedgerKind::JudgeCall.as_str()
            && self
                .version
                .is_none_or(|v| e.data["pack"].as_str() == Some(v))
            && self
                .p
                .session_id
                .as_deref()
                .is_none_or(|s| e.session_id.as_deref() == Some(s))
            && self.p.since.is_none_or(|t| e.at_unix_ms >= t)
    }
}

impl Core {
    /// `judge.list`: the newest judgments that match, oldest first. With no
    /// pack, every pack this build embeds: each one's scope. Read from the
    /// newest `judge.call` row back, through the index's tags, until one
    /// past the limit matches (theseus-wse2); the scopes scanned whole only
    /// while the index's shape is built after serving.
    pub fn judge_list(&self, p: JudgeListParams) -> Result<JudgeListResult, RpcFailure> {
        Ok(self.judge_list_read(&p, true)?.0)
    }

    /// `judge.list`, and what its read cost; `paged: false` scans the
    /// scopes, as every read did before the pages (a test's baseline).
    pub(crate) fn judge_list_read(
        &self,
        p: &JudgeListParams,
        paged: bool,
    ) -> anyhow::Result<(JudgeListResult, ListRead)> {
        let mut scopes: Vec<String> = match &p.pack {
            Some(pack) => vec![scope_of(pack)],
            None => theseus_judge::pack::embedded()
                .as_ref()
                .map(|packs| packs.iter().map(|p| scope_of(&p.id)).collect())
                .unwrap_or_default(),
        };
        scopes.sort();
        scopes.dedup();
        let ask = Ask {
            p,
            scopes,
            version: p.pack.as_deref().filter(|n| n.contains('.')),
            limit: p.limit.unwrap_or(LIST).clamp(1, MAX_LIST) as usize,
        };
        let read = match paged {
            true => self.judge_list_paged(&ask)?,
            false => None,
        };
        let (judgments, matched, more, cost) = match read {
            Some(r) => r,
            None => self.judge_list_scanned(&ask)?,
        };
        Ok((
            JudgeListResult {
                scopes: ask.scopes,
                matched,
                more,
                judgments,
            },
            cost,
        ))
    }

    /// The scopes whole, from position 0: every match counted, then the
    /// newest `limit` kept.
    fn judge_list_scanned(&self, ask: &Ask<'_>) -> anyhow::Result<Listed> {
        let mut cost = ListRead::default();
        let mut found = Vec::new();
        for scope in &ask.scopes {
            for r in self.store.scope_after(scope, 0)? {
                cost.decoded += 1;
                let e = entry(&r)?;
                if ask.keeps(&e) {
                    found.push(e);
                }
            }
        }
        found.sort_by_key(|e| e.position);
        let matched = found.len() as u64;
        let judgments = found.split_off(found.len().saturating_sub(ask.limit));
        Ok((judgments, matched, false, cost))
    }

    /// The `judge.call` rows from the newest back (the kind's tag, or the
    /// kind's in the session), since `since` by the ledger's clock, each
    /// in a listed scope and passing the filter, until `limit` and one
    /// more match: that one is `more`'s proof, and not shown. `None` while
    /// the index's shape is built.
    fn judge_list_paged(&self, ask: &Ask<'_>) -> anyhow::Result<Option<Listed>> {
        use theseus_store::pages::{ledger_kind, ledger_kind_session};
        let kind = theseus_protocol::LedgerKind::JudgeCall.as_str();
        let tag = match ask.p.session_id.as_deref() {
            Some(s) => ledger_kind_session(kind, s),
            None => ledger_kind(kind),
        };
        let mut cost = ListRead {
            decoded: 0,
            paged: true,
        };
        let mut found: Vec<LedgerEntry> = Vec::new();
        let mut more = false;
        let mut before = None;
        'pages: loop {
            let page = theseus_store::Page {
                kind: theseus_store::kinds::LEDGER,
                tags: vec![tag.clone()],
                after: None,
                before,
                since_ms: ask.p.since,
                until_ms: None,
                limit: (ask.limit + 1).max(PAGE),
            };
            let Some(out) = self.store.ledger_page(&page)? else {
                return Ok(None);
            };
            for r in out.records.iter().rev() {
                if !r.scope.as_ref().is_some_and(|s| ask.scopes.contains(s)) {
                    continue;
                }
                cost.decoded += 1;
                let e = entry(r)?;
                if !ask.keeps(&e) {
                    continue;
                }
                if found.len() == ask.limit {
                    more = true;
                    break 'pages;
                }
                found.push(e);
            }
            match (out.more, out.first) {
                (true, Some(first)) => before = Some(first),
                _ => break,
            }
        }
        found.reverse();
        let matched = found.len() as u64 + u64::from(more);
        Ok(Some((found, matched, more, cost)))
    }

    /// `judge.get`: one judgment's row, and its state from the blob its row
    /// names (`context.blob`, the state's digest).
    pub fn judge_get(&self, p: JudgeGetParams) -> Result<JudgeGetResult, RpcFailure> {
        let not_found = || RpcFailure::new(error_code::NOT_FOUND, format!("no judgment {}", p.id));
        let r = self.store.ledger_by_key(&p.id)?.ok_or_else(not_found)?;
        let judgment = entry(&r)?;
        if judgment.kind != theseus_protocol::LedgerKind::JudgeCall.as_str() {
            return Err(not_found());
        }
        let digest = judgment.data["context"]["blob"]
            .as_str()
            .or_else(|| judgment.data["state"]["sha256"].as_str())
            .unwrap_or_default()
            .to_string();
        let state = self
            .store
            .blobs()
            .base64(&digest)
            .and_then(|b| crate::blobs::decode(&b).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let state_missing = state.is_none().then(|| {
            if digest.is_empty() {
                "its row names no blob".to_string()
            } else {
                format!("blob {digest} is missing, or no longer matches its digest")
            }
        });
        Ok(JudgeGetResult {
            judgment,
            state,
            state_missing,
        })
    }
}
