//! The MCP board's facts (M7 §2.1, step 36b): each server's start, ready,
//! exit, failure, and changed tools. Each is a ledger row and narrative
//! lines, recorded by the board outside every turn; a call rides on its tool
//! call's own rows. The fields are owned, since the board records them off
//! the runtime's workers.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Tool;

use super::{Fact, Say};
use crate::narrative::count;

/// A server's start: its process spawned, or its HTTP connection begun.
pub struct McpStarted {
    pub server: String,
    pub transport: &'static str,
    pub pid: Option<u32>,
    /// Starts since the daemon's: 1 for the first.
    pub attempt: u64,
}

impl Fact for McpStarted {
    const KIND: Option<LedgerKind> = Some(LedgerKind::McpStarted);

    fn row(&self) -> Value {
        json!({"server": self.server, "transport": self.transport, "pid": self.pid,
            "attempt": self.attempt})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let pid = self.pid.map(|p| format!(", pid {p}")).unwrap_or_default();
        say.line(
            Tool,
            format!(
                "MCP server {} starting ({}{pid}).",
                self.server, self.transport
            ),
        );
    }
}

/// A server answered its handshake and listed its tools.
pub struct McpReady {
    pub server: String,
    pub ms: u64,
    pub tools: usize,
    pub prompts: usize,
    pub protocol: String,
    pub digest: String,
}

impl Fact for McpReady {
    const KIND: Option<LedgerKind> = Some(LedgerKind::McpReady);

    fn row(&self) -> Value {
        json!({"server": self.server, "ms": self.ms, "tools": self.tools,
            "prompts": self.prompts, "protocol": self.protocol, "digest": self.digest})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "MCP server {} ready in {} ms: {}, {}.",
                self.server,
                self.ms,
                count(self.tools as u64, "tool", "tools"),
                count(self.prompts as u64, "prompt", "prompts")
            ),
        );
    }
}

/// A server's connection ended, or its start failed; the next start is
/// after `backoff_ms`.
pub struct McpExited {
    pub server: String,
    pub why: String,
    pub crashes: u64,
    pub backoff_ms: u64,
}

impl Fact for McpExited {
    const KIND: Option<LedgerKind> = Some(LedgerKind::McpExited);

    fn row(&self) -> Value {
        json!({"server": self.server, "why": self.why, "crashes": self.crashes,
            "backoff_ms": self.backoff_ms})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "MCP server {} ended ({}); it restarts in {} ms.",
                self.server, self.why, self.backoff_ms
            ),
        );
    }
}

/// A server crashed too often: it stays down until `theseus mcp restart`.
pub struct McpFailed {
    pub server: String,
    pub why: String,
    pub crashes: u64,
}

impl Fact for McpFailed {
    const KIND: Option<LedgerKind> = Some(LedgerKind::McpFailed);

    fn row(&self) -> Value {
        json!({"server": self.server, "why": self.why, "crashes": self.crashes})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "MCP server {} failed: {} within 10 minutes, the last {}. It stays down until \
                 `theseus mcp restart {}`.",
                self.server,
                count(self.crashes, "crash", "crashes"),
                self.why,
                self.server
            ),
        );
    }
}

/// A server's tools changed since the list Theseus last offered (the "rug
/// pull" case): a name, a schema, or a description. The new list applies
/// from the next turn.
pub struct McpToolsChanged {
    pub server: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    pub digest: String,
}

impl McpToolsChanged {
    /// "added x; removed y; changed z".
    pub fn summary(&self) -> String {
        [
            ("added", &self.added),
            ("removed", &self.removed),
            ("changed", &self.changed),
        ]
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(w, v)| format!("{w} {}", v.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
    }
}

impl Fact for McpToolsChanged {
    const KIND: Option<LedgerKind> = Some(LedgerKind::McpToolsChanged);

    fn row(&self) -> Value {
        json!({"server": self.server, "added": self.added, "removed": self.removed,
            "changed": self.changed, "digest": self.digest})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "MCP server {}'s tools changed ({}): the new list is offered from the next turn.",
                self.server,
                self.summary()
            ),
        );
    }
}

/// A prompt whose definition changed since its last use (36c): a
/// description or an argument. The use goes ahead; the operator is told.
pub struct McpPromptChanged {
    pub server: String,
    pub prompt: String,
    pub before: String,
    pub after: String,
    pub summary: String,
}

impl Fact for McpPromptChanged {
    const KIND: Option<LedgerKind> = Some(LedgerKind::McpPromptChanged);

    fn row(&self) -> Value {
        json!({"server": self.server, "prompt": self.prompt, "before": self.before,
            "after": self.after, "summary": self.summary})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "MCP server {}'s prompt {} changed since its last use ({}); this use goes ahead.",
                self.server, self.prompt, self.summary
            ),
        );
    }
}
