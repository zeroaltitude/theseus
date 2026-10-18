//! The replayed model's hands (own-benchmark part (c)): what each of its
//! calls does. Three kinds, and nothing between them:
//!
//! - **On the copy.** `fs.read|glob|grep|list|write|edit|patch`, `git.diff|log`
//!   and `text.diff` run in process on the scratch copy, with the roots and the
//!   working directory set to it, and a call whose plan names a path outside
//!   it is refused. The model's paths into the real directory are mapped into
//!   the copy, and the copy's back out of the result, so it sees one world.
//! - **A program.** `proc.run` runs in L1 over the copy (a view built from an
//!   empty root; the copy is the only workspace; its writes go to L1's scratch
//!   and are discarded; egress is `[sandbox] egress` and nothing else; the
//!   environment is `PATH`, `HOME` and what the call itself named). L1 refuses
//!   a root daemon's job, and the call then fails with that reason: nothing
//!   falls back to running it outside L1.
//! - **A stub.** Every other tool would reach the owner, the world, or the
//!   daemon's state, and runs nothing: it returns the result the original turn
//!   recorded for that tool when it called it, else a sentence saying nothing
//!   was done. The row names each.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use theseus_kernel::job::L1;
use theseus_kernel::probe_run::ProbeRun;
use theseus_protocol::own_bench::WholeCall;
use theseus_tools::{Tool, ToolCtx};

use super::scratch::{remap, remap_json};
use crate::toolrun::ToolRuntime;

/// Tools that run on the copy.
pub const ON_THE_COPY: [&str; 10] = [
    "fs.read",
    "fs.glob",
    "fs.grep",
    "fs.list",
    "fs.write",
    "fs.edit",
    "fs.patch",
    "git.diff",
    "git.log",
    "text.diff",
];

/// The program a call runs.
pub const PROGRAM: &str = "proc.run";

/// The most of a program's output kept, per stream.
const OUTPUT_BYTES: usize = 64 * 1024;

/// How programs are run: L1, and in a test a stand-in.
pub trait Programs: Send + Sync {
    fn run(
        &self,
        l1: &L1,
        argv: &[String],
        cwd: &Path,
        env: &[(String, String)],
        deadline_ms: u64,
    ) -> ProbeRun;
}

/// L1, as a watch's command runs (`theseus_kernel::probe_run`).
pub struct InL1;

impl Programs for InL1 {
    fn run(
        &self,
        l1: &L1,
        argv: &[String],
        cwd: &Path,
        env: &[(String, String)],
        deadline_ms: u64,
    ) -> ProbeRun {
        theseus_kernel::probe_run::run_in(l1, argv, (cwd, env), "replay", deadline_ms, OUTPUT_BYTES)
    }
}

/// A result the original turn recorded, kept to stand in for the same tool.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub tool: String,
    pub content: String,
}

/// One run of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub argv: Vec<String>,
    pub exit: Option<i32>,
}

/// What a call gave the model.
pub struct Answer {
    pub text: String,
    pub is_error: bool,
}

pub struct Hands<'a> {
    pub rt: &'a ToolRuntime,
    pub orig: PathBuf,
    pub scratch: PathBuf,
    recorded: Vec<Recorded>,
    l1: L1,
    programs: Arc<dyn Programs>,
    result_max: usize,
    /// Seconds the replay may still run, for a program's deadline.
    pub seconds_left: u64,
    pub calls: Vec<WholeCall>,
    pub ran: Vec<Ran>,
    pub changed: BTreeSet<String>,
}

impl<'a> Hands<'a> {
    pub fn new(
        rt: &'a ToolRuntime,
        (orig, scratch): (PathBuf, PathBuf),
        recorded: Vec<Recorded>,
        programs: Arc<dyn Programs>,
    ) -> Self {
        let mut l1 = rt.sandbox.job_view();
        // The copy is the whole workspace: the real directory is not in view.
        l1.workspace = vec![scratch.clone()];
        l1.egress = rt.sandbox.cfg.egress.clone();
        Self {
            rt,
            orig,
            scratch,
            recorded,
            l1,
            programs,
            result_max: rt.result_max_chars,
            seconds_left: 300,
            calls: Vec::new(),
            ran: Vec::new(),
            changed: BTreeSet::new(),
        }
    }

    fn ctx(&self) -> ToolCtx {
        let mut ctx = self.rt.ctx.clone();
        ctx.roots = vec![self.scratch.clone()];
        ctx.cwd = self.scratch.clone();
        ctx.floor = Vec::new();
        ctx.secrets = None;
        ctx.aws = None;
        ctx.cores = None;
        ctx.approved = false;
        ctx
    }

    /// Answers one call by the tool's wire name.
    pub async fn call(&mut self, wire: &str, input: &Value) -> Answer {
        let Some(tool) = self.rt.registry.by_wire(wire).cloned() else {
            self.note(wire, "refused", None);
            return Answer {
                text: format!("Not run in the replay: there is no tool named {wire}."),
                is_error: true,
            };
        };
        let name = tool.name().to_string();
        let input = remap_json(input, &self.orig, &self.scratch);
        let answer = if name == PROGRAM {
            self.program(tool, &input).await
        } else if ON_THE_COPY.contains(&name.as_str()) {
            self.on_the_copy(tool, &input).await
        } else {
            self.stub(&name)
        };
        Answer {
            text: self.shown(&answer.text),
            is_error: answer.is_error,
        }
    }

    fn note(&mut self, tool: &str, how: &str, exit: Option<i32>) {
        self.note_why(tool, how, exit, None);
    }

    fn note_why(&mut self, tool: &str, how: &str, exit: Option<i32>, why: Option<String>) {
        self.calls.push(WholeCall {
            tool: tool.into(),
            how: how.into(),
            exit,
            why,
        });
    }

    /// Text for the model: the copy's paths as the real directory's, cut at
    /// the runtime's result cap.
    fn shown(&self, text: &str) -> String {
        let text = remap(text, &self.scratch, &self.orig);
        if text.chars().count() <= self.result_max {
            return text;
        }
        let kept: String = text.chars().take(self.result_max).collect();
        format!(
            "{kept}\n[cut: {} characters left out of this replayed result]",
            text.chars().count() - self.result_max
        )
    }

    fn stub(&mut self, name: &str) -> Answer {
        self.note(name, "stub", None);
        let at = self.recorded.iter().position(|r| r.tool == name);
        let text = match at {
            Some(i) => self.recorded.remove(i).content,
            None => format!(
                "Not run: {name} would reach outside the replay (a message, a task, a wake, a \
                 fetch or the daemon's own state), so nothing was done."
            ),
        };
        Answer {
            text,
            is_error: false,
        }
    }

    /// Whether every path a plan names lies in the copy.
    fn inside(&self, tool: &dyn Tool, input: &Value, ctx: &ToolCtx) -> Result<(), String> {
        let plan = tool
            .plan(input, ctx)
            .map_err(|e| format!("invalid input: {e}"))?;
        let root = theseus_tools::paths::canonical_best_effort(&self.scratch);
        for r in &plan.resources {
            let p = theseus_tools::paths::canonical_best_effort(&r.path);
            if !p.starts_with(&root) {
                return Err(format!(
                    "Not run in the replay: {} is outside the directory the replay copied.",
                    remap(&p.display().to_string(), &self.scratch, &self.orig)
                ));
            }
        }
        Ok(())
    }

    async fn on_the_copy(&mut self, tool: Arc<dyn Tool>, input: &Value) -> Answer {
        let ctx = self.ctx();
        if let Err(why) = self.inside(tool.as_ref(), input, &ctx) {
            self.note(tool.name(), "refused", None);
            return Answer {
                text: why,
                is_error: true,
            };
        }
        let (t, i, c) = (tool.clone(), input.clone(), ctx);
        let ran = tokio::task::spawn_blocking(move || t.run_with_media(&i, &c)).await;
        let (text, meta, is_error) = match ran {
            Ok(Ok((out, _media))) => (out.text, out.meta, false),
            Ok(Err(f)) => (f.message, f.meta, true),
            Err(e) => (
                format!("the replayed call did not finish: {e}"),
                Value::Null,
                true,
            ),
        };
        self.note(tool.name(), "ran", None);
        if !is_error && matches!(tool.name(), "fs.write" | "fs.edit") {
            if let Some(p) = meta.get("path").and_then(Value::as_str) {
                let rel = Path::new(p)
                    .strip_prefix(&self.scratch)
                    .unwrap_or(Path::new(p));
                self.changed.insert(rel.display().to_string());
            }
        }
        Answer { text, is_error }
    }

    async fn program(&mut self, tool: Arc<dyn Tool>, input: &Value) -> Answer {
        let ctx = self.ctx();
        let specs = match tool.jobs(input, &ctx) {
            Ok(s) => s,
            Err(e) => {
                self.note(PROGRAM, "refused", None);
                return Answer {
                    text: format!("invalid input: {e}"),
                    is_error: true,
                };
            }
        };
        let root = theseus_tools::paths::canonical_best_effort(&self.scratch);
        let mut text = String::new();
        let mut failed = false;
        for spec in specs {
            let cwd = theseus_tools::paths::canonical_best_effort(&spec.cwd);
            if !cwd.starts_with(&root) {
                self.note(PROGRAM, "refused", None);
                return Answer {
                    text: format!(
                        "Not run in the replay: {} is outside the directory the replay copied.",
                        remap(&cwd.display().to_string(), &self.scratch, &self.orig)
                    ),
                    is_error: true,
                };
            }
            let deadline_ms = spec.timeout_secs.min(self.seconds_left).max(1) * 1000;
            let (programs, l1, argv, env) = (
                self.programs.clone(),
                self.l1.clone(),
                spec.argv.clone(),
                spec.env.clone(),
            );
            let run = tokio::task::spawn_blocking(move || {
                programs.run(&l1, &argv, &cwd, &env, deadline_ms)
            })
            .await
            .unwrap_or_else(|e| ProbeRun {
                code: None,
                stdout: String::new(),
                stderr: String::new(),
                failed: Some(format!("the program's run did not finish: {e}")),
            });
            // `refused` is a policy's answer (L1 will not run a root daemon's job); a program
            // that could not start, or ran out of time, `failed`, and says why.
            let why = run.code.is_none().then(|| {
                run.failed
                    .clone()
                    .unwrap_or_else(|| "ended by a signal".into())
            });
            let how = match &why {
                None => "ran",
                Some(w) if w.contains(theseus_sandbox::ROOT_REFUSED) => "refused",
                Some(_) => "failed",
            };
            self.note_why(PROGRAM, how, run.code, why);
            self.ran.push(Ran {
                argv: spec.argv.clone(),
                exit: run.code,
            });
            text.push_str(&format!(
                "$ {}\n{}{}",
                spec.argv.join(" "),
                run.stdout,
                if run.stderr.is_empty() {
                    String::new()
                } else {
                    format!("[stderr]\n{}", run.stderr)
                }
            ));
            match (run.code, run.failed) {
                (Some(0), _) => text.push_str("\n[exit 0]\n"),
                (Some(c), _) => {
                    text.push_str(&format!("\n[exit {c}]\n"));
                    failed = true;
                }
                (None, why) => {
                    text.push_str(&format!(
                        "\n[did not run: {}]\n",
                        why.unwrap_or_else(|| "ended by a signal".into())
                    ));
                    failed = true;
                }
            }
            if failed {
                break;
            }
        }
        Answer {
            text,
            is_error: failed,
        }
    }
}
