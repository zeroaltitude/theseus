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

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_kernel::job::{spawn_detached, WrapperArgs};
use theseus_kernel::{
    Accepted, Action, Completion, Kernel, Outcome, Proposal, RetryClass, Spool, TurnGuard,
};
use theseus_protocol::{ConfirmRequest, GateRecord, GateResult, LedgerKind, PolicyNotified};
use theseus_tools::{Access, Backend, Plan, Registry, Retry, Tool, ToolClass, ToolCtx};

use crate::broker::Broker;
use crate::bus::EventSink;
use crate::fact;
use crate::ledger::LedgerRow;
use crate::narrative::{self, Narrator};
use crate::node::{Body, Node, ResultStatus};
use crate::policy::{Decision, Posture, ToolPolicy};
use crate::provider::ToolUse;
use crate::sandbox::{self, Sandbox};
use crate::scrub::Scrubber;
use crate::store::Store;

mod hands;
mod job;
mod late;
mod resume;
mod waits;

pub(crate) use late::{announce_cancelled, not_run_results};
pub use waits::{JobDone, JobWaits, Waiting};

/// Starts a job. The real one spawns `theseusd job-wrapper` detached, whose
/// report reaches the waiting turn through the notify socket and the drain;
/// tests substitute one that runs the command on a thread, spools the
/// result, and wakes the turn with `done` (Tier 7.1).
pub trait JobLauncher: Send + Sync {
    fn launch(&self, spool: &Spool, args: &WrapperArgs, done: JobDone) -> Result<u32>;
}

pub struct WrapperLauncher {
    pub self_exe: PathBuf,
}

impl JobLauncher for WrapperLauncher {
    fn launch(&self, spool: &Spool, args: &WrapperArgs, _: JobDone) -> Result<u32> {
        spawn_detached(
            &self.self_exe,
            &[theseus_kernel::job::WRAPPER_MODE],
            spool,
            args,
        )
    }
}

/// Runs the wrapper's body on a thread in this process (tests), and wakes
/// the turn once the job's completion is spooled.
pub struct InlineLauncher;

impl JobLauncher for InlineLauncher {
    fn launch(&self, _spool: &Spool, args: &WrapperArgs, done: JobDone) -> Result<u32> {
        let a = args.clone();
        std::thread::spawn(move || {
            let _ = theseus_kernel::job::run_wrapper(&a);
            done.wake();
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
    /// The class of the place the turn's words go to (the place rule,
    /// theseus-nbsh), fixed once it has taken its wakes and reports: a
    /// shared place's calls are only the public tools (`places::refusal`).
    pub class: crate::places::PlaceClass,
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
    /// Programs whose output is outside text (theseus-b5cl, `[policy]
    /// external_programs`): a job that runs one marks its result external.
    pub external_programs: Vec<String>,
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
    /// `[places] public_paths`, expanded and canonical: all a shared place's
    /// file tools reach (the place rule, theseus-nbsh).
    pub public_roots: Vec<PathBuf>,
    /// The jobs turns wait on (Tier 7.1): the drain leaves each one's
    /// completion to its turn, and wakes it.
    pub job_waits: Arc<JobWaits>,
    /// The MCP servers' tools offered now (M7 36b), which the board fills;
    /// offered after the built-ins, in private places only.
    pub mcp: Arc<crate::mcp::McpCatalog>,
    /// The sessions' terminals (`term.*`, theseus-n88g.4).
    pub terms: Arc<crate::term::Terms>,
    /// The language-server board (L2), when `[lsp]` is on.
    pub lsp: Option<Arc<crate::lsp::Board>>,
}

const INPROC_DEADLINE_MS: u64 = 120_000;

/// An in-process or async call's deadline: its tool's own, else the default.
fn inproc_deadline_ms(tool: &dyn Tool) -> u64 {
    tool.deadline()
        .map_or(INPROC_DEADLINE_MS, |d| d.as_millis() as u64)
}

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
            external_programs: Vec::new(),
            output_max_bytes: theseus_kernel::job::DEFAULT_OUTPUT_MAX_BYTES,
            disk: Arc::new(crate::disk::Disk::new(tmp, 0, 0)),
            aws: None,
            question_due: Default::default(),
            sandbox: Arc::new(Sandbox::new(&Default::default(), &[], &[], &[])),
            stops: Default::default(),
            public_roots: Vec::new(),
            job_waits: Arc::default(),
            mcp: Default::default(),
            terms: Arc::new(crate::term::Terms::new(Vec::new(), Vec::new())),
            lsp: None,
        }
    }

    /// A tool by its canonical name: a built-in, or an MCP server's.
    pub fn tool(&self, name: &str) -> Option<Arc<dyn Tool>> {
        match self.registry.get(name) {
            Some(t) => Some(t.clone()),
            None if name.starts_with(crate::policy::MCP_PREFIX) => self.mcp.get(name),
            None => None,
        }
    }

    /// A tool by the name the model called it.
    pub fn tool_by_wire(&self, wire: &str) -> Option<Arc<dyn Tool>> {
        match self.registry.by_wire(wire) {
            Some(t) => Some(t.clone()),
            None if wire.starts_with("mcp__") => self.mcp.by_wire(wire),
            None => None,
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
        // A terminal's program is given no grant: its keys come from the
        // model, and its screen is the model's to read (theseus-n88g.4).
        if tool.starts_with(crate::term::FAMILY)
            && tool[crate::term::FAMILY.len()..].starts_with('.')
        {
            return d;
        }
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

    /// The tools a place of `class` is offered (the place rule): a shared
    /// place's model never sees one it may not use.
    pub fn definitions_for(&self, class: crate::places::PlaceClass) -> Vec<Value> {
        let offered = |name: &str| crate::places::offered(class, name, &self.public_roots);
        let mut defs = self.registry.definitions_of(true, offered);
        // The MCP servers' tools, after the built-ins, by canonical name (the
        // catalog's order): none in a shared place (the place rule).
        if self.enabled() {
            defs.extend(
                self.mcp
                    .all()
                    .iter()
                    .filter(|t| offered(&t.canonical))
                    .map(|t| {
                        json!({
                            "name": t.wire,
                            "description": t.description,
                            "input_schema": Tool::input_schema(t.as_ref()),
                            "eager_input_streaming": true,
                        })
                    }),
            );
        }
        defs
    }

    /// The paragraph of the system prompt that describes the tools and their
    /// limits, for a private place. Deterministic for a given config, so it
    /// never churns the cache.
    pub fn system_note(&self) -> String {
        self.system_note_for(crate::places::PlaceClass::Private)
    }

    /// The tools paragraph for a place of `class`: a shared place's names only
    /// the tools it is offered, and says what it may reach, and why (the place
    /// rule). One per class, so it never churns the cache.
    pub fn system_note_for(&self, class: crate::places::PlaceClass) -> String {
        if !self.enabled() {
            return String::new();
        }
        let offered = |n: &str| crate::places::offered(class, n, &self.public_roots);
        // Each tool's posture, grouped in ladder order: "open: fs.glob, …; notify: …".
        let postures: Vec<String> = crate::policy::Posture::ALL
            .iter()
            .filter_map(|p| {
                let names: Vec<&str> = self
                    .registry
                    .all()
                    .map(|t| t.name())
                    .filter(|n| offered(n) && self.policy.posture(n).0 == *p)
                    .collect();
                (!names.is_empty()).then(|| format!("{}: {}", p.as_str(), names.join(", ")))
            })
            .collect();
        if class == crate::places::PlaceClass::Shared {
            let files = match self.public_roots.is_empty() {
                true => "no files".to_string(),
                false => format!(
                    "files only under {} (give their absolute paths)",
                    crate::places::shown(&self.public_roots)
                ),
            };
            return format!(
                "Tools. You act through tools; every call is recorded, checked against policy, and may wait for the operator's confirmation.\n\
                 - This place is shared: people besides the operator read it. So you are offered only the public tools here, and {files}: nothing of the operator's own (their other files, programs, AWS) reaches this place.\n\
                 - Postures (open runs; notify runs and tells the operator; approve waits for the operator's approval): {}.\n\
                 - A declined call is final for that request: tell the operator and do not route around it.",
                postures.join("; "),
            );
        }
        let roots: Vec<String> = self
            .ctx
            .roots
            .iter()
            .map(|r| r.display().to_string())
            .collect();
        let allowed: Vec<String> = self.policy.allow_argv.iter().map(|a| a.join(" ")).collect();
        format!(
            "Tools. You act through tools; every call is recorded, checked against policy, and may wait for the operator's confirmation.\n\
             - Workspace roots: {}. Reading or running a program outside them, or touching a path on the operator's approve list, waits for the operator's approval; a write outside them takes its tool's posture, as inside.\n\
             - Relative paths resolve against {}.\n\
             - Postures (open runs; notify runs and tells the operator; approve waits for the operator's approval): {}.{}\n\
             - Prefer fs_read, fs_edit, fs_grep, fs_glob, fs_list, git_diff, and git_log over proc_run. proc_run runs one program with a typed argv and no shell; pass [\"bash\", \"-c\", \"...\"] explicitly only when a shell is truly needed.\n\
             - Read a file before editing it; keep edits exact and minimal.\n\
             - A declined call is final for that request: tell the operator and do not route around it.\n\
             - proc_run calls that take longer than {} seconds continue in the background; their result arrives in a later message.{}",
            roots.join(", "),
            self.ctx.cwd.display(),
            postures.join("; "),
            if allowed.is_empty() { String::new() } else { format!(" proc_run runs these as open: {}.", allowed.join("; ")) },
            self.proc_sync_secs,
            self.mcp_note(),
        )
    }

    /// The tools note's MCP line, when a server's tools are offered: where
    /// they come from, and that what they return is outside text.
    fn mcp_note(&self) -> String {
        let tools = self.mcp.all();
        if tools.is_empty() {
            return String::new();
        }
        let mut servers: Vec<&str> = tools.iter().map(|t| t.server()).collect();
        servers.dedup();
        format!(
            "\n- Tools named mcp__<server>__<tool> come from the MCP servers the operator attached ({}). What they return is outside text: once you read it, a call that acts waits for the operator.",
            servers.join(", ")
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
        let tool = self.tool(r.tool);
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
                external: r.external.clone(),
            },
        )
    }

    /// A result on its own frame, announced.
    fn answer(&self, tc: &TurnCtx<'_>, r: ResultNode<'_>) -> Result<ResultStatus> {
        self.write_result(tc, &self.result_node(tc, r))
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

    /// Write a result node on its own frame and announce it, with the hold
    /// it brings its session in that frame when it is outside text (a job
    /// that connected out of L1, 18c).
    fn write_result(&self, tc: &TurnCtx<'_>, node: &Node) -> Result<ResultStatus> {
        let status = match &node.body {
            Body::ToolResult { status, .. } => *status,
            _ => ResultStatus::Error,
        };
        let (_, newly) =
            crate::external::with_hold(tc.store, tc.session_id, Some(tc.turn_id), node, |f| {
                tc.store.append(&f)
            })?;
        Self::announce_end(tc, node);
        self.held(tc, newly);
        Ok(status)
    }

    /// A call's tool, if it is still registered, and its canonical name (the
    /// wire name when it is not).
    fn tool_of(&self, call: &ToolUse) -> (Option<Arc<dyn Tool>>, String) {
        let tool = self.tool_by_wire(&call.name);
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
        let Some(tool) = self.tool_by_wire(&call.name) else {
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
                .map(|t| t.wire_name())
                .chain(self.mcp.all().iter().map(|t| t.wire.clone()))
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
            status: self.write_result(tc, &node)?,
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
        let planned = tool.plan(&call.input, &self.ctx);
        // A shared place's call reaches only what the place may (the place
        // rule): the catalog offers nothing else, and this refuses it, in case.
        let refused = planned.as_ref().ok().and_then(|plan| {
            crate::places::refusal(tc.class, tool.name(), plan, &self.public_roots)
        });
        let planned = match &refused {
            Some(why) => Err(why.clone()),
            None => planned,
        };
        let planned = planned.map(|plan| {
            let t = tightened.as_ref().map(crate::tighten::as_tightened);
            let (decision, job_class) = sandbox::decide(self, tool, &plan, &call.input, t);
            // A call that starts a language server is a run too (L2).
            let decision = crate::lsp::gate(self, tool, &plan, decision);
            // A private address's card in a shared place says where the page
            // goes (theseus-94a6).
            let decision = crate::places::private_fetch(tc.class, &plan, decision);
            // After the whole order (theseus-9bp): a call that acts in a
            // session that read external text waits. A read and a one-shot
            // `wake.at` keep their postures (T1b), and cost no record read;
            // a repeating wake is persistence, and is held (37a).
            let class = plan.class.unwrap_or(tool.class());
            let held = if crate::external::exempt(class, tool.name(), &call.input) {
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
                &call.input,
                &plan.summary,
            );
            // An MCP client's session (step 41b): its calls that act wait.
            let floor = mcp_floor(tc);
            let decision =
                crate::mcp_server::floor(decision, class, floor, tool.name(), &plan.summary);
            (plan, decision, job_class)
        });
        let result = match &planned {
            Err(e) => GateResult {
                gate: "deny".into(),
                reason: Some(match refused {
                    Some(_) => format!("{PLACE_REFUSAL}: {e}"),
                    None => format!("validation: {e}"),
                }),
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
        if let Ok((plan, _, bound)) = &planned {
            proposal.resource = plan.resources.first().map(|r| r.path.display().to_string());
            bound.bind(&mut proposal);
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
                refused: refused.is_some(),
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
        let (reason, text) = match bad.refused {
            true => (
                format!("{PLACE_REFUSAL}: {}", bad.error),
                format!("Not run: {}", bad.error),
            ),
            false => (
                format!("validation: {}", bad.error),
                format!("Invalid input: {}", bad.error),
            ),
        };
        let call_node = Self::tool_call_node(tc, assistant_node, call, tool, None, *bad.record);
        let node = self.result_node(
            tc,
            ResultNode {
                meta: json!({"reason": reason}),
                ..ResultNode::new(&call.id, tool, ResultStatus::Error, text)
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
            Backend::Inproc | Backend::Async | Backend::Harness => inproc_deadline_ms(tool),
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
        class: sandbox::Bound,
    ) -> Result<CallOutcome> {
        if tool.name() == crate::aws::hands::RUN {
            return self
                .run_hands(tc, correlation_id, tool.as_ref(), call)
                .await;
        }
        match tool.backend() {
            Backend::Inproc | Backend::Async => {
                self.run_inproc(tc, correlation_id, tool, call, ran_at)
                    .await
            }
            Backend::Job => {
                self.run_job(tc, correlation_id, tool.as_ref(), call, ran_at, &class)
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
            outcome: match status {
                ResultStatus::Ok => Outcome::Succeeded,
                ResultStatus::Unknown => Outcome::Unknown,
                _ => Outcome::Failed,
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
        let deadline_ms = inproc_deadline_ms(tool.as_ref());
        let deadline = Duration::from_millis(deadline_ms);
        let timed_out = || format!("timed out after {deadline_ms} ms");
        let mut aborted = false;
        // An async tool's failure may say more (an MCP server's, M7 36b):
        // that its outcome is unknown, or that its text is outside text.
        let mut failed = Value::Null;
        let (started, outcome, took) = if tool.backend() == Backend::Async {
            // A task of its own, so a panic is the call's error and not the
            // turn's, and its deadline can stop it. Only an approved call
            // reaches the private address it names.
            ctx.approved = ran_at == Posture::Approve;
            let started = theseus_protocol::now_unix_ms();
            let t0 = Instant::now();
            // A terminal's call needs its session (theseus-n88g.4).
            let run = match tool.family() == crate::term::FAMILY {
                true => self.terms.run(tool.name(), tc.session_id, &input, &ctx),
                false => t.run_async(&input, &ctx),
            };
            let mut task = tokio::spawn(run);
            let task_id = task.id();
            // A cancel or a stop aborts it (M4 18a).
            let _reachable = self.stops.track(correlation_id, task.abort_handle());
            let outcome = match tokio::time::timeout(deadline, &mut task).await {
                Ok(Ok(Ok((out, external)))) => Ok((out, None, external)),
                Ok(Ok(Err(f))) => {
                    failed = f.meta;
                    Err(f.message)
                }
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
            // Its language-server requests are its call's spans (L2).
            if let Some(l) = &self.lsp {
                l.bind(task_id, &call.id);
            }
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
        let (status, mut text, mut meta, img, external) = match (outcome, &by_cancel) {
            (_, Some(a)) => {
                let (status, text, meta) = crate::cancel::aborted_result(a);
                (status, text, meta, None, None)
            }
            (Ok((o, img, external)), None) => (ResultStatus::Ok, o.text, o.meta, img, external),
            (Err(m), None) => failure(m, failed),
        };
        // An edit's diagnostics, and what arrived since for the session's
        // pending files, in a private place only (L3); none for a call a
        // cancel settled.
        let settled = by_cancel.is_none().then_some(status);
        self.lsp_onto(tc, &call.id, tool.name(), settled, &mut text, &mut meta)
            .await;
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
        // A terminal's open and close (theseus-n88g.4).
        if tool.family() == crate::term::FAMILY && by_cancel.is_none() {
            fact::term::of_result(&tc.rec(), &meta);
        }
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
        for row in aws.iter().flat_map(|a| a.sessions()) {
            tc.record(&fact::tool::AwsSessionMinted { row: &row });
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
        let sid = tc.session_id;
        let (accepted, newly) =
            crate::external::with_hold(tc.store, sid, Some(tc.turn_id), node, |frame| {
                tc.kernel.accept_completion_with(c, frame)
            })?;
        self.held(tc, newly);
        Ok(accepted)
    }

    /// The narrative's line for a hold a result began.
    fn held(&self, tc: &TurnCtx<'_>, newly: Option<theseus_protocol::ExternalText>) {
        if let Some(h) = newly {
            let mode = self.external_text;
            tc.record(&fact::tool::HoldTaken { hold: &h, mode });
        }
    }
}
/// An async call's failure, as its result: `unknown` when the tool says its
/// outcome is (a connection that ended while it waited), and outside text
/// when the tool says its words are (an MCP server's error result).
#[allow(clippy::type_complexity)]
fn failure(
    message: String,
    meta: Value,
) -> (
    ResultStatus,
    String,
    Value,
    Option<theseus_tools::ImageData>,
    Option<theseus_tools::External>,
) {
    let status = match meta.get("outcome_unknown").and_then(Value::as_bool) {
        Some(true) => ResultStatus::Unknown,
        _ => ResultStatus::Error,
    };
    let external = meta
        .get("external")
        .and_then(Value::as_str)
        .map(|url| theseus_tools::External { url: url.into() });
    (status, message, meta, None, external)
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
    pub(crate) class: sandbox::Bound,
}

/// A call whose input the toollet refused, or that its place may not make
/// (`refused`, the place rule); its gate record is still stored.
struct Invalid {
    record: Box<GateRecord>,
    error: String,
    refused: bool,
}

/// The gate record's reason, and the result's word, for a call its place
/// may not make (the place rule, theseus-nbsh).
pub const PLACE_REFUSAL: &str = "place";

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

/// The workspace's roots, the floor's paths, and the approve list's, from
/// `cfg` with `state` the state dir in use: what the gate guards, and what
/// no L1 view shows (M4 17b).
fn guarded_paths(
    cfg: &crate::Config,
    state: &std::path::Path,
) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
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
    // The floor: Theseus's own state (its store, spool, and bindings file, in
    // the state dir actually in use) and the 1Password CLI's credentials.
    let canon_path = |p: PathBuf| theseus_tools::paths::canonical_best_effort(&p);
    let mut floor_paths = vec![
        canon_path(state.join("store")),
        canon_path(state.join("spool")),
        canon_path(cfg.discord.bindings_path(state)),
        canon("~/.config/op"),
    ];
    // The token file the daemon was given, by flag or by environment, and
    // the config note's last-known-good copy (theseus-2fo).
    for f in cfg.op_token_file.iter().chain(&cfg.config_copy) {
        floor_paths.push(canon_path(f.clone()));
    }
    let approve: Vec<PathBuf> = t.approve_paths.iter().map(|p| canon(p)).collect();
    (roots, floor_paths, approve)
}

/// L1's state for `cfg`, with `state` the state dir in use, as the runtime
/// builds it: `theseusd check` runs the self-test over the view a job gets
/// (theseus-gyin).
pub fn sandbox_for(cfg: &crate::Config, state: &std::path::Path) -> Sandbox {
    let (roots, floor, approve) = guarded_paths(cfg, state);
    Sandbox::new(&cfg.sandbox, &roots, &floor, &approve)
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
    let state = spool
        .as_ref()
        .and_then(|s| s.dir().parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| cfg.state_dir());
    let (roots, floor_paths, approve) = guarded_paths(cfg, &state);
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
    // No L1 view shows the floor or the approve list's paths (M4 17b).
    let sandbox = Arc::new(Sandbox::new(&cfg.sandbox, &roots, &floor_paths, &approve));
    let cpu = crate::cpu::CpuPool::for_host();
    // AWS (row 29, C1): its tools when the config binds an account. Nothing
    // runs until a call, or the daemon's check after serving.
    let aws = crate::aws::Aws::from_config(&cfg.aws, secrets.clone()).filter(|_| t.enabled);
    let proc_env: Vec<(String, String)> = t
        .proc_env
        .iter()
        .filter(|k| !forbidden_env(k))
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.clone(), v)))
        .collect();
    // The language servers (L2): their tools when [lsp] is on. Nothing
    // starts until a call for a file of a server's language.
    let lsp = (t.enabled && cfg.lsp.enabled)
        .then(|| crate::lsp::Board::new(&cfg.lsp, roots.clone(), proc_env.clone(), &state));
    let mut registry = if t.enabled {
        let mut r = theseus_tools::default_registry();
        // The web tools wait on the network, as async tools (DD5).
        let web = crate::web::Web::new(&t.web, t.result_max_chars, cpu.clone());
        for tool in web.tools() {
            r.register(tool);
        }
        for tool in aws.iter().flat_map(|a| a.tools()) {
            r.register(tool);
        }
        for tool in lsp.iter().flat_map(|b| b.tools()) {
            r.register(tool);
        }
        // Task sessions (DD7) and wakes (DD8): the harness runs them.
        r.register(Arc::new(crate::task::TaskCreate));
        r.register(Arc::new(crate::wake::WakeAt));
        r
    } else {
        Registry::new()
    };
    // Terminals (theseus-n88g.4): each program gets the job environment.
    let terms = Arc::new(crate::term::Terms::new(
        proc_env.clone(),
        cfg.policy.external_programs.clone(),
    ));
    if t.enabled {
        for tool in crate::term::tools::all(&terms) {
            registry.register(tool);
        }
    }
    let notify_socket = spool.as_ref().map(|s| s.dir().join("notify.sock"));
    // A program's name is resolved on the daemon's own PATH (theseus-dcy).
    let broker = Broker::new(&cfg.broker, secrets, std::env::var("PATH").ok());
    // web.search's key: its calls run at no looser a posture than the
    // key's, and health lists the grant with its uses (DD5).
    if t.enabled {
        broker.grant_tool("web.search", &t.web.search_key_secret);
    }
    // A program's AWS job session comes from these accounts (C2).
    if let Some(a) = &aws {
        broker.set_aws(a.clone());
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
        external_programs: cfg.policy.external_programs.clone(),
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
        public_roots: crate::places::public_roots(cfg),
        job_waits: Arc::default(),
        mcp: Default::default(),
        terms,
        lsp,
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

/// The floor of the turn's execution, when an MCP client opened its session
/// (step 41b, `mcp_server::floor_of`). An execution that cannot be read is
/// floored at `approve`: the gate never guesses the looser way.
fn mcp_floor(tc: &TurnCtx<'_>) -> Option<Posture> {
    match tc.kernel.execution(tc.execution_id) {
        Ok(Some(e)) => crate::mcp_server::floor_of(&e.authority),
        Ok(None) => None,
        Err(_) => Some(Posture::Approve),
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
}
