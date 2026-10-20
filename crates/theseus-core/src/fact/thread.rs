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
    /// The split's id, the nodes that left the source's compile (none for the model's tool, whose
    /// source keeps its turn's compile), and whether the turns went into a thread that existed.
    pub split: &'a str,
    pub moved: u32,
    pub into: bool,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for ThreadSplit<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadSplit);

    fn row(&self) -> Value {
        json!({"slug": self.thread.slug, "title": self.thread.title, "from": self.from,
               "quoted": self.quoted, "first_pos": self.first_pos, "last_pos": self.last_pos,
               "sealed": self.thread.sealed.is_some(), "split": self.split, "moved": self.moved,
               "into": self.into, "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {} split {} {} {}: {} quoted from @{} to @{}; {} nodes left its compile (nothing deleted, `thread unsplit {}` undoes it).",
                self.who,
                self.thread.slug,
                if self.into { "into" } else { "out of" },
                self.from,
                crate::narrative::count(self.quoted as u64, "message", "messages"),
                self.first_pos,
                self.last_pos,
                self.moved,
                self.split
            ),
        );
    }
}

/// A split undone (`thread.unsplit`): the source's compile reads the range again, the quoted turns
/// leave the target's.
pub struct ThreadUnsplit<'a> {
    pub split: &'a str,
    pub from: &'a str,
    pub thread: &'a str,
    pub restored: u32,
    pub retired: bool,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for ThreadUnsplit<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ThreadUnsplit);

    fn row(&self) -> Value {
        json!({"split": self.split, "from": self.from, "thread": self.thread,
               "restored": self.restored, "retired": self.retired,
               "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Thread: {} undid the split {} of {}: {} nodes are in its compile again, and {} {}.",
                self.who,
                self.split,
                self.from,
                self.restored,
                self.thread,
                if self.retired { "is done" } else { "no longer holds the quoted turns" }
            ),
        );
    }
}
