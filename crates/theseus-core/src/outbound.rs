//! A connection's one ordered outbound queue, with the backlog cap
//! (theseus-in3, design `stage2` §2.5). Responses and notifications share it,
//! so a turn's events precede its response on the wire, and so the cap is on
//! it, not on any one subscription.
//!
//! - Every message queued counts until the connection's writer has written
//!   it.
//! - Past `BACKLOG_CAP` queued (a test's connection may set a lower cap), a notification is dropped and counted, with
//!   its stream, and so is every later one until the writer has drained the
//!   queue. Responses always go.
//! - Once drained, the writer sends one `events.lost { dropped, streams }`,
//!   and notifications flow again: the client re-reads each stream named.
//!
//! A stuck client (a suspended `theseus watch`, a frozen tab) so costs the
//! daemon at most the cap's worth of messages, and learns what it missed.

use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use theseus_protocol::{EventsLost, Message};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// Messages a connection may have queued before its notifications are
/// dropped (the design's constant; a setting only if a real client meets it).
pub const BACKLOG_CAP: usize = 4096;

/// A notification's NDJSON line, its newline included: serialized once and
/// shared by every connection that queues it (theseus-celu.36).
pub type Line = Arc<str>;

/// What a connection's queue holds: a message its writer serializes (a
/// response, or anything else that goes to it alone), or a notification's
/// shared line, written as it is.
pub enum Item {
    Message(Message),
    Line(Line),
}

/// A notification on its way to the connections that hear it: serialized at
/// the first one that queues it, and that line shared by the rest. One that
/// no connection queues (none hears it, or every one is past its cap) is
/// never serialized (theseus-celu.36).
pub struct Note<'a> {
    msg: &'a Message,
    line: OnceCell<Option<Line>>,
}

impl<'a> Note<'a> {
    pub fn new(msg: &'a Message) -> Self {
        Self {
            msg,
            line: OnceCell::new(),
        }
    }

    pub fn message(&self) -> &'a Message {
        self.msg
    }

    /// Its line, serialized at the first ask. None for a message that does
    /// not serialize, which is skipped, as the writer always skipped one.
    fn line(&self) -> Option<Line> {
        self.line
            .get_or_init(|| {
                #[cfg(test)]
                counts::serialized();
                let mut s = serde_json::to_string(self.msg).ok()?;
                s.push('\n');
                Some(Line::from(s))
            })
            .clone()
    }
}

struct Shared {
    /// Queued and not yet written.
    queued: AtomicUsize,
    /// From the first dropped notification until the writer has drained the
    /// queue and taken what was lost.
    dropping: AtomicBool,
    /// What was dropped since: how many, and their streams.
    lost: Mutex<(u64, BTreeSet<String>)>,
    /// Every connection's dropped notifications, for health.
    total: Arc<AtomicU64>,
    /// Its cap: `BACKLOG_CAP` for a connection's (a test's may be lower),
    /// and none for a queue with no writer (a test's raw channel).
    cap: usize,
}

/// Where a queue's items go: a connection's writer, or a raw channel whose
/// reader (a test's) takes messages.
#[derive(Clone)]
enum Tx {
    Wire(UnboundedSender<Item>),
    Raw(UnboundedSender<Message>),
}

/// The sending side: what the bus, the narrator, and a turn's sink hold.
#[derive(Clone)]
pub struct Outbound {
    tx: Tx,
    shared: Arc<Shared>,
}

/// The connection writer's side.
pub struct Drain {
    rx: UnboundedReceiver<Item>,
    shared: Arc<Shared>,
}

/// A connection's queue: `total` counts what every connection drops.
pub fn channel(total: Arc<AtomicU64>) -> (Outbound, Drain) {
    channel_capped(total, BACKLOG_CAP)
}

/// A connection's queue with its cap: `BACKLOG_CAP`, or the lower one a
/// test sets (`Push::backlog_cap`), so a few hundred events overflow it.
pub fn channel_capped(total: Arc<AtomicU64>, cap: usize) -> (Outbound, Drain) {
    let (tx, rx) = unbounded_channel();
    let shared = Arc::new(Shared {
        queued: AtomicUsize::new(0),
        dropping: AtomicBool::new(false),
        lost: Mutex::default(),
        total,
        cap,
    });
    (
        Outbound {
            tx: Tx::Wire(tx),
            shared: shared.clone(),
        },
        Drain { rx, shared },
    )
}

/// A raw channel as a queue no writer drains: never capped, and each
/// notification its own copy of the message. For tests and in-process
/// readers that take every message.
impl From<UnboundedSender<Message>> for Outbound {
    fn from(tx: UnboundedSender<Message>) -> Self {
        Outbound {
            tx: Tx::Raw(tx),
            shared: Arc::new(Shared {
                queued: AtomicUsize::new(0),
                dropping: AtomicBool::new(false),
                lost: Mutex::default(),
                total: Arc::default(),
                cap: usize::MAX,
            }),
        }
    }
}

impl Outbound {
    /// A response, or anything else that must go: always queued. False once
    /// the connection is gone.
    pub fn respond(&self, m: Message) -> bool {
        self.queue(Item::Message(m))
    }

    /// Queue an item, counted until it is written.
    fn queue(&self, item: Item) -> bool {
        self.shared.queued.fetch_add(1, Ordering::AcqRel);
        let sent = match (&self.tx, item) {
            (Tx::Wire(tx), item) => tx.send(item).is_ok(),
            (Tx::Raw(tx), Item::Message(m)) => tx.send(m).is_ok(),
            // A raw channel is handed messages alone (`take`).
            (Tx::Raw(_), Item::Line(_)) => false,
        };
        if !sent {
            self.shared.queued.fetch_sub(1, Ordering::AcqRel);
        }
        sent
    }

    /// The note as this queue takes it: the shared line, or a raw channel's
    /// own copy of the message. None for a line that does not serialize.
    fn take(&self, note: &Note<'_>) -> Option<Item> {
        match self.tx {
            Tx::Wire(_) => note.line().map(Item::Line),
            Tx::Raw(_) => {
                #[cfg(test)]
                counts::cloned();
                Some(Item::Message(note.msg.clone()))
            }
        }
    }

    /// Queue a notification the note shares, or skip one that does not
    /// serialize: true unless the connection is gone.
    fn queue_note(&self, note: &Note<'_>) -> bool {
        match self.take(note) {
            Some(item) => self.queue(item),
            None => true,
        }
    }

    /// A notification of `stream` (`executions`, `session:<id>`,
    /// `narrative`, `policy`): queued, unless the queue is past the cap or
    /// still draining after it, when it is dropped and counted. Queued, it is
    /// the note's shared line, serialized at the first queue that takes it.
    /// False only once the connection is gone, so a dropped one keeps its
    /// subscriber.
    pub fn notify(&self, note: &Note<'_>, stream: &str) -> bool {
        if self.is_closed() {
            return false;
        }
        let s = &self.shared;
        let room =
            || !s.dropping.load(Ordering::Acquire) && s.queued.load(Ordering::Acquire) < s.cap;
        if room() {
            return self.queue_note(note);
        }
        let mut lost = s.lost.lock().unwrap();
        // The writer may have just drained the queue and taken the count.
        if room() {
            drop(lost);
            return self.queue_note(note);
        }
        s.dropping.store(true, Ordering::Release);
        lost.0 += 1;
        if !lost.1.contains(stream) {
            lost.1.insert(stream.to_string());
        }
        s.total.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn is_closed(&self) -> bool {
        match &self.tx {
            Tx::Wire(tx) => tx.is_closed(),
            Tx::Raw(tx) => tx.is_closed(),
        }
    }
}

impl Drain {
    /// The next item to write; None once every sender is gone.
    pub async fn recv(&mut self) -> Option<Item> {
        self.rx.recv().await
    }

    /// An item already queued, if any.
    pub fn try_recv(&mut self) -> Option<Item> {
        self.rx.try_recv().ok()
    }

    /// One item written. Once the queue has drained after dropping, what
    /// was lost, for the writer to send now, before anything queued later.
    pub fn written(&mut self) -> Option<EventsLost> {
        let s = &self.shared;
        let left = s.queued.fetch_sub(1, Ordering::AcqRel).saturating_sub(1);
        if left > 0 || !s.dropping.load(Ordering::Acquire) {
            return None;
        }
        let mut lost = s.lost.lock().unwrap();
        if s.queued.load(Ordering::Acquire) > 0 {
            return None;
        }
        let (dropped, streams) = std::mem::take(&mut *lost);
        s.dropping.store(false, Ordering::Release);
        Some(EventsLost {
            dropped,
            streams: streams.into_iter().collect(),
        })
    }
}

/// What the measurement counts (theseus-celu.36, `tests_push_once`): a
/// notification's serializations and deep copies, on this thread.
#[cfg(test)]
pub(crate) mod counts {
    use std::cell::Cell;

    thread_local! {
        static SERIALIZED: Cell<u64> = const { Cell::new(0) };
        static CLONED: Cell<u64> = const { Cell::new(0) };
    }

    pub fn serialized() {
        SERIALIZED.with(|c| c.set(c.get() + 1));
    }

    pub fn cloned() {
        CLONED.with(|c| c.set(c.get() + 1));
    }

    /// The serializations and copies since the last take.
    pub fn take() -> (u64, u64) {
        (SERIALIZED.with(|c| c.take()), CLONED.with(|c| c.take()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::{Event, Id, Response};

    /// A notification as its queue takes it.
    fn notify(tx: &Outbound, m: Message, stream: &str) -> bool {
        tx.notify(&Note::new(&m), stream)
    }

    fn note(i: u64) -> Message {
        Message::from(Event::EventsLost(EventsLost {
            dropped: i,
            streams: vec![],
        }))
    }

    /// Past the cap a notification is dropped and counted, a response still
    /// goes; once the writer drains, one notice says what was lost, and
    /// notifications flow again.
    #[test]
    fn past_the_cap_notifications_drop_until_the_queue_drains() {
        let total = Arc::new(AtomicU64::new(0));
        let (tx, mut rx) = channel(total.clone());
        for i in 0..BACKLOG_CAP as u64 {
            assert!(notify(&tx, note(i), "executions"));
        }
        assert!(notify(&tx, note(9), "executions"), "dropped, not closed");
        assert!(notify(&tx, note(9), "session:ses_1"));
        assert!(tx.respond(Message::Response(Response::ok(Id::Num(1), 1))));
        assert_eq!(total.load(Ordering::Relaxed), 2);
        let mut written = 0;
        let mut notices = Vec::new();
        while let Some(m) = rx.try_recv() {
            written += 1;
            // While draining, a new notification is dropped too.
            if written == 10 {
                assert!(notify(&tx, note(9), "narrative"));
            }
            drop(m);
            if let Some(l) = rx.written() {
                notices.push(l);
            }
        }
        assert_eq!(
            written,
            BACKLOG_CAP + 1,
            "every notification queued, and the response"
        );
        assert_eq!(
            notices,
            [EventsLost {
                dropped: 3,
                streams: vec![
                    "executions".into(),
                    "narrative".into(),
                    "session:ses_1".into()
                ]
            }]
        );
        assert!(notify(&tx, note(10), "executions"), "flowing again");
        assert!(rx.try_recv().is_some());
        assert_eq!(rx.written(), None, "nothing lost since");
    }

    /// A raw channel (a test's) is never capped, and a closed queue says so.
    #[test]
    fn a_raw_channel_is_never_capped_and_a_closed_one_says_so() {
        let (raw, mut rx) = unbounded_channel();
        let tx = Outbound::from(raw);
        for i in 0..(BACKLOG_CAP + 10) as u64 {
            assert!(notify(&tx, note(i), "executions"));
        }
        assert_eq!(
            std::iter::from_fn(|| rx.try_recv().ok()).count(),
            BACKLOG_CAP + 10
        );
        drop(rx);
        assert!(!notify(&tx, note(0), "executions"));
        assert!(!tx.respond(note(0)));
        assert!(tx.is_closed());
    }
}
