//! Diagnostics, pushed and pulled, and the bounded wait for "this file's
//! diagnostics after this change".
//!
//! - **Pushed** (`textDocument/publishDiagnostics`): kept per document, the
//!   last list with the version it was for (when the server says) and the
//!   count of messages this client had sent when it arrived.
//! - **Pulled** (`textDocument/diagnostic`, when the server declares or
//!   registers `diagnosticProvider`): asked for, with the last result id, so
//!   an unchanged answer reuses the last list. ty and TypeScript 7's server
//!   only pull.
//!
//! [`Client::diagnostics`] is the one call L3 makes after an edit: the
//! document is brought up to date with the disk, then, within a bound, the
//! server is asked (pull) or waited on (push) for a list that is for the
//! current version. A list pushed with a version is current when its version
//! is; one pushed with none is current when it arrived after the change was
//! sent. When the bound runs out the answer says so ([`Freshness::Stale`]),
//! with the last list it has, which may be for an older version, or none.
//!
//! **The check after a save** (`Options::check_token`, theseus-c6hv).
//! rust-analyzer's pull answers with its own analysis alone; rustc's errors
//! it pushes, from the `cargo check` it runs after a save, reported as
//! work-done progress under its check token. So for a saved document, after
//! the pull, the wait goes on, within the same bound, until every check
//! begun since the last save has ended, and the list pushed for this version
//! is added to the pulled one, each item once. A server busy loading runs no
//! check, so when none begins within [`CHECK_GRACE`] of the save, or of the
//! server's readiness if that came later, it is taken to run none (checks
//! off), and the answer is the pull's. At the bound the answer is stale,
//! with what it has. An unsaved document, or a server without a check
//! token, waits for no check.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::{Client, Error, Event, Shared};
use crate::docs::Doc;
use crate::jsonrpc::code;
use crate::types::{Diagnostic, DocumentDiagnosticReport, PublishDiagnosticsParams};
use crate::uri;

/// How soon after a save (or after the server is ready, if later) its check
/// must begin to be waited for: past it, the server is taken to run none.
pub const CHECK_GRACE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub(crate) struct Pushed {
    pub(crate) version: Option<i32>,
    pub(crate) items: Vec<Diagnostic>,
    /// The count of messages sent when it arrived.
    pub(crate) arrived_at: u64,
}

impl Pushed {
    /// Whether it is for `version` of `doc`: by its own version, or, with
    /// none, by arriving after that version was sent.
    fn current(&self, doc: &Doc, version: i32) -> bool {
        match self.version {
            Some(v) => v == version,
            None => doc.version == version && self.arrived_at >= doc.sent_at,
        }
    }
}

/// The after-save checks of a server with a check token, by the count of
/// messages sent when each began and ended.
#[derive(Debug, Default)]
pub(crate) struct Checks {
    /// Begun and not ended, by token: when each began.
    running: HashMap<String, u64>,
    /// When the latest check began.
    begun: u64,
    /// When the latest check ended.
    ended_at: u64,
}

impl Checks {
    pub(crate) fn begin(&mut self, token: &str, sent: u64) {
        self.running.insert(token.to_string(), sent);
        self.begun = self.begun.max(sent);
    }

    pub(crate) fn end(&mut self, token: &str, sent: u64) {
        self.running.remove(token);
        self.ended_at = sent;
    }

    /// Whether a check began since the save sent as message `save`, and
    /// whether one begun since still runs.
    fn since(&self, save: u64) -> (bool, bool) {
        (
            self.begun >= save,
            self.running.values().any(|b| *b >= save),
        )
    }
}

/// The answer had before a check's wait: for which version, and the count
/// of messages sent before it was asked for.
struct Answer {
    version: i32,
    items: Vec<Diagnostic>,
    freshness: Freshness,
    asked: u64,
}

/// What became of the wait on a save's check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    /// Every check begun since the save has ended; the count of messages
    /// sent when the last one ended.
    Ended(u64),
    /// None began within the grace.
    NoneBegan,
    /// The bound ran out, or the connection ended, first.
    Unfinished,
}

#[derive(Debug, Clone)]
pub(crate) struct Pulled {
    pub(crate) result_id: Option<String>,
    pub(crate) items: Vec<Diagnostic>,
}

/// How current a list of diagnostics is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Pulled after the change: the server's answer for this version.
    Pulled,
    /// Pushed for this version (or, from a server that gives no versions,
    /// after the change was sent).
    Pushed,
    /// The bound ran out first: the last list known, which may be older.
    Stale,
}

/// One document's diagnostics, and how they were had.
#[derive(Debug, Clone, PartialEq)]
pub struct FileDiagnostics {
    pub uri: String,
    /// The document's version they were asked for.
    pub version: i32,
    pub items: Vec<Diagnostic>,
    pub freshness: Freshness,
    /// How long the call took, the sync and the wait included.
    pub waited: Duration,
}

impl FileDiagnostics {
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.is_error())
    }
}

impl Shared {
    pub(crate) fn published(&self, params: Value) {
        let p: PublishDiagnosticsParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "lsp: unreadable publishDiagnostics, skipped");
                return;
            }
        };
        let key = uri::normalize(&p.uri);
        let count = p.diagnostics.len();
        {
            let mut st = self.lock();
            let arrived_at = st.sent;
            st.pushed.insert(
                key.clone(),
                Pushed {
                    version: p.version,
                    items: p.diagnostics,
                    arrived_at,
                },
            );
        }
        self.event(Event::Diagnostics {
            uri: key,
            version: p.version,
            count,
        });
        self.bump();
    }
}

impl Client {
    /// The last list pushed for a file, and the version it was for.
    pub fn pushed(&self, path: &Path) -> Option<(Option<i32>, Vec<Diagnostic>)> {
        let uri = self.uri(path).ok()?;
        let st = self.shared().lock();
        st.pushed.get(&uri).map(|p| (p.version, p.items.clone()))
    }

    /// Each document's count of errors in the last list pushed for it, by
    /// URI: what L3 compares before and after an edit to count the new
    /// errors in other files.
    pub fn pushed_errors(&self) -> std::collections::BTreeMap<String, usize> {
        let st = self.shared().lock();
        st.pushed
            .iter()
            .map(|(u, p)| (u.clone(), p.items.iter().filter(|d| d.is_error()).count()))
            .collect()
    }

    /// This file's diagnostics for its current text, within `bound`: open
    /// it if it is not, sync it with the disk, then pull (when the server
    /// can) or wait for a push for this version. A server that reports a
    /// status (rust-analyzer) is waited on until it is quiescent first,
    /// within the same bound. A saved document on a server that checks
    /// after a save also waits for that check, and takes its pushed list.
    pub async fn diagnostics(
        &self,
        path: &Path,
        bound: Duration,
    ) -> Result<FileDiagnostics, Error> {
        let start = Instant::now();
        let deadline = tokio::time::Instant::now() + bound;
        let uri = self.prepare(path).await?;
        if self.shared().opts.expects_server_status {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let _ = self.wait_ready(left).await;
        }
        let ready = tokio::time::Instant::now();
        let (version, pull, saved, asked) = {
            let st = self.shared().lock();
            let doc = st.docs.get(&uri);
            let v = doc.map_or(0, |d| d.version);
            let saved = doc.is_some_and(|d| d.saved.is_some());
            (v, st.caps.pull_diagnostics, saved, st.sent)
        };
        // A server may register the pull after `initialized` (ty does): a
        // push wait that sees the registration pulls instead.
        let pushed = if pull {
            None
        } else {
            self.wait_push(&uri, deadline).await
        };
        let (mut items, mut freshness) = match pushed {
            Some(got) => got,
            None => self.pull(&uri, deadline).await?,
        };
        let checks = self.shared().opts.check_token.is_some();
        if checks && saved && freshness != Freshness::Stale {
            let answer = Answer {
                version,
                items,
                freshness,
                asked,
            };
            (items, freshness) = self.with_check(&uri, answer, ready, deadline).await?;
        }
        Ok(FileDiagnostics {
            uri,
            version,
            items,
            freshness,
            waited: start.elapsed(),
        })
    }

    /// A saved document's answer, after its check: wait for the check
    /// within the deadline, then add what it pushed for this version.
    async fn with_check(
        &self,
        uri: &str,
        a: Answer,
        ready: tokio::time::Instant,
        deadline: tokio::time::Instant,
    ) -> Result<(Vec<Diagnostic>, Freshness), Error> {
        let (mut items, mut freshness) = (a.items, a.freshness);
        match self.wait_check(ready, deadline).await {
            Check::NoneBegan => return Ok((items, freshness)),
            Check::Unfinished => freshness = Freshness::Stale,
            // A server may write a check's end before its push (both in one
            // turn of its loop), and the answer to a pull sent after the end
            // arrived comes after that push: so the pull is asked again,
            // unless the first was sent after the end arrived.
            Check::Ended(ended_at) if freshness == Freshness::Pulled && ended_at > a.asked => {
                let (again, f) = self.pull(uri, deadline).await?;
                if f == Freshness::Stale {
                    freshness = Freshness::Stale;
                } else {
                    items = again;
                }
            }
            Check::Ended(_) => {}
        }
        let pulled = a.freshness == Freshness::Pulled;
        Ok((self.with_pushed(uri, a.version, items, pulled), freshness))
    }

    /// Wait until every check begun since the last save has ended, or none
    /// has begun within the grace of the save (or of `ready`, if later), or
    /// the deadline.
    async fn wait_check(
        &self,
        ready: tokio::time::Instant,
        deadline: tokio::time::Instant,
    ) -> Check {
        let s = self.shared();
        let mut rx = s.subscribe();
        loop {
            let (save, (begun, running), ended_at, closed) = {
                let st = s.lock();
                let Some(save) = st.last_save else {
                    return Check::NoneBegan;
                };
                let since = st.checks.since(save.sent);
                (save, since, st.checks.ended_at, st.closed.is_some())
            };
            let now = tokio::time::Instant::now();
            if begun && !running {
                return Check::Ended(ended_at);
            }
            let grace = save.at.max(ready) + CHECK_GRACE;
            if !begun && now >= grace {
                return Check::NoneBegan;
            }
            if closed || now >= deadline {
                return Check::Unfinished;
            }
            let until = if begun { deadline } else { grace.min(deadline) };
            if let Ok(Err(_)) = tokio::time::timeout_at(until, rx.changed()).await {
                return Check::Unfinished;
            }
        }
    }

    /// `items` with the list pushed for `version`: added to a pulled answer,
    /// each item once, or in place of a pushed one, as a push replaces the
    /// last.
    fn with_pushed(
        &self,
        uri: &str,
        version: i32,
        mut items: Vec<Diagnostic>,
        pulled: bool,
    ) -> Vec<Diagnostic> {
        let st = self.shared().lock();
        let (Some(doc), Some(p)) = (st.docs.get(uri), st.pushed.get(uri)) else {
            return items;
        };
        if !p.current(doc, version) {
            return items;
        }
        if !pulled {
            return p.items.clone();
        }
        for d in &p.items {
            if !items.contains(d) {
                items.push(d.clone());
            }
        }
        items
    }

    /// Pull until an answer, retrying a server's "ask again" within the
    /// deadline.
    async fn pull(
        &self,
        uri: &str,
        deadline: tokio::time::Instant,
    ) -> Result<(Vec<Diagnostic>, Freshness), Error> {
        loop {
            let (prev, identifier) = {
                let st = self.shared().lock();
                (
                    st.pulled.get(uri).and_then(|p| p.result_id.clone()),
                    st.caps.diagnostic_identifier.clone(),
                )
            };
            let mut params = json!({ "textDocument": { "uri": uri } });
            if let Some(p) = prev {
                params["previousResultId"] = json!(p);
            }
            if let Some(i) = identifier {
                params["identifier"] = json!(i);
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let answer = self
                .request_within("textDocument/diagnostic", params, left)
                .await;
            match answer {
                Ok(v) => return Ok((self.took_pull(uri, v)?, Freshness::Pulled)),
                Err(Error::Rpc { code: c, .. })
                    if (c == code::SERVER_CANCELLED || c == code::CONTENT_MODIFIED)
                        && tokio::time::Instant::now() + Duration::from_millis(50) < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(Error::Timeout { .. }) => return Ok((self.last_known(uri), Freshness::Stale)),
                Err(e) => return Err(e),
            }
        }
    }

    fn took_pull(&self, uri: &str, v: Value) -> Result<Vec<Diagnostic>, Error> {
        let report: DocumentDiagnosticReport = serde_json::from_value(v)
            .map_err(|e| Error::Protocol(format!("an unreadable diagnostic report: {e}")))?;
        let mut st = self.shared().lock();
        let items = match report {
            DocumentDiagnosticReport::Full { result_id, items } => {
                st.pulled.insert(
                    uri.to_string(),
                    Pulled {
                        result_id,
                        items: items.clone(),
                    },
                );
                items
            }
            DocumentDiagnosticReport::Unchanged { result_id } => {
                let p = st.pulled.entry(uri.to_string()).or_insert(Pulled {
                    result_id: None,
                    items: Vec::new(),
                });
                p.result_id = Some(result_id);
                p.items.clone()
            }
        };
        Ok(items)
    }

    fn last_known(&self, uri: &str) -> Vec<Diagnostic> {
        let st = self.shared().lock();
        st.pulled
            .get(uri)
            .map(|p| p.items.clone())
            .or_else(|| st.pushed.get(uri).map(|p| p.items.clone()))
            .unwrap_or_default()
    }

    /// Wait for a push that is current for the document, until the
    /// deadline. `None`: the server registered the pull meanwhile.
    async fn wait_push(
        &self,
        uri: &str,
        deadline: tokio::time::Instant,
    ) -> Option<(Vec<Diagnostic>, Freshness)> {
        let s = self.shared();
        let mut rx = s.subscribe();
        loop {
            {
                let st = s.lock();
                let doc = st.docs.get(uri);
                if let (Some(d), Some(p)) = (doc, st.pushed.get(uri)) {
                    if p.current(d, d.version) {
                        return Some((p.items.clone(), Freshness::Pushed));
                    }
                }
                if st.caps.pull_diagnostics {
                    return None;
                }
                if st.closed.is_some() {
                    break;
                }
            }
            if tokio::time::timeout_at(deadline, rx.changed())
                .await
                .is_err()
            {
                break;
            }
        }
        Some((self.last_known(uri), Freshness::Stale))
    }
}
