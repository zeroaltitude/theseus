//! Claim leases (M7 step 39b, theseus-ext.14; §2.4 "Claim leases"):
//! `task.claim { id, version }` sets `claim { by: exe_…, until_ms }`,
//! `[kernel] task_lease_minutes` ahead.
//!
//! - **A claim is an edit of the plan** (layer 2): it names the version it
//!   read and moves it by one. The holder claiming again renews it, and so
//!   does each of the holder's edits, in the frame the edit writes; a renewal
//!   alone never moves the version.
//! - **A held task refuses another's claim** whatever version it names:
//!   `blocked: claimed by session <short> until <HH:MM>`, the call's result,
//!   never retried in silence (§3.2a). A claim does not hold back another
//!   session's edits: compare-and-swap guards those.
//! - **A close ends it**, and a task session's report closes its record
//!   without one.
//! - **An expired lease frees the task** in the due pass, as wakes are
//!   found (`Core::free_expired_leases_if_due`, on the driver's tick): from
//!   the claims kept in memory (`Leases`), built once after serving, never a
//!   scan of every record per tick. Its record and its `task.lease_expired`
//!   row are one frame. Before that pass, a claim past its `until_ms` reads
//!   free (`claim_at`, `shown`): the claim check and every surface ask the
//!   time, not the pass.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::tools::{cas, change, locked, Done};
use super::{TaskClaim, TaskRecord};
use crate::toolrun::TurnCtx;
use crate::Core;

pub const CLAIM: &str = "task.claim";

/// The claims that hold, in memory: each claimed task's id and when its
/// lease lapses. None until it is built, after serving.
pub struct Leases {
    lease_ms: u64,
    held: Mutex<Option<BTreeMap<String, u64>>>,
}

impl Leases {
    pub fn new(lease_ms: u64) -> Self {
        Self {
            lease_ms: lease_ms.max(60_000),
            held: Mutex::new(None),
        }
    }

    /// A lease's length.
    pub fn lease_ms(&self) -> u64 {
        self.lease_ms
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, Option<BTreeMap<String, u64>>> {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A task as its frame left it: its claim kept, or gone. Before the
    /// claims are built there is nothing to keep: the build reads the
    /// record, which holds it.
    pub fn note(&self, t: &TaskRecord) {
        if let Some(m) = self.guard().as_mut() {
            match &t.claim {
                Some(c) if !t.state.is_closed() => {
                    m.insert(t.id.clone(), c.until_ms);
                }
                _ => {
                    m.remove(&t.id);
                }
            }
        }
    }

    /// Build the claims from the records, once (after serving). The lock is
    /// held across the read, so a claim whose frame lands meanwhile is noted
    /// after it, never lost.
    pub fn build(&self, store: &crate::store::Store) -> anyhow::Result<bool> {
        theseus_store::blocking(|| {
            let mut g = self.guard();
            if g.is_some() {
                return Ok(false);
            }
            let mut m = BTreeMap::new();
            for t in super::all(store)? {
                if let (Some(c), false) = (&t.claim, t.state.is_closed()) {
                    m.insert(t.id.clone(), c.until_ms);
                }
            }
            *g = Some(m);
            Ok(true)
        })
    }

    /// The tasks whose lease lapsed by `now_ms`; None before the build.
    pub fn due(&self, now_ms: u64) -> Option<Vec<String>> {
        let g = self.guard();
        let m = g.as_ref()?;
        Some(
            m.iter()
                .filter(|(_, until)| **until <= now_ms)
                .map(|(id, _)| id.clone())
                .collect(),
        )
    }

    /// How many claims it keeps (None before the build).
    pub fn held(&self) -> Option<usize> {
        self.guard().as_ref().map(BTreeMap::len)
    }
}

/// `blocked: claimed by session a1b2c3 until 14:05`.
pub fn blocked(c: &TaskClaim) -> String {
    format!(
        "blocked: claimed by session {} until {}",
        c.session_short(),
        crate::push::hm(c.until_ms)
    )
}

/// The holder's edit renews its claim (the frame the edit writes carries
/// it): true when it did.
pub fn renew(rec: &mut TaskRecord, tc: &TurnCtx<'_>, lease_ms: u64) -> bool {
    let now = tc.kernel.now_ms();
    match &mut rec.claim {
        Some(c) if c.by == tc.execution_id && c.holds_at(now) => {
            c.until_ms = now + lease_ms;
            true
        }
        _ => false,
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimIn {
    id: String,
    version: u64,
}

/// `task.claim`.
pub struct TaskClaimTool;

impl Tool for TaskClaimTool {
    fn name(&self) -> &'static str {
        CLAIM
    }

    fn description(&self) -> &'static str {
        "Claim a task in the task graph for this conversation: a lease (30 minutes unless the \
         operator set another) that tells other sessions it is yours. Your own edits of the task \
         renew it, claiming it again renews it, and closing it ends it; past its time it is \
         free. Name the `version` you read. A task another session holds is refused, \
         `blocked: claimed by session … until …`: do not retry it in silence, work on another \
         or say so."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The task's id (tsk_…), as the task graph shows it."},
                "version": {"type": "integer", "minimum": 1, "description": "The version you read (v7 in the task graph)."}
            },
            "required": ["id", "version"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // The record and the call's result are one frame.
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: ClaimIn = parse(input)?;
        Ok(Plan {
            summary: format!("claim task {} (v{})", i.id, i.version),
            ..Default::default()
        })
    }
}

/// `task.claim`'s run: a held task refuses before its version is compared;
/// the holder's claim renews without moving the version.
pub fn claim<'a>(tc: &TurnCtx<'a>, input: &Value, lease_ms: u64) -> Result<Done<'a>, String> {
    let i: ClaimIn = parse(input)?;
    let mut done = Done::default();
    let (lock, mut rec) = locked(tc, &i.id)?;
    done.locks.push(lock);
    if rec.state.is_closed() {
        return Err(format!(
            "Refused: task {} is {}; a closed task takes no claim",
            rec.id,
            rec.state.as_str()
        ));
    }
    let now = tc.kernel.now_ms();
    if let Some(c) = rec.claim_at(now).filter(|c| c.by != tc.execution_id) {
        return Err(format!(
            "{}. Task {} is another session's for now: work on another task, or wait \
             for its lease to end, then read it again.",
            blocked(c),
            rec.id
        ));
    }
    cas(tc, &rec, i.version)?;
    let renewed = rec.claim_at(now).is_some();
    let from = rec.version;
    rec.claim = Some(TaskClaim {
        by: tc.execution_id.into(),
        session: tc.session_id.into(),
        until_ms: now + lease_ms,
    });
    if !renewed {
        rec.version += 1;
    }
    rec.updated_at_ms = theseus_protocol::now_unix_ms();
    let until = crate::push::hm(now + lease_ms);
    let c = change(
        tc,
        &rec,
        (!renewed).then_some(from),
        json!({"until_ms": now + lease_ms, "renewed": renewed, "by": tc.execution_id}),
    );
    done.row(tc, "claimed", c)?;
    done.text = if renewed {
        format!(
            "Renewed your claim on task {} until {until} (still v{}).",
            rec.id, rec.version
        )
    } else {
        format!(
            "Claimed task {} until {until} (v{from} → v{}). Your edits of it renew the claim, \
             and closing it ends it.",
            rec.id, rec.version
        )
    };
    done.meta = json!({"task": rec.id, "version": rec.version, "until_ms": now + lease_ms, "renewed": renewed});
    Ok(done)
}

impl Core {
    /// Build the claims kept in memory, once, after serving.
    pub fn warm_leases(&self) {
        match self.tools.leases.build(&self.store) {
            Ok(true) => tracing::debug!(
                held = self.tools.leases.held().unwrap_or(0),
                "task claims read"
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "the task claims were not read"),
        }
    }

    /// The due pass's leases (the driver's tick): each lapsed claim freed.
    pub fn free_expired_leases_if_due(&self) -> usize {
        if self.tools.leases.held().is_none() {
            self.warm_leases();
        }
        self.free_expired_leases(self.kernel.now_ms())
    }

    /// Free every claim whose lease lapsed by `now_ms`: its record and its
    /// `task.lease_expired` row in one frame, under the task's lock. Returns
    /// how many it freed.
    pub fn free_expired_leases(&self, now_ms: u64) -> usize {
        let Some(due) = self.tools.leases.due(now_ms) else {
            return 0;
        };
        let mut freed = 0;
        for id in due {
            match self.free_lease(&id, now_ms) {
                Ok(true) => freed += 1,
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(task = %id, error = %format!("{e:#}"), "a lapsed claim was not freed");
                }
            }
        }
        freed
    }

    fn free_lease(&self, id: &str, now_ms: u64) -> anyhow::Result<bool> {
        let lock = self.store.lock_task(id);
        let Some(mut rec) = super::get(&self.store, id)? else {
            drop(lock);
            self.tools.leases.note(&TaskRecord {
                id: id.into(),
                ..TaskRecord::default()
            });
            return Ok(false);
        };
        let lapsed = match &rec.claim {
            Some(c) if !rec.state.is_closed() && !c.holds_at(now_ms) => c.clone(),
            _ => {
                // Renewed or ended since it was kept: keep it as it is.
                drop(lock);
                self.tools.leases.note(&rec);
                return Ok(false);
            }
        };
        rec.claim = None;
        rec.updated_at_ms = theseus_protocol::now_unix_ms();
        let c = crate::fact::task_graph::Change {
            session_id: lapsed.session.clone(),
            task: rec.clone(),
            from: None,
            detail: json!({"by": lapsed.by, "session": lapsed.session, "until_ms": lapsed.until_ms}),
        };
        let to = self.session_rec(&lapsed.session);
        let records = vec![
            super::record(&rec)?,
            crate::fact::task_graph::row_of(&to, "lease_expired", &c)?,
        ];
        self.store.append(&records)?;
        drop(lock);
        self.tools.leases.note(&rec);
        crate::fact::task_graph::announce(&to, "lease_expired", &c);
        Ok(true)
    }
}
