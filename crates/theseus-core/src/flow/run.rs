//! The run: a flow's steps, one at a time, each through `ToolRuntime`
//! (`ToolRuntime::flow_step`, which is the model's own path for a call), and
//! the engine over their results. Nothing here reaches a tool, the kernel or
//! the store but through that one call, which is the point.
//!
//! What ends a run, in the result the model reads:
//! - an **exit** (`exit:<name>`, or falling off the last step as `ok`): the
//!   exit's text, with the inputs filled in;
//! - a **hand-back** (`handback:<name>`): JSON, `{"handback", "step", "why",
//!   "state"}`, the state the spec's exit names (`conflict_files`,
//!   `rerere_resolved`, `output_tail`, `step`), so the model decides only
//!   what needs judgment;
//! - an engine hand-back: `step-failed` (an `err` result no `on` maps),
//!   `needs-approval` (a step whose posture asks: the exact call is in the
//!   state, and the model makes it itself, so it waits as any call waits),
//!   `step-running` (a step went on as a background job), `loop-bound` and
//!   `timeout`;
//! - a **refusal** (a step policy or the place rule refused, or a stop
//!   cancelled): the flow stops and the result is an error naming the step
//!   and the refusal.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use theseus_protocol::flow::FlowHandback;

use super::class::{self, Seen};
use super::load::Entry;
use super::spec::{self, bind, Jump, StepOut};
use crate::node::{Body, ResultStatus};
use crate::provider::ToolUse;
use crate::toolrun::{CallOutcome, Stepped, ToolRuntime, TurnCtx};

/// The most characters of an output a hand-back carries.
pub const TAIL: usize = 4_000;
/// The most characters of the whole result.
const RESULT_MAX: usize = 8_000;

/// How a run ended.
pub struct Run {
    /// `ok`, an exit's name, or `handback:<name>`: `flow.ran`'s `exit`.
    pub exit: String,
    /// The result the model reads: text and meta, or why the flow stopped.
    pub result: Result<(String, Value), String>,
    pub steps: usize,
}

/// The last `n` characters of `s`, saying so when it cut.
pub fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let kept: String = s.chars().skip(count - n).collect();
    format!("[… {} characters left out]\n{kept}", count - n)
}

/// What a run knows as it goes.
struct State<'a> {
    entry: &'a Entry,
    inputs: BTreeMap<String, Value>,
    outs: BTreeMap<String, StepOut>,
    visits: BTreeMap<usize, u32>,
    rows: Vec<Value>,
    conflicts: Vec<String>,
    left: Vec<String>,
}

impl State<'_> {
    fn meta(&self, exit: &str) -> Value {
        json!({"flow": self.entry.spec.name, "digest": self.entry.digest, "exit": exit,
               "steps": self.rows})
    }

    fn ended(&self, exit: &str, text: String) -> Run {
        Run {
            exit: exit.into(),
            result: Ok((tail(&text, RESULT_MAX), self.meta(exit))),
            steps: self.rows.len(),
        }
    }

    fn failed(&self, exit: &str, why: String) -> Run {
        Run {
            exit: exit.into(),
            result: Err(why),
            steps: self.rows.len(),
        }
    }

    /// A hand-back: the state `names` asks for, at `step`.
    fn handback(&self, name: &str, step: &str, why: String, output: &str, names: &[String]) -> Run {
        let mut state = Map::new();
        for n in names {
            match n.as_str() {
                "step" => {
                    state.insert("step".into(), json!(step));
                }
                "output_tail" => {
                    state.insert("output_tail".into(), json!(tail(output, TAIL)));
                }
                "conflict_files" => {
                    let files = if self.left.is_empty() {
                        &self.conflicts
                    } else {
                        &self.left
                    };
                    state.insert("conflict_files".into(), json!(files));
                }
                "rerere_resolved" => {
                    let done: Vec<&String> = self
                        .conflicts
                        .iter()
                        .filter(|f| !self.left.contains(f))
                        .collect();
                    state.insert("rerere_resolved".into(), json!(done));
                }
                _ => {}
            }
        }
        self.handed(FlowHandback {
            handback: name.into(),
            step: step.into(),
            why,
            state,
        })
    }

    fn handed(&self, h: FlowHandback) -> Run {
        let exit = format!("handback:{}", h.handback);
        let text = serde_json::to_string_pretty(&h).unwrap_or_default();
        Run {
            result: Ok((tail(&text, RESULT_MAX), self.meta(&exit))),
            exit,
            steps: self.rows.len(),
        }
    }

    /// An engine hand-back: the step and the tail of what it printed.
    fn engine(&self, name: &str, step: &str, why: String, output: &str) -> Run {
        self.handback(
            name,
            step,
            why,
            output,
            &["step".into(), "output_tail".into()],
        )
    }
}

/// The text and meta of the result a call wrote, newest first among the
/// store's latest nodes.
fn result_of(tc: &TurnCtx<'_>, id: &str) -> Option<(ResultStatus, String, Value)> {
    let nodes = tc.store.recent_nodes(256).ok()?;
    nodes.into_iter().rev().find_map(|(_, n)| match n.body {
        Body::ToolResult {
            tool_use_id,
            status,
            content,
            meta,
            late: false,
            ..
        } if tool_use_id == id && n.session_id == tc.session_id => Some((status, content, meta)),
        _ => None,
    })
}

/// The `assistant_node` of the response that holds call `id`: where its steps' own
/// calls hang in the inspector.
pub fn assistant_of(tc: &TurnCtx<'_>, id: &str) -> String {
    let Ok(nodes) = tc.store.recent_nodes(256) else {
        return String::new();
    };
    nodes
        .into_iter()
        .rev()
        .find_map(|(_, n)| match n.body {
            Body::ToolCall {
                tool_use_id,
                assistant_node,
                ..
            } if tool_use_id == id && n.session_id == tc.session_id => Some(assistant_node),
            _ => None,
        })
        .unwrap_or_default()
}

fn argv_of(args: &Value) -> Vec<String> {
    args.get("argv")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// What one step left, once it ran.
struct Ran {
    id: String,
    status: ResultStatus,
    exit: Option<i64>,
    /// What the program printed (its result text, for another tool).
    text: String,
    classes: Vec<&'static str>,
}

/// What the walk does next.
enum Go {
    To(usize),
    End(Run),
}

impl State<'_> {
    /// The inputs a caller gave, and the scope: or why the run does not start.
    fn begin(&mut self, rt: &ToolRuntime, tc: &TurnCtx<'_>, call: &ToolUse) -> Option<Run> {
        match self.entry.spec.resolve_inputs(&call.input) {
            Ok(i) => self.inputs = i,
            Err(e) => return Some(self.failed("invalid", e)),
        }
        rt.flow_scope(tc, &self.entry.spec)
            .err()
            .map(|e| self.failed("out-of-scope", e))
    }

    /// The caps and the step's own bound: whether step `at` may run now.
    fn may_run(&mut self, at: usize, t0: Instant) -> Option<Run> {
        let step = &self.entry.spec.steps[at];
        if self.rows.len() >= spec::MAX_STEPS {
            let why = format!("{} steps have run", spec::MAX_STEPS);
            return Some(self.engine("loop-bound", &step.id, why, ""));
        }
        if t0.elapsed() >= Duration::from_secs(super::MAX_SECS) {
            let why = format!("{} seconds have passed", super::MAX_SECS);
            return Some(self.engine("timeout", &step.id, why, ""));
        }
        let v = self.visits.entry(at).or_default();
        *v += 1;
        let visits = *v;
        (visits > step.max.unwrap_or(1)).then(|| {
            let why = format!("step {} ran {} times", step.id, visits - 1);
            self.engine("loop-bound", &step.id, why, "")
        })
    }

    /// Step `at` as the model's own call: its result, or the hand-back or
    /// refusal that ends the run in its place.
    async fn call_step(
        &mut self,
        (rt, tc): (&ToolRuntime, &TurnCtx<'_>),
        (call, assistant): (&ToolUse, &str),
        at: usize,
    ) -> Result<Ran, Run> {
        let step = &self.entry.spec.steps[at];
        let args = bind(&step.args, &self.inputs, &self.outs)
            .map_err(|e| self.engine("step-failed", &step.id, e, ""))?;
        let argv = argv_of(&args);
        let tu = ToolUse {
            id: format!("{}.{}", call.id, self.rows.len() + 1),
            name: theseus_tools::wire_name(&step.tool),
            input: args,
        };
        let began = Instant::now();
        let ran = rt.flow_step(tc, assistant, &tu).await.map_err(|e| {
            self.failed(
                "internal",
                format!("flow stopped at step {}: {e:#}", step.id),
            )
        })?;
        let status = match ran {
            Stepped::Asks { why } => return Err(self.asks(&step.id, &tu, &why)),
            Stepped::Ran(CallOutcome::Done { status }) => status,
            Stepped::Ran(CallOutcome::Background { correlation_id }) => {
                self.rows
                    .push(json!({"step": step.id, "tool": step.tool, "status": "background"}));
                let why = format!(
                    "step {} went on as a background job ({correlation_id}): read it with job_read or job_wait, then carry on by hand",
                    step.id
                );
                return Err(self.engine("step-running", &step.id, why, ""));
            }
            Stepped::Ran(CallOutcome::AwaitingConfirm { .. }) => {
                let why = "the step is waiting for approval".to_string();
                return Err(self.engine("needs-approval", &step.id, why, ""));
            }
        };
        let (status, text, meta) =
            result_of(tc, &tu.id).unwrap_or((status, String::new(), Value::Null));
        // Refused, declined or cancelled: the flow stops and says why.
        if matches!(status, ResultStatus::Declined | ResultStatus::Cancelled)
            || (status == ResultStatus::Error && text.starts_with("Not run:"))
        {
            self.rows
                .push(json!({"step": step.id, "tool": step.tool, "status": status.as_str()}));
            let why = format!(
                "flow {} stopped at step {}: {}",
                self.entry.spec.name,
                step.id,
                tail(&text, 600)
            );
            return Err(self.failed("refused", why));
        }
        let exit = meta.get("exit_code").and_then(Value::as_i64);
        // A program's output is what follows the runtime's header lines.
        let text = match step.tool.as_str() {
            "proc.run" => class::program_output(&text).to_string(),
            _ => text,
        };
        let classes = class::classes(&Seen {
            tool: &step.tool,
            argv: &argv,
            status,
            exit,
            text: &text,
        });
        self.rows.push(
            json!({"step": step.id, "tool": step.tool, "status": status.as_str(),
                              "exit": exit, "class": classes[0],
                "ms": began.elapsed().as_millis() as u64}),
        );
        Ok(Ran {
            id: step.id.clone(),
            status,
            exit,
            text,
            classes,
        })
    }

    /// A step that would wait: the exact call goes back to the model.
    fn asks(&self, step: &str, tu: &ToolUse, why: &str) -> Run {
        self.handed(FlowHandback {
            handback: "needs-approval".into(),
            step: step.into(),
            why: format!(
                "step {step} would wait for approval ({why}): make the call yourself, as it is below, and it will wait as any call of yours does"
            ),
            state: Map::from_iter([
                ("step".to_string(), json!(step)),
                ("call".to_string(), json!({"tool": tu.name, "input": tu.input})),
            ]),
        })
    }

    /// What a step's result says: remember it, then go where its class goes.
    fn after(&mut self, at: usize, ran: Ran) -> Go {
        let spec = &self.entry.spec;
        let step = &spec.steps[at];
        self.outs.insert(
            ran.id.clone(),
            StepOut {
                out: ran.text.clone(),
                exit: ran.exit,
            },
        );
        if ran.classes.contains(&"conflict") {
            for f in class::conflict_files(&ran.text) {
                if !self.conflicts.contains(&f) {
                    self.conflicts.push(f);
                }
            }
        }
        if ran.classes.contains(&"rerere-left") {
            self.left = class::listed_paths(&ran.text);
        } else if ran.classes.contains(&"rerere-resolved") {
            self.left.clear();
        }
        let jump = ran
            .classes
            .iter()
            .find_map(|c| step.on.get(*c))
            .map(|to| Jump::parse(to).unwrap_or(Jump::Exit("ok".into())));
        let succeeded = ran.status == ResultStatus::Ok && ran.exit.unwrap_or(0) == 0;
        match jump {
            Some(Jump::Goto(id)) => Go::To(spec.step(&id).map_or(at, |(i, _)| i)),
            Some(Jump::Exit(name)) => {
                Go::End(self.ended(&name, spec.exit_text(&name, &self.inputs)))
            }
            Some(Jump::Handback(name)) => {
                let names = spec.handback_state(&name);
                let why = format!("step {} ended {}", step.id, ran.classes[0]);
                Go::End(self.handback(&name, &step.id, why, &ran.text, &names))
            }
            None if !succeeded => {
                let why = format!(
                    "step {} ended {} and the flow maps no class of it",
                    step.id, ran.classes[0]
                );
                Go::End(self.engine("step-failed", &step.id, why, &ran.text))
            }
            None if at + 1 < spec.steps.len() => Go::To(at + 1),
            None => Go::End(self.ended("ok", spec.exit_text("ok", &self.inputs))),
        }
    }
}

/// Run `entry`'s flow for the call `call`, whose input is the flow's inputs.
pub async fn drive(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    entry: &Entry,
    call: &ToolUse,
    assistant: &str,
) -> Run {
    let mut st = State {
        entry,
        inputs: BTreeMap::new(),
        outs: BTreeMap::new(),
        visits: BTreeMap::new(),
        rows: Vec::new(),
        conflicts: Vec::new(),
        left: Vec::new(),
    };
    if let Some(stop) = st.begin(rt, tc, call) {
        return stop;
    }
    let (t0, mut at) = (Instant::now(), 0usize);
    loop {
        if let Some(stop) = st.may_run(at, t0) {
            return stop;
        }
        let ran = match st.call_step((rt, tc), (call, assistant), at).await {
            Ok(r) => r,
            Err(stop) => return stop,
        };
        match st.after(at, ran) {
            Go::To(next) => at = next,
            Go::End(run) => return run,
        }
    }
}
