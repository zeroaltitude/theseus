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
//! channel, in both forms. A new delivery decides folded or open from the
//! place's word now; once a write of the key has landed, the lane keeps
//! beside its cached message how it was written (folded or open, and
//! whether this delivery added the mark of a possible copy,
//! `render::RESENT`), and a later write of the key (a retry after a failed
//! settle) replays that, whatever the word says by then. The mark is a
//! fact of the delivery (`copy_mark`), never read from the words.
//!
//! A folded write's `(channel, message)` is kept with its post as an
//! additive key of the post's detail, `note_fold`, with `resent` when the
//! write bore the mark: both forms are rebuilt with it from that record,
//! never from the words. A press names the post
//! (`runtime/fold.rs`) and is honoured only when that stored pair is the
//! pressed channel and message: a note written open or with a button has no
//! such pair, so no press can fold it or take its buttons away. The words
//! are read back from the post itself, by the same functions the lane wrote
//! them with, and masked at the exit as every write is.

use std::collections::HashMap;

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
struct Folded {
    post: String,
    at: (u64, u64),
    resent: bool,
}

/// How a note key's cached message was written: replayed on a later write
/// of the key while the lane's cache still holds that message.
struct Shown {
    post: String,
    at: (u64, u64),
    folded: bool,
    resent: bool,
}

/// What a lane keeps of its notes' writes.
#[derive(Default)]
pub(super) struct Notes {
    /// Whether the delivery appended the copy's mark to the write being
    /// written now (`copy_mark`).
    appended: bool,
    /// Where the post being delivered was written folded.
    pending: Option<Folded>,
    /// By key, how each note's cached message was written, until its post
    /// settles.
    shown: HashMap<String, Shown>,
}

/// `s`, with the mark of a possible copy when `resent`.
fn marked(s: String, resent: bool) -> String {
    if resent {
        format!("{s}\n{}", crate::render::RESENT)
    } else {
        s
    }
}

/// The settle faults a test set: a post's next settle fails once, or, held,
/// every settle until it is released.
#[cfg(test)]
static FAIL_SETTLE: std::sync::Mutex<Vec<(String, bool)>> = std::sync::Mutex::new(Vec::new());

/// The posts whose settles a test's fault failed, one entry each time.
#[cfg(test)]
static SETTLE_FAILED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Make post `post`'s next settle by a lane fail once (a test's fault).
#[cfg(test)]
pub(crate) fn fail_settle_once(post: &str) {
    FAIL_SETTLE.lock().unwrap().push((post.to_string(), false));
}

/// Make every settle of post `post` fail until [`release_settle`].
#[cfg(test)]
pub(crate) fn hold_settle(post: &str) {
    FAIL_SETTLE.lock().unwrap().push((post.to_string(), true));
}

/// Let post `post`'s settles through again.
#[cfg(test)]
pub(crate) fn release_settle(post: &str) {
    FAIL_SETTLE.lock().unwrap().retain(|(p, _)| p != post);
}

/// Whether post `post`'s settle fault is still to fire.
#[cfg(test)]
pub(crate) fn settle_fault_pending(post: &str) -> bool {
    FAIL_SETTLE.lock().unwrap().iter().any(|(p, _)| p == post)
}

/// How many of post `post`'s settles a test's fault failed.
#[cfg(test)]
pub(crate) fn settle_failures(post: &str) -> usize {
    SETTLE_FAILED
        .lock()
        .unwrap()
        .iter()
        .filter(|p| *p == post)
        .count()
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

    /// Whether write `w` of post `a`, reaching `channel`, may be folded
    /// there: an FYI of an informing kind with no button and no message of
    /// its own to edit, not a met watch, and not in the notices place.
    /// Whether it is, is the place's word for a new delivery, else how its
    /// message was written ([`replay`](Self::replay)).
    fn may_fold(&self, a: &Action, w: &Write, channel: u64) -> bool {
        let notices = self.notices_place((0, String::new())).0;
        w.message.is_none()
            && w.buttons == Buttons::Keep
            && body_of(a)["purpose"].as_str() != Some("watch.met")
            && notices::informs(post_kind(a), &w.content)
            && (notices == 0 || channel != notices)
    }

    /// How key `key`'s message was written, folded and resent, while the
    /// lane's cache still holds that very message.
    fn replay(&self, key: &str) -> Option<(bool, bool)> {
        let s = self.notes.shown.get(key)?;
        (self.msgs.get(key) == Some(&s.at)).then_some((s.folded, s.resent))
    }

    /// Write `w` with the mark of a possible copy when the delivery is a
    /// `resend` and `w` is a new message's create; the lane keeps whether it
    /// did, for the note write that follows (`note_write`).
    pub(super) fn copy_mark(&mut self, w: &mut Write, resend: bool) {
        let append = resend
            && w.buttons != Buttons::V2
            && w.message.is_none()
            && !self.msgs.contains_key(&w.key);
        if append {
            w.content = format!("{}\n{}", w.content, crate::render::RESENT);
        }
        self.notes.appended = append;
    }

    /// Write `w` of post `a`. A note that [`may_fold`](Self::may_fold) is
    /// written at its effective destination ([`effective`](Self::effective)),
    /// folded or open as its message was written before
    /// ([`replay`](Self::replay)), else as the place says now, with the copy's
    /// mark as the delivery added it; how it was written is kept beside its
    /// cached message, and, folded, for the post's detail. Every other write
    /// is the lane's as ever.
    pub(super) async fn note_write(
        &mut self,
        a: &Action,
        w: &Write,
    ) -> Result<Option<(u64, u64)>, SendErr> {
        let channel = self.effective(w);
        if !self.may_fold(a, w, channel) {
            return self.write(w).await;
        }
        let appended = self.notes.appended;
        let tail = format!("\n{}", crate::render::RESENT);
        let words = match appended {
            true => w.content.strip_suffix(&tail).unwrap_or(&w.content),
            false => &w.content,
        };
        let (folded, resent) = match self.replay(&w.key) {
            Some((folded, resent)) => (folded, resent),
            None => (self.shared.steps_in(channel) == Steps::Folded, appended),
        };
        let mut f = w.clone();
        f.channel = channel;
        if folded {
            f.content = marked(line(post_kind(a)), resent);
            f.buttons = Buttons::FoldNote(a.correlation_id.clone(), false);
        } else {
            f.content = marked(words.to_string(), resent);
        }
        let wrote = self.write(&f).await;
        if let Ok(Some(at)) = wrote {
            let post = a.correlation_id.clone();
            if folded {
                let post = post.clone();
                self.notes.pending = Some(Folded { post, at, resent });
            }
            let msgs = &self.msgs;
            self.notes.shown.retain(|k, s| msgs.get(k) == Some(&s.at));
            let shown = Shown {
                post,
                at,
                folded,
                resent,
            };
            self.notes.shown.insert(w.key.clone(), shown);
        }
        wrote
    }

    /// Post `a`'s detail, with where it was written folded, if it was.
    pub(super) fn mark_folded(&mut self, a: &Action, mut detail: Value) -> Value {
        match self.notes.pending.take() {
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

    /// The outbox's settle of post `a`, as the lane records every post; once
    /// it is recorded, how its notes were written is no longer kept. A test
    /// may make it fail ([`fail_settle_once`], [`hold_settle`]).
    pub(super) fn outbox_settle(
        &mut self,
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
            if let Some(i) = f.iter().position(|(p, _)| p == correlation_id) {
                if !f[i].1 {
                    f.remove(i);
                }
                SETTLE_FAILED
                    .lock()
                    .unwrap()
                    .push(correlation_id.to_string());
                anyhow::bail!("a test's settle failed, as on a full disk");
            }
        }
        let outbox = &self.shared.core.outbox;
        let r = outbox.settle(correlation_id, outcome, receipt, first, detail, producer);
        if r.is_ok() {
            self.notes.shown.retain(|_, s| s.post != correlation_id);
        }
        r
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
