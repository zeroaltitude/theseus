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

impl Core {
    /// `judge.list`: the newest judgments that match, oldest first. With no
    /// pack, every pack this build embeds: each one's scope, merged by
    /// position.
    pub fn judge_list(&self, p: JudgeListParams) -> Result<JudgeListResult, RpcFailure> {
        let limit = p.limit.unwrap_or(LIST).clamp(1, MAX_LIST) as usize;
        let mut scopes: Vec<String> = match &p.pack {
            Some(pack) => vec![scope_of(pack)],
            None => theseus_judge::pack::embedded()
                .as_ref()
                .map(|packs| packs.iter().map(|p| scope_of(&p.id)).collect())
                .unwrap_or_default(),
        };
        scopes.sort();
        scopes.dedup();
        // A name with its version (`loop.v1`) is that version's alone.
        let version = p.pack.as_deref().filter(|n| n.contains('.'));
        let mut found = Vec::new();
        for scope in &scopes {
            for r in self.store.scope_after(scope, 0)? {
                let e = entry(&r)?;
                let keep = e.kind == theseus_protocol::LedgerKind::JudgeCall.as_str()
                    && version.is_none_or(|v| e.data["pack"].as_str() == Some(v))
                    && p.session_id
                        .as_deref()
                        .is_none_or(|s| e.session_id.as_deref() == Some(s))
                    && p.since.is_none_or(|t| e.at_unix_ms >= t);
                if keep {
                    found.push(e);
                }
            }
        }
        found.sort_by_key(|e| e.position);
        let matched = found.len() as u64;
        let judgments = found.split_off(found.len().saturating_sub(limit));
        Ok(JudgeListResult {
            scopes,
            matched,
            judgments,
        })
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
