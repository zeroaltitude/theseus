//! What every part of `look.work` shares: one conversation's view of the whole, the words for a place,
//! and the rule for words that come from another conversation.
//!
//! **Another conversation's words** (a task's title, a question's reason, a reminder's note, a
//! routine's name) are marked as such by the part that gives them, and given, as every tool's result is
//! (`content::judges`). They were held back from a turn bound for a model of restricted origin until that
//! rule was deleted (2026-11-21). **And** a session that was given the words of a conversation whose data
//! is marked anything but open (`content_wait::mark_of`, read from its labels) takes that conversation's
//! `secret`, `confidential` and `personal` flags on ([`Reader::taken`], landed by `work::words`): its own
//! mark then says what it now holds, which is what who may see what by place reads.
//!
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use theseus_protocol::ops::DataMark;

use crate::scrub::Cuts;
use crate::Core;

/// The core, and the conversation that asks: what a part reads, and which words are its own.
pub(super) struct Reader<'a> {
    pub core: &'a Core,
    pub here: &'a str,
    /// Where the look's cuts may fall: never inside a value the scrubber knew when the call began.
    pub cuts: &'a Cuts,
    /// What each other conversation's labels mark it as, read once.
    marks: Mutex<HashMap<String, Option<&'static str>>>,
    /// The other conversations whose marked words were given.
    given: Mutex<BTreeSet<String>>,
}

impl<'a> Reader<'a> {
    pub(super) fn new(core: &'a Core, here: &'a str, cuts: &'a Cuts) -> Self {
        Self {
            core,
            here,
            cuts,
            marks: Mutex::default(),
            given: Mutex::default(),
        }
    }

    /// Where `session` speaks, as a person reads it: a bound place's name, or the CLI or the web UI
    /// with the end of the session's id.
    pub(super) fn place(&self, session: &str) -> String {
        place_words(&self.core.runner.place_name(session), self.cuts)
    }

    /// `text`, as a line of at most `max` characters; when `session` is another conversation whose
    /// data is marked, it is noted, so that this session takes its flags on.
    pub(super) fn words(&self, session: &str, text: &str, max: usize) -> String {
        self.note_given(session);
        super::clip(text, max, self.cuts)
    }

    /// `text`, as a line of at most `max` characters, in the hand that wrote it: the owner's, given as
    /// theirs; a model's, the words of the session it wrote them in ([`Reader::words`]).
    pub(super) fn hand(&self, hand: &crate::content::Hand, text: &str, max: usize) -> String {
        match hand {
            crate::content::Hand::Owner => super::clip(text, max, self.cuts),
            crate::content::Hand::Model(session) => self.words(session, text, max),
            crate::content::Hand::Unproven(maker) => self.unproven(maker.as_deref(), text, max),
        }
    }

    /// `text` whose writer could not be established: as that maker's words when the record says a model
    /// made it (`maker`, so its flags are taken on as for any of that conversation's words), else as they
    /// stand with nothing to take.
    pub(super) fn unproven(&self, maker: Option<&str>, text: &str, max: usize) -> String {
        match maker {
            Some(session) => self.words(session, text, max),
            None => super::clip(text, max, self.cuts),
        }
    }

    /// What `session`'s labels mark it as, when it is not open: `None` for an open one. Read once.
    fn mark(&self, session: &str) -> Option<&'static str> {
        *self
            .marks
            .lock()
            .unwrap()
            .entry(session.to_string())
            .or_insert_with(
                || match crate::content_wait::mark_of(&self.core.store, session) {
                    DataMark::Open => None,
                    DataMark::Private => Some("private"),
                    DataMark::Sensitive => Some("sensitive"),
                },
            )
    }

    /// Note another conversation whose words are given and whose data is marked, so that the session
    /// takes its flags on.
    fn note_given(&self, session: &str) {
        if session != self.here && self.mark(session).is_some() {
            self.given.lock().unwrap().insert(session.to_string());
        }
    }

    /// The flags this session takes on: those, among the labels of the conversations whose marked words
    /// it was given, that made them marked (`secret`, `confidential`, `personal`; an `injection` label
    /// marks nothing and is not taken). Each flag once.
    pub(super) fn taken(&self) -> Vec<crate::content::Flag> {
        let given: Vec<String> = self.given.lock().unwrap().iter().cloned().collect();
        let mut out: Vec<crate::content::Flag> = Vec::new();
        for s in given {
            let rec = self
                .core
                .store
                .get_session::<crate::session::SessionRecord>(&s);
            for l in rec
                .ok()
                .flatten()
                .and_then(|r| r.data)
                .iter()
                .flat_map(|d| &d.labels)
            {
                let marks = !matches!(l.flag, crate::content::Flag::Injection);
                if marks && !out.contains(&l.flag) {
                    out.push(l.flag);
                }
            }
        }
        out
    }
}

/// How many characters a short field (a place's name, a tool's name, a state, a series, a span) may
/// take: each is clipped by itself, not left to the part's own cut.
pub(super) const FIELD_CHARS: usize = 80;

/// `s` as a short field: one line of at most [`FIELD_CHARS`] characters, cut at a known value's edge.
pub(super) fn field(s: &str, cuts: &Cuts) -> String {
    super::clip(s, FIELD_CHARS, cuts)
}

/// A place's name as a person reads it: the CLI or the web UI with the end of the session's id, else
/// the bound name; clipped. Either is cut at a known value's edge.
pub(super) fn place_words(name: &str, cuts: &Cuts) -> String {
    match name.strip_suffix(" on the CLI or the web UI") {
        Some(id) => format!(
            "the CLI or the web UI (…{})",
            id_tail(name, id.len(), cuts)
        ),
        None => field(name, cuts),
    }
}

/// The end of the id held by `name`'s first `end` bytes, as `crate::task::short` takes it (its last six
/// characters), its edges settled on the whole name: it starts after any known value its start would fall
/// inside, and ends before any that runs over its end. Empty when nothing is left between them.
fn id_tail<'n>(name: &'n str, end: usize, cuts: &Cuts) -> &'n str {
    let start = name[..end].char_indices().rev().nth(5).map_or(0, |(i, _)| i);
    let (to, from) = cuts.around(name, end, start);
    name.get(from..to).unwrap_or("")
}
