//! Tool calls (spec §3.16, §3.17, §3.23): every call the model makes becomes a
//! kernel action. The ordering is the gate's: the toollet validates its own
//! input and names what it will touch; policy says run, notify, or wait;
//! a confirm is bound to the exact proposal; authorization re-checks the
//! digest; dispatch is committed before anything runs. The `ToolCall` node is
//! written in the same frame as the `planned` transition and an in-process
//! `ToolResult` in the same frame as the settlement, so the transcript and the
//! kernel never disagree about what happened.
//!
//! `proc.run` runs through the detached job wrapper. A turn waits for it up to
//! `proc_sync_secs`; past that the model gets a background placeholder and the
//! real result arrives as a late message when the job settles, even across a
//! daemon restart (the spool keeps it).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_kernel::job::{spawn_detached, WrapperArgs};
use theseus_kernel::{
    Action, ActionState, Completion, Kernel, Outcome, Proposal, RetryClass, Spool, TurnGuard,
    BUDGET_TOOL, PROVIDER_TOOL,
};
use theseus_protocol::{notify, ConfirmRequest};
use theseus_store::Store as _;
use theseus_tools::{Backend, JobSpec, Plan, Registry, Retry, Tool, ToolClass, ToolCtx};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::narrative::{self, narrate_turn, Narrator};
use crate::node::{Body, Node, ResultStatus};
use crate::policy::{Decision, Posture, ToolPolicy};
use crate::provider::ToolUse;
use crate::scrub::Scrubber;
use crate::store::Store;

/// Starts a job. The real one spawns `theseusd job-wrapper` detached; tests
/// substitute one that runs the command on a thread and spools the result.
pub trait JobLauncher: Send + Sync {
    fn launch(&self, spool: &Spool, args: &WrapperArgs) -> Result<u32>;
}

pub struct WrapperLauncher {
    pub self_exe: PathBuf,
}

impl JobLauncher for WrapperLauncher {
    fn launch(&self, spool: &Spool, args: &WrapperArgs) -> Result<u32> {
        spawn_detached(
            &self.self_exe,
            &[theseus_kernel::job::WRAPPER_MODE],
            spool,
            args,
        )
    }
}

/// Runs the wrapper's body on a thread in this process (tests).
pub struct InlineLauncher;

impl JobLauncher for InlineLauncher {
    fn launch(&self, _spool: &Spool, args: &WrapperArgs) -> Result<u32> {
        let a = args.clone();
        std::thread::spawn(move || {
            let _ = theseus_kernel::job::run_wrapper(&a);
        });
        Ok(std::process::id())
    }
}

/// What a turn hands the tool layer for one call.
pub struct TurnCtx<'a> {
    pub kernel: &'a Kernel,
    pub store: &'a Store,
    pub guard: &'a TurnGuard,
    pub session_id: &'a str,
    pub execution_id: &'a str,
    pub turn_id: &'a str,
    pub loop_index: Option<u32>,
    pub sink: &'a EventSink,
    pub confirm_ttl_ms: u64,
    pub narrator: &'a Narrator,
}

impl TurnCtx<'_> {
    /// A ledger row for this turn. It is no state transition, so it rides in
    /// the turn's next frame (theseus-qa0). A row that cannot be encoded is
    /// logged, not fatal.
    pub fn ledger(&self, kind: &str, data: Value) {
        if let Err(e) = self
            .ledger_record(kind, data)
            .and_then(|r| self.store.defer(r))
        {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    /// A ledger row for this turn, as a record for a frame the caller builds.
    pub fn ledger_record(&self, kind: &str, data: Value) -> Result<theseus_store::NewRecord> {
        theseus_store::NewRecord::json(
            theseus_store::kinds::LEDGER,
            None,
            &LedgerRow::new(kind, Some(self.session_id), Some(self.turn_id), data),
        )
    }

    /// Tell the session's clients about a node this turn wrote.
    pub fn node_written(&self, node: &Node) {
        self.sink.send(
            notify::NODE_WRITTEN,
            json!({"session_id": self.session_id, "node_id": node.id, "kind": node.kind_str()}),
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallOutcome {
    /// A result node was written.
    Done { status: ResultStatus },
    /// The call waits for the operator; the turn must park.
    AwaitingConfirm { correlation_id: String },
    /// Still running as a job; a placeholder result was written.
    Background { correlation_id: String },
}

/// A `tool_use` as the model sent it.
pub struct Call<'a> {
    pub call: &'a ToolUse,
    /// Its raw input, when that was not valid JSON.
    pub invalid: Option<&'a str>,
}

/// What `run_calls` did with a response's calls (theseus-a60).
pub struct Batch {
    /// Every call that has an answer, or asked, in call order. The calls
    /// after the one that asked are not here: they were never gated.
    pub ran: Vec<Ran>,
    /// The call that waits for the operator.
    pub awaiting: Option<String>,
}

/// One call of a batch: what became of it, and when it ran.
pub struct Ran {
    /// Its place among the calls.
    pub index: usize,
    pub outcome: CallOutcome,
    pub started: Instant,
    pub ended: Instant,
    /// The group it ran in, numbered in the order the groups ran. A call
    /// answered at the gate, a write, a program, and a question are each a
    /// group of one.
    pub group: usize,
}

/// A call through the gate.
enum Admitted {
    /// Answered there: an unknown tool, or invalid input.
    Answered(CallOutcome),
    /// The policy runs it.
    Runs(Arc<dyn Tool>, Gated),
    /// It waits for the operator.
    Asks(Arc<dyn Tool>, Gated),
}

#[derive(Debug, Default, Clone)]
pub struct ResumeOutcome {
    /// Result nodes written (the model has something new to read).
    pub wrote: u32,
    /// A confirm is still pending and no input superseded it.
    pub awaiting: Option<String>,
    pub background: Vec<String>,
}

pub struct ToolRuntime {
    pub registry: Registry,
    pub policy: ToolPolicy,
    pub ctx: ToolCtx,
    pub spool: Option<Spool>,
    pub scrubber: Arc<Scrubber>,
    pub launcher: Arc<dyn JobLauncher>,
    pub notify_socket: Option<PathBuf>,
    pub result_max_chars: usize,
    pub proc_sync_secs: u64,
    /// The environment every job gets, resolved from the daemon's at startup.
    pub proc_env: Vec<(String, String)>,
    /// Calls per tool since the daemon started. Counting the store's history
    /// instead would put a scan of every node on the start path (§9).
    pub calls: Mutex<BTreeMap<String, u64>>,
    /// "Should have asked" (theseus-sgh): the tools that ask first because
    /// someone pressed it, from the store. The gate reads them per call.
    pub tightened: crate::tighten::Tightenings,
    /// The daemon's cores (theseus-a60): every in-process toollet runs under
    /// one of its permits.
    pub cpu: Arc<crate::cpu::CpuPool>,
}

const INPROC_DEADLINE_MS: u64 = 120_000;
const MAX_RESULT_READ: usize = 4 * 1024 * 1024;

/// Environment names a model may never set on a job.
fn forbidden_env(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.starts_with("OP_")
        || n.starts_with("AWS_")
        || n.starts_with("ANTHROPIC")
        || n.starts_with("THESEUS")
        || n.contains("TOKEN")
        || n.contains("SECRET")
        || n.contains("PASSWORD")
        || n.contains("KEY")
        || n == "LD_PRELOAD"
        || n == "LD_LIBRARY_PATH"
}

pub fn cap(text: &str, max: usize) -> (String, bool) {
    let n = text.chars().count();
    if n <= max || max < 64 {
        return (text.to_string(), false);
    }
    let head = max * 6 / 10;
    let tail = max - head;
    let h: String = text.chars().take(head).collect();
    let t: String = text.chars().skip(n - tail).collect();
    (
        format!(
            "{h}\n…[{} characters omitted; the full output is stored]…\n{t}",
            n - head - tail
        ),
        true,
    )
}

fn read_result_file(path: Option<&str>) -> (String, u64) {
    let Some(p) = path else {
        return (String::new(), 0);
    };
    match std::fs::read(p) {
        Ok(b) => {
            let total = b.len() as u64;
            let slice = if b.len() > MAX_RESULT_READ {
                &b[b.len() - MAX_RESULT_READ..]
            } else {
                &b[..]
            };
            (String::from_utf8_lossy(slice).into_owned(), total)
        }
        Err(_) => (String::new(), 0),
    }
}

impl ToolRuntime {
    pub fn disabled() -> Self {
        let tmp = std::env::temp_dir();
        Self {
            registry: Registry::new(),
            // No tools to call; were one to appear, it would wait.
            policy: ToolPolicy {
                roots: vec![],
                approve_paths: vec![],
                allow_argv: vec![],
                approve_argv: vec![],
                enforcement: crate::policy::Posture::Approve,
                tools: BTreeMap::new(),
                mcp: BTreeMap::new(),
                confirmer: "operator".into(),
                floor_paths: vec![],
                floor_argv: crate::policy::floor_argv(),
            },
            ctx: ToolCtx::for_tests(&tmp),
            spool: None,
            scrubber: Arc::new(Scrubber::default()),
            launcher: Arc::new(InlineLauncher),
            notify_socket: None,
            result_max_chars: 30_000,
            proc_sync_secs: 60,
            proc_env: vec![],
            calls: Mutex::new(BTreeMap::new()),
            tightened: Default::default(),
            cpu: crate::cpu::CpuPool::for_host(),
        }
    }

    /// A tool's posture now: the config's, or its tightening's when that is
    /// stricter (theseus-sgh).
    pub fn posture_now(&self, name: &str) -> crate::policy::PostureNow {
        let t = self.tightened.get(name);
        self.policy
            .posture_now(name, t.as_ref().map(crate::tighten::as_tightened))
    }

    pub fn enabled(&self) -> bool {
        !self.registry.is_empty()
    }

    pub fn definitions(&self) -> Vec<Value> {
        self.registry.definitions(true)
    }

    /// The paragraph of the system prompt that describes the tools and their
    /// limits. Deterministic for a given config, so it never churns the cache.
    pub fn system_note(&self) -> String {
        if !self.enabled() {
            return String::new();
        }
        let roots: Vec<String> = self
            .ctx
            .roots
            .iter()
            .map(|r| r.display().to_string())
            .collect();
        let allowed: Vec<String> = self.policy.allow_argv.iter().map(|a| a.join(" ")).collect();
        // Each tool's posture, grouped in ladder order: "open: fs.glob, …; notify: …".
        let postures: Vec<String> = crate::policy::Posture::ALL
            .iter()
            .filter_map(|p| {
                let names: Vec<&str> = self
                    .registry
                    .all()
                    .map(|t| t.name())
                    .filter(|n| self.policy.posture(n).0 == *p)
                    .collect();
                (!names.is_empty()).then(|| format!("{}: {}", p.as_str(), names.join(", ")))
            })
            .collect();
        format!(
            "Tools. You act through tools; every call is recorded, checked against policy, and may wait for the operator's confirmation.\n\
             - Workspace roots: {}. A path outside them, or on the operator's approve list, waits for the operator's approval.\n\
             - Relative paths resolve against {}.\n\
             - Postures (open runs; notify runs and tells the operator; approve waits for the operator's approval): {}.{}\n\
             - Prefer fs_read, fs_edit, fs_grep, fs_glob, fs_list, git_diff, and git_log over proc_run. proc_run runs one program with a typed argv and no shell; pass [\"bash\", \"-c\", \"...\"] explicitly only when a shell is truly needed.\n\
             - Read a file before editing it; keep edits exact and minimal.\n\
             - A declined call is final for that request: tell the operator and do not route around it.\n\
             - proc_run calls that take longer than {} seconds continue in the background; their result arrives in a later message.",
            roots.join(", "),
            self.ctx.cwd.display(),
            postures.join("; "),
            if allowed.is_empty() { String::new() } else { format!(" proc_run runs these as open: {}.", allowed.join("; ")) },
            self.proc_sync_secs,
        )
    }

    fn count(&self, tool: &str) {
        *self
            .calls
            .lock()
            .unwrap()
            .entry(tool.to_string())
            .or_default() += 1;
    }

    /// The node for a result, its text scrubbed of secret values and capped.
    fn result_node(&self, tc: &TurnCtx<'_>, r: ResultNode<'_>) -> Node {
        let (scrubbed, redactions) = self.scrubber.scrub(&r.text);
        let (content, truncated) = cap(&scrubbed, self.result_max_chars);
        let mut meta = r.meta;
        if redactions > 0 {
            meta["redactions"] = json!(redactions);
        }
        Node::tool_result(
            tc.session_id,
            Some(tc.turn_id),
            tc.loop_index,
            Body::ToolResult {
                tool_use_id: r.tool_use_id.into(),
                tool: r.tool.into(),
                status: r.status,
                is_error: matches!(
                    r.status,
                    ResultStatus::Error
                        | ResultStatus::Declined
                        | ResultStatus::Unknown
                        | ResultStatus::Cancelled
                ),
                content,
                correlation_id: r.correlation_id.map(str::to_string),
                bytes_total: r.bytes_total.unwrap_or(r.text.len() as u64),
                truncated,
                full_ref: r.full_ref,
                duration_ms: r.duration_ms,
                late: r.late,
                meta,
                image: r.image,
            },
        )
    }

    /// A result on its own frame, announced.
    fn answer(&self, tc: &TurnCtx<'_>, r: ResultNode<'_>) -> Result<ResultStatus> {
        Self::write_result(tc, &self.result_node(tc, r))
    }

    fn announce_end(tc: &TurnCtx<'_>, node: &Node) {
        if let Body::ToolResult {
            tool_use_id,
            tool,
            status,
            duration_ms,
            correlation_id,
            late,
            truncated,
            bytes_total,
            content,
            meta,
            ..
        } = &node.body
        {
            tc.sink.send(
                notify::TOOL_ENDED,
                json!({
                    "session_id": tc.session_id,
                    "turn_id": tc.turn_id,
                    "tool_use_id": tool_use_id,
                    "tool": tool,
                    "status": status.as_str(),
                    "duration_ms": duration_ms,
                    "correlation_id": correlation_id,
                    "late": late,
                    "truncated": truncated,
                    "bytes": bytes_total,
                    "node_id": node.id,
                    "exit_code": meta.get("exit_code"),
                    "preview": content.chars().take(2000).collect::<String>(),
                }),
            );
            if tc.narrator.on() {
                narrate_result(tc, node);
            }
        }
        tc.node_written(node);
    }

    /// A call's main resource for the narrative, scrubbed of any secret value.
    fn subject(&self, tool: &str, plan: &Plan) -> String {
        let s = narrative::subject(
            tool,
            plan.argv.as_deref(),
            plan.resources.first().map(|r| r.path.as_path()),
            &self.ctx.cwd,
        );
        self.scrubber.scrub(&s).0
    }

    /// Write a result node on its own frame and announce it.
    fn write_result(tc: &TurnCtx<'_>, node: &Node) -> Result<ResultStatus> {
        let status = match &node.body {
            Body::ToolResult { status, .. } => *status,
            _ => ResultStatus::Error,
        };
        tc.store.append(&[node.record()?])?;
        Self::announce_end(tc, node);
        Ok(status)
    }

    /// A call's tool, if it is still registered, and its canonical name (the
    /// wire name when it is not).
    fn tool_of(&self, call: &ToolUse) -> (Option<Arc<dyn Tool>>, String) {
        let tool = self.registry.by_wire(&call.name).cloned();
        let name = tool
            .as_ref()
            .map(|t| t.name().to_string())
            .unwrap_or_else(|| call.name.clone());
        (tool, name)
    }

    /// Answer a `tool_use` that will not run (the response was cut off, the
    /// operator moved on), so the transcript stays valid.
    pub fn not_run(&self, tc: &TurnCtx<'_>, call: &ToolUse, reason: &str) -> Result<()> {
        let (_, tool) = self.tool_of(call);
        self.answer(
            tc,
            ResultNode {
                meta: json!({"not_run": reason}),
                ..ResultNode::new(
                    &call.id,
                    &tool,
                    ResultStatus::Cancelled,
                    format!("Not run: {reason}."),
                )
            },
        )?;
        Ok(())
    }

    fn proposal_for(&self, tool: &dyn Tool, input: &Value) -> Proposal {
        Proposal {
            tool: tool.name().into(),
            args: input.clone(),
            resource: None,
            policy_context: json!({"roots": self.ctx.roots, "cwd": self.ctx.cwd}),
        }
    }

    fn tool_call_node(
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
        tool: &str,
        correlation_id: Option<&str>,
        gate: Value,
    ) -> Node {
        Node::tool_call(
            tc.session_id,
            Some(tc.turn_id),
            tc.loop_index,
            Body::ToolCall {
                tool_use_id: call.id.clone(),
                tool: tool.into(),
                wire_name: call.name.clone(),
                input: call.input.clone(),
                assistant_node: assistant_node.into(),
                correlation_id: correlation_id.map(str::to_string),
                gate,
            },
        )
    }

    /// A response's `tool_use`s (theseus-a60): gated in order, then run, the
    /// `Read` calls of a group together.
    /// - An unknown tool, invalid JSON, or invalid input is answered at once,
    ///   wherever it is.
    /// - The first call whose posture is approve ends the gating: it asks
    ///   after every call before it has finished, and the calls after it wait,
    ///   ungated, for the continuation.
    /// - The rest run in groups: consecutive `Read` calls at once, as futures
    ///   in the caller's task, and each `Write` or `Run` call alone. So a
    ///   write or a program starts after every call before it has finished,
    ///   and the calls after it start after it finishes.
    ///
    /// Every kernel call stays in the caller's task, one at a time: the
    /// kernel rewrites an execution's record from what it read.
    pub async fn run_calls(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        calls: &[Call<'_>],
    ) -> Result<Batch> {
        let mut ran = Vec::new();
        let mut runnable = Vec::new();
        let mut ask = None;
        for (i, c) in calls.iter().enumerate() {
            let started = Instant::now();
            match self.admit(tc, assistant_node, c)? {
                Admitted::Answered(outcome) => ran.push(Ran {
                    index: i,
                    outcome,
                    started,
                    ended: Instant::now(),
                    group: ran.len(),
                }),
                Admitted::Asks(tool, g) => {
                    ask = Some((i, tool, g));
                    break;
                }
                Admitted::Runs(tool, g) => runnable.push((i, tool, g)),
            }
        }
        let mut group = Vec::new();
        let mut next = ran.len();
        for (i, tool, g) in runnable {
            if tool.class() == ToolClass::Read {
                group.push((i, tool, g));
                continue;
            }
            for run in [std::mem::take(&mut group), vec![(i, tool, g)]] {
                self.run_group(tc, assistant_node, calls, run, &mut next, &mut ran)
                    .await?;
            }
        }
        self.run_group(tc, assistant_node, calls, group, &mut next, &mut ran)
            .await?;
        let mut awaiting = None;
        if let Some((i, tool, g)) = ask {
            let started = Instant::now();
            let outcome = self
                .start(tc, assistant_node, calls[i].call, tool, g)
                .await?;
            if let CallOutcome::AwaitingConfirm { correlation_id } = &outcome {
                awaiting = Some(correlation_id.clone());
            }
            ran.push(Ran {
                index: i,
                outcome,
                started,
                ended: Instant::now(),
                group: next,
            });
        }
        ran.sort_by_key(|r| r.index);
        Ok(Batch { ran, awaiting })
    }

    /// One call through the gate: counted, and answered now if it cannot run.
    fn admit(&self, tc: &TurnCtx<'_>, assistant_node: &str, c: &Call<'_>) -> Result<Admitted> {
        let call = c.call;
        let Some(tool) = self.registry.by_wire(&call.name).cloned() else {
            return self.unknown_tool(tc, call).map(Admitted::Answered);
        };
        self.count(tool.name());
        if let Some(raw) = c.invalid {
            return self
                .invalid_json(tc, call, tool.name(), raw)
                .map(Admitted::Answered);
        }
        match self.gate(tc, tool.as_ref(), call) {
            Err(bad) => self
                .invalid_input(tc, assistant_node, call, tool.name(), bad)
                .map(Admitted::Answered),
            Ok(g) if g.decision.posture == Posture::Approve => Ok(Admitted::Asks(tool, g)),
            Ok(g) => Ok(Admitted::Runs(tool, g)),
        }
    }

    /// A group of calls, each started at once in the caller's task. Every call
    /// finishes before the first error is returned, so none is left between
    /// its dispatch and its completion.
    async fn run_group(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        calls: &[Call<'_>],
        group: Vec<(usize, Arc<dyn Tool>, Gated)>,
        next: &mut usize,
        ran: &mut Vec<Ran>,
    ) -> Result<()> {
        if group.is_empty() {
            return Ok(());
        }
        let id = *next;
        *next += 1;
        let runs = group.into_iter().map(|(i, tool, g)| async move {
            let started = Instant::now();
            let r = self.start(tc, assistant_node, calls[i].call, tool, g).await;
            (i, started, Instant::now(), r)
        });
        let mut failed = None;
        for (index, started, ended, r) in futures_util::future::join_all(runs).await {
            match r {
                Ok(outcome) => ran.push(Ran {
                    index,
                    outcome,
                    started,
                    ended,
                    group: id,
                }),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }

    /// A gated call: planned (with its `ToolCall` node), then asked, or run.
    async fn start(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
        tool: Arc<dyn Tool>,
        g: Gated,
    ) -> Result<CallOutcome> {
        let a = self.plan_call(tc, assistant_node, call, tool.as_ref(), &g)?;
        if let Some(payload) = Self::notified(tc, call, tool.name(), &a.correlation_id, &g) {
            tc.sink.send(notify::POLICY_NOTIFIED, payload);
        }
        if tc.narrator.on() {
            self.narrate_gate(tc, tool.name(), &g);
        }
        if g.decision.posture == Posture::Approve {
            return self.ask(tc, a, call, tool.name(), g);
        }
        self.execute(tc, &a.correlation_id, tool, call).await
    }

    /// A notify posture runs the call and says so where the operator looks,
    /// naming the call so "should have asked" can point at it: the
    /// `tool.notified` row and notice, or None for any other posture.
    fn notified(
        tc: &TurnCtx<'_>,
        call: &ToolUse,
        tool: &str,
        correlation_id: &str,
        g: &Gated,
    ) -> Option<Value> {
        let n = g.decision.notify.as_ref()?;
        Some(json!({"session_id": tc.session_id, "turn_id": tc.turn_id,
            "tool_use_id": call.id, "correlation_id": correlation_id, "tool": tool,
            "input": call.input, "summary": g.plan.summary, "kind": n.kind,
            "setting": n.setting, "rule": n.rule}))
    }

    fn unknown_tool(&self, tc: &TurnCtx<'_>, call: &ToolUse) -> Result<CallOutcome> {
        narrate_turn!(
            tc,
            Tool,
            "The model called an unknown tool `{}`; it gets an error.",
            call.name.chars().take(40).collect::<String>()
        );
        let text = format!(
            "Unknown tool `{}`. Available: {}.",
            call.name,
            self.registry
                .all()
                .map(|t| theseus_tools::wire_name(t.name()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let r = ResultNode::new(&call.id, &call.name, ResultStatus::Error, text);
        Ok(CallOutcome::Done {
            status: self.answer(tc, r)?,
        })
    }

    fn invalid_json(
        &self,
        tc: &TurnCtx<'_>,
        call: &ToolUse,
        tool: &str,
        raw: &str,
    ) -> Result<CallOutcome> {
        let body = json!({"INVALID_JSON": raw}).to_string();
        let node = self.result_node(
            tc,
            ResultNode::new(&call.id, tool, ResultStatus::Error, body),
        );
        tc.ledger(
            "tool.invalid_input",
            json!({"tool": tool, "tool_use_id": call.id}),
        );
        narrate_turn!(
            tc,
            Tool,
            "{}: the input is not valid JSON, so it does not run.",
            tool
        );
        Ok(CallOutcome::Done {
            status: Self::write_result(tc, &node)?,
        })
    }

    /// The gate for one call (§3.17). The toollet's own typed parse names the
    /// resources, and an `Err` is invalid input. The policy then runs the
    /// call, notifies, or waits (§3.9); nothing it decides refuses one
    /// (theseus-8az). The record keeps the keys stored tool-call nodes carry,
    /// and `tool.proposed` shows it to the session's clients.
    fn gate(&self, tc: &TurnCtx<'_>, tool: &dyn Tool, call: &ToolUse) -> Result<Gated, Invalid> {
        let mut proposal = self.proposal_for(tool, &call.input);
        let tightened = self.tightened.get(tool.name());
        let planned = tool.plan(&call.input, &self.ctx).map(|plan| {
            let t = tightened.as_ref().map(crate::tighten::as_tightened);
            let decision = self.policy.decide_with(tool, &plan, t);
            (plan, decision)
        });
        let result = match &planned {
            Err(e) => json!({"gate": "deny", "reason": format!("validation: {e}")}),
            Ok((_, d)) if d.posture == Posture::Approve => {
                json!({"gate": "needs_confirm", "by": self.policy.confirmer})
            }
            Ok(_) => json!({"gate": "allow"}),
        };
        if let Ok((plan, _)) = &planned {
            proposal.resource = plan.resources.first().map(|r| r.path.display().to_string());
        }
        let record = json!({
            "result": result,
            "validated": planned.is_ok(),
            "decision": planned.as_ref().ok().map(|(_, d)| d),
            "plan": planned.as_ref().ok().map(|(p, _)| p),
            "proposal": proposal,
        });
        tc.sink.send(
            notify::TOOL_PROPOSED,
            json!({"session_id": tc.session_id, "turn_id": tc.turn_id, "tool_use_id": call.id, "tool": tool.name(), "input": call.input, "gate": record}),
        );
        match planned {
            Ok((plan, decision)) => Ok(Gated {
                plan,
                decision,
                proposal,
                record,
            }),
            Err(error) => Err(Invalid { record, error }),
        }
    }

    /// Invalid input: the call node and its error in one frame; it never runs.
    fn invalid_input(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
        tool: &str,
        bad: Invalid,
    ) -> Result<CallOutcome> {
        let reason = format!("validation: {}", bad.error);
        let call_node = Self::tool_call_node(tc, assistant_node, call, tool, None, bad.record);
        let node = self.result_node(
            tc,
            ResultNode {
                meta: json!({"reason": reason}),
                ..ResultNode::new(
                    &call.id,
                    tool,
                    ResultStatus::Error,
                    format!("Invalid input: {}", bad.error),
                )
            },
        );
        tc.store.append(&[call_node.record()?, node.record()?])?;
        tc.ledger(
            "tool.invalid_input",
            json!({"tool": tool, "tool_use_id": call.id, "reason": reason, "input": call.input}),
        );
        narrate_turn!(
            tc,
            Tool,
            "{}: the input is invalid, so it does not run.",
            tool
        );
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done {
            status: ResultStatus::Error,
        })
    }

    /// Plan the call as a kernel action, with its `ToolCall` node in the same
    /// frame. A call that waits for the operator keeps its proposal on the
    /// action (theseus-0g4). One the policy runs is authorized and dispatched
    /// in that frame too, with its `tool.notified` row (theseus-qa0).
    fn plan_call(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
        tool: &dyn Tool,
        g: &Gated,
    ) -> Result<Action> {
        let retry = map_retry(tool.retry());
        let deadline = Some(self.deadline_ms(tool, &call.input));
        let node = |a: &Action| {
            Self::tool_call_node(
                tc,
                assistant_node,
                call,
                tool.name(),
                Some(&a.correlation_id),
                g.record.clone(),
            )
            .record()
        };
        if g.decision.posture == Posture::Approve {
            return tc
                .kernel
                .plan_confirm_with(tc.guard, &g.proposal, retry, deadline, |a| {
                    Ok(vec![node(a)?])
                });
        }
        tc.kernel
            .plan_and_dispatch(tc.guard, &g.proposal, retry, deadline, 0, |a| {
                let mut records = vec![node(a)?];
                if let Some(p) = Self::notified(tc, call, tool.name(), &a.correlation_id, g) {
                    records.push(tc.ledger_record("tool.notified", p)?);
                }
                Ok(records)
            })
    }

    /// The narrative's line for the gate's decision.
    fn narrate_gate(&self, tc: &TurnCtx<'_>, tool: &str, g: &Gated) {
        let subject = self.subject(tool, &g.plan);
        let why = narrative::gate_why(
            &g.decision.reason,
            &g.plan.summary,
            tool,
            g.decision.posture.as_str(),
            g.plan.argv.as_deref(),
            &self.posture_now(tool).setting,
        );
        let why = self.scrubber.scrub(&why).0;
        match g.decision.posture {
            Posture::Approve => {
                narrate_turn!(
                    tc,
                    Tool,
                    "{subject}: posture approve ({why}), waiting for approval."
                )
            }
            Posture::Notify => {
                narrate_turn!(
                    tc,
                    Tool,
                    "{subject}: posture notify ({why}), running and telling the \
                     operator."
                )
            }
            Posture::Open => {
                narrate_turn!(tc, Tool, "{subject}: posture open ({why}), running.")
            }
        }
    }

    /// The call waits for the operator: the question goes to the ledger and
    /// to the session's clients, and the turn parks on it.
    fn ask(
        &self,
        tc: &TurnCtx<'_>,
        a: Action,
        call: &ToolUse,
        tool: &str,
        g: Gated,
    ) -> Result<CallOutcome> {
        let now = theseus_protocol::now_unix_ms();
        let req = ConfirmRequest {
            correlation_id: a.correlation_id.clone(),
            session_id: tc.session_id.into(),
            execution_id: tc.execution_id.into(),
            tool: tool.into(),
            input: call.input.clone(),
            resource: g.proposal.resource,
            reason: g.decision.reason,
            by: self.policy.confirmer.clone(),
            requested_at_ms: now,
            expires_at_ms: now + tc.confirm_ttl_ms,
            floor: g.decision.floor,
            budget: None,
        };
        tc.ledger("tool.confirm_requested", serde_json::to_value(&req)?);
        tc.sink.send(notify::CONFIRM_REQUESTED, &req);
        Ok(CallOutcome::AwaitingConfirm {
            correlation_id: a.correlation_id,
        })
    }

    fn deadline_ms(&self, tool: &dyn Tool, input: &Value) -> u64 {
        match tool.backend() {
            Backend::Inproc => INPROC_DEADLINE_MS,
            Backend::Job => {
                let t = input
                    .get("timeout_secs")
                    .and_then(Value::as_u64)
                    .unwrap_or(self.ctx.proc_timeout_secs)
                    .min(self.ctx.proc_timeout_max_secs);
                (t + 30) * 1000
            }
        }
    }

    /// Run a dispatched action to a result (or a background placeholder).
    /// Dispatch is already durable: `plan_and_dispatch`, or `dispatch` for a
    /// call that was confirmed or authorized before a restart.
    async fn execute(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: Arc<dyn Tool>,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        match tool.backend() {
            Backend::Inproc => self.run_inproc(tc, correlation_id, tool, call).await,
            Backend::Job => self.run_job(tc, correlation_id, tool.as_ref(), call).await,
        }
    }

    /// An in-process tool: its result node rides in its completion's frame.
    async fn run_inproc(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: Arc<dyn Tool>,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        tc.sink.send(
            notify::TOOL_STARTED,
            json!({"session_id": tc.session_id, "turn_id": tc.turn_id, "tool_use_id": call.id, "tool": tool.name(), "correlation_id": correlation_id, "backend": "inproc"}),
        );
        let (t, input, ctx) = (tool.clone(), call.input.clone(), self.ctx.clone());
        // A free core first (theseus-a60): the deadline counts the run, not
        // the wait for one. The call's time is its run's own, timed on its
        // core: its result may wait for the turn's task, busy with the frames
        // of the calls beside it.
        let run = self
            .cpu
            .spawn(move || {
                let t0 = Instant::now();
                (t.run_with_image(&input, &ctx), t0.elapsed())
            })
            .await;
        let started = theseus_protocol::now_unix_ms();
        let t0 = Instant::now();
        let (outcome, took) =
            match tokio::time::timeout(Duration::from_millis(INPROC_DEADLINE_MS), run).await {
                Ok(Ok((Ok(out), took))) => (Ok(out), took),
                Ok(Ok((Err(f), took))) => (Err(f.message), took),
                Ok(Err(join)) => (Err(format!("the tool panicked: {join}")), t0.elapsed()),
                Err(_) => (
                    Err(format!("timed out after {} ms", INPROC_DEADLINE_MS)),
                    t0.elapsed(),
                ),
            };
        let dur = took.as_millis() as u64;
        let (status, mut text, meta, img) = match outcome {
            Ok((o, img)) => (ResultStatus::Ok, o.text, o.meta, img),
            Err(m) => (ResultStatus::Error, m, Value::Null, None),
        };
        // An image the tool read goes to the blobs once; the node holds the
        // reference (theseus-9g2).
        let image =
            img.and_then(
                |d| match crate::attach::store_image(&d.bytes, tc.store.blobs()) {
                    Ok((info, digest)) => Some(crate::node::Attachment {
                        name: d.name,
                        media_type: info.media_type.into(),
                        size: d.bytes.len() as u64,
                        content: crate::node::AttachmentContent::Image {
                            digest,
                            width: info.width,
                            height: info.height,
                        },
                    }),
                    Err(why) => {
                        text.push_str(&format!(" It is not shown: {why}."));
                        None
                    }
                },
            );
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                duration_ms: Some(dur),
                meta: meta.clone(),
                image,
                ..ResultNode::new(&call.id, tool.name(), status, text)
            },
        );
        let c = Completion {
            correlation_id: correlation_id.into(),
            outcome: if status == ResultStatus::Ok {
                Outcome::Succeeded
            } else {
                Outcome::Failed
            },
            result_ref: Some(node.id.clone()),
            external_op_id: None,
            started_at_ms: started,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("inproc:{}", tool.name()),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"duration_ms": dur, "meta": meta})),
        };
        tc.kernel.accept_completion_with(&c, vec![node.record()?])?;
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done { status })
    }

    /// A job: started through the wrapper, waited for up to `proc_sync_secs`,
    /// then left to run in the background with a placeholder result.
    async fn run_job(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        let spec: JobSpec = match tool.job(&call.input, &self.ctx) {
            Ok(s) => s,
            Err(e) => return self.settle_job_failure(tc, correlation_id, tool.name(), call, &e),
        };
        let Some(spool) = self.spool.clone() else {
            return self.settle_job_failure(
                tc,
                correlation_id,
                tool.name(),
                call,
                "no completion spool is configured",
            );
        };
        let mut env = self.proc_env.clone();
        for (k, v) in &spec.env {
            if forbidden_env(k) {
                return self.settle_job_failure(
                    tc,
                    correlation_id,
                    tool.name(),
                    call,
                    &format!("environment variable {k} may not be set by a tool call"),
                );
            }
            env.retain(|(ek, _)| ek != k);
            env.push((k.clone(), v.clone()));
        }
        let args = WrapperArgs {
            spool_dir: spool.dir().to_path_buf(),
            correlation_id: correlation_id.into(),
            deadline_ms: spec.timeout_secs * 1000,
            notify_socket: self.notify_socket.clone(),
            argv: spec.argv.clone(),
            cwd: Some(spec.cwd.clone()),
            env,
        };
        // Outbox: `dispatched` was durable before the process exists.
        let pid = match self.launcher.launch(&spool, &args) {
            Ok(p) => p,
            Err(e) => {
                return self.settle_job_failure(
                    tc,
                    correlation_id,
                    tool.name(),
                    call,
                    &format!("could not start the job: {e}"),
                );
            }
        };
        tc.sink.send(
            notify::TOOL_STARTED,
            json!({"session_id": tc.session_id, "turn_id": tc.turn_id, "tool_use_id": call.id, "tool": tool.name(), "correlation_id": correlation_id, "backend": "job", "pid": pid, "argv": spec.argv, "cwd": spec.cwd}),
        );
        tc.ledger("tool.job_started", json!({"correlation_id": correlation_id, "pid": pid, "argv": spec.argv, "cwd": spec.cwd, "timeout_secs": spec.timeout_secs}));
        let t0 = Instant::now();
        let bound = Duration::from_secs(self.proc_sync_secs.min(spec.timeout_secs + 5));
        narrate_turn!(
            tc,
            Tool,
            "{} started as job {} (pid {pid}); the turn waits up to {} \
             for it.",
            self.scrubber
                .scrub(&narrative::subject(
                    tool.name(),
                    Some(&spec.argv),
                    None,
                    &spec.cwd
                ))
                .0,
            narrative::short(correlation_id),
            narrative::duration(bound.as_millis() as u64)
        );
        loop {
            if let Some(done) = Self::job_settled(tc.kernel, &spool, correlation_id)? {
                let r = ResultNode {
                    duration_ms: Some(t0.elapsed().as_millis() as u64),
                    ..Self::job_result(tc, &done, &call.id, tool.name())
                };
                return Ok(CallOutcome::Done {
                    status: self.answer(tc, r)?,
                });
            }
            if t0.elapsed() >= bound {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let text = format!(
            "Still running as background job {correlation_id} after {} seconds (timeout {} seconds). Its result will arrive in a later message; you can keep working or tell the operator you are waiting.",
            self.proc_sync_secs, spec.timeout_secs
        );
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                meta: json!({"pid": pid}),
                ..ResultNode::new(&call.id, tool.name(), ResultStatus::Background, text)
            },
        )?;
        Ok(CallOutcome::Background {
            correlation_id: correlation_id.into(),
        })
    }

    fn settle_job_failure(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &str,
        call: &ToolUse,
        msg: &str,
    ) -> Result<CallOutcome> {
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                ..ResultNode::new(&call.id, tool, ResultStatus::Error, msg)
            },
        );
        let now = theseus_protocol::now_unix_ms();
        let c = Completion {
            correlation_id: correlation_id.into(),
            outcome: Outcome::Failed,
            result_ref: None,
            external_op_id: None,
            started_at_ms: now,
            finished_at_ms: now,
            producer: format!("harness:{tool}"),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"error": msg})),
        };
        tc.kernel.accept_completion_with(&c, vec![node.record()?])?;
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done {
            status: ResultStatus::Error,
        })
    }

    /// Has the job settled? Accepts a spooled completion if it is there
    /// (idempotent with the harness's own drain) and returns the settled action.
    fn job_settled(kernel: &Kernel, spool: &Spool, correlation_id: &str) -> Result<Option<Action>> {
        if let Some(c) = spool.read_completion(correlation_id)? {
            kernel.accept_completion(&c)?;
            spool.remove(&spool.completion_path(correlation_id))?;
            spool.remove_pid(&correlation_id.to_string());
        }
        let a = kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow!("action {correlation_id} vanished"))?;
        Ok(match a.state {
            ActionState::Succeeded | ActionState::Failed | ActionState::OutcomeUnknown => Some(a),
            _ => None,
        })
    }

    /// A settled job's result: how it ended (its exit code, a timeout, or an
    /// unknown outcome), then its output, the tail of it when it is long,
    /// with the whole kept in the spool.
    fn job_result<'a>(
        tc: &TurnCtx<'_>,
        a: &'a Action,
        tool_use_id: &'a str,
        tool: &'a str,
    ) -> ResultNode<'a> {
        let completion: Option<Completion> = tc
            .store
            .inner()
            .as_ref()
            .latest_by_key(theseus_store::kinds::COMPLETION, &a.correlation_id)
            .ok()
            .flatten()
            .and_then(|r| r.decode().ok());
        let detail = completion
            .as_ref()
            .and_then(|c| c.detail.clone())
            .unwrap_or(Value::Null);
        let (out, total) = read_result_file(a.result_ref.as_deref());
        let exit = detail.get("exit_code").and_then(Value::as_i64);
        let status = match a.state {
            ActionState::Succeeded => ResultStatus::Ok,
            ActionState::Failed => ResultStatus::Error,
            _ => ResultStatus::Unknown,
        };
        let header = match (
            status,
            exit,
            detail.get("timed_out").and_then(Value::as_bool),
        ) {
            (_, _, Some(true)) => "[timed out and killed]\n".to_string(),
            (ResultStatus::Unknown, _, _) => {
                "[outcome unknown: the harness could not establish whether this finished]\n"
                    .to_string()
            }
            (_, Some(c), _) => format!("[exit code {c}]\n"),
            _ => String::new(),
        };
        let raw = if out.is_empty() {
            format!("{header}(no output)")
        } else {
            format!("{header}{out}")
        };
        ResultNode {
            correlation_id: Some(&a.correlation_id),
            duration_ms: detail.get("duration_ms").and_then(Value::as_u64),
            full_ref: a.result_ref.clone(),
            bytes_total: Some(total),
            meta: json!({"exit_code": exit, "detail": detail}),
            ..ResultNode::new(tool_use_id, tool, status, raw)
        }
    }

    /// Continuation: answer every `tool_use` of the last assistant message that
    /// has no result yet — run what was confirmed, close what was declined or
    /// superseded by new input, report what the restart left unknown, and wait
    /// on what is still pending. Each call is first placed (`Pending`), then
    /// acted on.
    pub async fn resume(&self, tc: &TurnCtx<'_>, has_input: bool) -> Result<ResumeOutcome> {
        let mut out = ResumeOutcome::default();
        let nodes = tc.store.transcript(tc.session_id)?;
        let Some((assistant, pending)) = unanswered(&nodes) else {
            return Ok(out);
        };
        let calls: HashMap<&str, &Node> = nodes
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolCall { tool_use_id, .. } => Some((tool_use_id.as_str(), &**n)),
                _ => None,
            })
            .collect();
        // Calls no turn has gated run as a response's calls do (theseus-a60).
        let mut fresh = Vec::new();
        for u in pending {
            let node = calls.get(u.id.as_str()).copied();
            let place = Self::where_is(tc, node)?;
            if matches!(place, Pending::NeverPlanned) && !has_input {
                fresh.push(u);
                continue;
            }
            if self
                .run_fresh(tc, &assistant.id, &mut fresh, &mut out)
                .await?
            {
                return Ok(out);
            }
            let job = match place {
                Pending::NeverPlanned => {
                    self.not_run(tc, &u, "the operator sent a new message before this ran")?;
                    None
                }
                Pending::StoppedAtGate => {
                    // Stopped at the gate (invalid input), but the result write was lost: answer again.
                    self.not_run(
                        tc,
                        &u,
                        "the harness restarted before its result was recorded",
                    )?;
                    None
                }
                Pending::Waiting(corr) if has_input => {
                    self.supersede(tc, &u, &corr)?;
                    None
                }
                Pending::Waiting(corr) => {
                    out.awaiting = Some(corr);
                    return Ok(out);
                }
                Pending::Confirmed(a) => self.run_confirmed(tc, &u, &a, node).await?,
                Pending::Authorized(a) => self.run_authorized(tc, &u, &a).await?,
                Pending::Dispatched(a) => self.check_dispatched(tc, &u, &a)?,
                Pending::Settled(a) => {
                    self.answer_settled(tc, &u, &a)?;
                    None
                }
                Pending::Cancelled(a) => {
                    self.answer_cancelled(tc, &u, &a)?;
                    None
                }
            };
            out.background.extend(job);
            out.wrote += 1;
        }
        self.run_fresh(tc, &assistant.id, &mut fresh, &mut out)
            .await?;
        Ok(out)
    }

    /// Calls no turn has gated, through `run_calls`. True when one of them
    /// now waits for the operator, where the continuation stops.
    async fn run_fresh(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        fresh: &mut Vec<ToolUse>,
        out: &mut ResumeOutcome,
    ) -> Result<bool> {
        if fresh.is_empty() {
            return Ok(false);
        }
        let calls: Vec<Call<'_>> = fresh
            .iter()
            .map(|call| Call {
                call,
                invalid: None,
            })
            .collect();
        let batch = self.run_calls(tc, assistant_node, &calls).await?;
        drop(calls);
        fresh.clear();
        for r in batch.ran {
            match r.outcome {
                CallOutcome::AwaitingConfirm { .. } => continue,
                CallOutcome::Background { correlation_id } => out.background.push(correlation_id),
                CallOutcome::Done { .. } => {}
            }
            out.wrote += 1;
        }
        out.awaiting = batch.awaiting;
        Ok(out.awaiting.is_some())
    }

    /// Where a call stands, from its tool-call node and its action.
    fn where_is(tc: &TurnCtx<'_>, node: Option<&Node>) -> Result<Pending> {
        let Some(node) = node else {
            return Ok(Pending::NeverPlanned);
        };
        let Body::ToolCall {
            correlation_id: Some(corr),
            ..
        } = &node.body
        else {
            return Ok(Pending::StoppedAtGate);
        };
        let a = tc
            .kernel
            .action(corr)?
            .ok_or_else(|| anyhow!("action {corr} vanished"))?;
        Ok(match a.state {
            _ if a.awaits_confirm() => Pending::Waiting(a.correlation_id),
            ActionState::Planned => Pending::Confirmed(a),
            ActionState::Authorized => Pending::Authorized(a),
            ActionState::Dispatched => Pending::Dispatched(a),
            ActionState::Succeeded | ActionState::Failed | ActionState::OutcomeUnknown => {
                Pending::Settled(a)
            }
            ActionState::Cancelled => Pending::Cancelled(a),
        })
    }

    /// New input came instead of an answer: the waiting call is declined.
    fn supersede(&self, tc: &TurnCtx<'_>, u: &ToolUse, corr: &str) -> Result<()> {
        let (_, name) = self.tool_of(u);
        narrate_turn!(
            tc,
            Approval,
            "{name}: new input came instead of an answer, so it is \
             declined."
        );
        tc.kernel.decline_action(
            corr,
            &self.policy.confirmer,
            "superseded: the operator sent a new message instead of confirming",
        )?;
        tc.sink.send(notify::CONFIRM_RESOLVED, json!({"session_id": tc.session_id, "correlation_id": corr, "approved": false, "superseded": true}));
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(corr),
                ..ResultNode::new(
                    &u.id,
                    &name,
                    ResultStatus::Declined,
                    "Not run: the operator sent a new message instead of confirming this call.",
                )
            },
        )?;
        Ok(())
    }

    /// A confirmed call: authorized against the proposal its confirm bound,
    /// then run. A confirm that no longer holds declines it instead. Returns
    /// the job it left running in the background, if any.
    async fn run_confirmed(
        &self,
        tc: &TurnCtx<'_>,
        u: &ToolUse,
        a: &Action,
        node: Option<&Node>,
    ) -> Result<Option<String>> {
        let (tool, name) = self.tool_of(u);
        let Some(tool) = tool else {
            self.not_run(tc, u, "the tool is no longer registered")?;
            return Ok(None);
        };
        let corr = &a.correlation_id;
        let authorized = confirm_proposal(tc.store, a, node)
            .and_then(|p| tc.kernel.authorize(corr, &p, Some(&self.policy.confirmer)));
        match authorized {
            Ok(_) => {
                // `action.confirm` announced the answer; this only acts on it.
                narrate_turn!(tc, Approval, "{name}: approved; running it now.");
                tc.kernel.dispatch(corr, None)?;
                match self.execute(tc, corr, tool, u).await? {
                    CallOutcome::Background { correlation_id } => Ok(Some(correlation_id)),
                    CallOutcome::AwaitingConfirm { .. } => {
                        unreachable!("an authorized action does not ask again")
                    }
                    CallOutcome::Done { .. } => Ok(None),
                }
            }
            Err(e) => {
                // The confirm expired or no longer matches: say so, never run it.
                narrate_turn!(
                    tc,
                    Approval,
                    "{name}: the approval no longer holds ({e}), so it \
                     does not run."
                );
                tc.kernel
                    .decline_action(corr, "harness", &format!("confirmation invalid: {e}"))?;
                self.answer(
                    tc,
                    ResultNode {
                        correlation_id: Some(corr),
                        ..ResultNode::new(
                            &u.id,
                            &name,
                            ResultStatus::Declined,
                            format!("Not run: the confirmation is no longer valid ({e})."),
                        )
                    },
                )?;
                Ok(None)
            }
        }
    }

    /// Authorized before a restart and never dispatched: run it now.
    async fn run_authorized(
        &self,
        tc: &TurnCtx<'_>,
        u: &ToolUse,
        a: &Action,
    ) -> Result<Option<String>> {
        let (tool, name) = self.tool_of(u);
        let Some(tool) = tool else {
            self.not_run(tc, u, "the tool is no longer registered")?;
            return Ok(None);
        };
        narrate_turn!(
            tc,
            Tool,
            "{name}: authorized before a restart; running it now."
        );
        tc.kernel.dispatch(&a.correlation_id, None)?;
        Ok(match self.execute(tc, &a.correlation_id, tool, u).await? {
            CallOutcome::Background { correlation_id } => Some(correlation_id),
            _ => None,
        })
    }

    /// Dispatched before a restart: the job's settled result, a placeholder if
    /// it still runs (returned as a background job), or `unknown`.
    fn check_dispatched(
        &self,
        tc: &TurnCtx<'_>,
        u: &ToolUse,
        a: &Action,
    ) -> Result<Option<String>> {
        let (tool, name) = self.tool_of(u);
        let corr = &a.correlation_id;
        let is_job = tool.as_ref().is_some_and(|t| t.backend() == Backend::Job);
        let settled = match &self.spool {
            Some(sp) if is_job => Self::job_settled(tc.kernel, sp, corr)?,
            _ => None,
        };
        if let Some(done) = settled {
            self.answer(tc, Self::job_result(tc, &done, &u.id, &name))?;
            return Ok(None);
        }
        let alive = is_job
            && self
                .spool
                .as_ref()
                .and_then(|sp| sp.read_pid(corr))
                .is_some_and(|pid| theseus_kernel::job::wrapper_alive(pid, corr));
        if alive {
            self.answer(tc, ResultNode { correlation_id: Some(corr), ..ResultNode::new(&u.id, &name, ResultStatus::Background, format!("Still running as background job {corr} (the harness restarted meanwhile). Its result will arrive in a later message.")) })?;
            return Ok(Some(corr.clone()));
        }
        let _ = tc.kernel.mark_unknown(corr, "interrupted_by_restart");
        self.answer(tc, ResultNode { correlation_id: Some(corr), ..ResultNode::new(&u.id, &name, ResultStatus::Unknown, "The harness restarted while this call was running, and whether it completed cannot be established. Check the current state before retrying.") })?;
        Ok(None)
    }

    /// Settled, but its result node was lost in a restart: a job's output is
    /// in the spool; an in-process call's is gone.
    fn answer_settled(&self, tc: &TurnCtx<'_>, u: &ToolUse, a: &Action) -> Result<()> {
        let (tool, name) = self.tool_of(u);
        let r = if tool.as_ref().is_some_and(|t| t.backend() == Backend::Job) {
            Self::job_result(tc, a, &u.id, &name)
        } else {
            let status = if a.state == ActionState::Succeeded {
                ResultStatus::Ok
            } else {
                ResultStatus::Unknown
            };
            ResultNode { correlation_id: Some(&a.correlation_id), ..ResultNode::new(&u.id, &name, status, "The call settled but its output was lost in a restart. Check the current state before relying on it.") }
        };
        self.answer(tc, r)?;
        Ok(())
    }

    /// Declined or cancelled before it ran.
    fn answer_cancelled(&self, tc: &TurnCtx<'_>, u: &ToolUse, a: &Action) -> Result<()> {
        let (_, name) = self.tool_of(u);
        // A decline records who declined; the model reads only the note.
        let (status, text) = match a.declined_note() {
            Some(note) => (
                ResultStatus::Declined,
                format!("Not run: the operator declined this call ({note})."),
            ),
            None => (
                ResultStatus::Cancelled,
                format!(
                    "Not run: {}.",
                    a.resolution.as_deref().unwrap_or("cancelled")
                ),
            ),
        };
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(&a.correlation_id),
                ..ResultNode::new(&u.id, &name, status, text)
            },
        )?;
        Ok(())
    }

    /// Settled actions queued for this execution since its last turn: a
    /// background job's real result becomes a late result node.
    pub fn absorb(&self, tc: &TurnCtx<'_>, settled: &[Action]) -> Result<u32> {
        let jobs: Vec<&Action> = settled
            .iter()
            .filter(|a| a.tool != PROVIDER_TOOL && a.tool != BUDGET_TOOL)
            .collect();
        if jobs.is_empty() {
            return Ok(0);
        }
        let nodes = tc.store.transcript(tc.session_id)?;
        let mut n = 0;
        for a in jobs {
            let placeholder = nodes.iter().find_map(|(_, node)| match &node.body {
                Body::ToolResult {
                    tool_use_id,
                    tool,
                    status: ResultStatus::Background,
                    correlation_id: Some(c),
                    late: false,
                    ..
                } if c == &a.correlation_id => Some((tool_use_id.clone(), tool.clone())),
                _ => None,
            });
            let Some((tool_use_id, tool)) = placeholder else {
                continue;
            };
            let already = nodes.iter().any(|(_, node)| matches!(&node.body, Body::ToolResult { tool_use_id: t, late: true, .. } if *t == tool_use_id));
            if already {
                continue;
            }
            self.answer(
                tc,
                ResultNode {
                    late: true,
                    ..Self::job_result(tc, a, &tool_use_id, &tool)
                },
            )?;
            tc.ledger(
                "tool.late_result",
                json!({"correlation_id": a.correlation_id, "tool": tool, "state": a.state}),
            );
            n += 1;
        }
        Ok(n)
    }
}

/// A tool result to write: the call it answers, its status and text, and
/// what else the call site knows. Name only what differs from `new`, as
/// `ResultNode { late: true, ..ResultNode::new(…) }`.
struct ResultNode<'a> {
    tool_use_id: &'a str,
    tool: &'a str,
    status: ResultStatus,
    /// Before it is scrubbed and capped.
    text: String,
    correlation_id: Option<&'a str>,
    duration_ms: Option<u64>,
    late: bool,
    /// The whole output, when the text is its tail.
    full_ref: Option<String>,
    /// Bytes of the whole output; the text's own length when None.
    bytes_total: Option<u64>,
    meta: Value,
    image: Option<crate::node::Attachment>,
}

impl<'a> ResultNode<'a> {
    fn new(
        tool_use_id: &'a str,
        tool: &'a str,
        status: ResultStatus,
        text: impl Into<String>,
    ) -> Self {
        Self {
            tool_use_id,
            tool,
            status,
            text: text.into(),
            correlation_id: None,
            duration_ms: None,
            late: false,
            full_ref: None,
            bytes_total: None,
            meta: Value::Null,
            image: None,
        }
    }
}

/// A call through the gate: the toollet's plan, the policy's decision, the
/// proposal a confirm would bind, and the record its node keeps.
struct Gated {
    plan: Plan,
    decision: Decision,
    proposal: Proposal,
    record: Value,
}

/// A call whose input the toollet refused; its gate record is still stored.
struct Invalid {
    record: Value,
    error: String,
}

/// Where one `tool_use` of the last assistant message stands when a turn
/// resumes it.
enum Pending {
    /// No tool-call node: the gate never saw it.
    NeverPlanned,
    /// A tool-call node and no action: it stopped at the gate, and its result
    /// was lost.
    StoppedAtGate,
    /// Waiting for the operator (`Action::awaits_confirm`).
    Waiting(String),
    /// Confirmed, and not yet authorized.
    Confirmed(Action),
    /// Authorized before a restart, and never dispatched.
    Authorized(Action),
    /// Dispatched: still running, settled in the spool, or lost.
    Dispatched(Action),
    /// Settled, and its result node lost in a restart.
    Settled(Action),
    /// Declined or cancelled before it ran.
    Cancelled(Action),
}

/// The last assistant message, and its `tool_use`s with no result yet.
fn unanswered(nodes: &[(u64, Arc<Node>)]) -> Option<(&Node, Vec<ToolUse>)> {
    let answered: HashSet<&str> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool_use_id,
                late: false,
                ..
            } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    let (_, last) = nodes
        .iter()
        .rev()
        .find(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))?;
    let Body::AssistantMessage { blocks, .. } = &last.body else {
        return None;
    };
    let pending = crate::provider::tool_uses_in(blocks)
        .into_iter()
        .filter(|u| !answered.contains(u.id.as_str()))
        .collect();
    Some((&**last, pending))
}

/// The tool runtime from config: registry, canonical roots, policy, limits,
/// and the job environment resolved once from the daemon's own.
pub fn build_runtime(
    cfg: &crate::Config,
    spool: Option<Spool>,
    scrubber: Arc<Scrubber>,
    launcher: Arc<dyn JobLauncher>,
) -> Result<ToolRuntime> {
    let t = &cfg.tools;
    let canon = |p: &str| theseus_tools::paths::canonical_best_effort(&crate::config::expand(p));
    // The workspace is what the config names: projects_dir first, then any
    // more roots. Nothing is assumed about where an operator keeps projects.
    let roots: Vec<PathBuf> = t
        .projects_dir
        .iter()
        .chain(t.roots.iter())
        .map(|r| canon(r))
        .collect();
    if roots.is_empty() && t.enabled {
        tracing::warn!(
            "no [tools].projects_dir (or roots): every path a tool names is outside the workspace"
        );
    }
    let cwd = t
        .cwd
        .as_deref()
        .map(canon)
        .or_else(|| roots.first().cloned())
        .unwrap_or_else(std::env::temp_dir);
    // The floor: Theseus's own state (its store, spool, and bindings file, in
    // the state dir actually in use) and the 1Password CLI's credentials.
    let state = spool
        .as_ref()
        .and_then(|s| s.dir().parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| cfg.state_dir());
    let canon_path = |p: PathBuf| theseus_tools::paths::canonical_best_effort(&p);
    let mut floor_paths = vec![
        canon_path(state.join("store")),
        canon_path(state.join("spool")),
        canon_path(cfg.discord.bindings_path(&state)),
        canon("~/.config/op"),
    ];
    // The token file the daemon was given, by flag or by environment, and
    // the config note's last-known-good copy (theseus-2fo).
    for f in cfg.op_token_file.iter().chain(&cfg.config_copy) {
        floor_paths.push(canon_path(f.clone()));
    }
    let approve: Vec<PathBuf> = t.approve_paths.iter().map(|p| canon(p)).collect();
    let registry = if t.enabled {
        theseus_tools::default_registry()
    } else {
        Registry::new()
    };
    let proc_env: Vec<(String, String)> = t
        .proc_env
        .iter()
        .filter(|k| !forbidden_env(k))
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.clone(), v)))
        .collect();
    let notify_socket = spool.as_ref().map(|s| s.dir().join("notify.sock"));
    let cpu = crate::cpu::CpuPool::for_host();
    Ok(ToolRuntime {
        registry,
        policy: ToolPolicy {
            roots: roots.clone(),
            approve_paths: approve,
            allow_argv: cfg.policy.allow_argv.clone(),
            approve_argv: cfg.policy.approve_argv.clone(),
            enforcement: cfg.policy.enforcement,
            tools: cfg.policy.tools.clone(),
            mcp: cfg.policy.mcp.clone(),
            confirmer: crate::turn::OPERATOR.into(),
            floor_paths,
            floor_argv: crate::policy::floor_argv(),
        },
        ctx: ToolCtx {
            roots,
            cwd,
            max_read_bytes: t.max_read_bytes,
            max_entries: t.max_entries,
            proc_timeout_secs: t.proc_timeout_secs,
            proc_timeout_max_secs: t.proc_timeout_max_secs,
            cores: Some(cpu.clone()),
        },
        spool,
        scrubber,
        launcher,
        notify_socket,
        result_max_chars: t.result_max_chars,
        proc_sync_secs: t.proc_sync_secs,
        proc_env,
        calls: Mutex::new(BTreeMap::new()),
        // Read from the store when the core starts (`Core::build`).
        tightened: Default::default(),
        cpu,
    })
}

/// The narrative's line for a result node: done, failed, not run, sent to the
/// background, or a late result. Sizes and the exit code, never the output.
fn narrate_result(tc: &TurnCtx<'_>, node: &Node) {
    let Body::ToolResult {
        tool,
        status,
        duration_ms,
        correlation_id,
        late,
        bytes_total,
        content,
        meta,
        ..
    } = &node.body
    else {
        return;
    };
    let size = format!(
        "{} ({})",
        narrative::count(narrative::lines_in(content), "line", "lines"),
        narrative::bytes(*bytes_total)
    );
    let exit = meta
        .get("exit_code")
        .and_then(Value::as_i64)
        .map(|c| format!("exit code {c}, "))
        .unwrap_or_default();
    let took = duration_ms
        .map(|ms| format!(" in {}", narrative::duration(ms)))
        .unwrap_or_default();
    let job = correlation_id
        .as_deref()
        .map(narrative::short)
        .unwrap_or_default();
    if *late {
        narrate_turn!(
            tc,
            Job,
            "A late result for {tool} (job {job}) arrived: {}, \
             {exit}{size}; the model reads it next.",
            status.as_str()
        );
        return;
    }
    match status {
        ResultStatus::Ok => narrate_turn!(tc, Tool, "{tool} done{took}: {exit}{size}."),
        ResultStatus::Error => {
            narrate_turn!(tc, Tool, "{tool} ended with an error{took}: {exit}{size}.")
        }
        ResultStatus::Background => {
            narrate_turn!(
                tc,
                Job,
                "{tool} continues in the background as job {job}; its \
                 result comes in a later message."
            )
        }
        ResultStatus::Declined => narrate_turn!(tc, Approval, "{tool} not run: it was declined."),
        ResultStatus::Cancelled => narrate_turn!(
            tc,
            Tool,
            "{tool} not run: {}.",
            meta.get("not_run")
                .and_then(Value::as_str)
                .unwrap_or("it was cancelled")
        ),
        ResultStatus::Unknown => {
            narrate_turn!(
                tc,
                Tool,
                "{tool}: the outcome is unknown; the harness could not \
                 establish whether it finished."
            )
        }
    }
}

/// The proposal a confirm binds and `authorize` re-checks: the action's own,
/// or, for an action planned before actions kept it (theseus-0g4), the one on
/// its tool-call node's gate record. `node` saves the transcript scan when the
/// caller has the node.
pub fn confirm_proposal(store: &Store, a: &Action, node: Option<&Node>) -> Result<Proposal> {
    if let Some(p) = &a.proposal {
        return Ok(p.clone());
    }
    let found;
    let node = match node {
        Some(n) => n,
        None => {
            found = store
                .session_nodes(&a.session_id)?
                .into_iter()
                .map(|(_, n)| n)
                .find(|n| {
                    matches!(&n.body, Body::ToolCall { correlation_id: Some(c), .. }
                        if *c == a.correlation_id)
                });
            found
                .as_ref()
                .ok_or_else(|| anyhow!("no tool call node for {}", a.correlation_id))?
        }
    };
    let Body::ToolCall { gate, .. } = &node.body else {
        return Err(anyhow!("no tool call node for {}", a.correlation_id));
    };
    serde_json::from_value(gate["proposal"].clone())
        .map_err(|e| anyhow!("stored proposal unreadable: {e}"))
}

fn map_retry(r: Retry) -> RetryClass {
    match r {
        Retry::SafeToRepeat => RetryClass::SafeToRepeat,
        Retry::NonRepeatable => RetryClass::NonRepeatable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_keeps_head_and_tail_and_forbidden_env_names() {
        let s = "a".repeat(100) + &"b".repeat(100);
        let (c, t) = cap(&s, 100);
        assert!(t);
        assert!(c.starts_with("aaaa") && c.ends_with("bbbb") && c.contains("omitted"));
        assert_eq!(cap("short", 100), ("short".to_string(), false));
        assert!(forbidden_env("OP_SERVICE_ACCOUNT_TOKEN"));
        assert!(forbidden_env("GITHUB_TOKEN"));
        assert!(forbidden_env("aws_secret_access_key"));
        assert!(!forbidden_env("RUST_LOG"));
    }
}
