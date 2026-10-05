//! The sink (design §2.5): every judgment the judge returns, written as a
//! `judge.call` ledger row keyed by its id and scoped `judge:<pack id>`, in
//! the sink's own frames: up to [`MAX_ROWS`] judgments a frame, or whatever
//! has landed within the window after the first. A turn never waits on it
//! and writes no frame for a shadow judgment. Each frame also carries the
//! breaker's moves (`judge.circuit`), the shed count (`judge.shed`, a
//! minute apart at most), and the shadow budget's record with what was
//! settled. A crash loses at most a window of rows; their spend is not
//! lost, since the budget's blocks were written before the calls, or, for
//! the judgments a turn waits on, beside them (theseus-otny: a crash inside
//! that one sync can leave one block's calls unbooked, a cent at most).

use std::sync::Weak;
use std::time::{Duration, Instant};

use theseus_judge::{Judgment, JudgmentSink};
use theseus_store::NewRecord;
use tokio::sync::mpsc;

use super::JudgeService;
use crate::fact::judge::{JudgeCall, JudgeCircuit, JudgeShed};

/// Judgments a frame holds at most.
pub const MAX_ROWS: usize = 32;

/// The recording's sink: a channel to the writer's task. It never blocks.
pub struct Channel(mpsc::UnboundedSender<Judgment>);

impl Channel {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Judgment>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self(tx), rx)
    }
}

impl JudgmentSink for Channel {
    fn record(&self, judgment: &Judgment) {
        let _ = self.0.send(judgment.clone());
    }
}

/// The writer: a frame per batch, off the runtime's workers. It ends when
/// the service is gone, or every sender is.
pub async fn run(
    mut rx: mpsc::UnboundedReceiver<Judgment>,
    svc: Weak<JudgeService>,
    every: Duration,
) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        let until = tokio::time::Instant::now() + every;
        while batch.len() < MAX_ROWS {
            match tokio::time::timeout_at(until, rx.recv()).await {
                Ok(Some(j)) => batch.push(j),
                Ok(None) | Err(_) => break,
            }
        }
        let Some(s) = svc.upgrade() else { return };
        let _ = tokio::task::spawn_blocking(move || s.write(&batch)).await;
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
