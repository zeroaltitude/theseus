//! A flow's steps walked over any runner of steps: the engine of `flow::run`
//! without `ToolRuntime`, so the dry replay and the A/B arm can run a minted
//! flow on a scratch copy with no model and nothing deployed.
//!
//! It reads a flow as `flow::run` does (the same `bind`, the same
//! `class::classes`, the same `Jump`, the same caps), and its exits are
//! `flow::run`'s: `ok`, an exit's name, or `handback:<name>`. What it leaves
//! out is the gate: the runner is the caller's, and the real flow's steps are
//! gated as the model's own calls (that is `flow::run`'s whole point). The
//! transitions are written twice, here and in `flow/run.rs`'s `after`: a
//! change to one is a change to the other, and `tests_walk` holds this copy
//! to the cases `flow/tests_run.rs` holds the real one to.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::flow::class::{self, Seen};
use crate::flow::spec::{self, bind, Jump, Spec, StepOut};
use crate::node::ResultStatus;

/// What a runner gives back for one step.
#[derive(Debug, Clone)]
pub struct StepResult {
    /// The step ran and did not fail as a call (a program's exit is `exit`).
    pub ok: bool,
    pub exit: Option<i64>,
    /// What the step printed, with no header or footer lines of the runner's.
    pub text: String,
}

/// Something that runs one step: the step's id, its tool and its bound arguments.
pub trait Steps {
    fn run(
        &mut self,
        step: &str,
        tool: &str,
        args: &Value,
    ) -> impl std::future::Future<Output = StepResult>;
}

/// One step that ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub id: String,
    pub exit: Option<i64>,
    /// The program's argv, for `proc.run`.
    pub argv: Vec<String>,
}

/// How a walk went.
#[derive(Debug, Clone, PartialEq)]
pub struct Walk {
    /// `ok`, an exit's name, or `handback:<name>`.
    pub exit: String,
    /// Each step run, in order.
    pub ran: Vec<Ran>,
    /// What the model would read.
    pub text: String,
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

fn handback(name: &str, step: &str, why: &str, tail: &str) -> (String, String) {
    let text = json!({"handback": name, "step": step, "why": why,
                      "state": {"step": step, "output_tail": crate::flow::run::tail(tail, 4000)}});
    (
        format!("handback:{name}"),
        serde_json::to_string_pretty(&text).unwrap_or_default(),
    )
}

/// Walk `spec` with the caller's `given` inputs over `runner`. Fails (the
/// flow does not start) when the inputs do not resolve.
pub async fn walk<S: Steps>(spec: &Spec, given: &Value, runner: &mut S) -> Result<Walk, String> {
    let inputs = spec.resolve_inputs(given)?;
    let mut outs: BTreeMap<String, StepOut> = BTreeMap::new();
    let mut visits: BTreeMap<usize, u32> = BTreeMap::new();
    let mut ran: Vec<Ran> = Vec::new();
    let (t0, mut at) = (Instant::now(), 0usize);
    let end = |ran: Vec<Ran>, exit: String, text: String| Ok(Walk { exit, ran, text });
    loop {
        let step = &spec.steps[at];
        if ran.len() >= spec::MAX_STEPS {
            let (e, t) = handback("loop-bound", &step.id, "too many steps ran", "");
            return end(ran, e, t);
        }
        if t0.elapsed() >= Duration::from_secs(crate::flow::MAX_SECS) {
            let (e, t) = handback("timeout", &step.id, "the flow ran too long", "");
            return end(ran, e, t);
        }
        let v = visits.entry(at).or_default();
        *v += 1;
        if *v > step.max.unwrap_or(1) {
            let (e, t) = handback("loop-bound", &step.id, "the step ran too often", "");
            return end(ran, e, t);
        }
        let args = match bind(&step.args, &inputs, &outs) {
            Ok(a) => a,
            Err(why) => {
                let (e, t) = handback("step-failed", &step.id, &why, "");
                return end(ran, e, t);
            }
        };
        let argv = argv_of(&args);
        let r = runner.run(&step.id, &step.tool, &args).await;
        ran.push(Ran {
            id: step.id.clone(),
            exit: r.exit,
            argv: argv.clone(),
        });
        let status = if r.ok {
            ResultStatus::Ok
        } else {
            ResultStatus::Error
        };
        let classes = class::classes(&Seen {
            tool: &step.tool,
            argv: &argv,
            status,
            exit: r.exit,
            text: &r.text,
        });
        outs.insert(
            step.id.clone(),
            StepOut {
                out: r.text.clone(),
                exit: r.exit,
            },
        );
        let jump = classes
            .iter()
            .find_map(|c| step.on.get(*c))
            .map(|to| Jump::parse(to).unwrap_or(Jump::Exit("ok".into())));
        let succeeded = r.ok && r.exit.unwrap_or(0) == 0;
        match jump {
            Some(Jump::Goto(id)) => at = spec.step(&id).map_or(at, |(i, _)| i),
            Some(Jump::Exit(name)) => {
                let text = spec.exit_text(&name, &inputs);
                return end(ran, name, text);
            }
            Some(Jump::Handback(name)) => {
                let why = format!("step {} ended {}", step.id, classes[0]);
                let (e, t) = handback(&name, &step.id, &why, &r.text);
                return end(ran, e, t);
            }
            None if !succeeded => {
                let why = format!(
                    "step {} ended {} and the flow maps no class of it",
                    step.id, classes[0]
                );
                let (e, t) = handback("step-failed", &step.id, &why, &r.text);
                return end(ran, e, t);
            }
            None if at + 1 < spec.steps.len() => at += 1,
            None => {
                let text = spec.exit_text("ok", &inputs);
                return end(ran, "ok".into(), text);
            }
        }
    }
}
