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
//!
//! The whole of the daemon's half is capped on wall time ([`DEADLINE`]), not
//! only on the day's spend: a run that passes it stops where it is and returns
//! what it has, and what it did not reach waits in the state for the next
//! night.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_protocol::import_sync::{ImportSyncRecord, ImportSyncResult, SyncPeople};
use theseus_protocol::method;

use super::server::{parse, Conn, RpcFailure};
use super::{import, Core};
use crate::import::people;
use crate::import::sync::STATE_KEY;

/// How long the daemon's half of one `theseus import sync` may run before it
/// stops where it is and returns what it has (theseus-ectzg).
///
/// The record was capped on the day's spend and on nothing else, so a people
/// step that ran long ran for as long as it liked while the client waited in
/// its one `await`. **The unit's `TimeoutStartSec=` bounds the CLIENT, not
/// this** (`scripts/user-service.sh`, 2 h): a client killed there leaves this
/// work running inside the daemon, which is the one thing that bound cannot
/// reach. **So this cap must stay strictly below the unit's**, or it can never
/// fire -- the client would be killed first and the daemon would carry on, and
/// the next night's run would begin beside a pass still going.
///
/// 90 minutes: above the slowest run ever observed to finish (about 85 min, on
/// 2026-11-11; the others on record took 5 to 24 min, and 2026-11-12's took
/// 7 min 25 s), and half an hour below the unit's 2 h -- room enough for this
/// to stop, return, and have the client print its summary before systemd
/// would kill it.
///
/// Passing it is **not** an error. The run stops, what it finished is already
/// written, and what it did not stays in the state's `people_todo` for the
/// next night: a cap that fires costs one night's remainder, and an uncapped
/// run costs every night after it.
const DEADLINE: Duration = Duration::from_secs(90 * 60);

/// One run's wall-clock cap: when it falls, and how long it was, for the line
/// the result carries.
#[derive(Clone, Copy)]
struct Cap {
    at: Instant,
    within: Duration,
}

impl Cap {
    fn new(within: Duration) -> Self {
        Self {
            at: Instant::now() + within,
            within,
        }
    }

    fn passed(&self) -> bool {
        Instant::now() >= self.at
    }

    /// Why a step stopped, in the words `stopped` already carries. The two
    /// reasons are told apart because they cost different things: a deadline
    /// leaves the rest for the next night, and a stopping daemon has no next
    /// night until it is started again.
    fn why(&self, core: &Core, left: &str) -> String {
        if core.outbox.stopping() {
            format!("the daemon is stopping, {left}")
        } else {
            // `Duration`'s own words (`5400s`, `1ms`): `as_secs()` reads
            // "0 s deadline" for any cap under a second, which is the shape
            // a test plants.
            format!("the run passed its {:?} deadline, {left}", self.within)
        }
    }
}

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
            .sync_record(rec, &by, DEADLINE)
            .await
            .map_err(RpcFailure::invalid)?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// One run's end, `within` of wall clock at most ([`DEADLINE`] in the
    /// daemon; a test plants its own).
    pub(super) async fn sync_record(
        self: &Arc<Self>,
        rec: ImportSyncRecord,
        by: &str,
        within: Duration,
    ) -> anyhow::Result<ImportSyncResult> {
        let t0 = Instant::now();
        let cap = Cap::new(within);
        let hour = self.runner.cfg.judge.learning_hour;
        let (mut state, todo) = self.sync_write(&rec, by).await?;
        let mut people: Option<SyncPeople> = None;
        if rec.error.is_none() && !rec.no_people && !todo.is_empty() {
            let p = self.sync_people(&rec, todo.clone(), by, cap).await?;
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
    ///
    /// `cap` is checked between tags and inside each tag's own frames (it is
    /// the other half of the `stopping` that writer already honours), and goes
    /// on to the extraction's session loop. A step that stops returns no
    /// session as done, so every one of `todo` waits for the next run.
    async fn sync_people(
        self: &Arc<Self>,
        rec: &ImportSyncRecord,
        todo: Vec<String>,
        by: &str,
        cap: Cap,
    ) -> anyhow::Result<(SyncPeople, Vec<String>)> {
        let mut out = SyncPeople::default();
        let mut tags: Vec<String> = rec.touched.iter().map(|t| t.tag.clone()).collect();
        tags.sort();
        tags.dedup();
        let of = tags.len();
        for (i, tag) in tags.into_iter().enumerate() {
            if cap.passed() {
                out.stopped = Some(cap.why(self, &format!("{} of {of} tags left", of - i)));
                return Ok((out, Vec::new()));
            }
            let (core, by, t) = (self.clone(), by.to_string(), tag.clone());
            let (r, stopped) = theseus_store::blocking(move || {
                let stopping = || core.outbox.stopping() || cap.passed();
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
            // The writer stops between frames, so a tag can be left part
            // written. It has always said so and was never asked, until now.
            if stopped {
                out.stopped = Some(cap.why(self, &format!("{} of {of} tags left", of - i - 1)));
                return Ok((out, Vec::new()));
            }
        }
        let s = self.people_sync_within(todo, Some(cap.at)).await?;
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
