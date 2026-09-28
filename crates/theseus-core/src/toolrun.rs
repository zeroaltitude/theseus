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
    run_gate, Action, ActionState, Authority, Completion, GateResult, Kernel, Outcome, Proposal,
    RetryClass, Spool, TurnGuard,
};
use theseus_protocol::{notify, ConfirmRequest};
use theseus_store::Store as _;
use theseus_tools::{Backend, JobSpec, Registry, Retry, Tool, ToolCtx};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, ResultStatus};
use crate::policy::{GatePolicy, ToolPolicy};
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
        spawn_detached(&self.self_exe, &["job-wrapper"], spool, args)
    }
}

/// Runs the wrapper's body on a thread in this process (tests).
pub struct InlineLauncher;

impl JobLauncher for InlineLauncher {
    fn launch(&self, _spool: &Spool, args: &WrapperArgs) -> Result<u32> {
        let a = args.clone();
        std::thread::spawn(move || {
            let _ = theseus_kernel::job::run_wrapper(a);
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
    pub authority: &'a Authority,
    pub confirm_ttl_ms: u64,
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
    pub calls: Mutex<BTreeMap<String, u64>>,
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
        }
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

    fn ledger(&self, tc: &TurnCtx<'_>, kind: &str, data: Value) {
        if let Err(e) = tc.store.append_ledger(&LedgerRow::new(
            kind,
            Some(tc.session_id),
            Some(tc.turn_id),
            data,
        )) {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn result_node(
        &self,
        tc: &TurnCtx<'_>,
        tool_use_id: &str,
        tool: &str,
        status: ResultStatus,
        raw: &str,
        correlation_id: Option<&str>,
        duration_ms: Option<u64>,
        late: bool,
        full_ref: Option<String>,
        bytes_total: Option<u64>,
        meta: Value,
    ) -> Node {
        let (scrubbed, redactions) = self.scrubber.scrub(raw);
        let (content, truncated) = cap(&scrubbed, self.result_max_chars);
        let mut meta = meta;
        if redactions > 0 {
            meta["redactions"] = json!(redactions);
        }
        Node::tool_result(
            tc.session_id,
            Some(tc.turn_id),
            tc.loop_index,
            Body::ToolResult {
                tool_use_id: tool_use_id.into(),
                tool: tool.into(),
                status,
                is_error: matches!(
                    status,
                    ResultStatus::Error
                        | ResultStatus::Denied
                        | ResultStatus::Unknown
                        | ResultStatus::Cancelled
                ),
                content,
                correlation_id: correlation_id.map(str::to_string),
                bytes_total: bytes_total.unwrap_or(raw.len() as u64),
                truncated,
                full_ref,
                duration_ms,
                late,
                meta,
            },
        )
    }

    fn announce_end(&self, tc: &TurnCtx<'_>, node: &Node) {
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
        }
        tc.sink.send(
            notify::NODE_WRITTEN,
            json!({"session_id": tc.session_id, "node_id": node.id, "kind": node.kind_str()}),
        );
    }

    /// Write a result node on its own frame and announce it.
    fn write_result(&self, tc: &TurnCtx<'_>, node: Node) -> Result<ResultStatus> {
        let status = match &node.body {
            Body::ToolResult { status, .. } => *status,
            _ => ResultStatus::Error,
        };
        tc.store.append(vec![node.record()?])?;
        self.announce_end(tc, &node);
        Ok(status)
    }

    /// Answer a `tool_use` that will not run (the response was cut off, the
    /// operator moved on), so the transcript stays valid.
    pub fn not_run(&self, tc: &TurnCtx<'_>, call: &ToolUse, reason: &str) -> Result<()> {
        let tool = self
            .registry
            .by_wire(&call.name)
            .map(|t| t.name().to_string())
            .unwrap_or_else(|| call.name.clone());
        let node = self.result_node(
            tc,
            &call.id,
            &tool,
            ResultStatus::Cancelled,
            &format!("Not run: {reason}."),
            None,
            None,
            false,
            None,
            None,
            json!({"not_run": reason}),
        );
        self.write_result(tc, node)?;
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
        &self,
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

    /// One `tool_use` from the model, through the gate and (if allowed) run.
    pub async fn process(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
        invalid_raw: Option<&str>,
    ) -> Result<CallOutcome> {
        let Some(tool) = self.registry.by_wire(&call.name).cloned() else {
            let node = self.result_node(
                tc,
                &call.id,
                &call.name,
                ResultStatus::Error,
                &format!(
                    "Unknown tool `{}`. Available: {}.",
                    call.name,
                    self.registry
                        .all()
                        .map(|t| theseus_tools::wire_name(t.name()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None,
                None,
                false,
                None,
                None,
                Value::Null,
            );
            return Ok(CallOutcome::Done {
                status: self.write_result(tc, node)?,
            });
        };
        self.count(tool.name());
        if let Some(raw) = invalid_raw {
            let body = json!({"INVALID_JSON": raw}).to_string();
            let node = self.result_node(
                tc,
                &call.id,
                tool.name(),
                ResultStatus::Error,
                &body,
                None,
                None,
                false,
                None,
                None,
                Value::Null,
            );
            self.ledger(
                tc,
                "tool.invalid_input",
                json!({"tool": tool.name(), "tool_use_id": call.id}),
            );
            return Ok(CallOutcome::Done {
                status: self.write_result(tc, node)?,
            });
        }

        let mut proposal = self.proposal_for(tool.as_ref(), &call.input);
        let gp = GatePolicy::new(tool.as_ref(), &self.ctx, &self.policy);
        let (result, trace) = run_gate(&gp, &mut proposal, tc.authority);
        let plan = gp.plan.lock().unwrap().clone();
        let decision = gp.decision.lock().unwrap().clone();
        proposal.resource = plan
            .as_ref()
            .and_then(|p| p.resources.first())
            .map(|r| r.path.display().to_string());
        let gate = json!({
            "result": result,
            "validated": trace.validated,
            "decision": decision,
            "plan": plan,
            "proposal": proposal,
        });
        tc.sink.send(
            notify::TOOL_PROPOSED,
            json!({"session_id": tc.session_id, "turn_id": tc.turn_id, "tool_use_id": call.id, "tool": tool.name(), "input": call.input, "gate": gate}),
        );

        match result {
            // Only validation stops a call here: the policy runs, notifies,
            // or waits (theseus-8az), so a `Deny` is input that failed the
            // toollet's own parse.
            GateResult::Deny { reason } => {
                let text = format!(
                    "Invalid input: {}",
                    reason.trim_start_matches("validation: ")
                );
                let call_node =
                    self.tool_call_node(tc, assistant_node, call, tool.name(), None, gate);
                let node = self.result_node(
                    tc,
                    &call.id,
                    tool.name(),
                    ResultStatus::Error,
                    &text,
                    None,
                    None,
                    false,
                    None,
                    None,
                    json!({"reason": reason}),
                );
                tc.store.append(vec![call_node.record()?, node.record()?])?;
                self.ledger(
                    tc,
                    "tool.invalid_input",
                    json!({"tool": tool.name(), "tool_use_id": call.id, "reason": reason, "input": call.input}),
                );
                self.announce_end(tc, &node);
                Ok(CallOutcome::Done {
                    status: ResultStatus::Error,
                })
            }
            GateResult::NeedsConfirm { by } => {
                let retry = map_retry(tool.retry());
                let a = tc.kernel.plan_action_with(
                    tc.guard,
                    &proposal,
                    retry,
                    Some(self.deadline_ms(tool.as_ref(), &call.input)),
                    0,
                    |a| {
                        Ok(vec![self
                            .tool_call_node(
                                tc,
                                assistant_node,
                                call,
                                tool.name(),
                                Some(&a.correlation_id),
                                gate.clone(),
                            )
                            .record()?])
                    },
                )?;
                let now = theseus_protocol::now_unix_ms();
                let floor = decision.as_ref().is_some_and(|d| d.floor);
                let req = ConfirmRequest {
                    correlation_id: a.correlation_id.clone(),
                    session_id: tc.session_id.into(),
                    execution_id: tc.execution_id.into(),
                    tool: tool.name().into(),
                    input: call.input.clone(),
                    resource: proposal.resource.clone(),
                    reason: decision.map(|d| d.reason).unwrap_or_default(),
                    by,
                    requested_at_ms: now,
                    expires_at_ms: now + tc.confirm_ttl_ms,
                    floor,
                };
                self.ledger(tc, "tool.confirm_requested", serde_json::to_value(&req)?);
                tc.sink.send(notify::CONFIRM_REQUESTED, &req);
                Ok(CallOutcome::AwaitingConfirm {
                    correlation_id: a.correlation_id,
                })
            }
            GateResult::Allow => {
                if let Some(n) = decision.as_ref().and_then(|d| d.notify.clone()) {
                    // A notify posture runs the call and says so where the operator looks.
                    let summary = plan.as_ref().map(|p| p.summary.clone()).unwrap_or_default();
                    let payload = json!({"session_id": tc.session_id, "turn_id": tc.turn_id,
                        "tool_use_id": call.id, "tool": tool.name(), "input": call.input,
                        "summary": summary, "kind": n.kind, "setting": n.setting, "rule": n.rule});
                    self.ledger(tc, "tool.notified", payload.clone());
                    tc.sink.send(notify::POLICY_NOTIFIED, payload);
                }
                let retry = map_retry(tool.retry());
                let a = tc.kernel.plan_action_with(
                    tc.guard,
                    &proposal,
                    retry,
                    Some(self.deadline_ms(tool.as_ref(), &call.input)),
                    0,
                    |a| {
                        Ok(vec![self
                            .tool_call_node(
                                tc,
                                assistant_node,
                                call,
                                tool.name(),
                                Some(&a.correlation_id),
                                gate.clone(),
                            )
                            .record()?])
                    },
                )?;
                tc.kernel.authorize(&a.correlation_id, &proposal, None)?;
                self.execute(tc, &a.correlation_id, tool, call).await
            }
        }
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

    /// Run an authorized action to a result (or a background placeholder).
    async fn execute(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: Arc<dyn Tool>,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        match tool.backend() {
            Backend::Inproc => {
                tc.kernel.dispatch(correlation_id, None)?;
                tc.sink.send(
                    notify::TOOL_STARTED,
                    json!({"session_id": tc.session_id, "turn_id": tc.turn_id, "tool_use_id": call.id, "tool": tool.name(), "correlation_id": correlation_id, "backend": "inproc"}),
                );
                let started = theseus_protocol::now_unix_ms();
                let t0 = Instant::now();
                let (t, input, ctx) = (tool.clone(), call.input.clone(), self.ctx.clone());
                let run = tokio::task::spawn_blocking(move || t.run(&input, &ctx));
                let outcome = match tokio::time::timeout(
                    Duration::from_millis(INPROC_DEADLINE_MS),
                    run,
                )
                .await
                {
                    Ok(Ok(Ok(out))) => Ok(out),
                    Ok(Ok(Err(f))) => Err(f.message),
                    Ok(Err(join)) => Err(format!("the tool panicked: {join}")),
                    Err(_) => Err(format!("timed out after {} ms", INPROC_DEADLINE_MS)),
                };
                let dur = t0.elapsed().as_millis() as u64;
                let (status, text, meta) = match outcome {
                    Ok(o) => (ResultStatus::Ok, o.text, o.meta),
                    Err(m) => (ResultStatus::Error, m, Value::Null),
                };
                let node = self.result_node(
                    tc,
                    &call.id,
                    tool.name(),
                    status,
                    &text,
                    Some(correlation_id),
                    Some(dur),
                    false,
                    None,
                    None,
                    meta.clone(),
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
                    usage_units: None,
                    detail: Some(json!({"duration_ms": dur, "meta": meta})),
                };
                tc.kernel.accept_completion_with(&c, vec![node.record()?])?;
                self.announce_end(tc, &node);
                Ok(CallOutcome::Done { status })
            }
            Backend::Job => {
                let spec: JobSpec = match tool.job(&call.input, &self.ctx) {
                    Ok(s) => s,
                    Err(e) => {
                        tc.kernel.dispatch(correlation_id, None)?;
                        return self
                            .settle_job_failure(tc, correlation_id, tool.name(), call, &e)
                            .await;
                    }
                };
                let Some(spool) = self.spool.clone() else {
                    tc.kernel.dispatch(correlation_id, None)?;
                    return self
                        .settle_job_failure(
                            tc,
                            correlation_id,
                            tool.name(),
                            call,
                            "no completion spool is configured",
                        )
                        .await;
                };
                let mut env = self.proc_env.clone();
                for (k, v) in &spec.env {
                    if forbidden_env(k) {
                        tc.kernel.dispatch(correlation_id, None)?;
                        return self
                            .settle_job_failure(
                                tc,
                                correlation_id,
                                tool.name(),
                                call,
                                &format!("environment variable {k} may not be set by a tool call"),
                            )
                            .await;
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
                // Outbox: `dispatched` is durable before the process exists.
                tc.kernel.dispatch(correlation_id, None)?;
                let pid = match self.launcher.launch(&spool, &args) {
                    Ok(p) => p,
                    Err(e) => {
                        return self
                            .settle_job_failure(
                                tc,
                                correlation_id,
                                tool.name(),
                                call,
                                &format!("could not start the job: {e}"),
                            )
                            .await;
                    }
                };
                tc.sink.send(
                    notify::TOOL_STARTED,
                    json!({"session_id": tc.session_id, "turn_id": tc.turn_id, "tool_use_id": call.id, "tool": tool.name(), "correlation_id": correlation_id, "backend": "job", "pid": pid, "argv": spec.argv, "cwd": spec.cwd}),
                );
                self.ledger(tc, "tool.job_started", json!({"correlation_id": correlation_id, "pid": pid, "argv": spec.argv, "cwd": spec.cwd, "timeout_secs": spec.timeout_secs}));
                let t0 = Instant::now();
                let bound = Duration::from_secs(self.proc_sync_secs.min(spec.timeout_secs + 5));
                loop {
                    if let Some(done) = self.job_settled(tc.kernel, &spool, correlation_id)? {
                        let node = self.job_result_node(
                            tc,
                            &call.id,
                            tool.name(),
                            &done,
                            Some(t0.elapsed().as_millis() as u64),
                            false,
                        );
                        let status = self.write_result(tc, node)?;
                        return Ok(CallOutcome::Done { status });
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
                let node = self.result_node(
                    tc,
                    &call.id,
                    tool.name(),
                    ResultStatus::Background,
                    &text,
                    Some(correlation_id),
                    None,
                    false,
                    None,
                    None,
                    json!({"pid": pid}),
                );
                self.write_result(tc, node)?;
                Ok(CallOutcome::Background {
                    correlation_id: correlation_id.into(),
                })
            }
        }
    }

    async fn settle_job_failure(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &str,
        call: &ToolUse,
        msg: &str,
    ) -> Result<CallOutcome> {
        let node = self.result_node(
            tc,
            &call.id,
            tool,
            ResultStatus::Error,
            msg,
            Some(correlation_id),
            None,
            false,
            None,
            None,
            Value::Null,
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
            usage_units: None,
            detail: Some(json!({"error": msg})),
        };
        tc.kernel.accept_completion_with(&c, vec![node.record()?])?;
        self.announce_end(tc, &node);
        Ok(CallOutcome::Done {
            status: ResultStatus::Error,
        })
    }

    /// Has the job settled? Accepts a spooled completion if it is there
    /// (idempotent with the harness's own drain) and returns the settled action.
    fn job_settled(
        &self,
        kernel: &Kernel,
        spool: &Spool,
        correlation_id: &str,
    ) -> Result<Option<Action>> {
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

    fn job_result_node(
        &self,
        tc: &TurnCtx<'_>,
        tool_use_id: &str,
        tool: &str,
        a: &Action,
        duration_ms: Option<u64>,
        late: bool,
    ) -> Node {
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
        let dur = duration_ms.or_else(|| detail.get("duration_ms").and_then(Value::as_u64));
        self.result_node(
            tc,
            tool_use_id,
            tool,
            status,
            &raw,
            Some(&a.correlation_id),
            dur,
            late,
            a.result_ref.clone(),
            Some(total),
            json!({"exit_code": exit, "detail": detail}),
        )
    }

    /// Continuation: answer every `tool_use` of the last assistant message that
    /// has no result yet — run what was confirmed, close what was declined or
    /// superseded by new input, report what the restart left unknown, and wait
    /// on what is still pending.
    pub async fn resume(&self, tc: &TurnCtx<'_>, has_input: bool) -> Result<ResumeOutcome> {
        let mut out = ResumeOutcome::default();
        let nodes = tc.store.session_nodes(tc.session_id)?;
        let answered: HashSet<String> = nodes
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    tool_use_id,
                    late: false,
                    ..
                } => Some(tool_use_id.clone()),
                _ => None,
            })
            .collect();
        let calls: HashMap<String, Node> = nodes
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolCall { tool_use_id, .. } => Some((tool_use_id.clone(), n.clone())),
                _ => None,
            })
            .collect();
        let Some((_, last)) = nodes
            .iter()
            .rev()
            .find(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        else {
            return Ok(out);
        };
        let Body::AssistantMessage { blocks, .. } = &last.body else {
            return Ok(out);
        };
        let pending: Vec<ToolUse> = crate::provider::tool_uses_in(blocks)
            .into_iter()
            .filter(|u| !answered.contains(&u.id))
            .collect();
        for u in pending {
            let call_node = calls.get(&u.id);
            let corr = call_node.and_then(|n| match &n.body {
                Body::ToolCall { correlation_id, .. } => correlation_id.clone(),
                _ => None,
            });
            let tool = self.registry.by_wire(&u.name).cloned();
            let Some(corr) = corr else {
                if call_node.is_some() {
                    // Stopped at the gate (invalid input), but the result write was lost: answer again.
                    self.not_run(
                        tc,
                        &u,
                        "the harness restarted before its result was recorded",
                    )?;
                    out.wrote += 1;
                    continue;
                }
                if has_input {
                    self.not_run(tc, &u, "the operator sent a new message before this ran")?;
                    out.wrote += 1;
                    continue;
                }
                match self.process(tc, &last.id, &u, None).await? {
                    CallOutcome::AwaitingConfirm { correlation_id } => {
                        out.awaiting = Some(correlation_id);
                        return Ok(out);
                    }
                    CallOutcome::Background { correlation_id } => {
                        out.background.push(correlation_id);
                        out.wrote += 1;
                    }
                    CallOutcome::Done { .. } => out.wrote += 1,
                }
                continue;
            };
            let a = tc
                .kernel
                .action(&corr)?
                .ok_or_else(|| anyhow!("action {corr} vanished"))?;
            let tool_name = tool
                .as_ref()
                .map(|t| t.name().to_string())
                .unwrap_or_else(|| u.name.clone());
            match a.state {
                ActionState::Planned if a.confirm.is_some() => {
                    let Some(tool) = tool else {
                        self.not_run(tc, &u, "the tool is no longer registered")?;
                        out.wrote += 1;
                        continue;
                    };
                    let proposal: Proposal = call_node
                        .and_then(|n| match &n.body {
                            Body::ToolCall { gate, .. } => {
                                serde_json::from_value(gate["proposal"].clone()).ok()
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| self.proposal_for(tool.as_ref(), &u.input));
                    match tc
                        .kernel
                        .authorize(&corr, &proposal, Some(&self.policy.confirmer))
                    {
                        Ok(_) => {
                            // `action.confirm` announced the answer; this only acts on it.
                            match self.execute(tc, &corr, tool, &u).await? {
                                CallOutcome::Background { correlation_id } => {
                                    out.background.push(correlation_id)
                                }
                                CallOutcome::AwaitingConfirm { .. } => {
                                    unreachable!("an authorized action does not ask again")
                                }
                                CallOutcome::Done { .. } => {}
                            }
                            out.wrote += 1;
                        }
                        Err(e) => {
                            // The confirm expired or no longer matches: say so, never run it.
                            tc.kernel.deny_action(
                                &corr,
                                "harness",
                                &format!("confirmation invalid: {e}"),
                            )?;
                            let node = self.result_node(
                                tc,
                                &u.id,
                                &tool_name,
                                ResultStatus::Denied,
                                &format!("Not run: the confirmation is no longer valid ({e})."),
                                Some(&corr),
                                None,
                                false,
                                None,
                                None,
                                Value::Null,
                            );
                            self.write_result(tc, node)?;
                            out.wrote += 1;
                        }
                    }
                }
                ActionState::Planned => {
                    if has_input {
                        tc.kernel.deny_action(
                            &corr,
                            &self.policy.confirmer,
                            "superseded: the operator sent a new message instead of confirming",
                        )?;
                        tc.sink.send(notify::CONFIRM_RESOLVED, json!({"session_id": tc.session_id, "correlation_id": corr, "approved": false, "superseded": true}));
                        let node = self.result_node(tc, &u.id, &tool_name, ResultStatus::Denied, "Not run: the operator sent a new message instead of confirming this call.", Some(&corr), None, false, None, None, Value::Null);
                        self.write_result(tc, node)?;
                        out.wrote += 1;
                    } else {
                        out.awaiting = Some(corr);
                        return Ok(out);
                    }
                }
                ActionState::Authorized => {
                    let Some(tool) = tool else {
                        self.not_run(tc, &u, "the tool is no longer registered")?;
                        out.wrote += 1;
                        continue;
                    };
                    if let CallOutcome::Background { correlation_id } =
                        self.execute(tc, &corr, tool, &u).await?
                    {
                        out.background.push(correlation_id)
                    }
                    out.wrote += 1;
                }
                ActionState::Dispatched => {
                    let is_job = tool
                        .as_ref()
                        .map(|t| t.backend() == Backend::Job)
                        .unwrap_or(false);
                    let settled = match &self.spool {
                        Some(sp) if is_job => self.job_settled(tc.kernel, sp, &corr)?,
                        _ => None,
                    };
                    if let Some(done) = settled {
                        let node = self.job_result_node(tc, &u.id, &tool_name, &done, None, false);
                        self.write_result(tc, node)?;
                        out.wrote += 1;
                        continue;
                    }
                    let alive = is_job
                        && self
                            .spool
                            .as_ref()
                            .and_then(|sp| sp.read_pid(&corr))
                            .map(theseus_kernel::job::pid_alive)
                            .unwrap_or(false);
                    if alive {
                        let node = self.result_node(tc, &u.id, &tool_name, ResultStatus::Background, &format!("Still running as background job {corr} (the harness restarted meanwhile). Its result will arrive in a later message."), Some(&corr), None, false, None, None, Value::Null);
                        self.write_result(tc, node)?;
                        out.background.push(corr);
                        out.wrote += 1;
                    } else {
                        let _ = tc.kernel.mark_unknown(&corr, "interrupted_by_restart");
                        let node = self.result_node(tc, &u.id, &tool_name, ResultStatus::Unknown, "The harness restarted while this call was running, and whether it completed cannot be established. Check the current state before retrying.", Some(&corr), None, false, None, None, Value::Null);
                        self.write_result(tc, node)?;
                        out.wrote += 1;
                    }
                }
                ActionState::Succeeded | ActionState::Failed | ActionState::OutcomeUnknown => {
                    let node = if tool
                        .as_ref()
                        .map(|t| t.backend() == Backend::Job)
                        .unwrap_or(false)
                    {
                        self.job_result_node(tc, &u.id, &tool_name, &a, None, false)
                    } else {
                        self.result_node(tc, &u.id, &tool_name, if a.state == ActionState::Succeeded { ResultStatus::Ok } else { ResultStatus::Unknown }, "The call settled but its output was lost in a restart. Check the current state before relying on it.", Some(&corr), None, false, None, None, Value::Null)
                    };
                    self.write_result(tc, node)?;
                    out.wrote += 1;
                }
                ActionState::Cancelled => {
                    let reason = a.resolution.clone().unwrap_or_else(|| "cancelled".into());
                    let status = if reason.starts_with("denied") {
                        ResultStatus::Denied
                    } else {
                        ResultStatus::Cancelled
                    };
                    let text = if status == ResultStatus::Denied {
                        // The kernel records "denied by <who>: <note>"; the model reads the note.
                        let note = reason.split_once(": ").map_or(reason.as_str(), |(_, n)| n);
                        format!("Not run: the operator declined this call ({note}).")
                    } else {
                        format!("Not run: {reason}.")
                    };
                    let node = self.result_node(
                        tc,
                        &u.id,
                        &tool_name,
                        status,
                        &text,
                        Some(&corr),
                        None,
                        false,
                        None,
                        None,
                        Value::Null,
                    );
                    self.write_result(tc, node)?;
                    out.wrote += 1;
                }
            }
        }
        Ok(out)
    }

    /// Settled actions queued for this execution since its last turn: a
    /// background job's real result becomes a late result node.
    pub fn absorb(&self, tc: &TurnCtx<'_>, settled: &[Action]) -> Result<u32> {
        let jobs: Vec<&Action> = settled
            .iter()
            .filter(|a| a.tool != crate::turn::PROVIDER_TOOL)
            .collect();
        if jobs.is_empty() {
            return Ok(0);
        }
        let nodes = tc.store.session_nodes(tc.session_id)?;
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
            let node = self.job_result_node(tc, &tool_use_id, &tool, a, None, true);
            self.write_result(tc, node)?;
            self.ledger(
                tc,
                "tool.late_result",
                json!({"correlation_id": a.correlation_id, "tool": tool, "state": a.state}),
            );
            n += 1;
        }
        Ok(n)
    }
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
    // The token file the daemon was given, by flag or by environment.
    if let Some(f) = &cfg.op_token_file {
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
        },
        spool,
        scrubber,
        launcher,
        notify_socket,
        result_max_chars: t.result_max_chars,
        proc_sync_secs: t.proc_sync_secs,
        proc_env,
        calls: Mutex::new(BTreeMap::new()),
    })
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
