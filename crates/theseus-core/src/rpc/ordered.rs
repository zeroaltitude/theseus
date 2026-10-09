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
//! What it is not: two connections are not ordered against each other (each
//! has its own lane), requests to two sessions on one connection never wait
//! for each other (the Discord binding speaks for every channel on one), and
//! reads never enter it.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

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
    entries: VecDeque<(u64, Key)>,
}

impl Lane {
    /// Enter a request, in its arrival order: the reader calls this before
    /// it spawns the request's task.
    pub(super) fn enter(self: &Arc<Self>) -> Ticket {
        let mut open = self.open.lock().unwrap();
        let seq = open.next;
        open.next += 1;
        open.entries.push_back((seq, Key::Unread));
        Ticket(Applied {
            lane: self.clone(),
            seq,
        })
    }

    /// An ordered method's ticket, entered now; none for any other.
    pub(super) fn ticket_for(self: &Arc<Self>, name: &str) -> Option<Ticket> {
        ORDERED.contains(&name).then(|| self.enter())
    }

    fn changed(&self) {
        self.moved.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Whether `seq` may run: no earlier entry still open is for its
    /// session, or not read yet.
    fn clear(&self, seq: u64) -> bool {
        let open = self.open.lock().unwrap();
        let Some(mine) = open.entries.iter().find(|(s, _)| *s == seq).map(|e| &e.1) else {
            return true;
        };
        if *mine == Key::Free {
            return true;
        }
        open.entries
            .iter()
            .take_while(|(s, _)| *s < seq)
            .all(|(_, k)| *k == Key::Free || (*k != Key::Unread && k != mine))
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
    /// on it. A lone request finds its lane clear and does not yield.
    pub(super) async fn wait(&self, key: Option<String>) {
        let lane = &self.0.lane;
        let mut moved = lane.moved.subscribe();
        {
            let mut open = lane.open.lock().unwrap();
            if let Some(e) = open.entries.iter_mut().find(|(s, _)| *s == self.0.seq) {
                e.1 = key.map_or(Key::Free, Key::Session);
            }
        }
        lane.changed();
        while !lane.clear(self.0.seq) {
            if moved.changed().await.is_err() {
                return;
            }
        }
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
}

impl Applied {
    fn mark(&self) {
        let removed = {
            let mut open = self.lane.open.lock().unwrap();
            let before = open.entries.len();
            open.entries.retain(|(s, _)| *s != self.seq);
            open.entries.len() != before
        };
        if removed {
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

/// The request this task serves has taken effect (its first frame is
/// written): the next one on its lane may run. Anywhere else, nothing.
pub(crate) fn applied() {
    let _ = APPLIED.try_with(Applied::mark);
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
