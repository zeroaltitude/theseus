//! `prefix.report` (theseus-rr15): the prefix library's picks, their cache
//! reads and the false fits (`prefix::report`), for `theseus model prefix`.
//! A read for the owner, from a surface that reads private text: the picks
//! name sessions and turns. It writes nothing and takes no lock.

use std::collections::BTreeSet;

use theseus_protocol::error_code;
use theseus_protocol::prefix::{PrefixReport, PrefixReportParams};
use theseus_store::pages::{ledger_kind, ledger_kind_session};
use theseus_store::{kinds, Page};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Surface;
use crate::ledger::LedgerRow;
use crate::node::Body;
use crate::prefix::report::{self, Next};
use crate::store::Store;
use crate::stub::Kind;

/// The newest rows with any of `tags`, at most `limit`, oldest first.
fn newest(store: &Store, tags: Vec<String>, limit: usize) -> anyhow::Result<Vec<LedgerRow>> {
    let mut out = Vec::new();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: None,
            until_ms: None,
            limit: 512.min(limit.max(1)),
        };
        let Some(got) = store.ledger_page(&page)? else {
            break;
        };
        for r in got.records.iter().rev() {
            if let Ok(row) = r.decode::<LedgerRow>() {
                out.push(row);
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) if out.len() < limit => before = Some(first),
            _ => break,
        }
    }
    out.truncate(limit);
    out.reverse();
    Ok(out)
}

/// The owner's first message after `turn` in `session`, and whether it asks
/// what that turn's message asked.
fn next_after(store: &Store, session: &str, turn: &str) -> Option<Next> {
    let nodes = store.transcript(session).ok()?;
    let mut asked: Option<String> = None;
    for (_, n) in nodes.iter().filter(|(_, n)| n.kind == Kind::UserMessage) {
        let Body::UserMessage { text, .. } = &n.body else {
            continue;
        };
        match (&asked, n.turn_id.as_deref()) {
            (None, Some(t)) if t == turn => asked = Some(text.clone()),
            (Some(a), Some(t)) if t != turn => {
                return Some(Next {
                    session_id: session.into(),
                    turn_id: t.into(),
                    re_ask: report::re_ask(a, text),
                });
            }
            _ => {}
        }
    }
    None
}

impl Core {
    pub(super) async fn prefix_report(
        &self,
        p: PrefixReportParams,
        surface: Surface,
    ) -> Result<PrefixReport, RpcFailure> {
        if !surface.reads_private() {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                format!(
                    "{} reads no private text: the prefix report is the owner's, from the CLI or \
                     the web UI",
                    surface.name()
                ),
            ));
        }
        let limit = p.limit.unwrap_or(2_000).clamp(1, 20_000) as usize;
        let store = self.store.clone();
        let mut r = tokio::task::spawn_blocking(move || -> anyhow::Result<PrefixReport> {
            let picks = newest(&store, vec![ledger_kind("prefix.picked")], limit)?;
            let sessions: BTreeSet<&str> = picks
                .iter()
                .filter_map(|p| p.session_id.as_deref())
                .collect();
            let (mut calls, mut wrongs, mut nexts) = (Vec::new(), Vec::new(), Vec::new());
            for s in &sessions {
                calls.extend(newest(
                    &store,
                    vec![ledger_kind_session("provider.call", s)],
                    limit,
                )?);
                let tags = report::WRONG
                    .iter()
                    .map(|k| ledger_kind_session(k, s))
                    .collect();
                wrongs.extend(newest(&store, tags, limit)?);
            }
            for p in picks
                .iter()
                .filter(|p| p.data["below_act"].as_bool() == Some(true))
            {
                if let (Some(s), Some(t)) = (p.session_id.as_deref(), p.turn_id.as_deref()) {
                    nexts.extend(next_after(&store, s, t));
                }
            }
            Ok(report::build(&picks, &calls, &wrongs, &nexts))
        })
        .await
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("the report stopped: {e}")))?
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))?;
        let stats = self.runner.prefix.stats(theseus_protocol::now_unix_ms());
        r.enabled = self.cfg.prefix.enabled;
        r.headers = stats.headers;
        r.bytes = stats.bytes;
        r.warm = stats.warm;
        r.max_bytes = self.cfg.prefix.max_bytes;
        Ok(r)
    }
}
