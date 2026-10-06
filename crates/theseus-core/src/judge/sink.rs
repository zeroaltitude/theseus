//! The sink (design §2.5): every judgment the judge returns, written as a
//! `judge.call` ledger row keyed by its id and scoped `judge:<pack id>`, in
//! the sink's own frames: up to [`MAX_ROWS`] judgments a frame, or whatever
//! has landed within the window after the first. A turn never waits on it
//! and writes no frame for a shadow judgment. Its frames are written only
//! between turns (theseus-0j2.8), through the memory pass's writer handshake
//! (`memory_pass::turns`, as consolidation writes): the store has one
//! writer, so a sink frame mid-sync held a turn's next frame, and with turns
//! back to back a frame due 2 s after a judgment landed inside a later turn.
//! A judgment's row and its facts stay in one frame, said once it is
//! written, and a press finds a judgment not yet written in `pending`.
//! Each frame also carries the
//! breaker's moves (`judge.circuit`), the shed count (`judge.shed`, a
//! minute apart at most), and the shadow budget's record with what was
//! settled. A clean stop writes every judgment still queued before its
//! last checkpoint (`JudgeService::flush_sink`, theseus-ych4). A crash or a
//! SIGKILL loses the queue: on a quiet daemon a window of rows, on a busy
//! one the backlog waiting for a moment between turns. Their spend is not
//! lost, since the budget's blocks were written before the calls, or, for
//! the judgments a turn waits on, beside them (theseus-otny: a crash inside
//! that one sync can leave one block's calls unbooked, a cent at most).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use theseus_judge::{Judgment, JudgmentSink};
use theseus_store::NewRecord;
use tokio::sync::Notify;

use super::JudgeService;
use crate::fact::judge::{JudgeCall, JudgeCircuit, JudgeShed};
use crate::memory_pass::turns::Turns;
use crate::memory_pass::Timing;

/// Judgments a frame holds at most. A backlog is written as frames of this
/// size back to back, each waiting for its own moment between turns: a
/// turn that begins while a frame is appended waits for that one append,
/// so the frame's size bounds that wait (theseus-s1am).
pub const MAX_ROWS: usize = 32;

/// The judgments settled and not yet written: the recording's sink pushes
/// here, the writer takes its frames from the front, and a clean stop takes
/// what is left (theseus-ych4).
#[derive(Default)]
pub struct Queue {
    judgments: Mutex<VecDeque<Judgment>>,
    /// A judgment landed, or the sink is gone.
    landed: Notify,
    /// The sinks pushing here: the writer ends once none is left and the
    /// queue is empty (a sink built and dropped in a race to build the
    /// judge never ends it).
    senders: AtomicUsize,
    /// Held while a frame's judgments are taken and written, so the
    /// writer's frames and the stop's never interleave.
    writer: Mutex<()>,
    /// The stop has flushed: nothing more is written, since a row after
    /// the stop's last checkpoint would be replayed by the next start.
    closed: AtomicBool,
}

impl Queue {
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Judgment>> {
        self.judgments
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The judgments settled and not yet written.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    pub(crate) fn push(&self, j: Judgment) {
        self.lock().push_back(j);
        self.landed.notify_waiters();
    }

    /// Wait until `n` judgments are queued (true), or until `until`, or
    /// until the sink is gone with fewer (false).
    async fn wait_for(&self, n: usize, until: Option<tokio::time::Instant>) -> bool {
        loop {
            let landed = self.landed.notified();
            tokio::pin!(landed);
            landed.as_mut().enable();
            let len = self.len();
            if len >= n {
                return true;
            }
            if self.senders.load(SeqCst) == 0 {
                return false;
            }
            match until {
                Some(u) => tokio::select! {
                    () = &mut landed => {}
                    () = tokio::time::sleep_until(u) => return false,
                },
                None => landed.await,
            }
        }
    }

    /// Take up to `n` judgments from the front.
    fn take(&self, n: usize) -> Vec<Judgment> {
        let mut q = self.lock();
        let n = q.len().min(n);
        q.drain(..n).collect()
    }

    /// One frame from the front, unless the stop has flushed. Returns the
    /// judgments still queued.
    fn write_one(&self, svc: &JudgeService) -> usize {
        let _w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let batch = self.take(MAX_ROWS);
        if !batch.is_empty() {
            if self.closed.load(SeqCst) {
                tracing::debug!(
                    rows = batch.len(),
                    "judge: the stop's last checkpoint is written: judgments after it are dropped"
                );
            } else {
                svc.write(&batch);
            }
        }
        self.len()
    }

    /// The stop's flush: every judgment queued, in frames of [`MAX_ROWS`],
    /// and nothing written after. Returns the judgments written.
    fn flush(&self, svc: &JudgeService) -> usize {
        let _w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        self.closed.store(true, SeqCst);
        let mut written = 0;
        loop {
            let batch = self.take(MAX_ROWS);
            if batch.is_empty() {
                return written;
            }
            svc.write(&batch);
            written += batch.len();
        }
    }
}

/// The recording's sink: a push onto the queue. It never blocks.
pub struct Channel(Arc<Queue>);

impl Channel {
    pub fn new(queue: Arc<Queue>) -> Self {
        queue.senders.fetch_add(1, SeqCst);
        Self(queue)
    }
}

impl JudgmentSink for Channel {
    fn record(&self, judgment: &Judgment) {
        self.0.push(judgment.clone());
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.0.senders.fetch_sub(1, SeqCst);
        self.0.landed.notify_waiters();
    }
}

/// The writer: a frame at a time, off the runtime's workers, and between
/// turns. It ends when the service is gone, or the sink is.
///
/// **One clock a backlog** (theseus-s1am): a pass begins with a judgment
/// landing on an empty queue and lasts until the queue is empty again, and
/// its between-turns bounds run from the pass's start, not from each
/// frame's. So once a busy daemon passes the quiet bound, the backlog is
/// written in the next gaps between turns, frame after frame, where a clock
/// restarted at each frame wrote one frame a quiet bound.
pub async fn run(q: Arc<Queue>, svc: Weak<JudgeService>, every: Duration) {
    let mut pass: Option<tokio::time::Instant> = None;
    loop {
        if pass.is_none() {
            // A new pass: the first judgment, then the window for more.
            if !q.wait_for(1, None).await {
                return;
            }
            let until = tokio::time::Instant::now() + every;
            q.wait_for(MAX_ROWS, Some(until)).await;
        }
        let start = *pass.get_or_insert_with(tokio::time::Instant::now);
        // A moment between turns: no turn running, and none for the pass's
        // quiet stretch (its bounds end the wait on a busy daemon). The
        // turns are held, never the service, so a stop never waits on this.
        let (turns, timing) = match svc.upgrade() {
            Some(s) => (s.between.get().cloned(), s.sink_timing()),
            None => return,
        };
        let _writing = match turns {
            Some(t) => Some(t.between(since(start, &timing), &timing).await),
            None => None,
        };
        let Some(s) = svc.upgrade() else { return };
        let q2 = q.clone();
        let left = tokio::task::spawn_blocking(move || q2.write_one(&s))
            .await
            .unwrap_or(0);
        if left == 0 {
            pass = None;
        }
    }
}

/// The clock a frame's wait runs from: the pass's start, so the quiet bound
/// holds for the whole backlog; but never more than the quiet bound ago, so
/// a frame is written beside running turns only after `busy_bound -
/// quiet_bound` of them with no gap, however long the backlog has lasted.
fn since(start: tokio::time::Instant, t: &Timing) -> tokio::time::Instant {
    let now = tokio::time::Instant::now();
    now.checked_sub(t.quiet_bound)
        .map_or(start, |floor| start.max(floor))
}

impl JudgeService {
    /// The sink's bounds: the memory pass's.
    #[cfg(not(test))]
    #[expect(clippy::unused_self, reason = "a test's build reads its own bounds")]
    fn sink_timing(&self) -> Timing {
        Timing::default()
    }

    /// The sink's bounds: a test's shorter ones, else the memory pass's.
    #[cfg(test)]
    fn sink_timing(&self) -> Timing {
        self.sink_timing.get().copied().unwrap_or_default()
    }

    /// A clean stop's flush (theseus-ych4): every judgment settled and not
    /// yet written goes in as few frames as it takes, before the stop's last
    /// checkpoint, and nothing is written after. Returns how many.
    pub fn flush_sink(&self) -> usize {
        self.queue.flush(self)
    }

    /// The judgments settled and not yet written.
    pub fn unwritten(&self) -> usize {
        self.queue.len()
    }

    /// A test's judgment, settled as the recording settles one.
    #[cfg(test)]
    pub(crate) fn settle(&self, j: &Judgment) {
        self.queue.push(j.clone());
    }
}

impl JudgeService {
    /// The daemon's running turns, which the sink's frames wait out (the
    /// core's, as it builds). A service never given them writes at once.
    pub fn write_between(&self, turns: Arc<Turns>) {
        let _ = self.between.set(turns);
    }
}

impl JudgeService {
    /// One frame: the batch's rows, the breaker's moves, the shed count, and
    /// the budget's record. Once it is written, each fact's sentences are
    /// said and each judgment's metrics recorded (23b). The states a turn's
    /// judgments left staged are written first, so no row names a blob the
    /// disk may yet lose (theseus-otny).
    fn write(&self, batch: &[Judgment]) {
        self.write_staged_blobs(batch);
        let mut records: Vec<NewRecord> = Vec::new();
        for j in batch {
            let (session, turn) = where_of(j);
            let scope = format!("judge:{}", j.pack.split('.').next().unwrap_or(&j.pack));
            match crate::fact::row(&call(j), session, turn) {
                Ok(mut r) => {
                    r.key = Some(j.id.clone());
                    records.push(r.scoped(&scope));
                }
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "judge: a judgment's row was not built")
                }
            }
            if let Some(t) = &j.circuit {
                let circuit = JudgeCircuit {
                    transition: t,
                    judgment: &j.id,
                    breaker: self.breaker_of(&j.pack),
                };
                records.extend(crate::fact::row(&circuit, None, None).ok());
            }
        }
        let shed = self
            .built
            .get()
            .and_then(|b| b.judge.inner().client().take_shed_report(Instant::now()));
        if let Some(shed) = shed {
            records.extend(crate::fact::row(&JudgeShed { shed }, None, None).ok());
        }
        records.extend(self.budget.record());
        let written = self.store.append(&records);
        // Written (or lost with their frame): a press reads them from the
        // store from now on.
        self.pending_remove(&batch.iter().map(|j| j.id.clone()).collect::<Vec<_>>());
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), rows = batch.len(), "judge: the judgments' frame was not written");
            return;
        }
        for j in batch {
            let (session, turn) = where_of(j);
            self.announce(session, turn, &call(j));
            if let Some(t) = &j.circuit {
                let circuit = JudgeCircuit {
                    transition: t,
                    judgment: &j.id,
                    breaker: self.breaker_of(&j.pack),
                };
                self.announce(None, None, &circuit);
                self.count_opened(j, t);
            }
            if let Some(t) = self.telemetry.get() {
                t.record_judgment(j, crate::fact::judge::disagrees(j));
            }
        }
        if let Some(shed) = shed {
            self.announce(None, None, &JudgeShed { shed });
        }
    }
}

impl JudgeService {
    /// The breaker of its own `pack` answers to (`rerank`); `None`, the
    /// shared one.
    fn breaker_of(&self, pack: &str) -> Option<&str> {
        self.built.get()?.judge.inner().breaker_of(pack)
    }

    /// rerank's own breaker opening counts toward its day brake on the
    /// ladder (26a: twice in a local day).
    fn count_opened(&self, j: &Judgment, t: &theseus_judge::breaker::Transition) {
        if matches!(t, theseus_judge::breaker::Transition::Opened { .. })
            && self.breaker_of(&j.pack) == Some(super::rerank::BREAKER)
        {
            let day = self.today();
            self.land(
                &j.pack,
                theseus_judge::learn::CanaryEvent::BreakerOpened { day },
            );
        }
    }
}

/// A judgment's fact: in 23b every judgment the sink writes is a shadow
/// one, paid by the judge's own budget.
fn call(j: &Judgment) -> JudgeCall<'_> {
    JudgeCall {
        judgment: j,
        budget: "shadow",
    }
}

/// Whose a judgment is: the session and turn its context names.
fn where_of(j: &Judgment) -> (Option<&str>, Option<&str>) {
    let s = |k: &str| j.context.get(k).and_then(|v| v.as_str());
    (s("session"), s("turn"))
}
