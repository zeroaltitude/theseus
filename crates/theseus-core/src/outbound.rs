//! A connection's one ordered outbound queue, with the backlog cap
//! (theseus-in3, design `stage2` §2.5). Responses and notifications share it,
//! so a turn's events precede its response on the wire, and so the cap is on
//! it, not on any one subscription.
//!
//! - Every message queued counts until the connection's writer has written
//!   it.
//! - Past `BACKLOG_CAP` queued, a notification is dropped and counted, with
//!   its stream, and so is every later one until the writer has drained the
//!   queue. Responses always go.
//! - Once drained, the writer sends one `events.lost { dropped, streams }`,
//!   and notifications flow again: the client re-reads each stream named.
//!
//! A stuck client (a suspended `theseus watch`, a frozen tab) so costs the
//! daemon at most the cap's worth of messages, and learns what it missed.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use theseus_protocol::{EventsLost, Message};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// Messages a connection may have queued before its notifications are
/// dropped (the design's constant; a setting only if a real client meets it).
pub const BACKLOG_CAP: usize = 4096;

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
    /// A queue with no writer (a test's raw channel) is never capped.
    capped: bool,
}

/// The sending side: what the bus, the narrator, and a turn's sink hold.
#[derive(Clone)]
pub struct Outbound {
    tx: UnboundedSender<Message>,
    shared: Arc<Shared>,
}

/// The connection writer's side.
pub struct Drain {
    rx: UnboundedReceiver<Message>,
    shared: Arc<Shared>,
}

/// A connection's queue: `total` counts what every connection drops.
pub fn channel(total: Arc<AtomicU64>) -> (Outbound, Drain) {
    let (tx, rx) = unbounded_channel();
    let shared = Arc::new(Shared {
        queued: AtomicUsize::new(0),
        dropping: AtomicBool::new(false),
        lost: Mutex::default(),
        total,
        capped: true,
    });
    (
        Outbound {
            tx,
            shared: shared.clone(),
        },
        Drain { rx, shared },
    )
}

/// A raw channel as a queue no writer drains: never capped. For tests and
/// in-process readers that take every message.
impl From<UnboundedSender<Message>> for Outbound {
    fn from(tx: UnboundedSender<Message>) -> Self {
        Outbound {
            tx,
            shared: Arc::new(Shared {
                queued: AtomicUsize::new(0),
                dropping: AtomicBool::new(false),
                lost: Mutex::default(),
                total: Arc::default(),
                capped: false,
            }),
        }
    }
}

impl Outbound {
    /// A response, or anything else that must go: always queued. False once
    /// the connection is gone.
    pub fn respond(&self, m: Message) -> bool {
        self.shared.queued.fetch_add(1, Ordering::AcqRel);
        if self.tx.send(m).is_err() {
            self.shared.queued.fetch_sub(1, Ordering::AcqRel);
            return false;
        }
        true
    }

    /// A notification of `stream` (`executions`, `session:<id>`,
    /// `narrative`, `policy`): queued, unless the queue is past the cap or
    /// still draining after it, when it is dropped and counted. False only
    /// once the connection is gone, so a dropped one keeps its subscriber.
    pub fn notify(&self, m: Message, stream: &str) -> bool {
        if self.tx.is_closed() {
            return false;
        }
        let s = &self.shared;
        let room = || {
            !s.dropping.load(Ordering::Acquire) && s.queued.load(Ordering::Acquire) < BACKLOG_CAP
        };
        if !s.capped || room() {
            return self.respond(m);
        }
        let mut lost = s.lost.lock().unwrap();
        // The writer may have just drained the queue and taken the count.
        if room() {
            drop(lost);
            return self.respond(m);
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
        self.tx.is_closed()
    }
}

impl Drain {
    /// The next message to write; None once every sender is gone.
    pub async fn recv(&mut self) -> Option<Message> {
        self.rx.recv().await
    }

    /// A message already queued, if any.
    pub fn try_recv(&mut self) -> Option<Message> {
        self.rx.try_recv().ok()
    }

    /// One message written. Once the queue has drained after dropping, what
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
            assert!(tx.notify(note(i), "executions"));
        }
        assert!(tx.notify(note(9), "executions"), "dropped, not closed");
        assert!(tx.notify(note(9), "session:ses_1"));
        assert!(tx.respond(Message::Response(Response::ok(Id::Num(1), 1))));
        assert_eq!(total.load(Ordering::Relaxed), 2);
        let mut written = 0;
        let mut notices = Vec::new();
        while let Some(m) = rx.try_recv() {
            written += 1;
            // While draining, a new notification is dropped too.
            if written == 10 {
                assert!(tx.notify(note(9), "narrative"));
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
        assert!(tx.notify(note(10), "executions"), "flowing again");
        assert!(rx.try_recv().is_some());
        assert_eq!(rx.written(), None, "nothing lost since");
    }

    /// A raw channel (a test's) is never capped, and a closed queue says so.
    #[test]
    fn a_raw_channel_is_never_capped_and_a_closed_one_says_so() {
        let (raw, mut rx) = unbounded_channel();
        let tx = Outbound::from(raw);
        for i in 0..(BACKLOG_CAP + 10) as u64 {
            assert!(tx.notify(note(i), "executions"));
        }
        assert_eq!(
            std::iter::from_fn(|| rx.try_recv().ok()).count(),
            BACKLOG_CAP + 10
        );
        drop(rx);
        assert!(!tx.notify(note(0), "executions"));
        assert!(!tx.respond(note(0)));
        assert!(tx.is_closed());
    }
}
