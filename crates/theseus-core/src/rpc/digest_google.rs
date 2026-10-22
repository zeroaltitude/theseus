//! The digest's Google line (theseus-batvg.6): what the owner's Google
//! account did in the window, counted from the `google.called` rows (their
//! method and outcome, never their content: the rows hold none) and from the
//! open mail watches. Nothing is read when `[google]` binds no account, so a
//! digest without Google is the one it was.
//!
//! - **Mail sent**: successful `gmail.users.drafts.send` calls.
//! - **Drafts waiting**: drafts a successful create or update named, with no
//!   later successful send of the same draft in the window.
//! - **Waiting on replies**: mail watches open now.
//! - **Docs written**: successful `drive.write` creates and updates.

use std::collections::BTreeSet;

use theseus_protocol::digest_google::DigestGoogle;
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use super::Core;
use crate::ledger::LedgerRow;

/// The `google.called` rows one digest reads, at most.
const MAX_ROWS: usize = 20_000;
const PAGE_ROWS: usize = 500;

impl Core {
    /// The window's `google.called` rows after `since`, oldest first.
    fn google_rows(&self, since: u64) -> Vec<LedgerRow> {
        let tag = ledger_kind(LedgerKind::GoogleCalled.as_str());
        let (mut rows, mut cursor) = (Vec::new(), since);
        while rows.len() < MAX_ROWS {
            let page = Page {
                kind: kinds::LEDGER,
                tags: vec![tag.clone()],
                after: Some(cursor),
                before: None,
                since_ms: None,
                until_ms: None,
                limit: PAGE_ROWS,
            };
            // The index's shape is built after serving: the tail stands in.
            let Ok(Some(out)) = self.store.ledger_page(&page) else {
                let tail: Vec<(u64, LedgerRow)> =
                    self.store.ledger_tail(MAX_ROWS).unwrap_or_default();
                return tail
                    .into_iter()
                    .filter(|(p, r)| *p > since && r.is_kind(LedgerKind::GoogleCalled.as_str()))
                    .map(|(_, r)| r)
                    .collect();
            };
            rows.extend(
                out.records
                    .iter()
                    .filter_map(|r| r.decode::<LedgerRow>().ok()),
            );
            match out.last {
                Some(l) if out.more => cursor = l,
                _ => break,
            }
        }
        rows
    }

    /// The Google line's counts for the window after `since`: none when
    /// `[google]` binds no account or every count is 0.
    pub(crate) fn digest_google(&self, since: u64) -> Option<DigestGoogle> {
        if self.cfg.google.is_empty() {
            return None;
        }
        let (mut sent, mut docs) = (0u32, 0u32);
        let mut waiting = BTreeSet::new();
        for row in self.google_rows(since) {
            let d = &row.data;
            if d["outcome"] != "ok" {
                continue;
            }
            let id = d["resource"].as_str().map(str::to_string);
            match d["method"].as_str().unwrap_or("") {
                "gmail.users.drafts.send" => {
                    sent += 1;
                    waiting.remove(&id.unwrap_or_default());
                }
                "gmail.users.drafts.create" | "gmail.users.drafts.update" => {
                    waiting.extend(id);
                }
                "drive.files.create" | "drive.files.update"
                    if d["tool"] == "drive.write" && d["class"] == "write" =>
                {
                    docs += 1;
                }
                _ => {}
            }
        }
        let replies = self
            .watch_list(None)
            .map(|l| l.iter().filter(|w| w.kind == "mail").count())
            .unwrap_or(0);
        let g = DigestGoogle {
            mail_sent: sent,
            drafts_waiting: waiting.len() as u32,
            waiting_on_replies: replies as u32,
            docs_written: docs,
        };
        (!g.is_empty()).then_some(g)
    }
}
