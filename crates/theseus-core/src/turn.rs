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
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{
    micros_to_usd, Action, ActionState, Authority, Completion, ExecState, Execution, Kernel,
    KernelError, Micros, Outcome as ActionOutcome, Proposal, RetryClass, TurnEnd, TurnGuard, Wake,
    BUDGET_TOOL, PROVIDER_TOOL,
};
use theseus_protocol::{
    BudgetAsk, CacheSummary, ConfirmRequest, ContextCompiled, SessionKind, Span, TurnSubmitResult,
    Usage,
};
use theseus_store::{frames_written_here, NewRecord};

use crate::advancer::{Advancer, Decision, LoopOutcome, UntilNoToolCalls};
use crate::bus::{EventSink, SessionBus};
use crate::catalog::Catalog;
use crate::compiler::{compile, CompileInput, Compiled, Overflowed, Recompile, RequestSpec};
use crate::config::{CacheTtl, Effort, ThinkingDisplay};
use crate::context_files::{ContextFile, ContextFiles, Unreadable};
use crate::fact::{self, Fact, To};
use crate::narrative::{self, Narrator};
use crate::node::{Body, Node};
use crate::provider::{
    Delta, ModelResponse, Overflow, Provider, ProviderError, ToolUse, WINDOW_EXCEEDED,
};
use crate::secrets::{SecretBoard, Waited};
use crate::session::{title_from, NotShown, SessionRecord, TargetRef, Then};
use crate::startup::StartupLog;
use crate::store::{SessionHold, Store};
use crate::task_graph::tools::Closing;
use crate::toolrun::{Call, CallOutcome, Ran, ToolRuntime, TurnCtx};
use crate::trace::Trace;
use crate::Config;

pub mod compaction;
mod compile_step;
mod inbound_step;
mod prompt_input;
mod recall_step;

/// The persona at the front of every system prompt. Frozen text: it sits at
/// the start of the cached prefix, so it never interpolates anything.
pub const PERSONA: &str = "You are Theseus, a coding and operations agent working for your operator through a harness that records everything you do. Be direct and concise; lead with what you found or did. When you are unsure, say so plainly.";

/// What a turn runs against, resolved from a profile plus any raw overrides.
#[derive(Debug, Clone)]
pub struct Target {
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    pub system: Option<String>,
    /// The persona in play (theseus-c48): its context files follow the
    /// system level's. `[context].default_persona` until Jev chooses one.
    pub persona: Option<String>,
    pub effort: Option<Effort>,
    pub thinking_display: ThinkingDisplay,
    pub max_loops: u32,
    pub refusal_fallbacks: bool,
    /// The profile's prompt-cache TTL (theseus-ev1).
    pub cache_ttl: CacheTtl,
}

impl From<&Target> for TargetRef {
    fn from(t: &Target) -> Self {
        TargetRef {
            profile: t.profile.clone(),
            provider: t.provider.clone(),
            model: t.model.clone(),
        }
    }
}

pub struct TurnRunner {
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
    pub latest_stops: std::sync::Mutex<std::collections::HashMap<String, (Instant, String)>>,
    /// Each place's class (the place rule, theseus-nbsh): the places the
    /// binding binds, as it told the core at its start.
    pub place_rule: crate::places::PlaceRule,
    /// Recall (M6 step 30a): `[memory]`, and the index it asks.
    pub memory: Arc<crate::recall::Memory>,
    /// The memory pass (M6 31a): after each turn, off its path.
    pub pass: Arc<crate::memory_pass::MemoryPass>,
    /// The ontology's snapshot (theseus-8kk.1), built after serving.
    pub ontology: crate::ontology::Board,
    /// Jev's judgments (M5 23a): each turn's end is handed to it, and it
    /// judges in a task of its own; the turn never waits on it.
    pub judge: Arc<crate::judge::JudgeService>,
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
}

/// Why a turn did not run: a secret it needs is not there.
struct SecretRefusal {
    /// `secret_failed` or `secret_resolving`.
    class: &'static str,
    secret: String,
    error: String,
}

/// One turn to run.
pub struct TurnRequest {
    pub session: SessionRecord,
    /// `None`: a continuation.
    pub input: Option<String>,
    pub target: Target,
    pub sink: EventSink,
    /// The client that asked (`web#3`) or `harness` for a continuation.
    pub author: String,
    pub recompile: Option<Recompile>,
    /// Files that came with the input (theseus-9g2); kept on its node.
    pub attachments: Vec<theseus_protocol::Attachment>,
    /// When the request reached the daemon; `None`: now.
    pub arrived: Option<Instant>,
    /// The surface's message it answers (a Discord message id), which the
    /// reply's post names (theseus-q4v).
    pub reply_to: Option<String>,
    /// An MCP server's prompt as the input (M7 36c): its messages are the
    /// input's nodes, and `input` is their text. `None` for any other turn.
    pub prompt: Option<crate::mcp::prompts::PromptInput>,
}

/// How long a turn may wait for admission before the client gets an error.
const ADMISSION_WAIT_MAX: Duration = Duration::from_secs(600);
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
    started: Instant,
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
    /// Jobs this turn started or resumed in the background.
    background: Vec<String>,
    stop_reason: String,
    /// Each loop that said something: its index and its assistant node,
    /// which the reply's post names (theseus-q4v).
    said: Vec<(u32, String)>,
    /// The surface's message the turn answers (a Discord message id).
    reply_to: Option<String>,
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
}

/// The class of a turn that faulted (R1): an error the turn did not report
/// itself, such as a frame the store would not write. It closes its books as
/// a failure does.
pub const FAULT_CLASS: &str = "internal";

/// Where a turn's body left it: to end as it planned, with the recompile it
/// took and did not apply, or to fail as it reports.
enum Next {
    Finish(Option<Recompile>),
    Fail(Failure),
}

/// What a turn's body takes from its request, beside what its `Turn` holds.
struct Asked {
    /// `None`: a continuation.
    input: Option<String>,
    attachments: Vec<theseus_protocol::Attachment>,
    author: String,
    recompile: Option<Recompile>,
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
        let trace = Trace::start_at(
            arrived,
            "turn",
            "turn",
            json!({
                "turn_id": tc.turn_id,
                "session_id": tc.session_id,
                "profile": target.profile,
                "provider": target.provider,
                "model": target.model,
                "continuation": continuation,
                "started_unix_ms": theseus_protocol::now_unix_ms(),
            }),
        );
        Self {
            tc,
            target,
            continuation,
            started,
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
            background: Vec::new(),
            stop_reason: String::new(),
            said: Vec::new(),
            reply_to: None,
            wakes: Vec::new(),
            wake_target: None,
            woke_by: Vec::new(),
            booked: false,
            deferred: false,
            ended: false,
            recall: recall_step::Recalled::default(),
        }
    }

    /// Record that the turn began, after its waits (`fact::turn::TurnStarted`).
    fn announce(&mut self, input: Option<&str>, files: usize, author: &str, waits: &Waits) {
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

impl TurnRunner {
    /// Resolve what a turn runs against. Precedence: raw `provider`/`model`
    /// overrides > the named `profile` > the live profile.
    pub fn resolve_target(
        &self,
        live_profile: &str,
        profile: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<Target> {
        let name = profile.unwrap_or(live_profile);
        let prof = self.cfg.profile(name)?;
        let provider = provider.unwrap_or(&prof.provider).to_string();
        if !self.providers.contains_key(&provider) {
            anyhow::bail!(
                "unknown provider {provider:?}; configured: {}",
                self.providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let model = model.unwrap_or(&prof.model).to_string();
        let max_tokens = prof
            .max_output_tokens
            .or_else(|| self.catalog.get(&model).map(|e| e.max_output_tokens))
            .unwrap_or(16_384);
        Ok(Target {
            profile: name.to_string(),
            provider,
            model,
            max_tokens,
            system: prof.system.clone(),
            persona: self.cfg.persona().map(str::to_string),
            effort: prof.effort,
            thinking_display: prof.thinking_display,
            max_loops: prof.max_loops,
            refusal_fallbacks: prof.refusal_fallbacks,
            cache_ttl: prof.cache_ttl,
        })
    }

    /// What a continuation runs on: the session's last turn's target, which
    /// every turn records from its start (theseus-kol), so a conversation
    /// does not change model under its own thinking blocks, whatever profile
    /// is live. A profile no longer configured gives its provider and model
    /// under the live profile's settings; only a session with neither falls
    /// back to the live profile.
    pub fn target_for_session(&self, s: &SessionRecord, live_profile: &str) -> Result<Target> {
        if let Some(t) = &s.last_target {
            let as_it_ran = self.resolve_target(
                live_profile,
                Some(&t.profile),
                Some(&t.provider),
                Some(&t.model),
            );
            if let Ok(tg) = as_it_ran.or_else(|_| {
                self.resolve_target(live_profile, None, Some(&t.provider), Some(&t.model))
            }) {
                return Ok(tg);
            }
        }
        self.resolve_target(live_profile, None, None, None)
    }

    fn first_party(&self, provider: &str) -> bool {
        self.cfg
            .all_providers()
            .get(provider)
            .map(|p| p.api_base.contains("api.anthropic.com"))
            .unwrap_or(false)
    }

    /// The system prompt, as its two blocks (theseus-ev1). The header, which
    /// every session of the profile and their tasks share: the persona, the
    /// tools paragraph, and the profile's own text. Then the context: each
    /// context file under its header (theseus-58a), the system level's, then
    /// the persona's, each header naming its level (theseus-c48); empty
    /// without files. Deterministic for a config and the files' contents; a
    /// change is a `system_changed` recompile. Nothing retractable belongs in
    /// the header (Appendix F, theseus-3nk): it is every session's prefix.
    pub fn system_blocks(
        &self,
        target: &Target,
        files: &[ContextFile],
        place: crate::ceiling::PlaceView,
    ) -> (String, String) {
        let mut parts = vec![PERSONA.to_string()];
        let note = self.tools.system_note_for(place);
        if !note.is_empty() {
            parts.push(note);
        }
        if let Some(s) = target.system.as_ref().filter(|s| !s.trim().is_empty()) {
            parts.push(s.clone());
        }
        let sections: Vec<&str> = files.iter().map(|f| f.section.as_str()).collect();
        (parts.join("\n\n"), sections.join("\n\n"))
    }

    /// The turn's request spec, fixed for all its loops, and the context
    /// files this daemon run finds unreadable for the first time. A task's
    /// own conversation is cached for 5 minutes whatever the profile says
    /// (theseus-ev1): its loops run seconds apart, so a 1-hour entry would
    /// only add its write premium. Its header keeps the profile's TTL, the
    /// one its parent's requests write.
    pub fn request_spec(
        &self,
        target: &Target,
        kind: SessionKind,
        place: crate::ceiling::PlaceView,
    ) -> (RequestSpec, Vec<Unreadable>) {
        let class = place.class;
        let paths = self.cfg.context_paths(target.persona.as_deref());
        let (mut files, unreadable) = self.context_files.load(&paths);
        // A shared place carries only the files marked public (the place
        // rule): the rest are their headers and why.
        if class == crate::places::PlaceClass::Shared {
            crate::context_files::withhold_shared(&mut files);
        }
        let (system_text, context_text) = self.system_blocks(target, &files, place);
        let spec = RequestSpec {
            profile: target.profile.clone(),
            provider: target.provider.clone(),
            model: target.model.clone(),
            max_tokens: target.max_tokens,
            system_text,
            context_text,
            context_files: files.iter().map(|f| f.file.clone()).collect(),
            persona: target.persona.clone(),
            tools: self.tools.definitions_for(place),
            effort: target.effort,
            thinking_display: target.thinking_display,
            refusal_fallbacks: target.refusal_fallbacks,
            first_party: self.first_party(&target.provider),
            cache_ttl: target.cache_ttl,
            conversation_ttl: match kind {
                SessionKind::Task => CacheTtl::FiveMinutes,
                SessionKind::Conversation => target.cache_ttl,
            },
            walk: None,
            memberships: vec![],
            guidance: vec![],
        };
        (spec, unreadable)
    }

    /// The class of the place a session's turn speaks in (the place rule,
    /// theseus-nbsh): where its words go. That is the session's place (a
    /// task's is its parent's, `outbox.target`), or, for a session no place
    /// runs on any more (`/new`), the place its wakes and reports answer in
    /// (theseus-4lx), the latest its takes kept. With neither, the CLI's or
    /// the web UI's: private. Shared when where it goes cannot be read.
    pub fn class_of(&self, session_id: &str) -> crate::places::PlaceClass {
        self.view_of(session_id).class
    }

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

    /// Take the kernel turn. With `wait` (an input's turn), wait for
    /// admission (ceiling, or another turn on the same execution), waking the
    /// execution in the same frame when it has parked meanwhile
    /// (theseus-l6y); without it (continuations), give up at once and let the
    /// driver try again.
    ///
    /// `frames` counts the frames it writes (theseus-wz4y): each try is one
    /// call, with no `.await` in it, so this thread's count is its own.
    async fn admit(
        &self,
        exec_id: &str,
        arrived: Instant,
        wait: bool,
        frames: &mut u64,
    ) -> Result<Option<TurnGuard>> {
        loop {
            let f0 = frames_written_here();
            let tried = if wait {
                self.kernel.admit_input(exec_id)
            } else {
                self.kernel.admit(exec_id)
            };
            *frames += frames_written_here() - f0;
            match tried {
                Ok(g) => return Ok(Some(g)),
                Err(e) => match e.downcast_ref::<KernelError>() {
                    Some(KernelError::AdmissionFull { .. })
                    | Some(KernelError::TurnHeld { .. })
                    | Some(KernelError::NotRunnable {
                        state: "running", ..
                    }) => {
                        if !wait {
                            return Ok(None);
                        }
                    }
                    Some(KernelError::NotRunnable {
                        state: "waiting" | "blocked",
                        ..
                    }) => {
                        if !wait {
                            return Ok(None);
                        }
                        let f0 = frames_written_here();
                        let woke = self.kernel.wake_input(exec_id);
                        *frames += frames_written_here() - f0;
                        woke?;
                        continue;
                    }
                    Some(KernelError::NotRunnable { state, .. }) => {
                        return Err(turn_error(
                            &format!("execution_{state}"),
                            "",
                            "",
                            arrived.elapsed().as_millis() as u64,
                            e,
                        ));
                    }
                    _ => return Err(e),
                },
            }
            if arrived.elapsed() > ADMISSION_WAIT_MAX {
                anyhow::bail!("admission wait exceeded {:?}", ADMISSION_WAIT_MAX);
            }
            let _ =
                tokio::time::timeout(Duration::from_millis(50), self.admission.notified()).await;
        }
    }

    /// Wait for the secrets a turn needs (theseus-qa0): its provider's key,
    /// and the first round of every secret, since the scrubber must know
    /// each value before any tool output passes through it. Both are
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
    pub fn stop_landed(&self, execution_id: &str, by: &str) {
        self.latest_stops
            .lock()
            .unwrap()
            .insert(execution_id.to_string(), (Instant::now(), by.to_string()));
    }

    /// Who stopped `execution_id` after an input that arrived at `arrived`,
    /// if anyone did.
    fn stopped_since(&self, execution_id: &str, arrived: Instant) -> Option<String> {
        self.latest_stops
            .lock()
            .unwrap()
            .get(execution_id)
            .filter(|(at, _)| *at > arrived)
            .map(|(_, by)| by.clone())
    }

    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub async fn run(&self, mut req: TurnRequest) -> Result<TurnSubmitResult> {
        let arrived = req.arrived.unwrap_or_else(Instant::now);
        let continuation = req.input.is_none();
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
        // An input wakes its execution and takes the turn in one frame
        // (theseus-l6y). When admission must wait, the wake is written alone,
        // and the wait below takes the turn.
        let mut admitted = None;
        if !continuation {
            self.supersede_budget_question(&exec, &req.sink, &req.author);
            match self.kernel.admit_input(&exec.id) {
                Ok(g) => admitted = Some(g),
                Err(e) => match e.downcast_ref::<KernelError>() {
                    Some(KernelError::AdmissionFull { .. } | KernelError::TurnHeld { .. })
                    | Some(KernelError::NotRunnable {
                        state: "running", ..
                    }) => {}
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
        let guard = match admitted {
            Some(g) => g,
            None => match self
                .admit(&exec.id, arrived, !continuation, &mut admission_frames)
                .await?
            {
                Some(g) => g,
                None => {
                    anyhow::bail!("execution {} is not ready for a continuation turn", exec.id)
                }
            },
        };
        let f0 = frames_written_here();
        // A `/stop` that landed after this input arrived and before its turn
        // held the execution (theseus-hmwv) found no turn to mark: it waited
        // on its secrets, or for admission. The operator said "do this",
        // then "stop", so it stops this turn now, as a stop during a turn
        // does: the kernel marks it, and its first step plans nothing.
        if !continuation {
            if let Some(by) = self.stopped_since(&guard.execution_id, arrived) {
                if let Err(e) = self.kernel.stop_execution(&guard.execution_id, &by) {
                    tracing::warn!(execution_id = %guard.execution_id, error = %format!("{e:#}"), "a stop before admission could not mark the turn");
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
        };
        let store = self.store.for_turn();
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
        };
        // A task (DD7): where it reports, and its title, for its report.
        let task_of = req.session.task.clone();
        let task_title = req.session.title.clone();
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
        // Its record closes in that frame too (39a), under its lock.
        let (lock, closing) = Closing::new(&frames.store, task_of.as_ref(), &failure_sink);
        let ended = frames.kernel.end_turn_with(guard, end, |e| {
            let Some(task) = task_of.as_ref().filter(|_| e.state.is_terminal()) else {
                return Ok(vec![]);
            };
            let last = if e.state == ExecState::Complete {
                let nodes = frames.store.transcript(&e.session_id)?;
                crate::task::last_message(nodes.iter().map(|(_, n)| &**n))
            } else {
                None
            };
            let report = crate::task::Report::new(e, task_title.clone(), last);
            let mut records = closing.records(e, report.node.as_deref())?;
            let Some(target) = &task.target else {
                return Ok(records);
            };
            let (post, more) =
                self.outbox
                    .stage(&e.session_id, &e.id, target, report.post_body())?;
            records.extend(more);
            *reported.lock().unwrap() = Some(post);
            Ok(records)
        });
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
                let to = self.session_rec(&failure_sink.session_id, To::Sink(&failure_sink));
                crate::task_graph::tools::announce_closed(&to, closed);
            }
            Err(e) => tracing::warn!(error = %e, "ledger append failed"),
        }
        drop(session_hold);
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
            rec.record(&fact::turn::TurnFailureTold { failed: &failed });
        }
        if rewake && stopped.is_none() {
            // A background result landed while the turn ran; the model has not
            // read it yet, so the driver takes another turn.
            let _ = self.kernel.wake(&exec_id, "late_result");
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
                )
            }
        }
    }

    /// One turn under the lock (§3.3): catch up on what happened while no
    /// turn ran, write the input, run loops while the model has something new
    /// to read, then book the turn and decide where the execution waits.
    async fn run_inner(
        &self,
        guard: &TurnGuard,
        frames: &Frames,
        req: TurnRequest,
        arrived: Instant,
        waits: Waits,
    ) -> Result<(TurnSubmitResult, TurnEnd, bool, Option<SessionHold>)> {
        let TurnRequest {
            mut session,
            input,
            target,
            sink,
            author,
            recompile,
            attachments,
            reply_to,
            prompt,
            ..
        } = req;
        let provider = self
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown provider {:?}", target.provider))?;
        // The caller's copy may be stale: re-read under the lock.
        if let Some(fresh) = self
            .store
            .get_session::<SessionRecord>(&session.session_id)?
        {
            session = fresh;
        }
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
        let tc = TurnCtx {
            kernel: &frames.kernel,
            store: &frames.store,
            guard,
            session_id: &sid,
            execution_id: &guard.execution_id,
            turn_id: &turn_id,
            loop_index: None,
            sink: &sink,
            confirm_ttl_ms: self.kernel.config().confirm_ttl_ms,
            narrator: &self.narrator,
            outbox: &self.outbox,
            posts: &frames.posts,
            target: Some(&target),
            task: task_of.as_ref(),
            // Taken again once the turn has read its wakes and reports.
            class: place.class,
            ceiling: place.ceiling,
        };
        if self.narrator.on() && self.narrator.first_sight(&sid) && session.turns > 0 {
            tc.rec().in_turn(None).record(&fact::turn::SessionResumed {
                session_id: &sid,
                turns: session.turns,
                cost_usd: session.cost_usd,
            });
        }
        let mut t = Turn::start(tc, &target, input.is_none(), arrived);
        t.reply_to = reply_to;
        t.announce(input.as_deref(), attachments.len(), &author, &waits);
        let asked = Asked {
            input,
            attachments,
            author,
            recompile,
            prompt,
            moved: moved.then_some(runs_on),
        };
        // Every exit from here closes the turn's books (R1): a failure the
        // turn reports (`fail`), and any other error, a fault, which `fault`
        // books the same way before it is returned. The body's `?`s all
        // land here, so no step after a paid loop can skip its spend.
        match self
            .turn_body(&mut t, &mut session, provider.as_ref(), asked)
            .await
        {
            Ok(Next::Finish(force)) => self.finish(t, &mut session, force),
            Ok(Next::Fail(f)) => Err(Self::fail(t, &mut session, f)),
            Err(e) => Err(Self::fault(t, &mut session, e)),
        }
    }

    /// The turn's body (§3.3): catch up on what happened while no turn ran,
    /// write the input, and run loops while the model has something new to
    /// read. An error is a fault, which the caller books (R1). The shape
    /// budget's marks came with its code, from `run_inner`.
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn turn_body(
        &self,
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        provider: &dyn Provider,
        asked: Asked,
    ) -> Result<Next> {
        let Asked {
            input,
            attachments,
            author,
            recompile,
            prompt,
            moved,
        } = asked;
        let (sid, turn_id, target) = (t.tc.session_id, t.tc.turn_id, t.target);

        // 1. What happened while no turn was running.
        let caught_up = self.catch_up(t, input.is_some()).await?;
        // Where the turn's words go, now that it has taken its wakes and
        // reports: their place, when the session's own has moved on.
        let here = self.view_of(sid);
        (t.tc.class, t.tc.ceiling) = (here.class, here.ceiling);

        // 2. The new input, with its files in the same node and frame.
        if let Some(p) = prompt {
            self.write_prompt_input(t, session, p, moved.as_ref())?;
        } else if let Some(text) = &input {
            let files = crate::attach::from_wire(
                attachments,
                self.cfg.tools.max_read_bytes,
                self.store.blobs(),
            );
            let first_file = files.first().map(|a| a.name.clone());
            let node = Node::user_with(sid, Some(turn_id), &author, text, files);
            if session.title.is_none() {
                let title = title_from(text);
                session.title = Some(match first_file {
                    Some(name) if title.is_empty() => title_from(&name),
                    _ => title,
                });
            }
            // A new target rides in the input's frame, under the record's
            // lock; the same one writes nothing more.
            let written = match &moved {
                Some(runs_on) => t.tc.store.update_session(sid, |r| {
                    r.last_target = Some(runs_on.clone());
                    Ok(vec![node.record()?])
                })?,
                None => None,
            };
            if written.is_none() {
                t.tc.store.append(&[node.record()?])?;
            }
            t.tc.node_written(&node);
            self.inbound_point(t, &node, &author);
        }

        // 3. The loops, while the model has something new to read.
        let mut run_model = Self::has_news(t, input.is_some() || caught_up > 0)?;
        // The spec is fixed for the turn: a context file edited during it
        // recompiles the next turn, never between a tool call and its result.
        // So is the place's class (the place rule): a class that changes
        // mid-turn applies at the next turn's first loop.
        let (mut spec, unreadable) = self.request_spec(target, session.kind, t.tc.place());
        spec.walk = self.walk(sid, t.tc.class);
        for u in &unreadable {
            tracing::warn!(path = %u.path, error = %u.error, session_id = %sid,
                "context file unreadable: the system block says it is missing (warned once per daemon run)");
            t.record(&fact::turn::ContextFileMissing {
                path: &u.path,
                error: &u.error,
                profile: &target.profile,
            });
        }
        // A recompile the operator asked for is taken from the stored record
        // under its lock, so one asked while this turn runs stays for the
        // next turn instead of being written over (theseus-xeo). A turn with
        // none pending writes nothing for it.
        let asked = if session.pending_recompile.is_some() {
            let mut taken = None;
            t.tc.store.update_session(sid, |r| {
                taken = r.pending_recompile.take();
                Ok(vec![])
            })?;
            session.pending_recompile = None;
            taken
        } else {
            None
        };
        let mut force = recompile.or(asked);
        // A provider's 400 that names an image hides it, and the call is made
        // again with its line, once a turn (theseus-0s4); `strip` asks the
        // next compilation to drop the thinking the change sat under.
        let mut image_retried = false;
        let mut strip: Option<&'static str> = None;
        // A request the provider said passed the window (theseus-9p88): the
        // next loop recompiles with a ring by the provider's numbers and calls
        // once more (`overflow`); a request that is that retry (`retrying`)
        // fails the turn if it passes the window too.
        let mut overflow: Option<Overflowing> = None;
        let mut retrying: Option<Overflowing> = None;
        let window = self.catalog.get(&target.model).map(|e| e.context_window);
        while run_model {
            // A `/stop` that landed (W1): the turn plans nothing more.
            if let Some(by) = stopped_by(&t.tc)? {
                Self::stopped(t, &by);
                break;
            }
            // A hands group over the session's budget asks as a model call does.
            if let Some(o) = self.tools.hands_over_budget(t.tc.execution_id) {
                self.ask_budget(t, session, o.needed, o.available, o.spent, o.limit)?;
                break;
            }
            let i = t.loops;
            t.loops += 1;
            t.record(&fact::turn::LoopOpened {
                index: i,
                max_loops: t.target.max_loops,
            });
            // Recall asks the index on the first loop (M6): in front of the
            // model (canary, live) it is read before the compile, under its
            // deadline; in shadow, once the call has answered.
            let recall = match i {
                0 => self.recall_first(t, session).await,
                _ => None,
            };
            let compiled = match self
                .compile_step(
                    t,
                    session,
                    &spec,
                    force.take(),
                    strip.take(),
                    overflow.as_ref().map(|o| &o.hint),
                    i,
                )
                .await?
            {
                Ok(c) => c,
                Err(f) => return Ok(Next::Fail(f)),
            };
            if let Some(o) = overflow.take() {
                // The ring dropped nothing, so the same request would pass the
                // window again: the turn fails without the call.
                if compiled.digest == o.digest {
                    return Ok(Next::Fail(Self::window_failure(t, &o, false)));
                }
                retrying = Some(o);
            }
            let said_before = (t.output.len(), t.said.len());
            let called = self.call_model(t, provider, &compiled, i).await;
            if let Some(r) = recall {
                self.recall_end(t, r).await;
            }
            let (resp, node) = match called? {
                Called::Answered(called) => *called,
                Called::Failed(failure) => {
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
                    t.record(&fact::turn::LoopCut { decision: "budget" });
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
            let answered = self.run_tools(t, &resp, &uses, &node, i).await?;
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
            run_model = Self::advance(t, &resp, uses.len(), answered, i, window_retry);
            t.last = Some(resp);
            if let Some(f) = window_failed {
                return Ok(Next::Fail(f));
            }
        }

        // 4. The caller's: results that settled while this turn ran, the
        // books, the park. A recompile this turn took and never applied (the
        // model was not called) goes back for the next.
        Ok(Next::Finish(force))
    }

    /// What happened while no turn was running: results that settled become
    /// late result nodes, and pending calls resume (a confirm answered, a
    /// restart survived). Returns how many nodes that wrote.
    async fn catch_up(&self, t: &mut Turn<'_>, has_input: bool) -> Result<u32> {
        let t0 = t.trace.now_us();
        let (settled, absorbed) = self.tools.absorb(&t.tc)?;
        // A budget question the kernel queued as a result: approved (the
        // spend was reset), or withdrawn by a raised limit (theseus-3pj).
        // Either way the call that did not fit proceeds.
        let budget = |approved: bool| {
            settled
                .iter()
                .any(|a| a.tool == BUDGET_TOOL && (a.state == ActionState::Succeeded) == approved)
        };
        let (reset, raised) = (budget(true), budget(false));
        // An approved reset of a call that no reset can fit: it alone needs
        // more than the limit (theseus-kks), or more than the limit leaves
        // once what a reset keeps held is taken (theseus-6g6). The retry gets
        // one try, and asks nothing more.
        t.retry_over_limit = settled.iter().any(|a| {
            a.tool == BUDGET_TOOL
                && a.state == ActionState::Succeeded
                && a.proposal.as_ref().is_some_and(|p| {
                    let needed = p.args["needed_micros"].as_u64().unwrap_or(0);
                    t.tc.kernel
                        .execution(t.tc.execution_id)
                        .ok()
                        .flatten()
                        .is_some_and(|e| needed > e.budget.available_after_reset())
                })
        });
        let resumed = self.tools.resume(&t.tc, has_input).await?;
        // The reports of this session's tasks that ended since its last turn
        // (DD7), after the calls above and before the new input.
        let reported = self.read_reports(t)?;
        // Its wakes that are due (DD8), last: each is this turn's input, or
        // comes before the input that arrived with it.
        let woke = Self::read_wakes(t)?;
        t.record(&fact::turn::CaughtUp {
            t0,
            settled: settled.len(),
            absorbed,
            resumed: &resumed,
            reset,
            raised,
            reported,
            woke,
        });
        t.awaiting = resumed.awaiting;
        t.background = resumed.background;
        Ok(absorbed + resumed.wrote + u32::from(reset || raised) + reported + woke)
    }

    /// A `/stop` by `by` ended the turn's loops (W1): its stop reason says so,
    /// and it posts no reply. Its end parks the execution on input.
    fn stopped(t: &mut Turn<'_>, by: &str) {
        t.stop_reason = "stopped".into();
        t.record(&fact::turn::Stopped { by });
    }

    /// The session's wakes that are due (DD8): each becomes a node the model
    /// reads as the user's, `⏰ wake (set 13:05): <note>`, written in the one
    /// frame that removes them from the execution, so none runs twice. None
    /// due, and nothing is written. The reply names them, and goes where the
    /// first of them was set from if the session posts nowhere now.
    fn read_wakes(t: &mut Turn<'_>) -> Result<u32> {
        let tc = &t.tc;
        let mut nodes = Vec::new();
        let fired = tc.kernel.take_wakes(tc.guard, |due| {
            let mut records = Vec::new();
            for f in due {
                let n = Node::relayed(
                    tc.session_id,
                    Some(tc.turn_id),
                    crate::node::Origin::Harness,
                    &format!("wake:{}", crate::task::short(&f.wake.id)),
                    &crate::wake::fired_text(f),
                );
                records.push(n.record()?);
                nodes.push(n);
            }
            // Where its reply goes if the session posts nowhere by then,
            // kept past this turn, so a retry of it answers there too
            // (theseus-4lx).
            if let Some(target) = due.iter().find_map(|f| f.wake.target.as_deref()) {
                records.push(tc.outbox.wake_target_record(tc.session_id, target)?);
            }
            Ok(records)
        })?;
        for n in &nodes {
            tc.node_written(n);
        }
        for f in &fired {
            tc.record(&fact::turn::WakeCameDue { fired: f });
        }
        t.wake_target = fired.iter().find_map(|f| f.wake.target.clone());
        t.wakes = fired
            .iter()
            .map(|f| json!({"wake_id": f.wake.id, "text": crate::wake::fired_text(f), "late_ms": f.late_ms}))
            .collect();
        Ok(fired.len() as u32)
    }

    /// The reports of the tasks this session started that ended since its
    /// last turn (DD7): each becomes a node in this session, read by the
    /// model with whatever came with this turn. One frame clears them from
    /// the execution and writes their nodes; none, and nothing is written.
    /// Those that asked for this turn (W1, `wake_parent`) are named on its
    /// reply's post, and a reply whose session posts nowhere now goes where
    /// the first of them reported.
    fn read_reports(&self, t: &mut Turn<'_>) -> Result<u32> {
        let tc = &t.tc;
        let mut nodes = Vec::new();
        let mut reports = Vec::new();
        let mut held = None;
        // A report from a task that holds external text brings the hold to
        // this session, in the frame that writes the report, under the
        // session record's lock (theseus-9bp): the report may carry what the
        // task read.
        let mut take = |rec: Option<SessionRecord>| {
            tc.kernel.take_reports(tc.guard, |ids| {
                let mut records = Vec::new();
                let mut rec = rec;
                for id in ids {
                    let Some(r) = crate::task::load_report(&self.store, &self.kernel, id)? else {
                        continue;
                    };
                    let n = Node::relayed(
                        tc.session_id,
                        Some(tc.turn_id),
                        crate::node::Origin::Harness,
                        &format!("task:{}", r.short),
                        &r.node_text(),
                    );
                    records.push(n.record()?);
                    // The first transmission edge (12a, theseus-n4m): the
                    // relayed node copies the task's last message, so
                    // `node.reach` follows it from there. A task that did
                    // not finish relays no message, and has no edge.
                    if let Some(last) = &r.node {
                        records.push(
                            crate::graph::Edge::new(
                                crate::graph::EdgeKind::DerivedFrom,
                                &n.id,
                                last,
                                crate::graph::VIA_REPORT,
                            )
                            .record()?,
                        );
                    }
                    if let Some(h) = &r.external {
                        if let Some(before) = rec.take() {
                            let h = crate::external::taken(
                                h,
                                &r.task,
                                crate::external::VIA_REPORT,
                                &n.id,
                                theseus_protocol::now_unix_ms(),
                            );
                            if let Some(more) =
                                crate::external::hold(before, h.clone(), Some(tc.turn_id))?
                            {
                                records.extend(more);
                                held = Some(h);
                            }
                        }
                    }
                    nodes.push(n);
                    reports.push(r);
                }
                // Where the reply to them goes if the session posts nowhere
                // by then, kept past this turn (theseus-4lx).
                if let Some(target) = reports.iter().find_map(|r| r.target.as_deref()) {
                    records.push(tc.outbox.wake_target_record(tc.session_id, target)?);
                }
                Ok(records)
            })
        };
        let taken = match tc
            .store
            .with_session(tc.session_id, |rec| take(Some(rec)))?
        {
            Some(taken) => taken,
            None => take(None)?,
        };
        for n in &nodes {
            tc.node_written(n);
        }
        if let Some(h) = &held {
            tc.record(&fact::tool::HoldTaken {
                hold: h,
                mode: self.tools.external_text,
            });
        }
        for r in reports
            .iter()
            .filter(|r| taken.woke.contains(&r.execution_id))
        {
            tc.record(&fact::turn::ReportWoke { report: r });
            t.woke_by.push(json!({"task": r.task, "short": r.short,
                "text": crate::task::woke_line(&r.short), "outcome": r.outcome}));
            if t.wake_target.is_none() {
                t.wake_target = r.target.clone();
            }
        }
        Ok(nodes.len() as u32)
    }

    /// Does the model have anything new to read: input or results it has not
    /// seen, or a user message still waiting for a reply? If not, the turn's
    /// stop reason says why. Only a continuation that wrote nothing asks the
    /// transcript, and what it finds after the model's last answer is unread
    /// by definition.
    fn has_news(t: &mut Turn<'_>, wrote: bool) -> Result<bool> {
        if t.awaiting.is_some() {
            t.stop_reason = "awaiting_confirm".into();
            t.record(&fact::turn::NoCall::AwaitingApproval);
            return Ok(false);
        }
        if wrote {
            return Ok(true);
        }
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let last = nodes
            .iter()
            .rev()
            .find(|(_, n)| !matches!(n.body, Body::ToolCall { .. }));
        // Results the model has not read are its next input (theseus-kol). A
        // continuation that finds them was woken to have them read: the turn
        // that wrote them failed or faulted before its model call returned
        // (its retry), a late result landed as it ended, or a crash cut it.
        // Ending `nothing_new` here once left a job's result unanswered for
        // good. A turn that stopped short of them by its own decision (its
        // loop cap, a `/stop`) parks on input, and nothing wakes it without
        // writing something new. A task goes on by itself as it always did
        // (DD7).
        let awaiting_reply = matches!(
            last.map(|(_, n)| &n.body),
            // A task's arrangement follows its brief (M5 27).
            Some(Body::UserMessage { .. } | Body::ToolResult { .. } | Body::Arrangement { .. })
        );
        if !awaiting_reply {
            t.stop_reason = "nothing_new".into();
            t.record(&fact::turn::NoCall::NothingNew);
        }
        Ok(awaiting_reply)
    }

    /// The provider call, as a kernel action (§3.16): planned (with its
    /// budget reservation), dispatched, streamed, and settled. The outer
    /// error is a fault; the rest the turn reports or parks on.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn call_model(
        &self,
        t: &mut Turn<'_>,
        provider: &dyn Provider,
        compiled: &Compiled,
        i: u32,
    ) -> Result<Called> {
        let target = t.target;
        let proposal = Proposal {
            tool: PROVIDER_TOOL.into(),
            args: json!({"provider": target.provider, "model": target.model, "max_tokens": target.max_tokens, "loop": i, "turn_id": t.tc.turn_id, "digest": compiled.digest}),
            resource: Some(target.provider.clone()),
            policy_context: json!({"profile": target.profile}),
        };
        // Budgets are dollars (theseus-0sg), so nothing runs unpriced.
        let Some(price) = self.catalog.get(&target.model) else {
            t.record(&fact::turn::ModelUnpriced {
                model: &target.model,
            });
            return Ok(Called::Failed(Failure {
                class: "unpriced".into(),
                transient: false,
                usage_unknown: false,
                reason: format!("unpriced: {}", target.model),
                source: anyhow::anyhow!(
                    "{} has no price: add a [catalog.\"{}\"] table with its provider, \
                     context_window, max_output_tokens, and four prices",
                    target.model,
                    target.model
                ),
            }));
        };
        // The output cap at the output price, the input estimate at the input price.
        let reserve = price.reserve_micros(target.max_tokens, compiled.est_tokens);
        let o0 = t.trace.now_us();
        // It never needs a confirm, so its plan, authorization, and dispatch
        // are one frame, with the loop's rows in front (theseus-qa0), and
        // a recall's node and edges (M6 30b).
        let rides = std::mem::take(&mut t.recall.rides);
        t.recall.pending = None;
        let action = match t.tc.kernel.plan_and_dispatch(
            t.tc.guard,
            &proposal,
            RetryClass::SafeToRepeat,
            Some(self.cfg.model.timeouts.total_secs * 1000),
            reserve,
            |_| Ok(rides),
        ) {
            Ok(a) => a,
            Err(e) => {
                if let Some(KernelError::OverBudget {
                    needed,
                    available,
                    spent,
                    limit,
                }) = e.downcast_ref::<KernelError>()
                {
                    return Ok(Called::OverBudget {
                        needed: *needed,
                        available: *available,
                        spent: *spent,
                        limit: *limit,
                    });
                }
                if let Some(KernelError::Stopped { by, .. }) = e.downcast_ref::<KernelError>() {
                    return Ok(Called::Stopped { by: by.clone() });
                }
                t.record(&fact::turn::ModelNotPlanned {
                    model: &target.model,
                    error: &e,
                });
                return Ok(Called::Failed(Failure {
                    class: "kernel".into(),
                    transient: false,
                    usage_unknown: false,
                    reason: format!("kernel: {e}"),
                    source: e,
                }));
            }
        };
        let started_ms = theseus_protocol::now_unix_ms();

        let (sink, tid) = (t.tc.sink.clone(), t.tc.turn_id.to_string());
        // The characters streamed so far, thinking included (it is billed as
        // output): what a call a stop cuts is estimated from (theseus-yey).
        let streamed = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counted = streamed.clone();
        let mut on_delta = move |d: Delta<'_>| match d {
            Delta::Text(text) => {
                counted.fetch_add(text.chars().count() as u64, Ordering::Relaxed);
                To::Sink(&sink).tell(&fact::turn::ModelDelta {
                    turn_id: &tid,
                    loop_index: i,
                    text,
                })
            }
            Delta::Thinking(text) => {
                counted.fetch_add(text.chars().count() as u64, Ordering::Relaxed);
                To::Sink(&sink).tell(&fact::turn::ModelThinking {
                    turn_id: &tid,
                    loop_index: i,
                    text,
                })
            }
            Delta::ToolUseStart { .. } => {}
        };
        let call_started = Instant::now();
        t.record(&fact::turn::ModelCalling {
            target,
            action: &action,
            reserve,
            output_micros: price.reserve_micros(target.max_tokens, 0),
            input_micros: price.reserve_micros(0, compiled.est_tokens),
            est_tokens: compiled.est_tokens,
            digest: &compiled.digest,
            o0,
        });
        let call_t0 = t.trace.now_us();
        // A `/stop` that lands while the call is in flight cuts its stream
        // (theseus-yey): the stop's signal wakes this, and the kernel's record
        // of it decides, so a signal that is not for this turn changes
        // nothing. A stop recorded before the call began, or in the moment
        // between its plan and here, cuts it before anything was sent.
        let armed = self.stops.arm(t.tc.execution_id);
        let outcome = {
            let mut stream =
                std::pin::pin!(provider.stream_message(&compiled.request, &mut on_delta));
            let mut sent = false;
            loop {
                let woken = armed.notify.notified();
                tokio::pin!(woken);
                woken.as_mut().enable();
                if let Some(by) = stopped_by(&t.tc)? {
                    break Err(Cut {
                        by,
                        sent,
                        out_chars: streamed.load(Ordering::Relaxed),
                        est_input: compiled.est_tokens,
                    });
                }
                tokio::select! {
                    r = &mut stream => break Ok(r),
                    _ = &mut woken => {}
                }
                sent = true;
            }
        };
        drop(armed);
        match outcome {
            Err(cut) => Ok(Self::settle_cut(
                t,
                &action,
                price,
                cut,
                started_ms,
                call_started,
                i,
            )),
            Ok(Ok(resp)) => {
                t.record(&fact::turn::ModelAnswered {
                    resp: &resp,
                    call_t0,
                });
                let node = self.settle_call(t, &action, compiled, &resp, started_ms, i)?;
                Ok(Called::Answered(Box::new((resp, node))))
            }
            Ok(Err(e)) => Ok(Called::Failed(Self::settle_failed(
                t,
                &action,
                started_ms,
                call_started,
                e,
                i,
            ))),
        }
    }

    /// The loop's call did not fit under the spend limit: ask the operator
    /// whether the spend may go back to $0 (theseus-0sg). The question goes
    /// where a tool approval goes (a `confirm.requested` to the session's
    /// clients: its Discord place, the web UI, `theseus confirm`), and the
    /// turn ends parked on it; the execution waits with the reason `budget`.
    fn ask_budget(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        needed: Micros,
        available: Micros,
        spent: Micros,
        limit: Micros,
    ) -> Result<()> {
        let call = json!({"profile": t.target.profile, "model": t.target.model, "max_output_tokens": t.target.max_tokens});
        let q =
            t.tc.kernel
                .ask_budget_for(t.tc.guard, needed, call.clone())?;
        // Its card rides in the turn's next frame, the one that parks it on
        // the question (theseus-q4v).
        if let Some(target) = self.outbox.target(t.tc.session_id) {
            let (post, records) = self.outbox.stage(
                t.tc.session_id,
                t.tc.execution_id,
                &target,
                json!({"kind": "card", "question": q.correlation_id}),
            )?;
            for r in records {
                t.tc.store.defer(r)?;
            }
            t.tc.posts.lock().unwrap().push(post);
        }
        let lifetime = session.cost_usd + t.cost.unwrap_or(0.0);
        let who = match session.task {
            Some(_) => format!("Task {}", crate::task::short(t.tc.session_id)),
            None => "This session".to_string(),
        };
        let kept = Kept::of(t.tc.kernel, t.tc.execution_id);
        let question = budget_question(&who, spent, limit, needed, kept, &call);
        let now = theseus_protocol::now_unix_ms();
        let req = ConfirmRequest {
            correlation_id: q.correlation_id.clone(),
            session_id: t.tc.session_id.into(),
            execution_id: t.tc.execution_id.into(),
            tool: BUDGET_TOOL.into(),
            input: json!({"spent_usd": micros_to_usd(spent), "limit_usd": micros_to_usd(limit), "needed_usd": micros_to_usd(needed), "available_usd": micros_to_usd(available), "model": t.target.model}),
            resource: None,
            reason: question,
            by: OPERATOR.into(),
            requested_at_ms: now,
            expires_at_ms: 0,
            floor: false,
            budget: Some(BudgetAsk {
                spent_usd: micros_to_usd(spent),
                limit_usd: micros_to_usd(limit),
                needed_usd: micros_to_usd(needed),
                lifetime_usd: lifetime,
            }),
            task: crate::task::task_ref(session),
            external_text: None,
        };
        t.record(&fact::turn::BudgetAsked {
            request: &req,
            session_id: t.tc.session_id,
            model: &t.target.model,
            limit,
            spent,
            needed,
            available,
        });
        t.record(&fact::turn::LoopEndedOnBudget {
            turn_id: t.tc.turn_id,
            index: t.loops.saturating_sub(1),
        });
        t.budget_question = Some(q.correlation_id);
        t.stop_reason = "budget".into();
        Ok(())
    }

    /// The operator approved a reset for a call that alone needed more than
    /// the whole limit, and the retry's call still does (theseus-kks): no
    /// reset can make it fit, so the turn fails instead of asking the same
    /// question again. Its failed notice names the figures and the remedies,
    /// and the execution waits on input (a task ends and reports it).
    fn over_limit(t: &mut Turn<'_>, needed: Micros, limit: Micros) -> Failure {
        let kept = Kept::of(t.tc.kernel, t.tc.execution_id);
        let lower = lower_cap(&t.target.profile, u64::from(t.target.max_tokens));
        let msg = if needed > limit {
            format!(
                "the call to {} alone reserves {}, more than the whole {} spend limit, so a \
                 reset cannot make it fit and the turn ends here. Raise `[kernel] \
                 spend_limit_usd` above {}, or {lower}, then send a message to try again",
                t.target.model,
                narrative::dollars(needed),
                narrative::dollars(limit),
                narrative::dollars(needed),
            )
        } else if kept.total() > 0 {
            // What a reset leaves held (theseus-6g6): the call fits the
            // limit, and not what the held amounts leave of it.
            format!(
                "the call to {} reserves {}, and a reset cannot make it fit: of the whole {} \
                 spend limit, {}, and a reset leaves it held, so the turn ends here. Raise \
                 `[kernel] spend_limit_usd` above {}, or {lower}, then send a message to try \
                 again",
                t.target.model,
                narrative::dollars(needed),
                narrative::dollars(limit),
                kept.clause(),
                narrative::dollars(needed + kept.total()),
            )
        } else {
            format!(
                "the call to {} reserves {}, and it still does not fit under the {} spend limit \
                 after the reset, so the turn ends here. Raise `[kernel] spend_limit_usd`, or \
                 {lower}, then send a message to try again",
                t.target.model,
                narrative::dollars(needed),
                narrative::dollars(limit),
            )
        };
        t.record(&fact::turn::OverLimit {
            message: &msg,
            target: t.target,
            needed,
            limit,
            held: kept.total(),
        });
        Failure {
            class: "over_limit".into(),
            transient: false,
            usage_unknown: false,
            reason: "over_limit".into(),
            source: anyhow::anyhow!("{msg}"),
        }
    }

    /// Settle a call that answered: the assistant node rides in the frame of
    /// the call's completion, and the call's usage, cost, and text join the
    /// turn's books.
    fn settle_call(
        &self,
        t: &mut Turn<'_>,
        action: &Action,
        compiled: &Compiled,
        resp: &ModelResponse,
        started_ms: u64,
        i: u32,
    ) -> Result<Node> {
        let target = t.target;
        let call_cost = self
            .catalog
            .cost_usd(&resp.model, &resp.usage)
            .or_else(|| self.catalog.cost_usd(&target.model, &resp.usage));
        t.cost = match (t.cost, call_cost) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        let node = Node::assistant(
            t.tc.session_id,
            t.tc.turn_id,
            i,
            Body::AssistantMessage {
                blocks: resp.content.clone(),
                model: resp.model.clone(),
                provider: target.provider.clone(),
                stop_reason: resp.stop_reason.clone(),
                usage: resp.usage.clone(),
                cost_usd: call_cost,
                catalog_version: Some(self.catalog.version.clone()),
                request_id: resp.request_id.clone(),
                correlation_id: Some(action.correlation_id.clone()),
                compilation_id: Some(compiled.compilation.id.clone()),
                request_digest: Some(compiled.digest.clone()),
            },
        );
        // The budget settles at the real cost, each token class at its own
        // price; a served model the catalog lacks is priced as the target.
        let cost = self
            .catalog
            .get(&resp.model)
            .or_else(|| self.catalog.get(&target.model))
            .map_or(action.reserved_micros, |e| e.cost_micros(&resp.usage));
        // The call's usage joins the turn's books with its cost, before its
        // frame is written: a turn that faults on that frame still books
        // what the provider charged (R1).
        add_usage(&mut t.usage, &resp.usage);
        let call = fact::turn::ProviderCall {
            loop_index: i,
            provider: &target.provider,
            resp,
            cost_usd: call_cost,
            catalog_version: &self.catalog.version,
            node_id: &node.id,
            correlation_id: &action.correlation_id,
            settled_micros: cost,
            s0: t.trace.now_us(),
        };
        // The call's `provider.call` row rides in its completion's frame, after
        // the node (theseus-qa0).
        let row = t.tc.rec().row(&call)?;
        let completion = Completion {
            correlation_id: action.correlation_id.clone(),
            outcome: ActionOutcome::Succeeded,
            result_ref: Some(node.id.clone()),
            external_op_id: resp.request_id.clone(),
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", target.provider),
            signature: None,
            cost_micros: Some(cost),
            detail: Some(json!({"served_model": resp.model, "message_id": resp.message_id})),
        };
        if let Err(e) =
            t.tc.kernel
                .accept_completion_with(&completion, vec![node.record()?, row])
        {
            // The answer's frame was not written, though the call ended and
            // its price is known: it settles alone, at that price, so the
            // budget holds no reservation for it, and the turn faults with
            // it booked (R1). If that frame fails too, the reconcile settles
            // the call as unknown at its deadline.
            let alone = Completion {
                result_ref: None,
                detail: Some(
                    json!({"served_model": resp.model, "message_id": resp.message_id,
                                    "answer": "not written: its frame failed"}),
                ),
                ..completion
            };
            if let Err(s) = t.tc.kernel.accept_completion(&alone) {
                tracing::warn!(correlation_id = %action.correlation_id, error = %format!("{s:#}"),
                    "a provider call's spend was not settled; the reconcile settles it at its deadline");
            }
            return Err(e);
        }
        t.tc.node_written(&node);
        if !resp.text.is_empty() {
            if !t.output.is_empty() {
                t.output.push_str("\n\n");
            }
            t.output.push_str(&resp.text);
            t.said.push((i, node.id.clone()));
        }
        t.announce_fact(&call);
        if resp.stop_reason.as_deref() == Some("refusal") {
            t.record(&fact::turn::ProviderRefused { resp });
        }
        Ok(node)
    }

    /// A provider's 400 that names an image (theseus-0s4): mark each image it
    /// names not shown, in the session record at once and in the turn's copy,
    /// with an `image.not_shown` row each, so that this turn's next call and
    /// every later request render its line. None when it names no image the
    /// session still shows. `Some(true)` when a copy of one sat before an
    /// answer of the model's in the refused request, whose thinking the next
    /// compilation strips.
    fn hide_refused(
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        compiled: &Compiled,
        f: &Failure,
    ) -> Result<Option<bool>> {
        let Some(ProviderError::InvalidRequest {
            status: 400,
            message,
        }) = f.source.downcast_ref::<ProviderError>()
        else {
            return Ok(None);
        };
        let messages = &compiled.request.messages;
        let refused: Vec<crate::attach::Refused> = crate::attach::refused(messages, message)
            .into_iter()
            .filter(|r| !session.not_shown.iter().any(|n| n.digest == r.digest))
            .collect();
        let Some(first) = refused.first() else {
            return Ok(None);
        };
        let at_ms = theseus_protocol::now_unix_ms();
        let marks: Vec<NotShown> = refused
            .iter()
            .map(|r| NotShown {
                digest: r.digest.clone(),
                why: r.why.clone(),
                at_ms,
            })
            .collect();
        for r in &refused {
            t.record(&fact::turn::ImageNotShown {
                refused: r,
                target: t.target,
            });
        }
        // Written now, with its rows: a restart before the turn ends keeps it.
        t.tc.store.update_session(t.tc.session_id, |rec| {
            for m in &marks {
                if !rec.not_shown.iter().any(|n| n.digest == m.digest) {
                    rec.not_shown.push(m.clone());
                }
            }
            Ok(vec![])
        })?;
        t.record(&fact::turn::ImagesHidden {
            model: &t.target.model,
            count: refused.len(),
            why: &first.why,
        });
        // Every copy of a hidden image renders as its line, not only the one
        // the 400 named. If any copy sat before an answer of the model's, the
        // history under that answer's thinking changed: the API binds a
        // thinking block to every message before it, so the block must go.
        let edited = messages
            .iter()
            .rposition(|m| m["role"] == "assistant")
            .is_some_and(|last| {
                messages[..last].iter().any(|m| {
                    crate::attach::digests_in(m)
                        .iter()
                        .any(|d| refused.iter().any(|r| &r.digest == d))
                })
            });
        session.not_shown.extend(marks);
        Ok(Some(edited))
    }

    /// The provider said this loop's request passed the window
    /// (theseus-9p88): a `context.overflow` row, and one line. The first
    /// time, the next loop recompiles with a ring by the provider's numbers
    /// and calls once more. A request that was that retry (`retried`) fails
    /// the turn instead: the turn never rings twice for one call.
    fn overflowed(
        t: &mut Turn<'_>,
        o: Overflowing,
        retried: Option<Overflowing>,
        i: u32,
    ) -> Result<Overflowing, Failure> {
        t.record(&fact::turn::ContextOverflow {
            loop_index: i,
            overflow: &o,
            retried: retried.is_some(),
            model: &t.target.model,
        });
        if retried.is_some() {
            return Err(Self::window_failure(t, &o, true));
        }
        Ok(o)
    }

    /// A request past the window that the turn's ring cannot fit
    /// (theseus-9p88): the retry passed it too (`retried`), or the ring could
    /// drop nothing, so the same request would. The error names the window
    /// and the estimate, and says what the operator can do.
    fn window_failure(t: &mut Turn<'_>, o: &Overflowing, retried: bool) -> Failure {
        let message = format!(
            "{}'s request is past its window{}: {}; Theseus estimated {} tokens against {}. The \
             next message lets the ring drop this exchange, or start a new session.",
            t.target.model,
            if retried {
                " again, after a ring dropped earlier turns"
            } else {
                ", and nothing earlier in the session can be dropped to fit it"
            },
            o.said(),
            narrative::thousands(o.hint.estimated),
            o.window_words()
        );
        t.record(&fact::turn::WindowFailed { retried });
        Failure {
            class: WINDOW_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{WINDOW_CLASS}: {message}"),
            source: anyhow::anyhow!(message),
        }
    }

    /// Settle a call that failed: as failed, or as unknown when the provider
    /// may have done the work, and tell the ledger why.
    fn settle_failed(
        t: &mut Turn<'_>,
        action: &Action,
        started_ms: u64,
        call_started: Instant,
        e: anyhow::Error,
        i: u32,
    ) -> Failure {
        let target = t.target;
        let pe = e.downcast_ref::<ProviderError>();
        let (class, transient, unknown) = pe
            .map(|p| (p.class(), p.is_transient(), p.usage_unknown()))
            .unwrap_or(("unknown", false, true));
        t.record(&fact::turn::ModelCallFailed {
            class,
            message: e.to_string(),
        });
        let s0 = t.trace.now_us();
        let settled = t.tc.kernel.accept_completion(&Completion {
            correlation_id: action.correlation_id.clone(),
            outcome: if unknown {
                ActionOutcome::Unknown
            } else {
                ActionOutcome::Failed
            },
            result_ref: None,
            external_op_id: None,
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", target.provider),
            signature: None,
            cost_micros: if unknown { None } else { Some(0) },
            detail: Some(json!({"class": class})),
        });
        t.record(&fact::turn::ProviderError {
            loop_index: i,
            target,
            class,
            transient,
            unknown,
            elapsed_ms: call_started.elapsed().as_millis() as u64,
            detail: pe.map(|p| serde_json::to_value(p).unwrap_or(Value::Null)),
            message: e.to_string(),
            correlation_id: &action.correlation_id,
            reserved_micros: action.reserved_micros,
            settled: settled
                .as_ref()
                .map(|a| format!("{a:?}"))
                .unwrap_or_else(|e| e.to_string()),
            s0,
        });
        Failure {
            class: class.into(),
            transient,
            usage_unknown: unknown,
            reason: format!("provider:{class}"),
            source: e,
        }
    }

    /// A `/stop` cut the model's call (theseus-yey): the stream is dropped, the
    /// turn acts on nothing it said, and no answer node is written. The call
    /// is settled as failed at an estimate of what it used, its reservation
    /// released and nothing held unknown: the input estimate its reservation
    /// used, and its output from the characters streamed (`CHARS_PER_TOKEN`
    /// to a token), at the model's prices, never more than it reserved. A stop
    /// before anything was sent used nothing. The `provider.cut` row says it is
    /// an estimate; the provider's own usage events, which a dropped stream
    /// loses, would make it exact.
    fn settle_cut(
        t: &mut Turn<'_>,
        action: &Action,
        price: &crate::catalog::CatalogEntry,
        cut: Cut,
        started_ms: u64,
        call_started: Instant,
        i: u32,
    ) -> Called {
        let target = t.target;
        let usage = if cut.sent {
            Usage {
                input_tokens: cut.est_input,
                output_tokens: cut.out_chars.div_ceil(CHARS_PER_TOKEN),
                ..Usage::default()
            }
        } else {
            Usage::default()
        };
        let cost = price.cost_micros(&usage).min(action.reserved_micros);
        t.record(&fact::turn::ModelCallFailed {
            class: "stopped",
            message: format!("cut by a stop from {}", cut.by),
        });
        let s0 = t.trace.now_us();
        let settled = t.tc.kernel.accept_completion(&Completion {
            correlation_id: action.correlation_id.clone(),
            outcome: ActionOutcome::Failed,
            result_ref: None,
            external_op_id: None,
            started_at_ms: started_ms,
            finished_at_ms: theseus_protocol::now_unix_ms(),
            producer: format!("provider:{}", target.provider),
            signature: None,
            cost_micros: Some(cost),
            detail: Some(json!({"class": "stopped", "by": cut.by, "estimated": true,
                "input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens,
                "output_chars": cut.out_chars})),
        });
        t.cost = t.cost.map(|c| c + micros_to_usd(cost));
        add_usage(&mut t.usage, &usage);
        t.record(&fact::turn::ModelCut {
            loop_index: i,
            target,
            correlation_id: &action.correlation_id,
            by: &cut.by,
            sent: cut.sent,
            usage: &usage,
            output_chars: cut.out_chars,
            cost,
            reserved_micros: action.reserved_micros,
            elapsed_ms: call_started.elapsed().as_millis() as u64,
            settled: settled
                .as_ref()
                .map(|a| format!("{a:?}"))
                .unwrap_or_else(|e| e.to_string()),
            s0,
        });
        Called::Stopped { by: cut.by }
    }

    /// Gate the model's tool calls in order, until one waits on a confirm,
    /// and run the ones before it, the reads of a group together
    /// (`ToolRuntime::run_calls`, theseus-a60). A response that did not stop
    /// for tools runs none of them. Returns how many calls have an answer.
    async fn run_tools(
        &self,
        t: &mut Turn<'_>,
        resp: &ModelResponse,
        uses: &[ToolUse],
        node: &Node,
        i: u32,
    ) -> Result<u32> {
        let tc = TurnCtx {
            loop_index: Some(i),
            ..t.tc
        };
        let stop = resp.stop_reason.as_deref();
        if stop != Some("tool_use") {
            let why = format!(
                "the response ended with stop reason `{}`",
                stop.unwrap_or("none")
            );
            for u in uses {
                self.tools.not_run(&tc, u, &why)?;
            }
            return Ok(0);
        }
        let calls: Vec<Call<'_>> = uses
            .iter()
            .map(|call| Call {
                call,
                invalid: resp.invalid_tool_inputs.get(&call.id).map(String::as_str),
            })
            .collect();
        let batch = self.tools.run_calls(&tc, &node.id, &calls).await?;
        t.tool_calls += batch.ran.len() as u32;
        let aws = self.tools.aws.as_deref();
        let lsp = self.tools.lsp.as_deref();
        Self::trace_calls(&mut t.trace, &self.tools, (aws, lsp), uses, &batch.ran);
        let mut answered = 0;
        for r in batch.ran {
            match r.outcome {
                CallOutcome::AwaitingConfirm { .. } => {}
                CallOutcome::Background { correlation_id } => {
                    t.background.push(correlation_id);
                    answered += 1;
                }
                CallOutcome::Done { .. } => answered += 1,
            }
        }
        t.awaiting = batch.awaiting;
        Ok(answered)
    }

    /// A span per call, in the order the calls ran. The calls of a group that
    /// ran together sit under one `tools` span, so their spans overlap there.
    /// Each names its tool, family, and backend (`none` for a tool that is
    /// not registered), and the call's result, which telemetry's tool metrics
    /// read (theseus-yf1). An AWS call's requests are spans under its own
    /// (AWS design §3.8).
    fn trace_calls(
        trace: &mut Trace,
        tools: &crate::toolrun::ToolRuntime,
        (aws, lsp): (Option<&crate::aws::Aws>, Option<&crate::lsp::Board>),
        uses: &[ToolUse],
        ran: &[Ran],
    ) {
        let span = |r: &Ran| {
            let wire = uses[r.index].name.as_str();
            let tool = tools.tool_by_wire(wire);
            let tool = tool.as_deref();
            let mut attrs = json!({"tool_use_id": uses[r.index].id, "outcome": format!("{:?}", r.outcome),
                "tool": tool.map_or(wire, |t| t.name()),
                "family": tool.map_or("unknown", |t| t.family()),
                "backend": tool.map_or("none", |t| t.backend().as_str()),
                "result": call_result(&r.outcome)});
            crate::mcp::span_attrs(tool, &mut attrs);
            Span {
                name: format!("tool {wire}"),
                kind: "tool".into(),
                start_us: trace.at(r.started),
                end_us: Some(trace.at(r.ended)),
                attrs,
                children: aws
                    .map(|a| a.spans(&uses[r.index].id, |i| trace.at(i)))
                    .into_iter()
                    .chain(lsp.map(|l| l.spans(&uses[r.index].id, |i| trace.at(i))))
                    .flatten()
                    .chain(crate::judge::gate::marks(&r.judged, |i| trace.at(i)))
                    .collect(),
            }
        };
        let mut groups: BTreeMap<usize, Vec<Span>> = BTreeMap::new();
        for r in ran {
            groups.entry(r.group).or_default().push(span(r));
        }
        for (_, mut spans) in groups {
            if spans.len() == 1 {
                trace.push(spans.remove(0));
                continue;
            }
            let start_us = spans.iter().map(|s| s.start_us).min().unwrap_or(0);
            let end_us = spans.iter().filter_map(|s| s.end_us).max();
            trace.push(Span {
                name: "tools".into(),
                kind: "tools".into(),
                start_us,
                end_us,
                attrs: json!({"calls": spans.len(), "together": true}),
                children: spans,
            });
        }
    }

    /// The Advancer decides whether the turn continues; the loop's end is
    /// recorded either way. `window_retry`: the answer was cut at the window
    /// and the call is made again (theseus-9p88), which the overflow's own
    /// line has said.
    fn advance(
        t: &mut Turn<'_>,
        resp: &ModelResponse,
        uses: usize,
        answered: u32,
        i: u32,
        window_retry: bool,
    ) -> bool {
        let advancer = UntilNoToolCalls {
            max_loops: t.target.max_loops,
        };
        let stop = resp.stop_reason.as_deref();
        let outcome = LoopOutcome {
            loop_index: i,
            provider_stop_reason: resp.stop_reason.clone(),
            tool_calls: answered,
            output_chars: resp.text.chars().count(),
        };
        let a0 = t.trace.now_us();
        let decision = if t.awaiting.is_some() {
            Decision::EndTurn("awaiting_confirm".into())
        } else if window_retry {
            Decision::Continue
        } else if matches!(
            stop,
            Some("refusal") | Some("max_tokens") | Some(WINDOW_EXCEEDED)
        ) {
            Decision::EndTurn(stop.unwrap_or_default().to_string())
        } else {
            advancer.decide(&outcome)
        };
        t.record(&fact::turn::LoopEnded {
            turn_id: t.tc.turn_id,
            outcome: &outcome,
            advancer: advancer.name(),
            decision: &decision,
            usage: &resp.usage,
            provider_stop_reason: resp.stop_reason.as_deref(),
            uses,
            answered,
            window_retry,
            a0,
        });
        match decision {
            Decision::Continue => true,
            Decision::EndTurn(reason) => {
                t.stop_reason = reason;
                false
            }
        }
    }

    /// End a turn that failed after it began, the one exit of every turn
    /// that does not finish (R1): book what its loops spent, close its trace,
    /// and write `turn.failed`. What the end had already done (`finish`, then
    /// a fault) is not done twice: books counted once, one accounting row
    /// (`turn.ended` or `turn.failed`), and the session's write once its last
    /// frame carries it.
    fn fail(mut t: Turn<'_>, session: &mut SessionRecord, f: Failure) -> anyhow::Error {
        t.close_books(session);
        if !t.ended {
            t.tc.record(&fact::turn::TurnFailed {
                turn_id: t.tc.turn_id,
                class: &f.class,
                reason: &f.reason,
                loops: t.loops,
                usage: &t.usage,
                cost_usd: t.cost,
                tool_calls: t.tool_calls,
            });
        }
        let trace = t
            .trace
            .finish(json!({"outcome": "failed", "class": f.class}));
        if !t.deferred {
            let written = t.tc.store.update_session(t.tc.session_id, |r| {
                r.take_turns_fields(session);
                Ok(vec![])
            });
            if let Err(e) = written {
                tracing::warn!(session_id = %t.tc.session_id, turn_id = %t.tc.turn_id,
                    error = %format!("{e:#}"), "a failed turn's books were not written to its session");
            }
        }
        TurnError {
            class: f.class,
            transient: f.transient,
            usage_unknown: f.usage_unknown,
            turn_id: t.tc.turn_id.into(),
            session_id: t.tc.session_id.into(),
            elapsed_ms: t.started.elapsed().as_millis() as u64,
            trace: Some(trace),
            usage: t.usage,
            cost_usd: t.cost,
            tool_calls: t.tool_calls,
            source: f.source,
        }
        .into()
    }

    /// A fault: an error the turn did not report itself, from any step after
    /// it began (R1). It is logged, and ends the turn as a failure of class
    /// `internal` does, so the session, the `turn.failed` row, telemetry,
    /// and the error itself carry what the turn's loops spent.
    fn fault(t: Turn<'_>, session: &mut SessionRecord, e: anyhow::Error) -> anyhow::Error {
        tracing::warn!(session_id = %t.tc.session_id, turn_id = %t.tc.turn_id,
            error = %format!("{e:#}"), "the turn faulted; its books are closed");
        let reason =
            crate::toolrun::cap(&format!("{FAULT_CLASS}: {e:#}"), 400, |_| String::new()).0;
        Self::fail(
            t,
            session,
            Failure {
                class: FAULT_CLASS.into(),
                transient: false,
                usage_unknown: false,
                reason,
                source: e,
            },
        )
    }

    /// Absorb results that settled while the turn ran, book the turn, write
    /// its result, and park the execution. The `bool` asks for another turn:
    /// a late result the model has not read. The session write waits for
    /// the turn's last frame, `end_turn`'s, under the returned hold
    /// (theseus-l6y). A fault at any step is the turn's one exit (`fault`),
    /// which books what this has not.
    fn finish(
        &self,
        mut t: Turn<'_>,
        session: &mut SessionRecord,
        unused_recompile: Option<Recompile>,
    ) -> Result<(TurnSubmitResult, TurnEnd, bool, Option<SessionHold>)> {
        let late = match self.tools.absorb(&t.tc) {
            Ok((_, late)) => late,
            Err(e) => return Err(Self::fault(t, session, e)),
        };
        // Where the execution waits: only reads, so it is decided before
        // anything is booked, and a fault here leaves nothing half done.
        let end = match &t.budget_question {
            Some(q) => TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: q.clone(),
                },
            },
            None => match self.park(t.tc.execution_id, t.awaiting.clone(), &t.background) {
                Ok(end) => end,
                Err(e) => return Err(Self::fault(t, session, e)),
            },
        };
        t.close_books(session);
        let target = t.target;
        session.last_target = Some(TargetRef::from(target));
        let w0 = t.trace.now_us();
        // Only the turn's own fields: a recompile asked meanwhile stays
        // (theseus-xeo). The record rides in the turn's last frame, where
        // the rows before it here and after it wait too; the span times its
        // making, and the frame's write is `end_turn`'s.
        let hold = match t.tc.store.defer_session(t.tc.session_id, |r| {
            r.take_turns_fields(session);
            if r.pending_recompile.is_none() {
                r.pending_recompile = unused_recompile;
            }
        }) {
            Ok(hold) => hold,
            Err(e) => return Err(Self::fault(t, session, e)),
        };
        t.deferred = true;

        let last = t.last.as_ref();
        let mut result = TurnSubmitResult {
            session_id: t.tc.session_id.into(),
            turn_id: t.tc.turn_id.into(),
            loops: t.loops,
            output: std::mem::take(&mut t.output),
            stop_reason: if t.stop_reason.is_empty() {
                "end_turn".into()
            } else {
                t.stop_reason.clone()
            },
            provider_stop_reason: last.and_then(|r| r.stop_reason.clone()),
            model: last.map_or_else(|| target.model.clone(), |r| r.model.clone()),
            provider: target.provider.clone(),
            profile: target.profile.clone(),
            usage: t.usage.clone(),
            elapsed_ms: t.started.elapsed().as_millis() as u64,
            first_token_ms: last.and_then(|r| r.timing.first_token_ms),
            request_id: last.and_then(|r| r.request_id.clone()),
            trace: None,
            execution_id: Some(t.tc.execution_id.into()),
            cost_usd: t.cost,
            tool_calls: t.tool_calls,
            awaiting_confirm: t.awaiting.clone().or_else(|| t.budget_question.clone()),
            stop_details: last.and_then(|r| r.stop_details.clone()),
            continuation: t.continuation,
            recalled: t.recall.count,
        };
        t.record(&fact::turn::TurnBooked {
            result: &result,
            session_usage: &session.usage,
            late,
            w0,
        });
        t.ended = true;
        // A task's turns post nothing to the place: its report does, once,
        // when it ends (DD7). A stopped turn posts nothing either (W1): the
        // stop's answer says what stopped.
        if t.tc.task.is_none() && result.stop_reason != "stopped" {
            if let Err(e) = self.stage_reply(&t, &result) {
                return Err(Self::fault(t, session, e));
            }
        }
        // The turn's frames (theseus-wz4y): those written so far, and its
        // last, `end_turn`'s, which carries this trace. The bench counts the
        // same turn's from the WAL (`theseus-sim bench turn`).
        let frames = t.tc.store.turn_frames().map(|n| n + 1);
        // A judgment of this turn's end is marked before its last frame (23b).
        self.judge
            .mark_turn_end(&mut t.trace, &result, t.tc.task.is_some());
        result.trace = Some(t.trace.finish(json!({
            "outcome": "complete",
            "loops": result.loops,
            "stop_reason": result.stop_reason,
            "usage": result.usage,
            "frames": frames,
        })));
        // The trace is finished: the fact draws no span.
        t.tc.record(&fact::turn::TurnEnded { result: &result });
        let is_task = t.tc.task.is_some();
        let stop = result.stop_reason.clone();
        // A stopped turn takes no other by itself (W1), even for a result
        // that landed during it: the next input's turn reads it.
        let mut again = late > 0 && stop != "stopped";
        // A task that has nothing left to wait on is done (DD7): its last
        // message is its report, which the frame that ends it carries. One
        // with results its model has not read (late ones, or a turn stopped at
        // its loop cap) takes another turn instead: no one else will wake it.
        // One with a wake pending parks on it, and the wake's turn goes on (37b).
        let end = match end {
            TurnEnd::Wait { wake: Wake::Input } if is_task => {
                if again || stop == "max_loops" {
                    again = true;
                    TurnEnd::Wait { wake: Wake::Input }
                } else if crate::task::parks_on_wake(t.tc.kernel, t.tc.execution_id) {
                    TurnEnd::Wait { wake: Wake::Input }
                } else {
                    TurnEnd::Complete {
                        reason: "reported".into(),
                    }
                }
            }
            end => end,
        };
        Ok((result, end, again, hold))
    }

    /// The turn's reply as an outbox post, when its session posts somewhere
    /// (theseus-q4v): each loop's text, by node, and what its footer says.
    /// It rides in the frame that ends the turn, so a turn that ended has its
    /// reply written, whether or not a binding is connected.
    fn stage_reply(&self, t: &Turn<'_>, result: &TurnSubmitResult) -> Result<()> {
        // What the reply answers: the wakes and reports this turn took. The
        // retry of a turn that failed before its reply took none, since the
        // first took them, and their nodes are still unanswered in the
        // session: it frames its reply from those, and finds the target the
        // take kept (theseus-4lx).
        let (mut wakes, mut reports) = (t.wakes.clone(), t.woke_by.clone());
        let mut retried = false;
        if wakes.is_empty() && reports.is_empty() {
            (wakes, reports) = Self::unread_relays(t)?;
            retried = !wakes.is_empty() || !reports.is_empty();
        }
        // A wake's turn in a session its place has moved on from (`/new`)
        // still answers where the wake was set (DD8).
        let Some(target) = self
            .outbox
            .target(t.tc.session_id)
            .or_else(|| t.wake_target.clone())
            .or_else(|| {
                retried
                    .then(|| self.outbox.wake_target(t.tc.session_id))
                    .flatten()
            })
        else {
            return Ok(());
        };
        let footer = TurnSubmitResult {
            output: String::new(),
            trace: None,
            ..result.clone()
        };
        let mut body = json!({
            "kind": "reply",
            "turn_id": result.turn_id,
            "loops": t.said,
            "result": footer,
            "reply_to": t.reply_to,
        });
        if !wakes.is_empty() {
            body["wakes"] = json!(wakes);
        }
        if !reports.is_empty() {
            body["reports"] = json!(reports);
        }
        let (post, records) =
            self.outbox
                .stage(t.tc.session_id, t.tc.execution_id, &target, body)?;
        for r in records {
            t.tc.store.defer(r)?;
        }
        t.tc.posts.lock().unwrap().push(post);
        Ok(())
    }

    /// The wakes and reports an earlier attempt took, which this turn answers
    /// (theseus-4lx): the harness's wake and task-report nodes written by a
    /// turn other than this one, after the last answer the session gave. Each
    /// is the line the reply's header names: the node's own text for a wake,
    /// and `task:<short>`'s report line, as the turn that took it would have
    /// said.
    fn unread_relays(t: &Turn<'_>) -> Result<(Vec<Value>, Vec<Value>)> {
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let (mut wakes, mut reports) = (Vec::new(), Vec::new());
        for (_, n) in nodes.iter().rev() {
            if n.turn_id.as_deref() == Some(t.tc.turn_id) {
                continue;
            }
            match &n.body {
                // An answer: what comes before it was read.
                Body::AssistantMessage { blocks, .. }
                    if crate::provider::tool_uses_in(blocks).is_empty() =>
                {
                    break
                }
                Body::UserMessage { text, .. }
                    if matches!(n.origin, crate::node::Origin::Harness) =>
                {
                    let author = n.author.as_deref().unwrap_or_default();
                    if author.starts_with("wake:") {
                        wakes.push(json!({"text": text}));
                    } else if let Some(short) = author.strip_prefix("task:") {
                        reports
                            .push(json!({"short": short, "text": crate::task::woke_line(short)}));
                    }
                }
                _ => {}
            }
        }
        wakes.reverse();
        reports.reverse();
        Ok((wakes, reports))
    }

    /// Where the execution waits: on the confirm, on outstanding jobs, or on
    /// the next input. (A turn over its budget waits on the budget question.)
    fn park(
        &self,
        exec_id: &str,
        awaiting: Option<String>,
        background: &[String],
    ) -> Result<TurnEnd> {
        let outstanding: Vec<String> = self
            .kernel
            .execution(exec_id)?
            .map(|e| e.outstanding)
            .unwrap_or_default()
            .into_iter()
            .filter(|c| {
                background.contains(c)
                    || self
                        .kernel
                        .action(c)
                        .ok()
                        .flatten()
                        .map(|a| a.tool != PROVIDER_TOOL)
                        .unwrap_or(false)
            })
            .collect();
        let wake = if let Some(c) = awaiting {
            Wake::Confirm { confirm_id: c }
        } else if !outstanding.is_empty() {
            Wake::Actions {
                correlation_ids: outstanding,
            }
        } else {
            Wake::Input
        };
        Ok(TurnEnd::Wait { wake })
    }

    /// A new compilation (it carries its own `derived_from`) and the
    /// session's pointer, in one frame. The stored record takes only the
    /// turn's fields (theseus-xeo).
    fn persist_compilation(
        store: &Store,
        compiled: &Compiled,
        session: &mut SessionRecord,
        turn_id: &str,
    ) -> Result<()> {
        let c = &compiled.compilation;
        session.compilation_id = Some(c.id.clone());
        let records = || -> Result<Vec<NewRecord>> {
            Ok(vec![
                NewRecord::json(theseus_store::kinds::COMPILATION, Some(&c.id), c)?
                    .scoped(&c.session_id),
                fact::row(
                    &fact::turn::ContextRecompiled { compilation: c },
                    Some(&c.session_id),
                    Some(turn_id),
                )?,
            ])
        };
        let written = store.update_session(&session.session_id, |r| {
            r.take_turns_fields(session);
            records()
        })?;
        if written.is_none() {
            // A session whose record was never stored: this is its first.
            let mut frame = records()?;
            frame.push(NewRecord::json(
                theseus_store::kinds::SESSION,
                Some(&session.session_id),
                &*session,
            )?);
            store.append(&frame)?;
        }
        Ok(())
    }
}

/// What a budget question says (theseus-0sg), here and wherever it is shown
/// again (`confirm.list`, the Discord card). `call` is the question's
/// `args.call`: the call's profile, model, and output cap, or `Null` for a
/// question asked before theseus-kks. A call that alone needs more than the
/// whole limit cannot fit after any reset (theseus-kks), nor can one that
/// needs more than the limit leaves once `kept` is taken, what a reset
/// leaves held (theseus-6g6), so its question says so, with the figures,
/// names the two remedies, and says what an approval then does: one more
/// try, and no second question.
pub(crate) fn budget_question(
    who: &str,
    spent: Micros,
    limit: Micros,
    needed: Micros,
    kept: Kept,
    call: &Value,
) -> String {
    if needed <= limit.saturating_sub(kept.total()) {
        return format!(
            "{who} has spent {} of its {} limit. Reset its spend to $0 and continue?",
            narrative::dollars(spent),
            narrative::dollars(limit)
        );
    }
    let model = call["model"].as_str().map_or_else(
        || "its next model call".to_string(),
        |m| format!("the call to {m}"),
    );
    let lower = match (call["profile"].as_str(), call["max_output_tokens"].as_u64()) {
        (Some(p), Some(n)) => lower_cap(p, n),
        _ => "lower the profile's `max_output_tokens`".to_string(),
    };
    if needed <= limit {
        return format!(
            "{who} is waiting on {model}, which reserves {}. Of its {} limit, {}, and a reset \
             leaves it held, so resetting its spend to $0 cannot make the call fit. Raise \
             `[kernel] spend_limit_usd` above {}, or {lower}. Approving resets the spend and \
             tries the call once more; if it still does not fit, the turn ends and does not \
             ask again.",
            narrative::dollars(needed),
            narrative::dollars(limit),
            kept.clause(),
            narrative::dollars(needed + kept.total()),
        );
    }
    format!(
        "{who} is waiting on {model}, which alone reserves {}: more than its whole {} limit, \
         so resetting its spend to $0 cannot make it fit. Raise `[kernel] spend_limit_usd` above \
         {}, or {lower}. Approving resets the spend and tries the call once more; if it still \
         does not fit, the turn ends and does not ask again.",
        narrative::dollars(needed),
        narrative::dollars(limit),
        narrative::dollars(needed),
    )
}

/// What an approved reset leaves held against an execution's limit
/// (theseus-6g6): the amounts reserved for calls in flight and held for calls
/// whose cost is unknown. A budget question and an over-limit failure name
/// them, since they are why a reset does not make a call fit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Kept {
    pub reserved: Micros,
    pub unknown: Micros,
}

impl Kept {
    /// The execution's now, or nothing held when it cannot be read.
    pub(crate) fn of(kernel: &Kernel, execution_id: &str) -> Self {
        kernel
            .execution(execution_id)
            .ok()
            .flatten()
            .map(|e| Self {
                reserved: e.budget.reserved_micros,
                unknown: e.budget.held_unknown_micros,
            })
            .unwrap_or_default()
    }

    pub(crate) fn total(self) -> Micros {
        self.reserved + self.unknown
    }

    /// What is held, as a clause that follows "of its limit,": `$1.00 is
    /// held for calls whose cost is unknown`.
    pub(crate) fn clause(self) -> String {
        let unknown = format!(
            "{} is held for calls whose cost is unknown",
            narrative::dollars(self.unknown)
        );
        let reserved = format!(
            "{} is reserved for calls in flight",
            narrative::dollars(self.reserved)
        );
        match (self.unknown > 0, self.reserved > 0) {
            (true, true) => format!("{unknown} and {reserved}"),
            (false, true) => reserved,
            _ => unknown,
        }
    }
}

/// The second remedy for a call over the whole limit (theseus-kks).
fn lower_cap(profile: &str, max_output_tokens: u64) -> String {
    format!(
        "lower `max_output_tokens` under `[profiles.{profile}]` (now {})",
        narrative::thousands(max_output_tokens)
    )
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

/// What became of a call, as its trace span says it (theseus-yf1): its result
/// node's status, or that it waits for the operator or runs in the
/// background.
pub(crate) fn call_result(o: &CallOutcome) -> &'static str {
    match o {
        CallOutcome::Done { status } => status.as_str(),
        CallOutcome::AwaitingConfirm { .. } => "awaiting_confirm",
        CallOutcome::Background { .. } => "background",
    }
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
