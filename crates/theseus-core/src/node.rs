//! Nodes (spec §4.1): a session's content as durable records. Record kind
//! `NODE`, keyed by node id, scoped to the session so a session's transcript is
//! one range scan. Append-only: a node is written once and never edited, which
//! is also what the provider's preserved-thinking rules require of a transcript.
//!
//! - `msg_` Message: what the operator said, or what the model returned (its
//!   content blocks stored verbatim, so thinking replays byte-for-byte).
//! - `tcl_` ToolCall: the harness's record of one tool invocation (the tool, its
//!   input, the gate's decision, the action's correlation id).
//! - `trs_` ToolResult: what went back to the model for one `tool_use`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::Usage;
use theseus_store::{kinds, NewRecord, Record};

pub const SCHEMA: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// A human through a protocol client.
    Operator,
    /// Model output.
    Agent,
    /// A tool's result.
    Tool,
    /// Something the harness itself said (a repair, a notice).
    Harness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Ok,
    /// The tool ran and failed, or its input was invalid.
    Error,
    /// The call never ran: the operator declined it, a new message superseded
    /// it, or its confirmation lapsed. Rows written before theseus-8az say
    /// `denied`.
    #[serde(alias = "denied")]
    Declined,
    /// Still running as a background job; the real result arrives later.
    Background,
    /// The outcome could not be established (the harness restarted mid-call).
    Unknown,
    Cancelled,
}

impl ResultStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ResultStatus::Ok => "ok",
            ResultStatus::Error => "error",
            ResultStatus::Declined => "declined",
            ResultStatus::Background => "background",
            ResultStatus::Unknown => "unknown",
            ResultStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    UserMessage {
        text: String,
    },
    AssistantMessage {
        /// Content blocks exactly as the provider returned them.
        blocks: Vec<Value>,
        model: String,
        provider: String,
        #[serde(default)]
        stop_reason: Option<String>,
        #[serde(default)]
        usage: Usage,
        #[serde(default)]
        cost_usd: Option<f64>,
        #[serde(default)]
        catalog_version: Option<String>,
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        correlation_id: Option<String>,
        /// The compilation and request digest this call was made from.
        #[serde(default)]
        compilation_id: Option<String>,
        #[serde(default)]
        request_digest: Option<String>,
    },
    ToolCall {
        tool_use_id: String,
        /// Canonical dotted name (`fs.read`).
        tool: String,
        /// The name on the wire (`fs_read`).
        wire_name: String,
        input: Value,
        assistant_node: String,
        #[serde(default)]
        correlation_id: Option<String>,
        /// The gate's decision and trace.
        #[serde(default)]
        gate: Value,
    },
    ToolResult {
        tool_use_id: String,
        tool: String,
        status: ResultStatus,
        is_error: bool,
        /// What the model sees: scrubbed of secrets, capped.
        content: String,
        #[serde(default)]
        correlation_id: Option<String>,
        #[serde(default)]
        bytes_total: u64,
        #[serde(default)]
        truncated: bool,
        /// Where the full output lives when `content` was capped.
        #[serde(default)]
        full_ref: Option<String>,
        #[serde(default)]
        duration_ms: Option<u64>,
        /// A background job's real result, arriving after its placeholder.
        #[serde(default)]
        late: bool,
        #[serde(default)]
        meta: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub schema: u16,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub loop_index: Option<u32>,
    pub origin: Origin,
    /// The client or principal that wrote it (`web#3`, `harness`).
    #[serde(default)]
    pub author: Option<String>,
    pub created_at_ms: u64,
    pub body: Body,
}

impl Node {
    fn new(
        prefix: &str,
        session_id: &str,
        turn_id: Option<&str>,
        origin: Origin,
        body: Body,
    ) -> Self {
        Self {
            id: crate::new_id(prefix),
            schema: SCHEMA,
            session_id: session_id.into(),
            turn_id: turn_id.map(str::to_string),
            loop_index: None,
            origin,
            author: None,
            created_at_ms: theseus_protocol::now_unix_ms(),
            body,
        }
    }

    pub fn user(session_id: &str, turn_id: Option<&str>, author: &str, text: &str) -> Self {
        let mut n = Self::new(
            "msg",
            session_id,
            turn_id,
            Origin::Operator,
            Body::UserMessage { text: text.into() },
        );
        n.author = Some(author.into());
        n
    }

    pub fn assistant(session_id: &str, turn_id: &str, loop_index: u32, body: Body) -> Self {
        let mut n = Self::new("msg", session_id, Some(turn_id), Origin::Agent, body);
        n.loop_index = Some(loop_index);
        n
    }

    pub fn tool_call(
        session_id: &str,
        turn_id: Option<&str>,
        loop_index: Option<u32>,
        body: Body,
    ) -> Self {
        let mut n = Self::new("tcl", session_id, turn_id, Origin::Harness, body);
        n.loop_index = loop_index;
        n
    }

    pub fn tool_result(
        session_id: &str,
        turn_id: Option<&str>,
        loop_index: Option<u32>,
        body: Body,
    ) -> Self {
        let mut n = Self::new("trs", session_id, turn_id, Origin::Tool, body);
        n.loop_index = loop_index;
        n
    }

    pub fn record(&self) -> Result<NewRecord> {
        Ok(NewRecord::json(kinds::NODE, Some(&self.id), self)?.scoped(&self.session_id))
    }

    pub fn from_record(r: &Record) -> Result<Self> {
        r.decode()
    }

    pub fn kind_str(&self) -> &'static str {
        match &self.body {
            Body::UserMessage { .. } => "user_message",
            Body::AssistantMessage { .. } => "assistant_message",
            Body::ToolCall { .. } => "tool_call",
            Body::ToolResult { .. } => "tool_result",
        }
    }

    /// A short human preview (Observatory, CLI).
    pub fn preview(&self, max: usize) -> String {
        let s = match &self.body {
            Body::UserMessage { text } => text.clone(),
            Body::AssistantMessage { blocks, .. } => {
                let t = crate::provider::text_of(blocks);
                let calls: Vec<String> = crate::provider::tool_uses_in(blocks)
                    .into_iter()
                    .map(|u| format!("→ {}", u.name))
                    .collect();
                if calls.is_empty() {
                    t
                } else if t.is_empty() {
                    calls.join(" ")
                } else {
                    format!("{t} {}", calls.join(" "))
                }
            }
            Body::ToolCall { tool, input, .. } => format!("{tool} {input}"),
            Body::ToolResult {
                tool,
                status,
                content,
                ..
            } => format!("{tool} [{}] {content}", status.as_str()),
        };
        let s = s.replace('\n', " ");
        if s.chars().count() > max {
            format!("{}…", s.chars().take(max).collect::<String>())
        } else {
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_roundtrip_through_records_with_session_scope() {
        let n = Node::user("ses_1", Some("turn_1"), "web#1", "hello");
        let r = n.record().unwrap();
        assert_eq!(r.kind, kinds::NODE);
        assert_eq!(r.scope.as_deref(), Some("ses_1"));
        assert!(n.id.starts_with("msg_"));
        let back: Node = serde_json::from_slice(&r.payload).unwrap();
        assert_eq!(back, n);
        assert_eq!(back.kind_str(), "user_message");
        let a = Node::assistant(
            "ses_1",
            "turn_1",
            0,
            Body::AssistantMessage {
                blocks: vec![
                    serde_json::json!({"type": "thinking", "thinking": "", "signature": "sig"}),
                    serde_json::json!({"type": "text", "text": "Reading it."}),
                    serde_json::json!({"type": "tool_use", "id": "toolu_1", "name": "fs_read", "input": {"path": "/x"}}),
                ],
                model: "m".into(),
                provider: "p".into(),
                stop_reason: Some("tool_use".into()),
                usage: Usage::default(),
                cost_usd: None,
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
            },
        );
        assert_eq!(a.preview(80), "Reading it. → fs_read");
    }
}
