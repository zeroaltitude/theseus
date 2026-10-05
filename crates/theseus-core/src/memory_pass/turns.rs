//! The turns the pass writes between (theseus-ms5m; Eddie's decision 10,
//! 2026-10-04): a count of the turns running in the daemon, every
//! session's, and a handshake with the pass's frame, so that no pass frame
//! lands inside a turn.
//!
//! - A turn is counted from the start of `TurnRunner::run`, before its first
//!   frame, until the end of it, after its last ([`Turns::begin`], a
//!   [`Running`] guard).
//! - The pass writes each frame under a [`Writing`] guard
//!   ([`Turns::between`]), given once no turn is running and none has run for
//!   the pass's quiet stretch ([`super::QUIET`]). A turn that begins while the
//!   pass writes waits at its start for that one frame.
//! - The handshake: a turn counts itself, then looks for a writing pass; the
//!   pass marks itself writing, then looks for a running turn, each in that
//!   order and sequentially consistent. Whichever looks second sees the
//!   other: the pass steps back for the turn, or the turn waits out the frame.
//!   So no frame is written while a counted turn runs.
//! - **Writers are counted** (31b): the pass and consolidation each write
//!   through [`Turns::between`], so `writing` counts the frames being
//!   written; a turn waits until none is, and a writer that steps back for
//!   a turn uncounts only itself.
//! - The bounds, so a busy daemon cannot starve the pass: a frame that has
//!   waited [`super::QUIET_BOUND`] for a quiet stretch takes any moment with no
//!   turn running, however short; one that has waited [`super::BUSY_BOUND`],
//!   a daemon with a turn running all that while, is written beside the
//!   running turns, the one case in which a pass frame lands inside a turn.
//!   The clock starts at a batch's first frame, so a backlog is written at
//!   once when a bound comes.

use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::Notify;
use tokio::time::Instant;

use super::Timing;

/// The daemon's running turns, and whether the pass is writing.
#[derive(Default)]
pub struct Turns {
    running: AtomicUsize,
    /// The frames being written: the pass's, and consolidation's.
    writing: AtomicUsize,
    /// When the last turn ended.
    ended: Mutex<Option<Instant>>,
    /// A turn ended: a waiting pass looks again.
    turn_ended: Notify,
    /// The pass's frame is written, or the pass stepped back: a turn waiting
    /// at its start goes on.
    written: Notify,
}

/// A running turn, counted until it drops.
pub struct Running(Arc<Turns>);

/// The pass's frame: written while this lives.
pub struct Writing(Arc<Turns>);

impl Turns {
    /// A turn begins: counted until the guard drops. A pass frame being
    /// written at this moment is waited out first (one append).
    pub async fn begin(self: &Arc<Self>) -> Running {
        self.running.fetch_add(1, SeqCst);
        // Counted from here, so a turn dropped while it waits is uncounted.
        let running = Running(self.clone());
        while self.writing.load(SeqCst) > 0 {
            let written = self.written.notified();
            tokio::pin!(written);
            written.as_mut().enable();
            if self.writing.load(SeqCst) > 0 {
                written.await;
            }
        }
        running
    }

    /// The turns running now.
    pub fn running(&self) -> usize {
        self.running.load(SeqCst)
    }

    fn last_ended(&self) -> Option<Instant> {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A moment between turns for one frame of a batch whose first frame
    /// began to wait at `since`: no turn running and none for `t.quiet`;
    /// after `t.quiet_bound`, no turn running; after `t.busy_bound`, now.
    pub async fn between(self: &Arc<Self>, since: Instant, t: &Timing) -> Writing {
        let (soft, hard) = (since + t.quiet_bound, since + t.busy_bound);
        loop {
            let ended = self.turn_ended.notified();
            tokio::pin!(ended);
            ended.as_mut().enable();
            let now = Instant::now();
            if now >= hard {
                self.writing.fetch_add(1, SeqCst);
                let running = self.running();
                if running > 0 {
                    tracing::info!(
                        running,
                        waited_s = t.busy_bound.as_secs(),
                        "memory: turns ran all through the pass's bound; its frame is written beside them"
                    );
                }
                return Writing(self.clone());
            }
            if self.running() > 0 {
                // A turn runs: wait for one to end, or for the next bound.
                let bound = if now < soft { soft } else { hard };
                tokio::select! {
                    () = &mut ended => {}
                    () = tokio::time::sleep_until(bound) => {}
                }
                continue;
            }
            // None runs. Before the soft bound, the last must have ended a
            // quiet stretch ago.
            if now < soft {
                if let Some(quiet_at) = self.last_ended().map(|e| e + t.quiet) {
                    if now < quiet_at {
                        tokio::time::sleep_until(quiet_at.min(soft)).await;
                        continue;
                    }
                }
            }
            self.writing.fetch_add(1, SeqCst);
            if self.running() == 0 {
                return Writing(self.clone());
            }
            // A turn began meanwhile: step back for it.
            self.writing.fetch_sub(1, SeqCst);
            self.written.notify_waiters();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        *self.0.ended.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        self.0.running.fetch_sub(1, SeqCst);
        self.0.turn_ended.notify_waiters();
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        self.0.writing.fetch_sub(1, SeqCst);
        self.0.written.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn timing() -> Timing {
        Timing {
            quiet: Duration::from_millis(500),
            quiet_bound: Duration::from_secs(60),
            busy_bound: Duration::from_secs(600),
            ..Timing::default()
        }
    }

    /// Whether `f` has finished after the runtime ran everything it could.
    async fn done<T>(f: &tokio::task::JoinHandle<T>) -> bool {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        f.is_finished()
    }

    /// The pass's frame waits for the running turn to end and then for a
    /// quiet stretch after it, however long the turn: a quiet WAL inside a
    /// turn is not between turns.
    #[tokio::test(start_paused = true)]
    async fn a_frame_waits_for_the_turn_and_a_quiet_stretch_after_it() {
        let turns = Arc::new(Turns::default());
        let t = timing();
        let turn = turns.begin().await;
        let since = Instant::now();
        let pass = {
            let turns = turns.clone();
            tokio::spawn(async move {
                let _w = turns.between(since, &t).await;
                Instant::now()
            })
        };
        tokio::time::sleep(Duration::from_secs(30)).await;
        assert!(!done(&pass).await, "no frame while a turn runs");
        drop(turn);
        let ended = Instant::now();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!done(&pass).await, "nor within the quiet stretch after it");
        let at = pass.await.unwrap();
        assert_eq!(
            at - ended,
            t.quiet,
            "written a quiet stretch after the turn"
        );
    }

    /// A turn that begins while the pass writes waits for the frame, and the
    /// pass waits for a turn that runs.
    #[tokio::test(start_paused = true)]
    async fn a_turn_beginning_during_a_frame_waits_for_it() {
        let turns = Arc::new(Turns::default());
        let t = timing();
        let writing = turns.between(Instant::now(), &t).await;
        let turn = {
            let turns = turns.clone();
            tokio::spawn(async move { turns.begin().await })
        };
        assert!(!done(&turn).await, "the turn waits for the frame");
        assert_eq!(turns.running(), 1, "and is counted while it waits");
        drop(writing);
        let running = turn.await.unwrap();
        // While it runs, the pass waits.
        let pass = {
            let turns = turns.clone();
            let since = Instant::now();
            tokio::spawn(async move { drop(turns.between(since, &t).await) })
        };
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(!done(&pass).await);
        drop(running);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(done(&pass).await);
    }

    /// Two writers (the pass and consolidation, 31b): a turn waits until
    /// both frames are written, not only the first to end.
    #[tokio::test(start_paused = true)]
    async fn a_turn_waits_for_every_writer() {
        let turns = Arc::new(Turns::default());
        let t = timing();
        let pass = turns.between(Instant::now(), &t).await;
        let consolidation = turns.between(Instant::now(), &t).await;
        let turn = {
            let turns = turns.clone();
            tokio::spawn(async move { turns.begin().await })
        };
        assert!(!done(&turn).await);
        drop(pass);
        assert!(
            !done(&turn).await,
            "consolidation's frame is still being written"
        );
        drop(consolidation);
        assert!(done(&turn).await);
    }

    /// The bounds: past the quiet bound, any moment with no turn running
    /// (no quiet stretch); past the busy bound, beside a running turn.
    #[tokio::test(start_paused = true)]
    async fn the_bounds_end_the_wait() {
        let turns = Arc::new(Turns::default());
        let t = timing();
        let since = Instant::now();
        let pass = {
            let turns = turns.clone();
            tokio::spawn(async move {
                let _w = turns.between(since, &t).await;
                Instant::now()
            })
        };
        // Turns back to back, 100 ms apart: never a quiet stretch.
        while since.elapsed() < Duration::from_secs(61) {
            let turn = turns.begin().await;
            tokio::time::sleep(Duration::from_millis(400)).await;
            drop(turn);
            tokio::time::sleep(Duration::from_millis(100)).await;
            if pass.is_finished() {
                break;
            }
        }
        let at = pass.await.unwrap() - since;
        assert!(
            at >= t.quiet_bound && at < t.quiet_bound + Duration::from_secs(1),
            "past the quiet bound, the first gap: {at:?}"
        );

        // A turn that never ends: the frame goes beside it at the busy bound.
        let _turn = turns.begin().await;
        let since = Instant::now();
        let w = turns.between(since, &t).await;
        assert_eq!(since.elapsed(), t.busy_bound);
        drop(w);
    }
}
