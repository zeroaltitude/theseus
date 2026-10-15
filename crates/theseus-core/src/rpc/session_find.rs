//! `session.list { dir, with_text }` (theseus-6yka): the conversations of one
//! directory (or, without a `dir`, of every one), for `theseus -c` and `-r`. A session belongs to a directory
//! when its recorded directory and the asked one have the same root: the git
//! repository holding each (a `.git` entry on the way up), else the directory
//! itself. So a conversation started in `repo/src` is found from `repo`.
//!
//! The page is read as `session.list { n, before }` reads one (through the
//! index's births, newest opened first, its cursor the `older` the answer
//! gives): the births are scanned a step at a time until `n` sessions of the
//! directory are found, and the answer holds every one of the steps read (so
//! it can hold more than `n`, and its cursor goes back from the last step),
//! put newest active first. `with_text` adds each session's first user message
//! and its last message, cut short, for a picker's search: a read of the
//! store, no model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use theseus_protocol::{SessionInfo, SessionListParams, SessionListResult};

use super::server::RpcFailure;
use super::Core;
use crate::session::SessionRecord;

/// Sessions read from the index at a time while looking for the directory's.
const SCAN: usize = 500;
/// What a page holds when the caller names no `n`.
const DEFAULT_N: usize = 200;
/// The longest text kept of a message.
const TEXT_CHARS: usize = 400;
/// Nodes read from each end of a session to find its first and last message.
const ENDS: usize = 8;
/// How many of a session's newest user messages a search reads, and how much
/// of each: its head and its tail.
const PROMPTS: usize = 12;
const PROMPT_ENDS: usize = 120;

/// The root of `dir`: the nearest ancestor (itself included) holding a `.git`
/// entry, else `dir`.
pub fn root_of(dir: &str) -> PathBuf {
    let p = Path::new(dir);
    p.ancestors()
        .find(|d| d.join(".git").exists())
        .unwrap_or(p)
        .to_path_buf()
}

impl Core {
    /// The answer to `session.list` with a `dir`.
    pub(super) fn session_list_in(
        &self,
        p: SessionListParams,
        dir: Option<&str>,
    ) -> Result<SessionListResult, RpcFailure> {
        self.session_list_scanning(p, dir, SCAN)
    }

    /// `session_list_in`, reading `scan` births at a step.
    pub(super) fn session_list_scanning(
        &self,
        p: SessionListParams,
        dir: Option<&str>,
        scan: usize,
    ) -> Result<SessionListResult, RpcFailure> {
        let want = dir.map(root_of);
        let n = p.n.unwrap_or(DEFAULT_N).clamp(1, 1000);
        let mut roots: HashMap<String, PathBuf> = HashMap::new();
        let mut found: Vec<SessionRecord> = Vec::new();
        let mut before = p.before;
        let older = loop {
            let (recs, next) = match self.sessions_paged(scan, before)? {
                Some(page) => page,
                None => {
                    // The index is not built yet: every session is read, once.
                    let mut all: Vec<SessionRecord> = self.store.live_sessions()?;
                    all.retain(|r| before.is_none_or(|b| r.created_at_unix_ms < b));
                    all.sort_by_key(|r| std::cmp::Reverse(r.created_at_unix_ms));
                    (all, None)
                }
            };
            for r in recs {
                let here = match &want {
                    None => true,
                    Some(want) => r.dir.as_deref().is_some_and(|d| {
                        roots.entry(d.to_string()).or_insert_with(|| root_of(d)) == want
                    }),
                };
                if here {
                    found.push(r);
                }
            }
            // Enough, at the end of a step: the next page goes back from it.
            if found.len() >= n || next.is_none() {
                break next;
            }
            before = next;
        };
        found.sort_by_key(|r| std::cmp::Reverse(r.last_active_ms.max(r.created_at_unix_ms)));
        let pending = self.pending_by_execution(
            &self
                .kernel
                .pending_confirms()?
                .into_iter()
                .filter(|a| found.iter().any(|r| r.session_id == a.session_id))
                .collect::<Vec<_>>(),
            None,
        );
        let mut sessions: Vec<SessionInfo> = found
            .iter()
            .map(|r| {
                let mut i = self.session_info(r, &pending);
                if p.with_text {
                    (i.first_text, i.last_text, i.prompts) = self.words_of(&r.session_id);
                }
                i
            })
            .collect();
        if let Some(want) = p.state {
            sessions.retain(|s| s.state == Some(want));
        }
        Ok(SessionListResult {
            sessions,
            older,
            live_window_ms: None,
            empty_grace_ms: None,
        })
    }

    /// Where a session started, as far as the store says: `discord` when
    /// it speaks to a place there, `cli` when a client gave it a directory.
    pub(super) fn started_on(&self, r: &SessionRecord) -> Option<String> {
        let target = self.outbox.try_target(&r.session_id).ok().flatten();
        match (target, &r.dir) {
            (Some(_), _) => Some("discord".to_string()),
            (None, Some(_)) => Some("cli".to_string()),
            (None, None) => None,
        }
    }

    /// The words a picker searches in a session: its first user message, its
    /// last message, and the head and tail of each of its newest user
    /// messages (a question often follows a long paste), read through the
    /// index's pages of the session's nodes.
    fn words_of(&self, session: &str) -> (Option<String>, Option<String>, Option<String>) {
        let text = |nodes: Vec<(u64, crate::node::Node)>, keep: &dyn Fn(&str) -> bool| {
            nodes
                .iter()
                .map(|(pos, n)| Self::node_info(*pos, n))
                .filter(|i| keep(&i.kind))
                .map(|i| i.text)
                .filter(|t| !t.trim().is_empty())
                .collect::<Vec<String>>()
        };
        let any = |k: &str| k == "user_message" || k == "assistant_message";
        let first = self
            .history_page(session, Some(ENDS), Some(0), None)
            .ok()
            .and_then(|h| text(h.nodes, &|k| k == "user_message").into_iter().next())
            .map(|t| cut(&t, TEXT_CHARS));
        let last = self
            .history_page(session, Some(ENDS), None, None)
            .ok()
            .and_then(|h| text(h.nodes, &any).pop())
            .map(|t| cut(&t, TEXT_CHARS));
        let prompts = self
            .nodes_paged(Some("user_message"), Some(session), PROMPTS)
            .ok()
            .flatten()
            .map(|nodes| text(nodes, &|_| true))
            .map(|all| {
                all.iter()
                    .map(|t| ends(t, PROMPT_ENDS))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|t| !t.is_empty());
        (first, last, prompts)
    }
}

/// `text` cut to `max` characters.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The first and the last `each` characters of `text`, or all of it when it
/// is short.
fn ends(text: &str, each: usize) -> String {
    let n = text.chars().count();
    if n <= each * 2 + 3 {
        return text.to_string();
    }
    let tail: String = text.chars().skip(n - each).collect();
    format!("{} … {tail}", cut(text, each))
}
