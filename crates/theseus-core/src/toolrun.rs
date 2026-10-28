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

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::job::{spawn_detached, WrapperArgs};
use theseus_kernel::{
    Accepted, Action, Completion, Kernel, Outcome, Proposal, RetryClass, Spool, TurnGuard,
};
use theseus_protocol::{ConfirmRequest, GateRecord, GateResult, LedgerKind, PolicyNotified};
use theseus_tools::{Access, Backend, Plan, Registry, Retry, Tool, ToolCtx};

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

mod guarded;
use guarded::guarded_paths;
pub(crate) mod draft_roots;
pub use guarded::sandbox_for;
pub(crate) mod batch;
mod calls;
mod flow_step;
pub(crate) use flow_step::Stepped;
pub mod catalogs;
mod complete;
pub mod fit;
mod glide;
pub(crate) mod handles;
mod hands;
pub(crate) mod job;
pub mod jobs_tools;
mod kept;
mod late;
mod mcp_floor;
pub mod misroute;
mod not_started;
pub(crate) mod note;
mod operands;
pub(crate) mod order;
mod parallel;
mod peek;
mod proc_cap;
mod proposal;
mod resume;
mod self_pr;
pub(crate) mod session_dir;
mod steps;
pub mod waits;

use mcp_floor::mcp_floor;
pub(crate) use mcp_floor::mcp_floor_of;

pub(crate) use batch::Turned;
pub use late::LateCall;
pub(crate) use late::{announce_cancelled, not_run_results};
pub use peek::{last_line, newest, peek, Newest, Peek};
pub use proposal::confirm_proposal;
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
    /// That place's ceiling (step 38a), taken with its class: a floor, and
    /// the tools it offers (`ceiling.rs`).
    pub ceiling: Option<&'static crate::ceiling::Ceiling>,
    /// Set for a China-origin draft's session (`draft_view.rs`): its requests
    /// take the stripped prefix, and its calls are gated in the draft's reach.
    pub draft: Option<&'static crate::draft_view::DraftReach>,
    /// The bound places, as a glide names them (38b).
    pub places: crate::places::Places<'a>,
    /// The session's directory, as its client sent it (theseus-aab7):
    /// where its calls work (`ToolRuntime::ctx_in`). None: `[tools] cwd`.
    pub dir: Option<&'a str>,
    /// The conversation's pending wakes as the turn last knew them: what its
    /// requests' commitments block shows (theseus-lherl), set by the turn's
    /// read of its wakes and by each wake tool's frame.
    pub commitments: &'a crate::commitments::Pending,
    /// The threads, as `thread.note` writes them (`thread_context`).
    pub threads: &'a crate::threads::Threads,
}

impl TurnCtx<'_> {
    /// Where the turn's words go: its class and its ceiling.
    pub fn place(&self) -> crate::ceiling::PlaceView {
        crate::ceiling::PlaceView {
            class: self.class,
            ceiling: self.ceiling,
            voice: false,
            draft: self.draft,
        }
    }
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
#[derive(Debug, Clone)]
pub struct Ran {
    /// Its place among the calls.
    pub index: usize,
    pub outcome: CallOutcome,
    pub started: Instant,
    pub ended: Instant,
    /// The group it ran in, numbered in the order the groups ran. A call
    /// answered at the gate and a question are each a group of one; reads,
    /// writes, and programs group by `parallel`'s rule (theseus-d1hi).
    pub group: usize,
    /// The judgments dispatched at its gate (M5 step 24): the turn's trace
    /// marks each under the call's span.
    pub judged: Vec<crate::judge::gate::Mark>,
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
    /// The calls the continuation answered, or that asked, in the order it
    /// took them, for their spans under the continuation's (theseus-8pei):
    /// each `Ran`'s `index` is its call's place here.
    pub calls: Vec<ToolUse>,
    pub ran: Vec<Ran>,
}

impl ResumeOutcome {
    /// A call the continuation answered alone, since `started`.
    fn answered(&mut self, call: &ToolUse, outcome: CallOutcome, started: Instant) {
        let group = self.next_group();
        self.ran.push(Ran {
            index: self.calls.len(),
            outcome,
            started,
            ended: Instant::now(),
            group,
            judged: Vec::new(),
        });
        self.calls.push(call.clone());
    }

    /// The calls `run_calls` ran for it, with their groups (theseus-a60).
    fn ran_batch(&mut self, calls: &[ToolUse], ran: Vec<Ran>) {
        let (index, group) = (self.calls.len(), self.next_group());
        self.calls.extend_from_slice(calls);
        self.ran.extend(ran.into_iter().map(|r| Ran {
            index: index + r.index,
            group: group + r.group,
            ..r
        }));
    }

    fn next_group(&self) -> usize {
        self.ran.iter().map(|r| r.group + 1).max().unwrap_or(0)
    }
}

pub struct ToolRuntime {
    /// The misroute guard: a turn placed in a thread on a guess (theseus-rha2).
    pub misroute: misroute::Guards,
    /// The catalog each place is offered, kept (`catalogs`).
    pub catalogs: catalogs::Catalogs,
    pub registry: Registry,
    pub policy: ToolPolicy,
    pub ctx: ToolCtx,
    pub spool: Option<Spool>,
    pub scrubber: Arc<Scrubber>,
    pub launcher: Arc<dyn JobLauncher>,
    pub notify_socket: Option<PathBuf>,
    pub result_max_chars: usize,
    pub proc_sync_secs: u64,
    /// The longest one `job_wait` waits (`[tools] job_wait_max_secs`).
    pub job_wait_max_secs: u64,
    /// The most programs one group of a response's calls runs at once
    /// (`[tools] parallel_runs`, theseus-d1hi); the rest wait for a slot.
    pub parallel_runs: usize,
    /// The most reads and writes of a group at once (`[tools] parallel_reads`,
    /// `parallel_writes`, theseus-0hufe).
    pub parallel_reads: usize,
    pub parallel_writes: usize,
    /// The environment every job gets, resolved from the daemon's at startup.
    pub proc_env: Vec<(String, String)>,
    /// Calls per tool since the daemon started. Counting the store's history
    /// instead would put a scan of every node on the start path (§9).
    pub calls: Mutex<BTreeMap<String, u64>>,
    /// The notes fitted calls' results start with (theseus-9dt2).
    pub fit_notes: fit::Notes,
    /// "Should have asked" (theseus-sgh): the tools that ask first because
    /// someone pressed it, from the store. The gate reads them per call.
    pub tightened: crate::tighten::Tightenings,
    /// The conversations' modes and plans (theseus-lyxy), read per call.
    pub modes: crate::policy::modes::Modes,
    /// The daemon's cores (theseus-a60): every in-process toollet runs under
    /// one of its permits.
    pub cpu: Arc<crate::cpu::CpuPool>,
    /// The secret broker (theseus-dcy): what a job's program and a toollet
    /// are given from the board.
    pub broker: Arc<Broker>,
    /// The most a job's raw output file keeps (theseus-102, `[tools]
    /// job_output_max_bytes`).
    pub output_max_bytes: u64,
    /// The free space under the state dir: below its floor a job is refused
    /// (theseus-102), and health reads it.
    pub disk: Arc<crate::disk::Disk>,
    /// The AWS accounts the config binds, behind the `aws.*` tools (AWS
    /// design §3.5); None when it binds none.
    pub aws: Option<Arc<crate::aws::Aws>>,
    pub google: Option<Arc<crate::google::Google>>,
    /// Tools left out of the registry, each with why (theseus-4o4c).
    pub not_offered: Vec<String>,
    /// `file.read`'s reader (theseus-c9l6): Deepgram's key and settings, and
    /// the caps.
    pub files: Arc<crate::file_read::Reader>,
    /// The earliest a waiting question may expire, in ms since the epoch
    /// (theseus-830): the driver reads the questions only once it has come.
    /// 0 until the driver has read them once; each question asked lowers it.
    pub question_due: std::sync::atomic::AtomicU64,
    /// The task claims that hold, kept in memory for the due pass (39b).
    pub leases: crate::task_graph::lease::Leases,
    /// The open watches, in memory (theseus-f2r2b): the `watch` tool adds to
    /// them, the driver's tick checks them. The core shares this board.
    pub watches: Arc<crate::watch::Board>,
    /// `[watch_for]`, for the tool's bounds.
    pub watches_cfg: crate::config::WatchesConfig,
    /// The finished tasks a new task's forecast is made from, and the work
    /// board's view of it (theseus-zz752).
    pub forecasts: Arc<crate::forecast::Book>,
    /// L1 (M4 17b): `[sandbox]`, the class, an L1 job's view, the probe.
    pub sandbox: Arc<Sandbox>,
    /// What a cancel reaches besides jobs, and health's cancel counts (18a).
    pub stops: crate::cancel::Stops,
    /// `[places] public_paths`, expanded and canonical: all a shared place's
    /// file tools reach (the place rule, theseus-nbsh).
    pub public_roots: Vec<PathBuf>,
    /// The draft trees' root: a root, but not in the tools note (theseus-iilre).
    pub drafts_root: Option<PathBuf>,
    /// The sealed holdout sets' path, which the gate refuses every call that
    /// reaches (theseus-pw1q.5).
    pub sealed: crate::rsi::sealed::Guard,
    /// The jobs turns wait on (Tier 7.1): the drain leaves each one's
    /// completion to its turn, and wakes it.
    pub job_waits: Arc<JobWaits>,
    /// Where a capped result's whole output is kept (theseus-v73m).
    pub outputs: crate::outputs::Outputs,
    /// The admission queue (theseus-4g0l): the turn waits in it, and a job's wait lends its slot.
    pub admission: Arc<crate::turn::admission::Admission>,
    /// The MCP servers' tools offered now (M7 36b), which the board fills;
    /// offered after the built-ins, in private places only.
    pub mcp: Arc<crate::mcp::McpCatalog>,
    /// The sessions' terminals (`term.*`, theseus-n88g.4).
    pub terms: Arc<crate::term::Terms>,
    /// The language-server board (L2), when `[lsp]` is on.
    pub lsp: Option<Arc<crate::lsp::Board>>,
    /// Proposed extensions (M7 43a): where they are frozen, and the board
    /// that tries them.
    pub extend: Arc<crate::extend::Extensions>,
    /// The loaded flows (theseus-pw1q.21.4): frozen specs, each a deferred
    /// tool in its scope. Empty by default, and then it adds nothing.
    pub flows: crate::flow::Board,
    /// The judge (M5 step 24): every call that acts goes to `gate` once it
    /// is planned. Set as the core is built; unset, nothing is judged.
    pub judge: std::sync::OnceLock<Arc<crate::judge::JudgeService>>,
    /// What `memory.lookup` reads (theseus-w9qv): the core, once built.
    pub lookup: crate::memory_lookup::Board,
    /// Routines (theseus-pvm5c): the records in memory and the defaults.
    pub routines: crate::routine::Book,
    /// What `thread.read` reads (theseus-f2g0): the core, once built.
    pub thread_read: crate::thread_read::Board,
    /// The deferred tools' counts and each conversation's notes (theseus-7m7w.2).
    pub defer: crate::tools_defer::Board,
    /// Projects (theseus-n5ui2): the defaults of a review and a reminder.
    pub projects: crate::config::ProjectsConfig,
    /// The job tools' core and the sessions' short job ids (theseus-n8gk).
    pub jobs: jobs_tools::Board,
    /// `self.propose_change`'s core and trees (theseus-pw1q.8).
    pub self_pr: Arc<crate::rsi::pr::Board>,
    /// The configured profiles, each what it runs on: a check's `profile`
    /// names one (M5 28a).
    pub profiles: BTreeMap<String, crate::session::TargetRef>,
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
/// much). A job's or a terminal's whole output is kept, and `kept.rs`'s
/// `rest` names the file (theseus-v73m); a tool's own `rest` names a call.
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
            misroute: Default::default(),
            catalogs: Default::default(),
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
                private_addresses: Default::default(),
            },
            ctx: ToolCtx::for_tests(&tmp),
            spool: None,
            scrubber: Arc::new(Scrubber::default()),
            launcher: Arc::new(InlineLauncher),
            notify_socket: None,
            result_max_chars: 30_000,
            proc_sync_secs: 60,
            job_wait_max_secs: crate::config::default_job_wait_max_secs(),
            parallel_runs: crate::config::default_parallel_runs(),
            parallel_reads: crate::config::default_parallel_reads(),
            parallel_writes: crate::config::default_parallel_writes(),
            proc_env: vec![],
            calls: Mutex::new(BTreeMap::new()),
            fit_notes: Default::default(),
            tightened: Default::default(),
            modes: Default::default(),
            cpu: crate::cpu::CpuPool::for_host(),
            broker: Arc::new(Broker::empty()),
            output_max_bytes: theseus_kernel::job::DEFAULT_OUTPUT_MAX_BYTES,
            disk: Arc::new(crate::disk::Disk::new(tmp.clone(), 0, 0)),
            aws: None,
            google: None,
            not_offered: Vec::new(),
            files: Arc::new(crate::file_read::Reader::disabled()),
            question_due: Default::default(),
            leases: crate::task_graph::lease::Leases::new(30 * 60_000),
            watches: Default::default(),
            watches_cfg: Default::default(),
            forecasts: Default::default(),
            sandbox: Arc::new(Sandbox::new(&Default::default(), &[], &[], &[])),
            stops: Default::default(),
            public_roots: Vec::new(),
            drafts_root: None,
            sealed: Default::default(),
            job_waits: Arc::default(),
            outputs: crate::outputs::Outputs::off(),
            admission: crate::turn::admission::Admission::new(8, Duration::from_secs(600)),
            mcp: Default::default(),
            terms: Arc::new(crate::term::Terms::new(Vec::new())),
            lsp: None,
            judge: Default::default(),
            lookup: Default::default(),
            routines: Default::default(),
            thread_read: Default::default(),
            defer: Default::default(),
            projects: Default::default(),
            jobs: Default::default(),
            self_pr: Arc::new(crate::rsi::pr::Board::new(tmp.join("state"))),
            extend: Arc::new(crate::extend::Extensions::new(
                tmp.join("extensions"),
                vec![],
            )),
            flows: crate::flow::Board::new(tmp.join("mint")),
            profiles: BTreeMap::new(),
        }
    }

    /// A tool by its canonical name: a built-in, or an MCP server's.
    pub fn tool(&self, name: &str) -> Option<Arc<dyn Tool>> {
        match self.registry.get(name) {
            Some(t) => Some(t.clone()),
            None if name.starts_with(crate::policy::MCP_PREFIX) => self.mcp.get(name),
            None if crate::flow::is_flow(name) => self.flows.tool(name),
            None => None,
        }
    }

    /// A tool by the name the model called it.
    pub fn tool_by_wire(&self, wire: &str) -> Option<Arc<dyn Tool>> {
        match self.registry.by_wire(wire) {
            Some(t) => Some(t.clone()),
            None if wire.starts_with("mcp__") => self.mcp.by_wire(wire),
            None if wire.starts_with(crate::flow::tool::WIRE_PREFIX) => {
                self.flows.tool_by_wire(wire)
            }
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

    /// The paragraph of the system prompt that describes the tools and their
    /// limits, for a private place. Deterministic for a given config, so it
    /// never churns the cache.
    pub fn system_note(&self) -> String {
        self.system_note_for(crate::places::PlaceClass::Private.into())
    }

    /// The tools paragraph for a place of `class`: a shared place's names only
    /// the tools it is offered, and says what it may reach, and why (the place
    /// rule). One per class and ceiling, so it never churns the cache.
    pub fn system_note_for(&self, place: crate::ceiling::PlaceView) -> String {
        if !self.enabled() {
            return String::new();
        }
        let class = place.class;
        let offered = |n: &str| place.offered(n, &self.public_roots);
        let floor = place.ceiling.and_then(|c| c.floor).unwrap_or_default();
        let ceiling_note = place.ceiling.map(|c| c.note()).unwrap_or_default();
        // Each tool's posture, said as exceptions to the one most tools share.
        let postures = note::say(
            &self
                .registry
                .all()
                .map(|t| t.name())
                .filter(|n| offered(n) && crate::tools_defer::listed(n))
                .map(|n| {
                    (
                        theseus_tools::wire_name(n),
                        self.policy.posture(n).0.max(floor),
                    )
                })
                .collect::<Vec<_>>(),
        );
        if let Some(reach) = place.draft {
            return reach.note(&postures, &ceiling_note);
        }
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
                 - A declined call is final for that request: tell the operator and do not route around it.{}",
                postures,
                ceiling_note,
            );
        }
        let roots: Vec<String> = self
            .ctx
            .roots
            .iter()
            .filter(|r| Some(*r) != self.drafts_root.as_ref())
            .map(|r| r.display().to_string())
            .collect();
        let allowed: Vec<String> = self.policy.allow_argv.iter().map(|a| a.join(" ")).collect();
        format!(
            "Tools. You act through tools; every call is recorded, checked against policy, and may wait for the operator's confirmation.\n\
             - Workspace roots: {}. Reading or running a program outside them, or touching a path on the operator's approve list, waits for the operator's approval; a write outside them takes its tool's posture, as inside.\n\
             - Relative paths resolve against the session's directory, which its own block names.\n\
             - Postures (open runs; notify runs and tells the operator; approve waits for the operator's approval): {}.{}\n\
             - Read a file before editing it; keep edits exact and minimal.\n\
             - {}\n\
             - A declined call is final for that request: tell the operator and do not route around it.{}",
            roots.join(", "),
            postures,
            if allowed.is_empty() { String::new() } else { format!(" proc_run runs these as open: {}.", allowed.join("; ")) },
            crate::task_graph::PLAN_NOTE,
            self.mcp_note(offered) + &ceiling_note,
        )
    }

    /// The tools note's MCP line, when a server's tools are offered here (the
    /// place rule and the place's ceiling, 38a): where they come from, and
    /// that what they return is outside text.
    fn mcp_note(&self, offered: impl Fn(&str) -> bool) -> String {
        let tools = self.mcp.all();
        let mut servers: Vec<&str> = tools
            .iter()
            .filter(|t| offered(&t.canonical))
            .map(|t| t.server())
            .collect();
        if servers.is_empty() {
            return String::new();
        }
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

    /// The node for a result, its text as the tool printed it, capped.
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
        let r = self.fit_noted(r);
        // What the cap leaves out: kept, or the tool's to say (theseus-v73m).
        let (content, truncated, full_ref) = self.capped(session_id, &r, &r.text);
        // A job of the daemon's own CLI names the native call that does it,
        // after the cap so no cut takes it (theseus-8c8s).
        let content = match &r.hint {
            Some(h) => format!("{content}\n{h}"),
            None => content,
        };
        // A call that ran in a group says so, so the model learns what one
        // response may hold (theseus-da46).
        let content = match parallel::beside_line() {
            Some(line) if !r.late => format!("{content}\n{line}"),
            _ => content,
        };
        Node::tool_result(
            session_id,
            turn_id,
            loop_index,
            Body::ToolResult {
                tool_use_id: r.tool_use_id.into(),
                tool: r.tool.into(),
                status: r.status,
                is_error: r.status.is_error(),
                content,
                correlation_id: r.correlation_id.map(str::to_string),
                bytes_total: r.bytes_total.unwrap_or(r.text.len() as u64),
                truncated,
                // The whole output a cut result keeps (theseus-v73m).
                full_ref,
                duration_ms: r.duration_ms,
                late: r.late,
                meta: r.meta,
                image: r.image,
                external: r.external,
            },
        )
    }

    /// A result on its own frame, announced.
    fn answer(&self, tc: &TurnCtx<'_>, r: ResultNode<'_>) -> Result<ResultStatus> {
        self.write_result(tc, &self.result_node(tc, r))
    }

    /// Write a result node on its own frame and announce it.
    fn write_result(&self, tc: &TurnCtx<'_>, node: &Node) -> Result<ResultStatus> {
        let status = match &node.body {
            Body::ToolResult { status, .. } => *status,
            _ => ResultStatus::Error,
        };
        tc.store.append(&[node.record()?])?;
        Self::announce_end(tc, node);
        self.queue_content(tc.session_id, std::slice::from_ref(node));
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

    fn proposal_for(tool: &dyn Tool, input: &Value, ctx: &ToolCtx) -> Proposal {
        Proposal {
            tool: tool.name().into(),
            args: input.clone(),
            resource: None,
            policy_context: json!({"roots": ctx.roots, "cwd": ctx.cwd}),
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

    /// A gated call: planned (with its `ToolCall` node), its notice sent,
    /// handed to the judge (which never delays it), then asked, or run.
    /// Returns its outcome and the judgments its gate dispatched.
    async fn start(
        &self,
        tc: &TurnCtx<'_>,
        assistant_node: &str,
        call: &ToolUse,
        tool: Arc<dyn Tool>,
        g: Gated,
    ) -> Result<(CallOutcome, Vec<crate::judge::gate::Mark>)> {
        let a = self.plan_call(tc, assistant_node, call, tool.as_ref(), &g)?;
        let notice = Self::notified(tc, call, tool.name(), &a.correlation_id, &g);
        if let Some(notice) = &notice {
            // Its row rode in the frame that planned the call.
            tc.rec().announce(&fact::tool::ToolNotified { notice });
        }
        tc.record(&fact::tool::GateDecided {
            runtime: self,
            tool: tool.name(),
            gated: &g,
        });
        let judged = self.judge_at_gate(tc, &a, call, tool.as_ref(), &g, notice.is_some());
        if g.decision.posture == Posture::Approve {
            return Ok((self.ask(tc, a, call, tool.name(), g)?, judged));
        }
        let (posture, class) = (g.decision.posture, g.class);
        let outcome = self
            .execute(tc, &a.correlation_id, tool, call, posture, class)
            .await?;
        Ok((outcome, judged))
    }

    /// `security.v3` at `gate` (M5 step 24): the choice to judge the call,
    /// and the mark. Everything else is the judge's own
    /// task: the gate's decision stands as it was made.
    fn judge_at_gate(
        &self,
        tc: &TurnCtx<'_>,
        a: &Action,
        call: &ToolUse,
        tool: &dyn Tool,
        g: &Gated,
        notified: bool,
    ) -> Vec<crate::judge::gate::Mark> {
        let Some(judge) = self.judge.get().filter(|j| j.gate_on()) else {
            return Vec::new();
        };
        let class = g.plan.class.unwrap_or(tool.class());
        let holds = || crate::external::read_any(tc.store, tc.session_id);
        if !crate::judge::gate::judged(class, tool.name(), holds) {
            return Vec::new();
        }
        let d = &g.decision;
        let gc = crate::judge::gate::GateCall {
            session_id: tc.session_id.into(),
            execution_id: tc.execution_id.into(),
            turn_id: tc.turn_id.into(),
            loop_index: tc.loop_index,
            task: tc.task.is_some(),
            correlation_id: a.correlation_id.clone(),
            tool_use_id: call.id.clone(),
            tool: tool.name().into(),
            class,
            posture: d.posture.as_str().into(),
            reason: d.reason.clone(),
            floor: d.floor,
            notified,
            input: call.input.clone(),
            plan: g.plan.clone(),
        };
        // The turn's clients hear a notified call's score, and v3's live
        // notice on an open one.
        let told = notified || judge.notices_live(tc.session_id);
        judge.at_gate(gc, told.then(|| tc.sink.clone()))
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
            by: None,
            judgment: None,
        })
    }

    /// The gate for one call (§3.17). The toollet's own typed parse names the
    /// resources, and an `Err` is invalid input. The policy then runs the
    /// call, notifies, or waits (§3.9); nothing it decides refuses one
    /// (theseus-8az). The record keeps the keys stored tool-call nodes carry,
    /// and `tool.proposed` shows it to the session's clients.
    fn gate(&self, tc: &TurnCtx<'_>, tool: &dyn Tool, call: &ToolUse) -> Result<Gated, Invalid> {
        let ctx = self.ctx_in(tc);
        let mut proposal = Self::proposal_for(tool, &call.input, &ctx);
        let planned = tool.plan(&call.input, &ctx);
        // Layer 1 guards only the owner's tasks (theseus-ext.10).
        let planned = planned.map(|p| {
            crate::task_graph::tools::authority_for(tc.store, tool.name(), &call.input, p)
        });
        // A job tool's job is its session's; a stop runs its program (theseus-n8gk).
        let planned = planned.and_then(|p| self.job_planned(tc, tool.name(), &call.input, p));
        // A glide's places (38b): the one it names must be bound here.
        let glide = (planned.is_ok() && crate::glide::is_glide(tool.name()))
            .then(|| crate::glide::resolve(tc, tool.name(), &call.input));
        // The order is `order.rs`, which `policy.explain` runs too (42a).
        let mcp = || mcp_floor(tc);
        // A routine's run: its grant, from the book in memory; any other
        // session pays one `Option` test (theseus-52tut).
        let routine_grant = tc
            .task
            .and_then(|t| self.routines.grant_for(tc.kernel, tc.store, t));
        let at = order::At {
            place: tc.place(),
            labels: &|| crate::content_wait::flags_of(tc.store, tc.session_id),
            mcp: &mcp,
            glide: glide.as_ref().and_then(|g| g.as_ref().ok()),
            session: Some(tc.session_id),
            ctx: &ctx,
            routine: routine_grant.as_ref(),
        };
        // A shared place's call reaches only what the place may (the place
        // rule): the catalog offers nothing else, and this refuses it, in case.
        // A batch's every step is judged (`batch.rs`, theseus-7gir.3).
        let mut refused = None;
        let planned = planned.and_then(|plan| {
            let glided = glide.as_ref().and_then(|g| g.as_ref().err().cloned());
            let judged = match self.judge(&at, tool, &plan, &call.input, &mut |_, _| {}) {
                // A batch whose steps differ in class (theseus-nrvq).
                Err(Turned::Invalid(e)) => return Err(e),
                Err(Turned::Place(why)) => Err(why),
                Ok((d, c)) => Ok((self.misroute_hold(tc, tool, &plan, d), c)),
            };
            judged
                .and_then(|j| glided.map_or(Ok(j), Err))
                .map(|(decision, job_class)| (plan, decision, job_class))
                .inspect_err(|why| refused = Some(why.clone()))
        });
        let result = match &planned {
            Err(e) => GateResult {
                gate: "deny".into(),
                reason: Some(match refused {
                    Some(_) => crate::policy::modes::refusal_reason(e),
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
                crate::policy::modes::refusal_reason(&bad.error),
                format!("Not run: {}", bad.error),
            ),
            false => (
                format!("validation: {}", bad.error),
                format!("Invalid input: {}", self.field_hinted(tool, &bad.error)),
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
            // A layer-1 task change's proposal is written on its task in this
            // frame, under the task's lock (39a).
            let task_lock = crate::task_graph::tools::lock_for_call(tc, tool.name(), &call.input);
            let mut proposed = None;
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
                    if let Some((more, c)) = crate::task_graph::tools::proposed(
                        tc,
                        tool.name(),
                        &call.input,
                        &a.correlation_id,
                    )? {
                        records.extend(more);
                        proposed = Some(c);
                    }
                    Ok(records)
                })?;
            drop(task_lock);
            if let Some(c) = proposed {
                crate::task_graph::tools::announce(tc, &[("change_proposed", c)]);
            }
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
        // Asked at its plan's time, and expiring then plus the TTL, as
        // `confirm.list`, the watch's snapshot, and the board say.
        let approve = g.decision.posture == crate::policy::Posture::Approve;
        let mut req = crate::rpc::offer(ConfirmRequest {
            grant_offer: crate::policy::grants::plan_offer(tool, tc.class, approve, g.class.l1()),
            correlation_id: a.correlation_id.clone(),
            session_id: tc.session_id.into(),
            execution_id: tc.execution_id.into(),
            turn_id: Some(tc.turn_id.into()),
            tool: tool.into(),
            input: call.input.clone(),
            resource: g.proposal.resource,
            reason: g.decision.reason,
            by: self.policy.confirmer.clone(),
            requested_at_ms: a.planned_at_ms,
            expires_at_ms: crate::ask::expiry_of(&a, tc.confirm_ttl_ms),
            floor: g.decision.floor,
            budget: None,
            task: tc.task.map(|_| theseus_protocol::TaskRef {
                task_id: tc.session_id.into(),
                short: crate::task::short(tc.session_id),
                title: None,
            }),
            change: crate::task_graph::tools::change_of(tc.store, tool, &call.input),
            preview: self.preview(tc.dir, tool, &call.input),
            ..Default::default()
        });
        // The question in plain words, for the screens that hear it pushed
        // (theseus-cudar); `confirm.list` words it again from the record.
        if let (Some(core), Ok(nodes)) =
            (self.self_pr.core(), tc.store.session_nodes(tc.session_id))
        {
            req.plain = core.plain_of(&req, &nodes);
        }
        tc.record(&fact::tool::CallAsked { request: &req });
        // It expires at its plan's time and the TTL, as `confirm.list` says
        // (theseus-830): the driver reads the questions by then.
        self.question_due.fetch_min(
            crate::ask::expiry_of(&a, tc.confirm_ttl_ms),
            std::sync::atomic::Ordering::SeqCst,
        );
        Ok(CallOutcome::AwaitingConfirm {
            correlation_id: a.correlation_id,
        })
    }

    pub(crate) fn deadline_ms(&self, tool: &dyn Tool, input: &Value) -> u64 {
        match tool.backend() {
            Backend::Inproc | Backend::Async | Backend::Harness => inproc_deadline_ms(tool),
            Backend::Job => {
                let t = |input: &Value| {
                    input
                        .get("timeout_secs")
                        .and_then(Value::as_u64)
                        .unwrap_or(self.ctx.proc_timeout_secs)
                        .min(self.ctx.proc_timeout_max_secs)
                };
                // A batch's deadline covers every step's timeout (theseus-7gir.3).
                let t = match tool.steps(input) {
                    Some(steps) => steps.iter().map(t).sum(),
                    None => t(input),
                };
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
            // A proposed extension's trial waits for its server (M7 43a).
            Backend::Harness if tool.name() == crate::rsi::pr::PROPOSE => {
                self.run_self_pr(tc, correlation_id, tool.as_ref(), call)
                    .await
            }
            Backend::Harness if crate::flow::is_flow(tool.name()) => {
                // A step is a call too, so the future is boxed; the allow-list
                // keeps a flow from naming another.
                Box::pin(self.run_flow(tc, correlation_id, tool.as_ref(), call)).await
            }
            Backend::Harness if tool.name() == crate::extend::PROPOSE => {
                self.run_extend(tc, correlation_id, tool.as_ref(), call)
                    .await
            }
            Backend::Harness => self.run_harness(tc, correlation_id, tool.as_ref(), call, ran_at),
        }
    }

    /// `extend.propose` (M7 43a): a harness call whose trial waits on its
    /// server, so it is awaited here, as an async tool is, and never on a
    /// core; its result is written as any harness call's.
    async fn run_extend(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
    ) -> Result<CallOutcome> {
        Self::harness_started(tc, correlation_id, tool, call);
        let started = theseus_protocol::now_unix_ms();
        let t0 = Instant::now();
        let done = crate::extend::propose(self, tc, &call.input, correlation_id).await;
        // A trial a cancel or a stop aborted (18a): the cancel settles the
        // call, with its verdict, and its result says so, riding as a late
        // one does.
        if matches!(&done, Err(e) if e == crate::extend::STOPPED) {
            if let Some(a) = crate::cancel::after_abort(tc.kernel, correlation_id).await {
                let (status, text, meta) = crate::cancel::aborted_result(&a);
                let node = self.result_node(
                    tc,
                    ResultNode {
                        correlation_id: Some(correlation_id),
                        duration_ms: Some(t0.elapsed().as_millis() as u64),
                        meta,
                        ..ResultNode::new(&call.id, tool.name(), status, text)
                    },
                );
                tc.store.append(&[node.record()?])?;
                Self::announce_end(tc, &node);
                return Ok(CallOutcome::Done { status });
            }
        }
        let none = crate::task_graph::tools::Done::default();
        self.harness_done(tc, correlation_id, tool, call, (started, t0), (done, none))
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
        ran_at: Posture,
    ) -> Result<CallOutcome> {
        // A glide's post rides in its completion's frame, and a read from a
        // shared place is marked outside text there (38b).
        if crate::glide::is_glide(tool.name()) {
            return self.run_glide(tc, correlation_id, tool, call, ran_at);
        }
        Self::harness_started(tc, correlation_id, tool, call);
        let started = theseus_protocol::now_unix_ms();
        let t0 = Instant::now();
        // A task edit's records ride in the frame that settles it, under its
        // task's lock (39a).
        let mut edit = crate::task_graph::tools::Done::default();
        let done = match tool.name() {
            crate::task::CREATE => crate::task::create_call(
                tc,
                &call.input,
                correlation_id,
                (&self.profiles, (&self.routines, &self.projects)),
                &self.forecasts,
            )
            .map(|c| edit.take(c)),
            crate::project_tool::ATTACH => crate::project_tool::attach(tc, &call.input),
            crate::wake::AT => crate::wake::set(tc, &call.input, correlation_id),
            crate::wake_tools::CANCEL => crate::wake_tools::cancel(tc, &call.input),
            crate::wake_tools::MOVE => crate::wake_tools::move_to(tc, &call.input),
            crate::routine_tool::CREATE => {
                crate::routine_tool::run(tc, &self.routines, &call.input, &|t| {
                    self.registry.get(t).is_some()
                })
            }
            crate::watch_tool::WATCH => crate::watch_tool::set(
                tc,
                &self.watches,
                &self.watches_cfg,
                self.google.as_ref().map(|g| &g.cfg),
                &call.input,
                correlation_id,
            ),
            crate::thread_context::NOTE => {
                crate::thread_context::note(tc, &call.input).map(|(done, records)| {
                    edit.records.extend(records);
                    done
                })
            }
            crate::ask::OWNER_ASK => crate::ask::result(tc, &call.input, correlation_id),
            name if crate::policy::modes::is_plan(name) => self
                .run_plan(tc, name, &call.input, correlation_id)
                .map(|d| edit.take(d)),
            // Run again after a restart: what the first run proposed.
            crate::extend::PROPOSE => crate::extend::proposed_by_call(tc, correlation_id),
            crate::rsi::pr::PROPOSE => Err(crate::rsi::pr::INTERRUPTED.into()),
            // Read-only, and for a self session alone (theseus-cudar).
            crate::rsi::evidence::EVIDENCE => {
                match crate::rsi::own_tree::is_self(self, tc.session_id) {
                    true => crate::rsi::evidence::run(
                        self.self_pr.state_dir(),
                        tc.store,
                        theseus_protocol::now_unix_ms(),
                        &call.input,
                    ),
                    false => Err("only a self session may read its evidence".into()),
                }
            }
            name if crate::task_graph::tools::is_edit(name) => {
                let how = (ran_at == Posture::Approve, self.leases.lease_ms());
                crate::task_graph::tools::run(tc, name, &call.input, correlation_id, how)
                    .map(|d| edit.take(d))
            }
            other => Err(format!("{other} is not a tool the harness runs")),
        };
        self.harness_done(tc, correlation_id, tool, call, (started, t0), (done, edit))
    }

    fn harness_started(tc: &TurnCtx<'_>, correlation_id: &str, tool: &dyn Tool, call: &ToolUse) {
        tc.record(&fact::tool::ToolStarted {
            session_id: tc.session_id,
            turn_id: tc.turn_id,
            tool_use_id: &call.id,
            tool: tool.name(),
            correlation_id,
            backend: tool.backend().as_str(),
        });
    }

    /// A harness call's result node, in its completion's frame; a task
    /// edit's records ride in the same frame, and its task locks are held
    /// until it is written (39a).
    fn harness_done(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        (started, t0): (u64, Instant),
        (done, mut edit): (
            Result<(String, Value), String>,
            crate::task_graph::tools::Done<'_>,
        ),
    ) -> Result<CallOutcome> {
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
        let mut records = vec![node.record()?];
        records.append(&mut edit.records);
        tc.kernel.accept_completion_with(&c, records)?;
        drop(edit.locks);
        for (_, c) in &edit.changes {
            self.leases.note(&c.task);
        }
        crate::task_graph::tools::announce(tc, &edit.changes);
        crate::project::remind::after(self, tc, &edit.changes);
        self.plan_moved(tc, &edit.changes);
        Self::announce_end(tc, &node);
        // A harness tool's result is an item like any other tool's
        // (theseus-sh1n3): this site wrote one and queued nothing, so a
        // `task.create` result — or a wake's, a plan's, an `rsi.evidence`'s —
        // was counted unjudged for ever and took its session off every model
        // of restricted origin.
        self.queue_content(tc.session_id, std::slice::from_ref(&node));
        Ok(CallOutcome::Done { status })
    }

    /// An in-process tool: its result node rides in its completion's frame.
    /// A toollet computes on a core; an async tool (DD5) waits as a task on
    /// the runtime and holds none.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
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
        let (t, input) = (tool.clone(), call.input.clone());
        let mut ctx = self.ctx_in(tc).into_owned();
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
        let mut pre_images = Vec::new();
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
            let run = match tool.family() {
                crate::term::FAMILY => theseus_tools::with_no_media(self.terms.run(
                    tool.name(),
                    tc.session_id,
                    &input,
                    &ctx,
                )),
                // The session's files, found on this task (theseus-c9l6).
                crate::file_read::FAMILY => self.read_file(tc, &input, &ctx),
                // The owner's memory, read for this session (theseus-w9qv).
                crate::memory_lookup::FAMILY => self.lookup.run(tc, &input),
                // Another thread, read on purpose (theseus-f2g0).
                crate::thread_read::FAMILY => self.thread_read.run(tc, &input),
                crate::tools_defer::FAMILY => self.defer.run(tc.session_id, &input),
                // Part of a thread, a thread of its own (theseus-vt9k).
                crate::thread_split::FAMILY => crate::thread_split::run(tc, correlation_id, &input),
                // A job's handle needs its session (theseus-n8gk).
                f if Self::core(f) => self.run_job_tool(tc, tool.name(), &input, correlation_id),
                _ => t.run_async_with_media(&input, &ctx),
            };
            let mut task = tokio::spawn(run);
            let task_id = task.id();
            // A cancel or a stop aborts it (M4 18a).
            let _reachable = self.stops.track(correlation_id, task.abort_handle());
            let outcome = match tokio::time::timeout(deadline, &mut task).await {
                Ok(Ok(Ok((out, external, media)))) => Ok((out, media, external)),
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
            // An edit tool's files are kept first (theseus-23rk).
            let rewind = crate::rewind::Rewind::of(tc, &call.id);
            let run = self
                .cpu
                .spawn(move || {
                    let held = rewind.hold(t.as_ref(), &input, &ctx);
                    let t0 = Instant::now();
                    let r = t.run_with_media(&input, &ctx);
                    (r, t0.elapsed(), rewind.settle(held, t.name()))
                })
                .await;
            let started = theseus_protocol::now_unix_ms();
            let t0 = Instant::now();
            let (outcome, took) = match tokio::time::timeout(deadline, run).await {
                Ok(Ok((Ok((out, img)), took, kept))) => {
                    pre_images = kept;
                    (Ok((out, img, None)), took)
                }
                Ok(Ok((Err(f), took, kept))) => {
                    pre_images = kept;
                    (Err(f.message), took)
                }
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
        // A transcript file.read made is spend (theseus-c9l6).
        if tool.family() == crate::file_read::FAMILY && status == ResultStatus::Ok {
            Self::book_transcript(tc, &meta);
        }
        // An image or a PDF's pages the tool read go to the blobs once; the
        // node holds the reference (theseus-9g2, theseus-c9l6).
        let image = img.and_then(|m| Self::keep_media(tc, tool.name(), m, &mut text));
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
        fact::google::of_result(&tc.rec(), &meta);
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
            None => self.complete(tc, &c, &node, crate::rewind::records(&pre_images)?)?,
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

    /// `file.read` (theseus-c9l6): the session's files, found now, and the
    /// reading in the call's own task. What its session may still spend
    /// bounds a transcript; a shared place's reading is outside text.
    fn read_file(
        &self,
        tc: &TurnCtx<'_>,
        input: &Value,
        ctx: &ToolCtx,
    ) -> theseus_tools::AsyncMediaRun {
        let nodes = match tc.store.session_nodes(tc.session_id) {
            Ok(n) => n,
            Err(e) => {
                let why = format!("this session's files could not be read: {e:#}");
                return Box::pin(std::future::ready(Err(theseus_tools::ToolFailure::new(
                    why,
                ))));
            }
        };
        let available = tc
            .kernel
            .execution(tc.execution_id)
            .ok()
            .flatten()
            .map_or(u64::MAX, |e| match tc.kernel.overdraws(&e) {
                // A limit that notifies stops no transcript (theseus-usei).
                true => u64::MAX,
                false => e.budget.available(),
            });
        // A draft's reads reach a model outside the trust: held too.
        let shared = tc.class != crate::places::PlaceClass::Private || tc.draft.is_some();
        self.files.run(
            &nodes,
            tc.store.blobs_handle(),
            tc.session_id,
            ctx,
            input,
            (shared, available),
        )
    }

    /// A transcript `file.read` made (theseus-c9l6): booked to the turn's
    /// execution as a voice call's is, with its `speech.transcribed` row.
    fn book_transcript(tc: &TurnCtx<'_>, meta: &Value) {
        let Some(sp) = meta.get("speech") else {
            return;
        };
        let (provider, model) = (
            sp["provider"].as_str().unwrap_or("deepgram"),
            sp["model"].as_str().unwrap_or(""),
        );
        let seconds = sp["seconds"].as_f64().unwrap_or(0.0).max(0.0);
        let chars = sp["chars"].as_u64().unwrap_or(0) as usize;
        let Some(price) = crate::catalog::speech_price(provider, model) else {
            return;
        };
        let cost = price.cost_micros(Duration::from_secs_f64(seconds), chars);
        let data = json!({"provider": provider, "model": model, "seconds": seconds, "chars": chars,
                          "tool": crate::file_read::READ, "file": meta["file"], "digest": meta["digest"]});
        if let Err(e) =
            tc.kernel
                .book_spend(tc.execution_id, cost, LedgerKind::SpeechTranscribed, data)
        {
            tracing::warn!(error = %e, "a transcript's spend could not be booked");
        }
    }

    /// A file a tool read, kept in the blobs for its result node: an image,
    /// or a PDF's pages with their text, whose `file.read` row is written
    /// here (theseus-c9l6). What could not be kept says so in the result.
    fn keep_media(
        tc: &TurnCtx<'_>,
        tool: &str,
        media: theseus_tools::Media,
        text: &mut String,
    ) -> Option<crate::node::Attachment> {
        let blobs = tc.store.blobs();
        let kept = match media {
            theseus_tools::Media::Image(d) => {
                crate::attach::store_image(&d.bytes, blobs).map(|(info, digest)| {
                    crate::node::Attachment {
                        name: d.name,
                        media_type: info.media_type.into(),
                        size: d.bytes.len() as u64,
                        content: crate::node::AttachmentContent::Image {
                            digest,
                            width: info.width,
                            height: info.height,
                        },
                    }
                })
            }
            theseus_tools::Media::Pdf(p) => {
                let via = if tool == "http.fetch" {
                    "http.fetch"
                } else {
                    "fs.read"
                };
                theseus_store::blocking(|| {
                    crate::attach::keep_pages(p.name, &p.bytes, &p.read, via, blobs)
                })
                .map(|(a, mut read)| {
                    (read.ms, read.capped) = (p.ms, p.capped);
                    tc.record(&fact::tool::FileRead { read: &read });
                    a
                })
            }
        };
        match kept {
            Ok(a) => Some(a),
            Err(why) => {
                text.push_str(&format!(" It is not shown: {why}."));
                None
            }
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
    Option<theseus_tools::Media>,
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
    /// The line its text ends with, past the cap: the native call a job of
    /// the daemon's own CLI stands in for (`native::hint`, theseus-8c8s).
    hint: Option<String>,
    /// Where the output in its text is, kept whole when cut (theseus-v73m).
    kept: Option<kept::Source>,
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
            hint: None,
            kept: None,
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
fn unanswered(nodes: &[(u64, crate::stub::Stub)]) -> Option<(&Node, Vec<ToolUse>)> {
    use crate::stub::Kind;
    let at = nodes
        .iter()
        .rposition(|(_, n)| n.kind == Kind::AssistantMessage)?;
    let last = &nodes[at].1;
    // A call's result follows the reply that holds it: only what came after
    // is read, so a long session's earlier results stay stubs (step 33).
    let answered: HashSet<&str> = nodes[at + 1..]
        .iter()
        .filter(|(_, n)| n.kind == Kind::ToolResult)
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool_use_id,
                late: false,
                ..
            } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
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
    let state = spool
        .as_ref()
        .and_then(|s| s.dir().parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| cfg.state_dir());
    let (roots, floor_paths, approve) = draft_roots::guarded_with_drafts(cfg, &state);
    // Proposed extensions are frozen beside the store (M7 43a).
    let extend = Arc::new(crate::extend::Extensions::new(
        state.join("extensions"),
        roots.clone(),
    ));
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
    let google = crate::google::build(cfg, &secrets, t.enabled);
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
    // web.search needs its key's [secrets] entry (theseus-4o4c).
    let not_offered: Vec<String> = t
        .enabled
        .then(|| crate::web::search_missing(cfg))
        .flatten()
        .into_iter()
        .collect();
    if not_offered
        .iter()
        .any(|_| cfg.policy.tools.contains_key("web.search"))
    {
        tracing::warn!(
            "[policy.tools] names web.search, which is not offered ({})",
            not_offered[0]
        );
    }
    let mut registry = if t.enabled {
        let mut r = theseus_tools::default_registry();
        // The web tools wait on the network, as async tools (DD5).
        let private = cfg.policy.private_addresses;
        let web = crate::web::Web::new(&t.web, t.result_max_chars, cpu.clone(), private);
        for tool in web.tools_with(not_offered.is_empty()) {
            r.register(tool);
        }
        let (aws_tools, google_tools) = (
            aws.iter().flat_map(|a| a.tools()),
            google.iter().flat_map(|g| g.tools()),
        );
        aws_tools
            .chain(google_tools)
            .for_each(|tool| r.register(tool));
        for tool in lsp.iter().flat_map(|b| b.tools()) {
            r.register(tool);
        }
        // Task sessions (DD7), wakes (DD8), proposed extensions (M7 43a),
        // and glides (38b): the harness runs them.
        crate::task_wait::register(&mut r);
        r.register(Arc::new(crate::task_graph::tools::TaskUpdate));
        r.register(Arc::new(crate::task_graph::tools::TaskSplit));
        r.register(Arc::new(crate::task_graph::tools::TaskClose));
        r.register(Arc::new(crate::task_graph::lease::TaskClaimTool));
        r.register(Arc::new(crate::wake::WakeAt));
        r.register(Arc::new(crate::wake_tools::WakeCancel));
        r.register(Arc::new(crate::wake_tools::WakeMove));
        r.register(Arc::new(crate::routine_tool::RoutineCreate {
            zone: cfg.kernel.to_kernel_config().zone,
            cfg: cfg.routines.clone(),
            floor_ms: cfg.kernel.min_repeat_minutes.max(1) * 60_000,
            max_ahead_ms: cfg.kernel.to_kernel_config().wakes.max_ahead_ms,
        }));
        r.register(Arc::new(crate::watch_tool::WatchTool));
        r.register(Arc::new(crate::thread_context::ThreadNote));
        r.register(Arc::new(crate::thread_split::ThreadSplitTool));
        r.register(Arc::new(crate::ask::OwnerAsk));
        r.register(Arc::new(crate::project_tool::ProjectAttach));
        crate::policy::modes::register(&mut r);
        r.register(Arc::new(crate::extend::Propose));
        r.register(Arc::new(crate::glide::ChannelPost));
        r.register(Arc::new(crate::glide::ChannelRead));
        // The files people gave the session (theseus-c9l6).
        r.register(Arc::new(crate::file_read::FileRead));
        // A read of memory the model calls (theseus-w9qv).
        r.register(Arc::new(crate::memory_lookup::MemoryLookup));
        // Another thread's card and notes, read on purpose (theseus-f2g0).
        r.register(Arc::new(crate::thread_read::ThreadRead));
        crate::rsi::pr::register(cfg, &mut r);
        crate::rsi::evidence::register(cfg, &mut r);
        r.register(Arc::new(crate::tools_defer::find::FindTool));
        r
    } else {
        Registry::new()
    };
    // Terminals (theseus-n88g.4): each program gets the job environment.
    let terms = Arc::new(
        crate::term::Terms::new(proc_env.clone())
            .configured(t.term.keep_background, t.proc_sync_secs),
    );
    if t.enabled {
        for tool in crate::term::tools::all(&terms) {
            registry.register(tool);
        }
    }
    let notify_socket = spool.as_ref().map(|s| s.dir().join("notify.sock"));
    // file.read's transcripts go to Deepgram as voice's speech does
    // (theseus-c9l6).
    let files = Arc::new(crate::file_read::Reader {
        secrets: secrets.clone(),
        hearing: crate::file_read::Hearing {
            key_secret: cfg.voice.key_secret.clone(),
            model: cfg.voice.prerecorded_model().to_string(),
            language: cfg.voice.language.clone(),
            api_base: cfg.voice.api_base.clone(),
            max_minutes: t.transcribe_max_minutes,
        },
        max_file: t.max_attachment_bytes,
        http: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(100))
            .build()
            .unwrap_or_default(),
    });
    // A program's name is resolved on the daemon's own PATH (theseus-dcy).
    let broker = Broker::new(&cfg.broker, secrets, std::env::var("PATH").ok());
    // web.search's key: its calls run at no looser a posture than the
    // key's, and health lists the grant with its uses (DD5).
    if t.enabled && not_offered.is_empty() {
        broker.grant_tool("web.search", &t.web.search_key_secret);
    }
    // A program's AWS job session comes from these accounts (C2).
    if let Some(a) = &aws {
        broker.set_aws(a.clone());
    }
    let self_pr = Arc::new(crate::rsi::pr::Board::new(state.clone()));
    Ok(ToolRuntime {
        misroute: Default::default(),
        catalogs: Default::default(),
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
            private_addresses: cfg.policy.private_addresses,
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
            umask: theseus_kernel::umask::operator(),
            ..Default::default()
        },
        spool,
        scrubber,
        launcher,
        notify_socket,
        result_max_chars: t.result_max_chars,
        proc_sync_secs: t.proc_sync_secs,
        job_wait_max_secs: t.job_wait_max_secs,
        parallel_runs: t.parallel_runs.max(1),
        parallel_reads: t.parallel_reads.max(1),
        parallel_writes: t.parallel_writes.max(1),
        proc_env,
        calls: Mutex::new(BTreeMap::new()),
        fit_notes: Default::default(),
        // Read from the store when the core starts (`Core::build`).
        tightened: Default::default(),
        modes: Default::default(),
        cpu,
        broker: Arc::new(broker),
        output_max_bytes: t.job_output_max_bytes,
        outputs: crate::outputs::Outputs::new(
            state.join("outputs"),
            t.outputs_keep_days,
            t.outputs_max_bytes,
        ),
        flows: crate::flow::Board::new(state.join("self").join("mint")),
        disk: Arc::new(crate::disk::Disk::new(
            state,
            cfg.server.disk_warn_mb,
            cfg.server.disk_floor_mb,
        )),
        aws,
        google,
        not_offered,
        files,
        question_due: Default::default(),
        leases: crate::task_graph::lease::Leases::new(cfg.kernel.task_lease_ms()),
        watches: Default::default(),
        watches_cfg: cfg.watch_for.clone(),
        forecasts: Default::default(),
        sandbox,
        stops: Default::default(),
        public_roots: crate::places::public_roots(cfg),
        drafts_root: crate::config::routing_cn::drafts_root(cfg),
        sealed: crate::rsi::sealed::guard_of(cfg).unwrap_or_default(),
        job_waits: Arc::new(waits::JobWaits::steered(terms.steers.clone())),
        admission: crate::turn::admission::Admission::new(
            cfg.kernel.admission_ceiling,
            cfg.kernel.admission_stuck(),
        ),
        mcp: Default::default(),
        terms,
        lsp,
        extend,
        judge: Default::default(),
        lookup: Default::default(),
        routines: crate::routine::Book::new(&cfg.routines),
        thread_read: Default::default(),
        defer: Default::default(),
        projects: cfg.projects.clone(),
        jobs: Default::default(),
        self_pr,
        profiles: cfg
            .all_profiles()
            .iter()
            .map(|(name, p)| {
                let target = crate::session::TargetRef {
                    profile: name.clone(),
                    provider: p.provider.clone(),
                    model: p.model.clone(),
                };
                (name.clone(), target)
            })
            .collect(),
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
