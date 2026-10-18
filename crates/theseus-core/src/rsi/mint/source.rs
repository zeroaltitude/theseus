//! The check half as instances to replay: each occurrence's recorded calls
//! read from its session, its directory rebuilt as it was at the stretch's
//! start (the whole-turn replay's own `scratch` and `undo`), its inputs read
//! back from the calls' values, and the facts its recording holds.
//!
//! Values are read from the transcript into memory for one validation and
//! never stored or reported. What the recording cannot tell, it does not
//! claim: a recording that ran programs records no changed paths (a merge's
//! files are in no record), so its paths are `None`, and the instance's
//! directory is the session's directory now with the recorded edits taken
//! back, which a program's own changes may have moved (`caveats`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use super::spec::Skeleton;
use super::validate::dry::{is_remote, Facts, Instance, Remote};
use super::validate::walk::{walk, StepResult, Steps};
use crate::flow::class;
use crate::flow::spec::InputType;
use crate::node::Body;
use crate::own_bench::whole::scratch::{self, Scratch};
use crate::own_bench::whole::undo::{self, Recorded as Call};
use crate::rpc::Core;
use crate::rsi::mine::shape::{self, normalize, Norm, Where};
use crate::rsi::mine::{Motif, Occurrence};

/// One recorded call and its result.
struct Rec {
    tool: String,
    input: Value,
    ok: bool,
    meta: Value,
    text: String,
}

/// The calls from the stretch's start to the session's end: tool, input,
/// whether it succeeded, its meta. What `undo` reads to put the directory back.
type Tail = Vec<(String, Value, bool, Value)>;

/// The steps a walk ran: id and exit code.
type Ran = Vec<(String, Option<i64>)>;

/// A step the occurrence's calls matched, in the order they ran.
struct Matched {
    step: usize,
    norm: Norm,
    rec: usize,
}

/// A runner that answers each step with the result the recording holds for
/// it, and a failure for a step it never ran. A synthetic step (the skeleton's
/// `git rerere status` after a merge) was run by no recording: it reads what
/// the step it follows printed, as the miner did, and answers as the program
/// would.
struct Recording {
    results: BTreeMap<String, StepResult>,
    synthetic: BTreeMap<String, String>,
}

impl Steps for Recording {
    async fn run(&mut self, step: &str, _tool: &str, _args: &Value) -> StepResult {
        if let Some(from) = self.synthetic.get(step) {
            let text = self.results.get(from).map_or("", |r| r.text.as_str());
            let left = shape::features(text).contains(&"rerere-left");
            return StepResult {
                ok: true,
                exit: Some(0),
                text: if left {
                    "(unmerged paths)\n".into()
                } else {
                    String::new()
                },
            };
        }
        self.results.get(step).cloned().unwrap_or(StepResult {
            ok: false,
            exit: None,
            text: "the recording never ran this step".into(),
        })
    }
}

fn argv_words(head: &str) -> Vec<String> {
    head.split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// An input's value from a raw slot value, as its type reads it.
fn input_value(ty: InputType, raw: &str) -> Value {
    match ty {
        InputType::Num => raw.parse::<i64>().map_or_else(|_| json!(raw), |n| json!(n)),
        _ => json!(raw),
    }
}

impl Core {
    /// The occurrences' calls: those whose WAL position is in `first..=last`,
    /// with their results.
    fn recorded_stretch(&self, o: &Occurrence) -> Result<(Vec<Rec>, Tail)> {
        let transcript = self.store.transcript(&o.session)?;
        let mut results: BTreeMap<String, (bool, Value, String)> = BTreeMap::new();
        for (_, n) in &transcript {
            if let Body::ToolResult {
                tool_use_id,
                is_error,
                meta,
                content,
                status,
                ..
            } = &n.body
            {
                let ok = !is_error && *status == crate::node::ResultStatus::Ok;
                results.insert(tool_use_id.clone(), (ok, meta.clone(), content.clone()));
            }
        }
        let (mut stretch, mut tail) = (Vec::new(), Vec::new());
        for (pos, n) in &transcript {
            if *pos < o.first {
                continue;
            }
            let Body::ToolCall {
                tool_use_id,
                tool,
                input,
                ..
            } = &n.body
            else {
                continue;
            };
            let (ok, meta, text) =
                results
                    .get(tool_use_id)
                    .cloned()
                    .unwrap_or((false, Value::Null, String::new()));
            tail.push((tool.clone(), input.clone(), ok, meta.clone()));
            if *pos <= o.last {
                stretch.push(Rec {
                    tool: tool.clone(),
                    input: input.clone(),
                    ok,
                    meta,
                    text,
                });
            }
        }
        Ok((stretch, tail))
    }

    /// The check half as instances, with the scratch copies that hold their
    /// directories (dropped, they are removed).
    pub(crate) async fn mint_instances(
        self: &std::sync::Arc<Self>,
        (motif, sk): (&Motif, &Skeleton),
        check: &[Occurrence],
    ) -> Result<(Vec<Instance>, Vec<Scratch>)> {
        let (mut out, mut guards) = (Vec::new(), Vec::new());
        for o in check {
            if o.turn.is_empty() {
                continue; // a bench trial has no session to rebuild
            }
            let me = self.clone();
            let (motif, sk, o) = (motif.clone(), sk.clone(), o.clone());
            let built =
                tokio::task::spawn_blocking(move || me.mint_instance(&motif, &sk, &o)).await??;
            let Some((inst, scratch)) = built else {
                continue;
            };
            guards.push(scratch);
            out.push(inst);
        }
        Ok((out, guards))
    }

    fn mint_instance(
        &self,
        m: &Motif,
        sk: &Skeleton,
        o: &Occurrence,
    ) -> Result<Option<(Instance, Scratch)>> {
        let session = self
            .store
            .get_session::<crate::session::SessionRecord>(&o.session)?
            .context("the session is gone")?;
        let cwd = self.runner.tools.cwd_for(session.dir.as_deref());
        let (stretch, tail) = self.recorded_stretch(o)?;
        let root = crate::flow::spec::repo_root(&cwd).map(|r| r.display().to_string());
        let matched = match_steps(m, &stretch, &Where { root });
        let by_step: BTreeMap<String, StepResult> = matched
            .iter()
            .map(|mt| (m.steps[mt.step].id.clone(), result_of(&stretch, mt)))
            .collect();
        let inputs = inputs_of(m, sk, &matched);
        let (exit, ran) = walked(sk, &inputs, by_step)?;
        let tests = matched
            .iter()
            .rev()
            .find(|mt| {
                m.steps[mt.step].shape.tool == "proc.run"
                    && class::test_runner(&argv_words(&m.steps[mt.step].shape.head))
            })
            .map(|mt| result_of(&stretch, mt).exit == Some(0));
        let remote: Vec<Remote> = matched
            .iter()
            .filter(|mt| m.steps[mt.step].shape.tool == "proc.run")
            .filter_map(|mt| {
                let argv = argv_words(&m.steps[mt.step].shape.head);
                is_remote(&argv).then(|| {
                    let r = result_of(&stretch, mt);
                    Remote {
                        argv,
                        exit: r.exit,
                        text: r.text,
                    }
                })
            })
            .collect();
        let (s, written) = self.rebuilt(&cwd, o, &tail)?;
        // A recording that ran programs cannot say which paths it changed.
        let programs = stretch.iter().any(|r| r.tool == "proc.run");
        let changed = (!programs).then(|| {
            written
                .iter()
                .map(|w| {
                    w.path()
                        .strip_prefix(&cwd)
                        .unwrap_or_else(|_| Path::new(""))
                        .display()
                        .to_string()
                })
                .collect()
        });
        let inst = Instance {
            id: super::brief::occurrence_id(o),
            dir: s.dir.clone(),
            inputs,
            recorded: Facts {
                exit,
                ran,
                tests,
                changed,
            },
            remote,
        };
        Ok(Some((inst, s)))
    }

    /// The directory as it was at the stretch's start: a copy of it as it is,
    /// with the recorded writes taken back.
    fn rebuilt(
        &self,
        cwd: &Path,
        o: &Occurrence,
        tail: &Tail,
    ) -> Result<(Scratch, Vec<undo::Written>)> {
        let rt = &self.runner.tools;
        let fail = |e: crate::own_bench::rebuild::NotRebuildable| {
            anyhow::anyhow!("{} ({})", e.why, e.part)
        };
        let span = undo::collect(
            tail.iter().map(|(tool, input, ok, meta)| Call {
                tool,
                input,
                ok: *ok,
                meta,
            }),
            |t| {
                rt.registry
                    .get(t)
                    .is_some_and(|t| t.class() == theseus_tools::ToolClass::Write)
            },
        )
        .map_err(fail)?;
        let max = self.cfg.route_bench.whole_tree_mb * 1024 * 1024;
        let skip: Vec<std::path::PathBuf> = rt
            .policy
            .approve_paths
            .iter()
            .chain(&rt.policy.floor_paths)
            .cloned()
            .collect();
        let s = scratch::copy(cwd, &std::env::temp_dir(), &o.turn, (max, &skip)).map_err(fail)?;
        undo::undo(cwd, &s.dir, &span.written).map_err(fail)?;
        Ok((s, span.written))
    }
}

/// Which of the motif's steps each recorded call was, in order.
fn match_steps(m: &Motif, stretch: &[Rec], w: &Where) -> Vec<Matched> {
    let (mut matched, mut cursor) = (Vec::new(), 0usize);
    for (ri, rec) in stretch.iter().enumerate() {
        for norm in normalize(&rec.tool, &rec.input, w) {
            if norm.shape.authored {
                continue;
            }
            if let Some(ix) =
                (cursor..m.steps.len()).find(|i| m.steps[*i].shape.text == norm.shape.text)
            {
                cursor = ix + 1;
                matched.push(Matched {
                    step: ix,
                    norm,
                    rec: ri,
                });
            }
        }
    }
    matched
}

/// What a matched call gave: a program's output without the runtime's header.
fn result_of(stretch: &[Rec], mt: &Matched) -> StepResult {
    let r = &stretch[mt.rec];
    let text = if r.tool == "proc.run" {
        class::program_output(&r.text).to_string()
    } else {
        r.text.clone()
    };
    StepResult {
        ok: r.ok,
        exit: r.meta.get("exit_code").and_then(Value::as_i64),
        text,
    }
}

/// The inputs, from the values the matched calls carried.
fn inputs_of(m: &Motif, sk: &Skeleton, matched: &[Matched]) -> Value {
    let mut inputs = serde_json::Map::new();
    for (name, (step_id, slot)) in &sk.from {
        let raw = matched
            .iter()
            .find(|mt| &m.steps[mt.step].id == step_id)
            .and_then(|mt| mt.norm.values.get(*slot))
            .map(|r| r.as_str().to_string());
        if let (Some(raw), Some(inp)) = (raw, sk.spec.inputs.get(name)) {
            inputs.insert(name.clone(), input_value(inp.ty, &raw));
        }
    }
    Value::Object(inputs)
}

/// The recorded path and exit: the flow walked over the recording.
fn walked(
    sk: &Skeleton,
    inputs: &Value,
    results: BTreeMap<String, StepResult>,
) -> Result<(String, Ran)> {
    let rt = tokio::runtime::Builder::new_current_thread().build()?;
    let mut rec = Recording {
        results,
        synthetic: sk.synthetic.clone(),
    };
    Ok(match rt.block_on(walk(&sk.spec, inputs, &mut rec)) {
        Ok(w) => (
            super::validate::dry::canonical(&w.exit),
            w.ran.iter().map(|r| (r.id.clone(), r.exit)).collect(),
        ),
        Err(e) => (format!("unreadable:{e}"), Vec::new()),
    })
}
