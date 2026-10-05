//! `judge.prove` (M5 L3, roadmap row 50; design §2.9, "The prove"): the
//! exit report for `loop.v1`'s canary. The records are
//! `learning::prove`'s, built from the ledger on the blocking pool; the
//! report is `theseus_judge::prove`'s, the same generator `theseus-judge
//! prove` runs over a records file. A read: it writes nothing, so it is
//! anyone's to ask, inside a job too.

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::Value;
use theseus_judge::prove::{markdown, prove, ProveMinimum};
use theseus_protocol::error_code;
use theseus_protocol::judge_runs::{JudgeProveParams, JudgeProveResult};
use theseus_protocol::packs::PackModeRow;
use theseus_protocol::LedgerKind;

use super::server::RpcFailure;
use super::Core;
use crate::judge::LOOP_PACK;
use crate::learning::{backfill::day_start, local_midnight, prove as records};
use crate::ledger::LedgerRow;

/// What the records cannot say yet.
const NUDGES: &str = "nudges and unnecessary nudges are 0: the canary's nudge is step 26b's, and \
                      nothing records one yet";

impl Core {
    /// The learning ledger's two reads from one arm of `dispatch`, which
    /// stays within clippy's length that way: the report, and the prove.
    pub(super) async fn rpc_ledger(
        self: &std::sync::Arc<Self>,
        name: &str,
        params: Value,
    ) -> Result<Value, RpcFailure> {
        let v = match name {
            theseus_protocol::method::JUDGE_PROVE => {
                serde_json::to_value(self.judge_prove(params).await?)
            }
            _ => serde_json::to_value(self.learning_report(params).await?),
        };
        v.map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))
    }

    /// `judge.prove`: the records of the tasks that ended in the window, and
    /// the generator's report over them.
    pub(crate) async fn judge_prove(
        self: &std::sync::Arc<Self>,
        params: Value,
    ) -> Result<JudgeProveResult, RpcFailure> {
        // Its params are optional: none reads the default window.
        let p: JudgeProveParams = match params {
            Value::Null => JudgeProveParams::default(),
            v => serde_json::from_value(v).map_err(|e| RpcFailure::invalid(e.into()))?,
        };
        let core = self.clone();
        tokio::task::spawn_blocking(move || core.prove_now(&p))
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
            .map_err(RpcFailure::invalid)
    }

    /// The window: the days given, else since `loop.v1`'s latest move to
    /// canary (its `pack.mode` row), else everything.
    fn prove_window(
        &self,
        p: &JudgeProveParams,
    ) -> anyhow::Result<(Option<u64>, Option<u64>, String)> {
        let until = match p.until.as_deref() {
            // The day whole: up to the next local midnight.
            Some(d) => Some(
                local_midnight(
                    day_start(d).map_err(|_| {
                        anyhow::anyhow!("--until is a local day such as 2026-10-04, not {d:?}")
                    })? + 36 * 3_600_000,
                ) - 1,
            ),
            None => None,
        };
        if let Some(d) = p.since.as_deref() {
            let since = day_start(d)?;
            return Ok((
                Some(since),
                until,
                format!("tasks that ended since {}", d.trim()),
            ));
        }
        // The rows read from their scope, not through the ladder, whose
        // first load can write the adoption table's rows.
        let mut moved = None;
        for r in self
            .store
            .scope_after(&crate::judge::ladder::scope(LOOP_PACK), 0)?
        {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.kind != LedgerKind::PackMode.as_str() {
                continue;
            }
            if let Ok(m) = serde_json::from_value::<PackModeRow>(row.data) {
                if m.pack == LOOP_PACK && m.mode == "canary" && !m.declined {
                    moved = Some((row.at_unix_ms, m));
                }
            }
        }
        Ok(match moved {
            Some((at, r)) => (
                Some(at),
                until,
                format!(
                    "tasks that ended since {LOOP_PACK}'s move to {} on {}",
                    crate::fact::ladder::mode_words(&r.mode, r.share),
                    crate::judge::spend::local_day(at)
                ),
            ),
            None => (
                None,
                until,
                format!("every task that ended: {LOOP_PACK} has not moved to canary"),
            ),
        })
    }

    fn prove_now(&self, p: &JudgeProveParams) -> anyhow::Result<JudgeProveResult> {
        let began = Instant::now();
        let (since_ms, until_ms, window) = self.prove_window(p)?;
        let (input, unreadable) = self.prove_input(since_ms, until_ms)?;
        let mut built = records::build(&input);
        if unreadable > 0 {
            built
                .left_out
                .insert(records::UNREADABLE.to_string(), unreadable);
        }
        let mut min = ProveMinimum::default();
        if let Some(n) = p.min_tasks {
            min.tasks_per_arm = n as usize;
        }
        if let Some(n) = p.min_labeled {
            min.labeled_per_metric = n as usize;
        }
        let report = prove(&built.records, min);
        let mut arms = BTreeMap::from([("canary".to_string(), 0), ("control".to_string(), 0)]);
        for r in &built.records {
            *arms.entry(r.arm.as_str().to_string()).or_insert(0) += 1;
        }
        Ok(JudgeProveResult {
            pack: LOOP_PACK.into(),
            since_ms,
            until_ms,
            window,
            verdict: report.verdict.kind.as_str().into(),
            markdown: markdown(&report),
            report: serde_json::to_value(&report)?,
            tasks: u32::try_from(input.tasks.len()).unwrap_or(u32::MAX) + unreadable,
            arms,
            left_out: built.left_out,
            notes: vec![NUDGES.into()],
            records: p.records.then(|| records::jsonl(&built.records)),
            elapsed_ms: began.elapsed().as_millis() as u64,
        })
    }
}
