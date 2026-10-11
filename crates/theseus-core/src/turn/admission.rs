//! Admission (§3.2a's scheduler; theseus-4g0l): which turn takes the next of
//! the ceiling's slots. One queue per class: a person's typed input first,
//! then wakes and continuations, then task turns; first come, first served
//! inside a class; and aging, so no class starves: a waiting turn ranks one
//! class higher for each [`AGE_STEP`] it has waited.
//!
//! A slot given back goes to exactly the next turn, picked from the three
//! queues' heads (O(1)): nothing polls, and nothing wakes a herd. It replaced
//! a 50 ms poll on a `notify_waiters` that woke every waiting turn, which
//! finished 42% of a burst's pairs out of order and a person's message 67th
//! of 81 (the scale report's row 7).
//!
//! A turn waiting on a foreground job lends its slot after [`LEND_AFTER`]
//! (`toolrun/job.rs`) and takes one again at its class's head when the job
//! ends. A wait fails only when the queue has not moved, no turn admitted,
//! for the stuck window (`[kernel] admission_stuck_secs`, 600 s): a long
//! queue that moves is slow, never stuck.
//!
//! The kernel counts no ceiling for a turn this queue admitted
//! (`Kernel::admit_queued`): a turn that lent its slot still holds its turn
//! there. The queue's state is one mutex, held across no `.await`. A grant
//! travels to its waiter as a [`Grant`], which gives the slot back when
//! dropped unread, so a waiter gone between the grant and its read loses no
//! slot.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant as StdInstant};

use anyhow::Result;
use theseus_kernel::{KernelError, TurnGuard};
use theseus_store::frames_written_here;
use tokio::sync::{oneshot, Notify};
use tokio::time::Instant;

use super::{turn_error, TurnRunner};

/// How long a waiting turn waits before it ranks one class higher. A task
/// turn that has waited this long ranks with the wakes, and twice this long
/// with a person's input, served from there in arrival order with them. So
/// a task turn waits at most two steps beyond the turns that arrived before
/// it, however many wakes and messages arrive after. Thirty seconds is a few
/// turns' length at the ceiling: long enough that a person's message rarely
/// queues behind an aged task, short enough that a run of wakes holds no
/// task back for long.
pub const AGE_STEP: Duration = Duration::from_secs(30);

/// A turn waiting on a foreground job lends its slot once it has waited
/// this long: a short job keeps its slot, so its turn goes on at once.
pub const LEND_AFTER: Duration = Duration::from_secs(2);

/// Who a turn is, for admission. Lower goes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// A person's typed input, in a conversation.
    Person = 0,
    /// A conversation's wake or continuation: a job's result, an answer, a
    /// due time, a crash's resume.
    Wake = 1,
    /// A task session's turn, its continuations included.
    Task = 2,
}

impl Class {
    const ALL: [Class; 3] = [Class::Person, Class::Wake, Class::Task];

    /// A turn's class: a task session's turns are tasks', and the rest are a
    /// person's when they bring input, a wake's when they do not.
    pub fn of(kind: theseus_protocol::SessionKind, has_input: bool) -> Class {
        match (kind, has_input) {
            (theseus_protocol::SessionKind::Task, _) => Class::Task,
            (_, true) => Class::Person,
            (_, false) => Class::Wake,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Class::Person => "person",
            Class::Wake => "wake",
            Class::Task => "task",
        }
    }

    /// Its rank after waiting `waited`: one class higher a step, to the top.
    fn aged(self, waited: Duration, step: Duration) -> u64 {
        let steps = waited.as_nanos() / step.as_nanos().max(1);
        (self as u64).saturating_sub(u64::try_from(steps).unwrap_or(u64::MAX))
    }
}

/// Where a turn reached the queue: its class and its ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Queued {
    pub class: Class,
    pub ticket: u64,
}

/// Why a wait for a slot failed: no turn was admitted for the whole window.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "admission is stuck: no turn was admitted for {window_s} s while this {class} turn waited \
     ({in_use} of {ceiling} slots held, {waiting} waiting). A queue that moves never fails a \
     turn: the turns holding the slots are not ending"
)]
pub struct Stuck {
    pub window_s: u64,
    pub class: &'static str,
    pub in_use: u32,
    pub ceiling: u32,
    pub waiting: usize,
}

struct Waiter {
    /// Arrival order. A retake's is below every arrival's, so it is its
    /// class's head.
    seq: u64,
    since: Instant,
    tx: oneshot::Sender<Grant>,
}

struct State {
    /// Slots held, lent ones not counted.
    in_use: u32,
    queues: [VecDeque<Waiter>; 3],
    next_seq: u64,
    /// The next turn's ticket: its place in the order turns reached the
    /// queue, taking a free slot or joining a line.
    next_ticket: u64,
    /// The next retake's seq, counting down below every arrival's.
    next_front: u64,
    /// When a turn was last admitted, or the queue last emptied.
    moved: Instant,
    admitted: u64,
}

impl State {
    fn waiting(&self) -> usize {
        self.queues.iter().map(VecDeque::len).sum()
    }

    /// The next waiter: the best of the three heads by aged rank, then by
    /// arrival. A queue's head is its best (the oldest, so the most aged),
    /// so three looks find the best of all.
    fn pop_next(&mut self, now: Instant, step: Duration) -> Option<Waiter> {
        let mut best: Option<(u64, u64, usize)> = None;
        for (i, q) in self.queues.iter().enumerate() {
            if let Some(w) = q.front() {
                let rank = Class::ALL[i].aged(now.saturating_duration_since(w.since), step);
                if best.is_none_or(|(r, s, _)| (rank, w.seq) < (r, s)) {
                    best = Some((rank, w.seq, i));
                }
            }
        }
        best.and_then(|(_, _, i)| self.queues[i].pop_front())
    }

    fn ticket(&mut self) -> u64 {
        self.next_ticket += 1;
        self.next_ticket
    }

    fn admit_now(&mut self) {
        self.in_use += 1;
        self.moved = Instant::now();
        self.admitted += 1;
    }
}

/// The queue. One per daemon, on the tool runtime, which the turn and a
/// job's wait both reach.
pub struct Admission {
    ceiling: u32,
    stuck: Duration,
    /// [`AGE_STEP`], but for a test's.
    step: Duration,
    state: Mutex<State>,
    /// Each running turn's lent mark, by its execution: a job's wait lends
    /// the slot of the turn it waits in.
    bound: Mutex<HashMap<String, (Arc<AtomicBool>, Class)>>,
    /// A slot that held a turn was dropped, so a turn ended: a turn that
    /// found its execution's turn held looks again.
    ended: Notify,
}

impl std::fmt::Debug for Admission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (in_use, waiting) = self.load();
        f.debug_struct("Admission")
            .field("ceiling", &self.ceiling)
            .field("in_use", &in_use)
            .field("waiting", &waiting)
            .finish()
    }
}

/// A slot handed to a waiter, in flight to it: dropped unread, it gives the
/// slot back.
pub struct Grant(Option<Arc<Admission>>);

impl Grant {
    /// The waiter has it: it is the waiter's slot now.
    fn keep(mut self) {
        self.0 = None;
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        if let Some(a) = self.0.take() {
            a.give_back();
        }
    }
}

/// A turn's slot: given back when dropped, unless lent.
pub struct Slot {
    adm: Arc<Admission>,
    class: Class,
    ticket: u64,
    lent: Arc<AtomicBool>,
    bound: Option<String>,
}

impl std::fmt::Debug for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slot")
            .field("class", &self.class)
            .field("lent", &self.lent.load(Ordering::SeqCst))
            .field("bound", &self.bound)
            .finish()
    }
}

impl Slot {
    pub fn class(&self) -> Class {
        self.class
    }

    /// Where its turn reached the queue: its class, and its ticket, the
    /// order of arrival at the queue (a turn's trace carries both).
    pub fn queued(&self) -> Queued {
        Queued {
            class: self.class,
            ticket: self.ticket,
        }
    }

    /// The slot is the turn of `execution_id` now: a job's wait in it may
    /// lend it (`Admission::lend`).
    pub fn bind(&mut self, execution_id: &str) {
        self.adm
            .bound
            .lock()
            .unwrap()
            .insert(execution_id.to_string(), (self.lent.clone(), self.class));
        self.bound = Some(execution_id.to_string());
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let held_a_turn = self.bound.is_some();
        if let Some(id) = self.bound.take() {
            // Its own entry: a later turn of the execution may hold it now.
            let mut bound = self.adm.bound.lock().unwrap();
            if bound
                .get(&id)
                .is_some_and(|(l, _)| Arc::ptr_eq(l, &self.lent))
            {
                bound.remove(&id);
            }
        }
        if !self.lent.load(Ordering::SeqCst) {
            self.adm.give_back();
        }
        // Only a slot that held a turn says a turn ended: a waiter that found
        // its execution held drops its own unbound slot before it waits, and
        // must not wake itself.
        if held_a_turn {
            self.adm.ended.notify_waiters();
        }
    }
}

impl Admission {
    pub fn new(ceiling: u32, stuck: Duration) -> Arc<Self> {
        Self::with_step(ceiling, stuck, AGE_STEP)
    }

    /// With an aging step of a test's.
    pub fn with_step(ceiling: u32, stuck: Duration, step: Duration) -> Arc<Self> {
        Arc::new(Self {
            ceiling: ceiling.max(1),
            stuck,
            step,
            state: Mutex::new(State {
                in_use: 0,
                queues: Default::default(),
                next_seq: 1 << 62,
                next_ticket: 0,
                next_front: 1 << 62,
                moved: Instant::now(),
                admitted: 0,
            }),
            bound: Mutex::default(),
            ended: Notify::new(),
        })
    }

    pub fn ceiling(&self) -> u32 {
        self.ceiling
    }

    /// Slots held now (lent ones not counted), and turns waiting.
    pub fn load(&self) -> (u32, usize) {
        let s = self.state.lock().unwrap();
        (s.in_use, s.waiting())
    }

    /// Whether a turn waits while a slot is free: never, once a call has
    /// returned (the stress test's sampler reads it).
    #[cfg(test)]
    fn waits_with_a_slot_free(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.waiting() > 0 && s.in_use < self.ceiling
    }

    /// Turns admitted since the start: a grant, a free slot taken, a retake.
    pub fn admitted(&self) -> u64 {
        self.state.lock().unwrap().admitted
    }

    fn slot(self: &Arc<Self>, class: Class, ticket: u64) -> Slot {
        Slot {
            adm: self.clone(),
            class,
            ticket,
            lent: Arc::default(),
            bound: None,
        }
    }

    /// A slot now, only when one is free and no turn waits: a turn that
    /// asks never passes the queue.
    pub fn try_take(self: &Arc<Self>, class: Class) -> Option<Slot> {
        let mut s = self.state.lock().unwrap();
        if s.in_use >= self.ceiling || s.waiting() > 0 {
            return None;
        }
        s.admit_now();
        let ticket = s.ticket();
        Some(self.slot(class, ticket))
    }

    /// Join `class`'s queue (`front`: at its head), or take a free slot at
    /// once when none waits: `Ok` is the slot, `Err` the wait for one.
    /// Its ticket comes with it.
    fn join(&self, class: Class, front: bool) -> (u64, Result<(), oneshot::Receiver<Grant>>) {
        let mut s = self.state.lock().unwrap();
        let ticket = s.ticket();
        if s.in_use < self.ceiling && s.waiting() == 0 {
            s.admit_now();
            return (ticket, Ok(()));
        }
        let (tx, rx) = oneshot::channel();
        let seq = if front {
            s.next_front -= 1;
            s.next_front
        } else {
            s.next_seq += 1;
            s.next_seq
        };
        let w = Waiter {
            seq,
            since: Instant::now(),
            tx,
        };
        let q = &mut s.queues[class as usize];
        if front {
            q.push_front(w);
        } else {
            q.push_back(w);
        }
        (ticket, Err(rx))
    }

    /// Wait for a slot in `class`'s queue (`front`: at its head, for a turn
    /// that had one). Fails only when no turn was admitted for the stuck
    /// window while it waited.
    pub async fn take(self: &Arc<Self>, class: Class, front: bool) -> Result<Slot, Stuck> {
        let since = Instant::now();
        let (ticket, joined) = self.join(class, front);
        let mut rx = match joined {
            Ok(()) => return Ok(self.slot(class, ticket)),
            Err(rx) => rx,
        };
        loop {
            let deadline = self.state.lock().unwrap().moved.max(since) + self.stuck;
            tokio::select! {
                got = &mut rx => {
                    // Its sender leaves the queue only by a grant, or by
                    // this wait's own failure below.
                    if let Ok(g) = got {
                        g.keep();
                    }
                    return Ok(self.slot(class, ticket));
                }
                () = tokio::time::sleep_until(deadline) => {}
            }
            let mut s = self.state.lock().unwrap();
            if Instant::now().saturating_duration_since(s.moved.max(since)) < self.stuck {
                continue;
            }
            // A grant is sent under this lock: one sent as the timer fired
            // is here now, and it is this turn's.
            if let Ok(g) = rx.try_recv() {
                drop(s);
                g.keep();
                return Ok(self.slot(class, ticket));
            }
            // Leave the queue: closed, this waiter's sender reads closed.
            // Only this failure path searches the queues.
            rx.close();
            for q in &mut s.queues {
                q.retain(|w| !w.tx.is_closed());
            }
            return Err(Stuck {
                window_s: self.stuck.as_secs(),
                class: class.as_str(),
                in_use: s.in_use,
                ceiling: self.ceiling,
                waiting: s.waiting(),
            });
        }
    }

    /// The turn of `execution_id` waits on something that is not its model
    /// or this machine (a foreground job): its slot goes to the next waiting
    /// turn. True when it was held and is lent now.
    pub fn lend(self: &Arc<Self>, execution_id: &str) -> bool {
        let Some((lent, _)) = self.bound.lock().unwrap().get(execution_id).cloned() else {
            return false;
        };
        if lent.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.give_back();
        true
    }

    /// Take a slot again after [`Admission::lend`], at the head of the
    /// turn's class: the next one given back is its own, unless an older
    /// turn has aged past its class. Never fails, since the turn is under
    /// way. Nothing lent, and it returns at once.
    pub async fn retake(self: &Arc<Self>, execution_id: &str) {
        let Some((lent, class)) = self.bound.lock().unwrap().get(execution_id).cloned() else {
            return;
        };
        if !lent.load(Ordering::SeqCst) {
            return;
        }
        if let (_, Err(rx)) = self.join(class, true) {
            // Its sender leaves the queue only by a grant: a retake never
            // fails.
            if let Ok(g) = rx.await {
                g.keep();
            }
        }
        lent.store(false, Ordering::SeqCst);
    }

    /// One slot freed: hand it on to the next waiter, or keep it free.
    fn give_back(self: &Arc<Self>) {
        let mut s = self.state.lock().unwrap();
        s.in_use = s.in_use.saturating_sub(1);
        self.hand_on(&mut s);
    }

    fn hand_on(self: &Arc<Self>, s: &mut State) {
        let now = Instant::now();
        while s.in_use < self.ceiling {
            let Some(w) = s.pop_next(now, self.step) else {
                // Nothing waits: the queue is empty, not stuck.
                s.moved = now;
                return;
            };
            s.in_use += 1;
            match w.tx.send(Grant(Some(self.clone()))) {
                Ok(()) => {
                    s.moved = now;
                    s.admitted += 1;
                }
                Err(mut g) => {
                    // Its waiter went away: the slot is still free. Disarm
                    // the token, which would take this lock again.
                    g.0 = None;
                    s.in_use -= 1;
                }
            }
        }
    }
}

impl TurnRunner {
    /// A slot and the kernel's turn of `exec_id`, waiting in `class`'s queue.
    /// With `wait` (an input's turn), it waits for another turn held on the
    /// same execution to end, and wakes the execution in the same frame when
    /// it has parked meanwhile (theseus-l6y); without it (continuations), a
    /// turn held there, or an execution not ready, gives up at once and the
    /// driver tries again. It fails only when the queue is stuck.
    ///
    /// `frames` counts the frames it writes (theseus-wz4y): each try is one
    /// call, with no `.await` in it, so this thread's count is its own.
    pub(super) async fn admit(
        &self,
        exec_id: &str,
        arrived: StdInstant,
        wait: bool,
        class: Class,
        frames: &mut u64,
    ) -> Result<Option<(TurnGuard, Slot)>> {
        let queue = &self.tools.admission;
        let mut front = false;
        loop {
            let mut slot = queue.take(class, front).await.map_err(|stuck| {
                turn_error(
                    "admission_stuck",
                    "",
                    "",
                    arrived.elapsed().as_millis() as u64,
                    anyhow::Error::new(stuck),
                )
            })?;
            // Registered before the look, so a turn's end between the look
            // and the wait is not missed.
            let ended = queue.ended.notified();
            tokio::pin!(ended);
            ended.as_mut().enable();
            let f0 = frames_written_here();
            let tried = if wait {
                self.kernel.admit_input_queued(exec_id, true)
            } else {
                self.kernel.admit_queued(exec_id)
            };
            *frames += frames_written_here() - f0;
            let e = match tried {
                Ok(g) => {
                    slot.bind(exec_id);
                    return Ok(Some((g, slot)));
                }
                Err(e) => e,
            };
            match e.downcast_ref::<KernelError>() {
                Some(KernelError::TurnHeld { .. })
                | Some(KernelError::NotRunnable {
                    state: "running", ..
                }) => {
                    // Its execution's turn is another's: the slot goes on,
                    // and this turn waits for a turn's end.
                    drop(slot);
                    if !wait {
                        return Ok(None);
                    }
                    let window = self.cfg.kernel.admission_stuck();
                    if tokio::time::timeout(window, ended).await.is_err() {
                        return Err(turn_error(
                            "admission_stuck",
                            "",
                            "",
                            arrived.elapsed().as_millis() as u64,
                            anyhow::anyhow!(
                                "admission is stuck: this execution's turn did not end in {} s",
                                window.as_secs()
                            ),
                        ));
                    }
                    front = true;
                }
                Some(KernelError::NotRunnable {
                    state: "waiting" | "blocked",
                    ..
                }) if wait => {
                    // `admit_input_queued` wakes it in the frame that admits;
                    // a park between the two takes the same slot again.
                    drop(slot);
                    front = true;
                }
                Some(KernelError::NotRunnable {
                    state: "waiting" | "blocked",
                    ..
                }) => return Ok(None),
                Some(KernelError::NotRunnable { state, .. }) => {
                    return Err(turn_error(
                        &format!("execution_{state}"),
                        "",
                        "",
                        arrived.elapsed().as_millis() as u64,
                        e,
                    ));
                }
                _ => return Err(e),
            }
        }
    }
}

#[cfg(test)]
#[path = "tests_admission.rs"]
mod tests;
