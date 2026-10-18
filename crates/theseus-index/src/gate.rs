//! A recall query's load gate (theseus-d7fy). A query's embedding splits each
//! matmul across the queries' pool, and beside a busy machine four threads
//! lose to one (theseus-y9a3: each matmul waits for its slowest job, and one
//! preempted thread stalls the layer; with 1.5 times the cores in runnable
//! threads, one thread had a recall's vectors within its deadline 10 times in
//! 12, four threads 1 in 16). Idle, four win (75 ms against 90). So the pool
//! is several threads, and the gate reads the machine before each query:
//!
//! - **calm**: the query embeds on the full pool, as any number of others do;
//! - **busy** (CPU or IO pressure at or over `theseus_store::pressure`'s
//!   thresholds): the query embeds on **one thread**, and the gate admits **one
//!   query at a time**: the others wait in order, inside their own recall's
//!   deadline (a caller that has gone, [`Missed::Gone`], leaves the line at
//!   once), and a caller that sets none leaves at [`MAX_WAIT`] with the words
//!   alone ([`Missed::Waited`], a `skipped` vector source, never an error).
//!
//! Pressure is read at most once a [`LOOK_EVERY`] (PSI's averages move every
//! 2 s), and never a file per query. A kernel without PSI reads as calm: the
//! gate is open. The wait runs on the connection's own thread, which keeps
//! the daemon's priority (`priority`): the gate never lowers a query.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long pressure's read is kept. PSI's `avg10` moves every 2 s.
pub const LOOK_EVERY: Duration = Duration::from_secs(1);

/// The longest a query waits for its turn when its caller never goes: a
/// recall's deadline is 250 ms and its caller closes the connection at it, so
/// this is for a caller that sets none (`theseus memory index search`).
pub const MAX_WAIT: Duration = Duration::from_secs(1);

/// How often a waiting query asks whether its caller has gone.
const SLICE: Duration = Duration::from_millis(5);

/// How a query is to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Calm: the queries' full pool.
    Full,
    /// Busy: one thread, and the only query running.
    One,
}

/// Why a query left the gate without its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missed {
    /// Its caller closed the connection while it waited.
    Gone,
    /// It waited [`MAX_WAIT`].
    Waited,
}

/// The gate's counts, as `index.status` says them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Whether pressure holds the gate shut now.
    pub shut: bool,
    /// Queries that waited for their turn, since start.
    pub waited: u64,
    /// Of those, the ones that left without it.
    pub expired: u64,
    /// Queries that embedded on one thread, since start.
    pub one_thread: u64,
}

#[derive(Debug, Default)]
struct Line {
    held: bool,
    next: u64,
    queue: VecDeque<u64>,
}

/// The gate: a cached read of the machine's pressure, and the line a busy
/// machine's queries wait in.
#[derive(Debug)]
pub struct Gate {
    look: Duration,
    read: Mutex<Option<(Instant, bool)>>,
    line: Mutex<Line>,
    turn: Condvar,
    waited: AtomicU64,
    expired: AtomicU64,
    one_thread: AtomicU64,
    /// A test's say over the machine's pressure: 0 none, 1 calm, 2 busy.
    #[cfg(test)]
    forced: std::sync::atomic::AtomicU8,
}

impl Default for Gate {
    fn default() -> Self {
        Self::with_look(LOOK_EVERY)
    }
}

/// A query's turn: on [`Lane::One`] it is the line's only query, and its drop
/// lets the next one in.
#[must_use = "the turn ends when this is dropped"]
pub struct Permit<'a> {
    gate: &'a Gate,
    lane: Lane,
}

impl Gate {
    /// A gate that keeps pressure's read for `look`.
    pub fn with_look(look: Duration) -> Self {
        Self {
            look,
            read: Mutex::new(None),
            line: Mutex::new(Line::default()),
            turn: Condvar::new(),
            waited: AtomicU64::new(0),
            expired: AtomicU64::new(0),
            one_thread: AtomicU64::new(0),
            #[cfg(test)]
            forced: std::sync::atomic::AtomicU8::new(0),
        }
    }

    /// A test's say: the machine is `busy` (or calm) whatever the kernel
    /// reads, until `None`.
    #[cfg(test)]
    pub(crate) fn force(&self, busy: Option<bool>) {
        let v = busy.map_or(0, |b| 1 + u8::from(b));
        self.forced.store(v, Ordering::SeqCst);
        *self.read.lock().unwrap() = None;
    }

    /// Whether the machine is busy now: the kernel's pressure, read at most
    /// once a `look`. Nothing without PSI.
    pub fn shut(&self) -> bool {
        #[cfg(test)]
        match self.forced.load(Ordering::SeqCst) {
            1 => return false,
            2 => return true,
            _ => {}
        }
        let mut read = self.read.lock().unwrap();
        if let Some((at, busy)) = *read {
            if at.elapsed() < self.look {
                return busy;
            }
        }
        let busy = theseus_store::pressure::busy().is_some();
        *read = Some((Instant::now(), busy));
        busy
    }

    /// Take a query's turn: at once and on the full pool when calm; when busy,
    /// in order, one query at a time, until `gone` says its caller has or
    /// [`MAX_WAIT`] passes.
    pub fn admit(&self, gone: &(dyn Fn() -> bool + Sync)) -> Result<Permit<'_>, Missed> {
        self.admit_within(gone, MAX_WAIT)
    }

    /// [`Gate::admit`] with its own longest wait.
    pub fn admit_within(
        &self,
        gone: &(dyn Fn() -> bool + Sync),
        max: Duration,
    ) -> Result<Permit<'_>, Missed> {
        if !self.shut() {
            return Ok(Permit {
                gate: self,
                lane: Lane::Full,
            });
        }
        let start = Instant::now();
        let mut line = self.line.lock().unwrap();
        let ticket = line.next;
        line.next += 1;
        line.queue.push_back(ticket);
        let mut counted = false;
        loop {
            if !line.held && line.queue.front() == Some(&ticket) {
                line.queue.pop_front();
                line.held = true;
                break;
            }
            if !counted {
                counted = true;
                self.waited.fetch_add(1, Ordering::Relaxed);
            }
            // The machine went calm while it waited: the gate is open, and a
            // calm query takes no turn.
            if !self.shut() {
                line.queue.retain(|t| *t != ticket);
                self.turn.notify_all();
                drop(line);
                return Ok(Permit {
                    gate: self,
                    lane: Lane::Full,
                });
            }
            let missed = if gone() {
                Some(Missed::Gone)
            } else if start.elapsed() >= max {
                Some(Missed::Waited)
            } else {
                None
            };
            if let Some(m) = missed {
                line.queue.retain(|t| *t != ticket);
                self.expired.fetch_add(1, Ordering::Relaxed);
                // The one behind may be first now.
                self.turn.notify_all();
                return Err(m);
            }
            line = self.turn.wait_timeout(line, SLICE).unwrap().0;
        }
        drop(line);
        // Its turn came: the machine may have gone calm while it waited.
        let lane = if self.shut() { Lane::One } else { Lane::Full };
        if lane == Lane::One {
            self.one_thread.fetch_add(1, Ordering::Relaxed);
        } else {
            self.release();
            // Not the line's any more: a calm query takes no slot.
            return Ok(Permit { gate: self, lane });
        }
        Ok(Permit { gate: self, lane })
    }

    fn release(&self) {
        self.line.lock().unwrap().held = false;
        self.turn.notify_all();
    }

    /// The counts now.
    pub fn counts(&self) -> Counts {
        Counts {
            shut: self.shut(),
            waited: self.waited.load(Ordering::Relaxed),
            expired: self.expired.load(Ordering::Relaxed),
            one_thread: self.one_thread.load(Ordering::Relaxed),
        }
    }
}

impl Permit<'_> {
    /// How this query is to run.
    pub fn lane(&self) -> Lane {
        self.lane
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if self.lane == Lane::One {
            self.gate.release();
        }
    }
}
