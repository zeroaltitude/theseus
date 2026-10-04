//! The sink (design §2.5): every judgment the judge returns, written as a
//! `judge.call` ledger row keyed by its id and scoped `judge:<pack id>`, in
//! the sink's own frames: up to [`MAX_ROWS`] judgments a frame, or whatever
//! has landed within the window after the first. A turn never waits on it
//! and writes no frame for a shadow judgment. Each frame also carries the
//! breaker's moves (`judge.circuit`), the shed count (`judge.shed`, a
//! minute apart at most), and the shadow budget's record with what was
//! settled. A crash loses at most a window of rows; their spend is not
//! lost, since the budget's blocks were written before the calls.

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
    /// the budget's record.
    fn write(&self, batch: &[Judgment]) {
        let mut records: Vec<NewRecord> = Vec::new();
        for j in batch {
            let session = j.context.get("session").and_then(|v| v.as_str());
            let turn = j.context.get("turn").and_then(|v| v.as_str());
            let scope = format!("judge:{}", j.pack.split('.').next().unwrap_or(&j.pack));
            match crate::fact::row(
                &JudgeCall {
                    judgment: j,
                    budget: "shadow",
                },
                session,
                turn,
            ) {
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
                };
                records.extend(crate::fact::row(&circuit, None, None).ok());
            }
        }
        if let Some(b) = self.built.get() {
            if let Some(shed) = b.judge.inner().client().take_shed_report(Instant::now()) {
                records.extend(crate::fact::row(&JudgeShed { shed }, None, None).ok());
            }
        }
        records.extend(self.budget.record());
        if let Err(e) = self.store.append(&records) {
            tracing::warn!(error = %format!("{e:#}"), rows = batch.len(), "judge: the judgments' frame was not written");
        }
        // Written (or lost with their frame): a press reads them from the
        // store from now on.
        self.pending_remove(&batch.iter().map(|j| j.id.clone()).collect::<Vec<_>>());
    }
}
