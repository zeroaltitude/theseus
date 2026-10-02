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
use std::io::{Read, Seek, SeekFrom};
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
use theseus_protocol::{
    ConfirmRequest, ConfirmResolved, Event, GateRecord, GateResult, PolicyNotified, ToolEnded,
    ToolProposed, ToolStarted,
};
use theseus_store::Store as _;
use theseus_tools::{Access, Backend, JobSpec, Plan, Registry, Retry, Tool, ToolClass, ToolCtx};
use zeroize::Zeroize;

use crate::broker::Broker;
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
    /// Where a card for a call that waits goes (theseus-q4v).
    pub outbox: &'a crate::outbox::Outbox,
    /// Posts whose records wait in `store` for the turn's next frame: the
    /// turn indexes them once that is written.
    pub posts: &'a std::sync::Mutex<Vec<Action>>,
    /// What the turn runs against: a task it starts runs on it too (DD7).
    pub target: Option<&'a crate::turn::Target>,
    /// Where this session came from, when it is a task (DD7): its notices
    /// name it, and it starts no tasks.
    pub task: Option<&'a crate::session::TaskOf>,
}

impl TurnCtx<'_> {
    /// Where this turn's facts go (`crate::fact`): its session and turn, its
    /// clients, and its next frame.
    pub fn rec(&self) -> crate::fact::Rec<'_> {
        crate::fact::Rec {
            narrator: self.narrator,
            session: Some(self.session_id),
            turn: Some(self.turn_id),
            to: crate::fact::To::Sink(self.sink),
            store: self.store,
        }
    }

    /// Record a fact of this turn's on each of its channels.
    pub fn record<F: crate::fact::Fact>(&self, f: &F) {
        self.rec().record(f);
    }

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
        self.record(&crate::fact::turn::NodeWritten {
            session_id: self.session_id,
            node,
        });
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
    /// The secret broker (theseus-dcy): what a job's program and a toollet
    /// are given from the board.
    pub broker: Arc<Broker>,
    /// What a call that acts gets once its session has read external text
    /// (theseus-9bp, `[policy] external_text`).
    pub external_text: crate::external::Mode,
    /// The most a job's raw output file keeps (theseus-102, `[tools]
    /// job_output_max_bytes`).
    pub output_max_bytes: u64,
    /// The free space under the state dir: below its floor a job is refused
    /// (theseus-102), and health reads it.
    pub disk: Arc<crate::disk::Disk>,
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

/// `text` cut to about `max` characters when it is longer: its head and its
/// tail, each on a line's edge where the text has one near the cut, and
/// between them one line that says how much is not shown, and how to get it
/// as `rest` says from what was left out (theseus-46v; empty: only how
/// much). Nothing keeps a tool's whole result: a job's raw output is deleted
/// once its result is written (theseus-wz2), and an in-process result was
/// never kept. So a tool's `rest` names another call, never a stored copy.
pub fn cap(text: &str, max: usize, rest: impl FnOnce(&str) -> String) -> (String, bool) {
    let n = text.chars().count();
    if n <= max || max < 64 {
        return (text.to_string(), false);
    }
    let at = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(i, _)| i)
    };
    let head = max * 6 / 10;
    let (mut h, mut t) = (at(head), at(n - (max - head)));
    // A line's edge, when the head keeps at least half of what it would, and
    // so does the tail.
    let mut whole = 0;
    if let Some(i) = text[..h].rfind('\n').filter(|i| i + 1 >= h / 2) {
        h = i + 1;
        whole += 1;
    }
    if text[..t].ends_with('\n') {
        whole += 1;
    } else if let Some(i) = text[t..].find('\n').filter(|i| *i < (text.len() - t) / 2) {
        t += i + 1;
        whole += 1;
    }
    let left = &text[h..t];
    let chars = narrative::count(left.chars().count() as u64, "character", "characters");
    let size = match whole {
        2 => format!(
            "{} ({chars})",
            narrative::count(left.lines().count() as u64, "line", "lines")
        ),
        _ => chars,
    };
    let how = match rest(left) {
        r if r.is_empty() => r,
        r => format!(": {r}"),
    };
    let head = text[..h].strip_suffix('\n').unwrap_or(&text[..h]);
    (
        format!("{head}\n…[{size} not shown{how}]…\n{}", &text[t..]),
        true,
    )
}

/// `4 MiB` for a whole number of MiB, else the bytes: the sizes a job's
/// result names.
fn size(n: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if n >= MIB && n.is_multiple_of(MIB) {
        format!("{} MiB", n / MIB)
    } else {
        narrative::count(n, "byte", "bytes")
    }
}

/// The end of a job's raw output, as the result reads it.
#[derive(Debug, Default, PartialEq)]
struct Tail {
    text: String,
    /// The file's whole length.
    total: u64,
    /// The bytes before what was read.
    unread: u64,
}

/// The last `MAX_RESULT_READ` bytes of a job's raw output, read by seek
/// (theseus-102): whatever the job printed, the daemon holds no more than
/// that. A cut through a UTF-8 character moves to the character's end.
fn read_result_file(path: Option<&str>) -> Tail {
    let Some(p) = path else {
        return Tail::default();
    };
    std::fs::File::open(p)
        .and_then(|mut f| read_tail(&mut f, MAX_RESULT_READ as u64))
        .unwrap_or_default()
}

/// `read_result_file` over any reader that seeks.
fn read_tail<R: Read + Seek>(r: &mut R, max: u64) -> std::io::Result<Tail> {
    let total = r.seek(SeekFrom::End(0))?;
    let mut unread = total.saturating_sub(max);
    r.seek(SeekFrom::Start(unread))?;
    let mut b = Vec::with_capacity((total - unread) as usize);
    // Bounded by the length seen: a file that grows meanwhile is not followed.
    r.take(total - unread).read_to_end(&mut b)?;
    let skip = if unread > 0 {
        b.iter().take(3).take_while(|&&c| c & 0xC0 == 0x80).count()
    } else {
        0
    };
    unread += skip as u64;
    Ok(Tail {
        text: String::from_utf8_lossy(&b[skip..]).into_owned(),
        total,
        unread,
    })
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
            broker: Arc::new(Broker::empty()),
            external_text: Default::default(),
            output_max_bytes: theseus_kernel::job::DEFAULT_OUTPUT_MAX_BYTES,
            disk: Arc::new(crate::disk::Disk::new(tmp, 0, 0)),
        }
    }

    /// The daemon's PATH, as every job gets it unless the call sets its own.
    fn proc_path(&self) -> Option<&str> {
        self.proc_env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.as_str())
    }

    /// The gate's decision with what the broker would give the call
    /// (theseus-dcy): it names the grant, and holds the call to no looser a
    /// posture than each secret's.
    fn brokered(&self, tool: &str, plan: &Plan, input: &Value, d: Decision) -> Decision {
        let cwd = plan
            .resources
            .iter()
            .find(|r| r.access == Access::Exec)
            .map_or(self.ctx.cwd.as_path(), |r| r.path.as_path());
        let path = input
            .pointer("/env/PATH")
            .and_then(Value::as_str)
            .or_else(|| self.proc_path());
        let grants = self.broker.at_gate(tool, plan.argv.as_deref(), cwd, path);
        let (Some(granted), Some((need, setting))) =
            (crate::broker::got(&grants), self.broker.need(&grants))
        else {
            return d;
        };
        let why = format!(
            "{}: {setting}",
            crate::broker::gets(&grants).unwrap_or_default()
        );
        Decision {
            granted: Some(granted),
            ..d.at_least(need, &why, &setting, tool, &plan.summary)
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
        // How to get what the cap leaves out is the tool's to say (theseus-46v).
        let tool = self.registry.get(r.tool);
        let (content, truncated) = cap(&scrubbed, self.result_max_chars, |left| {
            tool.map_or_else(|| theseus_tools::REST_NARROWER.into(), |t| t.rest(left))
        });
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
                // A job's raw output is deleted once its result is written
                // (theseus-wz2), so no node names a file any more.
                full_ref: None,
                duration_ms: r.duration_ms,
                late: r.late,
                meta,
                image: r.image,
                external: r.external,
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
            tc.sink.send(Event::ToolEnded(ToolEnded {
                session_id: tc.session_id.into(),
                turn_id: tc.turn_id.into(),
                tool_use_id: tool_use_id.clone(),
                tool: tool.clone(),
                status: status.as_str().into(),
                duration_ms: *duration_ms,
                correlation_id: correlation_id.clone(),
                late: *late,
                truncated: *truncated,
                bytes: *bytes_total,
                node_id: node.id.clone(),
                exit_code: meta.get("exit_code").and_then(Value::as_i64),
                // A `/stop` ended it, and who stopped it (theseus-4uw).
                stopped_by: meta
                    .get("stopped_by")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                preview: content.chars().take(2000).collect(),
            }));
            if tc.narrator.on() {
                narrate_result(tc, node);
            }
        }
        tc.node_written(node);
    }

    /// A call's main resource for the narrative, scrubbed of any secret value.
    fn subject(&self, tool: &str, plan: &Plan) -> String {
        let s = match &plan.url {
            // A network call's subject is what it asks for (DD5).
            Some(url) => format!("{tool} {url}"),
            None => narrative::subject(
                tool,
                plan.argv.as_deref(),
                plan.resources.first().map(|r| r.path.as_path()),
                &self.ctx.cwd,
            ),
        };
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
        self.not_run_with(tc, call, reason, json!({"not_run": reason}))
    }

    /// Answer a `tool_use` that a `/stop` landing while the model answered
    /// keeps from running (W1): it says who stopped it (`stopped_by`), so the
    /// surfaces show a stop, not a cancel or a failure (theseus-4uw).
    pub fn not_run_stopped(&self, tc: &TurnCtx<'_>, call: &ToolUse, by: &str) -> Result<()> {
        let reason = format!("the operator stopped this turn (/stop, by {by})");
        let meta = json!({"not_run": reason, "stopped_by": by});
        self.not_run_with(tc, call, &reason, meta)
    }

    fn not_run_with(
        &self,
        tc: &TurnCtx<'_>,
        call: &ToolUse,
        reason: &str,
        meta: Value,
    ) -> Result<()> {
        let (_, tool) = self.tool_of(call);
        self.answer(
            tc,
            ResultNode {
                meta,
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
        gate: GateRecord,
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
                gate: Some(Box::new(gate)),
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
        if let Some(notice) = Self::notified(tc, call, tool.name(), &a.correlation_id, &g) {
            tc.sink.send(Event::PolicyNotified(notice));
        }
        if tc.narrator.on() {
            self.narrate_gate(tc, tool.name(), &g);
        }
        if g.decision.posture == Posture::Approve {
            return self.ask(tc, a, call, tool.name(), g);
        }
        self.execute(tc, &a.correlation_id, tool, call, g.decision.posture)
            .await
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
    ) -> Option<PolicyNotified> {
        let notice = g.decision.notify.clone()?;
        Some(PolicyNotified {
            session_id: tc.session_id.into(),
            turn_id: tc.turn_id.into(),
            tool_use_id: call.id.clone(),
            correlation_id: correlation_id.into(),
            tool: tool.into(),
            input: call.input.clone(),
            summary: g.plan.summary.clone(),
            notice,
            granted: g.decision.granted.clone(),
            // A task's notice names the task (DD7).
            task: tc.task.is_some().then(|| crate::task::short(tc.session_id)),
        })
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
            let decision = self.brokered(tool.name(), &plan, &call.input, decision);
            // After the whole order (theseus-9bp): a call that acts in a
            // session that read external text waits. A read and `wake.at`
            // keep their postures (T1b), and cost no record read.
            let held = if crate::external::exempt(tool.class(), tool.name()) {
                Ok(None)
            } else {
                crate::external::held(tc.store, tc.session_id)
            };
            let decision = crate::external::gate(
                decision,
                tool.class(),
                &held,
                self.external_text,
                tool.name(),
                &plan.summary,
            );
            (plan, decision)
        });
        let result = match &planned {
            Err(e) => GateResult {
                gate: "deny".into(),
                reason: Some(format!("validation: {e}")),
                by: None,
            },
            Ok((_, d)) if d.posture == Posture::Approve => GateResult {
                gate: "needs_confirm".into(),
                reason: None,
                by: Some(self.policy.confirmer.clone()),
            },
            Ok(_) => GateResult {
                gate: "allow".into(),
                ..Default::default()
            },
        };
        if let Ok((plan, _)) = &planned {
            proposal.resource = plan.resources.first().map(|r| r.path.display().to_string());
        }
        let record = GateRecord {
            result,
            validated: planned.is_ok(),
            decision: planned.as_ref().ok().map(|(_, d)| d.record()),
            plan: planned.as_ref().ok().map(|(p, _)| p.clone()),
            proposal: proposal.clone(),
        };
        tc.sink.send(Event::ToolProposed(ToolProposed {
            session_id: tc.session_id.into(),
            turn_id: tc.turn_id.into(),
            tool_use_id: call.id.clone(),
            tool: tool.name().into(),
            input: call.input.clone(),
            gate: record.clone(),
        }));
        match planned {
            Ok((plan, decision)) => Ok(Gated {
                plan,
                decision,
                proposal,
                record,
            }),
            Err(error) => Err(Invalid {
                record: Box::new(record),
                error,
            }),
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
        let call_node = Self::tool_call_node(tc, assistant_node, call, tool, None, *bad.record);
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
            // Its card is written with the question, in the same frame, when
            // the session posts somewhere (theseus-q4v).
            let target = tc.outbox.target(tc.session_id);
            let mut card = None;
            let a = tc
                .kernel
                .plan_confirm_with(tc.guard, &g.proposal, retry, deadline, |a| {
                    let n = Self::tool_call_node(
                        tc,
                        assistant_node,
                        call,
                        tool.name(),
                        Some(&a.correlation_id),
                        g.record.clone(),
                    );
                    let mut records = vec![n.record()?];
                    if let Some(target) = &target {
                        let (post, more) = tc.outbox.stage(
                            tc.session_id,
                            tc.execution_id,
                            target,
                            json!({"kind": "card", "question": a.correlation_id, "node": n.id}),
                        )?;
                        records.extend(more);
                        card = Some(post);
                    }
                    Ok(records)
                })?;
            if let Some(post) = card {
                tc.outbox.posted(&post);
            }
            return Ok(a);
        }
        tc.kernel
            .plan_and_dispatch(tc.guard, &g.proposal, retry, deadline, 0, |a| {
                let mut records = vec![node(a)?];
                if let Some(p) = Self::notified(tc, call, tool.name(), &a.correlation_id, g) {
                    records.push(tc.ledger_record("tool.notified", serde_json::to_value(&p)?)?);
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
            task: tc.task.map(|_| theseus_protocol::TaskRef {
                task_id: tc.session_id.into(),
                short: crate::task::short(tc.session_id),
                title: None,
            }),
            external_text: g.decision.external,
        };
        tc.ledger("tool.confirm_requested", serde_json::to_value(&req)?);
        tc.sink.send(Event::ConfirmRequested(req));
        Ok(CallOutcome::AwaitingConfirm {
            correlation_id: a.correlation_id,
        })
    }

    fn deadline_ms(&self, tool: &dyn Tool, input: &Value) -> u64 {
        match tool.backend() {
            Backend::Inproc | Backend::Async | Backend::Harness => INPROC_DEADLINE_MS,
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
    /// `ran_at` is the posture it runs at: the gate's, or `approve` for a call
    /// the operator approved. The broker gives it no secret whose posture is
    /// stricter (theseus-dcy).
    async fn execute(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: Arc<dyn Tool>,
        call: &ToolUse,
        ran_at: Posture,
    ) -> Result<CallOutcome> {
        match tool.backend() {
            Backend::Inproc | Backend::Async => {
                self.run_inproc(tc, correlation_id, tool, call, ran_at)
                    .await
            }
            Backend::Job => {
                self.run_job(tc, correlation_id, tool.as_ref(), call, ran_at)
                    .await
            }
            Backend::Harness => self.run_harness(tc, correlation_id, tool.as_ref(), call),
        }
    }

    /// A tool the harness runs itself, in the turn's own task (`task.create`,
    /// DD7): its result node rides in its completion's frame, as an
    /// in-process tool's does. Run again after a restart, it finds what it did
    /// the first time instead of doing it twice.
    fn run_harness(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        tc.sink.send(Event::ToolStarted(ToolStarted {
            session_id: tc.session_id.into(),
            turn_id: tc.turn_id.into(),
            tool_use_id: call.id.clone(),
            tool: tool.name().into(),
            correlation_id: correlation_id.into(),
            backend: tool.backend().as_str().into(),
            ..Default::default()
        }));
        let started = theseus_protocol::now_unix_ms();
        let t0 = Instant::now();
        let done = match tool.name() {
            crate::task::CREATE => crate::task::create(tc, &call.input, correlation_id),
            crate::wake::AT => crate::wake::set(tc, &call.input, correlation_id),
            other => Err(format!("{other} is not a tool the harness runs")),
        };
        let dur = t0.elapsed().as_millis() as u64;
        let (status, text, meta) = match done {
            Ok((text, meta)) => (ResultStatus::Ok, text, meta),
            Err(why) => (ResultStatus::Error, why, Value::Null),
        };
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                duration_ms: Some(dur),
                meta: meta.clone(),
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
            producer: format!("harness:{}", tool.name()),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"duration_ms": dur, "meta": meta})),
        };
        tc.kernel.accept_completion_with(&c, vec![node.record()?])?;
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done { status })
    }

    /// An in-process tool: its result node rides in its completion's frame.
    /// A toollet computes on a core; an async tool (DD5) waits as a task on
    /// the runtime and holds none.
    async fn run_inproc(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: Arc<dyn Tool>,
        call: &ToolUse,
        ran_at: Posture,
    ) -> Result<CallOutcome> {
        tc.sink.send(Event::ToolStarted(ToolStarted {
            session_id: tc.session_id.into(),
            turn_id: tc.turn_id.into(),
            tool_use_id: call.id.clone(),
            tool: tool.name().into(),
            correlation_id: correlation_id.into(),
            backend: tool.backend().as_str().into(),
            ..Default::default()
        }));
        let (t, input, mut ctx) = (tool.clone(), call.input.clone(), self.ctx.clone());
        // A toollet granted a secret reads it through the broker, bound to
        // this call (theseus-dcy). Its secrets settle first, as a turn's do,
        // since a toollet runs on a core and cannot wait.
        let bound = if self.broker.has_tool(tool.name()) {
            self.broker.settle_for_tool(tool.name()).await;
            let b = Arc::new(crate::broker::Bound {
                broker: self.broker.clone(),
                tool: tool.name().into(),
                ran_at,
                handed: Mutex::default(),
            });
            ctx.secrets = Some(b.clone());
            Some(b)
        } else {
            None
        };
        let deadline = Duration::from_millis(INPROC_DEADLINE_MS);
        let timed_out = || format!("timed out after {} ms", INPROC_DEADLINE_MS);
        let (started, outcome, took) = if tool.backend() == Backend::Async {
            // A task of its own, so a panic is the call's error and not the
            // turn's, and its deadline can stop it. Only an approved call
            // reaches the private address it names.
            ctx.approved = ran_at == Posture::Approve;
            let started = theseus_protocol::now_unix_ms();
            let t0 = Instant::now();
            let mut task = tokio::spawn(t.run_async(&input, &ctx));
            let outcome = match tokio::time::timeout(deadline, &mut task).await {
                Ok(Ok(Ok((out, external)))) => Ok((out, None, external)),
                Ok(Ok(Err(f))) => Err(f.message),
                Ok(Err(join)) => Err(format!("the tool panicked: {join}")),
                Err(_) => {
                    task.abort();
                    Err(timed_out())
                }
            };
            (started, outcome, t0.elapsed())
        } else {
            // A free core first (theseus-a60): the deadline counts the run,
            // not the wait for one. The call's time is its run's own, timed
            // on its core: its result may wait for the turn's task, busy with
            // the frames of the calls beside it.
            let run = self
                .cpu
                .spawn(move || {
                    let t0 = Instant::now();
                    (t.run_with_image(&input, &ctx), t0.elapsed())
                })
                .await;
            let started = theseus_protocol::now_unix_ms();
            let t0 = Instant::now();
            let (outcome, took) = match tokio::time::timeout(deadline, run).await {
                Ok(Ok((Ok((out, img)), took))) => (Ok((out, img, None)), took),
                Ok(Ok((Err(f), took))) => (Err(f.message), took),
                Ok(Err(join)) => (Err(format!("the tool panicked: {join}")), t0.elapsed()),
                Err(_) => (Err(timed_out()), t0.elapsed()),
            };
            (started, outcome, took)
        };
        let dur = took.as_millis() as u64;
        let (status, mut text, meta, img, external) = match outcome {
            Ok((o, img, external)) => (ResultStatus::Ok, o.text, o.meta, img, external),
            Err(m) => (ResultStatus::Error, m, Value::Null, None, None),
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
                external,
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
            producer: format!("{}:{}", tool.backend().as_str(), tool.name()),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"duration_ms": dur, "meta": meta})),
        };
        for secret in bound
            .iter()
            .flat_map(|b| b.handed.lock().unwrap().split_off(0))
        {
            tc.ledger(
                "secret.granted",
                json!({"tool": tool.name(), "secret": secret, "correlation_id": correlation_id}),
            );
        }
        self.complete(tc, &c, &node)?;
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done { status })
    }

    /// An in-process result's completion frame. A result marked external
    /// (DD5) that its session is the first to read since it was last trusted
    /// brings the session's hold in the same frame, under the session
    /// record's lock (theseus-9bp): no crash leaves the text in the context
    /// without the hold.
    fn complete(&self, tc: &TurnCtx<'_>, c: &Completion, node: &Node) -> Result<()> {
        let read = match &node.body {
            Body::ToolResult {
                external: Some(e),
                status: ResultStatus::Ok,
                tool,
                meta,
                ..
            } => Some((tool, e.url.as_str(), meta)),
            _ => None,
        };
        let Some((tool, url, meta)) = read else {
            tc.kernel.accept_completion_with(c, vec![node.record()?])?;
            return Ok(());
        };
        // A search's hold names its query; the URL stays on the node.
        let query = crate::external::search_query(tool, meta);
        let mut newly = None;
        let done = tc.store.with_session(tc.session_id, |rec| {
            let h =
                crate::external::read(&node.id, tool, url, query, theseus_protocol::now_unix_ms());
            let mut frame = vec![node.record()?];
            if let Some(more) = crate::external::hold(rec, h.clone(), Some(tc.turn_id))? {
                frame.extend(more);
                newly = Some(h);
            }
            tc.kernel.accept_completion_with(c, frame)?;
            Ok(())
        })?;
        if done.is_none() {
            // Every session a surface opens has a record before its first
            // turn; one without cannot keep a hold.
            tracing::warn!(session_id = %tc.session_id, "external text read in a session with no record: no hold is kept");
            tc.kernel.accept_completion_with(c, vec![node.record()?])?;
        }
        if let Some(h) = newly {
            narrate_turn!(
                tc,
                Approval,
                "{}",
                crate::external::narrated(&h, self.external_text)
            );
        }
        Ok(())
    }

    /// A job: started through the wrapper, waited for up to `proc_sync_secs`,
    /// then left to run in the background with a placeholder result.
    async fn run_job(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        ran_at: Posture,
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
        // Below the floor no job starts (theseus-102): the store keeps room
        // to write, and the result says why, to the model and every surface.
        if let Some(r) = self.disk.refusal() {
            tc.ledger(
                "job.refused",
                json!({"correlation_id": correlation_id, "tool": tool.name(),
                    "free_mb": r.free_mb, "floor_mb": r.floor_mb}),
            );
            return self.settle_job_failure(tc, correlation_id, tool.name(), call, &r.to_string());
        }
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
        // The broker's variables, from the board (theseus-dcy): a program run
        // by its own argv gets its grant, and nothing stands in for a secret
        // it does not get.
        let path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone());
        let brokered = self
            .broker
            .for_job(&spec.argv, &spec.cwd, path.as_deref(), ran_at)
            .await;
        for (k, v) in &brokered.env {
            env.retain(|(ek, _)| ek != k);
            env.push((k.clone(), v.expose().to_string()));
        }
        let mut args = WrapperArgs {
            spool_dir: spool.dir().to_path_buf(),
            correlation_id: correlation_id.into(),
            deadline_ms: spec.timeout_secs * 1000,
            notify_socket: self.notify_socket.clone(),
            argv: spec.argv.clone(),
            cwd: Some(spec.cwd.clone()),
            env,
            umask: theseus_kernel::umask::operator(),
            // Each granted variable and its secret's name: the wrapper
            // withholds the value from the job's raw output (theseus-l0d).
            redact: brokered
                .granted
                .iter()
                .filter_map(|g| Some((g.variable.clone()?, g.secret.clone())))
                .filter(|(var, _)| brokered.env.iter().any(|(k, _)| k == var))
                .collect(),
            output_max_bytes: self.output_max_bytes,
        };
        // The values go with the spawn, as its environment, or nowhere; the
        // copies here are wiped either way.
        let wipe = |args: &mut WrapperArgs| {
            for (k, v) in args.env.iter_mut() {
                if brokered.env.iter().any(|(g, _)| g == k) {
                    v.zeroize();
                }
            }
        };
        // Outbox: `dispatched` was durable before the process exists. A stop
        // or a cancel that came since read the spool for a pid that is not
        // there yet, and reached nothing: the job is not started
        // (theseus-36to).
        if let Some(a) = Self::told_to_stop(tc.kernel, correlation_id)? {
            wipe(&mut args);
            return self.not_started(tc, &a, call, tool.name());
        }
        let launched = self.launcher.launch(&spool, &args);
        wipe(&mut args);
        let pid = match launched {
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
        let granted = crate::broker::got(&brokered.granted);
        let withheld: Vec<String> = brokered
            .withheld
            .iter()
            .map(|(g, _)| format!("{} got no {}", g.to, g.variable.as_deref().unwrap_or("")))
            .collect();
        tc.sink.send(Event::ToolStarted(ToolStarted {
            session_id: tc.session_id.into(),
            turn_id: tc.turn_id.into(),
            tool_use_id: call.id.clone(),
            tool: tool.name().into(),
            correlation_id: correlation_id.into(),
            backend: "job".into(),
            pid: Some(pid),
            argv: Some(spec.argv.clone()),
            cwd: Some(spec.cwd.clone()),
            granted: Some(granted.clone()),
            withheld: Some(withheld),
        }));
        tc.ledger("tool.job_started", json!({"correlation_id": correlation_id, "pid": pid, "argv": spec.argv, "cwd": spec.cwd, "timeout_secs": spec.timeout_secs}));
        for g in &brokered.granted {
            tc.ledger(
                "secret.granted",
                json!({"program": g.to, "variable": g.variable, "secret": g.secret,
                    "correlation_id": correlation_id, "tool": tool.name()}),
            );
        }
        for (g, why) in &brokered.withheld {
            tc.ledger(
                "secret.withheld",
                json!({"program": g.to, "variable": g.variable, "secret": g.secret,
                    "correlation_id": correlation_id, "tool": tool.name(), "why": why}),
            );
        }
        let note = brokered.note();
        let t0 = Instant::now();
        let bound = Duration::from_secs(self.proc_sync_secs.min(spec.timeout_secs + 5));
        narrate_turn!(
            tc,
            Tool,
            "{} started as job {} (pid {pid}){}; the turn waits up to {} \
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
            granted
                .as_ref()
                .map_or_else(String::new, |g| format!("; {g}")),
            narrative::duration(bound.as_millis() as u64)
        );
        // A stop or a cancel that came during the launch read the spool
        // before the pid was in it, and reached nothing: the job is stopped
        // here, now that its pid is written (theseus-36to). The stop writes
        // `cancel = requested`, then reads the pid; the launch writes the
        // pid, then reads the cancel: one of them sees the other. When both
        // do, the job's group gets a second SIGTERM, which is harmless.
        if tc
            .kernel
            .action(correlation_id)?
            .is_some_and(|a| a.cancel.is_some())
        {
            self.stop_launched(tc, correlation_id, pid).await;
        }
        loop {
            if let Some(done) = Self::job_settled(tc.kernel, &spool, correlation_id)? {
                let mut r = ResultNode {
                    duration_ms: Some(t0.elapsed().as_millis() as u64),
                    ..self.job_result(tc, &done, &call.id, tool.name())
                };
                if let Some(n) = &note {
                    r.text = format!("{n}\n{}", r.text);
                }
                return Ok(CallOutcome::Done {
                    status: self.answer_job(tc, r, &done)?,
                });
            }
            if t0.elapsed() >= bound {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut text = format!(
            "Still running as background job {correlation_id} after {} seconds (timeout {} seconds). Its result will arrive in a later message; you can keep working or tell the operator you are waiting.",
            self.proc_sync_secs, spec.timeout_secs
        );
        if let Some(n) = &note {
            text.push_str(&format!("\n{n}"));
        }
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

    /// The job's action, when a stop or a cancel has told it to stop, or it
    /// has settled, before its launch (theseus-36to).
    fn told_to_stop(kernel: &Kernel, correlation_id: &str) -> Result<Option<Action>> {
        let a = kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow!("action {correlation_id} vanished"))?;
        Ok((a.cancel.is_some() || a.state.is_settled()).then_some(a))
    }

    /// A job a stop or a cancel reached before its launch is never started
    /// (theseus-36to). Its action settles cancelled, its termination
    /// verified, since nothing ran, unless the stop settled it first, having
    /// found no pid to signal (unsupported). Its result reads as any call a
    /// stop caught before it ran ("Not run: stopped by …"), and a
    /// `job.not_started` row says why. Nothing was written to the spool.
    fn not_started(
        &self,
        tc: &TurnCtx<'_>,
        a: &Action,
        call: &ToolUse,
        tool: &str,
    ) -> Result<CallOutcome> {
        let a = if a.state.is_settled() {
            a.clone()
        } else {
            tc.kernel.cancel_verified(&a.correlation_id)?
        };
        tc.ledger(
            "job.not_started",
            json!({"correlation_id": a.correlation_id, "tool": tool,
                "cancel": a.cancel, "resolution": a.resolution}),
        );
        narrate_turn!(
            tc,
            Tool,
            "{tool} was not started as job {}: {} before its launch.",
            narrative::short(&a.correlation_id),
            a.resolution
                .as_deref()
                .unwrap_or("its execution was cancelled")
        );
        let (status, _) = not_run_answer(&a);
        self.answer_cancelled(tc, call, &a)?;
        Ok(CallOutcome::Done { status })
    }

    /// Stop a job a stop or a cancel reached during its launch
    /// (theseus-36to), as `terminate_all` stops one: SIGTERM to its process
    /// group, the grace on the runtime's timer, then SIGKILL; its cancel is
    /// acknowledged, then verified or left uncertain. A cancel step on an
    /// action the stop already settled writes nothing, so the
    /// `job.stopped_at_launch` row is what says the job was stopped.
    async fn stop_launched(&self, tc: &TurnCtx<'_>, correlation_id: &str, pid: u32) {
        let _ = tc.kernel.cancel_acknowledged(correlation_id);
        let mut stopping = theseus_kernel::job::Stopping::start(
            [(pid, correlation_id.to_string())],
            theseus_kernel::job::STOP_GRACE,
        );
        while let Some(wait) = stopping.poll() {
            tokio::time::sleep(wait).await;
        }
        let gone = stopping.all_gone();
        let _ = if gone {
            tc.kernel.cancel_verified(correlation_id)
        } else {
            tc.kernel.cancel_uncertain(correlation_id)
        };
        tc.ledger(
            "job.stopped_at_launch",
            json!({"correlation_id": correlation_id, "pid": pid, "gone": gone}),
        );
        narrate_turn!(
            tc,
            Tool,
            "Job {} was told to stop as it launched, before its pid was known; {}.",
            narrative::short(correlation_id),
            if gone {
                "it is stopped"
            } else {
                "it is not gone yet"
            }
        );
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
        // A job a stop or a cancel killed is settled too: the turn waiting on
        // it hears so at once, not at its bound (W1).
        Ok(match a.state {
            ActionState::Succeeded
            | ActionState::Failed
            | ActionState::OutcomeUnknown
            | ActionState::Cancelled => Some(a),
            _ => None,
        })
    }

    /// A settled job's result: how it ended (its exit code, a timeout, or an
    /// unknown outcome), then its output, the tail of it when it is long.
    /// Answer it with `answer_job`, which then deletes the raw output.
    fn job_result<'a>(
        &self,
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
        let Tail {
            text: out,
            total,
            unread,
        } = read_result_file(self.raw_output(a).as_deref());
        let exit = detail.get("exit_code").and_then(Value::as_i64);
        let status = match a.state {
            ActionState::Succeeded => ResultStatus::Ok,
            ActionState::Failed => ResultStatus::Error,
            ActionState::Cancelled => ResultStatus::Cancelled,
            _ => ResultStatus::Unknown,
        };
        let header = match (
            status,
            exit,
            detail.get("timed_out").and_then(Value::as_bool),
        ) {
            (_, _, Some(true)) => "[timed out and killed]\n".to_string(),
            // A `/stop` killed it (W1), or a cancel did.
            (ResultStatus::Cancelled, _, _) => format!(
                "[cancelled: {}]\n",
                a.resolution
                    .as_deref()
                    .unwrap_or("its execution was cancelled")
            ),
            (ResultStatus::Unknown, _, _) => {
                "[outcome unknown: the harness could not establish whether this finished]\n"
                    .to_string()
            }
            (_, Some(c), _) => format!("[exit code {c}]\n"),
            _ => String::new(),
        };
        // A job that printed past the cap kept only the cap's worth: the
        // result says how much it printed, how much was dropped, and how to
        // get the rest (theseus-102), as the cap's line does (theseus-46v).
        // Since theseus-gsn9 the cap keeps the output's two ends, and the
        // bytes dropped are those between them; a wrapper from before keeps
        // the head alone.
        let dropped = detail.get("dropped").and_then(Value::as_u64).unwrap_or(0);
        let ends = match (
            detail.get("head").and_then(Value::as_u64),
            detail.get("tail").and_then(Value::as_u64),
        ) {
            (Some(h), Some(t)) => Some((h, t)),
            _ => None,
        };
        let bytes = |n: u64| narrative::count(n, "byte", "bytes");
        let header = match dropped {
            0 => header,
            d => {
                let cap = detail
                    .get("output_max_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(total);
                let rest = self
                    .registry
                    .get(tool)
                    .map(|t| t.rest(""))
                    .filter(|r| !r.is_empty())
                    .map(|r| format!("; {r}"))
                    .unwrap_or_default();
                match ends {
                    Some((h, t)) => format!(
                        "{header}[truncated: it printed {}, more than its output cap of {}: its \
                         first {} and its last {} are kept, and the {} between them were \
                         dropped{rest}]\n",
                        bytes(h + d + t),
                        size(cap),
                        bytes(h),
                        bytes(t),
                        bytes(d),
                    ),
                    None => format!(
                        "{header}[truncated: it printed {}, and the {} past its output cap of {} \
                         were dropped{rest}]\n",
                        bytes(total + d),
                        bytes(d),
                        size(cap),
                    ),
                }
            }
        };
        // The end the copy held when the wrapper reported, which the file
        // takes only at the pipe's end (theseus-gsn9).
        let header = match detail.get("held").and_then(Value::as_u64) {
            Some(n) if n > 0 => format!(
                "{header}[when it reported, a process it started still held its output open, and \
                 its last {} were not yet written]\n",
                bytes(n)
            ),
            _ => header,
        };
        // No report, and the file stopped where the head does: the output
        // went past the head, and its end waited in the job's wrapper, which
        // was killed before the pipe's end (theseus-gsn9).
        let (head, tail) = theseus_kernel::redact::split(self.output_max_bytes);
        let header = if completion.is_none() && tail > 0 && total == head {
            format!(
                "{header}[its output reached the first {}, all the file takes before the end, and \
                 its end was lost: the job's wrapper was killed before it could write it]\n",
                bytes(head)
            )
        } else {
            header
        };
        // Only the end of a very long output is read: the cut says so, as the
        // cap's does for what it leaves out (theseus-46v).
        let header = match (unread, dropped, ends) {
            (0, _, _) => header,
            (n, 0, _) | (n, _, Some(_)) => format!(
                "{header}[the first {} of {} not read; its last {} follow]\n",
                bytes(n),
                if dropped == 0 {
                    "its output"
                } else {
                    "what was kept"
                },
                size(total - n)
            ),
            (n, _, None) => format!(
                "{header}[the first {} of what was kept not read; the {} before the cap follow]\n",
                bytes(n),
                size(total - n)
            ),
        };
        let raw = if out.is_empty() {
            format!("{header}(no output)")
        } else {
            format!("{header}{out}")
        };
        let mut meta = json!({"exit_code": exit, "detail": detail});
        stopped_meta(&mut meta, status, a);
        ResultNode {
            correlation_id: Some(&a.correlation_id),
            duration_ms: detail.get("duration_ms").and_then(Value::as_u64),
            bytes_total: Some(total),
            meta,
            ..ResultNode::new(tool_use_id, tool, status, raw)
        }
    }

    /// A job's result on its own frame, announced; then the job's raw output
    /// goes (theseus-wz2). The file held what the job printed before the
    /// scrubber saw it, a printed secret too; the node holds the scrubbed,
    /// capped text, and nothing reads the file again. A restart between the
    /// two leaves the file, 0600 in the private spool.
    fn answer_job(&self, tc: &TurnCtx<'_>, r: ResultNode<'_>, a: &Action) -> Result<ResultStatus> {
        let status = self.answer(tc, r)?;
        self.remove_job_output(a);
        Ok(status)
    }

    /// Where a job's raw output is: the file its completion names, or, for a
    /// job killed before its completion (a stop, a cancel), the spool's file
    /// for its id, when there is one (theseus-ewev). A stopped job's result
    /// reads it too, so it shows what the job printed before the stop.
    fn raw_output(&self, a: &Action) -> Option<String> {
        a.result_ref.clone().or_else(|| {
            let path = self.spool.as_ref()?.result_path(&a.correlation_id);
            path.exists().then(|| path.to_string_lossy().into_owned())
        })
    }

    /// Delete a job's raw output once its result's node is written
    /// (theseus-wz2). A job killed before its completion has no `result_ref`,
    /// so its file waited for the spool's sweep after its cancelled result was
    /// written; now it goes then too, unless the job's wrapper still lives,
    /// as the sweep checks (theseus-ewev).
    fn remove_job_output(&self, a: &Action) {
        if let Some(path) = a.result_ref.as_deref() {
            self.remove_raw_output(path);
            return;
        }
        let Some(spool) = &self.spool else {
            return;
        };
        let corr = &a.correlation_id;
        let alive = spool
            .read_pid(corr)
            .is_some_and(|pid| theseus_kernel::job::wrapper_alive(pid, corr));
        if !alive {
            self.remove_raw_output(&spool.result_path(corr).to_string_lossy());
        }
    }

    /// Delete a job's raw output, once its result's node is written
    /// (theseus-wz2). A failure is a warning: the file stays 0600 in the
    /// private spool.
    fn remove_raw_output(&self, path: &str) {
        if let Some(spool) = &self.spool {
            if let Err(e) = spool.remove_result(std::path::Path::new(path)) {
                tracing::warn!(path, error = %format!("{e:#}"), "a job's raw output was not removed");
            }
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
        tc.sink.send(Event::ConfirmResolved(ConfirmResolved {
            session_id: tc.session_id.into(),
            correlation_id: corr.into(),
            superseded: true,
            ..Default::default()
        }));
        if let Err(e) = tc
            .outbox
            .closed(corr, crate::outbox::Closed::new("superseded", None))
        {
            tracing::warn!(error = %format!("{e:#}"), "the card's settle was not written");
        }
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
        // Authorized and dispatched in one frame (theseus-l6y). A confirm
        // that no longer holds writes nothing, and is declined below; a
        // cancel or a stop that landed first is the error.
        let authorized = match confirm_proposal(tc.store, a, node) {
            Ok(p) => {
                tc.kernel
                    .authorize_and_dispatch(corr, &p, Some(&self.policy.confirmer), None)?
            }
            Err(e) => Err(e),
        };
        match authorized {
            Ok(_) => {
                // `action.confirm` announced the answer; this only acts on it.
                narrate_turn!(tc, Approval, "{name}: approved; running it now.");
                match self.execute(tc, corr, tool, u, Posture::Approve).await? {
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
        Ok(
            match self
                .execute(tc, &a.correlation_id, tool, u, Posture::Approve)
                .await?
            {
                CallOutcome::Background { correlation_id } => Some(correlation_id),
                _ => None,
            },
        )
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
        // A harness tool run again finds what it did (DD7's task ids).
        if let Some(t) = tool.as_ref().filter(|t| t.backend() == Backend::Harness) {
            self.run_harness(tc, corr, t.as_ref(), u)?;
            return Ok(None);
        }
        let is_job = tool.as_ref().is_some_and(|t| t.backend() == Backend::Job);
        let settled = match &self.spool {
            Some(sp) if is_job => Self::job_settled(tc.kernel, sp, corr)?,
            _ => None,
        };
        if let Some(done) = settled {
            self.answer_job(tc, self.job_result(tc, &done, &u.id, &name), &done)?;
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
        // A harness call the reconciler marked unknown (the daemon was down
        // past its deadline) runs again, and finds what it did (DD7).
        if let Some(t) = tool
            .as_ref()
            .filter(|t| t.backend() == Backend::Harness)
            .filter(|_| a.state == ActionState::OutcomeUnknown)
        {
            self.run_harness(tc, &a.correlation_id, t.as_ref(), u)?;
            return Ok(());
        }
        if tool.as_ref().is_some_and(|t| t.backend() == Backend::Job) {
            self.answer_job(tc, self.job_result(tc, a, &u.id, &name), a)?;
            return Ok(());
        }
        let status = if a.state == ActionState::Succeeded {
            ResultStatus::Ok
        } else {
            ResultStatus::Unknown
        };
        self.answer(tc, ResultNode { correlation_id: Some(&a.correlation_id), ..ResultNode::new(&u.id, &name, status, "The call settled but its output was lost in a restart. Check the current state before relying on it.") })?;
        Ok(())
    }

    /// Declined or cancelled before it ran.
    fn answer_cancelled(&self, tc: &TurnCtx<'_>, u: &ToolUse, a: &Action) -> Result<()> {
        let (_, name) = self.tool_of(u);
        let (status, text) = not_run_answer(a);
        let mut meta = Value::Null;
        stopped_meta(&mut meta, status, a);
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(&a.correlation_id),
                meta,
                ..ResultNode::new(&u.id, &name, status, text)
            },
        )?;
        Ok(())
    }

    /// Take the settled actions queued for this execution since its last
    /// turn: a background job's real result becomes a late result node, in
    /// the frame that takes it from the queue (theseus-kol), so a crash
    /// between the two cannot lose it. Returns what was taken, and how many
    /// late results were written.
    pub fn absorb(&self, tc: &TurnCtx<'_>) -> Result<(Vec<Action>, u32)> {
        let (mut late, mut outputs) = (Vec::new(), Vec::new());
        let settled = tc.kernel.take_results_with(tc.guard, |settled| {
            let (nodes, records, raw) = self.late_results(tc, settled)?;
            late = nodes;
            outputs = raw;
            Ok(records)
        })?;
        for node in &late {
            Self::announce_end(tc, node);
        }
        // Their nodes are written, so the jobs' raw output goes, as for a
        // result read within a turn (`answer_job`, theseus-wz2), a stopped
        // job's included (theseus-ewev).
        for a in &outputs {
            self.remove_job_output(a);
        }
        Ok((settled, late.len() as u32))
    }

    /// The late results among `settled`: for each job whose call was
    /// answered `background` and has no late result yet, its result's node
    /// and its `tool.late_result` row, for the frame that takes it from the
    /// queue, and its action, whose raw output goes once that frame is
    /// written.
    fn late_results(
        &self,
        tc: &TurnCtx<'_>,
        settled: &[Action],
    ) -> Result<(Vec<Node>, Vec<theseus_store::NewRecord>, Vec<Action>)> {
        let (mut late, mut records, mut outputs) = (Vec::new(), Vec::new(), Vec::new());
        let jobs: Vec<&Action> = settled
            .iter()
            .filter(|a| a.tool != PROVIDER_TOOL && a.tool != BUDGET_TOOL)
            .collect();
        if jobs.is_empty() {
            return Ok((late, records, outputs));
        }
        let nodes = tc.store.transcript(tc.session_id)?;
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
            let node = self.result_node(
                tc,
                ResultNode {
                    late: true,
                    ..self.job_result(tc, a, &tool_use_id, &tool)
                },
            );
            records.push(node.record()?);
            records.push(tc.ledger_record(
                "tool.late_result",
                json!({"correlation_id": a.correlation_id, "tool": tool, "state": a.state}),
            )?);
            late.push(node);
            outputs.push(a.clone());
        }
        Ok((late, records, outputs))
    }
}

/// What a call that never ran tells the model: who declined it, or why it
/// was cancelled (its action's resolution).
fn not_run_answer(a: &Action) -> (ResultStatus, String) {
    // A decline records who declined; the model reads only the note.
    match a.declined_note() {
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
    }
}

/// A call a `/stop` ended (W1) says who stopped it, in its result's `meta`
/// (`stopped_by`), which `tool.ended` carries: every surface then shows it as
/// a stop the operator asked for, `⏹️ stopped by …`, never as a failure, and
/// apart from a cancel's `not run` (theseus-4uw). Only a call that did not
/// finish: one that finished before the stop reached it keeps its result.
fn stopped_meta(meta: &mut Value, status: ResultStatus, a: &Action) {
    let Some(by) = a.stopped_by().filter(|_| status == ResultStatus::Cancelled) else {
        return;
    };
    if !meta.is_object() {
        *meta = json!({});
    }
    meta["stopped_by"] = json!(by);
}

/// The results of the tool calls a cancel ended before they ran
/// (theseus-w98), as the session's next turn would have written them: a
/// node for each call in `not_run` whose tool-call node the session holds and
/// no result answers yet. A cancelled execution takes no more turns, so the
/// cancel writes them, in its own frame. A provider call and the budget
/// question have no tool-call node, and get none.
pub(crate) fn not_run_results(
    store: &Store,
    session_id: &str,
    not_run: &[Action],
) -> Result<Vec<theseus_store::NewRecord>> {
    let calls: Vec<&Action> = not_run
        .iter()
        .filter(|a| a.tool != PROVIDER_TOOL && a.tool != BUDGET_TOOL)
        .collect();
    if calls.is_empty() {
        return Ok(vec![]);
    }
    let nodes = store.session_nodes(session_id)?;
    let answered: HashSet<&str> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for a in calls {
        let call = nodes.iter().find_map(|(_, n)| match &n.body {
            Body::ToolCall {
                tool_use_id,
                tool,
                correlation_id: Some(c),
                ..
            } if *c == a.correlation_id => Some((n, tool_use_id, tool)),
            _ => None,
        });
        let Some((call, tool_use_id, tool)) = call else {
            continue;
        };
        if answered.contains(tool_use_id.as_str()) {
            continue;
        }
        let (status, text) = not_run_answer(a);
        let mut meta = Value::Null;
        stopped_meta(&mut meta, status, a);
        let node = Node::tool_result(
            session_id,
            call.turn_id.as_deref(),
            call.loop_index,
            Body::ToolResult {
                tool_use_id: tool_use_id.clone(),
                tool: tool.clone(),
                status,
                is_error: true,
                bytes_total: text.len() as u64,
                content: text,
                correlation_id: Some(a.correlation_id.clone()),
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta,
                image: None,
                external: None,
            },
        );
        out.push(node.record()?);
    }
    Ok(out)
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
    /// Bytes of the whole output; the text's own length when None.
    bytes_total: Option<u64>,
    meta: Value,
    image: Option<crate::node::Attachment>,
    /// Where its text came from, when that is outside Theseus (DD5).
    external: Option<theseus_tools::External>,
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
            bytes_total: None,
            meta: Value::Null,
            image: None,
            external: None,
        }
    }
}

/// A call through the gate: the toollet's plan, the policy's decision, the
/// proposal a confirm would bind, and the record its node keeps.
struct Gated {
    plan: Plan,
    decision: Decision,
    proposal: Proposal,
    record: GateRecord,
}

/// A call whose input the toollet refused; its gate record is still stored.
struct Invalid {
    record: Box<GateRecord>,
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
/// the job environment resolved once from the daemon's own, and the secret
/// broker over the daemon's board.
pub fn build_runtime(
    cfg: &crate::Config,
    spool: Option<Spool>,
    scrubber: Arc<Scrubber>,
    launcher: Arc<dyn JobLauncher>,
    secrets: Arc<crate::secrets::SecretBoard>,
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
    let cpu = crate::cpu::CpuPool::for_host();
    let registry = if t.enabled {
        let mut r = theseus_tools::default_registry();
        // The web tools wait on the network, as async tools (DD5).
        let web = crate::web::Web::new(&t.web, t.result_max_chars, cpu.clone());
        for tool in web.tools() {
            r.register(tool);
        }
        // Task sessions (DD7) and wakes (DD8): the harness runs them.
        r.register(Arc::new(crate::task::TaskCreate));
        r.register(Arc::new(crate::wake::WakeAt));
        r
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
    // A program's name is resolved on the daemon's own PATH (theseus-dcy).
    let broker = Broker::new(&cfg.broker, secrets, std::env::var("PATH").ok());
    // web.search's key: its calls run at no looser a posture than the
    // key's, and health lists the grant with its uses (DD5).
    if t.enabled {
        broker.grant_tool("web.search", &t.web.search_key_secret);
    }
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
            floor_paths: floor_paths.clone(),
            floor_argv: crate::policy::floor_argv(),
        },
        ctx: ToolCtx {
            roots,
            floor: floor_paths,
            cwd,
            max_read_bytes: t.max_read_bytes,
            max_entries: t.max_entries,
            proc_timeout_secs: t.proc_timeout_secs,
            proc_timeout_max_secs: t.proc_timeout_max_secs,
            cores: Some(cpu.clone()),
            secrets: None,
            approved: false,
            umask: theseus_kernel::umask::operator(),
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
        broker: Arc::new(broker),
        external_text: cfg.policy.external_text,
        output_max_bytes: t.job_output_max_bytes,
        disk: Arc::new(crate::disk::Disk::new(
            state,
            cfg.server.disk_warn_mb,
            cfg.server.disk_floor_mb,
        )),
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
        // A `/stop` ended it: what the operator asked for (theseus-4uw).
        ResultStatus::Cancelled if meta.get("stopped_by").is_some() => narrate_turn!(
            tc,
            Tool,
            "{tool} stopped by {}{}.",
            meta["stopped_by"].as_str().unwrap_or("the operator"),
            duration_ms
                .map(|ms| format!(", after {}", narrative::duration(ms)))
                .unwrap_or_default()
        ),
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
    gate.as_ref()
        .map(|g| g.proposal.clone())
        .ok_or_else(|| anyhow!("no gate record for {}", a.correlation_id))
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
        let (c, t) = cap(&s, 100, |_| String::new());
        assert!(t);
        assert!(c.starts_with("aaaa") && c.ends_with("bbbb"), "{c}");
        assert!(c.contains("\n…[100 characters not shown]…\n"), "{c}");
        assert_eq!(
            cap("short", 100, |_| unreachable!()),
            ("short".to_string(), false)
        );
        assert!(forbidden_env("OP_SERVICE_ACCOUNT_TOKEN"));
        assert!(forbidden_env("GITHUB_TOKEN"));
        assert!(forbidden_env("aws_secret_access_key"));
        assert!(!forbidden_env("RUST_LOG"));
    }

    /// A cut lands on lines' edges, says how many lines and characters are
    /// not shown, hands `rest` exactly those, and says what `rest` answers,
    /// never that anything is stored (theseus-46v).
    #[test]
    fn cap_cuts_on_line_edges_and_says_what_it_left_out_and_how_to_get_it() {
        let text: String = (1..=60)
            .map(|i| format!("entry {i:02} of the roster\n"))
            .collect();
        let mut seen = String::new();
        let (c, t) = cap(&text, 400, |left| {
            seen = left.to_string();
            "a narrower call returns them".into()
        });
        assert!(t);
        let (head, rest) = c.split_once("\n…[").unwrap();
        let (marker, tail) = rest.split_once("]…\n").unwrap();
        assert_eq!(
            format!("{head}\n{seen}{tail}"),
            text,
            "nothing else is lost"
        );
        assert!(
            seen.starts_with("entry ") && seen.ends_with('\n'),
            "{seen:?}"
        );
        assert_eq!(
            marker,
            format!(
                "{} lines ({} characters) not shown: a narrower call returns them",
                seen.lines().count(),
                crate::narrative::thousands(seen.chars().count() as u64)
            )
        );
        assert!(!c.contains("stored"), "{c}");
        // Cuts that fall on edges already keep every line they can.
        let even: String = (0..100).map(|i| format!("line {i:04}\n")).collect();
        let (c, _) = cap(&even, 200, |_| String::new());
        assert!(
            c.contains("\nline 0011\n…[80 lines (800 characters) not shown]…\nline 0092\n"),
            "{c}"
        );
        // One line too long to cut on an edge: characters only.
        let (c, _) = cap(&"x".repeat(5000), 400, |_| "R".into());
        assert!(c.contains("\n…[4,600 characters not shown: R]…\n"), "{c}");
    }

    /// A reader that counts the bytes read from it.
    struct Counted<R> {
        inner: R,
        read: u64,
    }

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read += n as u64;
            Ok(n)
        }
    }

    impl<R: Seek> Seek for Counted<R> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    /// Review 2's R3 (theseus-102): a job's output is read by seek, so the
    /// daemon reads its last 4 MiB and no more, whatever the job printed.
    /// Here 40 MiB, of which 4 MiB are read; and a sparse 8 GiB file, which a
    /// whole read could not hold, gives its tail (no disk is filled: the file
    /// is a hole and one line).
    #[test]
    fn a_jobs_output_is_read_by_seek_and_only_its_tail() {
        let big = 40 * 1024 * 1024;
        let mut body = vec![b'.'; big];
        body[..5].copy_from_slice(b"first");
        body[big - 5..].copy_from_slice(b"final");
        let mut r = Counted {
            inner: std::io::Cursor::new(body),
            read: 0,
        };
        let t = read_tail(&mut r, MAX_RESULT_READ as u64).unwrap();
        assert_eq!(r.read, MAX_RESULT_READ as u64, "only the tail is read");
        assert_eq!(t.total, big as u64);
        assert_eq!(t.unread, (big - MAX_RESULT_READ) as u64);
        assert!(t.text.ends_with("final") && !t.text.contains("first"));

        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("act_huge.out");
        let f = std::fs::File::create(&path).unwrap();
        let huge = 8u64 << 30;
        f.set_len(huge - 12).unwrap();
        drop(f);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut f, b"the last one").unwrap();
        drop(f);
        let t = read_result_file(path.to_str());
        assert_eq!(t.total, huge);
        assert_eq!(t.unread, huge - MAX_RESULT_READ as u64);
        assert_eq!(t.text.len(), MAX_RESULT_READ);
        assert!(t.text.ends_with("the last one"));
        assert_eq!(read_result_file(None), Tail::default());
        assert_eq!(
            read_result_file(Some("/no/such/invented.out")),
            Tail::default()
        );
    }

    /// A cut through a UTF-8 character moves to the character's end, and the
    /// bytes it skips count as not read.
    #[test]
    fn a_tail_cut_through_a_character_starts_at_the_next_one() {
        // "é" is two bytes: a cut 3 bytes from the end lands inside it.
        let mut r = std::io::Cursor::new("aé\nbc".as_bytes().to_vec());
        let t = read_tail(&mut r, 4).unwrap();
        assert_eq!(t.text, "\nbc");
        assert_eq!((t.total, t.unread), (6, 3));
    }
}
