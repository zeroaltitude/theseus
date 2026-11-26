//! An FYI note folded where it lands (issue 56): a post the notices place's
//! rule says only informs (`notices::informs`: its kind is on `INFORMING` and
//! its head says FYI), with no button of its own and no met watch, written
//! to a conversation that folds its steps (`steps = "folded"`, the default),
//! is one closed line, `-# ℹ️ FYI: Theseus restarted`, said from its kind and
//! never from its words, with **Show note**. The fold is decided on the
//! write's actual channel, the DM a refused note falls back to included
//! (`notices.rs`): the notices place never folds, whatever its word and
//! whatever `announce` says. A note with a button and every other post are
//! written as ever. A note that comes while a turn runs is a post like any
//! other: its own message beside the turn's fold, folded or not by this
//! rule, never inside the fold, which takes only the turn's process
//! messages (`render/fold.rs`).
//!
//! The channel is the write's EFFECTIVE destination: the message this
//! post's key already has (`msgs`), when it has one, else the planned
//! channel. So a post sent again after its settle failed edits the message
//! it already wrote, and keeps it folded, or open, as it was written.
//!
//! A folded write's `(channel, message)` is kept with its post as an
//! additive key of the post's detail, `note_fold`, with `resent` when the
//! write bore the mark of a possible copy (`render::RESENT`): both forms
//! are rebuilt with that mark from the record, never from the words. A press names the post
//! (`runtime/fold.rs`) and is honoured only when that stored pair is the
//! pressed channel and message: a note written open or with a button has no
//! such pair, so no press can fold it or take its buttons away. The words
//! are read back from the post itself, by the same functions the lane wrote
//! them with, and masked at the exit as every write is.

use serde_json::{json, Value};
use theseus_core::outbox::{body_of, kind_of as post_kind};
use theseus_kernel::{Action, ActionState};

use super::{notices, Lane, Outcome, Receipt, SendErr, Write};
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

/// Where post `post` was written folded, and whether that write bore the
/// mark of a possible copy, until its settle records it.
pub(super) struct Folded {
    post: String,
    at: (u64, u64),
    resent: bool,
}

/// `s`, with the mark of a possible copy when `resent`.
fn marked(s: String, resent: bool) -> String {
    if resent {
        format!("{s}\n{}", crate::render::RESENT)
    } else {
        s
    }
}

/// The posts whose next settle a test makes fail once, as a full disk would.
#[cfg(test)]
static FAIL_SETTLE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Make post `post`'s next settle by a lane fail once (a test's fault).
#[cfg(test)]
pub(crate) fn fail_settle_once(post: &str) {
    FAIL_SETTLE.lock().unwrap().push(post.to_string());
}

/// Whether post `post`'s settle fault is still to fire.
#[cfg(test)]
pub(crate) fn settle_fault_pending(post: &str) -> bool {
    FAIL_SETTLE.lock().unwrap().iter().any(|p| p == post)
}

/// What a press on a folded note asks for: its post, open or closed, where.
pub(crate) struct NoteAsk {
    pub post: String,
    pub open: bool,
    pub at: (u64, u64),
}

impl Lane {
    /// The channel write `w` will actually reach: the one its key's message
    /// is in, when the lane wrote that message already (`write` edits it
    /// there), else the planned one. A note's key is no turn's, so no
    /// durable row recalls it (`recall.rs`): the lane's map is all there is.
    fn effective(&self, w: &Write) -> u64 {
        self.msgs.get(&w.key).map_or(w.channel, |(c, _)| *c)
    }

    /// Whether write `w` of post `a`, reaching `channel`, is folded there:
    /// an FYI of an informing kind with no button and no message of its own
    /// to edit, not a met watch, in a place that folds its steps and is not
    /// the notices place.
    fn folds(&self, a: &Action, w: &Write, channel: u64) -> bool {
        let notices = self.notices_place((0, String::new())).0;
        w.message.is_none()
            && w.buttons == Buttons::Keep
            && body_of(a)["purpose"].as_str() != Some("watch.met")
            && notices::informs(post_kind(a), &w.content)
            && (notices == 0 || channel != notices)
            && self.shared.steps_in(channel) == Steps::Folded
    }

    /// Write `w` of post `a`, folded when it [`folds`](Self::folds) where it
    /// actually lands ([`effective`](Self::effective)); where a folded write
    /// landed, and whether it bore a copy's mark (`RESENT`, kept on the
    /// line), is kept for the post's detail.
    pub(super) async fn note_write(
        &mut self,
        a: &Action,
        w: &Write,
    ) -> Result<Option<(u64, u64)>, SendErr> {
        let channel = self.effective(w);
        if !self.folds(a, w, channel) {
            return self.write(w).await;
        }
        let resent = w.content.ends_with(crate::render::RESENT);
        let mut f = w.clone();
        f.channel = channel;
        f.content = marked(line(post_kind(a)), resent);
        f.buttons = Buttons::FoldNote(a.correlation_id.clone(), false);
        let wrote = self.write(&f).await;
        if let Ok(Some(at)) = wrote {
            let post = a.correlation_id.clone();
            self.note_folded = Some(Folded { post, at, resent });
        }
        wrote
    }

    /// Post `a`'s detail, with where it was written folded, if it was.
    pub(super) fn mark_folded(&mut self, a: &Action, mut detail: Value) -> Value {
        match self.note_folded.take() {
            Some(f) if f.post == a.correlation_id => {
                let (c, m) = f.at;
                detail[MARK] = json!({"channel": c.to_string(), "id": m.to_string()});
                if f.resent {
                    detail[MARK]["resent"] = json!(true);
                }
            }
            _ => {}
        }
        detail
    }

    /// The outbox's settle of post `a`, as the lane records every post; a
    /// test may make it fail once ([`fail_settle_once`]).
    pub(super) fn outbox_settle(
        &self,
        correlation_id: &str,
        outcome: Outcome,
        receipt: Receipt,
        first: Option<String>,
        detail: Value,
        producer: &str,
    ) -> anyhow::Result<Action> {
        #[cfg(test)]
        {
            let mut f = FAIL_SETTLE.lock().unwrap();
            if let Some(i) = f.iter().position(|p| p == correlation_id) {
                f.remove(i);
                anyhow::bail!("a test's settle failed, as on a full disk");
            }
        }
        let outbox = &self.shared.core.outbox;
        outbox.settle(correlation_id, outcome, receipt, first, detail, producer)
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
        let Some(a) = self
            .shared
            .core
            .kernel
            .outbox_action(&n.post)
            .ok()
            .flatten()
        else {
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
        let resent = a.detail.as_ref().is_some_and(|d| d[MARK]["resent"] == true);
        let content = if n.open { text } else { line(post_kind(&a)) };
        let content = marked(content, resent);
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
