//! One turn to run (`TurnRequest`), moved out of turn.rs, which is at its
//! line ceiling.

use super::{EventSink, Recompile, SessionRecord, Target};

/// One turn to run.
pub struct TurnRequest {
    pub session: SessionRecord,
    /// `None`: a continuation.
    pub input: Option<String>,
    pub target: Target,
    pub sink: EventSink,
    /// The client that asked (`web#3`) or `harness` for a continuation.
    pub author: String,
    pub recompile: Option<Recompile>,
    /// Files that came with the input (theseus-9g2); kept on its node.
    pub attachments: Vec<theseus_protocol::Attachment>,
    /// When the request reached the daemon; `None`: now.
    pub arrived: Option<std::time::Instant>,
    /// The surface's message it answers (a Discord message id), which the
    /// reply's post names (theseus-q4v).
    pub reply_to: Option<String>,
    /// An MCP server's prompt as the input (M7 36c): its messages are the
    /// input's nodes, and `input` is their text. `None` for any other turn.
    pub prompt: Option<crate::mcp::prompts::PromptInput>,
    /// Where the reply is posted when it is not the session's own place (a
    /// message in the owner's DM answered there, in a thread's turn;
    /// theseus-sjzm).
    pub answer_at: Option<String>,
    /// The surface the input came through (`standing::surface_name`), which
    /// the records scoped to a surface read (theseus-slvl.4); none for a
    /// continuation and for a caller that names none.
    pub surface: Option<&'static str>,
}
