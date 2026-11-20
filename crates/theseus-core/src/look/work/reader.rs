//! What every part of `look.work` shares: one conversation's view of the whole, the words for a place,
//! and the rule for words that come from another conversation.
//!
//! **Another conversation's words** (a task's title, a question's reason, a reminder's note, a
//! routine's name) are marked as such by the part that gives them. They are held back only when the
//! asking turn's own model is one the session's labels could keep off (`TurnRunner::restricted_profile`,
//! the predicate the data gate and the route's floor read for a turn's target) AND the conversation they
//! come from holds data marked anything but open (`content_wait::mark_of`, read from its labels): a text
//! carrying a secret, a confidential or a personal item could otherwise reach such a model before its
//! judgment lands. For any other model the words are given, marked, and judged as every tool's result is
//! (`content::judges`), by the data gate and the session's label rule. **And** a session that was given
//! another conversation's marked words takes that conversation's flags on ([`Reader::taken`], landed by
//! `work::words`), so a later turn of it on a model of restricted origin is refused by the data gate's own
//! rule and does not wait on a judgment to arrive.
//!
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use theseus_protocol::ops::DataMark;

use crate::Core;

/// The core, and the conversation that asks: what a part reads, and which words are its own.
pub(super) struct Reader<'a> {
    pub core: &'a Core,
    pub here: &'a str,
    /// Whether the asking turn's model is one the labels could keep off: another conversation's marked
    /// words are held back from it.
    pub restricted: bool,
    /// What each other conversation's labels mark it as, read once.
    marks: Mutex<HashMap<String, Option<&'static str>>>,
    /// The other conversations whose marked words were given (to an unrestricted turn).
    given: Mutex<BTreeSet<String>>,
}

impl<'a> Reader<'a> {
    pub(super) fn new(core: &'a Core, here: &'a str, restricted: bool) -> Self {
        Self {
            core,
            here,
            restricted,
            marks: Mutex::default(),
            given: Mutex::default(),
        }
    }

    /// Where `session` speaks, as a person reads it: a bound place's name, or the CLI or the web UI
    /// with the end of the session's id.
    pub(super) fn place(&self, session: &str) -> String {
        place_words(&self.core.runner.place_name(session))
    }

    /// `text`, as a line of at most `max` characters, when `session` is this conversation; when it is
    /// another, the same, or the note that it is held back.
    pub(super) fn words(&self, session: &str, text: &str, max: usize) -> String {
        match self.holds(session) {
            Some(mark) if session != self.here => {
                format!("(held back: that conversation holds data marked {mark})")
            }
            _ => super::clip(text, max),
        }
    }

    /// `text`, as a line of at most `max` characters, in the hand that wrote it: the owner's, given as
    /// theirs; a model's, the words of the session it wrote them in ([`Reader::words`]: held back from a
    /// restricted turn when that conversation is marked, taken on by an unrestricted one).
    pub(super) fn hand(&self, hand: &crate::content::Hand, text: &str, max: usize) -> String {
        match hand {
            crate::content::Hand::Owner => super::clip(text, max),
            crate::content::Hand::Model(session) => self.words(session, text, max),
            crate::content::Hand::Unproven => self.unproven(text, max),
        }
    }

    /// `text` whose writer could not be established: held from a restricted turn, with a line that says so
    /// and names no conversation; given to any other turn, with no flags to take (no session is known to
    /// take them from).
    pub(super) fn unproven(&self, text: &str, max: usize) -> String {
        match self.restricted {
            true => "(held back: its writer could not be established)".into(),
            false => super::clip(text, max),
        }
    }

    /// `text` from notes Theseus imported (a proposal's words), as a line: held back from a model that
    /// could not be sent them, as another conversation's marked words are.
    pub(super) fn imported(&self, text: &str, max: usize) -> String {
        match self.restricted {
            true => "(held back: it comes from imported notes, and this turn's model is of restricted origin)".into(),
            false => super::clip(text, max),
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

    /// Whether another conversation's words are held back, and the mark that holds them. A
    /// conversation that is marked and whose words are given anyway (the turn is not of restricted
    /// origin) is noted, so that the session takes its flags on.
    fn holds(&self, session: &str) -> Option<&'static str> {
        if session == self.here {
            return None;
        }
        let mark = self.mark(session)?;
        match self.restricted {
            true => Some(mark),
            false => {
                self.given.lock().unwrap().insert(session.to_string());
                None
            }
        }
    }

    /// The flags this session takes on: those, among the labels of the conversations whose marked words
    /// it was given, that keep a model of restricted origin off (`Flag::disqualifies_cn`: the labels
    /// that made them marked; an `injection` label marks nothing and is not taken). Each flag once.
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
                if l.flag.disqualifies_cn() && !out.contains(&l.flag) {
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

/// `s` as a short field: one line of at most [`FIELD_CHARS`] characters.
pub(super) fn field(s: &str) -> String {
    super::clip(s, FIELD_CHARS)
}

/// A place's name as a person reads it: the CLI or the web UI with the end of the session's id, else
/// the bound name; clipped.
pub(super) fn place_words(name: &str) -> String {
    match name.strip_suffix(" on the CLI or the web UI") {
        Some(id) => format!("the CLI or the web UI (…{})", crate::task::short(id)),
        None => field(name),
    }
}
