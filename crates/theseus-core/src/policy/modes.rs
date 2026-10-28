//! Modes (theseus-lyxy): `ask`, `edits`, `plan` and `auto`, an overlay on the
//! config's postures for one conversation. The wire types and their words
//! are `theseus_protocol::modes`; this is the board, the gate's step and the
//! plan's two tools.
//!
//! - **`ask`** changes nothing. A session no one moved has no entry, so a
//!   new conversation, which is a new session, starts here.
//! - **`edits`** runs the file tools (`fs.edit`, `fs.write`, `fs.patch`)
//!   under the session's directory at `notify`, through scoped grants'
//!   one matching rule (`grants::covers`, an edits grant the mode holds), so
//!   its limits are the grant's: a path outside the directory, a floor path,
//!   a layer-1 change, an approve-listed path and `proc.run` still ask.
//! - **`auto`** runs what only the tool's posture made wait at `notify`, as
//!   a grant does, with the same conditions. The floor still asks.
//! - **`plan`** is read-only: a call whose class is not a read (a writer, a
//!   program, an MCP tool, a post) is refused with words the model can act on,
//!   before the order runs, and `plan_present` asks the person to approve a
//!   plan. Approving it records the steps on the session and moves the
//!   conversation to `ask`, or `edits` when the person picked that.
//!
//! A mode only ever lowers a posture the posture alone raised, so it never
//! passes the floor, `approve_paths`, the place rule's refusal, L1's own asks
//! (its egress) or any layer after the order's step for it. A shared place
//! has no mode but `ask` (`rpc/modes.rs` refuses the rest, and the gate
//! reads none for a shared place). The board is memory, one field read per
//! gated call (an atomic while no session has a mode); the record is the
//! ledger's `session.mode` rows, one per change, read back once after serving
//! (`Core::load_modes`). The plan is not on it: a plan is the task graph's
//! items, and the two tools are in the deferred `plan` family, loaded while
//! the conversation is in `plan` mode (`tools_defer`).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::modes::{PlanState, SessionMode};
use theseus_protocol::PlaceClass;
use theseus_tools::{parse, paths, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::grants::{self, Call, EDIT_TOOLS};
use super::{Decision, Notice, Posture};
use crate::task_graph::plan::{self, MAX_STEPS, MAX_STEP_CHARS};
use crate::task_graph::tools::Done;
use crate::toolrun::order::At;
use crate::toolrun::{ToolRuntime, TurnCtx};
use theseus_protocol::grants::{SessionGrant, KIND_EDITS};

/// The tool that puts a plan to the person.
pub const PLAN_PRESENT: &str = "plan.present";
/// The tool that moves a step of the approved plan.
pub const PLAN_STEP: &str = "plan.step";

/// What a refusal in plan mode begins with: `reason` words a refusal as its
/// own, apart from a place's.
pub const REFUSAL: &str = "plan mode: read-only";

/// The modes in force, per session, in memory.
#[derive(Default)]
pub struct Modes {
    inner: RwLock<BTreeMap<String, SessionMode>>,
    /// Whether any session has a mode: the gate's cheap early exit.
    any: AtomicBool,
    /// What the person picked on a plan's question, by its call's correlation
    /// id, for the tool's run to read once the call is approved. Lost on a
    /// restart between the answer and the run, which leaves `ask`.
    picks: Mutex<BTreeMap<String, SessionMode>>,
}

impl Modes {
    /// Whether any session has a mode.
    pub fn any(&self) -> bool {
        self.any.load(Ordering::Relaxed)
    }

    /// The session's mode: `ask` for one no one moved.
    pub fn mode_of(&self, session: &str) -> SessionMode {
        if !self.any() {
            return SessionMode::Ask;
        }
        self.inner
            .read()
            .unwrap()
            .get(session)
            .copied()
            .unwrap_or(SessionMode::Ask)
    }

    /// Set the session's mode; `ask` is no entry.
    pub fn set(&self, session: &str, mode: SessionMode) {
        let mut m = self.inner.write().unwrap();
        if mode == SessionMode::Ask {
            m.remove(session);
        } else {
            m.insert(session.to_string(), mode);
        }
        self.any.store(!m.is_empty(), Ordering::Relaxed);
    }

    /// The person's pick on a plan's question.
    pub fn pick(&self, correlation_id: &str, mode: SessionMode) {
        self.picks
            .lock()
            .unwrap()
            .insert(correlation_id.to_string(), mode);
    }

    /// A pick taken back: its answer was refused.
    pub(crate) fn unpick(&self, correlation_id: &str) {
        self.picks.lock().unwrap().remove(correlation_id);
    }

    fn take_pick(&self, correlation_id: &str) -> Option<SessionMode> {
        self.picks.lock().unwrap().remove(correlation_id)
    }
}

/// Whether `reason` words a refusal in plan mode.
pub fn is_refusal(why: &str) -> bool {
    why.starts_with(REFUSAL)
}

/// A refusal's `reason` in the gate's record: a place's is `place: …`, a
/// mode's `mode: …`.
pub fn refusal_reason(why: &str) -> String {
    match is_refusal(why) {
        true => format!("mode: {why}"),
        false => format!("{}: {why}", crate::toolrun::PLACE_REFUSAL),
    }
}

impl ToolRuntime {
    /// `refusal`, and then plan mode's: a read-only conversation refuses a
    /// call whose class is not a read.
    pub(crate) fn refusal_at(&self, at: &At<'_>, tool: &dyn Tool, plan: &Plan) -> Option<String> {
        self.refusal(at.place, tool.name(), plan)
            .or_else(|| plan_refusal(self, at, tool, plan))
            .or_else(|| helper_refusal(self, at, tool))
            .or_else(|| content_refusal(self, at, tool, plan))
    }
}

/// The `injection` label's refusal (theseus-9ymz0, `content::refusal`), in
/// words the model can act on: while the session is frozen, a call is refused
/// unless it is a local read of files and code (`content::local_read`): a
/// write, a program, and anything that leaves the machine, a fetch or a
/// search included. A local read, and every call in `notify` mode (the bench
/// daemon's, `[content] injection`), never reads the session's labels.
fn content_refusal(rt: &ToolRuntime, at: &At<'_>, tool: &dyn Tool, plan: &Plan) -> Option<String> {
    let class = plan.class.unwrap_or(tool.class());
    if class == ToolClass::Read && crate::content::local_read(tool.name()) {
        return None;
    }
    at.session?;
    let mode = rt.jobs.core()?.cfg.content.injection;
    if mode == crate::content::InjectionMode::Notify {
        return None;
    }
    let flags = (at.labels)();
    crate::content::refusal(flags.as_ref(), mode, class, tool.name())
}

/// A helper's refusal (theseus-z9mze.3): it reads and reports, so a call to a
/// tool outside the reader set is refused in its words, even one the model was
/// not offered.
fn helper_refusal(rt: &ToolRuntime, at: &At<'_>, tool: &dyn Tool) -> Option<String> {
    let why = crate::task_fan_out::refusal(tool)?;
    let core = rt.jobs.core()?;
    crate::task_fan_out::is_helper(&core.store, &core.kernel, at.session?).then_some(why)
}

/// Plan mode's refusal, in words the model can act on.
fn plan_refusal(rt: &ToolRuntime, at: &At<'_>, tool: &dyn Tool, plan: &Plan) -> Option<String> {
    let modes = &rt.modes;
    let session = at.session?;
    if !modes.any()
        || at.place.class != PlaceClass::Private
        || modes.mode_of(session) != SessionMode::Plan
    {
        return None;
    }
    let class = plan.class.unwrap_or(tool.class());
    (class != ToolClass::Read).then(|| {
        format!(
            "{REFUSAL}; present the plan with plan_present and nothing here runs until the person \
             approves it. {} was not run",
            tool.wire_name()
        )
    })
}

/// A call's grant-shaped view: the program as written, the argv by base
/// name, and the resources canonical.
fn call_of<'a>(
    tool: &'a str,
    program: Option<&'a str>,
    nargv: &'a [String],
    resources: &'a [std::path::PathBuf],
) -> Call<'a> {
    Call {
        tool,
        program,
        nargv,
        resources,
    }
}

/// The gate's step for a session's mode, with scoped grants' (`order.rs`):
/// `plan_present` always asks; in `edits` a file edit under the session's
/// directory, and in `auto` any call, that only the tool's posture made wait
/// runs at `notify` instead. Any other decision returns as it came, and every
/// later layer still raises it.
pub(crate) fn gate(
    rt: &ToolRuntime,
    at: &At<'_>,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    l1: bool,
    d: Decision,
) -> Decision {
    let modes = &rt.modes;
    // The plan's tools are the harness's own: the question asks, and a
    // step's mark runs quietly, whatever `[policy]` says of tools it does
    // not list (an operator's tightening still holds).
    match tool.name() {
        PLAN_PRESENT => return asks(d),
        PLAN_STEP if rt.tightened.get(PLAN_STEP).is_none() => return quiet(d),
        // A flow's call only starts its steps, each gated as the model's own.
        n if crate::flow::is_flow(n) && rt.tightened.get(n).is_none() => {
            return crate::flow::quiet(d)
        }
        _ => {}
    }
    let Some(session) = at.session else {
        return d;
    };
    if !modes.any() {
        return d;
    }
    let mode = modes.mode_of(session);
    if !matches!(mode, SessionMode::Edits | SessionMode::Auto)
        || d.posture != Posture::Approve
        || l1
        || d.floor
        || at.place.class != PlaceClass::Private
        || rt.tightened.get(tool.name()).is_some()
        || grants::has_env(input)
        || !rt.policy.posture_only(tool, plan)
    {
        return d;
    }
    if tool.name() == grants::RUN_TOOL && !rt.policy.args_within_roots(plan) {
        return d;
    }
    let (nargv, program, resources) = grants::view(plan);
    let call = call_of(tool.name(), program.as_deref(), &nargv, &resources);
    let what = match mode {
        SessionMode::Edits => {
            let root = paths::canonical_best_effort(&at.ctx.cwd);
            let held = SessionGrant {
                kind: KIND_EDITS.into(),
                root: Some(root.display().to_string()),
            };
            let inside = rt.ctx.roots.iter().any(|r| paths::within(&root, r));
            if !EDIT_TOOLS.contains(&tool.name()) || !inside || !grants::covers(&held, &call) {
                return d;
            }
            format!("edits under {}", root.display())
        }
        _ => "any call its posture alone asked about".to_string(),
    };
    let rule = format!(
        "{} — notify ({} mode: {what}, this conversation; shift+tab changes it)",
        tool.name(),
        mode.name()
    );
    Decision {
        notify: Some(Notice {
            kind: "notify".into(),
            setting: format!("ran in {} mode: {what}, this conversation", mode.name()),
            rule: rule.clone(),
        }),
        ..Decision::new(Posture::Notify, rule)
    }
}

/// `plan_step` only moves the checklist: it runs open where nothing above
/// raised it past its posture (it has no path, so the floor, the lists and
/// the roots cannot).
fn quiet(d: Decision) -> Decision {
    if d.floor || d.external.is_some() || d.posture == Posture::Open {
        return d;
    }
    Decision {
        posture: Posture::Open,
        reason: format!("{PLAN_STEP} — open (it only moves the plan's checklist)"),
        notify: None,
        ..d
    }
}

/// `plan_present` asks whatever the posture says: its question is the plan.
fn asks(d: Decision) -> Decision {
    if d.posture == Posture::Approve {
        return d;
    }
    Decision {
        posture: Posture::Approve,
        reason: format!("{PLAN_PRESENT} — approve (the plan is the question)"),
        notify: None,
        ..d
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresentInput {
    steps: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepInput {
    index: usize,
    state: String,
}

/// The steps a call names, trimmed: one to `MAX_STEPS`, none empty.
fn steps_of(input: &Value) -> Result<Vec<String>, String> {
    let i: PresentInput = parse(input)?;
    let steps: Vec<String> = i
        .steps
        .iter()
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if steps.is_empty() || steps.len() > MAX_STEPS {
        return Err(format!(
            "a plan has 1 to {MAX_STEPS} steps, one sentence each; this has {}",
            steps.len()
        ));
    }
    if let Some(i) = steps
        .iter()
        .position(|s| s.is_empty() || s.chars().count() > MAX_STEP_CHARS)
    {
        return Err(format!(
            "step {} is empty or past {MAX_STEP_CHARS} characters: one short sentence each",
            i + 1
        ));
    }
    Ok(steps)
}

/// `plan_present`: the plan, as the question the person answers.
pub struct PlanPresent;

impl Tool for PlanPresent {
    fn name(&self) -> &'static str {
        PLAN_PRESENT
    }

    fn description(&self) -> &'static str {
        "Put a plan to the person for approval, as numbered steps (one sentence each). In plan \
         mode nothing that changes files or runs a program is allowed until they approve it. \
         Approving records the steps as plan items; then do them and call plan_step as each \
         begins and ends. A declined plan stays a plan: revise it and present it again."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"steps": {
                "type": "array",
                "items": {"type": "string"},
                "description": "The steps, in order, each one sentence."
            }},
            "required": ["steps"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let steps = steps_of(input)?;
        Ok(Plan {
            summary: format!("approve this plan ({} steps)", steps.len()),
            ..Default::default()
        })
    }
}

/// `plan_step`: where one step of the plan stands.
pub struct PlanStepTool;

impl Tool for PlanStepTool {
    fn name(&self) -> &'static str {
        PLAN_STEP
    }

    fn description(&self) -> &'static str {
        "Mark a step of the plan, by its number (1 is the first): doing, done or skipped. The \
         steps are the plan items on the task board, and the person sees the checklist move."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {"type": "integer", "minimum": 1, "description": "The step's number."},
                "state": {"type": "string", "enum": ["doing", "done", "skipped"]}
            },
            "required": ["index", "state"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        // It writes a task record, as `task.close` does: a write with no path
        // runs in a group of its own, so two marks in one response keep their
        // order; and plan mode, which refuses a write, has no step to mark.
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (index, state) = step_in(input)?;
        Ok(Plan {
            summary: format!("plan step {index} is {}", state.name()),
            ..Default::default()
        })
    }
}

/// `plan_step`'s input: the step's place and the state it moves to.
fn step_in(input: &Value) -> Result<(usize, PlanState), String> {
    let i: StepInput = parse(input)?;
    let state = PlanState::parse(&i.state)
        .filter(|s| *s != PlanState::Todo)
        .ok_or_else(|| format!("state is doing, done or skipped, not {:?}", i.state))?;
    Ok((i.index, state))
}

/// Whether a tool is one of the plan's two.
pub fn is_plan(name: &str) -> bool {
    name == PLAN_PRESENT || name == PLAN_STEP
}

/// Register the plan's two tools.
pub fn register(r: &mut theseus_tools::Registry) {
    r.register(std::sync::Arc::new(PlanPresent));
    r.register(std::sync::Arc::new(PlanStepTool));
}

impl ToolRuntime {
    /// A harness call of `plan_present` or `plan_step`: its items' records
    /// ride the call's frame, and its words and meta are the call's result.
    /// Approving a plan also moves the conversation to the mode the person
    /// picked on the question, else `ask`.
    pub(crate) fn run_plan<'a>(
        &self,
        tc: &TurnCtx<'a>,
        tool: &str,
        input: &Value,
        correlation_id: &str,
    ) -> Result<Done<'a>, String> {
        if tool == PLAN_STEP {
            let (index, state) = step_in(input)?;
            return plan::step(tc, index, state);
        }
        let steps = steps_of(input)?;
        let modes = &self.modes;
        let mode = modes.take_pick(correlation_id).unwrap_or(SessionMode::Ask);
        let words = format!("this conversation is in {} mode", mode.name());
        let (done, items) = plan::create(tc, &steps, correlation_id, &words)?;
        let previous = modes.mode_of(tc.session_id);
        modes.set(tc.session_id, mode);
        tc.record(&crate::fact::modes::ModeChanged {
            session_id: tc.session_id,
            mode,
            previous: Some(previous),
            plan: &items,
            by: "plan approved",
        });
        Ok(done)
    }

    /// Tell the conversation's watchers its plan moved, once the frame that
    /// moved it is written: every surface redraws its checklist from the
    /// items as they stand. A plan item the model changed with `task_create`
    /// or `task_close` is the same plan.
    pub(crate) fn plan_moved(
        &self,
        tc: &TurnCtx<'_>,
        changes: &[(&'static str, crate::fact::task_graph::Change)],
    ) {
        if !changes.iter().any(|(_, c)| plan::is_item(&c.task)) {
            return;
        }
        let items = plan::view(tc.store, tc.session_id);
        tc.record(&crate::fact::modes::PlanMoved {
            session_id: tc.session_id,
            mode: self.modes.mode_of(tc.session_id),
            plan: &items,
            by: "plan",
        });
    }
}
