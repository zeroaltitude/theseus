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
//! - `arr_` Arrangement: a task's quoted pieces (M5 27), after its brief.
//! - `syn_` Synthesis: consolidation's cited entry on a cluster of nodes
//!   recall admits together (M6 31b), in the memory's harness session.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::Usage;
use theseus_store::{kinds, NewRecord};

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
    /// A message an MCP server's prompt gave (M7 36c): the operator chose
    /// the prompt, the server wrote its words.
    Mcp,
    /// A message of the operator's past history, brought in by `import`
    /// (theseus-0lrr.6): its body says from where (`Body::Imported`'s
    /// source, unit and digest) and whose words they were (its integrity).
    Import,
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

/// A file that came with an operator's message (theseus-9g2), as its node
/// keeps it. The node is written once, so this is what every later render
/// of the message reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    /// `text/plain`, `image/png`, …; empty when the sender did not say.
    #[serde(default)]
    pub media_type: String,
    /// Bytes of the whole file, as the sender reported it.
    #[serde(default)]
    pub size: u64,
    pub content: AttachmentContent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentContent {
    /// Its text, capped at `[tools].max_read_bytes` on a character
    /// boundary; `cut` when the file was longer.
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        cut: bool,
    },
    /// An image, stored once in the store's blobs by the SHA-256 of its
    /// bytes (`blobs.rs`); the node holds the reference, never the bytes.
    /// `media_type` is the type its bytes say, and `size` its bytes.
    Image {
        digest: String,
        width: u32,
        height: u32,
    },
    /// Not read, and why (too large, a type that is not read, a failed download).
    NotRead { reason: String },
    /// A file kept whole in the store's blobs by the SHA-256 of its bytes
    /// (theseus-c9l6): a PDF, which a model that reads PDFs reads as a
    /// document and any other model as its text. What was made of it when it
    /// arrived is in the blobs too, by digest, so every later render reads
    /// the same bytes. `media_type` is the type its bytes say, and `size`
    /// its bytes.
    File {
        digest: String,
        /// A PDF's pages.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pages: Option<u32>,
        /// The blob of its text, one string per page (JSON), when it was read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// Its first pages as PDFs of their own, fewest pages first, for a
        /// request that cannot carry the whole file (`attach::PAGE_LIMITS`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<FilePart>,
        /// Why its text was not read when it arrived.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unread: Option<String>,
    },
}

/// A file's first pages as a PDF of their own (theseus-c9l6), in the blobs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilePart {
    /// It holds pages 1 to `last`.
    pub last: u32,
    pub digest: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    UserMessage {
        text: String,
        /// Files that came with it, in order (theseus-9g2). Absent on a
        /// message without any, so stored nodes keep their bytes.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<Attachment>,
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
        /// The gate's record: its input's validation, the policy's verdict,
        /// the plan, and the proposal a confirm binds. Written with its keys
        /// sorted, as every record before the type was (theseus-0g4).
        #[serde(default, serialize_with = "theseus_protocol::canonical")]
        gate: Option<Box<theseus_protocol::GateRecord>>,
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
        /// An image the tool returned (`fs.read` of a PNG), stored once in
        /// the blobs like an attached one (theseus-9g2); or a PDF's pages
        /// (`fs.read` with `pages`, `http.fetch` of a PDF), a `File`
        /// (theseus-c9l6). The name stays `image`, so older nodes read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<Attachment>,
        /// Its text came from outside, from this URL (`http.fetch`,
        /// `web.search`; DD5): external text, which never becomes durable
        /// without the operator's confirmation (§5.2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        external: Option<theseus_tools::External>,
    },
    /// What recall put in front of the model (M6 step 30b, §2.8): references
    /// to its sources, never copies. It renders after the turn's new message,
    /// in the same user turn, as testimony: each item's frozen header, then
    /// its source's text over the frozen range, read by position
    /// (`recall::render`), so every later request repeats the same bytes.
    /// (`Lesson` comes with 35b.)
    Recall {
        recall_id: String,
        /// The session's arm (`baseline`).
        arm: String,
        items: Vec<RecalledRef>,
    },
    /// A task's arrangement (M5 27, theseus-vug.2): the messages of its
    /// parent's session that `task.create` quoted, written after the brief
    /// in the frame that opens the task. It carries what the child's
    /// compilation renders (`arrangement::render`), so the compiler reads
    /// only the child's own nodes.
    Arrangement {
        pieces: Vec<crate::arrangement::Piece>,
        /// The call acknowledged the fidelity check.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        fidelity_ack: bool,
        /// A check task's claim (M5 28a, `check.rs`): the checked task's
        /// report, rendered after the pieces. Absent in nodes written before
        /// it (format 14), and on every other task's arrangement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        claim: Option<crate::check::Claim>,
    },
    /// A compaction's summary (M6 step 30c, §2.5): what a cheap profile
    /// wrote of a run of the session's leading nodes that the ring would
    /// have dropped. Its range is its lineage (no edge per node). A
    /// compilation that includes it renders it first in its prefix, whatever
    /// its position, as testimony under its frozen `header`; the nodes of
    /// its range never render again. A later compaction folds it into the
    /// next summary, whose range starts where its did.
    Summary {
        /// The WAL positions of the first and last node it summarizes.
        first: u64,
        last: u64,
        /// How many messages it summarizes (a folded summary's included).
        nodes: u32,
        text: String,
        /// The profile and the model that wrote it.
        profile: String,
        model: String,
        #[serde(default)]
        cost_usd: Option<f64>,
        /// `[Summary of 212 earlier messages, 2026-09-20 to 2026-09-27
        /// (@120 to @4810), written by glm on glm-5.3-flash]`, frozen when it
        /// was written (§2.11's testimony; before 35a, without the positions
        /// and the model).
        header: String,
    },
    /// Consolidation's synthesis (M6 step 31b, design §2.7): one short entry,
    /// every sentence citing, that a profile wrote of a cluster of nodes
    /// recall admits together. In the books (spec P8) it is an encyclopedia
    /// entry, by topic: never an SOP (those are the operator's alone), nor
    /// a recipe (promoted after repeated success). Its origin is `agent`; it
    /// lives in the memory's harness session (`consolidate::session`), which
    /// is never compiled, with a `derived_from` edge to each source (`via =
    /// "synthesis"`). Never shown in shadow: only the `+synthesis` arm puts
    /// a checked one before a model.
    Synthesis {
        /// The entry, each sentence ending with its citations, `[1]`,
        /// numbered as `sources`.
        text: String,
        /// The nodes it cites, by id: `[1]` is the first.
        sources: Vec<String>,
        /// The citation check's verdict (`citation.v1`).
        check: crate::consolidate::CitationCheck,
        stage: crate::consolidate::Stage,
        /// The digest of its sources' ids, sorted: a cluster synthesized
        /// before is not proposed again.
        cluster: String,
        /// The profile and the model that wrote it, and what it cost.
        profile: String,
        model: String,
        #[serde(default)]
        cost_usd: Option<f64>,
    },
    /// One message of an imported episode (theseus-0lrr.6), in its imported
    /// session, which never takes a turn. Its node's origin is `import`, its
    /// author the episode's (`zeroaltitude`, `agent:main`, `person:<name>`,
    /// `tool`, `outside`), and its `created_at_ms` the message's own time.
    Imported {
        text: String,
        /// Whose words they were: the operator's, an agent's, or outside
        /// text, which the index marks external, so recall keeps it out
        /// unless the config admits external text, and frames it as such.
        integrity: crate::import::Integrity,
        /// The episode's source (`openclaw-store`, `wiki`).
        source: String,
        /// The source's own id of the message, and its text's sha256.
        unit: String,
        sha256: String,
        /// Its place in the episode, from 0.
        idx: u32,
    },
    /// An imported episode's summary (theseus-0lrr.6), citing its messages
    /// by node id: `cites[0]` is the message the pipeline numbered first.
    ImportedSummary {
        text: String,
        cites: Vec<String>,
        /// The model that wrote it.
        model: String,
    },
    /// A tombstone (§5.6's erasure marker; theseus-0lrr.6): the node's
    /// payload, gone. Written under the node's own id, origin and time,
    /// after the record it erases, so the newest record of the id is this
    /// one; the index's follower drops a node it meets as a tombstone
    /// (nothing to index), and a rebuild does the same.
    Erased {
        /// The kind of body it replaced (`imported`).
        was: String,
        at_ms: u64,
        /// The receipt: what ordered it (`import.erase of openclaw-2026-10`).
        why: String,
    },
}

/// One source a `Recall` node renders: where it is, the byte range of its
/// text shown, and the header frozen when it was recalled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecalledRef {
    pub node_id: String,
    pub session_id: String,
    /// Its WAL position: the render reads it there.
    pub position: u64,
    /// The bytes of its text (`recall::text_of`) shown, `[start, end)`.
    pub chunk: (u32, u32),
    /// `a reply by glm-5.3-flash in #harbor, 2026-09-30 14:34 UTC (as of
    /// @18231)`, frozen when recalled (before 35a, `in <session>`).
    pub header: String,
    /// The pack's estimate of its tokens: the session's cap counts them.
    pub tokens: u64,
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
    // A node from 19a to the place rule carries a `label` (NODE schemas 5
    // and 6): it reads, and is left unread (NODE schema 7, theseus-nbsh).
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
        Self::user_with(session_id, turn_id, author, text, Vec::new())
    }

    /// An operator's message with the files that came with it (theseus-9g2).
    pub fn user_with(
        session_id: &str,
        turn_id: Option<&str>,
        author: &str,
        text: &str,
        attachments: Vec<Attachment>,
    ) -> Self {
        let mut n = Self::new(
            "msg",
            session_id,
            turn_id,
            Origin::Operator,
            Body::UserMessage {
                text: text.into(),
                attachments,
            },
        );
        n.author = Some(author.into());
        n
    }

    /// One message of an MCP server's prompt (M7 36c), user-role: the words
    /// are the server's, so its origin is `mcp` and its author names the
    /// prompt, `prompt:<server>/<name>`.
    pub fn prompt_message(
        session_id: &str,
        turn_id: Option<&str>,
        author: &str,
        text: &str,
        attachments: Vec<Attachment>,
    ) -> Self {
        let mut n = Self::user_with(session_id, turn_id, author, text, attachments);
        n.origin = Origin::Mcp;
        n
    }

    /// A message the model reads as the user's that no operator typed (DD7):
    /// a task's brief, written by its parent's model (`Agent`), or a task's
    /// report in its parent's session, written by the harness (`Harness`).
    pub fn relayed(
        session_id: &str,
        turn_id: Option<&str>,
        origin: Origin,
        author: &str,
        text: &str,
    ) -> Self {
        let mut n = Self::new(
            "msg",
            session_id,
            turn_id,
            origin,
            Body::UserMessage {
                text: text.into(),
                attachments: Vec::new(),
            },
        );
        n.author = Some(author.into());
        n
    }

    /// A task's arrangement, by its parent's model (M5 27).
    pub fn arrangement(
        session_id: &str,
        author: &str,
        pieces: Vec<crate::arrangement::Piece>,
        fidelity_ack: bool,
    ) -> Self {
        let mut n = Self::new(
            "arr",
            session_id,
            None,
            Origin::Agent,
            Body::Arrangement {
                pieces,
                fidelity_ack,
                claim: None,
            },
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

    /// The notes recall put in front of the model (M6 30b), written by the
    /// harness in the turn's provider call's plan frame.
    pub fn recall(
        session_id: &str,
        turn_id: &str,
        recall_id: &str,
        arm: &str,
        items: Vec<RecalledRef>,
    ) -> Self {
        let body = Body::Recall {
            recall_id: recall_id.into(),
            arm: arm.into(),
            items,
        };
        let mut n = Self::new("rcn", session_id, Some(turn_id), Origin::Harness, body);
        n.loop_index = Some(0);
        n.author = Some("harness".into());
        n
    }

    /// A compaction's summary (30c), written by the harness.
    pub fn summary(session_id: &str, turn_id: &str, body: Body) -> Self {
        let mut n = Self::new("sum", session_id, Some(turn_id), Origin::Harness, body);
        n.author = Some("harness".into());
        n
    }

    /// Consolidation's synthesis (31b), in the memory's harness session:
    /// the agent's words, written by the harness's run.
    pub fn synthesis(session_id: &str, body: Body) -> Self {
        let mut n = Self::new("syn", session_id, None, Origin::Agent, body);
        n.author = Some("consolidation".into());
        n
    }

    /// A message of an imported episode (theseus-0lrr.6): origin `import`,
    /// its id given (the import's ids are its episode's, so a retry writes
    /// the same ones), its time the message's own.
    pub fn imported(id: String, session_id: &str, author: &str, at_ms: u64, body: Body) -> Self {
        Self {
            id,
            schema: SCHEMA,
            session_id: session_id.into(),
            turn_id: None,
            loop_index: None,
            origin: Origin::Import,
            author: Some(author.into()),
            created_at_ms: at_ms,
            body,
        }
    }

    /// This node's tombstone (§5.6): the same id, session, origin, author
    /// and time, its body an erasure marker.
    pub fn erased(&self, at_ms: u64, why: &str) -> Self {
        Self {
            body: Body::Erased {
                was: self.kind_str().to_string(),
                at_ms,
                why: why.to_string(),
            },
            ..self.clone()
        }
    }

    pub fn record(&self) -> Result<NewRecord> {
        Ok(NewRecord::json(kinds::NODE, Some(&self.id), self)?.scoped(&self.session_id))
    }

    pub fn kind_str(&self) -> &'static str {
        match &self.body {
            Body::UserMessage { .. } => "user_message",
            Body::AssistantMessage { .. } => "assistant_message",
            Body::ToolCall { .. } => "tool_call",
            Body::ToolResult { .. } => "tool_result",
            Body::Recall { .. } => "recall",
            Body::Arrangement { .. } => "arrangement",
            Body::Summary { .. } => "summary",
            Body::Synthesis { .. } => "synthesis",
            Body::Imported { .. } => "imported",
            Body::ImportedSummary { .. } => "imported_summary",
            Body::Erased { .. } => "erased",
        }
    }

    /// A short human preview (Observatory, CLI).
    pub fn preview(&self, max: usize) -> String {
        let s = match &self.body {
            Body::UserMessage { text, attachments } => {
                crate::attach::display_text(text, attachments, self.author.as_deref())
            }
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
            Body::Recall { items, .. } => format!(
                "recalled {}",
                crate::narrative::count(items.len() as u64, "note", "notes")
            ),
            Body::Arrangement { pieces, .. } => format!(
                "{}: {}",
                crate::arrangement::clip(pieces.len()),
                pieces
                    .iter()
                    .map(|p| format!("{} \"{}\"", p.role.as_str(), p.first_line))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Body::Summary { header, text, .. } => format!("{header} {text}"),
            Body::Synthesis { text, check, .. } => {
                format!("[synthesis, {}] {text}", check.as_str())
            }
            Body::Imported {
                text, integrity, ..
            } => format!("[imported, {}] {text}", integrity.as_str()),
            Body::ImportedSummary { text, .. } => format!("[imported summary] {text}"),
            Body::Erased { was, why, .. } => format!("[erased {was}: {why}]"),
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
