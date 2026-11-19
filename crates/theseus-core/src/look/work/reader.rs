//! What every part of `look.work` shares: one conversation's view of the whole, the words for a place,
//! and the rule for words that come from another conversation.
//!
//! **Another conversation's words** (a task's title, a question's reason, a reminder's note, a
//! routine's name) are marked as such by the part that gives them, and are held back here when the
//! conversation they come from holds data marked anything but open (`content_wait::mark_of`, read from
//! its labels): a conversation whose labels say it holds a secret, a confidential or a personal item
//! does not have its words carried into another one by this tool. What is given is then judged as every
//! tool's result is (`content::judges`), by the data gate and the session's label rule.

use std::cell::RefCell;
use std::collections::HashMap;

use theseus_protocol::ops::DataMark;

use crate::Core;

/// The core, and the conversation that asks: what a part reads, and which words are its own.
pub(super) struct Reader<'a> {
    pub core: &'a Core,
    pub here: &'a str,
    /// Whether each other conversation's words are held back, read once.
    held: RefCell<HashMap<String, Option<&'static str>>>,
}

impl<'a> Reader<'a> {
    pub(super) fn new(core: &'a Core, here: &'a str) -> Self {
        Self {
            core,
            here,
            held: RefCell::default(),
        }
    }

    /// Where `session` speaks, as a person reads it: a bound place's name, or the CLI or the web UI
    /// with the end of the session's id.
    pub(super) fn place(&self, session: &str) -> String {
        let name = self.core.runner.place_name(session);
        match name.strip_suffix(" on the CLI or the web UI") {
            Some(id) => format!("the CLI or the web UI (…{})", crate::task::short(id)),
            None => name,
        }
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

    /// What `session`'s labels mark it as, when it is not open: `None` for an open one.
    fn holds(&self, session: &str) -> Option<&'static str> {
        if session == self.here {
            return None;
        }
        *self
            .held
            .borrow_mut()
            .entry(session.to_string())
            .or_insert_with(
                || match crate::content_wait::mark_of(&self.core.store, session) {
                    DataMark::Open => None,
                    DataMark::Private => Some("private"),
                    DataMark::Sensitive => Some("sensitive"),
                },
            )
    }
}
