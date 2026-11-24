//! The turn runner (spec §3.3, §3.3a, §4.4a, §4.6).
//!
//! A turn is the loops run under one acquisition of the session's turn lock,
//! ended by the Advancer. A loop: the compiler renders the session (a
//! compilation plus its append tail) into a request, the provider call runs as
//! a kernel action, the model's response becomes an assistant node in the same
//! frame as the call's settlement, and every `tool_use` goes through the tool
//! runtime (gate, confirm, action, result node). The Advancer continues while
//! the model is calling tools and every call has an answer.
//!
//! A turn with no input is a **continuation** (the harness's driver runs it):
//! results that settled since the last turn become late result nodes, pending
//! tool calls are resumed (a confirm answered, a restart survived), and the
//! model runs only if it has something new to read.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{
    micros_to_usd, Action, ActionState, Authority, Completion, ExecState, Execution, Kernel,
    KernelError, Micros, Outcome as ActionOutcome, Proposal, RetryClass, TurnEnd, TurnGuard, Wake,
    BUDGET_TOOL, PROVIDER_TOOL,
};
use theseus_protocol::{
    BudgetAsk, CacheSummary, ConfirmRequest, ContextCompiled, SessionKind, TurnSubmitResult, Usage,
};
use theseus_store::{frames_written_here, NewRecord};

use crate::advancer::{Advancer, Decision, LoopOutcome, UntilNoToolCalls};
use crate::bus::{EventSink, SessionBus};
use crate::catalog::Catalog;
use crate::compiler::{situation, CompileInput, Compiled, Overflowed, RequestSpec};
use crate::config::{CacheTtl, Effort, ThinkingDisplay};
use crate::content::{footer_labels, ItemKind};
use crate::context_files::{ContextFile, ContextFiles, Unreadable};
use crate::fact::{self, Fact, To};
use crate::narrative::{self, Narrator};
use crate::node::{Body, Node, ResultStatus};
use crate::provider::{
    Delta, ModelResponse, Overflow, Provider, ProviderError, ToolUse, WINDOW_EXCEEDED,
};
use crate::secrets::{SecretBoard, Waited};
use crate::session::{title_from, NotShown, SessionRecord, TargetRef, Then};
use crate::startup::StartupLog;
use crate::store::{SessionHold, Store};
use crate::task_graph::tools::Closing;
use crate::toolrun::{Call, CallOutcome, ToolRuntime, TurnCtx};
use crate::trace::Trace;
use crate::Config;

pub mod admission;
mod admit_step;
mod admit_wait;
pub use admit_wait::AdmitWaiting;
pub(crate) mod advance_step;
#[cfg(test)]
pub(crate) use admit_step::admit_hook;
pub(crate) mod brief_step;
pub(crate) mod budget_step;
mod budget_words;
mod calls;
mod catch_up;
pub(crate) mod ceiling_step;
mod checkpoint_step;
mod cite_step;
pub mod clock_step;
pub(crate) mod close_out_step;
mod content_step;
pub mod delay_step;
pub(crate) use budget_words::{budget_question, lower_cap, Kept};
#[cfg(test)]
pub(crate) use calls::call_result;
mod compile_step;
mod cut_step;
pub(crate) mod data_step;
mod defer_step;
mod deferred_step;
pub mod effort_step;
pub mod end_note_step;
mod end_step;
// For `env_tail::HEAD`, the block's opener, which `task_graph::view::is_head` reads.
pub(crate) mod env_tail;
mod exchange_step;
mod fallback_step;
mod families_step;
mod fill_step;
pub(crate) mod helper_step;
mod inbound_step;
mod join_step;
mod later_step;
mod model_call;
mod noop_step;
pub(crate) mod now_step;
pub mod nudge_step;
pub mod overage;
mod persona_engage;
pub(crate) mod persona_step;
pub mod progress_step;
mod project_step;
mod promise_step;
mod prompt_input;
mod recall_step;
pub mod repeat_step;
mod reply_step;
mod request;
mod rerank_step;
mod resume_step;
mod retract_step;
mod retry_step;
mod route_step;
mod self_budget_step;
pub use self_budget_step::SELF_BUDGET_CLASS;
mod settle;
pub(crate) mod situation_step;
mod spec_step;
mod standing_step;
pub mod stdio_step;
mod steer_step;
mod stopping_step;
mod subagent_step;
mod surface_step;
mod target;
pub(crate) mod tier;
mod title_step;
pub(crate) mod view_step;
pub use view_step::preview::Preview;

pub use request::TurnRequest;
#[cfg(test)]
pub(crate) use route_step::first_compile_hook;
pub use route_step::{LiveSwitched, SWITCHED};
pub use target::Target;

/// The built-in persona at the front of the system prompt of a session whose
/// persona in play has no readable file of its own (theseus-slvl.2): the
/// default file, `persona/default.md`. Frozen text: it sits at the start of
/// the cached prefix, so it never interpolates anything. An owner's persona
/// file replaces it whole (`persona::owned`).
pub const PERSONA: &str = crate::config::persona::DEFAULT_TEXT;

/// One `/stop`'s notice to the turns of its execution (theseus-hmwv): when it
/// landed, who sent it, and whether a turn has stopped itself on it.
#[derive(Clone, Debug)]
pub struct StopNotice {
    pub at: Instant,
    pub by: String,
    pub taken: bool,
}

/// The assembly note, after the persona: how the harness builds each
/// request, so the model never takes what the harness added (a recalled
/// note, a wake, a report) for something the person sent (theseus-fpm2).
/// Frozen text in the shared header, as the persona is.
pub const ASSEMBLY: &str = "Context. The harness assembles each request, as the memory page at the end of the system part says. A recalled note is background the harness chose, not something the person sent. The person sees their own messages and your replies, not the rest of the request, so use a recalled note when it helps, and otherwise don't mention it. When you state something from recalled testimony, cite its handle. A person's message may start with a bracketed tag that names where they wrote from.";
pub struct TurnRunner {
    /// Last ledger position before this process can book model spend.
    pub(crate) day_seed_through: u64,
    pub(crate) day_read_failure: std::sync::Mutex<crate::day_ceiling::ReadFailure>,
    pub cfg: Arc<Config>,
    /// Providers by name; `cfg.model.provider` is the default.
    pub providers: BTreeMap<String, Arc<dyn Provider>>,
    pub store: Store,
    /// The durable kernel: admission, the per-execution turn lock, budgets,
    /// and the action/completion record of every provider and tool call.
    pub kernel: Arc<Kernel>,
    /// Woken whenever a turn ends or an execution changes.
    pub admission: Arc<tokio::sync::Notify>,
    pub catalog: Arc<Catalog>,
    pub tools: Arc<ToolRuntime>,
    pub bus: Arc<SessionBus>,
    pub narrator: Arc<Narrator>,
    /// What each context file held when last read (theseus-58a).
    pub context_files: ContextFiles,
    /// Each secret's state: a turn waits for its provider's key (theseus-qa0).
    pub secrets: Arc<SecretBoard>,
    /// Where a turn's first wait for a provider's key is recorded.
    pub startup_log: Arc<StartupLog>,
    /// What must reach a channel: a turn's reply, its cards, a failure
    /// (theseus-q4v).
    pub outbox: Arc<crate::outbox::Outbox>,
    /// What a `/stop` tells the model call a turn waits on (theseus-yey).
    pub stops: StopSignals,
    /// The latest `/stop` of each execution since this process started, and
    /// who sent it (theseus-hmwv): an input that arrived before it, and
    /// whose turn is admitted after it, is stopped as its turn starts.
    pub latest_stops: std::sync::Mutex<std::collections::HashMap<String, StopNotice>>,
    /// The input turns that have not yet taken their execution's hold
    /// (waiting for their secrets, or in `admit()`): a stop reaches them
    /// (`AdmitWaiting`).
    pub admit_waiting: AdmitWaiting,
    /// Each place's class (the place rule, theseus-nbsh): the places the
    /// binding binds, as it told the core at its start.
    pub place_rule: crate::places::PlaceRule,
    /// Recall (M6 step 30a): `[memory]`, and the index it asks.
    pub memory: Arc<crate::recall::Memory>,
    /// The memory pass (M6 31a): after each turn, off its path.
    pub pass: Arc<crate::memory_pass::MemoryPass>,
    /// The ontology's snapshot (theseus-8kk.1), built after serving.
    pub ontology: crate::ontology::Board,
    /// The owner's standing records (theseus-a7vi), in memory.
    pub standing: crate::standing::Board,
    /// Personas' last-seen state and sessions' own choices (theseus-slvl.2).
    pub persona: crate::persona::Board,
    /// The threads (theseus-nhew), in memory.
    pub threads: crate::threads::Threads,
    /// Jev's judgments (M5 23a): each turn's end is handed to it, and it
    /// judges in a task of its own; the turn never waits on it.
    pub judge: Arc<crate::judge::JudgeService>,
    /// The owner's last `profile.use`, which moves a session routing moved
    /// before it (theseus-9yyr).
    pub live_switched: LiveSwitched,
    /// The sessions this run has compiled: the first with nothing new resumes (35a).
    pub run_compiles: situation::RunCompiles,
    /// Who retries a failed turn: the driver, or a `--stdio` daemon's client.
    pub failed_turns: stdio_step::FailedTurns,
    /// Each execution's reconsiderations of a delay (theseus-5x8ry): the rate
    /// bound, the cap, and the adviser's unavailable run, per kind.
    pub reconsiders: delay_step::Reconsiders,
    /// Each execution's spend at its last check (theseus-jse4).
    pub(crate) spend_marks: budget_step::SpendMarks,
    /// The models found to refuse per-message effort, for this run (theseus-posww).
    pub effort_refusals: effort_step::EffortRefusals,
    /// A task wait's room, and the sticky effort's last request (`last_request`).
    pub last_request: crate::last_request::LastRequest,
    /// A minute's copy of the day's thread cache numbers (theseus-ytt8).
    pub(crate) cache_numbers: crate::keep_warm_threads::CacheNumbers,
    /// The self budget's account (theseus-pw1q.3).
    pub self_budget: crate::rsi::budget::Budget,
    /// What the harness made of the owner's yes, staged at `turn.submit` for
    /// the turn it starts (theseus-2yowd).
    pub yes_facts: crate::yes_fact::Staged,
    /// The turns' shared part of their exchanges (issue 44).
    pub exchanges: crate::exchange::Board,
}

/// What a `/stop` tells the turn that holds its execution while the model's
/// call is in flight (theseus-yey): one `Notify` per execution that has a call
/// out, armed by `call_model` and dropped when the call ends. It only wakes the
/// call: the turn reads the kernel's record of the stop to decide, and a stop
/// with no call in flight finds none, so the turn reads that record at its
/// next step, as it always has.
#[derive(Default)]
pub struct StopSignals {
    calls: std::sync::Mutex<BTreeMap<String, Arc<tokio::sync::Notify>>>,
}

impl StopSignals {
    /// Arm the signal for `execution_id`'s call, until the guard drops.
    fn arm(&self, execution_id: &str) -> Armed<'_> {
        let notify = Arc::new(tokio::sync::Notify::new());
        self.calls
            .lock()
            .unwrap()
            .insert(execution_id.to_string(), notify.clone());
        Armed {
            signals: self,
            execution_id: execution_id.to_string(),
            notify,
        }
    }

    /// Wake the call in flight for `execution_id`, if there is one.
    pub fn signal(&self, execution_id: &str) -> bool {
        match self.calls.lock().unwrap().get(execution_id) {
            Some(n) => {
                n.notify_waiters();
                true
            }
            None => false,
        }
    }
}

/// A call's armed signal; the entry goes when the call ends, however it ends.
struct Armed<'a> {
    signals: &'a StopSignals,
    execution_id: String,
    notify: Arc<tokio::sync::Notify>,
}

impl Drop for Armed<'_> {
    fn drop(&mut self) {
        self.signals
            .calls
            .lock()
            .unwrap()
            .remove(&self.execution_id);
    }
}

/// How many characters of streamed text the estimate of a cut call's output
/// counts as one token (theseus-yey): English prose runs about four, code and
/// other languages fewer, and the budget is a gate, so the estimate leans to
/// more tokens.
const CHARS_PER_TOKEN: u64 = 3;

/// The longest a turn waits for its secrets (theseus-qa0). A round takes
/// about a second, and each `op` gives up after 10 s, so the bound is
/// reached only when the vault hangs past its own timeouts.
pub const SECRET_WAIT: Duration = Duration::from_secs(30);

/// What a turn waited for before it ran, in microseconds from its arrival.
struct Waits {
    /// Its secrets (theseus-qa0), before admission.
    secrets_us: u64,
    /// Arrival to admission, the secrets included.
    lock_us: u64,
    /// Admission and the turn lock alone.
    admit_us: u64,
    /// Its class and ticket in the admission queue (theseus-4g0l).
    queued: admission::Queued,
}

/// A turn's own handles on the store and the kernel (theseus-qa0). A ledger
/// row the turn writes is no state transition, so it waits in `store` for
/// the next frame either of them commits for the turn.
struct Frames {
    store: Store,
    kernel: Kernel,
    /// Outbox posts whose records wait in `store` too (the reply, a budget
    /// question's card): indexed once the turn's last frame is written
    /// (theseus-q4v).
    posts: std::sync::Mutex<Vec<theseus_kernel::Action>>,
    /// The session's pending wakes, as the turn knows them (theseus-lherl).
    commitments: crate::commitments::Pending,
}

/// Why a turn did not run: a secret it needs is not there.
struct SecretRefusal {
    /// `secret_failed` or `secret_resolving`.
    class: &'static str,
    secret: String,
    error: String,
}

/// The principal of every local protocol client (file permissions are the auth).
pub const OPERATOR: &str = "operator";

fn turn_error(
    class: &str,
    session: &str,
    turn: &str,
    elapsed_ms: u64,
    source: anyhow::Error,
) -> anyhow::Error {
    TurnError {
        class: class.into(),
        transient: false,
        usage_unknown: false,
        turn_id: turn.into(),
        session_id: session.into(),
        elapsed_ms,
        trace: None,
        usage: Usage::default(),
        cost_usd: Some(0.0),
        tool_calls: 0,
        source,
    }
    .into()
}

/// A turn in flight: the context it hands the tool layer, its trace, and
/// what it has done so far, which becomes its result and its session's books.
struct Turn<'a> {
    /// `loop_index` stays `None`; a loop's tool calls get their own copy.
    tc: TurnCtx<'a>,
    target: &'a Target,
    continuation: bool,
    /// Its author is the pipeline's own (`defer_step::is_self`), so its work is noticed work.
    self_authored: bool,
    started: Instant,
    /// When the input arrived: the start of the frames the trace names the
    /// slowest of.
    arrived: Instant,
    trace: Trace,
    loops: u32,
    output: String,
    usage: Usage,
    /// `None` once any call's price is unknown.
    cost: Option<f64>,
    tool_calls: u32,
    /// The model's latest response.
    last: Option<ModelResponse>,
    /// The tool call waiting on a confirm.
    awaiting: Option<String>,
    /// The budget question the turn parks on: a call did not fit.
    budget_question: Option<String>,
    /// This turn retries a call the operator approved a reset for though it
    /// alone needed more than the whole limit (theseus-kks). If its first
    /// call still does not fit the limit, the turn ends instead of asking the
    /// same question again.
    retry_over_limit: bool,
    /// The self budget's account of this turn (theseus-pw1q.3).
    self_acct: self_budget_step::Acct,
    /// The burn-rate stop whose question is being asked (theseus-9e02).
    braked: Option<theseus_kernel::BurnStop>,
    /// Jobs this turn started or resumed in the background.
    background: Vec<String>,
    stop_reason: String,
    /// Each loop that said something: its index and its assistant node,
    /// which the reply's post names (theseus-q4v).
    said: Vec<(u32, String)>,
    /// A harness nudge is the newest thing this turn relayed, so the next
    /// loop's words answer the harness and not the person (theseus-emqbm,
    /// `nudge_step::relayed`). Taken by that loop's call.
    nudged: bool,
    /// The loops whose words answered one: streamed nowhere and left out of
    /// the reply's post, which keeps the turn's last words whatever prompted
    /// them (`reply_step::posted_loops`). The nodes are written as ever.
    nudge_answers: std::collections::BTreeSet<u32>,
    /// The surface's message the turn answers (a Discord message id).
    reply_to: Option<String>,
    /// Where its reply is posted instead of its session's place.
    answer_at: Option<String>,
    /// The wakes this turn took (DD8): each one's id and line, which the
    /// reply's post shows above it.
    wakes: Vec<Value>,
    /// Where the first of them was set from, for a reply whose session posts
    /// nowhere now.
    wake_target: Option<String>,
    /// The task reports that started this turn (W1, `wake_parent`): each
    /// one's task and line, which the reply's post shows above it, as a
    /// wake's.
    woke_by: Vec<Value>,
    /// How far the turn's end got with its books (R1), so that a fault part
    /// way through the end closes the rest and counts nothing twice: the
    /// session's copy has them, its write waits for the turn's last frame,
    /// and the `turn.ended` row is recorded.
    booked: bool,
    deferred: bool,
    ended: bool,
    /// What recall put in front of the model this turn (M6 30b), until its
    /// node rides the provider call's plan frame.
    recall: recall_step::Recalled,
    /// The loop signatures this turn has repeated (`p5kln-p0-unbounded-hold`).
    repeats: repeat_step::Repeats,
    /// Routing's verdict and what it moved (M5 25e).
    route: route_step::RouteState,
    /// The harness's part of the block beside the message, each part rendered once at the turn's first
    /// compile (`now_step`), composed at each freeze of its view.
    now: Option<now_step::Rendered>,
    /// The deliverable's note was said at the turn's close-out (`close_out_step::deliverable_note`).
    deliverable_told: bool,
    /// A running task's checkpoints (theseus-ypnxt.9).
    checkpoint: checkpoint_step::Checkpoints,
    /// A refusal moved the turn to its model's fallback (theseus-7gir.18).
    fallback: Option<fallback_step::FellBack<'a>>,
    /// The same-model retries of refusals (theseus-keuap).
    again: fallback_step::Again,
    /// The stop judged live, and the one nudge (theseus-fh0u).
    nudge: nudge_step::Nudge,
    /// The one end note, and the drop of the turn's files at its end (theseus-53ujg.1).
    end_note: end_note_step::Held,
    /// Its exchange, until its end writes it (issue 44).
    exchange: exchange_step::Held,
    /// The last compile's size and window (theseus-wy6v.2).
    fill: fill_step::Fill,
    /// The new compilations' system blocks on their way to the blobs (theseus-nsp9l.4).
    keeping: compile_step::Keeping,
    /// The standing blocks the turn's spec and tail carry, and its surface.
    standing: standing_step::Held,
    /// In a place set to `view`, the view its first call assembled (issue 44, `view_step`).
    view: Option<crate::compiler::view::Frozen>,
}

/// The class of a turn that faulted (R1): an error the turn did not report
/// itself, such as a frame the store would not write. It closes its books as
/// a failure does.
pub const FAULT_CLASS: &str = "internal";

/// Where a turn's body left it: to end as it planned, or to fail as it reports.
enum Next {
    Finish,
    Fail(Failure),
}

/// What a turn's body takes from its request, beside what its `Turn` holds.
struct Asked {
    /// `None`: a continuation.
    input: Option<String>,
    attachments: Vec<theseus_protocol::Attachment>,
    author: String,
    prompt: Option<crate::mcp::prompts::PromptInput>,
    /// The session's target, when the turn runs elsewhere than its last
    /// turn did: the input's frame writes it.
    moved: Option<TargetRef>,
}

/// What became of a loop's provider call.
enum Called {
    Answered(Box<(ModelResponse, Node)>),
    /// The call failed after it began; the turn reports it.
    Failed(Failure),
    /// Its reservation did not fit under the spend limit; nothing ran.
    OverBudget {
        needed: Micros,
        available: Micros,
        spent: Micros,
        limit: Micros,
    },
    /// The session's burn-rate line stopped the call (theseus-9e02): the
    /// owner is asked to resume it.
    Braked(theseus_kernel::BurnStop),
    /// A `/stop` landed (W1): the kernel planned no call, or the call's stream
    /// was cut (theseus-yey), and the turn ends.
    Stopped {
        by: String,
    },
}

/// A model call a `/stop` cut (theseus-yey): who stopped it, whether its
/// request was sent, and what is known of its use.
struct Cut {
    by: String,
    /// The stream was polled, so the request went out; a stop before that cut
    /// a call that used nothing.
    sent: bool,
    /// The characters streamed before the cut.
    out_chars: u64,
    /// The input estimate the call's reservation used.
    est_input: u64,
    /// What its request held of the provider's cache (theseus-3p36).
    prefix: cut_step::Prefix,
    /// What the provider's own usage events had said when the stream was cut
    /// (theseus-k48c): none when it had said nothing, and then the call is
    /// booked at the estimate above.
    seen: Option<Usage>,
}

/// Who stopped the turn `tc` runs, if a `/stop` landed during it (W1): the
/// execution's stop mark, which the turn's end clears.
fn stopped_by(tc: &TurnCtx<'_>) -> Result<Option<String>> {
    Ok(tc
        .kernel
        .execution(tc.execution_id)?
        .and_then(|e| e.stopped)
        .map(|s| s.by))
}

impl<'a> Turn<'a> {
    fn start(tc: TurnCtx<'a>, target: &'a Target, continuation: bool, arrived: Instant) -> Self {
        let started = Instant::now();
        let mut root = json!({
            "turn_id": tc.turn_id,
            "session_id": tc.session_id,
            "profile": target.profile,
            "provider": target.provider,
            "model": target.model,
            "continuation": continuation,
            "started_unix_ms": theseus_protocol::now_unix_ms(),
        });
        // Its request's wait in its connection's lane, before `arrived`
        // (theseus-klo2's review, finding 7).
        if let Some(us) = crate::rpc::ordered::lane_us() {
            root["lane_us"] = json!(us);
        }
        let trace = Trace::start_at(arrived, "turn", "turn", root);
        let execution_id = tc.execution_id;
        Self {
            tc,
            target,
            continuation,
            self_authored: false,
            started,
            arrived,
            trace,
            loops: 0,
            output: String::new(),
            usage: Usage::default(),
            cost: Some(0.0),
            tool_calls: 0,
            last: None,
            awaiting: None,
            budget_question: None,
            retry_over_limit: false,
            braked: None,
            self_acct: Default::default(),
            background: Vec::new(),
            stop_reason: String::new(),
            said: Vec::new(),
            nudged: false,
            nudge_answers: std::collections::BTreeSet::new(),
            reply_to: None,
            answer_at: None,
            wakes: Vec::new(),
            wake_target: None,
            woke_by: Vec::new(),
            booked: false,
            deferred: false,
            ended: false,
            recall: recall_step::Recalled::default(),
            repeats: repeat_step::Repeats::default(),
            route: route_step::RouteState::default(),
            now: None,
            deliverable_told: false,
            checkpoint: checkpoint_step::Checkpoints::default(),
            fallback: None,
            again: Default::default(),
            nudge: nudge_step::Nudge::default(),
            end_note: end_note_step::Held::of(execution_id),
            exchange: exchange_step::Held::default(),
            fill: fill_step::Fill::default(),
            keeping: compile_step::Keeping::default(),
            standing: standing_step::Held::default(),
            view: None,
        }
    }

    /// Record that the turn began, after its waits (`fact::turn::TurnStarted`).
    fn announce(&mut self, input: Option<&str>, files: usize, author: &str, waits: &Waits) {
        self.self_authored = defer_step::is_self(author);
        let f = fact::turn::TurnStarted {
            session_id: self.tc.session_id,
            turn_id: self.tc.turn_id,
            execution_id: &self.tc.guard.execution_id,
            kernel_turn: self.tc.guard.turn,
            target: self.target,
            continuation: self.continuation,
            input_chars: input.map(|s| s.chars().count()),
            attachments: files,
            author,
            secrets_us: waits.secrets_us,
            lock_us: waits.lock_us,
            admit_us: waits.admit_us,
            queued: waits.queued,
        };
        self.record(&f);
    }

    /// Record a fact of this turn's: its span in the trace, then every other
    /// channel it has.
    fn record<F: Fact>(&mut self, f: &F) {
        f.span(&mut self.trace);
        self.tc.record(f);
    }

    /// A fact of this turn's whose row rode in a frame the turn wrote itself
    /// (`Rec::row`): its span, its notification, and its sentences.
    fn announce_fact<F: Fact>(&mut self, f: &F) {
        f.span(&mut self.trace);
        self.tc.rec().announce(f);
    }

    /// Count the turn in its session. The end and the one failure exit
    /// (`fail`, which a fault reaches too) call it, so a turn that fails in a
    /// later loop keeps what its earlier loops spent; a second call, from a
    /// fault after the end booked it, counts nothing again (R1).
    fn close_books(&mut self, session: &mut SessionRecord) {
        if std::mem::replace(&mut self.booked, true) {
            return;
        }
        session.turns += 1;
        session.last_turn_id = Some(self.tc.turn_id.to_string());
        session.last_active_ms = theseus_protocol::now_unix_ms();
        session.tool_calls += self.tool_calls as u64;
        session.cost_usd += self.cost.unwrap_or(0.0);
        add_usage(&mut session.usage, &self.usage);
    }
}

/// The failure class of a request past the model's window that the turn's
/// own ring could not fit (theseus-9p88).
pub const WINDOW_CLASS: &str = "context_window";

/// A request of this turn's that the provider said passed the model's
/// window (theseus-9p88): it refused the prompt, or cut the answer there.
pub(crate) struct Overflowing {
    /// `refused` or `cut`.
    pub(crate) source: &'static str,
    /// What the next compilation rings by.
    pub(crate) hint: Overflowed,
    /// The request's digest: a retry that would render the same request is
    /// not made.
    pub(crate) digest: String,
    /// The catalog's window for the model.
    pub(crate) window: Option<u64>,
    /// A cut answer's output tokens.
    pub(crate) output: Option<u64>,
}

impl Overflowing {
    /// A 400 that says the prompt is too long.
    fn refused(compiled: &Compiled, o: Overflow, window: Option<u64>) -> Self {
        Overflowing {
            source: "refused",
            hint: Overflowed {
                counted: o.tokens,
                estimated: compiled.est_tokens,
                maximum: o.maximum,
                retrying: None,
            },
            digest: compiled.digest.clone(),
            window,
            output: None,
        }
    }

    /// An answer cut at the window: its prompt as the provider counted it,
    /// and that plus its output is where the window ended.
    fn cut(compiled: &Compiled, resp: &ModelResponse, node: &Node, window: Option<u64>) -> Self {
        let u = &resp.usage;
        let prompt = u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens;
        Overflowing {
            source: "cut",
            hint: Overflowed {
                counted: (prompt > 0).then_some(prompt),
                estimated: compiled.est_tokens,
                maximum: (prompt > 0).then_some(prompt + u.output_tokens),
                retrying: Some(node.id.clone()),
            },
            digest: compiled.digest.clone(),
            window,
            output: Some(u.output_tokens),
        }
    }

    /// What the provider said, in words.
    pub(crate) fn said(&self) -> String {
        let n = |v: u64| narrative::thousands(v);
        match (self.source, self.hint.counted, self.hint.maximum) {
            ("cut", Some(p), _) => format!(
                "its answer was cut there after {} tokens in and {} out",
                n(p),
                n(self.output.unwrap_or(0))
            ),
            ("cut", None, _) => "its answer was cut there".into(),
            (_, Some(c), Some(m)) => format!(
                "the provider refused it, counting {} tokens against a maximum of {}",
                n(c),
                n(m)
            ),
            (_, Some(c), None) => format!("the provider refused it, counting {} tokens", n(c)),
            _ => "the provider refused it as too long".into(),
        }
    }

    /// The window, as the catalog and the provider say it.
    pub(crate) fn window_words(&self) -> String {
        let n = |v: u64| narrative::thousands(v);
        match (self.window, self.hint.maximum) {
            (Some(w), Some(m)) if self.source == "refused" && m < w => format!(
                "a window of {} (the provider's; the catalog says {})",
                n(m),
                n(w)
            ),
            (Some(w), _) => format!("a window of {}", n(w)),
            (None, Some(m)) => format!("a window of {} (the provider's)", n(m)),
            (None, None) => "no window in the catalog".into(),
        }
    }
}

/// A turn that failed after it began, in a way the client is told about:
/// the kernel would not plan the provider call (the execution was
/// cancelled, the model has no price), or the call failed. A call over the
/// budget is not a failure: the turn parks on a question to the operator.
/// Any other error is a fault and propagates as it is.
struct Failure {
    class: String,
    transient: bool,
    usage_unknown: bool,
    /// The `turn.failed` row's reason.
    reason: String,
    source: anyhow::Error,
}

/// `f`'s future, built in this frame and moved to the heap (theseus-b4sf):
/// its caller's poll frame keeps only the closure and a pointer, where an
/// awaited call would keep a slot the size of the future at opt-level 0.
fn boxed<F: std::future::Future>(f: impl FnOnce() -> F) -> std::pin::Pin<Box<F>> {
    Box::pin(f())
}

/// What `run_inner` hands `run`: the result, how the turn ends, whether a
/// late result wakes it again, and the session's hold.
type Inner = (TurnSubmitResult, TurnEnd, bool, Option<SessionHold>);

impl TurnRunner {
    /// Make sure the session has a kernel execution (sessions written before
    /// M2 have none) and return it.
    fn execution_for(&self, session: &mut SessionRecord) -> Result<Execution> {
        if let Some(id) = &session.execution_id {
            if let Some(e) = self.kernel.execution(id)? {
                return Ok(e);
            }
        }
        let e = self.kernel.open_execution(
            &session.session_id,
            session.kind,
            Authority {
                principal: OPERATOR.to_string(),
                ..Default::default()
            },
            None,
            None,
        )?;
        session.execution_id = Some(e.id.clone());
        let written = self.store.update_session(&session.session_id, |r| {
            r.execution_id = Some(e.id.clone());
            Ok(vec![])
        })?;
        if written.is_none() {
            self.store.put_session(&session.session_id, session)?;
        }
        self.session_rec(&session.session_id, To::Nobody)
            .record(&fact::turn::ExecutionOpened {
                session_id: &session.session_id,
                execution_id: &e.id,
                limit_micros: e.budget.limit_micros,
            });
        Ok(e)
    }

    /// Where a session's own facts go, outside a turn: its rows are written
    /// now, and its notifications go `to`.
    fn session_rec<'a>(&'a self, session: &'a str, to: To<'a>) -> fact::Rec<'a> {
        fact::Rec {
            narrator: &self.narrator,
            session: Some(session),
            turn: None,
            to,
            store: &self.store,
        }
    }

    /// New input came while a budget question was open: the question is
    /// superseded (a new message is not an answer), and the turn's next call
    /// asks again if it still does not fit.
    fn supersede_budget_question(&self, exec: &Execution, sink: &EventSink, author: &str) {
        let Some(q) = exec.budget.question.as_deref() else {
            return;
        };
        match self.kernel.decline_action(
            q,
            OPERATOR,
            "superseded: a new message came instead of an answer; the next call asks again if \
             it still does not fit",
        ) {
            Ok(_) => {
                self.session_rec(&exec.session_id, To::Sink(sink)).record(
                    &fact::turn::BudgetQuestionSuperseded {
                        session_id: &exec.session_id,
                        question: q,
                        author,
                    },
                );
                if let Err(e) = self
                    .outbox
                    .closed(q, crate::outbox::Closed::new("superseded", Some(author)))
                {
                    tracing::warn!(error = %format!("{e:#}"), "the card's settle was not written");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, correlation_id = q, "superseding a budget question failed")
            }
        }
    }

    /// Wait for the secrets a turn needs (theseus-qa0): its provider's key,
    /// and the first round of every secret, since the exits must know each
    /// value before any request leaves (the record holds a value raw; a
    /// request is substituted from what the board holds ready now, and a
    /// secret that is `Failed` is not known). Both are
    /// usually there already. Returns how long it waited, or why the turn
    /// cannot run. Fail closed: no provider call goes out without its key.
    async fn await_secrets(&self, target: &Target) -> Result<u64, SecretRefusal> {
        let Some(name) = self
            .cfg
            .all_providers()
            .get(&target.provider)
            .map(|p| p.api_key_secret.clone())
        else {
            return Ok(0);
        };
        if self.secrets.get(&name).is_some() && self.secrets.is_settled() {
            return Ok(0);
        }
        let t0 = Instant::now();
        let outcome = match self.secrets.wait(&name, SECRET_WAIT).await {
            Waited::Ready(_) | Waited::Absent => {
                let left = SECRET_WAIT.saturating_sub(t0.elapsed());
                self.secrets
                    .wait_settled(left)
                    .await
                    .map_err(|still| SecretRefusal {
                        class: "secret_resolving",
                        error: format!(
                            "{} still resolving after {} s; the scrubber needs every value \
                             before a tool result passes through it",
                            still.join(", "),
                            SECRET_WAIT.as_secs()
                        ),
                        secret: still.join(", "),
                    })
            }
            Waited::Failed(why) => Err(SecretRefusal {
                class: "secret_failed",
                secret: name.clone(),
                error: why,
            }),
            Waited::Resolving => Err(SecretRefusal {
                class: "secret_resolving",
                secret: name.clone(),
                error: format!("still resolving after {} s", SECRET_WAIT.as_secs()),
            }),
        };
        let waited_us = t0.elapsed().as_micros() as u64;
        // A provider's first wait is a startup phase: a slow start names it.
        let phase = format!("provider.{}", target.provider);
        if !self.startup_log.has(&phase) {
            self.startup_log.record(
                &phase,
                true,
                t0,
                json!({"secret": name, "waited_ms": waited_us / 1000, "outcome": outcome.as_ref().err().map_or("ready", |r| r.class)}),
            );
        }
        outcome.map(|()| waited_us)
    }

    /// A turn whose secret is not there: an input turn is refused with the
    /// class, in the ledger and the narrative; a continuation fails as a
    /// fault, so the driver keeps its execution queued and tries again.
    fn refuse(&self, req: &TurnRequest, r: SecretRefusal) -> anyhow::Error {
        let sid = &req.session.session_id;
        let source = anyhow::anyhow!("secret {} did not resolve: {}", r.secret, r.error);
        if req.input.is_none() {
            return source.context(format!(
                "a continuation of session {sid} waits for its secrets"
            ));
        }
        self.session_rec(sid, To::Sink(&req.sink))
            .record(&fact::turn::TurnRefused {
                class: r.class,
                secret: &r.secret,
                error: &r.error,
                author: &req.author,
                provider: &req.target.provider,
            });
        tracing::warn!(session_id = %sid, class = r.class, secret = %r.secret, error = %r.error, "turn refused: a secret it needs is not there");
        turn_error(r.class, sid, "", 0, source)
    }

    /// A `/stop` of `execution_id` by `by` is landing (theseus-hmwv): said
    /// before the kernel's stop, so a turn admitted once it is done sees it.
    /// Returns the notice's stamp, which the stop's answer asks about
    /// (`stop_taken_by_a_turn`).
    pub fn stop_landed(&self, execution_id: &str, by: &str) -> Instant {
        let at = Instant::now();
        // A hold's yes kept for a call of this execution is moot: nothing runs on it now.
        self.tools.profile.drop_yes_of(execution_id);
        self.latest_stops.lock().unwrap().insert(
            execution_id.to_string(),
            StopNotice {
                at,
                by: by.to_string(),
                taken: false,
            },
        );
        at
    }

    /// The `/stop` of `execution_id` that landed after an input that arrived
    /// at `arrived`, if one did.
    fn stopped_since(&self, execution_id: &str, arrived: Instant) -> Option<StopNotice> {
        self.latest_stops
            .lock()
            .unwrap()
            .get(execution_id)
            .filter(|n| n.at > arrived)
            .cloned()
    }

    /// A turn stopped itself on `notice`: the stop that sent it was heard.
    fn stop_taken(&self, execution_id: &str, notice: &StopNotice) {
        if let Some(n) = self.latest_stops.lock().unwrap().get_mut(execution_id) {
            if n.at == notice.at {
                n.taken = true;
            }
        }
    }

    /// Whether a turn stopped itself on the notice stamped `landed`. A turn
    /// admitted between a stop's notice and its kernel step stops itself on
    /// the notice and ends, so the kernel's step finds the execution parked
    /// again and no turn: the stop's answer asks this, so that it still says
    /// a turn was running, as it does when the kernel's step comes first.
    /// A later stop's notice is another stamp, and answers for itself.
    pub fn stop_taken_by_a_turn(&self, execution_id: &str, landed: Instant) -> bool {
        self.latest_stops
            .lock()
            .unwrap()
            .get(execution_id)
            .is_some_and(|n| n.at == landed && n.taken)
    }

    /// One turn, its future on the heap (theseus-b4sf): at opt-level 0 a
    /// caller's poll frame keeps a slot the size of each future it builds
    /// (a turn's was 160 KiB), so this returns a box, not the future.
    pub fn run(
        &self,
        mut req: TurnRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TurnSubmitResult>> + Send + '_>>
    {
        Box::pin(async move {
            let arrived = *req.arrived.get_or_insert_with(Instant::now);
            // Listed from its start until it holds its execution: a `/stop`
            // that lands while this input's turn waits for its secrets, or
            // for admission, reaches it (`AdmitWaiting`).
            let session = req.session.session_id.clone();
            let waiting = req
                .input
                .is_some()
                .then(|| self.admit_waiting.enter(&session, arrived));
            let execution = req.session.execution_id.clone();
            let ran = self.run_body(req, waiting.as_ref()).await;
            if let (Err(e), Some(w)) = (&ran, &waiting) {
                // It failed before it held its execution: a stop that
                // counted it was not what ended it.
                if !w.is_held() && w.withdraw() {
                    self.stop_not_taken(&session, execution.as_deref(), arrived, e);
                }
            }
            ran
        })
    }

    /// A turn a stop counted as reached ended before admission for a reason
    /// of its own (its secrets refused, its execution not runnable). The
    /// stop's answer and the kernel's row had gone by then, and said a turn
    /// was reached: the ledger and the session say what in fact ended it.
    fn stop_not_taken(
        &self,
        session_id: &str,
        execution_id: Option<&str>,
        arrived: Instant,
        error: &anyhow::Error,
    ) {
        let by = execution_id
            .and_then(|x| self.stopped_since(x, arrived))
            .map_or_else(|| "a stop".to_string(), |n| n.by);
        self.session_rec(session_id, To::Nobody)
            .record(&fact::driver::StopNotTaken {
                execution_id: execution_id.unwrap_or_default(),
                by: &by,
                why: &failure_reason(error),
            });
    }

    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn run_body(
        &self,
        mut req: TurnRequest,
        waiting: Option<&admit_wait::Waiting<'_>>,
    ) -> Result<TurnSubmitResult> {
        let arrived = req.arrived.unwrap_or_else(Instant::now);
        let continuation = req.input.is_none();
        // A person's message: Jev's connections open now, beside the
        // admission's frames, for the judgments its input node will ask
        // (theseus-otny). A slash command asks none.
        if req
            .input
            .as_deref()
            .is_some_and(|i| !crate::judge::inbound::slash_command(i))
        {
            self.judge.warm_on_message();
            // And the index's embedding model, loading beside the admission
            // so that this turn's recall finds it (issue 21 of 1.0.7).
            self.memory.warm_on_message();
        }
        // Running until this returns, after the turn's last frame: the memory
        // pass writes only between turns (theseus-ms5m).
        let _running = self.pass.turns().begin().await;
        let secret_wait_us = match self.await_secrets(&req.target).await {
            Ok(us) => us,
            Err(r) => return Err(self.refuse(&req, r)),
        };
        // The frames the turn writes before its store handle exists
        // (theseus-wz4y): its execution's opening, a superseded question,
        // and its admission. Each stretch between two reads of this thread's
        // count has no `.await`, so the count is the turn's own.
        let mut admission_frames = 0u64;
        let f0 = frames_written_here();
        let exec = self.execution_for(&mut req.session);
        admission_frames += frames_written_here() - f0;
        let exec = exec?;
        let f0 = frames_written_here();
        // The narrative's clock, taken only when it is on.
        let admitting = self.narrator.on().then(Instant::now);
        // An input holds its execution's turn, writing nothing: its wake and
        // admission ride its input's frame (theseus-2uby, `admit_step`). When
        // admission must wait, the wait below writes the wake alone and takes
        // the turn, as `admit_input` did (theseus-l6y).
        let mut admitted = None;
        // Its place in the queue (theseus-4g0l): a slot now only when one is
        // free and no turn waits.
        let class = admission::Class::of(req.session.kind, !continuation);
        if !continuation {
            self.supersede_budget_question(&exec, &req.sink, &req.author);
            // A slot from the queue first (theseus-4g0l), then the turn held
            // without the kernel's count, the queue keeps the ceiling; with none
            // free, the wake alone, and the queue's wait below admits.
            let slot = self.tools.admission.try_take(class);
            let held = match &slot {
                Some(_) => self.kernel.hold_turn_queued(&exec.id),
                None => self.kernel.admit_input_queued(&exec.id, false),
            };
            match held {
                Ok(g) => admitted = slot.map(|s| (g, s)),
                Err(e) => match e.downcast_ref::<KernelError>() {
                    Some(KernelError::AdmissionFull { .. } | KernelError::TurnHeld { .. })
                    | Some(KernelError::NotRunnable {
                        state: "running", ..
                    }) => {}
                    Some(KernelError::ExecutionRetained { .. }) => {
                        return Err(turn_error(
                            "retained_record",
                            &req.session.session_id,
                            "",
                            0,
                            e,
                        ));
                    }
                    Some(KernelError::NotRunnable { state, .. }) => {
                        self.session_rec(&req.session.session_id, To::Sink(&req.sink))
                            .record(&fact::turn::TurnNotRunnable {
                                author: &req.author,
                                state,
                            });
                        return Err(turn_error(
                            &format!("execution_{state}"),
                            &req.session.session_id,
                            "",
                            0,
                            e,
                        ));
                    }
                    _ => return Err(e),
                },
            }
            if matches!(exec.state, ExecState::Waiting | ExecState::Blocked) {
                self.session_rec(&req.session.session_id, To::Sink(&req.sink))
                    .record(&fact::turn::WokenByInput {
                        author: &req.author,
                    });
            }
        }
        admission_frames += frames_written_here() - f0;
        // Held or waited for: a stop behind it on its lane reaches it (theseus-klo2).
        crate::rpc::ordered::reachable();
        // A person's message that finds a turn running joins it (theseus-qu1lc).
        if admitted.is_none() {
            if let Some(joined) = self.steered(&req, class, arrived).await {
                return joined;
            }
        }
        // The slot is the turn's until it returns, after its last frame.
        let (guard, _slot) = match admitted {
            Some((g, mut slot)) => {
                slot.bind(&g.execution_id);
                (g, slot)
            }
            None => match self
                .admit(
                    &exec.id,
                    arrived,
                    !continuation,
                    class,
                    !continuation && req.sink.owner,
                    &mut admission_frames,
                )
                .await?
            {
                Some(held) => held,
                None => {
                    anyhow::bail!("execution {} is not ready for a continuation turn", exec.id)
                }
            },
        };
        if let Some(w) = waiting {
            w.held();
        }
        let steer = self
            .tools
            .job_waits
            .steers
            .open(&req.session.session_id, self.cfg.kernel.steer_queued);
        self.note_owner_turn(&req, class);
        let f0 = frames_written_here();
        // A `/stop` that landed after this input arrived and before its turn
        // held the execution (theseus-hmwv) found no turn to mark: it waited
        // on its secrets, or for admission. The operator said "do this",
        // then "stop", so it stops this turn now, as a stop during a turn
        // does: the kernel marks it, and its first step plans nothing.
        // A held turn reads it in its admission's frame (`admit_step`).
        if !continuation && self.kernel.admitted(&guard).unwrap_or(true) {
            if let Some(notice) = self.stopped_since(&guard.execution_id, arrived) {
                match self.kernel.stop_execution(&guard.execution_id, &notice.by) {
                    Ok(_) => self.stop_taken(&guard.execution_id, &notice),
                    Err(e) => {
                        tracing::warn!(execution_id = %guard.execution_id, error = %format!("{e:#}"), "a stop before admission could not mark the turn");
                    }
                }
            }
        }
        let admit_us = admitting.map_or(0, |t| t.elapsed().as_micros() as u64);
        let admission_wait_us = arrived.elapsed().as_micros() as u64;
        let failure_sink = req.sink.clone();
        let waits = Waits {
            secrets_us: secret_wait_us,
            lock_us: admission_wait_us,
            admit_us,
            queued: _slot.queued(),
        };
        let store = self.store.for_turn(&req.session.session_id);
        store.count_frames(admission_frames + (frames_written_here() - f0));
        let frames = Frames {
            // The results this turn settles for its execution are its own:
            // it reads them itself, so none is queued (theseus-l6y).
            kernel: self
                .kernel
                .view(store.shared())
                .turn_of(&guard.execution_id),
            store,
            posts: Default::default(),
            commitments: Default::default(),
        };
        // A task (DD7): where it reports, and its title, for its report.
        let task_of = req.session.task.clone();
        let answer_at = req.answer_at.clone();
        let task_title = req.session.title.clone();
        let task_session = req.session.session_id.clone();
        // The turn's session write waits for its last frame, under the
        // record's lock, which is held until that frame is written.
        let (r, session_hold) = match self.run_inner(&guard, &frames, req, arrived, waits).await {
            Ok((res, end, rewake, hold)) => (Ok((res, end, rewake)), hold),
            Err(e) => (Err(e), None),
        };
        let (end, rewake) = match &r {
            Ok((_, end, rewake)) => (end.clone(), *rewake),
            // No one waits on a task's input: a failed turn ends it, and it
            // reports that it failed.
            Err(e) if task_of.is_some() => (
                TurnEnd::Fail {
                    reason: failure_reason(e),
                },
                false,
            ),
            Err(_) => (TurnEnd::Wait { wake: Wake::Input }, false),
        };
        let exec_id = guard.execution_id.clone();
        // A turn a `/stop` ended (W1): its end parks the execution on input,
        // and it says nothing of its own, since the stop's answer did.
        let stopped = frames
            .kernel
            .execution(&exec_id)
            .ok()
            .flatten()
            .and_then(|e| e.stopped)
            .map(|s| s.by);
        // Where it parks, for its line: kept only when the narrative is on.
        let parked = self.narrator.on().then(|| end.clone());
        // A task's report rides in the frame that ends it, and only in a
        // frame that does: a turn that ends after a cancel landed writes
        // none, and the cancel reports (DD7).
        let reported = std::sync::Mutex::new(None);
        let said = std::sync::Mutex::new(None);
        // Its record closes in that frame too (39a), under its lock.
        let (lock, closing) = Closing::new(&frames.store, task_of.as_ref(), &failure_sink);
        // A late result's wake rides in the end's frame (theseus-6qwr).
        let hold_gone = || task_of.is_some() && !self.task_paused(&task_session);
        let ran_on = r
            .as_ref()
            .ok()
            .map(|(res, _, _)| crate::session::TargetRef {
                profile: res.profile.clone(),
                provider: res.provider.clone(),
                model: res.model.clone(),
            });
        let ended = end_step::end_and_wake(&frames.kernel, guard, end, rewake, hold_gone, |e| {
            let Some(task) = task_of.as_ref().filter(|_| e.state.is_terminal()) else {
                return Ok(vec![]);
            };
            let last = if e.state == ExecState::Complete {
                let nodes = frames.store.transcript(&e.session_id)?;
                crate::task::last_message(nodes.iter().map(|(_, n)| &**n))
            } else {
                None
            };
            let report = crate::task::Report {
                // What the task ran on, as its turn says (issue 28).
                ran_on: ran_on.clone(),
                ..crate::task::Report::new(e, task_title.clone(), last)
            }
            .with_close(crate::task::close_of(&frames.store, &e.session_id));
            let mut records = closing.records(e, report.node.as_deref())?;
            let joined = self.tools.job_waits.steers.joined(&e.session_id);
            records.extend(crate::task::joined_record(&e.session_id, joined)?);
            // Work handed to a profile (`profile.run`) speaks in the conversation as well.
            let spoke = crate::profile_tool::run::speak(&self.outbox, &frames.store, e, &report)?;
            let place = spoke.map(|s| {
                records.extend(s.records);
                *said.lock().unwrap() = Some(s.post);
                s.place
            });
            let Some(target) = task.target_for_now(&frames.store, &e.session_id, &report) else {
                return Ok(records);
            };
            // Where its report would go is the place it spoke in: one message stands for both.
            if place.as_deref() == Some(target.as_str()) {
                return Ok(records);
            }
            let (post, more) =
                self.outbox
                    .stage(&e.session_id, &e.id, &target, report.post_body())?;
            records.extend(more);
            *reported.lock().unwrap() = Some(post);
            Ok(records)
        });
        let (ended, woke) = match ended {
            Ok((e, woke)) => (Ok(e), woke),
            Err(e) => (Err(e), false),
        };
        drop(lock);
        let closed = closing.end();
        // A cancel that landed while the turn ran left the calls it was making
        // unanswered, and wrote nothing into this turn's transcript: this turn
        // answers them now that it has ended (theseus-0o8).
        let cancelled = matches!(&ended, Ok(e) if e.state == ExecState::Cancelled);
        let session_ended = matches!(&ended, Ok(e) if e.state.is_terminal());
        if let Err(e) = ended {
            tracing::warn!(error = %e, "end_turn failed");
        }
        // Rows still waiting when the turn's last frame failed or wrote none.
        match frames.store.flush() {
            // The turn's posts rode in its last frame: now a binding may send them.
            Ok(()) => {
                for a in frames.posts.lock().unwrap().drain(..) {
                    self.outbox.posted(&a);
                }
                if let Some(a) = reported.lock().unwrap().take() {
                    self.outbox.posted(&a);
                }
                if let Some(a) = said.lock().unwrap().take() {
                    self.outbox.posted(&a);
                }
                let to = self.session_rec(&failure_sink.session_id, To::Sink(&failure_sink));
                crate::task_graph::tools::announce_closed(&to, closed);
            }
            Err(e) => tracing::warn!(error = %e, "ledger append failed"),
        }
        drop(session_hold);
        // What an asked profile answered this turn and the turn did not post is not kept until a
        // restart (issue 28): a faulted turn's is posted below, as the answer of a turn that
        // failed; a task's is dropped.
        let unposted = self.tools.profile.sweep(&failure_sink.session_id);
        self.last_request.turn_ended(&failure_sink.session_id);
        clock_step::turn_ended(&failure_sink.session_id);
        if let Ok((res, _, _)) = &r {
            self.judge.after_turn(res, task_of.is_some());
            self.pass.after_turn(res);
        }
        // A failed turn extends its session's run of failures (theseus-ljr),
        // which says whether its execution retries and whether the turn posts
        // the run's notice. A task's failure ends it (DD7), and a stopped
        // turn waits on its next input (W1): neither is in a run.
        let settled = frames.kernel.own_settled() > 0;
        let run = match &r {
            Err(e) if task_of.is_none() && stopped.is_none() => {
                Some(self.extend_failing(&failure_sink.session_id, continuation, e, settled))
            }
            _ => None,
        };
        // A turn that faults after it settled a result it reads itself (its
        // provider call's answer, a call's result) is woken, so that its
        // continuation reads what it has not and resumes its unanswered
        // calls, unless its run of failures parks it on input. That result's
        // queue entry once requeued it; the entry is not written now
        // (theseus-l6y).
        let woke_on_fault = run.as_ref().is_some_and(|(_, then, _)| *then != Then::Park)
            && settled
            && match self.kernel.wake(&exec_id, "fault") {
                Ok(_) => true,
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), execution_id = %exec_id, "a faulted turn's execution was not woken");
                    false
                }
            };
        let rec = self.session_rec(&failure_sink.session_id, To::Sink(&failure_sink));
        if session_ended {
            self.tools.reads.forget(&failure_sink.session_id);
            self.tools.job_waits.task_settled();
            self.tools.job_waits.steers.unjoin(&failure_sink.session_id);
            let by = crate::term::BY_SESSION_END;
            self.tools
                .terms
                .close_session_recorded(&failure_sink.session_id, by, &rec)
                .await;
        }
        if cancelled {
            match self.tools.answer_after_cancel(
                &self.kernel,
                &self.store,
                &failure_sink.session_id,
                &exec_id,
            ) {
                Ok(nodes) => {
                    crate::toolrun::announce_cancelled(&rec, &failure_sink.session_id, &nodes)
                }
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), execution_id = %exec_id, "a cancelled turn's unanswered calls were not answered");
                }
            }
        }
        if let Some(end) = &parked {
            let turn_id = match &r {
                Ok((res, _, _)) => Some(res.turn_id.clone()),
                Err(e) => e
                    .downcast_ref::<TurnError>()
                    .map(|t| t.turn_id.clone())
                    .filter(|t| !t.is_empty()),
            };
            rec.in_turn(turn_id.as_deref()).record(&fact::turn::Parked {
                end,
                kernel: &self.kernel,
            });
        }
        if let Err(e) = &r {
            let te = e.downcast_ref::<TurnError>();
            if let Some(by) = &stopped {
                rec.record(&fact::turn::StoppedAtStep { by });
            } else if te.is_none_or(|t| t.class == FAULT_CLASS) {
                // A fault, not a failure the turn reports itself: one before
                // the turn began, or one its books closed on (`fault`, R1).
                rec.record(&fact::turn::TurnFaulted);
            }
            let failed = theseus_protocol::TurnFailed {
                session_id: failure_sink.session_id.clone(),
                turn_id: te.map(|t| t.turn_id.clone()).filter(|t| !t.is_empty()),
                execution_id: Some(exec_id.clone()),
                continuation,
                class: match &stopped {
                    Some(_) => Some("stopped".into()),
                    None => te.map(|t| t.class.clone()),
                },
                // The class and the turn ride beside it: the cause, once.
                error: te.map_or_else(|| format!("{e:#}"), |t| format!("{:#}", t.source)),
                then: run.as_ref().map(|(_, then, _)| then.as_str().to_string()),
            };
            // A failed turn must be seen where its reply would have gone, once
            // per run of failures (theseus-ljr): its first, if it retries with
            // backoff, and the one that parks it. A task's report says it
            // failed instead (DD7), and a stopped turn's stop said it (W1).
            if let Some((failing, _, true)) = &run {
                let mut body = serde_json::to_value(&failed).unwrap_or_default();
                body["kind"] = json!("failed");
                body["turns"] = json!(failing.turns);
                if let Err(e) = self.outbox.post_for(&failed.session_id, &exec_id, body) {
                    tracing::warn!(error = %format!("{e:#}"), "the failed turn's notice was not written");
                }
            }
            // An answer that was produced and paid for is shown, though its turn then failed or
            // was stopped: it was staged in no frame, so it is posted here, marked as such
            // (issue 28). A task's turn posts none.
            if task_of.is_none() {
                self.post_unposted_answers(
                    (
                        &failure_sink.session_id,
                        &exec_id,
                        failed.turn_id.as_deref().unwrap_or_default(),
                        answer_at,
                    ),
                    unposted,
                    if stopped.is_some() {
                        "stopped"
                    } else {
                        "failed"
                    },
                );
            }
            rec.record(&fact::turn::TurnFailureTold { failed: &failed });
        }
        // A background result landed while the turn ran, which the model
        // has not read: the end's frame queued it for the driver's next turn.
        if woke {
            rec.record(&fact::turn::WokenAgain);
        }
        if let Some((f, then, _)) = &run {
            rec.record(&fact::turn::RetryDecided {
                run: f,
                then: *then,
                woke: woke_on_fault,
                settled,
            });
        }
        self.admission.notify_waiters();
        steer.finish(r.as_ref().ok().map(|(res, _, _)| res));
        r.map(|(res, _, _)| res)
    }

    /// Extend the session's run of failures by a failed turn (theseus-ljr):
    /// the rule's answer (`Failing::after`), written with the run in one
    /// frame, beside a `turn.next` row. An input turn starts a new run. A
    /// record that cannot be written still gets the rule's answer, from the
    /// run as it was.
    fn extend_failing(
        &self,
        sid: &str,
        continuation: bool,
        e: &anyhow::Error,
        settled: bool,
    ) -> (crate::session::Failing, Then, bool) {
        let te = e.downcast_ref::<TurnError>();
        // A fault is not a class the provider gave: one retry, as any
        // failure that may not pass.
        let (class, transient) =
            te.map_or(("internal", false), |t| (t.class.as_str(), t.transient));
        let mut decided = None;
        let written = self.store.update_session(sid, |r| {
            let prev = r.failing.take().filter(|_| continuation);
            let (run, then, notice) = crate::session::Failing::after(
                prev,
                class,
                transient,
                settled,
                theseus_protocol::now_unix_ms(),
                self.failed_turns.driver_retries(),
            );
            let row = self
                .session_rec(sid, To::Nobody)
                .in_turn(te.map(|t| t.turn_id.as_str()).filter(|t| !t.is_empty()))
                .row(&fact::turn::TurnNext {
                    then,
                    class,
                    transient,
                    settled,
                    run: &run,
                    notice,
                })?;
            r.failing = Some(run.clone());
            decided = Some((run, then, notice));
            Ok(vec![row])
        });
        match (written, decided) {
            (Ok(Some(_)), Some(d)) => d,
            (w, _) => {
                if let Err(err) = w {
                    tracing::warn!(session_id = %sid, error = %format!("{err:#}"), "the run of failures was not written");
                }
                let prev = self
                    .store
                    .get_session::<SessionRecord>(sid)
                    .ok()
                    .flatten()
                    .and_then(|r| r.failing)
                    .filter(|_| continuation);
                crate::session::Failing::after(
                    prev,
                    class,
                    transient,
                    settled,
                    theseus_protocol::now_unix_ms(),
                    self.failed_turns.driver_retries(),
                )
            }
        }
    }

    /// One turn under the lock (§3.3): catch up on what happened while no
    /// turn ran, write the input, run loops while the model has something new
    /// to read, then book the turn and decide where the execution waits.
    /// Its future is boxed, as `run`'s is (theseus-b4sf).
    fn run_inner<'s>(
        &'s self,
        guard: &'s TurnGuard,
        frames: &'s Frames,
        req: TurnRequest,
        arrived: Instant,
        waits: Waits,
    ) -> std::pin::Pin<Box<impl std::future::Future<Output = Result<Inner>> + Send + 's>> {
        boxed(move || self.run_inner_body(guard, frames, req, arrived, waits))
    }

    async fn run_inner_body(
        &self,
        guard: &TurnGuard,
        frames: &Frames,
        req: TurnRequest,
        arrived: Instant,
        waits: Waits,
    ) -> Result<Inner> {
        let TurnRequest {
            mut session,
            input,
            mut target,
            sink,
            author,
            attachments,
            reply_to,
            prompt,
            answer_at,
            surface,
            ..
        } = req;
        // The caller's copy may be stale: re-read under the lock. Its
        // directory is the one `turn.submit` moved it to (theseus-aab7).
        if let Some(fresh) = self
            .store
            .get_session::<SessionRecord>(&session.session_id)?
        {
            session = SessionRecord {
                dir: session.dir.or(fresh.dir.clone()),
                ..fresh
            };
        }
        self.speak_as_session(&mut target, &session);
        let (target, read) = self.route_base(&mut session, target, input.is_some());
        let provider = self
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown provider {:?}", target.provider))?;
        // Where routing moves the turn (25e), a refusal (theseus-7gir.18), and
        // a task's checkpoints (theseus-ypnxt.9).
        let slots = data_step::slots();
        // What this turn runs on is the session's target from the turn's
        // start (theseus-kol): its copy carries it into every session write
        // the turn makes (a recompile's, a failure's, its end's), and an
        // input that changes it writes it in the input's own frame. So a
        // continuation after a crash or a failed call runs where this turn
        // ran, not on whatever profile is live.
        let runs_on = TargetRef::from(&target);
        let moved = session.last_target.as_ref() != Some(&runs_on);
        session.last_target = Some(runs_on.clone());
        // New input starts a new run of failures (theseus-ljr): it is the
        // retry a parked execution waits for. The turn's session writes
        // carry the reset.
        if input.is_some() {
            session.failing = None;
        }
        let sid = session.session_id.clone();
        let turn_id = crate::new_id("turn");
        let task_of = session.task.clone();
        let place = self.view_of(&sid);
        let dir = session.dir.clone();
        let tc = TurnCtx {
            kernel: &frames.kernel,
            store: &frames.store,
            guard,
            session_id: &sid,
            execution_id: &guard.execution_id,
            turn_id: &turn_id,
            loop_index: None,
            sink: &sink,
            confirm_ttl_ms: self.ask_ttl_ms(&frames.store, &sid, task_of.is_some()),
            narrator: &self.narrator,
            outbox: &self.outbox,
            posts: &frames.posts,
            target: Some(&target),
            task: task_of.as_ref(),
            // Taken again once the turn has read its wakes and reports.
            class: place.class,
            ceiling: place.ceiling,
            places: crate::places::Places {
                rule: &self.place_rule,
                cfg: &self.cfg,
            },
            dir: dir.as_deref(),
            commitments: &frames.commitments,
        };
        if self.narrator.on() && self.narrator.first_sight(&sid) && session.turns > 0 {
            tc.rec().in_turn(None).record(&fact::turn::SessionResumed {
                session_id: &sid,
                turns: session.turns,
                cost_usd: session.cost_usd,
            });
        }
        let mut t = Turn::start(tc, &target, input.is_none(), arrived);
        (t.reply_to, t.answer_at) = (reply_to, answer_at);
        t.standing.surface = surface;
        t.route.read = read;
        t.announce(input.as_deref(), attachments.len(), &author, &waits);
        let asked = Asked {
            input,
            attachments,
            author,
            prompt,
            moved: moved.then_some(runs_on),
        };
        // Every exit from here closes the turn's books (R1): a failure the
        // turn reports (`fail`), and any other error, a fault, which `fault`
        // books the same way before it is returned. The body's `?`s all
        // land here, so no step after a paid loop can skip its spend.
        match boxed(|| {
            let fork = (&slots.0, &slots.1, &slots.2);
            self.body_kept(&mut t, &mut session, provider.as_ref(), asked, fork)
        })
        .await
        {
            Ok(Next::Finish) => self.finish(t, &mut session),
            Ok(Next::Fail(f)) => Err(Self::fail(t, &mut session, f)),
            Err(e) => Err(Self::fault(t, &mut session, e)),
        }
    }

    /// The turn's body, then the wait for the new system blocks it wrote
    /// beside its calls (theseus-nsp9l.4): a turn that has ended has its
    /// blocks kept.
    async fn body_kept<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        provider: &dyn Provider,
        asked: Asked,
        fork: (
            &'a OnceLock<Target>,
            &'a OnceLock<Target>,
            &'a checkpoint_step::Slots,
        ),
    ) -> Result<Next> {
        let body = self.turn_body(t, session, provider, asked, fork).await;
        Self::blocks_kept(t).await;
        body
    }

    /// The turn's body (§3.3): catch up on what happened while no turn ran,
    /// write the input, and run loops while the model has something new to
    /// read. An error is a fault, which the caller books (R1). The shape
    /// budget's marks came with its code, from `run_inner`.
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn turn_body<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        provider: &dyn Provider,
        asked: Asked,
        (routed, fallback, moves): (
            &'a OnceLock<Target>,
            &'a OnceLock<Target>,
            &'a checkpoint_step::Slots,
        ),
    ) -> Result<Next> {
        let Asked {
            input,
            attachments,
            author,
            prompt,
            moved,
        } = asked;
        let (sid, target) = (t.tc.session_id, t.target);

        // 1. What happened while no turn was running. A held turn that has
        // anything to take is admitted first (theseus-2uby).
        if let Err(e) = self.admit_before_catch_up(t) {
            return Ok(Next::Fail(Self::not_admitted(t, &author, e)?));
        }
        let caught_up = boxed(|| self.catch_up(t, input.is_some())).await?;
        // Where the turn's words go, now that it has taken its wakes and
        // reports: their place, when the session's own has moved on.
        let here = self.view_of(sid);
        (t.tc.class, t.tc.ceiling) = (here.class, here.ceiling);

        // 2. The new input, with its files in the same node and frame.
        let prompted = prompt.is_some();
        if let Some(p) = prompt {
            if let Err(e) = self.admit_alone(t) {
                return Ok(Next::Fail(Self::not_admitted(t, &author, e)?));
            }
            self.write_prompt_input(t, session, p, moved.as_ref())?;
            crate::rpc::ordered::applied();
        } else if let Some(text) = &input {
            let files = self.accept_files(t, attachments);
            let first_file = files.first().map(|a| a.name.clone());
            let node = t.user_node(&author, text, files);
            self.title_input(session, text, first_file.as_deref());
            // The turn's admission rides in the input's frame (theseus-2uby),
            // and so does a new target, under the record's lock; the same one
            // writes nothing more.
            // A Discord message's id rides in the node's frame (theseus-ejdk),
            // so a reply that cites the message can link to it.
            let origin = t
                .reply_to
                .as_deref()
                .and_then(|m| crate::recall::cite::origin_record(&node.id, "discord", m));
            let written = match &moved {
                Some(runs_on) => t.tc.store.with_session(sid, |mut r| {
                    r.last_target = Some(runs_on.clone());
                    let rec = NewRecord::json(theseus_store::kinds::SESSION, Some(sid), &r)?;
                    let recs = [node.record()?]
                        .into_iter()
                        .chain(origin.clone())
                        .chain([rec]);
                    Ok(self.admit_with(t, recs.collect()))
                })?,
                None => None,
            };
            let written = match written {
                Some(w) => w,
                None => self.admit_with(t, [node.record()?].into_iter().chain(origin).collect()),
            };
            if let Err(e) = written {
                return Ok(Next::Fail(Self::not_admitted(t, &author, e)?));
            }
            // Stored: the next request on its client's lane may run (theseus-klo2).
            crate::rpc::ordered::applied();
            t.tc.node_written(&node);
            // Its submission's mark and its exchange (issue 44).
            let arrival = t.tc.sink.arrival.clone();
            Self::exchange_heard(t, &node, arrival.as_deref(), false);
            self.inbound_point(t, &node, &author, false);
            self.content_input(t, &node);
        } else {
            // A task's first turn has no message: its brief is judged.
            self.brief_point(t);
            self.content_brief(t);
        }
        self.exchange_open(t, prompted);

        // 3. The loops, while the model has something new to read.
        let mut run_model = Self::has_news(t, input.is_some() || caught_up > 0)?;
        // The spec is fixed for the turn: a context file edited during it
        // recompiles the next turn, never between a tool call and its result.
        // So is the place's class (the place rule): a class that changes
        // mid-turn applies at the next turn's first loop.
        let (mut spec, unreadable) = self.request_spec(target, session.kind, t.tc.place());
        let flagged = self.session_flagged(sid); // §8's notice, read once (S31-1)
        spec.session_text = self.session_block(t.tc.dir, t.tc.place(), flagged);
        spec.walk = self.walk(sid, t.tc.place().class);
        self.stand(t, &mut spec);
        self.helper_spec(&mut spec, sid);
        self.families_ready(t, &spec.model).await;
        self.injection(t, false);
        self.defer_tools(t, &mut spec);
        self.observe_persona(t, target, &spec);
        let (project_dir, project_place) = (t.tc.dir, t.tc.place());
        self.observe_project(t, project_dir, project_place, flagged);
        for u in &unreadable {
            tracing::warn!(path = %u.path, error = %u.error, session_id = %sid,
                "context file unreadable: the system block says it is missing (warned once per daemon run)");
            t.record(&fact::turn::ContextFileMissing {
                path: &u.path,
                error: &u.error,
                profile: &target.profile,
                persona: u.persona.as_deref(),
            });
            self.persona_file_missing(u);
        }
        // A new chain within the turn, when the provider dropped a block whose prefix changed.
        let mut force = false;
        // A provider's 400 that names an image hides it, and the call is made
        // again with its line, once a turn (theseus-0s4); `strip` asks the
        // next compilation to drop the thinking the change sat under.
        let mut image_retried = false;
        let mut effort_asked = false;
        // A transient failure's retries in this turn (`[model.retries]`).
        let mut retried = retry_step::Retried::default();
        let mut strip: Option<&'static str> = None;
        // A request the provider said passed the window (theseus-9p88): the
        // next loop recompiles with a ring by the provider's numbers and calls
        // once more (`overflow`); a request that is that retry (`retrying`)
        // fails the turn if it passes the window too.
        let mut overflow: Option<Overflowing> = None;
        let mut retrying: Option<Overflowing> = None;
        let window = self.catalog.get(&target.model).map(|e| e.context_window);
        let mut routed_to: Option<Arc<dyn Provider>> = None;
        while run_model {
            // A `/stop` that landed (W1): the turn plans nothing more.
            if let Some(by) = stopped_by(&t.tc)? {
                Self::stopped(t, &by);
                break;
            }
            // A task the owner holds parks here (`task.pause`).
            if self.paused_at(t) {
                break;
            }
            // A hands group over the session's budget asks as a model call does.
            if let Some(o) = self.tools.hands_over_budget(t.tc.execution_id) {
                self.ask_budget(t, session, o.needed, o.available, o.spent, o.limit)?;
                break;
            }
            // A job that ended since the last loop: its result, before the
            // next request (theseus-n8gk).
            if t.loops > 0 {
                self.take_late(t)?;
                self.steer_in(t)?;
                if let Some(f) =
                    self.checkpoint_apply(t, session, &mut spec, &mut routed_to, moves)?
                {
                    return Ok(Next::Fail(f));
                }
                self.defer_found(t, &mut spec);
            }
            Self::clock_line(t)?;
            Self::helper_note(t)?;
            let i = t.loops;
            t.loops += 1;
            t.record(&fact::turn::LoopOpened {
                index: i,
                max_loops: t.target.max_loops,
            });
            // Recall asks the index on the first loop (M6): it is read before
            // the compile, under its deadline.
            if i == 0 {
                self.recall_first(t, session).await;
            }
            let compiled = match self
                .compile_routed(
                    t,
                    session,
                    &mut spec,
                    &mut routed_to,
                    routed,
                    std::mem::take(&mut force),
                    strip.take(),
                    overflow.as_ref().map(|o| &o.hint),
                    i,
                )
                .await?
            {
                Ok(c) => c,
                // A request that could not be made smaller than the one the provider refused for its
                // size: the window's failure, in its words (nothing earlier could be dropped).
                Err(f) => match (&overflow, f.class == WINDOW_CLASS) {
                    (Some(o), true) => return Ok(Next::Fail(Self::window_failure(t, o, false))),
                    _ => return Ok(Next::Fail(f)),
                },
            };
            if let Some(o) = overflow.take() {
                // The ring dropped nothing, so the same request would pass the
                // window again: the turn fails without the call. A view's retry
                // is judged on its finished size instead (`view_step::room`): a
                // smaller finish under the same compile is a smaller request.
                if compiled.digest == o.digest && t.view.is_none() {
                    return Ok(Next::Fail(Self::window_failure(t, &o, false)));
                }
                retrying = Some(o);
            }
            // After every await of the compile, before the provider: a view admitted under a reading the
            // gate no longer gives is compiled again (issue 44, `view_step::view_readmitted`).
            let compiled = match self.view_readmitted(t, session, &spec, compiled, i).await? {
                Ok(c) => c,
                Err(f) => return Ok(Next::Fail(f)),
            };
            let said_before = (t.output.len(), t.said.len());
            self.spend_checked(t, session)?;
            let on = routed_to.as_deref().unwrap_or(provider);
            let called = self.call_model(t, on, &compiled, i).await;
            self.self_settled(t, &called);
            let called = called?;
            // A multiple the call's settle crossed, whatever its outcome (theseus-jse4).
            self.spend_checked(t, session)?;
            let (resp, node) = match called {
                Called::Answered(called) => {
                    self.note_sent(t, &spec, &compiled.request.model);
                    // The provider dropped a block whose prefix had changed, and the history
                    // still holds it: the next compile strips the old thinking, so the call
                    // after it is not made twice (`provider::binding::repeat_for`).
                    if called.0.prefix_repeated {
                        force = true;
                    }
                    *called
                }
                Called::Failed(failure) => {
                    if self
                        .retry_after(t, &failure, &mut retried, &mut spec, fallback)
                        .await?
                    {
                        continue;
                    }
                    if self.without_message_effort(
                        t,
                        &mut spec,
                        &compiled,
                        &failure,
                        &mut effort_asked,
                    ) {
                        continue;
                    }
                    if !image_retried {
                        if let Some(edited) = Self::hide_refused(t, session, &compiled, &failure)? {
                            image_retried = true;
                            strip = edited.then_some("image_not_shown");
                            t.record(&fact::turn::LoopCut {
                                decision: "image_not_shown",
                            });
                            continue;
                        }
                    }
                    let refused = failure
                        .source
                        .downcast_ref::<ProviderError>()
                        .and_then(ProviderError::overflow);
                    if let Some(o) = refused {
                        let o = Overflowing::refused(&compiled, o, window);
                        match Self::overflowed(t, o, retrying.take(), i) {
                            Ok(next) => {
                                overflow = Some(next);
                                t.record(&fact::turn::LoopCut {
                                    decision: WINDOW_CLASS,
                                });
                                continue;
                            }
                            Err(f) => return Ok(Next::Fail(f)),
                        }
                    }
                    return Ok(Next::Fail(failure));
                }
                Called::OverBudget {
                    needed,
                    available,
                    spent,
                    limit,
                } => {
                    if t.retry_over_limit {
                        t.record(&fact::turn::LoopCut {
                            decision: "over_limit",
                        });
                        return Ok(Next::Fail(Self::over_limit(t, needed, limit)));
                    }
                    self.ask_budget(t, session, needed, available, spent, limit)?;
                    t.record(&fact::turn::LoopEndedOnBudget {
                        turn_id: t.tc.turn_id,
                        index: i,
                    });
                    t.record(&fact::turn::LoopCut { decision: "budget" });
                    break;
                }
                Called::Braked(b) => {
                    let (spent, line) = (b.spent, b.line);
                    t.braked = Some(b);
                    self.ask_budget(t, session, 0, 0, spent, line)?;
                    t.record(&fact::turn::LoopEndedOnBudget {
                        turn_id: t.tc.turn_id,
                        index: i,
                    });
                    t.record(&fact::turn::LoopCut { decision: "brake" });
                    break;
                }
                Called::Stopped { by } => {
                    t.record(&fact::turn::LoopCut {
                        decision: "stopped",
                    });
                    Self::stopped(t, &by);
                    break;
                }
            };
            Self::fallback_answered(t, &resp);
            self.defer_response(t, &resp);
            let uses = resp.tool_uses();
            // A call that fit: a later one over the limit asks again.
            t.retry_over_limit = false;
            // The model answered: whatever run of failures the session was in
            // has ended (theseus-ljr).
            session.failing = None;
            // A stop that landed while the model answered (W1): its answer is
            // kept, and none of its calls run.
            if let Some(by) = stopped_by(&t.tc)? {
                let tc = TurnCtx {
                    loop_index: Some(i),
                    ..t.tc
                };
                for u in &uses {
                    self.tools.not_run_stopped(&tc, u, &by)?;
                }
                t.record(&fact::turn::LoopCut {
                    decision: "stopped",
                });
                t.last = Some(resp);
                Self::stopped(t, &by);
                break;
            }
            // A task wait's fan-in is bounded by the room this request leaves (`last_request`).
            self.last_request
                .record_wait_room(t.tc.session_id, &uses, &compiled);
            let answered = boxed(|| self.run_tools(t, &resp, &uses, &node, i)).await?;
            // An answer cut at the window (theseus-9p88): none of its calls
            // ran, and the call is made once more on a ring, which leaves the
            // cut answer out; so does the reply. A retry cut too fails the turn.
            let mut window_retry = false;
            let mut window_failed = None;
            if resp.stop_reason.as_deref() == Some(WINDOW_EXCEEDED) {
                let o = Overflowing::cut(&compiled, &resp, &node, window);
                match Self::overflowed(t, o, retrying.take(), i) {
                    Ok(next) => {
                        overflow = Some(next);
                        window_retry = true;
                        t.output.truncate(said_before.0);
                        t.said.truncate(said_before.1);
                    }
                    Err(f) => window_failed = Some(f),
                }
            } else {
                // The retry answered: a later overflow in the turn is its own.
                retrying = None;
            }
            // A refusal goes once to its model's fallback (theseus-7gir.18).
            let fell = self.fall_back(t, &mut spec, fallback, (&resp, &node), said_before);
            // The fallback answered that request: the next goes back to the
            // routed model (theseus-wf482).
            Self::go_back(t, &mut spec, (&resp, &node));
            run_model = self.advance(t, &resp, uses.len(), answered, i, window_retry || fell);
            self.loops_noticed(t, i, run_model)?;
            self.repeat_noticed(t, &uses, run_model)?;
            run_model = self.progress(t, i, run_model)?;
            let ended = checkpoint_step::Ended {
                compiled: &compiled,
                resp: &resp,
                uses: &uses,
                index: i,
                more: run_model,
            };
            self.checkpoint_ended(t, session, ended);
            t.last = Some(resp);
            // A stop that announced more work goes on once (theseus-fh0u).
            run_model = run_model || (window_failed.is_none() && self.stop_nudge(t, i).await?);
            // A stop that would leave command outputs or unrun edits, once (theseus-53ujg.1).
            run_model = run_model || (window_failed.is_none() && self.end_note(t, i)?);
            // A stop whose message names a doubt, early in the turn, once (theseus-rqu7j).
            run_model = run_model || (window_failed.is_none() && self.doubt_note(t, i)?);
            // A stop of a turn that built something in coding or bench work: call it once as the task
            // spells it, once (theseus-eb4o4; at close-out since issue 46).
            run_model = run_model || (window_failed.is_none() && self.deliverable_note(t, i)?);
            if let Some(f) = window_failed {
                return Ok(Next::Fail(f));
            }
        }

        // 4. The caller's: results that settled while this turn ran, the
        // books, the park.
        Ok(Next::Finish)
    }
}

/// A failed turn, with the classification the protocol reports in `error.data`.
/// Its own text names the turn and the class; the cause is its `source`, so
/// `{:#}` says the cause once (theseus-woy: it once said it twice).
#[derive(Debug, thiserror::Error)]
#[error("turn {turn_id} failed ({class})")]
pub struct TurnError {
    pub class: String,
    pub transient: bool,
    pub usage_unknown: bool,
    pub turn_id: String,
    pub session_id: String,
    pub elapsed_ms: u64,
    pub trace: Option<theseus_protocol::Span>,
    /// What the turn's finished loops spent before it failed.
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    pub tool_calls: u32,
    #[source]
    pub source: anyhow::Error,
}

/// Why a task's turn failed, as its report says it (DD7): the class and the
/// cause, clipped.
fn failure_reason(e: &anyhow::Error) -> String {
    let why = match e.downcast_ref::<TurnError>() {
        Some(t) if t.class == FAULT_CLASS => format!("an internal error ({:#})", t.source),
        Some(t) => format!("{} ({:#})", t.class, t.source),
        None => format!("an internal error ({e:#})"),
    };
    crate::toolrun::cap(&why, 400, |_| String::new()).0
}

pub fn add_usage(into: &mut Usage, u: &Usage) {
    into.input_tokens += u.input_tokens;
    into.output_tokens += u.output_tokens;
    into.cache_read_input_tokens += u.cache_read_input_tokens;
    into.cache_creation_input_tokens += u.cache_creation_input_tokens;
    into.cache_creation_1h_input_tokens += u.cache_creation_1h_input_tokens;
}
