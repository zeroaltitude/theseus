//! A thread's card at the head of its messages (theseus-h4a7, thread-context).
//!
//! In a thread's session the card (title, goal, working set and the lines of
//! where it stands) is the first block of the first message, so the layers
//! before it stay the same bytes in every thread (`layers`). It is a
//! **snapshot**: taken when the compilation is made, kept with it (a META
//! record, `thread.card.<compilation id>`, written in the compilation's own
//! frame), and rendered unchanged by every request that appends to that
//! compilation. `thread.note` and an edit of the thread change the card the
//! next recompile takes, never the prefix a request replays in between.

use serde_json::{json, Value};
use theseus_protocol::threads::ThreadInfo;

use super::{Messages, RequestSpec};

/// The card a spec renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// The card as the thread stands now: what a recompile takes.
    pub now: String,
    /// The card the session's current compilation took, when a record of it
    /// is held: what an append renders.
    pub recorded: Option<String>,
    /// What this spec renders (`apply` sets it).
    pub shown: String,
}

impl Card {
    pub fn new(now: String, recorded: Option<String>) -> Self {
        Card {
            shown: recorded.clone().unwrap_or_else(|| now.clone()),
            now,
            recorded,
        }
    }
}

/// The spec an append renders and the one a recompile renders, with their
/// cards: an append the recorded snapshot, a recompile the thread as it
/// stands. Unchanged when the spec has no card.
pub(super) fn split<'a>(
    appended: std::borrow::Cow<'a, RequestSpec>,
    recompiled: std::borrow::Cow<'a, RequestSpec>,
) -> (
    std::borrow::Cow<'a, RequestSpec>,
    std::borrow::Cow<'a, RequestSpec>,
) {
    use std::borrow::Cow;
    let Some(card) = appended.card.clone() else {
        return (appended, recompiled);
    };
    let mut rec = recompiled.into_owned();
    if let Some(c) = rec.card.as_mut() {
        c.shown = c.now.clone();
    }
    let mut app = appended.into_owned();
    if let Some(c) = app.card.as_mut() {
        c.shown = card.recorded.clone().unwrap_or_else(|| card.now.clone());
    }
    (Cow::Owned(app), Cow::Owned(rec))
}

/// The card's text: one block, the same bytes for the same thread.
pub fn text(t: &ThreadInfo) -> String {
    let mut out = format!("[Thread card: {}", t.title);
    if !t.goal.is_empty() {
        out.push_str(&format!("\nGoal: {}", t.goal));
    }
    let w = &t.working_set;
    for (label, list) in [
        ("roots", &w.roots),
        ("repos", &w.repos),
        ("branches", &w.branches),
        ("refs", &w.refs),
        ("files", &w.files),
        ("hosts", &w.hosts),
    ] {
        if !list.is_empty() {
            out.push_str(&format!("\nWorking set, {label}: {}", list.join(", ")));
        }
    }
    if !t.card.is_empty() {
        out.push_str("\nWhere it stands:");
        for line in &t.card {
            out.push_str(&format!("\n- {line}"));
        }
    }
    out.push_str(
        "\nThis card is as of the last time the conversation was rewritten; a line you add \
         with thread.note shows here after the next rewrite, and in the meantime in your own \
         call.]",
    );
    out
}

/// Put the card first in the messages: a block at the head of the first
/// user message, else a message of its own before it.
pub(super) fn attach(spec: &RequestSpec, m: &mut Messages) {
    let Some(card) = spec.card.as_ref() else {
        return;
    };
    let block = json!({"type": "text", "text": card.shown});
    match m.messages.first_mut() {
        Some(first) if first["role"] == "user" => {
            if let Some(Value::Array(blocks)) = first.get_mut("content") {
                blocks.insert(0, block);
                return;
            }
        }
        _ => {}
    }
    m.messages
        .insert(0, json!({"role": "user", "content": [block]}));
    m.tail_from += 1;
    for e in &mut m.efforts {
        e.0 += 1;
    }
}
