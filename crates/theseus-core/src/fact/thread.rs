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

/// A thread was folded into another (`thread.merged`, theseus-vt9k): the row
/// holds the retired thread's card and the state it was in, so nothing the
/// merge changed is lost to the record.
pub struct ThreadMerged<'a> {
    pub from: &'a ThreadInfo,
    pub into: &'a ThreadInfo,
    /// The state `from` was in before it was retired.
    pub was: &'a str,
    pub topics_left_out: &'a [String],
    /// The surfaces `from` had, now archived: a surface that reads the row
    /// points each at `into`'s.
    pub archived: &'a [String],
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for ThreadMerged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadMerged);

    fn row(&self) -> Value {
        json!({"from": self.from.slug, "into": self.into.slug, "was": self.was,
               "card": self.from.card, "topics_left_out": self.topics_left_out,
               "archived_surfaces": self.archived,
               "sealed": self.into.sealed.is_some(), "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {} merged the thread {} into {}; {} is retired and nothing of it is deleted.",
                self.who, self.from.slug, self.into.slug, self.from.slug
            ),
        );
    }
}

/// A merge was undone (`thread.unmerged`).
pub struct ThreadUnmerged<'a> {
    pub from: &'a ThreadInfo,
    pub into: &'a ThreadInfo,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for ThreadUnmerged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadUnmerged);

    fn row(&self) -> Value {
        json!({"from": self.from.slug, "into": self.into.slug,
               "state": self.from.state.as_str(), "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {} undid the merge of {} into {}; {} is {} again.",
                self.who,
                self.from.slug,
                self.into.slug,
                self.from.slug,
                self.from.state.as_str()
            ),
        );
    }
}

/// Part of a thread became a thread of its own (`thread.split`): the new
/// thread's record, the source, and the span of positions it quotes.
pub struct ThreadSplit<'a> {
    pub thread: &'a ThreadInfo,
    pub from: &'a str,
    pub quoted: u32,
    pub first_pos: u64,
    pub last_pos: u64,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for ThreadSplit<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadSplit);

    fn row(&self) -> Value {
        json!({"slug": self.thread.slug, "title": self.thread.title, "from": self.from,
               "quoted": self.quoted, "first_pos": self.first_pos, "last_pos": self.last_pos,
               "sealed": self.thread.sealed.is_some(), "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {} split the thread {} out of {}: {} quoted from @{} to @{}; nothing of {} moved.",
                self.who,
                self.thread.slug,
                self.from,
                crate::narrative::count(self.quoted as u64, "message", "messages"),
                self.first_pos,
                self.last_pos,
                self.from
            ),
        );
    }
}
