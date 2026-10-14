//! The threads' facts (theseus-nhew): one row for each edit of a thread, in
//! the frame that writes it, and a line of the narrative. The rows hold the
//! slug, the state and what changed, never the card's words (the record
//! holds them), except `thread.done`, whose row carries the card it leaves
//! to memory.

use serde_json::{json, Value};
use theseus_protocol::threads::{ThreadChanged, ThreadInfo};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, LedgerKind};

use super::{Fact, Say};

/// What an edit of a thread says of itself: the thread as written, who made
/// the edit and through which surface, and what it changed.
pub struct Change<'a> {
    pub thread: &'a ThreadInfo,
    pub who: &'a str,
    pub via: &'a str,
    /// The fields an edit changed (`thread.edited` only).
    pub changed: &'a [&'static str],
}

impl Change<'_> {
    fn data(&self) -> Value {
        let t = self.thread;
        json!({"slug": t.slug, "title": t.title, "state": t.state.as_str(),
               "topics": t.topics, "who": self.who, "via": self.via})
    }
}

macro_rules! thread_fact {
    ($(#[$doc:meta])* $name:ident, $kind:path, $change:literal, $line:literal) => {
        $(#[$doc])*
        pub struct $name<'a>(pub Change<'a>);

        impl Fact for $name<'_> {
            const KIND: Option<LedgerKind> = Some($kind);
            const METHOD: Option<&'static str> = Some(notify::THREAD_CHANGED);

            fn event(&self) -> Option<Event> {
                let c = &self.0;
                Some(Event::ThreadChanged(ThreadChanged::of(
                    c.thread, $change, c.who,
                )))
            }

            fn row(&self) -> Value {
                let mut d = self.0.data();
                if Self::KIND == Some(LedgerKind::ThreadEdited) {
                    d["changed"] = json!(self.0.changed);
                }
                if Self::KIND == Some(LedgerKind::ThreadDone) {
                    d["card"] = json!(self.0.thread.card);
                    d["goal"] = json!(self.0.thread.goal);
                    d["session"] = json!(self.0.thread.session_id);
                }
                d
            }

            fn narrate(&self, say: &mut Say<'_>) {
                say.line(
                    Session,
                    format!($line, who = self.0.who, slug = self.0.thread.slug),
                );
            }
        }
    };
}

thread_fact!(
    /// A thread was made (`thread.created`).
    ThreadCreated,
    LedgerKind::ThreadCreated,
    "created",
    "Thread: {who} made the thread {slug}; its first message opens its session."
);
thread_fact!(
    /// A thread's record was edited (`thread.edited`).
    ThreadEdited,
    LedgerKind::ThreadEdited,
    "edited",
    "Thread: {who} edited the thread {slug}."
);
thread_fact!(
    /// A thread was paused (`thread.paused`).
    ThreadPaused,
    LedgerKind::ThreadPaused,
    "paused",
    "Thread: {who} paused the thread {slug}; it takes no message until it is resumed."
);
thread_fact!(
    /// A paused thread took messages again (`thread.resumed`).
    ThreadResumed,
    LedgerKind::ThreadResumed,
    "resumed",
    "Thread: {who} resumed the thread {slug}."
);
thread_fact!(
    /// A thread was finished (`thread.done`): its session retires and its
    /// card goes to memory.
    ThreadDone,
    LedgerKind::ThreadDone,
    "done",
    "Thread: {who} finished the thread {slug}; its session is retired and its card is left to memory."
);
thread_fact!(
    /// A finished thread was taken up again (`thread.reopened`).
    ThreadReopened,
    LedgerKind::ThreadReopened,
    "reopened",
    "Thread: {who} reopened the thread {slug}; its next message opens a successor session."
);

/// A thread's session was opened by its first message (`thread.changed`
/// with `opened`): no row of its own (the session's records are the
/// record), the notification the Discord binding makes a Discord thread on.
pub struct ThreadOpened<'a>(pub Change<'a>);

impl Fact for ThreadOpened<'_> {
    const METHOD: Option<&'static str> = Some(notify::THREAD_CHANGED);

    fn event(&self) -> Option<Event> {
        let c = &self.0;
        Some(Event::ThreadChanged(ThreadChanged::of(
            c.thread, "opened", c.who,
        )))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {}'s first message opened the thread {}'s session.",
                self.0.who, self.0.thread.slug
            ),
        );
    }
}
