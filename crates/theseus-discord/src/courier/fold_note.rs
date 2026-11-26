//! An FYI note folded where it lands (issue 56): a post the notices place's
//! rule says only informs (`notices::informs`: its kind is on `INFORMING` and
//! its head says FYI), with no button of its own and no met watch, written
//! to a conversation that folds its steps (`steps = "folded"`, the default),
//! is one closed line, `-# ℹ️ FYI: Theseus restarted`, said from its kind and
//! never from its words, with **Show note**. The fold is decided on the
//! write's actual channel, the DM a refused note falls back to included
//! (`notices.rs`): the notices place never folds, whatever its word and
//! whatever `announce` says. A note with a button and every other post are
//! written as ever.
//!
//! A folded write's `(channel, message)` is kept with its post as an
//! additive key of the post's detail, `note_fold`. A press names the post
//! (`runtime/fold.rs`) and is honoured only when that stored pair is the
//! pressed channel and message: a note written open or with a button has no
//! such pair, so no press can fold it or take its buttons away. The words
//! are read back from the post itself, by the same functions the lane wrote
//! them with, and masked at the exit as every write is.

use serde_json::{json, Value};
use theseus_core::outbox::{body_of, kind_of as post_kind};
use theseus_kernel::{Action, ActionState};

use super::{notices, Lane, SendErr, Write};
use crate::bindings::Steps;
use crate::render::Buttons;

/// The post's detail key that says where it was written folded.
const MARK: &str = "note_fold";

/// What a folded note says of itself, by its post's kind.
fn what(kind: &str) -> &'static str {
    match kind {
        "onto_card" => "something new is kept",
        "jev_paused" => "Jev's warnings are paused",
        "restarted" => "Theseus restarted",
        "disk" => "the disk",
        "mcp_changed" => "an MCP server's tools changed",
        "mcp_prompt_changed" => "an MCP prompt changed",
        _ => "a note",
    }
}

/// A folded note's closed line.
pub(crate) fn line(kind: &str) -> String {
    format!("-# ℹ️ FYI: {}", what(kind))
}

/// The words post `a` was written with, when it is an FYI of an informing
/// kind: its text, or what the lane makes of its body.
fn words(a: &Action) -> Option<String> {
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

/// Whether post `a` was written folded at exactly `at`, channel and message.
fn folded_at(a: &Action, at: (u64, u64)) -> bool {
    let d = a.detail.as_ref().map_or(&Value::Null, |d| &d[MARK]);
    a.state == ActionState::Succeeded
        && d["channel"].as_str() == Some(at.0.to_string().as_str())
        && d["id"].as_str() == Some(at.1.to_string().as_str())
}

/// What a press on a folded note asks for: its post, open or closed, where.
pub(crate) struct NoteAsk {
    pub post: String,
    pub open: bool,
    pub at: (u64, u64),
}

impl Lane {
    /// Whether write `w` of post `a`, about to go to `w.channel`, is folded
    /// there: an FYI of an informing kind with no button and no message of
    /// its own to edit, not a met watch, in a place that folds its steps and
    /// is not the notices place.
    fn folds(&self, a: &Action, w: &Write) -> bool {
        let notices = self.notices_place((0, String::new())).0;
        w.message.is_none()
            && w.buttons == Buttons::Keep
            && body_of(a)["purpose"].as_str() != Some("watch.met")
            && notices::informs(post_kind(a), &w.content)
            && (notices == 0 || w.channel != notices)
            && self.shared.steps_in(w.channel) == Steps::Folded
    }

    /// Write `w` of post `a`, folded when it [`folds`](Self::folds) where it
    /// lands; where a folded write landed is kept for the post's detail. A
    /// copy's mark (`RESENT`) stays on the line.
    pub(super) async fn note_write(
        &mut self,
        a: &Action,
        w: &Write,
    ) -> Result<Option<(u64, u64)>, SendErr> {
        if !self.folds(a, w) {
            return self.write(w).await;
        }
        let mut f = w.clone();
        f.content = line(post_kind(a));
        if w.content.ends_with(crate::render::RESENT) {
            f.content = format!("{}\n{}", f.content, crate::render::RESENT);
        }
        f.buttons = Buttons::FoldNote(a.correlation_id.clone(), false);
        let wrote = self.write(&f).await;
        if let Ok(Some(at)) = wrote {
            self.note_folded = Some((a.correlation_id.clone(), at));
        }
        wrote
    }

    /// Post `a`'s detail gains where it was written folded, if it was.
    pub(super) fn mark_folded(&mut self, a: &Action, detail: &mut Value) {
        match self.note_folded.take() {
            Some((post, (c, m))) if post == a.correlation_id => {
                detail[MARK] = json!({"channel": c.to_string(), "id": m.to_string()});
            }
            _ => {}
        }
    }

    /// A folded note pressed: its words, or its line again, on the message
    /// its post was written to folded, and on no other; `take` claims the
    /// press just before the edit (false: its presser stopped waiting, and
    /// nothing changes).
    pub(super) async fn note(
        &mut self,
        n: &NoteAsk,
        take: impl FnOnce() -> bool,
    ) -> Result<(), super::Told> {
        use super::Told;
        let Some(a) = self.shared.core.kernel.outbox_action(&n.post).ok().flatten() else {
            return Err(Told::NoteGone);
        };
        if !folded_at(&a, n.at) {
            return Err(Told::NotHere);
        }
        let Some(text) = words(&a) else {
            return Err(Told::NoteGone);
        };
        if !take() {
            return Err(Told::Cancelled);
        }
        let content = if n.open { text } else { line(post_kind(&a)) };
        // The vault's values never leave, as at every exit (`write`).
        let content = self.shared.core.exits.text(&content).0.into_owned();
        let buttons = Buttons::FoldNote(n.post.clone(), n.open);
        match self.edit(n.at.0, n.at.1, &content, &buttons).await {
            Ok(()) => Ok(()),
            Err(e) if e.away => {
                self.away(&e);
                Err(Told::Away)
            }
            Err(e) => {
                self.shared.board.error("note press", None, e.message);
                Err(Told::NoteGone)
            }
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
