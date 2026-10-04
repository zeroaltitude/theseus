//! The memory pass's states (M6 step 31a): `memory.v1`'s, one node as memory
//! keeps it, and `attribution.v1`'s, a reply and the notes recall put in
//! front of the model, one `relied_on` Noul per note. Each holds only what
//! the session's own model saw: the node's text as stored (a tool result as
//! it was shown, scrubbed and capped), the operator's message, the reply,
//! and each note's excerpt as rendered.

use super::*;

/// The most notes `attribution.v1` asks about (its pack's `max`).
pub const NOTES: usize = 6;

/// `memory.v1`'s input: one node the memory pass labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryInput {
    /// `operator`, `assistant`, `tool`, or `relay` (a task's brief or report).
    pub role: String,
    /// A tool result's tool.
    #[serde(default)]
    pub tool: Option<String>,
    pub text: String,
    /// The message before it in the session, for what it may correct.
    #[serde(default)]
    pub previous: Option<String>,
}

/// One note recall admitted, as the model saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteInput {
    /// The recalled node's id: the Noul's key.
    pub id: String,
    pub excerpt: String,
}

/// `attribution.v1`'s input: a turn whose recall admitted notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributionInput {
    /// The operator's message the turn answered.
    pub ask: String,
    /// The assistant's reply.
    pub reply: String,
    #[serde(default)]
    pub notes: Vec<NoteInput>,
}

/// `memory.v1`: the node's text, who wrote it, its tool, and the message
/// before it.
pub fn memory(i: &MemoryInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("memory", MEMORY_VERSION, cap, scrub);
    b.scalar("role", c.clip(&i.role, 20));
    if let Some(t) = &i.tool {
        b.scalar("tool", c.clip(t, 60));
    }
    b.text("text", 9, share(cap, 60), Keep::Both, &i.text)
        .opt_text(
            "previous_message",
            5,
            share(cap, 30),
            Keep::Tail,
            i.previous.as_deref(),
        );
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

/// `attribution.v1`: the operator's message and the reply; each note, at
/// most [`NOTES`], a dynamic item (`notes`), its excerpt clipped and quoted.
pub fn attribution(i: &AttributionInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let mut b = StateBuilder::new("attribution", ATTRIBUTION_VERSION, cap, scrub);
    b.text("message", 7, share(cap, 25), Keep::Both, &i.ask)
        .text("reply", 9, share(cap, 45), Keep::Both, &i.reply)
        .scalar("notes_shown", i.notes.len());
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Notes,
        i.notes
            .iter()
            .take(NOTES)
            .map(|n| Item {
                key: n.id.clone(),
                text: format!("“{}”", clip_with(scrub, &n.excerpt, 300)),
            })
            .collect(),
    );
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}
