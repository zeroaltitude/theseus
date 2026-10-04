//! The jobs turns wait on (Tier 7.1): a synchronous `proc.run` hears of its
//! job's end as an event, not by a look every 50 ms.
//!
//! A turn waits on its job from before the launch (`JobWaits::wait`). The
//! drain that finds the job's completion in the spool leaves it to that turn
//! and wakes it (`wake`): the turn takes it and writes it with its result's
//! node in one frame. A job run in this process (tests) wakes its turn
//! itself (`JobDone`). A stop or a cancel wakes the turn of each job it
//! stopped, and the reconciler every waiting turn, to look again; a look each
//! `LOOK` is the backstop for a word that never comes. Once the turn stops
//! waiting (its result, or its bound: the job goes on in the background), the
//! drain takes the job's completion as it takes any other.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

/// The backstop: a waiting turn looks at its job at least this often.
pub const LOOK: Duration = Duration::from_secs(1);

/// The jobs turns wait on, each with its turn's wake, and how many times a
/// turn has looked at its job: about twice a job, where a look every 50 ms
/// was 1,200 a minute.
#[derive(Default)]
pub struct JobWaits {
    waiting: Mutex<HashMap<String, Arc<Notify>>>,
    looks: AtomicU64,
}

impl JobWaits {
    /// The looks turns have taken at their jobs since the daemon started.
    pub fn looks(&self) -> u64 {
        self.looks.load(Ordering::Relaxed)
    }

    /// A turn looks at its job.
    pub(super) fn looked(&self) {
        self.looks.fetch_add(1, Ordering::Relaxed);
    }

    /// A turn waits on job `id` until the returned wait drops.
    pub fn wait(self: &Arc<Self>, id: &str) -> Waiting {
        let notify = Arc::new(Notify::new());
        self.waiting
            .lock()
            .unwrap()
            .insert(id.to_string(), notify.clone());
        Waiting {
            waits: self.clone(),
            id: id.to_string(),
            notify,
        }
    }

    /// Wake the turn that waits on job `id`: whether one does, which makes
    /// the job's completion that turn's to take.
    pub fn wake(&self, id: &str) -> bool {
        match self.waiting.lock().unwrap().get(id) {
            Some(n) => {
                n.notify_one();
                true
            }
            None => false,
        }
    }

    /// Wake every waiting turn to look at its job again.
    pub fn wake_all(&self) {
        for n in self.waiting.lock().unwrap().values() {
            n.notify_one();
        }
    }
}

/// A turn's wait on its job, until it drops.
pub struct Waiting {
    waits: Arc<JobWaits>,
    id: String,
    notify: Arc<Notify>,
}

impl Waiting {
    /// Until a wake, or `at_most`. A wake that came while the turn was
    /// looking is kept, and ends this one at once.
    pub async fn woken(&self, at_most: Duration) {
        let _ = tokio::time::timeout(at_most, self.notify.notified()).await;
    }

    /// What a launcher that runs the job in this process wakes this wait with.
    pub fn done(&self) -> JobDone {
        JobDone {
            waits: self.waits.clone(),
            id: self.id.clone(),
        }
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let mut waiting = self.waits.waiting.lock().unwrap();
        if waiting
            .get(&self.id)
            .is_some_and(|n| Arc::ptr_eq(n, &self.notify))
        {
            waiting.remove(&self.id);
        }
    }
}

/// How a job run in this process wakes its turn once its completion is
/// spooled: no drain hears of it. The real wrapper's report reaches the turn
/// through the notify socket and the drain instead.
#[derive(Clone)]
pub struct JobDone {
    waits: Arc<JobWaits>,
    id: String,
}

impl JobDone {
    pub fn wake(&self) {
        self.waits.wake(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wake reaches the turn waiting on its job, even one that came before
    /// the turn began to wait, and none other; once the wait drops, the job
    /// is nobody's, so the drain takes its completion.
    #[tokio::test]
    async fn a_wake_reaches_the_turn_waiting_on_its_job_and_no_other() {
        let waits = Arc::new(JobWaits::default());
        let (a, b) = (waits.wait("act_a"), waits.wait("act_b"));
        assert!(waits.wake("act_a"), "a turn waits on act_a");
        let t0 = std::time::Instant::now();
        a.woken(Duration::from_secs(5)).await;
        assert!(t0.elapsed() < Duration::from_secs(1), "the kept wake");
        let t0 = std::time::Instant::now();
        b.woken(Duration::from_millis(30)).await;
        assert!(
            t0.elapsed() >= Duration::from_millis(25),
            "act_b's turn slept"
        );
        a.done().wake();
        a.woken(Duration::from_secs(5)).await;
        drop(a);
        assert!(!waits.wake("act_a"), "nobody waits on act_a any more");
        waits.wake_all();
        let t0 = std::time::Instant::now();
        b.woken(Duration::from_secs(5)).await;
        assert!(t0.elapsed() < Duration::from_secs(1), "woken with the rest");
    }
}
