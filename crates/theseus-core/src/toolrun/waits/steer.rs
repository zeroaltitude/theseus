//! A person's message queued behind a running turn (theseus-qu1lc): the turn
//! hears of it at once, its waits return, and the message joins the turn at
//! its next loop instead of running as a turn of its own once the turn ends.
//!
//! A running turn opens a [`Steer`] for its session (`Steers::open`, until it
//! ends). A person's input that finds the session's turn held offers itself
//! to it (`Steers::offer`) before it waits in the admission queue. An offer
//! is in memory only, like the queued request it stands for: nothing is
//! written until the turn takes it, and the turn writes the message's node in
//! the frame that takes it, so a crash leaves it either written once or not
//! at all.
//!
//! The signal is O(1) and polls nothing. [`Steer::pending`] is an atomic
//! load, `Steers::any` another over every session (a turn with nothing
//! queued anywhere pays that one at each loop boundary), and a wait lends its
//! thread to [`Watch::raised`], a `Notify`, never a timer.
//!
//! Three things end an offer. The turn takes it at a loop boundary
//! (`Steer::take`): the offerer then waits for the turn's result and returns
//! it as its own. The turn ends without having taken it (its last call made
//! no tool calls, or it failed): the offerer withdraws and queues as before.
//! The offerer goes away (its client left): the offer withdraws itself.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use theseus_protocol::{Attachment, TurnSubmitResult};
use tokio::sync::{oneshot, Notify};

/// A message's own: what its node is made of.
pub struct Steered {
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub author: String,
    /// The owner wrote it, from a private place (`Core::owner_input`): the
    /// only words the injection question is left out of.
    pub owner: bool,
    /// The code of the surface it was written from (`surface::code_of`).
    pub surface: Option<String>,
    /// The surface's message it answers (a Discord message id).
    pub reply_to: Option<String>,
    /// When it reached the daemon.
    pub arrived: Instant,
}

/// What the turn answers a message it took, once it ends: its result, or why
/// it has none.
pub type Joined = Result<TurnSubmitResult, String>;

struct Entry {
    ticket: u64,
    message: Steered,
    taken: oneshot::Sender<()>,
    done: oneshot::Sender<Joined>,
}

#[derive(Default)]
struct State {
    /// False once the turn has ended: no offer is accepted.
    closed: bool,
    next: u64,
    queue: VecDeque<Entry>,
    /// The messages the turn took, owed the turn's result.
    joined: Vec<oneshot::Sender<Joined>>,
}

/// One running turn's side of it.
pub struct Steer {
    state: Mutex<State>,
    /// Messages offered and not yet taken: the flag a wait or a loop boundary
    /// reads.
    count: AtomicUsize,
    /// Every session's, for the boundary's one load.
    total: Arc<AtomicUsize>,
    /// An offer arrived: waits return.
    raised: Notify,
    /// The turn ended: offers look again.
    closed: Notify,
}

impl Steer {
    /// Whether a message waits for the turn: one atomic load.
    pub fn pending(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }

    /// The messages offered so far, in arrival order, for the turn to write
    /// as its own. They are out of the queue now, so nobody else takes them;
    /// `joined` (or dropping the guards) settles what they were owed.
    pub fn take(&self) -> Taken {
        let mut s = self.state.lock().unwrap();
        let entries: Vec<Entry> = s.queue.drain(..).collect();
        self.count.fetch_sub(entries.len(), Ordering::AcqRel);
        self.total.fetch_sub(entries.len(), Ordering::AcqRel);
        Taken { entries }
    }
}

/// Messages taken out of the queue, not yet written.
pub struct Taken {
    entries: Vec<Entry>,
}

impl Taken {
    pub fn messages(&self) -> impl Iterator<Item = &Steered> {
        self.entries.iter().map(|e| &e.message)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// They were written: each offerer learns it, and is owed the turn's
    /// result. Dropped unwritten, an offerer is told nothing was taken and
    /// queues as it would have.
    pub fn written(self, steer: &Steer) {
        let mut s = steer.state.lock().unwrap();
        for e in self.entries {
            s.joined.push(e.done);
            let _ = e.taken.send(());
        }
    }
}

/// The open steers, by session.
#[derive(Default)]
pub struct Steers {
    open: Mutex<std::collections::HashMap<String, Arc<Steer>>>,
    total: Arc<AtomicUsize>,
    /// The messages each task took from the owner, until it ends
    /// (theseus-slmj1): its report says it was joined.
    joined: Mutex<std::collections::HashMap<String, u32>>,
    /// The tasks the owner holds (`task.pause`).
    pub pauses: super::pause::Pauses,
}

/// A running turn's hold on its steer: it closes when dropped.
pub struct Turning {
    steers: Arc<Steers>,
    session: String,
    steer: Option<Arc<Steer>>,
}

impl Steers {
    /// Whether any message waits for any turn: the loop boundary's one load.
    pub fn any(&self) -> bool {
        self.total.load(Ordering::Acquire) > 0
    }

    /// A turn of `session` begins, taking messages offered to it while `on`
    /// (`[kernel] steer_queued`).
    pub fn open(self: &Arc<Self>, session: &str, on: bool) -> Turning {
        let steer = on.then(|| {
            let steer = Arc::new(Steer {
                state: Mutex::default(),
                count: AtomicUsize::new(0),
                total: self.total.clone(),
                raised: Notify::new(),
                closed: Notify::new(),
            });
            self.open
                .lock()
                .unwrap()
                .insert(session.to_string(), steer.clone());
            steer
        });
        Turning {
            steers: self.clone(),
            session: session.to_string(),
            steer,
        }
    }

    /// The steer of `session`'s running turn, if it has one.
    pub fn get(&self, session: &str) -> Option<Arc<Steer>> {
        self.open.lock().unwrap().get(session).cloned()
    }

    /// `session`, a task, took `n` more messages from the owner.
    pub fn join(&self, session: &str, n: u32) {
        *self
            .joined
            .lock()
            .unwrap()
            .entry(session.to_string())
            .or_default() += n;
    }

    /// The messages the task `session` has taken from the owner.
    pub fn joined(&self, session: &str) -> u32 {
        self.joined
            .lock()
            .unwrap()
            .get(session)
            .copied()
            .unwrap_or(0)
    }

    /// The task ended and its count was written.
    pub fn unjoin(&self, session: &str) {
        self.joined.lock().unwrap().remove(session);
    }

    /// What a wait of `session`'s turn yields to.
    pub fn watch(&self, session: &str) -> Watch {
        Watch(self.get(session))
    }

    /// Offer a message to `session`'s running turn. `None`: no turn takes
    /// offers (none runs, it has ended, or the switch is off), and the
    /// message queues as it always did.
    pub fn offer(&self, session: &str, message: Steered) -> Option<Offer> {
        let steer = self.get(session)?;
        let (taken_tx, taken) = oneshot::channel();
        let (done_tx, done) = oneshot::channel();
        let ticket = {
            let mut s = steer.state.lock().unwrap();
            if s.closed {
                return None;
            }
            s.next += 1;
            let ticket = s.next;
            s.queue.push_back(Entry {
                ticket,
                message,
                taken: taken_tx,
                done: done_tx,
            });
            steer.count.fetch_add(1, Ordering::AcqRel);
            self.total.fetch_add(1, Ordering::AcqRel);
            ticket
        };
        steer.raised.notify_waiters();
        Some(Offer {
            ticket: Ticket { steer, ticket },
            taken,
            done,
        })
    }
}

impl Turning {
    /// The steer the turn reads at its loop boundaries; none when the switch
    /// is off.
    pub fn steer(&self) -> Option<&Arc<Steer>> {
        self.steer.as_ref()
    }

    /// The turn ended: the messages it took are answered with its result
    /// (`None`: it failed, which they are told), and no offer is accepted.
    pub fn finish(mut self, result: Option<&TurnSubmitResult>) {
        let Some(steer) = self.steer.take() else {
            return;
        };
        self.forget(&steer);
        let joined = {
            let mut s = steer.state.lock().unwrap();
            s.closed = true;
            std::mem::take(&mut s.joined)
        };
        for tx in joined {
            let _ = tx.send(match result {
                Some(r) => Ok(r.clone()),
                None => Err("the turn it joined failed".into()),
            });
        }
        steer.closed.notify_waiters();
    }

    fn forget(&self, steer: &Arc<Steer>) {
        let mut open = self.steers.open.lock().unwrap();
        if open
            .get(&self.session)
            .is_some_and(|s| Arc::ptr_eq(s, steer))
        {
            open.remove(&self.session);
        }
    }
}

impl Drop for Turning {
    fn drop(&mut self) {
        // Ended without `finish` (a dropped future): offers look again, and
        // the messages it took are told it is gone, as their senders drop.
        if let Some(steer) = self.steer.take() {
            self.forget(&steer);
            steer.state.lock().unwrap().closed = true;
            steer.closed.notify_waiters();
        }
    }
}

/// What a wait reads: whether a message waits for its turn, and a signal for
/// when one arrives. A wait outside a turn that takes offers never yields.
pub struct Watch(Option<Arc<Steer>>);

impl Watch {
    /// A message waits.
    pub fn pending(&self) -> bool {
        self.0.as_ref().is_some_and(|s| s.pending())
    }

    /// Resolves at the next message to arrive, whatever already waits; never,
    /// for a turn that takes none.
    pub async fn next(&self) {
        match &self.0 {
            Some(s) => s.raised.notified().await,
            None => std::future::pending().await,
        }
    }

    /// Resolves when a message waits (at once if one does); never, for a
    /// turn that takes none.
    pub async fn raised(&self) {
        let Some(s) = &self.0 else {
            return std::future::pending().await;
        };
        loop {
            let n = s.raised.notified();
            tokio::pin!(n);
            n.as_mut().enable();
            if s.pending() {
                return;
            }
            n.await;
        }
    }
}

/// How an offer ended.
pub enum Settled {
    /// The turn took the message and wrote it; its result, once it ends, is
    /// the answer.
    Taken(Answer),
    /// The turn ended without it: queue as before.
    Withdrawn,
}

/// The turn's answer to a message it took, to come.
pub struct Answer(oneshot::Receiver<Joined>);

impl Answer {
    /// Until the turn ends.
    pub async fn answer(self) -> Joined {
        match self.0.await {
            Ok(joined) => joined,
            Err(_) => Err("the turn it joined was lost".into()),
        }
    }
}

/// A ticket in a steer's queue: out of it when dropped, however the wait for
/// it ended. A ticket is the queue's once, so withdrawing twice is harmless.
struct Ticket {
    steer: Arc<Steer>,
    ticket: u64,
}

impl Ticket {
    /// Take the message back out of the queue: true when it was still there.
    fn withdraw(&self) -> bool {
        let mut s = self.steer.state.lock().unwrap();
        let Some(i) = s.queue.iter().position(|e| e.ticket == self.ticket) else {
            return false;
        };
        s.queue.remove(i);
        self.steer.count.fetch_sub(1, Ordering::AcqRel);
        self.steer.total.fetch_sub(1, Ordering::AcqRel);
        true
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        // Dropped unsettled (the client left): no turn writes a message
        // nobody waits for.
        self.withdraw();
    }
}

/// A message offered to a running turn.
pub struct Offer {
    ticket: Ticket,
    taken: oneshot::Receiver<()>,
    done: oneshot::Receiver<Joined>,
}

impl Offer {
    /// Until the turn takes the message and writes it, or ends without it.
    pub async fn settle(self) -> Settled {
        let Offer {
            ticket,
            mut taken,
            done,
        } = self;
        // The turn's end, though it came before this looked.
        let ended = async {
            loop {
                let n = ticket.steer.closed.notified();
                tokio::pin!(n);
                n.as_mut().enable();
                if ticket.steer.state.lock().unwrap().closed {
                    return;
                }
                n.await;
            }
        };
        tokio::pin!(ended);
        let was_taken = tokio::select! {
            got = &mut taken => got.is_ok(),
            () = &mut ended => match ticket.withdraw() {
                // Out of the queue by the turn already: it says whether it
                // wrote the message.
                false => taken.await.is_ok(),
                true => false,
            },
        };
        if !was_taken {
            return Settled::Withdrawn;
        }
        Settled::Taken(Answer(done))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn message(text: &str) -> Steered {
        Steered {
            text: text.into(),
            attachments: vec![],
            author: "test".into(),
            owner: false,
            surface: None,
            reply_to: None,
            arrived: Instant::now(),
        }
    }

    /// An offer raises the wait at once, is taken in arrival order, and its
    /// offerer is answered with the turn's result.
    #[tokio::test(start_paused = true)]
    async fn an_offer_raises_a_wait_and_is_taken_in_order() {
        let steers = Arc::new(Steers::default());
        let turning = steers.open("ses_a", true);
        let watch = steers.watch("ses_a");
        assert!(!watch.pending() && !steers.any());
        let waiting = tokio::spawn(async move {
            watch.raised().await;
        });
        tokio::task::yield_now().await;
        let t0 = tokio::time::Instant::now();
        let first = steers.offer("ses_a", message("one")).unwrap();
        let second = steers.offer("ses_a", message("two")).unwrap();
        waiting.await.unwrap();
        assert!(
            t0.elapsed() < Duration::from_millis(100),
            "returned at once"
        );
        assert!(steers.any());
        let steer = turning.steer().unwrap().clone();
        let taken = steer.take();
        let texts: Vec<&str> = taken.messages().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["one", "two"], "arrival order");
        assert!(!steer.pending() && !steers.any(), "taken: nothing waits");
        taken.written(&steer);
        let (a, b) = (tokio::spawn(first.settle()), tokio::spawn(second.settle()));
        tokio::task::yield_now().await;
        turning.finish(Some(&TurnSubmitResult::default()));
        for h in [a, b] {
            let Settled::Taken(answer) = h.await.unwrap() else {
                panic!("taken");
            };
            assert!(answer.answer().await.is_ok());
        }
    }

    /// A turn that ends with a message untaken lets its offerer queue as
    /// before, and takes no offer after.
    #[tokio::test(start_paused = true)]
    async fn a_turn_that_ends_untaken_withdraws_its_offers() {
        let steers = Arc::new(Steers::default());
        let turning = steers.open("ses_a", true);
        let offer = steers.offer("ses_a", message("late")).unwrap();
        let settling = tokio::spawn(offer.settle());
        tokio::task::yield_now().await;
        turning.finish(Some(&TurnSubmitResult::default()));
        assert!(matches!(settling.await.unwrap(), Settled::Withdrawn));
        assert!(!steers.any(), "withdrawn: nothing waits");
        assert!(steers.offer("ses_a", message("after")).is_none());
    }

    /// An offerer that goes away takes its message with it, and the switch
    /// off offers nothing.
    #[tokio::test(start_paused = true)]
    async fn a_dropped_offer_leaves_nothing_and_the_switch_off_takes_none() {
        let steers = Arc::new(Steers::default());
        let turning = steers.open("ses_a", true);
        drop(steers.offer("ses_a", message("gone")).unwrap());
        assert!(!steers.any() && turning.steer().unwrap().take().is_empty());
        drop(turning);
        let off = steers.open("ses_b", false);
        assert!(off.steer().is_none() && steers.offer("ses_b", message("no")).is_none());
        assert!(!steers.watch("ses_b").pending());
    }
}
