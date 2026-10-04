//! The language-server board's facts (L2, theseus-n88g.8): what the board
//! (`crate::lsp`) records of each server it runs. Each is a row and nothing
//! else, as the index tender's are: a server lives outside every turn, so
//! it has no notification, sentence, or span of its own. Its hook writes each
//! row off the runtime's workers (`Core::build`'s `lsp_ledger`). A request a
//! tool makes is the call's `lsp.request` span (`LspRequest`), in its turn.

use std::path::Path;

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, Span};

use super::Fact;

/// A server started for a root: the first call for a file of its language
/// there, judged at `proc.run`'s posture for its argv.
pub struct LspStarted<'a> {
    pub server: &'a str,
    pub root: &'a Path,
    pub pid: Option<u32>,
    pub argv: &'a [String],
    pub log: &'a Path,
}

impl Fact for LspStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LspStarted);

    fn row(&self) -> Value {
        json!({"server": self.server, "root": self.root.display().to_string(), "pid": self.pid,
            "argv": self.argv, "log": self.log.display().to_string()})
    }
}

/// The server is ready: no work-done progress running, and, for one that
/// reports a status (rust-analyzer), quiescent.
pub struct LspReady<'a> {
    pub server: &'a str,
    pub root: &'a Path,
    pub pid: Option<u32>,
    pub ready_ms: u64,
}

impl Fact for LspReady<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LspReady);

    fn row(&self) -> Value {
        json!({"server": self.server, "root": self.root.display().to_string(), "pid": self.pid,
            "ready_ms": self.ready_ms})
    }
}

/// The board stopped a server: `idle`, `timeout` (a request unanswered),
/// or `daemon` (the daemon's stop, SIGTERM to its group, never waited for).
pub struct LspStopped<'a> {
    pub server: &'a str,
    pub root: &'a Path,
    pub pid: Option<u32>,
    pub why: &'a str,
    pub ran_ms: u64,
    pub requests: u64,
}

impl Fact for LspStopped<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LspStopped);

    fn row(&self) -> Value {
        json!({"server": self.server, "root": self.root.display().to_string(), "pid": self.pid,
            "why": self.why, "ran_ms": self.ran_ms, "requests": self.requests})
    }
}

/// A start that failed, or a server that ended unasked (a crash): the next
/// call starts it again.
pub struct LspFailed<'a> {
    pub server: &'a str,
    pub root: &'a Path,
    pub pid: Option<u32>,
    pub why: &'a str,
}

impl Fact for LspFailed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LspFailed);

    fn row(&self) -> Value {
        json!({"server": self.server, "root": self.root.display().to_string(), "pid": self.pid,
            "why": self.why})
    }
}

/// The `lsp.request` span of one request a tool call made, under the call's
/// span, with its server and method (`lsp.server`, `lsp.method`), placed on
/// the turn's clock by its caller. Telemetry's `theseus.lsp.request.duration`
/// reads these spans.
pub fn request_span(server: &str, method: &str, outcome: &str, start_us: u64, end_us: u64) -> Span {
    Span {
        name: "lsp.request".into(),
        kind: "lsp".into(),
        start_us,
        end_us: Some(end_us),
        attrs: json!({"lsp.server": server, "lsp.method": method, "outcome": outcome}),
        children: Vec::new(),
    }
}
