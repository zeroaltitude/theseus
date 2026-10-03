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
    Accepted, Action, ActionState, CancelState, Completion, ExecState, Execution, Kernel, Outcome,
    Proposal, RetryClass, Spool, TurnGuard, BUDGET_TOOL, PROVIDER_TOOL,
};
use theseus_protocol::{ConfirmRequest, GateRecord, GateResult, LedgerKind, PolicyNotified};
use theseus_store::Store as _;
use theseus_tools::{Access, Backend, JobSpec, Plan, Registry, Retry, Tool, ToolClass, ToolCtx};
use zeroize::Zeroize;

use crate::broker::Broker;
use crate::bus::EventSink;
use crate::fact;
use crate::ledger::LedgerRow;
use crate::narrative::{self, Narrator};
use crate::node::{Body, Node, ResultStatus};
use crate::policy::{Decision, Posture, ToolPolicy};
use crate::provider::ToolUse;
use crate::sandbox::{self, Class, Sandbox};
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
    pub fn ledger(&self, kind: LedgerKind, data: Value) {
        if let Err(e) = self
            .ledger_record(kind, data)
            .and_then(|r| self.store.defer(r))
        {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    /// A ledger row for this turn, as a record for a frame the caller builds.
    pub fn ledger_record(&self, kind: LedgerKind, data: Value) -> Result<theseus_store::NewRecord> {
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
    /// The AWS accounts the config binds, behind the `aws.*` tools (AWS
    /// design §3.5); None when it binds none.
    pub aws: Option<Arc<crate::aws::Aws>>,
    /// The earliest a waiting question may expire, in ms since the epoch
    /// (theseus-830): the driver reads the questions only once it has come.
    /// 0 until the driver has read them once; each question asked lowers it.
    pub question_due: std::sync::atomic::AtomicU64,
    /// L1 (M4 17b): `[sandbox]`, the class, an L1 job's view, the probe.
    pub sandbox: Arc<Sandbox>,
    /// What a cancel reaches besides jobs, and health's cancel counts (18a).
    pub stops: crate::cancel::Stops,
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
                aws: BTreeMap::new(),
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
            aws: None,
            question_due: Default::default(),
            sandbox: Arc::new(Sandbox::new(&Default::default(), &[], &[], &[])),
            stops: Default::default(),
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
    pub(crate) fn brokered(&self, tool: &str, plan: &Plan, input: &Value, d: Decision) -> Decision {
        let cwd = plan
            .resources
            .iter()
            .find(|r| r.access == Access::Exec)
            .map_or(self.ctx.cwd.as_path(), |r| r.path.as_path());
        let path = input
            .pointer("/env/PATH")
            .and_then(Value::as_str)
            .or_else(|| self.proc_path());
        // The variables the call sets: a granted program gets nothing from a
        // call that sets any (review 2's H7).
        let env: Vec<&str> = input
            .get("env")
            .and_then(Value::as_object)
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let grants = self
            .broker
            .at_gate(tool, plan.argv.as_deref(), &env, cwd, path);
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
             - Workspace roots: {}. Reading or running a program outside them, or touching a path on the operator's approve list, waits for the operator's approval; a write outside them takes its tool's posture, as inside.\n\
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
        self.result_node_in(tc.session_id, Some(tc.turn_id), tc.loop_index, r)
    }

    /// `result_node` for a writer that holds no turn: a cancel's sweep names
    /// the turn and loop of the call it answers (theseus-0o8).
    fn result_node_in(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        loop_index: Option<u32>,
        r: ResultNode<'_>,
    ) -> Node {
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
            session_id,
            turn_id,
            loop_index,
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
        tc.record(&fact::tool::ToolEnded {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            node,
        });
        tc.node_written(node);
    }

    /// A call's main resource for the narrative, scrubbed of any secret value.
    pub(crate) fn subject(&self, tool: &str, plan: &Plan) -> String {
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
            if g.plan.class.unwrap_or(tool.class()) == ToolClass::Read {
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
            // Its row rode in the frame that planned the call.
            tc.rec()
                .announce(&fact::tool::ToolNotified { notice: &notice });
        }
        tc.record(&fact::tool::GateDecided {
            runtime: self,
            tool: tool.name(),
            gated: &g,
        });
        if g.decision.posture == Posture::Approve {
            return self.ask(tc, a, call, tool.name(), g);
        }
        let (posture, class) = (g.decision.posture, g.class);
        self.execute(tc, &a.correlation_id, tool, call, posture, class)
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
        tc.record(&fact::tool::UnknownTool { name: &call.name });
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
        tc.record(&fact::tool::InvalidJson {
            tool,
            tool_use_id: &call.id,
        });
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
            let (decision, job_class) = sandbox::decide(self, tool, &plan, &call.input, t);
            // After the whole order (theseus-9bp): a call that acts in a
            // session that read external text waits. A read and `wake.at`
            // keep their postures (T1b), and cost no record read.
            let class = plan.class.unwrap_or(tool.class());
            let held = if crate::external::exempt(class, tool.name()) {
                Ok(None)
            } else {
                crate::external::held(tc.store, tc.session_id)
            };
            let decision = crate::external::gate(
                decision,
                class,
                &held,
                self.external_text,
                tool.name(),
                &plan.summary,
            );
            (plan, decision, job_class)
        });
        let result = match &planned {
            Err(e) => GateResult {
                gate: "deny".into(),
                reason: Some(format!("validation: {e}")),
                by: None,
            },
            Ok((_, d, _)) if d.posture == Posture::Approve => GateResult {
                gate: "needs_confirm".into(),
                reason: None,
                by: Some(self.policy.confirmer.clone()),
            },
            Ok(_) => GateResult {
                gate: "allow".into(),
                ..Default::default()
            },
        };
        if let Ok((plan, _, class)) = &planned {
            proposal.resource = plan.resources.first().map(|r| r.path.display().to_string());
            sandbox::bind_class(&mut proposal, *class);
        }
        let record = GateRecord {
            result,
            validated: planned.is_ok(),
            decision: planned.as_ref().ok().map(sandbox::record),
            plan: planned.as_ref().ok().map(|(p, _, _)| p.clone()),
            proposal: proposal.clone(),
        };
        tc.record(&fact::tool::ToolProposed {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            call,
            tool: tool.name(),
            record: &record,
        });
        match planned {
            Ok((plan, decision, class)) => Ok(Gated {
                plan,
                decision,
                proposal,
                record,
                class,
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
        tc.record(&fact::tool::InvalidInput {
            tool,
            call,
            reason: &reason,
        });
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
                    records.push(tc.rec().row(&fact::tool::ToolNotified { notice: &p })?);
                }
                Ok(records)
            })
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
        tc.record(&fact::tool::CallAsked { request: &req });
        // It expires at its plan's time and the TTL, as `confirm.list` says
        // (theseus-830): the driver reads the questions by then.
        self.question_due.fetch_min(
            a.planned_at_ms + tc.confirm_ttl_ms,
            std::sync::atomic::Ordering::SeqCst,
        );
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
        class: Class,
    ) -> Result<CallOutcome> {
        match tool.backend() {
            Backend::Inproc | Backend::Async => {
                self.run_inproc(tc, correlation_id, tool, call, ran_at)
                    .await
            }
            Backend::Job => {
                self.run_job(tc, correlation_id, tool.as_ref(), call, ran_at, class)
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
        tc.record(&fact::tool::ToolStarted {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            tool_use_id: &call.id,
            tool: tool.name(),
            correlation_id,
            backend: tool.backend().as_str(),
        });
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
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn run_inproc(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: Arc<dyn Tool>,
        call: &ToolUse,
        ran_at: Posture,
    ) -> Result<CallOutcome> {
        tc.record(&fact::tool::ToolStarted {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            tool_use_id: &call.id,
            tool: tool.name(),
            correlation_id,
            backend: tool.backend().as_str(),
        });
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
        // An AWS call names its execution and itself to AWS (AWS design §3.5).
        if let Some(a) = self.aws.as_ref().filter(|_| tool.family() == "aws") {
            ctx.aws = Some(a.bind(tc.execution_id, correlation_id, &call.id));
        }
        let aws = ctx.aws.clone();
        let deadline = Duration::from_millis(INPROC_DEADLINE_MS);
        let timed_out = || format!("timed out after {} ms", INPROC_DEADLINE_MS);
        let mut aborted = false;
        let (started, outcome, took) = if tool.backend() == Backend::Async {
            // A task of its own, so a panic is the call's error and not the
            // turn's, and its deadline can stop it. Only an approved call
            // reaches the private address it names.
            ctx.approved = ran_at == Posture::Approve;
            let started = theseus_protocol::now_unix_ms();
            let t0 = Instant::now();
            let mut task = tokio::spawn(t.run_async(&input, &ctx));
            // A cancel or a stop aborts it (M4 18a).
            let _reachable = self.stops.track(correlation_id, task.abort_handle());
            let outcome = match tokio::time::timeout(deadline, &mut task).await {
                Ok(Ok(Ok((out, external)))) => Ok((out, None, external)),
                Ok(Ok(Err(f))) => Err(f.message),
                Ok(Err(join)) if join.is_cancelled() => {
                    aborted = true;
                    Err(join.to_string())
                }
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
        // A task a cancel aborted (18a): the cancel settles its call, with its
        // verdict, and its result says so, riding as a late one does.
        let by_cancel = match aborted {
            true => crate::cancel::after_abort(tc.kernel, correlation_id).await,
            false => None,
        };
        let (status, mut text, meta, img, external) = match (outcome, &by_cancel) {
            (_, Some(a)) => {
                let (status, text, meta) = crate::cancel::aborted_result(a);
                (status, text, meta, None, None)
            }
            (Ok((o, img, external)), None) => (ResultStatus::Ok, o.text, o.meta, img, external),
            (Err(m), None) => (ResultStatus::Error, m, Value::Null, None, None),
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
            tc.record(&fact::tool::SecretHanded {
                tool: tool.name(),
                secret: &secret,
                correlation_id,
            });
        }
        for r in aws.iter().flat_map(|a| a.requests()) {
            tc.record(&fact::tool::AwsCalled { row: &r.row });
        }
        let accepted = match by_cancel {
            Some(_) => Accepted::LateAfterCancel {
                correlation_id: correlation_id.into(),
            },
            None => self.complete(tc, &c, &node)?,
        };
        // A call a cancel settled while it ran: its completion is recorded on
        // the action alone, and the kernel dropped the node that rode with it.
        // The call did run, and this turn still holds what it said, so the
        // transcript gets it as the call's result (theseus-0o8).
        if matches!(accepted, Accepted::LateAfterCancel { .. }) {
            tc.store.append(&[node.record()?])?;
        }
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done { status })
    }

    /// An in-process result's completion frame, and what the kernel did with
    /// it. A result marked external (DD5) that its session is the first to
    /// read since it was last trusted brings the session's hold in the same
    /// frame, under the session record's lock (theseus-9bp): no crash leaves
    /// the text in the context without the hold.
    fn complete(&self, tc: &TurnCtx<'_>, c: &Completion, node: &Node) -> Result<Accepted> {
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
            return tc.kernel.accept_completion_with(c, vec![node.record()?]);
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
            tc.kernel.accept_completion_with(c, frame)
        })?;
        let accepted = match done {
            Some(accepted) => accepted,
            None => {
                // Every session a surface opens has a record before its first
                // turn; one without cannot keep a hold.
                tracing::warn!(session_id = %tc.session_id, "external text read in a session with no record: no hold is kept");
                tc.kernel.accept_completion_with(c, vec![node.record()?])?
            }
        };
        if let Some(h) = newly {
            tc.record(&fact::tool::HoldTaken {
                hold: &h,
                mode: self.external_text,
            });
        }
        Ok(accepted)
    }

    /// A job: started through the wrapper, waited for up to `proc_sync_secs`,
    /// then left to run in the background with a placeholder result.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn run_job(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        ran_at: Posture,
        class: Class,
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
            tc.record(&fact::tool::JobRefused {
                correlation_id,
                tool: tool.name(),
                free_mb: r.free_mb,
                floor_mb: r.floor_mb,
            });
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
        // by its own argv, by a call that sets no variable of its own (review
        // 2's H7), gets its grant, and nothing stands in for a secret it does
        // not get.
        let path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone());
        let set: Vec<&str> = spec.env.iter().map(|(k, _)| k.as_str()).collect();
        let (brokered, sandbox) =
            sandbox::for_job(self, class, &spec, &set, path.as_deref(), ran_at).await;
        for (k, v) in &brokered.env {
            env.retain(|(ek, _)| ek != k);
            env.push((k.clone(), v.expose().to_string()));
        }
        // A git given a secret, or the git a gh given one runs: no hooks and
        // no fsmonitor program (theseus-ur1t).
        brokered.pin(&mut env);
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
            sandbox,
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
        let note = brokered.note();
        let t0 = Instant::now();
        let bound = Duration::from_secs(self.proc_sync_secs.min(spec.timeout_secs + 5));
        sandbox::started(self, tc, correlation_id, tool.name(), &spec, &args);
        tc.record(&fact::tool::JobStarted {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            tool_use_id: &call.id,
            tool: tool.name(),
            correlation_id,
            pid,
            argv: &spec.argv,
            cwd: &spec.cwd,
            timeout_secs: spec.timeout_secs,
            granted: granted.as_deref(),
            withheld: &withheld,
            bound_ms: bound.as_millis() as u64,
            scrubber: &self.scrubber,
            class,
        });
        for g in &brokered.granted {
            tc.record(&fact::tool::SecretGranted {
                grant: g,
                correlation_id,
                tool: tool.name(),
            });
        }
        for (g, why) in &brokered.withheld {
            tc.record(&fact::tool::SecretWithheld {
                grant: g,
                why,
                correlation_id,
                tool: tool.name(),
            });
        }
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
                    ..self.job_result(tc.store, &done, &call.id, tool.name())
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
            tc.kernel.cancel_verified(&a.correlation_id, None)?
        };
        tc.record(&fact::tool::JobNotStarted { action: &a, tool });
        let (status, _) = not_run_answer(&a);
        self.answer_cancelled(tc, call, &a)?;
        Ok(CallOutcome::Done { status })
    }

    /// Stop a job a stop or a cancel reached during its launch
    /// (theseus-36to), through the one stop (`terminate_all`): its wrapper
    /// is asked to stop its tree, and its cancel walks to a verdict. A
    /// cancel step on an action the stop already settled writes nothing, so
    /// the `job.stopped_at_launch` row is what says the job was stopped.
    async fn stop_launched(&self, tc: &TurnCtx<'_>, correlation_id: &str, pid: u32) {
        let ended = self
            .terminate_all(tc.kernel, &[correlation_id.to_string()])
            .await;
        for e in ended.iter().filter(|e| e.written) {
            e.record(&tc.rec());
        }
        let gone = ended.iter().all(|e| e.verdict.verified());
        tc.record(&fact::tool::JobStoppedAtLaunch {
            correlation_id,
            pid,
            gone,
        });
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
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn job_result<'a>(
        &self,
        store: &Store,
        a: &'a Action,
        tool_use_id: &'a str,
        tool: &'a str,
    ) -> ResultNode<'a> {
        let completion: Option<Completion> = store
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
                "[cancelled: {}{}]\n",
                a.resolution
                    .as_deref()
                    .unwrap_or("its execution was cancelled"),
                crate::cancel::words(a).map_or(String::new(), |w| format!("; {w}"))
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
        let header = format!("{}{header}", sandbox::result_lines(&detail));
        let raw = if out.is_empty() {
            format!("{header}(no output)")
        } else {
            format!("{header}{out}")
        };
        let mut meta = json!({"exit_code": exit, "detail": detail});
        crate::cancel::stopped_meta(&mut meta, status, a);
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
    /// written; now it goes then too (theseus-ewev). Neither goes while the
    /// job's wrapper still lives, as the sweep checks: a job can report while a
    /// process it started holds its output open, and its wrapper then still
    /// writes the file (its end waits in the ring), so the file stays, and
    /// the sweep takes it once the wrapper has exited (theseus-5wgd).
    pub(crate) fn remove_job_output(&self, a: &Action) {
        let Some(spool) = &self.spool else {
            return;
        };
        let corr = &a.correlation_id;
        if spool.wrapper_lives(corr) {
            return;
        }
        match a.result_ref.as_deref() {
            Some(path) => self.remove_raw_output(path),
            None => self.remove_raw_output(&spool.result_path(corr).to_string_lossy()),
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
                Pending::NeverAsked(a) => {
                    self.answer_never_asked(tc, &u, &a)?;
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
            _ if a.awaits_confirm() && Self::never_asked(&a, node) => Pending::NeverAsked(a),
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

    /// Whether a planned call that `awaits_confirm` was never asked
    /// (theseus-ni5). `awaits_confirm` reads "planned, no confirm bound" as a
    /// question, which is also what a call looks like when a restart came
    /// between its plan and its authorization, in a build that wrote the two
    /// in separate frames. The kernel cannot tell the two apart; the call's
    /// node can: a question asked since theseus-0g4 keeps its proposal on
    /// the action, and one from before it kept it on the node, whose gate
    /// said `needs_confirm`. A call with no proposal whose gate said `allow`
    /// was never asked.
    fn never_asked(a: &Action, node: &Node) -> bool {
        a.proposal.is_none()
            && matches!(&node.body, Body::ToolCall { gate: Some(g), .. } if g.result.gate == "allow")
    }

    /// A call planned and never asked, found by a continuation: nothing asked
    /// the operator and nothing ran it, so it is declined by the harness and
    /// answered not run, and the model may ask again (theseus-ni5). Before,
    /// the turn parked on a question no card ever posted.
    fn answer_never_asked(&self, tc: &TurnCtx<'_>, u: &ToolUse, a: &Action) -> Result<()> {
        let (_, name) = self.tool_of(u);
        let corr = &a.correlation_id;
        tc.kernel
            .decline_action(corr, "harness", "planned before a restart and never asked")?;
        tc.record(&fact::tool::CallNeverAsked { tool: &name });
        self.answer(
            tc,
            ResultNode {
                correlation_id: Some(corr),
                ..ResultNode::new(
                    &u.id,
                    &name,
                    ResultStatus::Cancelled,
                    "Not run: this call was planned before a restart and the operator was never \
                     asked about it. Ask again if it is still wanted.",
                )
            },
        )?;
        Ok(())
    }

    /// New input came instead of an answer: the waiting call is declined.
    fn supersede(&self, tc: &TurnCtx<'_>, u: &ToolUse, corr: &str) -> Result<()> {
        let (_, name) = self.tool_of(u);
        tc.kernel.decline_action(
            corr,
            &self.policy.confirmer,
            "superseded: the operator sent a new message instead of confirming",
        )?;
        tc.record(&fact::tool::CallSuperseded {
            session_id: tc.session_id,
            correlation_id: corr,
            tool: &name,
        });
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
        // It runs in the class its proposal names, as the confirm bound it.
        let mut class = Class::L0;
        let authorized = match confirm_proposal(tc.store, a, node) {
            Ok(p) => {
                class = sandbox::class_in(&p);
                tc.kernel
                    .authorize_and_dispatch(corr, &p, Some(&self.policy.confirmer), None)?
            }
            Err(e) => Err(e),
        };
        match authorized {
            Ok(_) => {
                // `action.confirm` announced the answer; this only acts on it.
                tc.record(&fact::tool::ApprovedRunning { tool: &name });
                let ran = self.execute(tc, corr, tool, u, Posture::Approve, class);
                match ran.await? {
                    CallOutcome::Background { correlation_id } => Ok(Some(correlation_id)),
                    CallOutcome::AwaitingConfirm { .. } => {
                        unreachable!("an authorized action does not ask again")
                    }
                    CallOutcome::Done { .. } => Ok(None),
                }
            }
            Err(e) => {
                // The confirm expired or no longer matches: say so, never run it.
                tc.record(&fact::tool::ApprovalVoid {
                    tool: &name,
                    error: &e,
                });
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
        tc.record(&fact::tool::AuthorizedResumed { tool: &name });
        // One with no proposal to read predates L1 (theseus-0g4): L0.
        let class =
            confirm_proposal(tc.store, a, None).map_or(Class::L0, |p| sandbox::class_in(&p));
        tc.kernel.dispatch(&a.correlation_id, None)?;
        Ok(
            match self
                .execute(tc, &a.correlation_id, tool, u, Posture::Approve, class)
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
            self.answer_job(tc, self.job_result(tc.store, &done, &u.id, &name), &done)?;
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
            self.answer_job(tc, self.job_result(tc.store, a, &u.id, &name), a)?;
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
        crate::cancel::stopped_meta(&mut meta, status, a);
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
                    ..self.job_result(tc.store, a, &tool_use_id, &tool)
                },
            );
            records.push(node.record()?);
            records.push(tc.rec().row(&fact::tool::LateResult {
                correlation_id: &a.correlation_id,
                tool: &tool,
                state: a.state,
            })?);
            late.push(node);
            outputs.push(a.clone());
        }
        Ok((late, records, outputs))
    }

    /// Answer what a cancelled execution left unanswered in its transcript
    /// (theseus-0o8): the calls of its last assistant message that no result
    /// answers, and the jobs it ended whose placeholder never got their end.
    /// A cancelled execution takes no more turns, so no turn writes them. It
    /// runs where a cancel's last work is done, once no turn holds the
    /// execution: after the cancel has stopped what it could
    /// (`Core::cancel_execution`), and at the end of the turn that held the
    /// execution when it came, which owns its transcript until then. It
    /// runs under the execution's lock and answers each call once, so those
    /// two cannot both write one. Returns the nodes written, for the caller
    /// to announce.
    pub fn answer_after_cancel(
        &self,
        kernel: &Kernel,
        store: &Store,
        session_id: &str,
        execution_id: &str,
    ) -> Result<Vec<Node>> {
        let mut written = Vec::new();
        let mut outputs = Vec::new();
        kernel.frame(&[execution_id], |k| {
            let Some(e) = k.execution(execution_id)? else {
                return Ok(());
            };
            if e.state != ExecState::Cancelled || k.holds_turn(execution_id) {
                return Ok(());
            }
            let (nodes, raw) = self.cancelled_results(k, store, session_id, &e)?;
            let records = nodes.iter().map(Node::record).collect::<Result<Vec<_>>>()?;
            k.stage(&records)?;
            (written, outputs) = (nodes, raw);
            Ok(())
        })?;
        // The nodes are written, so the jobs' raw output goes, as for a
        // result read within a turn (`answer_job`, theseus-wz2).
        for a in &outputs {
            self.remove_job_output(a);
        }
        Ok(written)
    }

    /// The result nodes for `answer_after_cancel`, and the jobs whose raw output
    /// they read.
    fn cancelled_results(
        &self,
        kernel: &Kernel,
        store: &Store,
        session_id: &str,
        e: &Execution,
    ) -> Result<(Vec<Node>, Vec<Action>)> {
        let nodes: crate::store::Transcript = store
            .session_nodes(session_id)?
            .into_iter()
            .map(|(pos, n)| (pos, Arc::new(n)))
            .collect();
        let (mut out, mut raw) = (Vec::new(), Vec::new());
        let mut write = |at: &Node, r: ResultNode<'_>, job: Option<&Action>| {
            out.push(self.result_node_in(session_id, at.turn_id.as_deref(), at.loop_index, r));
            raw.extend(job.cloned());
        };
        // The last assistant message's calls that nothing answers: planned and
        // ended by the cancel, dispatched and stopped by it, or never planned.
        if let Some((assistant, pending)) = unanswered(&nodes) {
            for u in pending {
                let call = nodes.iter().find_map(|(_, n)| match &n.body {
                    Body::ToolCall { tool_use_id, .. } if *tool_use_id == u.id => Some(&**n),
                    _ => None,
                });
                let corr = call.and_then(|n| match &n.body {
                    Body::ToolCall {
                        correlation_id: Some(c),
                        ..
                    } => Some(c.as_str()),
                    _ => None,
                });
                let action = corr.map(|c| kernel.action(c)).transpose()?.flatten();
                let (_, tool) = self.tool_of(&u);
                let answer = match &action {
                    Some(a) => self.cancelled_call(store, e, a, &u.id, &tool),
                    None => Some((
                        ResultNode::new(
                            &u.id,
                            &tool,
                            ResultStatus::Cancelled,
                            format!("Not run: {}.", cancel_why(e)),
                        ),
                        false,
                    )),
                };
                if let Some((r, job)) = answer {
                    write(
                        call.unwrap_or(assistant),
                        r,
                        job.then_some(action.as_ref()).flatten(),
                    );
                }
            }
        }
        // A job answered `background` and ended by the cancel, or settled
        // before it and never taken: the end a later turn writes as a late
        // result (theseus-kol), which a cancelled execution never takes.
        let ended: HashSet<&str> = nodes
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    tool_use_id,
                    late: true,
                    ..
                } => Some(tool_use_id.as_str()),
                _ => None,
            })
            .collect();
        for (_, n) in &nodes {
            let Body::ToolResult {
                tool_use_id,
                tool,
                status: ResultStatus::Background,
                correlation_id: Some(c),
                late: false,
                ..
            } = &n.body
            else {
                continue;
            };
            if ended.contains(tool_use_id.as_str()) {
                continue;
            }
            let Some(a) = kernel.action(c)? else {
                continue;
            };
            if let Some((r, job)) = self.cancelled_call(store, e, &a, tool_use_id, tool) {
                write(n, ResultNode { late: true, ..r }, job.then_some(&a));
            }
        }
        Ok((out, raw))
    }

    /// How one call of a cancelled execution ended, as its result: the
    /// result, and whether it is a job's, whose raw output goes once its node
    /// is written. None while the cancel is still stopping it: the sweep
    /// after the call settles answers it.
    fn cancelled_call<'a>(
        &self,
        store: &Store,
        e: &Execution,
        a: &'a Action,
        tool_use_id: &'a str,
        tool: &'a str,
    ) -> Option<(ResultNode<'a>, bool)> {
        let job = self
            .registry
            .get(tool)
            .is_some_and(|t| t.backend() == Backend::Job);
        let why = cancel_why(e);
        let says = |status, text: String, cancel: &str| {
            let mut meta = json!({"cancel": cancel});
            crate::cancel::stopped_meta(&mut meta, status, a);
            ResultNode {
                correlation_id: Some(&a.correlation_id),
                meta,
                ..ResultNode::new(tool_use_id, tool, status, text)
            }
        };
        Some(match (a.state, a.cancel) {
            // Still being stopped.
            (ActionState::Dispatched, _) => return None,
            // Never sent: ended in the cancel's frame, or declined before it.
            (ActionState::Planned | ActionState::Authorized, _)
            | (ActionState::Cancelled, None) => {
                let (status, text) = if a.state == ActionState::Cancelled {
                    not_run_answer(a)
                } else {
                    (ResultStatus::Cancelled, format!("Not run: {why}."))
                };
                let mut meta = Value::Null;
                crate::cancel::stopped_meta(&mut meta, status, a);
                let r = ResultNode {
                    correlation_id: Some(&a.correlation_id),
                    meta,
                    ..ResultNode::new(tool_use_id, tool, status, text)
                };
                (r, false)
            }
            // Told to stop while it ran. What a job printed before it stopped
            // is in its result; a call that cannot be stopped, or that was
            // not verified gone, may have run: unknown, never "not sent".
            (ActionState::Cancelled, Some(c)) => {
                let (status, how, tag) = match c {
                    CancelState::TerminationVerified => (
                        ResultStatus::Cancelled,
                        // A job's head line says how; another call's line does.
                        crate::cancel::words(a)
                            .filter(|_| !job)
                            .map_or("it was stopped".into(), |w| format!("it was stopped ({w})")),
                        "stopped",
                    ),
                    CancelState::Unsupported => (
                        ResultStatus::Unknown,
                        "it cannot be stopped once started, so it may have finished: check the \
                         current state before relying on it"
                            .to_string(),
                        "unsupported",
                    ),
                    _ => (
                        ResultStatus::Unknown,
                        format!(
                            "it was told to stop, but its end was not verified{}, so it may still be \
                             running: check the current state before relying on it",
                            a.verdict.as_ref().and_then(|v| v.why.as_deref()).map_or(String::new(), |w| format!(" ({w})"))
                        ),
                        "uncertain",
                    ),
                };
                let line = format!("{} while this call was running; {how}.", sentence(&why));
                if job {
                    let mut r = self.job_result(store, a, tool_use_id, tool);
                    r.status = status;
                    r.text = format!("[{line}]\n{}", r.text);
                    r.meta["cancel"] = json!(tag);
                    (r, true)
                } else {
                    (says(status, line, tag), false)
                }
            }
            // Settled before the cancel, its result never read: a job's is
            // in the spool; an in-process call's was written with its
            // settle, so only a lost one reaches here.
            (ActionState::Succeeded | ActionState::Failed | ActionState::OutcomeUnknown, _) => {
                if job {
                    (self.job_result(store, a, tool_use_id, tool), true)
                } else {
                    let line = format!(
                        "{}. This call settled as {} and its result was never recorded: check \
                         the current state before relying on it.",
                        sentence(&why),
                        a.state.as_str()
                    );
                    (says(ResultStatus::Unknown, line, "settled"), false)
                }
            }
        })
    }
}

/// Tell a session's clients about the results `ToolRuntime::answer_after_cancel`
/// wrote, as a turn tells them of its own (`announce_end`).
pub(crate) fn announce_cancelled(rec: &crate::fact::Rec<'_>, session_id: &str, nodes: &[Node]) {
    for node in nodes {
        rec.record(&fact::tool::ToolEnded {
            session_id,
            turn_id: node.turn_id.as_deref().unwrap_or_default(),
            node,
        });
        rec.record(&fact::turn::NodeWritten { session_id, node });
    }
}

/// Why a cancelled execution ended, as its calls' results say it: "the
/// execution was cancelled by operator".
fn cancel_why(e: &Execution) -> String {
    format!(
        "the execution was {}",
        e.ended_reason.as_deref().unwrap_or("cancelled")
    )
}

/// `text` with a capital first letter, for the start of a sentence.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// What a call that never ran tells the model: who declined it, that nobody
/// answered its question in time, or why it was cancelled (its action's
/// resolution).
fn not_run_answer(a: &Action) -> (ResultStatus, String) {
    // An expired question is no one's decline (theseus-830).
    if let Some(answer) = crate::rpc::expired_answer(a) {
        return answer;
    }
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
        crate::cancel::stopped_meta(&mut meta, status, a);
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
pub(crate) struct Gated {
    pub(crate) plan: Plan,
    pub(crate) decision: Decision,
    pub(crate) proposal: Proposal,
    pub(crate) record: GateRecord,
    pub(crate) class: Class,
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
    /// Planned and never asked: a restart came between its plan and its
    /// authorization, and its gate said `allow` (theseus-ni5).
    NeverAsked(Action),
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
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
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
    // No L1 view shows the floor or the approve list's paths (M4 17b).
    let sandbox = Arc::new(Sandbox::new(&cfg.sandbox, &roots, &floor_paths, &approve));
    let cpu = crate::cpu::CpuPool::for_host();
    // AWS (row 29, C1): its tools when the config binds an account. Nothing
    // runs until a call, or the daemon's check after serving.
    let aws = crate::aws::Aws::from_config(&cfg.aws, secrets.clone()).filter(|_| t.enabled);
    let registry = if t.enabled {
        let mut r = theseus_tools::default_registry();
        // The web tools wait on the network, as async tools (DD5).
        let web = crate::web::Web::new(&t.web, t.result_max_chars, cpu.clone());
        for tool in web.tools() {
            r.register(tool);
        }
        for tool in aws.iter().flat_map(|a| a.tools()) {
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
            aws: cfg.policy.aws.clone(),
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
            aws: None,
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
        aws,
        question_due: Default::default(),
        sandbox,
        stops: Default::default(),
    })
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
