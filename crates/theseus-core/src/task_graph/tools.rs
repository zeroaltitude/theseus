//! The task tools (39a, §3.24's `task` family): `task.update`, `task.split`,
//! and `task.close` beside `task.create` (`task.rs`), which records a plan
//! item without `brief` (`create_item`). The harness runs each, under the
//! task's lock: it reads the record, compares the version the call names,
//! and writes the new record in the frame that settles the call, so the
//! record and the call's result land together or not at all.
//!
//! The three layers (§3.5: "so the agent cannot redefine success"):
//! - **Layer 2, the plan** (title, children, deps, owner): applies at once.
//! - **Layer 3, evidence**: each entry is a node and its identity, and
//!   `task.close` only appends.
//! - **Layer 1, the objective and acceptance, and abandoning**: the owner's
//!   on the owner's tasks (`TaskRecord::is_owners`: a task session opened
//!   with a brief, and every record from before theseus-ext.10). There the
//!   call's plan names the authority (`Plan::authority`), so the gate makes it
//!   wait at every posture, as the floor does. Asked, its proposal is written
//!   on the record (`proposed`, `task.change_proposed`); the operator's yes
//!   runs the call, which applies it in one frame (`task.change_accepted`),
//!   and a no clears it and leaves the task (`declined`,
//!   `task.change_declined`), as an expiry does (`task.change_expired`).
//!   Who may answer is who may answer any call: the owner, from a private
//!   place (`judge_act`), and the CLI refuses inside a job. On a task whose
//!   layer 1 the model wrote (a plan item, a split's child), the harness drops
//!   the authority before the gate decides (`authority_for`), so the call runs
//!   at its tool's own posture, and the change applies at once, versioned as
//!   any edit (`task.updated`, `task.closed`).

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_store::NewRecord;
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::{
    check, get, record, NewTask, TaskEvidence, TaskOrigin, TaskProposal, TaskRecord, TaskState,
};
use crate::fact::task_graph::{self as facts, Change};
use crate::store::SessionLock;
use crate::toolrun::TurnCtx;

pub const UPDATE: &str = "task.update";
pub const SPLIT: &str = "task.split";
pub const CLOSE: &str = "task.close";
/// The tools this module and `lease.rs` add, for the template's
/// `[policy.tools]` list.
pub const NAMES: [&str; 4] = [UPDATE, SPLIT, CLOSE, super::lease::CLAIM];

/// The most children one split makes, and the longest title.
pub const MAX_SPLIT: usize = 12;
pub const MAX_TITLE: usize = 200;
/// The most evidence one close appends.
pub const MAX_EVIDENCE: usize = 20;

/// Whether the harness runs `name` here.
pub fn is_edit(name: &str) -> bool {
    NAMES.contains(&name)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    deps: Option<Vec<String>>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    acceptance: Option<Vec<String>>,
}

impl Patch {
    /// The layer-1 fields it changes, as a card names them.
    fn layer1(&self) -> Option<&'static str> {
        match (self.objective.is_some(), self.acceptance.is_some()) {
            (true, true) => Some("objective and acceptance"),
            (true, false) => Some("objective"),
            (false, true) => Some("acceptance"),
            (false, false) => None,
        }
    }

    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.deps.is_none()
            && self.owner.is_none()
            && self.layer1().is_none()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateIn {
    id: String,
    version: u64,
    patch: Patch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitIn {
    id: String,
    version: u64,
    into: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Done,
    Abandoned,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceIn {
    identity: String,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseIn {
    id: String,
    version: u64,
    outcome: Outcome,
    #[serde(default)]
    evidence: Vec<EvidenceIn>,
}

fn title_ok(t: &str) -> Result<(), String> {
    let n = t.trim().chars().count();
    if n == 0 {
        return Err("a title is empty: say what the task is".into());
    }
    if n > MAX_TITLE {
        return Err(format!(
            "a title is {n} characters, over the {MAX_TITLE} a task takes"
        ));
    }
    Ok(())
}

fn update_in(input: &Value) -> Result<UpdateIn, String> {
    let i: UpdateIn = parse(input)?;
    if i.patch.is_empty() {
        return Err(
            "the patch changes nothing: give title, deps, owner, objective, or acceptance".into(),
        );
    }
    if let Some(t) = &i.patch.title {
        title_ok(t)?;
    }
    if i.patch
        .owner
        .as_deref()
        .is_some_and(|o| o.trim().is_empty() || o.len() > 64)
    {
        return Err("owner is `agent` or a principal's name, up to 64 characters".into());
    }
    Ok(i)
}

fn split_in(input: &Value) -> Result<SplitIn, String> {
    let i: SplitIn = parse(input)?;
    if i.into.is_empty() || i.into.len() > MAX_SPLIT {
        return Err(format!("`into` names 1 to {MAX_SPLIT} children's titles"));
    }
    for t in &i.into {
        title_ok(t)?;
    }
    Ok(i)
}

fn close_in(input: &Value) -> Result<CloseIn, String> {
    let i: CloseIn = parse(input)?;
    if i.outcome == Outcome::Done && i.evidence.is_empty() {
        return Err(
            "closing a task `done` takes its evidence: at least one entry, each an \
                    identity (a commit, a job id, a snapshot id) and, if you have it, its node"
                .into(),
        );
    }
    if i.evidence.len() > MAX_EVIDENCE {
        return Err(format!("at most {MAX_EVIDENCE} pieces of evidence a close"));
    }
    if i.evidence
        .iter()
        .any(|e| e.identity.trim().is_empty() || e.identity.len() > 300)
    {
        return Err("each piece of evidence has an identity of 1 to 300 characters".into());
    }
    Ok(i)
}

/// What the gate's card says of a layer-1 call: the change, before and
/// after, as far as the input says it.
fn authority_of(name: &str, input: &Value) -> Option<String> {
    match name {
        UPDATE => {
            let i = update_in(input).ok()?;
            let what = i.patch.layer1()?;
            let mut s = format!(
                "changing task {}'s {what} is the operator's to accept",
                i.id
            );
            if let Some(o) = &i.patch.objective {
                s.push_str(&format!("; objective after: \"{o}\""));
            }
            if let Some(a) = &i.patch.acceptance {
                s.push_str(&format!("; acceptance after: \"{}\"", a.join("\"; \"")));
            }
            Some(s)
        }
        CLOSE => {
            let i = close_in(input).ok()?;
            (i.outcome == Outcome::Abandoned).then(|| {
                format!(
                    "abandoning task {} is the operator's to accept (abandoning is not completing)",
                    i.id
                )
            })
        }
        _ => None,
    }
}

/// What a layer-1 call changes, for its question (39b): the task it names,
/// its title, the field, and the field before (the record as it is) and
/// after (the call's input), or abandoning it. None for any other call, or a
/// task the call does not name.
pub fn change_of(
    store: &crate::store::Store,
    name: &str,
    input: &Value,
) -> Option<theseus_protocol::tasks::TaskChange> {
    authority_of(name, input)?;
    let id = input.get("id")?.as_str()?;
    let all = super::all(store).ok()?;
    let t = super::resolve(&all, id).ok()?;
    let lines = |a: &[String]| a.join("; ");
    let (field, before, after) = match name {
        CLOSE => (
            "abandon".to_string(),
            t.state.as_str().to_string(),
            TaskState::Abandoned.as_str().to_string(),
        ),
        _ => {
            let i = update_in(input).ok()?;
            let field = i.patch.layer1()?.to_string();
            let mut before = Vec::new();
            let mut after = Vec::new();
            if let Some(o) = &i.patch.objective {
                before.push(t.objective.clone());
                after.push(o.clone());
            }
            if let Some(a) = &i.patch.acceptance {
                before.push(lines(&t.acceptance));
                after.push(lines(a));
            }
            (field, before.join(" / "), after.join(" / "))
        }
    };
    Some(theseus_protocol::tasks::TaskChange {
        task: t.id.clone(),
        title: t.title.clone(),
        field,
        before,
        after,
    })
}

/// The task a layer-1 call names, when it is the owner's: the one whose
/// change waits for the operator. None for any other call, a plan item or a
/// split's child, or a task the call does not name (its run refuses it).
fn owners_target(store: &crate::store::Store, name: &str, input: &Value) -> Option<TaskRecord> {
    authority_of(name, input)?;
    let id = input.get("id")?.as_str()?;
    let all = super::all(store).ok()?;
    let t = super::resolve(&all, id).ok()?;
    t.is_owners().then(|| t.clone())
}

/// Whether a layer-1 call waits for the operator: it names one of the
/// owner's tasks, or a task that cannot be read (the safe side).
fn waits(store: &crate::store::Store, name: &str, input: &Value) -> bool {
    if authority_of(name, input).is_none() {
        return false;
    }
    let Some(id) = input.get("id").and_then(Value::as_str) else {
        return true;
    };
    match super::all(store) {
        Ok(all) => super::resolve(&all, id).map_or(true, TaskRecord::is_owners),
        Err(_) => true,
    }
}

/// The harness's word on a task call's plan, before the gate decides
/// (theseus-ext.10): a layer-1 change to a task the model wrote applies at
/// once, so its plan names no authority and the call runs at its tool's own
/// posture. The owner's tasks keep it.
pub fn authority_for(
    store: &crate::store::Store,
    name: &str,
    input: &Value,
    mut plan: Plan,
) -> Plan {
    if plan.authority.is_some() && !waits(store, name, input) {
        plan.authority = None;
    }
    plan
}

fn id_schema() -> Value {
    json!({"type": "string", "description": "The task's id (tsk_…), as the task graph shows it."})
}

fn version_schema() -> Value {
    json!({"type": "integer", "minimum": 1, "description": "The version you read (v7 in the task graph): the edit is refused if the task has changed since."})
}

/// `task.update`.
pub struct TaskUpdate;

impl Tool for TaskUpdate {
    fn name(&self) -> &'static str {
        UPDATE
    }

    fn description(&self) -> &'static str {
        "Edit a task in the task graph. Name the `version` you read; if the task changed since, \
         the edit is refused with the task as it is now, and you read it again. `title`, `deps` \
         (the tasks it waits on), and `owner` apply at once. On a task the task graph marks \
         \"the operator's objective\", `objective` and `acceptance` are the operator's: a change \
         to either becomes a proposal that waits for the operator's yes, and nothing changes \
         until then. On any other task (a plan item, a split's child) they apply at once."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": id_schema(),
                "version": version_schema(),
                "patch": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "deps": {"type": "array", "items": {"type": "string"}, "description": "The tasks it waits on, replacing the list."},
                        "owner": {"type": "string", "description": "`agent`, or a person."},
                        "objective": {"type": "string", "description": "On the operator's task, a proposal that waits for the operator."},
                        "acceptance": {"type": "array", "items": {"type": "string"}, "description": "One line each. On the operator's task, a proposal that waits for the operator."}
                    },
                    "additionalProperties": false
                }
            },
            "required": ["id", "version", "patch"],
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
        let i = update_in(input)?;
        let mut fields: Vec<&str> = Vec::new();
        for (f, on) in [
            ("title", i.patch.title.is_some()),
            ("deps", i.patch.deps.is_some()),
            ("owner", i.patch.owner.is_some()),
            ("objective", i.patch.objective.is_some()),
            ("acceptance", i.patch.acceptance.is_some()),
        ] {
            if on {
                fields.push(f);
            }
        }
        Ok(Plan {
            summary: format!(
                "update task {} (v{}): {}",
                i.id,
                i.version,
                fields.join(", ")
            ),
            authority: authority_of(UPDATE, input),
            ..Default::default()
        })
    }
}

/// `task.split`.
pub struct TaskSplit;

impl Tool for TaskSplit {
    fn name(&self) -> &'static str {
        SPLIT
    }

    fn description(&self) -> &'static str {
        "Split a task into children, one per title in `into`, under it in the task graph. Name \
         the `version` you read; a stale one is refused with the task as it is now."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": id_schema(),
                "version": version_schema(),
                "into": {"type": "array", "minItems": 1, "maxItems": MAX_SPLIT, "items": {"type": "string"}, "description": "The children's titles."}
            },
            "required": ["id", "version", "into"],
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
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i = split_in(input)?;
        Ok(Plan {
            summary: format!("split task {} (v{}) into {}", i.id, i.version, i.into.len()),
            ..Default::default()
        })
    }
}

/// `task.close`.
pub struct TaskClose;

impl Tool for TaskClose {
    fn name(&self) -> &'static str {
        CLOSE
    }

    fn description(&self) -> &'static str {
        "Close a task: `done` with its evidence (each an identity, such as `commit:<sha>` or \
         `job:<id>`, and its node when you have one), or `abandoned`. Evidence is only ever \
         added. Abandoning a task the task graph marks \"the operator's objective\" is the \
         operator's: it waits for the operator's yes; any other task's applies at once. Name \
         the `version` you read; a stale one is refused with the task as it is now."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": id_schema(),
                "version": version_schema(),
                "outcome": {"type": "string", "enum": ["done", "abandoned"]},
                "evidence": {
                    "type": "array",
                    "maxItems": MAX_EVIDENCE,
                    "items": {
                        "type": "object",
                        "properties": {
                            "identity": {"type": "string", "description": "commit:<sha>, job:<id>, snapshot:<id>, or what identifies it."},
                            "node": {"type": "string", "description": "The node that shows it, if you have its id."},
                            "note": {"type": "string"}
                        },
                        "required": ["identity"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["id", "version", "outcome"],
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
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i = close_in(input)?;
        let how = match i.outcome {
            Outcome::Done => "done",
            Outcome::Abandoned => "abandoned",
        };
        Ok(Plan {
            summary: format!(
                "close task {} (v{}) {how}, with {} piece(s) of evidence",
                i.id,
                i.version,
                i.evidence.len()
            ),
            authority: authority_of(CLOSE, input),
            ..Default::default()
        })
    }
}

/// What an edit did: the call's result, the records its frame writes (the
/// task records and their rows), the changes to announce once that frame is
/// written, and the locks held until then.
pub struct Done<'a> {
    pub text: String,
    pub meta: Value,
    pub records: Vec<NewRecord>,
    pub changes: Vec<(&'static str, Change)>,
    pub locks: Vec<SessionLock<'a>>,
}

impl Default for Done<'_> {
    fn default() -> Self {
        Self {
            text: String::new(),
            meta: Value::Null,
            records: vec![],
            changes: vec![],
            locks: vec![],
        }
    }
}

impl<'a> Done<'a> {
    /// Keep what `d` writes and announces, and hand back its result.
    pub fn take(&mut self, mut d: Done<'a>) -> (String, Value) {
        self.records.append(&mut d.records);
        self.changes.append(&mut d.changes);
        self.locks.append(&mut d.locks);
        (d.text, d.meta)
    }

    pub(crate) fn row(
        &mut self,
        tc: &TurnCtx<'_>,
        verb: &'static str,
        c: Change,
    ) -> Result<(), String> {
        self.records
            .push(record(&c.task).map_err(|e| format!("{e:#}"))?);
        self.records
            .push(facts::row_of(&tc.rec(), verb, &c).map_err(|e| format!("{e:#}"))?);
        self.changes.push((verb, c));
        Ok(())
    }
}

/// Announce what an edit did, once its frame is written.
pub fn announce(tc: &TurnCtx<'_>, changes: &[(&'static str, Change)]) {
    for (verb, c) in changes {
        facts::announce(&tc.rec(), verb, c);
    }
}

pub(crate) fn change(
    tc: &TurnCtx<'_>,
    task: &TaskRecord,
    from: Option<u64>,
    detail: Value,
) -> Change {
    Change {
        session_id: tc.session_id.into(),
        task: task.clone(),
        from,
        detail,
    }
}

/// Lock a task and read it, or say why not.
pub(super) fn locked<'a>(
    tc: &TurnCtx<'a>,
    id: &str,
) -> Result<(SessionLock<'a>, TaskRecord), String> {
    let all = super::all(tc.store).map_err(|e| format!("{e:#}"))?;
    let id = super::resolve(&all, id)?.id.clone();
    let lock = tc.store.lock_task(&id);
    let rec = get(tc.store, &id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| format!("no task is named `{id}`"))?;
    Ok((lock, rec))
}

/// The compare of compare-and-swap, with its refusal recorded.
pub(super) fn cas(tc: &TurnCtx<'_>, rec: &TaskRecord, version: u64) -> Result<(), String> {
    check(rec, version).map_err(|stale| {
        facts::record(
            &tc.rec(),
            "stale_refused",
            &change(tc, &stale.now, None, json!({"named": stale.named})),
        );
        stale.message()
    })
}

/// Run one edit (the harness's side). `approved`: the operator approved the
/// call, which a layer-1 change needs. `lease_ms`: a claim's lease, which
/// the holder's edit renews (39b).
pub fn run<'a>(
    tc: &TurnCtx<'a>,
    name: &str,
    input: &Value,
    correlation_id: &str,
    (approved, lease_ms): (bool, u64),
) -> Result<Done<'a>, String> {
    match name {
        UPDATE => update(tc, input, approved, lease_ms),
        SPLIT => split(tc, input, correlation_id, lease_ms),
        CLOSE => close(tc, input, correlation_id, approved),
        super::lease::CLAIM => super::lease::claim(tc, input, lease_ms),
        other => Err(format!("{other} is not a task tool")),
    }
}

/// `task.update`: layer 2 applies; layer 1 only with the operator's yes.
fn update<'a>(
    tc: &TurnCtx<'a>,
    input: &Value,
    approved: bool,
    lease_ms: u64,
) -> Result<Done<'a>, String> {
    let mut done = Done::default();
    let now = theseus_protocol::now_unix_ms();
    let i = update_in(input)?;
    let (lock, mut rec) = locked(tc, &i.id)?;
    done.locks.push(lock);
    cas(tc, &rec, i.version)?;
    if rec.state.is_closed() {
        return Err(format!(
            "Refused: task {} is {}; a closed task changes no more",
            rec.id,
            rec.state.as_str()
        ));
    }
    // A plan item's layer 1 is the model's: it applies at once
    // (theseus-ext.10); the owner's waits for the operator's yes.
    let touches_layer1 = i.patch.layer1().is_some();
    let layer1 = i.patch.layer1().filter(|_| rec.is_owners());
    if layer1.is_some() && !approved {
        return Err(format!(
            "Not applied: a change to task {}'s {} is the operator's, and it ran \
             without the operator's yes",
            rec.id,
            layer1.unwrap_or_default()
        ));
    }
    if let Some(deps) = &i.patch.deps {
        let all = super::all(tc.store).map_err(|e| format!("{e:#}"))?;
        for d in deps {
            if d == &rec.id {
                return Err(format!("task {} cannot wait on itself", rec.id));
            }
            if !all.iter().any(|t| &t.id == d) {
                return Err(format!("no task is named `{d}` (deps name tasks by id)"));
            }
        }
    }
    let from = rec.version;
    let mut fields: Vec<&str> = vec![];
    if let Some(t) = i.patch.title {
        rec.title = t.trim().to_string();
        fields.push("title");
    }
    if let Some(d) = i.patch.deps {
        rec.deps = d;
        fields.push("deps");
    }
    if let Some(o) = i.patch.owner {
        rec.owner = o.trim().to_string();
        fields.push("owner");
    }
    if let Some(o) = i.patch.objective {
        rec.objective = o;
        fields.push("objective");
    }
    if let Some(a) = i.patch.acceptance {
        rec.acceptance = a;
        fields.push("acceptance");
    }
    if touches_layer1 {
        rec.proposal = None;
    }
    super::lease::renew(&mut rec, tc, lease_ms);
    rec.version += 1;
    rec.updated_at_ms = now;
    let verb = if layer1.is_some() {
        "change_accepted"
    } else {
        "updated"
    };
    let c = change(tc, &rec, Some(from), json!({"fields": fields.join(",")}));
    done.row(tc, verb, c)?;
    done.text = format!(
        "Updated task {} ({}): v{from} → v{}.{}",
        rec.id,
        fields.join(", "),
        rec.version,
        if layer1.is_some() {
            " The operator accepted the change."
        } else {
            ""
        }
    );
    done.meta = json!({"task": rec.id, "version": rec.version, "from": from, "fields": fields});
    Ok(done)
}

/// `task.split`: children under the task, which moves a version.
fn split<'a>(
    tc: &TurnCtx<'a>,
    input: &Value,
    correlation_id: &str,
    lease_ms: u64,
) -> Result<Done<'a>, String> {
    let mut done = Done::default();
    let now = theseus_protocol::now_unix_ms();
    let i = split_in(input)?;
    let (lock, mut rec) = locked(tc, &i.id)?;
    done.locks.push(lock);
    cas(tc, &rec, i.version)?;
    if rec.state.is_closed() {
        return Err(format!(
            "Refused: task {} is {}",
            rec.id,
            rec.state.as_str()
        ));
    }
    let base = super::id_from(correlation_id);
    let principal = super::principal_of(tc.kernel, tc.execution_id);
    let kids: Vec<TaskRecord> = i
        .into
        .iter()
        .enumerate()
        .map(|(n, title)| {
            NewTask {
                id: format!("{base}{:x}", n + 1),
                title: title.trim(),
                objective: String::new(),
                acceptance: vec![],
                parent: Some(rec.id.clone()),
                deps: vec![],
                session: None,
                origin: TaskOrigin {
                    session: tc.session_id.into(),
                    principal: principal.clone(),
                    by_model: true,
                },
                state: TaskState::Accepted,
            }
            .build(now)
        })
        .collect();
    let from = rec.version;
    super::lease::renew(&mut rec, tc, lease_ms);
    rec.version += 1;
    rec.updated_at_ms = now;
    let ids: Vec<&str> = kids.iter().map(|k| k.id.as_str()).collect();
    let c = change(
        tc,
        &rec,
        Some(from),
        json!({"children": kids.len(), "ids": ids}),
    );
    done.row(tc, "split", c)?;
    for k in &kids {
        let c = change(tc, k, None, json!({"parent": rec.id}));
        done.row(tc, "created", c)?;
    }
    done.text = format!(
        "Split task {} into {} (v{from} → v{}):\n{}",
        rec.id,
        kids.len(),
        rec.version,
        kids.iter().map(super::line).collect::<Vec<_>>().join("\n")
    );
    done.meta = json!({"task": rec.id, "version": rec.version, "from": from, "children": ids});
    Ok(done)
}

/// `task.close`: evidence appended, and the outcome; abandoning only with
/// the operator's yes.
fn close<'a>(
    tc: &TurnCtx<'a>,
    input: &Value,
    correlation_id: &str,
    approved: bool,
) -> Result<Done<'a>, String> {
    let mut done = Done::default();
    let now = theseus_protocol::now_unix_ms();
    let i = close_in(input)?;
    let (lock, mut rec) = locked(tc, &i.id)?;
    done.locks.push(lock);
    cas(tc, &rec, i.version)?;
    if rec.state.is_closed() {
        return Err(format!(
            "Refused: task {} is closed already ({})",
            rec.id,
            rec.state.as_str()
        ));
    }
    let owners = rec.is_owners();
    if i.outcome == Outcome::Abandoned && owners && !approved {
        return Err(format!(
            "Not applied: abandoning task {} is the operator's, and it ran without the \
             operator's yes",
            rec.id
        ));
    }
    let holder = tc
        .store
        .transcript(tc.session_id)
        .ok()
        .and_then(|n| crate::task::holder_of(&n, correlation_id));
    let from = rec.version;
    let added = i.evidence.len();
    for e in i.evidence {
        rec.evidence.push(TaskEvidence {
            node: e.node.or_else(|| holder.clone()),
            identity: e.identity.trim().to_string(),
            note: e.note,
            by: tc.session_id.into(),
            at_ms: now,
        });
    }
    rec.state = match i.outcome {
        Outcome::Done => TaskState::Done,
        Outcome::Abandoned => TaskState::Abandoned,
    };
    rec.proposal = None;
    // A close ends a claim (39b).
    rec.claim = None;
    rec.version += 1;
    rec.updated_at_ms = now;
    if i.outcome == Outcome::Abandoned && owners {
        let c = change(tc, &rec, Some(from), json!({"fields": "abandon"}));
        done.row(tc, "change_accepted", c)?;
    }
    let c = change(
        tc,
        &rec,
        Some(from),
        json!({"added": added, "evidence": rec.evidence}),
    );
    done.row(tc, "closed", c)?;
    done.text = format!(
        "Closed task {} {} (v{from} → v{}), with {}.",
        rec.id,
        rec.state.as_str(),
        rec.version,
        crate::narrative::count(added as u64, "piece of evidence", "pieces of evidence")
    );
    done.meta =
        json!({"task": rec.id, "version": rec.version, "from": from, "state": rec.state.as_str()});
    Ok(done)
}

/// `task.create` without `brief` (M7 Q12): a plan item, recorded and nothing
/// more. It delegates nothing, so it needs no arrangement (the default 39a
/// takes); one given is resolved, and its `objective` and `acceptance`
/// pieces are the record's.
#[derive(Debug, Default)]
pub struct Item<'i> {
    pub title: Option<&'i str>,
    pub objective: Option<&'i str>,
    pub acceptance: &'i [String],
    pub parent: Option<&'i str>,
    pub deps: &'i [String],
    pub arrangement: Option<&'i crate::arrangement::Input>,
}

/// The record a task's objective and acceptance take from its pieces, else
/// from what the call says.
pub fn from_pieces(
    pieces: &[crate::arrangement::Piece],
    objective: Option<&str>,
    acceptance: &[String],
    title: &str,
) -> (String, Vec<String>) {
    use crate::arrangement::Role;
    let text = |p: &crate::arrangement::Piece| -> String {
        p.text
            .as_deref()
            .unwrap_or(&p.first_line)
            .chars()
            .take(1000)
            .collect()
    };
    let standing = |r: Role| {
        pieces
            .iter()
            .filter(move |p| p.role == r && p.superseded_by.is_none())
    };
    let objective = standing(Role::Objective)
        .next()
        .map(text)
        .or_else(|| objective.map(String::from))
        .unwrap_or_else(|| title.to_string());
    let mut accept: Vec<String> = standing(Role::Acceptance).map(text).collect();
    accept.extend(acceptance.iter().cloned());
    (objective, accept)
}

pub fn create_item<'a>(
    tc: &TurnCtx<'a>,
    item: &Item<'_>,
    correlation_id: &str,
) -> Result<Done<'a>, String> {
    let title = item
        .title
        .ok_or("a plan item needs a `title` (or give a `brief` to start a task session)")?
        .trim();
    title_ok(title)?;
    let id = super::id_from(correlation_id);
    if let Some(had) = get(tc.store, &id).map_err(|e| format!("{e:#}"))? {
        return Ok(Done {
            text: format!(
                "Task {} (\"{}\") was already recorded by this call: {}",
                had.id,
                had.title,
                super::line(&had)
            ),
            meta: json!({"task": had.id, "version": had.version, "opened": false}),
            ..Done::default()
        });
    }
    let all = super::all(tc.store).map_err(|e| format!("{e:#}"))?;
    let parent = match item.parent {
        Some(p) => {
            let p = super::resolve(&all, p)?;
            if p.state.is_closed() {
                return Err(format!(
                    "task {} is {}: a closed task takes no children",
                    p.id,
                    p.state.as_str()
                ));
            }
            Some(p.id.clone())
        }
        None => None,
    };
    for d in item.deps {
        if !all.iter().any(|t| &t.id == d) {
            return Err(format!("no task is named `{d}` (deps name tasks by id)"));
        }
    }
    let pieces = match item.arrangement {
        Some(a) => {
            a.check()?;
            let nodes = tc
                .store
                .transcript(tc.session_id)
                .map_err(|e| format!("{e:#}"))?;
            let holder = crate::task::holder_of(&nodes, correlation_id);
            crate::arrangement::resolve(a, &nodes, holder.as_deref()).map_err(|r| r.message)?
        }
        None => vec![],
    };
    let (objective, acceptance) = from_pieces(&pieces, item.objective, item.acceptance, title);
    let rec = NewTask {
        id,
        title,
        objective,
        acceptance,
        parent,
        deps: item.deps.to_vec(),
        session: None,
        origin: TaskOrigin {
            session: tc.session_id.into(),
            principal: super::principal_of(tc.kernel, tc.execution_id),
            by_model: true,
        },
        state: TaskState::Accepted,
    }
    .build(theseus_protocol::now_unix_ms());
    let mut done = Done::default();
    done.row(
        tc,
        "created",
        change(tc, &rec, None, json!({"pieces": pieces.len()})),
    )?;
    done.text = format!(
        "Recorded task {} as a plan item (no session works on it; split it, close it with its \
         evidence, or start a task session for it with `brief`): {}",
        rec.id,
        super::line(&rec)
    );
    done.meta = json!({"task": rec.id, "version": rec.version, "opened": true});
    Ok(done)
}

/// The lock a layer-1 call's proposal is written under, as its question is
/// asked: the task's, before the execution's.
pub fn lock_for_call<'a>(tc: &TurnCtx<'a>, name: &str, input: &Value) -> Option<SessionLock<'a>> {
    let id = owners_target(tc.store, name, input)?.id;
    Some(tc.store.lock_task(&id))
}

/// A layer-1 call's proposal, written on its task in the frame that asks
/// its question (`task.change_proposed`): the record (its version
/// unchanged, since nothing of it changed yet) and the row. None for any
/// other call, a task the model wrote (its change waits for nothing), or
/// one whose version is stale already: its run is refused.
pub fn proposed(
    tc: &TurnCtx<'_>,
    name: &str,
    input: &Value,
    card: &str,
) -> anyhow::Result<Option<(Vec<NewRecord>, Change)>> {
    let Some(rec) = owners_target(tc.store, name, input) else {
        return Ok(None);
    };
    let Some(mut rec) = get(tc.store, &rec.id)? else {
        return Ok(None);
    };
    let version = input.get("version").and_then(Value::as_u64).unwrap_or(0);
    if rec.version != version || rec.state.is_closed() {
        return Ok(None);
    }
    let patch = input.get("patch");
    let fields = match name {
        CLOSE => "abandon",
        _ => match (
            patch.and_then(|p| p.get("objective")).is_some(),
            patch.and_then(|p| p.get("acceptance")).is_some(),
        ) {
            (true, true) => "objective,acceptance",
            (true, false) => "objective",
            _ => "acceptance",
        },
    };
    rec.proposal = Some(TaskProposal {
        objective: patch
            .and_then(|p| p.get("objective"))
            .and_then(Value::as_str)
            .map(String::from),
        acceptance: patch
            .and_then(|p| p.get("acceptance"))
            .and_then(|a| serde_json::from_value(a.clone()).ok()),
        abandon: name == CLOSE,
        by: tc.session_id.into(),
        card: card.into(),
        base_version: rec.version,
        at_ms: theseus_protocol::now_unix_ms(),
    });
    let c = change(
        tc,
        &rec,
        None,
        json!({"fields": fields, "card": card, "before": {"objective": rec.objective, "acceptance": rec.acceptance}}),
    );
    let records = vec![
        record(&rec)?,
        facts::row_of(&tc.rec(), "change_proposed", &c)?,
    ];
    Ok(Some((records, c)))
}

/// A declined layer-1 call (`task.change_declined`): its proposal cleared,
/// the task otherwise as it was, for the answer's frame. The caller holds
/// the task's lock (`lock_for_answer`). None when the call is no layer-1
/// call, or its task no longer holds its proposal.
pub fn declined(
    store: &crate::store::Store,
    rec: &crate::fact::Rec<'_>,
    a: &theseus_kernel::Action,
) -> anyhow::Result<Option<(Vec<NewRecord>, Change)>> {
    cleared(store, rec, a, "change_declined")
}

/// An expired layer-1 question (`task.change_expired`, theseus-ext.10): its
/// proposal cleared in the expiry's frame, as a decline's is, so the view
/// never says a change waits when none does. The caller holds the task's
/// lock (`lock_for_answer`).
pub fn expired(
    store: &crate::store::Store,
    rec: &crate::fact::Rec<'_>,
    a: &theseus_kernel::Action,
) -> anyhow::Result<Option<(Vec<NewRecord>, Change)>> {
    cleared(store, rec, a, "change_expired")
}

fn cleared(
    store: &crate::store::Store,
    rec: &crate::fact::Rec<'_>,
    a: &theseus_kernel::Action,
    verb: &str,
) -> anyhow::Result<Option<(Vec<NewRecord>, Change)>> {
    let Some(p) = &a.proposal else {
        return Ok(None);
    };
    if authority_of(&p.tool, &p.args).is_none() {
        return Ok(None);
    }
    let Some(id) = p.args.get("id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let all = super::all(store)?;
    let Ok(t) = super::resolve(&all, id) else {
        return Ok(None);
    };
    let Some(mut t) = get(store, &t.id)? else {
        return Ok(None);
    };
    if t.proposal.as_ref().map(|p| p.card.as_str()) != Some(a.correlation_id.as_str()) {
        return Ok(None);
    }
    t.proposal = None;
    let c = Change {
        session_id: a.session_id.clone(),
        task: t.clone(),
        from: None,
        detail: json!({"card": a.correlation_id}),
    };
    let records = vec![record(&t)?, facts::row_of(rec, verb, &c)?];
    Ok(Some((records, c)))
}

/// The lock an answer to a layer-1 call takes before its frame.
pub fn lock_for_answer<'a>(
    store: &'a crate::store::Store,
    a: &theseus_kernel::Action,
) -> Option<SessionLock<'a>> {
    let p = a.proposal.as_ref()?;
    authority_of(&p.tool, &p.args)?;
    let id = p.args.get("id")?.as_str()?;
    let all = super::all(store).ok()?;
    let id = super::resolve(&all, id).ok()?.id.clone();
    Some(store.lock_task(&id))
}

/// A task session's close by its report (`turn.rs`): the change the frame
/// that ends the task made, to announce after. Its record's lock is taken
/// before that frame and dropped once it is written.
pub struct Closing<'a> {
    store: &'a crate::store::Store,
    closed: std::sync::Mutex<Option<Change>>,
}

impl<'a> Closing<'a> {
    /// The lock, for a task session's turn, and the slot.
    pub fn new(
        store: &'a crate::store::Store,
        task: Option<&crate::session::TaskOf>,
        sink: &crate::bus::EventSink,
    ) -> (Option<SessionLock<'a>>, Self) {
        let lock = task.map(|_| store.lock_task(&super::of_session(&sink.session_id)));
        let me = Self {
            store,
            closed: std::sync::Mutex::new(None),
        };
        (lock, me)
    }

    /// The record closed `done` or `failed`, and its `task.closed` row, for
    /// the frame that ends the task. Nothing for a session with no record,
    /// or a turn that did not end it.
    pub fn records(
        &self,
        e: &theseus_kernel::Execution,
        report_node: Option<&str>,
    ) -> anyhow::Result<Vec<NewRecord>> {
        let Some(rec) = super::closed_by_report(self.store, e, report_node)? else {
            return Ok(vec![]);
        };
        let c = Change {
            session_id: e.session_id.clone(),
            task: rec.clone(),
            from: Some(rec.version - 1),
            detail: json!({"added": 1, "by": "report", "evidence": rec.evidence}),
        };
        let row = crate::fact::row(&facts::TaskClosed(&c), Some(&e.session_id), None)?;
        *self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(c);
        Ok(vec![record(&rec)?, row])
    }

    /// The change, once its frame is written.
    pub fn end(self) -> Option<Change> {
        self.closed
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Announce a report's close, once its frame is written.
pub fn announce_closed(rec: &crate::fact::Rec<'_>, c: Option<Change>) {
    if let Some(c) = c {
        facts::announce(rec, "closed", &c);
    }
}
