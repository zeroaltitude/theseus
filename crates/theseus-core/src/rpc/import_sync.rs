//! `import.sync.plan` and `import.sync.record` (theseus-pjmj7; `crate::import::sync`):
//! the daemon's half of `theseus import sync`. Both are the owner's acts, from a
//! private place (`judge_act(Act::Import)`), and the CLI refuses them inside a job
//! (`OPERATORS`).
//!
//! The record is three steps and two frames, so a stop between them loses nothing
//! the next run needs: the state and the run's row first (the cursor moves, the
//! sessions to read for people are listed); then the touched tags' people from the
//! stored records (`import.people`, no model call) and the people extraction over
//! the new messages (`judge::people::sync`, under the sweep's cap); then the state
//! again with the step's spend and what it left. A run that failed, or asked for no
//! people step, stops after the first.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::import_sync::{ImportSyncRecord, ImportSyncResult, SyncPeople};
use theseus_protocol::method;

use super::server::{parse, Conn, RpcFailure};
use super::{import, Core};
use crate::import::people;
use crate::import::sync::STATE_KEY;

impl Core {
    pub(super) async fn import_sync(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name == method::IMPORT_SYNC_PLAN {
            self.import_judged(conn, name_of(name), "the OpenClaw sync's plan")?;
            let core = self.clone();
            let plan = import::blocking(move || core.sync_plan()).await?;
            return serde_json::to_value(plan).map_err(|e| RpcFailure::invalid(e.into()));
        }
        let rec: ImportSyncRecord = parse(params)?;
        let by = self.import_judged(conn, name_of(name), "the OpenClaw sync's record")?;
        let r = self
            .sync_record(rec, &by)
            .await
            .map_err(RpcFailure::invalid)?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// One run's end.
    async fn sync_record(
        self: &Arc<Self>,
        rec: ImportSyncRecord,
        by: &str,
    ) -> anyhow::Result<ImportSyncResult> {
        let t0 = std::time::Instant::now();
        let hour = self.runner.cfg.judge.learning_hour;
        let (mut state, todo) = self.sync_write(&rec, by).await?;
        let mut people: Option<SyncPeople> = None;
        if rec.error.is_none() && !rec.no_people && !todo.is_empty() {
            let p = self.sync_people(&rec, todo.clone(), by).await?;
            let done: std::collections::HashSet<&String> = p.1.iter().collect();
            state.people_todo = todo.into_iter().filter(|s| !done.contains(s)).collect();
            state.people_usd = p.0.spent_usd;
            let mut out = p.0;
            out.left = state.people_todo.len() as u64;
            self.people_write(&[crate::judge::people::run::meta(STATE_KEY, &state)?])
                .await?;
            people = Some(out);
        }
        let now = theseus_protocol::now_unix_ms();
        Ok(ImportSyncResult {
            sync: state.health(now, hour),
            people,
            ms: t0.elapsed().as_secs_f64() * 1e3,
        })
    }

    /// The people steps: the touched tags' handles, then the extraction over
    /// `todo`. The spend and the sessions that need no more come back.
    async fn sync_people(
        self: &Arc<Self>,
        rec: &ImportSyncRecord,
        todo: Vec<String>,
        by: &str,
    ) -> anyhow::Result<(SyncPeople, Vec<String>)> {
        let mut out = SyncPeople::default();
        let mut tags: Vec<String> = rec.touched.iter().map(|t| t.tag.clone()).collect();
        tags.sort();
        tags.dedup();
        for tag in tags {
            let (core, by, t) = (self.clone(), by.to_string(), tag.clone());
            let (r, _) = theseus_store::blocking(move || {
                let stopping = || core.outbox.stopping();
                let o = core.runner.ontology.snapshot(&core.store)?;
                let not = core.not_people(&[], &o);
                people::assign_unless(
                    &core.store,
                    &core.runner.ontology,
                    (&t, &by),
                    false,
                    &not,
                    stopping,
                )
            })?;
            out.declared += r.made;
            out.joined += r.joined;
            out.tags.push(tag);
        }
        let s = self.people_sync(todo).await?;
        out.swept = s.swept;
        out.candidates = s.candidates;
        out.judged = s.judged;
        out.spent_usd = theseus_kernel::micros_to_usd(s.spent);
        out.stopped = s.stopped;
        Ok((out, s.done))
    }
}

/// The method's static name, for the act's record.
fn name_of(name: &str) -> &'static str {
    match name {
        method::IMPORT_SYNC_PLAN => method::IMPORT_SYNC_PLAN,
        _ => method::IMPORT_SYNC_RECORD,
    }
}
