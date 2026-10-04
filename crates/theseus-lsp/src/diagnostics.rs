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

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::{Client, Error, Event, Shared};
use crate::jsonrpc::code;
use crate::types::{Diagnostic, DocumentDiagnosticReport, PublishDiagnosticsParams};
use crate::uri;

#[derive(Debug, Clone)]
pub(crate) struct Pushed {
    pub(crate) version: Option<i32>,
    pub(crate) items: Vec<Diagnostic>,
    /// The count of messages sent when it arrived.
    pub(crate) arrived_at: u64,
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

    /// This file's diagnostics for its current text, within `bound`: open
    /// it if it is not, sync it with the disk, then pull (when the server
    /// can) or wait for a push for this version. A server that reports a
    /// status (rust-analyzer) is waited on until it is quiescent first,
    /// within the same bound.
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
        let (version, pull) = {
            let st = self.shared().lock();
            let v = st.docs.get(&uri).map_or(0, |d| d.version);
            (v, st.caps.pull_diagnostics)
        };
        // A server may register the pull after `initialized` (ty does): a
        // push wait that sees the registration pulls instead.
        let pushed = if pull {
            None
        } else {
            self.wait_push(&uri, deadline).await
        };
        let (items, freshness) = match pushed {
            Some(got) => got,
            None => self.pull(&uri, deadline).await?,
        };
        Ok(FileDiagnostics {
            uri,
            version,
            items,
            freshness,
            waited: start.elapsed(),
        })
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
                    let current = match p.version {
                        Some(v) => v == d.version,
                        None => p.arrived_at >= d.sent_at,
                    };
                    if current {
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
