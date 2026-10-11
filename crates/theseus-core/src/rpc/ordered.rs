//! One connection's requests that change a session apply in their arrival
//! order (theseus-klo2). Every request runs in a task of its own, so two
//! quick ones raced to their session: a paste of five lines, sent as five
//! `turn.submit`s on one connection, was stored 1, 3, 5, 4, 2.
//!
//! The lane is per connection and per session. The reader enters each
//! ordered request in the order its line arrived (`Lane::enter`, a lock and
//! a push, no hop); its task names the session it changes (`Core::lane_key`),
//! waits until every earlier one on the same connection and session has
//! taken effect, and then runs. A request takes effect at its first frame,
//! never at its answer: a `turn.submit` once its input is stored (its turn
//! may run for minutes, and ask a question the next request answers), a stop
//! or a cancel once the kernel has marked it, before the jobs it ends are
//! waited for. That point calls `applied()`; a request that never reaches it
//! (refused, failed) takes effect, as far as the lane cares, when it ends.
//!
//! A stop or a cancel waits less: once an earlier `turn.submit` for its
//! session holds the execution, or waits for admission to take it, the stop
//! reaches that input as it stands (`reachable()`, the kernel's mark of the
//! running turn, or `stopped_since` at its admission), so it goes ahead. A
//! stop behind an input queued for a running turn must not wait for that
//! turn's model to answer.
//!
//! The wait is measured (theseus-klo2's review, finding 7): from the line's
//! arrival to its lane clearing, as the turn trace root's `lane_us`
//! (`lane_us()`, read by the turn it starts) and `theseus.rpc.lane.wait` by
//! method, since a turn's own `arrived` is taken after the lane.
//!
//! What it is not: two connections are not ordered against each other (each
//! has its own lane), requests to two sessions on one connection never wait
//! for each other (the Discord binding speaks for every channel on one), and
//! reads never enter it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_protocol::method;
use tokio::sync::watch;

use super::Core;

/// The methods that change a session, each applied in its arrival order on
/// its connection: a turn's input, an answer, `/stop` and the cancel, a
/// recompile asked for, and the owner's retire and reopen. A task's cancel
/// and a wake's name another execution than the caller's session, and
/// `profile.use` no session at all: they stay unordered.
pub(super) const ORDERED: [&str; 7] = [
    method::TURN_SUBMIT,
    method::ACTION_CONFIRM,
    method::EXECUTION_STOP,
    method::EXECUTION_CANCEL,
    method::SESSION_RECOMPILE,
    method::SESSION_RETIRE,
    method::SESSION_REOPEN,
];

/// The ordered methods that stop what runs: they wait for an earlier input
/// on their session only until it is `reachable()`.
const STOPS: [&str; 2] = [method::EXECUTION_STOP, method::EXECUTION_CANCEL];

/// The session an ordered request changes, once its task has read it.
#[derive(Clone, PartialEq, Eq)]
enum Key {
    /// Not read yet: every later request waits for it.
    Unread,
    /// This session.
    Session(String),
    /// None the request can name (a new session, a correlation id that is
    /// no action): it waits for nothing, and nothing waits for it.
    Free,
}

/// One connection's lane: the ordered requests that have not taken effect,
/// in arrival order.
#[derive(Default)]
pub(super) struct Lane {
    open: Mutex<Open>,
    /// Moved at each change (a key read, a request applied): the waiters look
    /// again.
    moved: watch::Sender<u64>,
}

#[derive(Default)]
struct Open {
    next: u64,
    entries: VecDeque<Entry>,
}

struct Entry {
    seq: u64,
    key: Key,
    /// A stop or a cancel.
    stops: bool,
    /// An input a stop reaches now (`reachable()`).
    reached: bool,
}

impl Lane {
    /// Enter a request that is not a stop, in its arrival order: the unit
    /// tests' shorthand (the reader enters by `ticket_for`, before it spawns
    /// the request's task).
    #[cfg(test)]
    pub(super) fn enter(self: &Arc<Self>) -> Ticket {
        self.enter_as(false)
    }

    /// Enter a request that stops (`stops`) or any other.
    pub(super) fn enter_as(self: &Arc<Self>, stops: bool) -> Ticket {
        let mut open = self.open.lock().unwrap();
        let seq = open.next;
        open.next += 1;
        open.entries.push_back(Entry {
            seq,
            key: Key::Unread,
            stops,
            reached: false,
        });
        Ticket(Applied {
            lane: self.clone(),
            seq,
            entered: Instant::now(),
            waited_us: Arc::default(),
        })
    }

    /// An ordered method's ticket, entered now; none for any other.
    pub(super) fn ticket_for(self: &Arc<Self>, name: &str) -> Option<Ticket> {
        ORDERED
            .contains(&name)
            .then(|| self.enter_as(STOPS.contains(&name)))
    }

    fn changed(&self) {
        self.moved.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Whether `seq` may run: no earlier entry still open is for its
    /// session, or not read yet; for a stop, none that it cannot reach yet.
    fn clear(&self, seq: u64) -> bool {
        let open = self.open.lock().unwrap();
        let Some(mine) = open.entries.iter().find(|e| e.seq == seq) else {
            return true;
        };
        if mine.key == Key::Free {
            return true;
        }
        open.entries
            .iter()
            .take_while(|e| e.seq < seq)
            .all(|e| match &e.key {
                Key::Free => true,
                Key::Unread => false,
                k => *k != mine.key || (mine.stops && e.reached),
            })
    }

    #[cfg(test)]
    pub(super) fn open(&self) -> usize {
        self.open.lock().unwrap().entries.len()
    }
}

/// A request's place in its lane. Dropped, it has taken effect.
pub(super) struct Ticket(Applied);

impl Ticket {
    /// Name the session the request changes, then wait for the earlier ones
    /// on it. A lone request finds its lane clear and does not yield. How long
    /// it waited, from its line's arrival (`lane_us()` reads it in the
    /// request's task).
    pub(super) async fn wait(&self, key: Option<String>) -> Duration {
        let lane = &self.0.lane;
        let mut moved = lane.moved.subscribe();
        {
            let mut open = lane.open.lock().unwrap();
            if let Some(e) = open.entries.iter_mut().find(|e| e.seq == self.0.seq) {
                e.key = key.map_or(Key::Free, Key::Session);
            }
        }
        lane.changed();
        while !lane.clear(self.0.seq) {
            if moved.changed().await.is_err() {
                break;
            }
        }
        let waited = self.0.entered.elapsed();
        let us = u64::try_from(waited.as_micros()).unwrap_or(u64::MAX);
        self.0.waited_us.store(us, Ordering::Relaxed);
        waited
    }

    /// The mark the request's own code calls at the point it takes effect.
    pub(super) fn applied(&self) -> Applied {
        self.0.clone()
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.0.mark();
    }
}

/// The mark itself: idempotent, and the request's task holds it while it runs
/// (`scope`), so the code that writes its first frame can say so.
#[derive(Clone)]
pub(super) struct Applied {
    lane: Arc<Lane>,
    seq: u64,
    /// When the reader entered it: its line's arrival.
    entered: Instant,
    /// Its wait in the lane, once it ended.
    waited_us: Arc<AtomicU64>,
}

impl Applied {
    fn mark(&self) {
        let removed = {
            let mut open = self.lane.open.lock().unwrap();
            let before = open.entries.len();
            open.entries.retain(|e| e.seq != self.seq);
            open.entries.len() != before
        };
        if removed {
            self.lane.changed();
        }
    }

    fn reach(&self) {
        let reached = {
            let mut open = self.lane.open.lock().unwrap();
            open.entries
                .iter_mut()
                .find(|e| e.seq == self.seq && !e.reached)
                .map(|e| e.reached = true)
                .is_some()
        };
        if reached {
            self.lane.changed();
        }
    }
}

tokio::task_local! {
    static APPLIED: Applied;
}

/// Run an ordered request's work with its mark in reach of `applied()`.
pub(super) async fn scope<F: std::future::Future>(mark: Applied, work: F) -> F::Output {
    APPLIED.scope(mark, work).await
}

/// The input this task serves now holds its execution, or waits for
/// admission to take it: a stop from here on reaches it, so a stop behind it
/// on its lane may go ahead. Anywhere else, nothing.
pub(crate) fn reachable() {
    let _ = APPLIED.try_with(Applied::reach);
}

/// The request this task serves has taken effect (its first frame is
/// written): the next one on its lane may run. Anywhere else, nothing.
pub(crate) fn applied() {
    let _ = APPLIED.try_with(Applied::mark);
}

/// How long the ordered request this task serves waited in its lane, in µs,
/// from its line's arrival; `None` outside one (a driver's continuation, an
/// unordered method).
pub(crate) fn lane_us() -> Option<u64> {
    APPLIED
        .try_with(|a| a.waited_us.load(Ordering::Relaxed))
        .ok()
}

impl Core {
    /// The session an ordered request changes: named by its params, or read
    /// from the action or execution it names. None when there is none to
    /// read (a new session's turn), or the read fails: the method then
    /// answers that itself.
    pub(super) fn lane_key(&self, name: &str, params: &Value) -> Option<String> {
        let field = |k: &str| params.get(k).and_then(Value::as_str);
        match name {
            method::ACTION_CONFIRM => {
                let a = self.kernel.action(field("correlation_id")?).ok()??;
                Some(a.session_id)
            }
            method::EXECUTION_STOP | method::EXECUTION_CANCEL => {
                let e = self.kernel.execution(field("execution_id")?).ok()??;
                Some(e.session_id)
            }
            _ => field("session_id").map(str::to_string),
        }
    }
}
