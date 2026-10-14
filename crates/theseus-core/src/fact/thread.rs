//! The threads' facts (theseus-nhew): one row for each edit of a thread, in
//! the frame that writes it, and a line of the narrative. The rows hold the
//! slug, the state and what changed, never the card's words (the record
//! holds them), except `thread.done`, whose row carries the card it leaves
//! to memory.

use serde_json::{json, Value};
use theseus_protocol::threads::ThreadInfo;
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

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
    ($(#[$doc:meta])* $name:ident, $kind:path, $line:literal) => {
        $(#[$doc])*
        pub struct $name<'a>(pub Change<'a>);

        impl Fact for $name<'_> {
            const KIND: Option<LedgerKind> = Some($kind);

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
    "Thread: {who} made the thread {slug}; its first message opens its session."
);
thread_fact!(
    /// A thread's record was edited (`thread.edited`).
    ThreadEdited,
    LedgerKind::ThreadEdited,
    "Thread: {who} edited the thread {slug}."
);
thread_fact!(
    /// A thread was paused (`thread.paused`).
    ThreadPaused,
    LedgerKind::ThreadPaused,
    "Thread: {who} paused the thread {slug}; it takes no message until it is resumed."
);
thread_fact!(
    /// A paused thread took messages again (`thread.resumed`).
    ThreadResumed,
    LedgerKind::ThreadResumed,
    "Thread: {who} resumed the thread {slug}."
);
thread_fact!(
    /// A thread was finished (`thread.done`): its session retires and its
    /// card goes to memory.
    ThreadDone,
    LedgerKind::ThreadDone,
    "Thread: {who} finished the thread {slug}; its session is retired and its card is left to memory."
);
thread_fact!(
    /// A finished thread was taken up again (`thread.reopened`).
    ThreadReopened,
    LedgerKind::ThreadReopened,
    "Thread: {who} reopened the thread {slug}; its next message opens a successor session."
);
