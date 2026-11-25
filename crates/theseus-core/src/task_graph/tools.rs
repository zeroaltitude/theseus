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
//!   with a brief, and every record from before theseus-ext.10). A change to
//!   one runs at its tool's own posture, and the owner is told, with an undo
//!   that writes the task's record before it back (`undo.rs`, issue 57). Where
//!   the operator set the tool to `approve`, the call's plan names the
//!   authority (`Plan::authority`), so its card says what changes. A
//!   self-loop item's abandoning is never told (`undo::keeps_card`): it keeps
//!   its card unless the operator set `task.close` to `notify`, as before.
//!   Asked, its proposal is written
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
use theseus_store::{kinds, NewRecord};
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
    /// A milestone's or project's date, in `wake.at`'s words; `none` clears
    /// it (theseus-n5ui2).
    #[serde(default)]
    due: Option<String>,
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
            && self.due.is_none()
            && self.layer1().is_none()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateIn {
    id: String,
    version: u64,
    patch: Patch,
    /// The owner's message the change is in (issue 44): its node id.
    #[serde(default)]
    said: Option<String>,
    /// What the change does to his objective.
    #[serde(default)]
    kind: Option<super::words::Kind>,
}

impl UpdateIn {
    fn ask(&self) -> super::words::Ask<'_> {
        super::words::Ask {
            named: &self.id,
            version: self.version,
            said: self.said.as_deref(),
            kind: self.kind,
            objective: self.patch.objective.as_deref(),
            acceptance: self.patch.acceptance.as_deref(),
        }
    }
}

/// Whether a `task.update` on the owner's task applies without a card, in
/// his own current words (`words.rs`); why not otherwise. Read again under
/// the task's lock when the call runs.
pub fn by_words(tc: &TurnCtx<'_>, input: &Value) -> Result<super::words::Kind, super::words::Why> {
    would(tc, input).and_then(|(kind, acting)| match acting {
        true => Ok(kind),
        false => Err(super::words::Why::Shadow),
    })
}

/// What the rule makes of a `task.update` naming his words: the kind it
/// would apply, and whether its judge acts; or why it keeps the card.
fn would(tc: &TurnCtx<'_>, input: &Value) -> Result<(super::words::Kind, bool), super::words::Why> {
    use super::words::Why;
    let i = update_in(input).map_err(|_| Why::NoWords)?;
    let (Some(said), Some(_)) = (i.said.as_deref(), i.kind) else {
        return Err(Why::NoWords);
    };
    let all = super::all(tc.store).map_err(|_| Why::NotOneTask)?;
    let rec = super::resolve(&all, &i.id).map_err(|_| Why::NotOneTask)?;
    let ev = super::words::Records::of(tc);
    let kind = super::words::decide(&ev, rec, &i.ask())?;
    Ok((kind, super::words::Evidence::acting(&ev, said)))
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
    tc: &TurnCtx<'_>,
    name: &str,
    input: &Value,
    plan: Plan,
    (asks, abandon_follows_posture): (bool, bool),
) -> Plan {
    let mut plan = plan;
    // Told, not asked (issue 57): an act the owner can undo exactly runs at
    // its tool's own posture, and he is told, with an Undo. `asks`: the
    // operator set the tool to approve, and its card keeps its words.
    if !asks && plan.authority.is_some() && told(tc.store, name, input) {
        plan.authority = None;
        return plan;
    }
    // The operator set `task.close` below approve: abandoning a self-loop item
    // then follows it, a notice, not a hard-coded ask.
    if name == CLOSE && abandon_follows_posture {
        plan.authority = None;
        return plan;
    }
    // A routine's creation always waits (theseus-pvm5c), and a command watch
    // asks the owner once at its creation (theseus-f2r2b): neither is a task
    // call, so this is not the place that clears their question; layer 1 only
    // guards the owner's tasks. A question for the owner (`owner_ask`, theseus-j7cje)
    // always waits for the owner too.
    let own = name == crate::routine_tool::CREATE
        || name == crate::watch_tool::WATCH
        || name == crate::ask::OWNER_ASK;
    // Only a task edit's authority is conditional; another tool that names one
    // (`thread.split`, and task.create's plan naming a project's review
    // routine, theseus-n5ui2) always waits for the owner.
    if plan.authority.is_some() && !own && is_edit(name) && !waits(tc.store, name, input) {
        plan.authority = None;
    }
    // A change in the owner's own current words that only narrows needs no
    // card once an independent judgment says so (issue 44; none does yet, so
    // every change keeps it).
    if plan.authority.is_some() && name == UPDATE && by_words(tc, input).is_ok() {
        plan.authority = None;
    }
    plan
}

/// Whether a call that names an authority is told rather than asked when
/// its tool is not set to approve (issue 57): a change to the owner's task
/// (a self-loop item's abandoning aside, `undo::keeps_card`), a routine, a
/// project's review routine, and a watch. Each has its undo.
fn told(store: &crate::store::Store, name: &str, input: &Value) -> bool {
    match name {
        UPDATE => true,
        CLOSE => owners_target(store, name, input).is_none_or(|t| !super::undo::keeps_card(&t)),
        n => n == crate::routine_tool::CREATE
            || n == crate::task::CREATE
            || n == crate::watch_tool::WATCH,
    }
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
         to either applies and tells the operator, who can undo it. On any other task (a plan \
         item, a split's child) they apply at once. On \
         the operator's task, name the operator's message that asks the change in `said`, and \
         its `kind`."
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
                        "due": {"type": "string", "description": "A milestone's or project's date (`2026-11-03`, `fri 09:00`, `in 3d`), or `none`. A milestone's date sets its reminder."},
                        "objective": {"type": "string", "description": "On the operator's task, the operator is told and can undo it."},
                        "acceptance": {"type": "array", "items": {"type": "string"}, "description": "One line each. On the operator's task, the operator is told and can undo it."}
                    },
                    "additionalProperties": false
                },
                "said": {"type": "string", "description": "The operator's message asking it."},
                "kind": {"type": "string", "enum": ["narrows", "widens", "replaces"]}
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
            ("due", i.patch.due.is_some()),
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
         operator's: it runs and tells them, and they can undo it (an item of Theseus's own \
         self-improvement backlog waits for their yes); any other task's applies at once. Name \
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
/// call, which a self-loop item's abandoning needs; `said_yes`: it was really
/// asked and answered (an abandon that only follows a notify posture is
/// approved, not asked). A layer-1 change of the owner's task that ran without
/// his yes is told, with an undo (issue 57). `lease_ms`: a claim's lease, which
/// the holder's edit renews (39b).
pub fn run<'a>(
    tc: &TurnCtx<'a>,
    name: &str,
    input: &Value,
    correlation_id: &str,
    (approved, said_yes, lease_ms): (bool, bool, u64),
) -> Result<Done<'a>, String> {
    match name {
        UPDATE => update(tc, input, said_yes, lease_ms),
        SPLIT => split(tc, input, correlation_id, lease_ms),
        CLOSE => close(tc, input, correlation_id, approved, said_yes),
        super::lease::CLAIM => super::lease::claim(tc, input, lease_ms),
        other => Err(format!("{other} is not a task tool")),
    }
}

/// `task.update`: layer 2 applies; the owner's layer 1 with his yes, in his
/// own words, or told with an undo.
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
    let by_words = his_words(tc, &rec, &i, layer1, approved, &mut done)?;
    let told = layer1.is_some() && !approved && by_words.is_none();
    let before = rec.clone();
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
    let (fields, retitled) = patched(tc, &mut rec, i.patch)?;
    if touches_layer1 {
        rec.proposal = None;
    }
    super::lease::renew(&mut rec, tc, lease_ms);
    rec.version += 1;
    rec.updated_at_ms = now;
    // The words behind an owner's change, by reference, when the call named
    // them and his yes or his words carried it (a told one went by neither).
    let kept = match (i.said, i.kind, layer1) {
        (Some(node), Some(kind), Some(_)) if !told => Some(super::words::Words {
            task: rec.id.clone(),
            version: rec.version,
            from,
            node,
            turn: tc.turn_id.into(),
            session: tc.session_id.into(),
            kind,
            carded: by_words.is_none(),
            before: None,
            at_ms: now,
        }),
        _ => None,
    };
    let verb = if layer1.is_some() && !told {
        "change_accepted"
    } else {
        "updated"
    };
    let mut detail = json!({"fields": fields.join(",")});
    if let (Some(w), Some(kind)) = (&kept, by_words) {
        detail["words"] = json!({"said": w.node, "kind": kind.as_str()});
    }
    if told {
        super::undo::tell(&mut detail, &before).map_err(|e| format!("{e:#}"))?;
    }
    let c = change(tc, &rec, Some(from), detail);
    done.row(tc, verb, c)?;
    if told {
        if let Some(row) = done.records.last_mut() {
            super::undo::key(row, &rec);
        }
    }
    done.records.extend(retitled);
    if let Some(w) = &kept {
        done.records
            .push(super::words::words_record(w).map_err(|e| format!("{e:#}"))?);
    }
    done.text = match by_words {
        Some(_) => by_words_text(&rec),
        None => format!(
            "Updated task {} ({}): v{from} → v{}.{}",
            rec.id,
            fields.join(", "),
            rec.version,
            match (layer1.is_some(), told) {
                (true, true) => " The operator is told, with an undo.",
                (true, false) => " The operator accepted the change.",
                _ => "",
            }
        ),
    };
    done.meta = json!({"task": rec.id, "version": rec.version, "from": from, "fields": fields});
    Ok(done)
}

/// His own current words, read again under the task's lock (issue 44): the
/// kind of a layer-1 change of the owner's task that ran without his yes and
/// applies in his words; none for a change his yes ran, no layer-1 change, or
/// one his words do not carry (it is told, issue 57). A change that applies in
/// his words spends his message's receipt, under the message's own lock, in
/// the change's frame.
fn his_words<'a>(
    tc: &TurnCtx<'a>,
    rec: &TaskRecord,
    i: &UpdateIn,
    layer1: Option<&str>,
    approved: bool,
    done: &mut Done<'a>,
) -> Result<Option<super::words::Kind>, String> {
    if layer1.is_none() || approved {
        return Ok(None);
    }
    if let Some(node) = i.said.as_deref() {
        done.locks.push(super::judged::lock_said(tc, node));
    }
    let applied = super::words::applies(&super::words::Records::of(tc), rec, &i.ask());
    if let (Ok(kind), Some(node)) = (applied, i.said.as_deref()) {
        let r = super::judged::Receipt {
            node: node.into(),
            task: rec.id.clone(),
            base_version: rec.version,
            kind,
            turn: tc.turn_id.into(),
            acting: true,
            at_ms: theseus_protocol::now_unix_ms(),
        };
        done.records
            .push(super::judged::receipt_record(&r).map_err(|e| format!("{e:#}"))?);
    }
    Ok(applied.ok())
}

/// The patch applied to `rec`: the fields it changed, and a retitle's mark.
fn patched(
    tc: &TurnCtx<'_>,
    rec: &mut TaskRecord,
    patch: Patch,
) -> Result<(Vec<&'static str>, Option<NewRecord>), String> {
    let mut fields: Vec<&'static str> = vec![];
    let mut retitled = None;
    if let Some(t) = patch.title {
        rec.title = t.trim().to_string();
        fields.push("title");
        retitled = Some(retitle_mark(tc, &rec.id)?);
    }
    if let Some(d) = patch.deps {
        rec.deps = d;
        fields.push("deps");
    }
    if let Some(o) = patch.owner {
        rec.owner = o.trim().to_string();
        fields.push("owner");
    }
    if let Some(d) = &patch.due {
        if rec.kind == theseus_protocol::tasks::TaskKind::Item {
            return Err(format!(
                "task {} is a plain item: `due` belongs to a milestone or a project",
                rec.id
            ));
        }
        rec.due_at_ms = match d.trim() {
            "none" | "" => None,
            when => Some(crate::project::create::due_of(tc.kernel, when, "due")?),
        };
        fields.push("due");
    }
    if let Some(o) = patch.objective {
        rec.objective = o;
        fields.push("objective");
    }
    if let Some(a) = patch.acceptance {
        rec.acceptance = a;
        fields.push("acceptance");
    }
    Ok((fields, retitled))
}

/// What the model is told of a change made in the owner's own words: what
/// was accepted, so the reply can say it back, and the task hears it at its
/// next step.
fn by_words_text(rec: &TaskRecord) -> String {
    format!(
        "Version {} of task {}'s objective accepted, in the operator's own words: \"{}\". \
         Accepted, not yet heard: the task hears it at its next step.",
        rec.version, rec.id, rec.objective
    )
}

/// The mark that says the new title is this session's model's words, whoever wrote the one before
/// (`task_graph::title_hand`), for the frame of the edit.
fn retitle_mark(tc: &TurnCtx<'_>, task: &str) -> Result<NewRecord, String> {
    NewRecord::json(
        kinds::META,
        Some(&super::title_by_key(task)),
        &tc.session_id,
    )
    .map_err(|e| format!("{e:#}"))
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
                    ..Default::default()
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

/// `task.close`: evidence appended, and the outcome; abandoning the owner's
/// task with his yes, or told with an undo (a self-loop item's only approved).
fn close<'a>(
    tc: &TurnCtx<'a>,
    input: &Value,
    correlation_id: &str,
    approved: bool,
    said_yes: bool,
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
    let abandons = i.outcome == Outcome::Abandoned && owners;
    let keeps_card = super::undo::keeps_card(&rec);
    if abandons && keeps_card && !approved {
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
    let told = abandons && !said_yes && !keeps_card;
    let before = rec.clone();
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
    if abandons && said_yes {
        let c = change(tc, &rec, Some(from), json!({"fields": "abandon"}));
        done.row(tc, "change_accepted", c)?;
    }
    let mut detail = json!({"added": added, "evidence": rec.evidence});
    if told {
        super::undo::tell(&mut detail, &before).map_err(|e| format!("{e:#}"))?;
    }
    let c = change(tc, &rec, Some(from), detail);
    done.row(tc, "closed", c)?;
    if told {
        if let Some(row) = done.records.last_mut() {
            super::undo::key(row, &rec);
        }
    }
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
    /// `project` or `milestone` (theseus-n5ui2); an item by default.
    pub kind: theseus_protocol::tasks::TaskKind,
    /// A milestone's or project's date, in `wake.at`'s words.
    pub due: Option<&'i str>,
    /// A project's milestones, made in the same frame.
    pub milestones: &'i [theseus_protocol::projects::MilestoneSpec],
    /// A project's review: a schedule, or `none` for no review.
    pub review: Option<&'i str>,
    /// What a project's review routine is made with.
    pub ext: Option<crate::project::Ext<'i>>,
    pub title: Option<&'i str>,
    pub objective: Option<&'i str>,
    pub acceptance: &'i [String],
    pub parent: Option<&'i str>,
    pub deps: &'i [String],
    pub arrangement: Option<&'i crate::arrangement::Input>,
    /// Who started it (theseus-o1h3f.1); none is the owner's.
    pub own: Option<&'i crate::task_self::Own>,
    /// The least wait before a project's review's first run (`routine::held`).
    pub hold_ms: u64,
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
        let t = p.text.as_deref().unwrap_or(&p.first_line);
        crate::scrub::safe::head(t, 1000).0.to_string()
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
            crate::arrangement::resolve(a, &nodes, holder.as_deref(), &tc.scrubber.cuts())
                .map_err(|r| r.message)?
        }
        None => vec![],
    };
    let (objective, acceptance) = from_pieces(&pieces, item.objective, item.acceptance, title);
    let long = crate::project::create::Long::read(tc, item, &all, parent.as_deref())?;
    let rec = NewTask {
        id,
        title,
        objective,
        acceptance,
        parent,
        deps: item.deps.to_vec(),
        session: None,
        origin: {
            let mut o = TaskOrigin {
                session: tc.session_id.into(),
                principal: super::principal_of(tc.kernel, tc.execution_id),
                by_model: true,
                ..Default::default()
            };
            item.own.inspect(|w| w.apply(&mut o));
            o
        },
        state: TaskState::Accepted,
    }
    .build(theseus_protocol::now_unix_ms());
    let (rec, more, said) = long.apply(tc, rec, item)?;
    let mut done = Done::default();
    done.row(
        tc,
        "created",
        change(tc, &rec, None, json!({"pieces": pieces.len()})),
    )?;
    for m in &more {
        done.row(tc, "created", change(tc, m, None, json!({"pieces": 0})))?;
    }
    done.text = format!(
        "Recorded task {} as a plan item (no session works on it; split it, close it with its \
         evidence, or start a task session for it with `brief`): {}{said}",
        rec.id,
        super::line(&rec)
    );
    done.meta = json!({"task": rec.id, "version": rec.version, "opened": true});
    Ok(done)
}

/// The lock a layer-1 call's proposal is written under, as its question is
/// asked: the task's, before the execution's.
pub fn lock_for_call<'a>(tc: &TurnCtx<'a>, name: &str, input: &Value) -> Vec<SessionLock<'a>> {
    let Some(rec) = owners_target(tc.store, name, input) else {
        return Vec::new();
    };
    let mut locks = vec![tc.store.lock_task(&rec.id)];
    // The message a change names is held too, through the proposal's frame:
    // its shadow receipt is measured once (issue 44).
    if let Some(said) = input
        .get("said")
        .and_then(Value::as_str)
        .filter(|_| name == UPDATE)
    {
        locks.push(super::judged::lock_said(tc, said));
    }
    locks
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
    let mut detail = json!({"fields": fields, "card": card, "before": {"objective": rec.objective, "acceptance": rec.acceptance}});
    // A proposal that named his words and fell to the card says why, and
    // what the rule would have done: the cards counted, and the shadow's
    // measure (issue 44). One that would have applied spends its message's
    // receipt here, so a replay is counted as one.
    let mut spent = None;
    if name == UPDATE && input.get("said").is_some() {
        // Its message's lock is the caller's (`lock_for_call`), held until
        // this frame commits.
        let said = input["said"].as_str().unwrap_or_default();
        let (would, card) = match would(tc, input) {
            // It would apply and its judge acts, yet the card asks: another
            // rule of the gate held it, and his yes decides.
            Ok((_, true)) => ("apply", "held"),
            Ok((kind, false)) => {
                spent = Some(super::judged::receipt_record(&super::judged::Receipt {
                    node: said.into(),
                    task: rec.id.clone(),
                    base_version: rec.version,
                    kind,
                    turn: tc.turn_id.into(),
                    acting: false,
                    at_ms: theseus_protocol::now_unix_ms(),
                })?);
                ("apply", super::words::Why::Shadow.as_str())
            }
            Err(why) => ("card", why.as_str()),
        };
        detail["words"] = json!({
            "said": input["said"],
            "kind": input.get("kind").cloned().unwrap_or(Value::Null),
            "card": card,
            "would": would,
        });
    }
    let c = change(tc, &rec, None, detail);
    let mut records = vec![
        record(&rec)?,
        facts::row_of(&tc.rec(), "change_proposed", &c)?,
    ];
    records.extend(spent);
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
