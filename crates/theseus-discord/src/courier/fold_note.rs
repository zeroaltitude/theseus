//! An FYI note that stays in a conversation, folded (issue 56): a post the
//! notices place's rule says only informs (`notices::informs`: its kind is on
//! `INFORMING` and its head says FYI), written to a place that folds its steps
//! (`steps = "folded"`, the default), with no button of its own, is one closed
//! line, `-# ℹ️ FYI: Theseus restarted`, said from its kind and never from its
//! words, with **Show note**. A note the notices place takes is not folded
//! there; a note with a button (an Undo, a watch's answers), a met watch, and
//! every post that is not an FYI of an informing kind are written as ever.
//!
//! A press carries the post's correlation id (`runtime/fold.rs`), and the
//! words are read back from the post itself (`kernel.action`), by the same
//! functions the lane wrote them with, only when the pressed message is the
//! one that post was written to: a button forged elsewhere shows nothing.

use serde_json::Value;
use theseus_core::outbox::{body_of, kind_of as post_kind};
use theseus_kernel::Action;

use super::{notices, Lane, Write};
use crate::bindings::Steps;
use crate::render::Buttons;

/// What a folded note says of itself, by its post's kind.
fn what(kind: &str) -> &'static str {
    match kind {
        "onto_card" => "something new is kept",
        "jev_paused" => "Jev's warnings are paused",
        "restarted" => "Theseus restarted",
        "disk" => "the disk",
        "mcp_changed" => "an MCP server's tools changed",
        "mcp_prompt_changed" => "an MCP prompt changed",
        "self_join" => "self-improvement",
        _ => "a note",
    }
}

/// A folded note's closed line.
pub(crate) fn line(kind: &str) -> String {
    format!("-# ℹ️ FYI: {}", what(kind))
}

/// The words post `a` was written with, when it is an FYI of an informing
/// kind: its text, or what the lane makes of its body.
pub(crate) fn words(a: &Action) -> Option<String> {
    let (kind, body) = (post_kind(a), body_of(a));
    let text = match kind {
        "restarted" => {
            let tables: Vec<String> = body["tables"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect();
            crate::render::restarted(body["at_unix_ms"].as_u64().unwrap_or(0), &tables)
        }
        "disk" => crate::diskwords::disk_note(body),
        "mcp_changed" | "mcp_prompt_changed" => super::mcp_note(body),
        _ => body["text"].as_str()?.to_string(),
    };
    notices::informs(kind, &text).then_some(text)
}

/// Whether post `a`'s body is a met watch, which speaks as a reminder.
fn met(body: &Value) -> bool {
    body["purpose"].as_str() == Some("watch.met")
}

impl Lane {
    /// Post `a`'s writes, each FYI note that stays in a folded place as its
    /// closed line with Show note. Its ping is as it was.
    pub(super) fn fold_notes(&self, a: &Action, writes: &mut [Write]) {
        let kind = post_kind(a);
        if met(body_of(a)) {
            return;
        }
        for w in writes.iter_mut() {
            if w.message.is_some()
                || w.buttons != Buttons::Keep
                || !notices::informs(kind, &w.content)
                || self.shared.steps_in(w.channel) != Steps::Folded
            {
                continue;
            }
            w.content = line(kind);
            w.buttons = Buttons::FoldNote(a.correlation_id.clone(), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use theseus_core::owner_note::{note, Kind};

    use super::*;

    /// The closed line says the kind's few words and nothing of the note.
    #[test]
    fn a_folded_note_says_its_kind_alone() {
        assert_eq!(line("restarted"), "-# ℹ️ FYI: Theseus restarted");
        assert_eq!(line("notice"), "-# ℹ️ FYI: a note");
        for k in notices::INFORMING {
            let l = line(k);
            assert!(l.starts_with("-# ℹ️ FYI: ") && !l.contains('\n'), "{l}");
        }
        let fyi = note(Kind::Fyi, "Personas moved", "a start", "secret words");
        assert!(notices::informs("notice", &fyi));
        assert!(!line("notice").contains("secret"));
    }
}
