//! The turn's facts: a turn, its loops, and its model calls (`turn.rs`).

use serde_json::{json, Value};
use theseus_kernel::{micros_to_usd, Action, FiredWake, Kernel, Micros, TurnEnd, Wake};
use theseus_protocol::NarrativePart::{Approval, Context, Loop, Model, Session, Turn};
use theseus_protocol::{
    notify, ConfirmRequest, ConfirmResolved, Event, LedgerKind, TurnSubmitResult, Usage,
};

use super::{Fact, Say};
use crate::advancer::{Decision, LoopOutcome};
use crate::compiler::{Compilation, Compiled, RequestSpec};
use crate::narrative;
use crate::node::Node;
use crate::provider::ModelResponse;
use crate::session::{Failing, Then};
use crate::toolrun::ResumeOutcome;
use crate::trace::Trace;
use crate::turn::{Overflowing, Target, WINDOW_CLASS};

/// A wait for admission and the turn lock the narrative mentions.
pub const LOCK_WAIT_NOTICEABLE_US: u64 = 50_000;

/// A turn began, admitted under its execution's lock (`turn.started`), and
/// what it waited for first: the vault's word on the config, its secrets,
/// and admission.
pub struct TurnStarted<'a> {
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub execution_id: &'a str,
    /// The kernel's count of the execution's turns.
    pub kernel_turn: u64,
    pub target: &'a Target,
    /// No input: the driver's continuation.
    pub continuation: bool,
    /// The input's length in characters; `None` for a continuation.
    pub input_chars: Option<usize>,
    pub attachments: usize,
    /// Who asked: a client (`web#3`), or `harness`.
    pub author: &'a str,
    /// What it waited for, in microseconds from its arrival: its secrets
    /// (theseus-qa0), arrival to admission (the secrets included), and
    /// admission and the turn lock alone.
    pub secrets_us: u64,
    pub lock_us: u64,
    pub admit_us: u64,
}

impl Fact for TurnStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnStarted);
    const METHOD: Option<&'static str> = Some(notify::TURN_STARTED);

    fn row(&self) -> Value {
        let t = self.target;
        let mut row = json!({"input_chars": self.input_chars, "profile": t.profile, "provider": t.provider, "model": t.model, "execution_id": self.execution_id, "kernel_turn": self.kernel_turn, "continuation": self.continuation, "author": self.author});
        if self.attachments > 0 {
            row["attachments"] = json!(self.attachments);
        }
        row
    }

    fn event(&self) -> Option<Event> {
        Some(Event::TurnStarted(theseus_protocol::TurnStarted {
            session_id: self.session_id.into(),
            turn_id: self.turn_id.into(),
            execution_id: Some(self.execution_id.into()),
            continuation: self.continuation,
        }))
    }

    fn span(&self, trace: &mut Trace) {
        let secrets_end = self.secrets_us;
        if self.secrets_us > 0 {
            trace.record(
                "secrets.wait",
                "lock",
                0,
                secrets_end,
                json!({"provider": self.target.provider, "note": "the provider's key and the first round of secrets (theseus-qa0)"}),
            );
        }
        trace.record(
            "admission.wait",
            "lock",
            secrets_end,
            self.lock_us,
            json!({"execution_id": self.execution_id, "turn": self.kernel_turn, "note": "kernel admission + per-execution turn lock"}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let t = self.target;
        if self.secrets_us > 0 {
            say.line(
                Turn,
                format!(
                    "Waited {} for the vault: {} needs its key, and every secret must be known \
                     before a tool result is scrubbed.",
                    narrative::duration(self.secrets_us / 1000),
                    t.provider
                ),
            );
        }
        let with_files = match self.attachments {
            0 => String::new(),
            n => format!(
                " and {}",
                narrative::count(n as u64, "attachment", "attachments")
            ),
        };
        let loops = narrative::count(t.max_loops as u64, "loop", "loops");
        say.line(
            Turn,
            match self.input_chars {
                Some(chars) => format!(
                    "Turn {} started by {} on {} ({}): {} of input{with_files}; up to {loops}.",
                    narrative::short(self.turn_id),
                    self.author,
                    t.profile,
                    t.model,
                    narrative::count(chars as u64, "character", "characters"),
                ),
                None => format!(
                    "Continuation turn {} started by the {} on {} ({}): no new input; up to \
                     {loops}.",
                    narrative::short(self.turn_id),
                    self.author,
                    t.profile,
                    t.model,
                ),
            },
        );
        if self.admit_us >= LOCK_WAIT_NOTICEABLE_US {
            say.line(
                Turn,
                format!(
                    "It waited {} for admission and the turn lock.",
                    narrative::duration(self.admit_us / 1000)
                ),
            );
        }
    }
}

// ---------------------------------------------------------------- the session's, around a turn

/// A session from before M2 had no execution, and one was opened for it.
pub struct ExecutionOpened<'a> {
    pub session_id: &'a str,
    pub execution_id: &'a str,
    pub limit_micros: Micros,
}

impl Fact for ExecutionOpened<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Session {} had no execution; opened {} with a spend limit of {}.",
                narrative::short(self.session_id),
                narrative::short(self.execution_id),
                narrative::dollars(self.limit_micros)
            ),
        );
    }
}

/// New input came while a budget question was open, and superseded it: a
/// new message is not an answer.
pub struct BudgetQuestionSuperseded<'a> {
    pub session_id: &'a str,
    pub question: &'a str,
    pub author: &'a str,
}

impl Fact for BudgetQuestionSuperseded<'_> {
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.session_id.into(),
            correlation_id: self.question.into(),
            by: Some(self.author.into()),
            superseded: true,
            ..Default::default()
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "The budget question was superseded by new input from {}; the next call asks \
                 again if it still does not fit.",
                self.author
            ),
        );
    }
}

/// An input's turn was refused: a secret it needs is not there
/// (`turn.refused`).
pub struct TurnRefused<'a> {
    /// `secret_failed` or `secret_resolving`.
    pub class: &'a str,
    pub secret: &'a str,
    pub error: &'a str,
    pub author: &'a str,
    pub provider: &'a str,
}

impl Fact for TurnRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnRefused);

    fn row(&self) -> Value {
        json!({"class": self.class, "secret": self.secret, "error": self.error, "author": self.author, "provider": self.provider})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Turn,
            format!(
                "A turn from {} was refused: {} needs the secret {}, which {}.",
                self.author,
                self.provider,
                self.secret,
                if self.class == "secret_failed" {
                    "did not resolve"
                } else {
                    "is still resolving"
                }
            ),
        );
    }
}

/// An input's turn was refused: its execution cannot run (it ended).
pub struct TurnNotRunnable<'a> {
    pub author: &'a str,
    pub state: &'a str,
}

impl Fact for TurnNotRunnable<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Turn,
            format!(
                "A turn from {} was refused: the execution is {}.",
                self.author, self.state
            ),
        );
    }
}

/// New input woke a waiting execution.
pub struct WokenByInput<'a> {
    pub author: &'a str,
}

impl Fact for WokenByInput<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, format!("Woken by new input from {}.", self.author));
    }
}

/// A session's first turn since the daemon started, with turns behind it.
pub struct SessionResumed<'a> {
    pub session_id: &'a str,
    pub turns: u64,
    pub cost_usd: f64,
}

impl Fact for SessionResumed<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Session {} resumed: its first turn since the daemon started, after {} and {}.",
                narrative::short(self.session_id),
                narrative::count(self.turns, "turn", "turns"),
                narrative::money(Some(self.cost_usd))
            ),
        );
    }
}

/// Where the execution waits once the turn ended.
pub struct Parked<'a> {
    pub end: &'a TurnEnd,
    /// The kernel, to name the tool whose approval it waits on.
    pub kernel: &'a Kernel,
}

impl Fact for Parked<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let p = match self.end {
            TurnEnd::Wait {
                wake: Wake::Confirm { confirm_id },
            } => {
                let tool = self
                    .kernel
                    .action(confirm_id)
                    .ok()
                    .flatten()
                    .map_or_else(|| "a call".to_string(), |a| a.tool);
                format!("Parked until the operator answers the approval for {tool}.")
            }
            TurnEnd::Wait {
                wake: Wake::Actions { correlation_ids },
            } => match correlation_ids.len() {
                1 => "Parked until 1 background job finishes.".into(),
                n => format!("Parked until one of {n} background jobs finishes."),
            },
            TurnEnd::Wait { wake: Wake::Input } => "Parked until the next input.".into(),
            TurnEnd::Wait {
                wake: Wake::Budget { .. },
            } => "Parked on the budget until the operator resets the spend; a new message asks \
                  again."
                .into(),
            other => format!("The turn ends the execution's wait: {other:?}."),
        };
        say.line(Session, p);
    }
}

// ---------------------------------------------------------------- the turn's start

/// What happened while no turn ran, caught up at a turn's start: results
/// settled and written, calls resumed, task reports and wakes read.
pub struct CaughtUp<'a> {
    /// When the catching up began, on the trace's clock.
    pub t0: u64,
    pub settled: usize,
    pub absorbed: u32,
    pub resumed: &'a ResumeOutcome,
    /// A budget question the kernel queued as a result: approved (the spend
    /// was reset), or withdrawn by a raised limit.
    pub reset: bool,
    pub raised: bool,
    pub reported: u32,
    pub woke: u32,
    /// The spans of the calls answered here: the late results, then the
    /// calls resumed (theseus-8pei), under the continuation's.
    pub calls: Vec<theseus_protocol::Span>,
}

impl Fact for CaughtUp<'_> {
    fn span(&self, trace: &mut Trace) {
        let r = self.resumed;
        trace.push(theseus_protocol::Span {
            name: "continuation".into(),
            kind: "tool".into(),
            start_us: self.t0,
            end_us: Some(trace.now_us()),
            attrs: json!({"settled": self.settled, "late_results": self.absorbed, "resumed": r.wrote, "awaiting": r.awaiting, "background": r.background, "budget_reset": self.reset, "limit_raised": self.raised, "task_reports": self.reported, "wakes": self.woke}),
            children: self.calls.clone(),
        });
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let mut done = Vec::new();
        if self.reset {
            done.push("the spend was reset, so the waiting call proceeds".to_string());
        } else if self.raised {
            done.push("the spend limit was raised, so the waiting call proceeds".to_string());
        }
        if self.settled > 0 {
            done.push(format!(
                "{} settled",
                narrative::count(self.settled as u64, "action", "actions")
            ));
        }
        if self.absorbed > 0 {
            done.push(format!(
                "{} written",
                narrative::count(self.absorbed as u64, "late result", "late results")
            ));
        }
        if self.resumed.wrote > 0 {
            done.push(format!(
                "{} answered",
                narrative::count(self.resumed.wrote as u64, "pending call", "pending calls")
            ));
        }
        if self.resumed.awaiting.is_some() {
            done.push("a call still waits for approval".into());
        }
        if self.reported > 0 {
            done.push(format!(
                "{} read",
                narrative::count(self.reported as u64, "task report", "task reports")
            ));
        }
        if self.woke > 0 {
            done.push(format!(
                "{} came due",
                narrative::count(self.woke as u64, "wake", "wakes")
            ));
        }
        if !done.is_empty() {
            say.line(
                Turn,
                format!("Caught up on the time between turns: {}.", done.join(", ")),
            );
        }
    }
}

/// A `/stop` ended the turn's loops (W1): it plans nothing more and posts no
/// reply.
pub struct Stopped<'a> {
    pub by: &'a str,
}

impl Fact for Stopped<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Turn,
            format!(
                "Stopped by {}: the turn plans nothing more, posts no reply, and the session \
                 waits on its next input.",
                self.by
            ),
        );
    }
}

/// A wake came due (DD8): its note is this turn's input.
pub struct WakeCameDue<'a> {
    pub fired: &'a FiredWake,
}

impl Fact for WakeCameDue<'_> {
    /// Its point for the metrics (`theseus.wakes.fired`, `theseus.wakes.
    /// late_ms`; 37a), on the turn's trace.
    fn span(&self, trace: &mut Trace) {
        let f = self.fired;
        let at = trace.now_us();
        let mut attrs =
            json!({"wake_id": f.wake.id, "repeat": f.wake.repeat.is_some(), "late_ms": f.late_ms});
        if f.wake.repeat.is_some() {
            attrs["occurrence"] = json!(f.wake.occurrence);
            attrs["missed"] = json!(f.missed);
        }
        trace.record("wake.fired", "wake", at, at, attrs);
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let f = self.fired;
        if f.wake.repeat.is_some() {
            // "Wake a1b2c3 fired (#4, 2 missed while down); next 21:00 Thu:
            // \"…\" is this turn's input."
            let mut how = vec![format!("#{}", f.wake.occurrence)];
            if f.missed > 0 {
                let down = if f.while_down { " while down" } else { "" };
                how.push(format!("{} missed{down}", f.missed));
            } else if f.late_ms > crate::wake::LATE_AFTER_MS {
                how.push(format!("{} late", crate::wake::span(f.late_ms)));
            }
            let next = match f.next_due_at_ms {
                Some(n) => format!("next {}", crate::wake::next_of(n)),
                None => "the series ended (until)".into(),
            };
            say.line(
                Session,
                format!(
                    "Wake {} fired ({}); {next}: \"{}\" is this turn's input.",
                    crate::task::short(&f.wake.id),
                    how.join(", "),
                    crate::session::title_from(&f.wake.note)
                ),
            );
            return;
        }
        say.line(
            Session,
            format!(
                "Wake {} came due{}: \"{}\" is this turn's input.",
                crate::task::short(&f.wake.id),
                if f.late_ms > crate::wake::LATE_AFTER_MS {
                    format!(", {} late", crate::wake::span(f.late_ms))
                } else {
                    String::new()
                },
                crate::session::title_from(&f.wake.note)
            ),
        );
    }
}

/// A task's report started this turn (W1, `wake_parent`).
pub struct ReportWoke<'a> {
    pub report: &'a crate::task::Report,
}

impl Fact for ReportWoke<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.report;
        say.line(
            Session,
            format!(
                "Task {}'s report started this turn (wake_parent): \"{}\" {}.",
                r.short,
                r.title.as_deref().unwrap_or("its brief"),
                r.outcome
            ),
        );
    }
}

/// The model is not called: a call still waits for approval, or there is
/// nothing new for it to read.
pub enum NoCall {
    AwaitingApproval,
    NothingNew,
}

impl Fact for NoCall {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Turn,
            match self {
                NoCall::AwaitingApproval => {
                    "A call still waits for approval, so the model is not called.".into()
                }
                NoCall::NothingNew => {
                    "Nothing new for the model to read, so it is not called.".into()
                }
            },
        );
    }
}

/// A context file could not be read, so the system block says it is missing
/// (`context.file_missing`; warned once per daemon run).
pub struct ContextFileMissing<'a> {
    pub path: &'a str,
    pub error: &'a str,
    pub profile: &'a str,
}

impl Fact for ContextFileMissing<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextFileMissing);

    fn row(&self) -> Value {
        json!({"path": self.path, "error": self.error, "profile": self.profile})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "Context: the context file {} could not be read ({}), so the system block says \
                 it is missing.",
                self.path, self.error
            ),
        );
    }
}

// ---------------------------------------------------------------- a loop

/// A loop began: its span opens, and the narrative counts it.
pub struct LoopOpened {
    /// Its index, from 0.
    pub index: u32,
    pub max_loops: u32,
}

impl Fact for LoopOpened {
    fn span(&self, trace: &mut Trace) {
        trace.enter(
            &format!("loop {}", self.index),
            "loop",
            json!({"loop": self.index}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Loop,
            format!("Loop {} of up to {}.", self.index + 1, self.max_loops),
        );
    }
}

/// The loop's context was compiled: an append to the current compilation,
/// or a new one (`context.compiled`; its span and its notification carry
/// the same summary).
pub struct ContextCompiled<'a> {
    pub summary: &'a theseus_protocol::ContextCompiled,
    pub compiled: &'a Compiled,
    pub spec: &'a RequestSpec,
    /// When the compile began and ended, on the trace's clock.
    pub c0: u64,
    pub c1: u64,
}

impl Fact for ContextCompiled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextCompiled);
    const METHOD: Option<&'static str> = Some(notify::CONTEXT_COMPILED);

    fn row(&self) -> Value {
        serde_json::to_value(self.summary).unwrap_or(Value::Null)
    }

    fn event(&self) -> Option<Event> {
        Some(Event::ContextCompiled(self.summary.clone()))
    }

    fn span(&self, trace: &mut Trace) {
        trace.record("compile", "compile", self.c0, self.c1, self.row());
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let (compiled, spec) = (self.compiled, self.spec);
        let missing = spec
            .context_files
            .iter()
            .filter(|f| f.missing.is_some())
            .count();
        let mut files = match (spec.context_files.len() - missing, missing) {
            (0, 0) => String::new(),
            (n, 0) => format!(
                ", {}",
                narrative::count(n as u64, "context file", "context files")
            ),
            (n, m) => format!(
                ", {} ({m} missing)",
                narrative::count(n as u64, "context file", "context files")
            ),
        };
        if let Some(p) = &spec.persona {
            files.push_str(&format!(", persona {p}"));
        }
        // A shared place's files not marked public (the place rule).
        if self.summary.withheld > 0 {
            files.push_str(&format!(
                ", {} withheld (not public, in a shared place)",
                narrative::count(self.summary.withheld, "context file", "context files")
            ));
        }
        let sizes = format!(
            "prefix {} + tail {}, {}, about {} tokens{files}",
            narrative::count(compiled.prefix_nodes as u64, "node", "nodes"),
            compiled.tail_nodes,
            narrative::count(compiled.messages as u64, "message", "messages"),
            narrative::thousands(compiled.est_tokens)
        );
        if compiled.new_compilation {
            say.line(
                Context,
                format!(
                    "Context: new compilation {} ({}) because {}: {sizes}.",
                    narrative::short(&compiled.compilation.id),
                    compiled.compilation.strategy,
                    narrative::trigger_phrase(compiled.trigger.as_deref().unwrap_or("unknown")),
                ),
            );
        } else {
            say.line(
                Context,
                format!(
                    "Context: appending to compilation {}: {sizes}.",
                    narrative::short(&compiled.compilation.id),
                ),
            );
        }
        // A system block left without its cache breakpoint is said once, when
        // the compilation is made (theseus-ev1); its manifest records it.
        if compiled.new_compilation {
            let layout = &compiled.cache;
            if !layout.caches {
                say.line(
                    Context,
                    format!(
                        "Context: no cache breakpoints, since the catalog says {} does not cache.",
                        spec.model
                    ),
                );
            }
            for b in layout.blocks.iter().filter(|b| layout.caches && !b.marked) {
                say.line(
                    Context,
                    format!(
                        "Context: no cache breakpoint on the system's {} block: with the tools, \
                         its prefix is {} bytes, too short to reach {}'s minimum of {} tokens.",
                        b.block,
                        narrative::thousands(b.prefix_bytes),
                        spec.model,
                        narrative::thousands(layout.min_tokens as u64)
                    ),
                );
            }
        }
        if !compiled.repairs.is_empty() {
            say.line(
                Context,
                format!(
                    "Context: repaired {} with a synthetic result.",
                    narrative::count(
                        compiled.repairs.len() as u64,
                        "tool call that had no result",
                        "tool calls that had no result"
                    )
                ),
            );
        }
    }
}

/// The loop's request is ready, with its model and its tools
/// (`loop.started`).
pub struct LoopStarted<'a> {
    pub turn_id: &'a str,
    pub index: u32,
    pub model: &'a str,
    pub tools_offered: u32,
}

impl Fact for LoopStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LoopStarted);
    const METHOD: Option<&'static str> = Some(notify::LOOP_STARTED);

    fn row(&self) -> Value {
        json!({"loop": self.index})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::LoopStarted(theseus_protocol::LoopStarted {
            turn_id: self.turn_id.into(),
            loop_index: self.index,
            model: self.model.into(),
            tools_offered: self.tools_offered,
        }))
    }
}

/// The loop ended before its answer was run: the span closes with why
/// (`budget`, `stopped`, `image_not_shown`, the window).
pub struct LoopCut<'a> {
    pub decision: &'a str,
}

impl Fact for LoopCut<'_> {
    fn span(&self, trace: &mut Trace) {
        trace.exit(json!({"decision": self.decision}));
    }
}

/// The Advancer decided whether the turn continues, and the loop ended
/// (`loop.ended`). `retry`: the call is made again, which the retry's own
/// line has said.
pub struct LoopEnded<'a> {
    pub turn_id: &'a str,
    pub outcome: &'a LoopOutcome,
    pub advancer: &'a str,
    pub decision: &'a Decision,
    pub usage: &'a Usage,
    pub provider_stop_reason: Option<&'a str>,
    /// The calls the model asked for, and how many have an answer.
    pub uses: usize,
    pub answered: u32,
    /// The call is made again: an answer cut at the window (theseus-9p88),
    /// or a refusal's fallback (theseus-7gir.18). Its own fact says so.
    pub retry: bool,
    /// When the Advancer began deciding, on the trace's clock.
    pub a0: u64,
}

impl Fact for LoopEnded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LoopEnded);
    const METHOD: Option<&'static str> = Some(notify::LOOP_ENDED);

    fn row(&self) -> Value {
        json!({"loop": self.outcome.loop_index, "outcome": self.outcome, "advancer": self.advancer, "decision": self.decision, "usage": self.usage})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::LoopEnded(theseus_protocol::LoopEnded {
            turn_id: self.turn_id.into(),
            loop_index: self.outcome.loop_index,
            provider_stop_reason: self.provider_stop_reason.map(str::to_string),
            tool_calls: self.uses as u32,
            advancer: self.advancer.into(),
            decision: self.decision.label(),
        }))
    }

    fn span(&self, trace: &mut Trace) {
        trace.record(
            "advancer",
            "advancer",
            self.a0,
            trace.now_us(),
            json!({"advancer": self.advancer, "decision": self.decision.label()}),
        );
        trace.exit(json!({"decision": self.decision.label(), "usage": self.usage}));
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let i = self.outcome.loop_index;
        match self.decision {
            Decision::Continue if self.retry => {}
            Decision::Continue => say.line(
                Loop,
                format!(
                    "Loop {}: continuing, because the model asked for {} and {}.",
                    i + 1,
                    narrative::count(self.uses as u64, "tool", "tools"),
                    if self.answered == 1 {
                        "it has an answer"
                    } else {
                        "each has an answer"
                    }
                ),
            ),
            Decision::EndTurn(reason) if reason == "no_tool_calls" && self.uses > 0 => say.line(
                Loop,
                format!(
                    "Stopping: none of the model's {} ran.",
                    narrative::count(self.uses as u64, "call", "calls")
                ),
            ),
            Decision::EndTurn(reason) => say.line(
                Loop,
                format!("Stopping: {}.", narrative::end_phrase(reason)),
            ),
        }
    }
}

// ---------------------------------------------------------------- a model call

/// The model has no price in the catalog, so it is not called: budgets are
/// dollars (theseus-0sg), and nothing runs unpriced.
pub struct ModelUnpriced<'a> {
    pub model: &'a str,
}

impl Fact for ModelUnpriced<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "{} has no price in the catalog, so it is not called.",
                self.model
            ),
        );
    }
}

/// The kernel would not plan the model call (the execution was cancelled).
pub struct ModelNotPlanned<'a> {
    pub model: &'a str,
    pub error: &'a anyhow::Error,
}

impl Fact for ModelNotPlanned<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "The kernel would not plan the call to {}: {}.",
                self.model, self.error
            ),
        );
    }
}

/// The model call was planned with its reservation and dispatched, and it
/// begins: the outbox's span, and the call's own span opens.
pub struct ModelCalling<'a> {
    pub target: &'a Target,
    pub action: &'a Action,
    /// The reservation: the whole, and the output cap's and the input
    /// estimate's parts.
    pub reserve: Micros,
    pub output_micros: Micros,
    pub input_micros: Micros,
    pub est_tokens: u64,
    pub digest: &'a str,
    /// When its plan began, on the trace's clock.
    pub o0: u64,
}

impl Fact for ModelCalling<'_> {
    fn span(&self, trace: &mut Trace) {
        let (t, a) = (self.target, self.action);
        trace.record(
            "action.outbox",
            "store",
            self.o0,
            trace.now_us(),
            json!({"correlation_id": a.correlation_id, "tool": a.tool, "reserved_usd": micros_to_usd(self.reserve)}),
        );
        trace.enter(
            "provider.call",
            "provider",
            json!({"provider": t.provider, "model": t.model, "max_tokens": t.max_tokens, "digest": self.digest}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let t = self.target;
        say.line(
            Model,
            format!(
                "Calling {} on {}: reserving {} ({} for {} output tokens, {} for about {} input \
                 tokens).",
                t.model,
                t.provider,
                narrative::dollars(self.reserve),
                narrative::dollars(self.output_micros),
                narrative::thousands(t.max_tokens as u64),
                narrative::dollars(self.input_micros),
                narrative::thousands(self.est_tokens)
            ),
        );
    }
}

/// A piece of the model's answer as it streams (`model.delta`).
pub struct ModelDelta<'a> {
    pub turn_id: &'a str,
    pub loop_index: u32,
    pub text: &'a str,
}

impl Fact for ModelDelta<'_> {
    const METHOD: Option<&'static str> = Some(notify::MODEL_DELTA);

    fn event(&self) -> Option<Event> {
        Some(Event::ModelDelta(theseus_protocol::ModelDelta {
            turn_id: self.turn_id.into(),
            loop_index: self.loop_index,
            text: self.text.into(),
        }))
    }
}

/// A piece of the model's thinking summary as it streams (`model.thinking`).
pub struct ModelThinking<'a> {
    pub turn_id: &'a str,
    pub loop_index: u32,
    pub text: &'a str,
}

impl Fact for ModelThinking<'_> {
    const METHOD: Option<&'static str> = Some(notify::MODEL_THINKING);

    fn event(&self) -> Option<Event> {
        Some(Event::ModelThinking(theseus_protocol::ModelDelta {
            turn_id: self.turn_id.into(),
            loop_index: self.loop_index,
            text: self.text.into(),
        }))
    }
}

/// The provider answered: the call's span closes with what it said, after
/// its first byte and first token, and the loop's whole text goes to the
/// session's clients at once (`model.answered`, theseus-ck0n), before the
/// settle's frame, so a place shows it without waiting on that sync.
pub struct ModelAnswered<'a> {
    pub resp: &'a ModelResponse,
    /// When the call's span opened, on the trace's clock.
    pub call_t0: u64,
    pub turn_id: &'a str,
    pub loop_index: u32,
}

impl Fact for ModelAnswered<'_> {
    const METHOD: Option<&'static str> = Some(notify::MODEL_ANSWERED);

    fn event(&self) -> Option<Event> {
        Some(Event::ModelAnswered(theseus_protocol::ModelDelta {
            turn_id: self.turn_id.into(),
            loop_index: self.loop_index,
            text: self.resp.text.clone(),
        }))
    }

    fn span(&self, trace: &mut Trace) {
        let resp = self.resp;
        if let Some(fb) = resp.timing.first_byte_ms {
            trace.mark_at(self.call_t0 + fb * 1000, "first_byte", "mark", Value::Null);
        }
        if let Some(ft) = resp.timing.first_token_ms {
            trace.mark_at(self.call_t0 + ft * 1000, "first_token", "mark", Value::Null);
        }
        trace.exit(json!({
            "request_id": resp.request_id,
            "served_model": resp.model,
            "usage": resp.usage,
            "stop_reason": resp.stop_reason,
            "blocks": resp.content.len(),
            "output_chars": resp.text.chars().count(),
            "rate_limit_tokens_remaining": resp.rate_limit.tokens_remaining,
        }));
    }
}

/// The model call settled with its answer (`provider.call`): its row rides
/// in its completion's frame, after the assistant node (theseus-qa0).
pub struct ProviderCall<'a> {
    pub loop_index: u32,
    pub provider: &'a str,
    pub resp: &'a ModelResponse,
    /// What the catalog prices it at; `None`, a price the catalog lacks.
    pub cost_usd: Option<f64>,
    pub catalog_version: &'a str,
    pub node_id: &'a str,
    /// The call's action, and what the budget settled at.
    pub correlation_id: &'a str,
    pub settled_micros: Micros,
    /// When its settle began, on the trace's clock.
    pub s0: u64,
}

impl Fact for ProviderCall<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProviderCall);

    fn row(&self) -> Value {
        let resp = self.resp;
        json!({
            "loop": self.loop_index,
            "provider": self.provider,
            "model": resp.model,
            "request_id": resp.request_id,
            "usage": resp.usage,
            "cost_usd": self.cost_usd,
            "catalog_version": self.catalog_version,
            "timing": resp.timing,
            "rate_limit": resp.rate_limit,
            "stop_reason": resp.stop_reason,
            "stop_details": resp.stop_details,
            "blocks": resp.content.len(),
            "input_transformations": resp.input_transformations,
            "node_id": self.node_id,
        })
    }

    fn span(&self, trace: &mut Trace) {
        trace.record(
            "action.settle",
            "store",
            self.s0,
            trace.now_us(),
            json!({"correlation_id": self.correlation_id, "outcome": "succeeded", "cost_usd": micros_to_usd(self.settled_micros), "node_id": self.node_id}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let resp = self.resp;
        say.line(
            Model,
            format!(
                "{} answered in {}{}: {} in{}, {} out, {}; {}.",
                resp.model,
                narrative::duration(resp.timing.total_ms),
                resp.timing
                    .first_token_ms
                    .map(|ms| format!(" (first token {})", narrative::duration(ms)))
                    .unwrap_or_default(),
                narrative::count(
                    resp.usage.input_tokens
                        + resp.usage.cache_read_input_tokens
                        + resp.usage.cache_creation_input_tokens,
                    "token",
                    "tokens"
                ),
                if resp.usage.cache_read_input_tokens > 0 {
                    format!(
                        " ({} from the cache)",
                        narrative::thousands(resp.usage.cache_read_input_tokens)
                    )
                } else {
                    String::new()
                },
                narrative::thousands(resp.usage.output_tokens),
                narrative::money(self.cost_usd),
                narrative::stop_phrase(resp.stop_reason.as_deref(), resp.tool_uses().len())
            ),
        );
    }
}

/// The model refused (`provider.refusal`): the turn ends, or goes once to
/// the model's fallback (`FellBack`, theseus-7gir.18).
pub struct ProviderRefused<'a> {
    pub resp: &'a ModelResponse,
}

impl Fact for ProviderRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProviderRefusal);

    fn row(&self) -> Value {
        json!({"stop_details": self.resp.stop_details, "model": self.resp.model})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let category = self
            .resp
            .stop_details
            .as_ref()
            .and_then(|d| d["category"].as_str());
        say.line(
            Model,
            match category {
                Some(c) => format!("{} refused ({c}).", self.resp.model),
                None => format!("{} refused.", self.resp.model),
            },
        );
    }
}

/// A refused request goes once to its model's fallback, and the rest of the
/// turn runs there (`provider.fallback`, theseus-7gir.18): the model that
/// refused, the one it goes to, the refusal's category, and the refused
/// answer, which the fallback's requests leave out.
pub struct FellBack<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub category: Option<&'a str>,
    pub loop_index: Option<u32>,
    pub refused: &'a str,
}

impl Fact for FellBack<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProviderFallback);

    fn row(&self) -> Value {
        json!({"from": self.from, "to": self.to, "category": self.category, "loop": self.loop_index, "refused": self.refused})
    }

    fn span(&self, trace: &mut Trace) {
        let attrs = json!({"from": self.from, "to": self.to, "category": self.category});
        trace.mark("fallback", "mark", attrs);
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "The request goes to {} instead, once, by the same provider, and the rest of \
                 the turn runs there ([model.retries] refusal).",
                self.to
            ),
        );
    }
}

/// The loop's call did not fit under the spend limit, and the turn asks the
/// operator to reset the spend (theseus-0sg): the question goes where a tool
/// approval goes.
pub struct BudgetAsked<'a> {
    pub request: &'a ConfirmRequest,
    pub session_id: &'a str,
    pub model: &'a str,
    pub limit: Micros,
    pub spent: Micros,
    pub needed: Micros,
    pub available: Micros,
}

impl Fact for BudgetAsked<'_> {
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_REQUESTED);

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmRequested(self.request.clone()))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Session {} reached its {} limit: it has spent {}, and the call to {} needs {} \
                 with {} left; waiting for the operator to reset it.",
                narrative::short(self.session_id),
                narrative::dollars(self.limit),
                narrative::dollars(self.spent),
                self.model,
                narrative::dollars(self.needed),
                narrative::dollars(self.available)
            ),
        );
    }
}

/// The loop that asked about the budget ended on the question, before its
/// call: its clients hear it end, and its `loop.ended` row (theseus-nhg4) has
/// `LoopEnded`'s shape, so the ledger's readers read it. It made no call, so
/// its outcome has no stop reason, no calls, and no output, and its usage is
/// zero; its advancer and its decision are `budget`. `LoopCut` closes its span.
pub struct LoopEndedOnBudget<'a> {
    pub turn_id: &'a str,
    pub index: u32,
}

impl Fact for LoopEndedOnBudget<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LoopEnded);
    const METHOD: Option<&'static str> = Some(notify::LOOP_ENDED);

    fn row(&self) -> Value {
        let outcome = LoopOutcome {
            loop_index: self.index,
            provider_stop_reason: None,
            tool_calls: 0,
            output_chars: 0,
        };
        json!({"loop": self.index, "outcome": outcome, "advancer": "budget", "decision": {"decision": "budget"}, "usage": Usage::default()})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::LoopEnded(theseus_protocol::LoopEnded {
            turn_id: self.turn_id.into(),
            loop_index: self.index,
            provider_stop_reason: None,
            tool_calls: 0,
            advancer: "budget".into(),
            decision: "budget".into(),
        }))
    }
}

/// The retry of a call the operator approved a reset for still does not fit,
/// since it alone needs more than the whole limit (theseus-kks), or more than
/// the limit leaves once what a reset keeps held is taken (theseus-6g6): the
/// turn ends (`budget.over_limit`).
pub struct OverLimit<'a> {
    /// What the turn's failure says, figures and remedies included.
    pub message: &'a str,
    pub target: &'a Target,
    pub needed: Micros,
    pub limit: Micros,
    /// Reserved for calls in flight and held for calls whose cost is unknown,
    /// which a reset leaves as it is.
    pub held: Micros,
}

impl Fact for OverLimit<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::BudgetOverLimit);

    fn row(&self) -> Value {
        let t = self.target;
        json!({"model": t.model, "profile": t.profile, "max_output_tokens": t.max_tokens, "needed_usd": micros_to_usd(self.needed), "limit_usd": micros_to_usd(self.limit), "held_usd": micros_to_usd(self.held)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!("Still over the limit after the reset: {}.", self.message),
        );
    }
}

/// An image the provider refused is not shown from now on (theseus-0s4):
/// one row each (`image.not_shown`).
pub struct ImageNotShown<'a> {
    pub refused: &'a crate::attach::Refused,
    pub target: &'a Target,
}

impl Fact for ImageNotShown<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImageNotShown);

    fn row(&self) -> Value {
        let r = self.refused;
        json!({"digest": r.digest, "message_index": r.message, "why": r.why,
               "provider": self.target.provider, "model": self.target.model})
    }
}

/// The provider refused images, and the call is made again with their lines.
pub struct ImagesHidden<'a> {
    pub model: &'a str,
    pub count: usize,
    /// Why the first was refused.
    pub why: &'a str,
}

impl Fact for ImagesHidden<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let (it, line) = match self.count {
            1 => ("it is", "its line"),
            _ => ("they are", "their lines"),
        };
        say.line(
            Model,
            format!(
                "{} refused {} ({}): from now on {it} not shown, and the call is made again \
                 with {line}.",
                self.model,
                narrative::count(self.count as u64, "image", "images"),
                self.why
            ),
        );
    }
}

/// The provider said a request passed the model's window (theseus-9p88):
/// the first time, the next loop recompiles with a ring and calls once more
/// (`context.overflow`).
pub struct ContextOverflow<'a> {
    pub loop_index: u32,
    pub(crate) overflow: &'a Overflowing,
    /// The request was that retry: the turn fails instead.
    pub retried: bool,
    pub model: &'a str,
}

impl Fact for ContextOverflow<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextOverflow);

    fn row(&self) -> Value {
        let o = self.overflow;
        json!({"loop": self.loop_index, "source": o.source, "provider_tokens": o.hint.counted,
               "provider_maximum": o.hint.maximum, "output_tokens": o.output,
               "window": o.window, "estimate": o.hint.estimated,
               "then": if self.retried { "fail" } else { "ring" },
               "node_id": o.hint.retrying})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        if self.retried {
            return;
        }
        let o = self.overflow;
        say.line(
            Context,
            format!(
                "Context: {}'s request passed its window: {}; Theseus estimated {} tokens \
                 against {}. The context is recompiled with a ring{}, to call once more.",
                self.model,
                o.said(),
                narrative::thousands(o.hint.estimated),
                o.window_words(),
                if o.source == "cut" {
                    " that leaves the cut answer out"
                } else {
                    ""
                }
            ),
        );
    }
}

/// A request past the window that the turn's ring cannot fit: the turn
/// fails (theseus-9p88).
pub struct WindowFailed {
    /// The retry passed the window too; otherwise the ring could drop
    /// nothing.
    pub retried: bool,
}

impl Fact for WindowFailed {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "Context: {}, so the turn fails.",
                if self.retried {
                    "the retry passed the window too"
                } else {
                    "the ring could drop nothing earlier in the session, and the same request \
                     would pass the window again"
                }
            ),
        );
    }
}

/// The provider call failed: its span closes with the error.
pub struct ModelCallFailed<'a> {
    pub class: &'a str,
    pub message: String,
}

impl Fact for ModelCallFailed<'_> {
    fn span(&self, trace: &mut Trace) {
        trace.exit(json!({"error": self.class, "message": self.message}));
    }
}

/// The failed call settled, as failed or, when the provider may have done
/// the work, as unknown (`provider.error`).
pub struct ProviderError<'a> {
    pub loop_index: u32,
    pub target: &'a Target,
    pub class: &'a str,
    pub transient: bool,
    pub unknown: bool,
    pub elapsed_ms: u64,
    pub detail: Option<Value>,
    pub message: String,
    pub correlation_id: &'a str,
    pub reserved_micros: Micros,
    /// What the kernel's settle returned, as the span says it.
    pub settled: String,
    /// When its settle began, on the trace's clock.
    pub s0: u64,
}

impl Fact for ProviderError<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProviderError);

    fn row(&self) -> Value {
        json!({
            "loop": self.loop_index,
            "provider": self.target.provider,
            "model": self.target.model,
            "class": self.class,
            "transient": self.transient,
            "usage_unknown": self.unknown,
            "elapsed_ms": self.elapsed_ms,
            "detail": self.detail,
            "message": self.message,
        })
    }

    fn span(&self, trace: &mut Trace) {
        trace.record(
            "action.settle",
            "store",
            self.s0,
            trace.now_us(),
            json!({"correlation_id": self.correlation_id, "outcome": if self.unknown {"unknown"} else {"failed"}, "result": self.settled}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "{} failed after {}: {}{}; {}.",
                self.target.model,
                narrative::duration(self.elapsed_ms),
                self.class,
                if self.transient { " (transient)" } else { "" },
                if self.unknown {
                    format!(
                        "whether the provider did the work is unknown, so its reservation of {} \
                         stays held",
                        narrative::dollars(self.reserved_micros)
                    )
                } else {
                    "the call is settled as failed".into()
                }
            ),
        );
    }
}

/// A call that failed with a class that passes with time is made again
/// inside its turn, after its backoff (`[model.retries]`, theseus-7gir.21):
/// the wait, as a span, and a line. The failed call is its `provider.error`
/// row; the retry is the loop's next call.
pub struct ModelRetried<'a> {
    pub model: &'a str,
    pub class: &'a str,
    /// This retry, from 1, and the most the config allows.
    pub retry: u32,
    pub of: u32,
    /// When the wait began, on the trace's clock, and how long it was.
    pub w0: u64,
    pub waited_ms: u64,
}

impl Fact for ModelRetried<'_> {
    fn span(&self, trace: &mut Trace) {
        trace.record(
            "retry",
            "wait",
            self.w0,
            trace.now_us(),
            json!({"class": self.class, "retry": self.retry, "of": self.of}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "{} is called again after {} ({}: retry {} of {}, [model.retries]).",
                self.model,
                narrative::duration(self.waited_ms),
                self.class,
                self.retry,
                self.of
            ),
        );
    }
}

/// A `/stop` cut the model's call while its stream ran (theseus-yey): the call
/// settled as failed at an estimate of what it used (`provider.cut`). The
/// estimate is the input the call's reservation assumed and the output
/// counted from the characters streamed; a stop before anything was sent used
/// nothing.
pub struct ModelCut<'a> {
    pub loop_index: u32,
    pub target: &'a Target,
    pub correlation_id: &'a str,
    pub by: &'a str,
    /// The request had gone out.
    pub sent: bool,
    /// The estimated usage, which the settle booked.
    pub usage: &'a theseus_protocol::Usage,
    pub output_chars: u64,
    /// What the settle booked, and what the call reserved.
    pub cost: Micros,
    pub reserved_micros: Micros,
    pub elapsed_ms: u64,
    /// What the kernel's settle returned, as the span says it.
    pub settled: String,
    /// When its settle began, on the trace's clock.
    pub s0: u64,
}

impl Fact for ModelCut<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProviderCut);

    fn row(&self) -> Value {
        json!({
            "loop": self.loop_index,
            "provider": self.target.provider,
            "model": self.target.model,
            "by": self.by,
            "estimated": true,
            "sent": self.sent,
            "input_tokens": self.usage.input_tokens,
            "output_tokens": self.usage.output_tokens,
            "output_chars": self.output_chars,
            "cost_usd": micros_to_usd(self.cost),
            "reserved_usd": micros_to_usd(self.reserved_micros),
            "elapsed_ms": self.elapsed_ms,
        })
    }

    fn span(&self, trace: &mut Trace) {
        trace.record(
            "action.settle",
            "store",
            self.s0,
            trace.now_us(),
            json!({"correlation_id": self.correlation_id, "outcome": "failed", "result": self.settled}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "{} was cut by a stop from {} after {}: {}, booked as an estimate of {}, and its \
                 reservation of {} released.",
                self.target.model,
                self.by,
                narrative::duration(self.elapsed_ms),
                if self.sent {
                    format!(
                        "{} streamed",
                        narrative::count(self.output_chars, "character", "characters")
                    )
                } else {
                    "nothing had been sent".to_string()
                },
                narrative::dollars(self.cost),
                narrative::dollars(self.reserved_micros)
            ),
        );
    }
}

// ---------------------------------------------------------------- the turn's end

/// A node the turn wrote, told to the session's clients (`node.written`),
/// so their history stays live.
pub struct NodeWritten<'a> {
    pub session_id: &'a str,
    pub node: &'a Node,
}

impl Fact for NodeWritten<'_> {
    const METHOD: Option<&'static str> = Some(notify::NODE_WRITTEN);

    fn event(&self) -> Option<Event> {
        Some(Event::NodeWritten(theseus_protocol::NodeWritten {
            session_id: self.session_id.into(),
            node_id: self.node.id.clone(),
            kind: self.node.kind_str().into(),
        }))
    }
}

/// A new compilation, written with the session's pointer to it in one
/// frame (`context.recompiled`).
pub struct ContextRecompiled<'a> {
    pub compilation: &'a Compilation,
}

impl Fact for ContextRecompiled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextRecompiled);

    fn row(&self) -> Value {
        let c = self.compilation;
        json!({"compilation_id": c.id, "trigger": c.trigger, "strategy": c.strategy, "as_of": c.as_of, "includes": c.includes.len(), "derived_from": c.derived_from, "strip_thinking": c.manifest.strip_thinking, "model": c.manifest.model})
    }
}

/// The turn failed after it began (`turn.failed`): what its finished loops
/// spent is booked.
pub struct TurnFailed<'a> {
    pub turn_id: &'a str,
    pub class: &'a str,
    /// The row's reason.
    pub reason: &'a str,
    /// Loops begun, the failed one included.
    pub loops: u32,
    pub usage: &'a Usage,
    pub cost_usd: Option<f64>,
    pub tool_calls: u32,
}

impl Fact for TurnFailed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnFailed);

    fn row(&self) -> Value {
        json!({"loops": self.loops, "reason": self.reason, "usage_so_far": self.usage, "cost_usd": self.cost_usd, "tool_calls": self.tool_calls})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let finished = self.loops.saturating_sub(1) as u64;
        if self.loops == 0 {
            // A fault before the turn's first loop (R1).
            say.line(
                Turn,
                format!(
                    "Turn {} failed ({}) before its first loop; it spent {}.",
                    narrative::short(self.turn_id),
                    self.class,
                    narrative::money(self.cost_usd)
                ),
            );
        } else if finished == 0 {
            say.line(
                Turn,
                format!(
                    "Turn {} failed ({}) in loop {}, before any loop finished; it spent {}.",
                    narrative::short(self.turn_id),
                    self.class,
                    self.loops,
                    narrative::money(self.cost_usd)
                ),
            );
        } else {
            say.line(
                Turn,
                format!(
                    "Turn {} failed ({}) in loop {}, after {}; {} spent {}: {}, {}.",
                    narrative::short(self.turn_id),
                    self.class,
                    self.loops,
                    narrative::count(finished, "finished loop", "finished loops"),
                    if finished == 1 {
                        "that loop"
                    } else {
                        "those loops"
                    },
                    narrative::money(self.cost_usd),
                    narrative::count(self.usage.output_tokens, "token out", "tokens out"),
                    narrative::count(self.tool_calls as u64, "tool call", "tool calls")
                ),
            );
        }
    }
}

/// The turn's books, as its `turn.ended` row: loops, usage, cost, and tool
/// calls, with the session's whole usage. Its session write's span is the
/// one before it.
pub struct TurnBooked<'a> {
    pub result: &'a TurnSubmitResult,
    pub session_usage: &'a Usage,
    /// Results that settled while it ran, written as late result nodes.
    pub late: u32,
    /// When the session's write began, on the trace's clock.
    pub w0: u64,
}

impl Fact for TurnBooked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnEnded);

    fn row(&self) -> Value {
        let result = self.result;
        let mut row = json!({"loops": result.loops, "stop_reason": result.stop_reason, "usage": result.usage, "cost_usd": result.cost_usd, "tool_calls": result.tool_calls, "session_usage": self.session_usage, "elapsed_ms": result.elapsed_ms, "first_token_ms": result.first_token_ms, "provider": result.provider, "model": result.model, "awaiting_confirm": result.awaiting_confirm, "continuation": result.continuation, "late_results": self.late});
        // A refusal's fallback (theseus-7gir.18): only a turn that had one says.
        if let Some(f) = &result.fallback {
            row["fallback"] = json!(f);
        }
        row
    }

    fn span(&self, trace: &mut Trace) {
        trace.record(
            "session.write",
            "store",
            self.w0,
            trace.now_us(),
            Value::Null,
        );
    }
}

/// The turn ended: its trace is written (`turn.trace`), its clients get its
/// result (`turn.ended`, the trace on it), and the narrative says what it
/// did.
pub struct TurnEnded<'a> {
    /// The result, its trace finished.
    pub result: &'a TurnSubmitResult,
}

impl Fact for TurnEnded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnTrace);
    const METHOD: Option<&'static str> = Some(notify::TURN_ENDED);

    fn row(&self) -> Value {
        serde_json::to_value(&self.result.trace).unwrap_or(Value::Null)
    }

    fn event(&self) -> Option<Event> {
        Some(Event::TurnEnded(self.result.clone()))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let result = self.result;
        say.line(
            Turn,
            format!(
                "Turn {} ended after {} in {}: {}, {}, {}; {}.",
                narrative::short(&result.turn_id),
                narrative::count(result.loops as u64, "loop", "loops"),
                narrative::duration(result.elapsed_ms),
                narrative::count(result.tool_calls as u64, "tool call", "tool calls"),
                narrative::count(result.usage.output_tokens, "token out", "tokens out"),
                narrative::money(result.cost_usd),
                narrative::end_phrase(&result.stop_reason)
            ),
        );
    }
}

// ---------------------------------------------------------------- after a failed turn

/// The session's run of failures, extended by a failed turn, and what the
/// rule says comes next (`turn.next`, theseus-ljr): its row rides with the
/// run's record in one frame.
pub struct TurnNext<'a> {
    pub then: Then,
    pub class: &'a str,
    pub transient: bool,
    pub settled: bool,
    pub run: &'a Failing,
    /// The run's notice goes out with this failure.
    pub notice: bool,
}

impl Fact for TurnNext<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnNext);

    fn row(&self) -> Value {
        let run = self.run;
        json!({"then": self.then, "class": self.class, "transient": self.transient, "settled": self.settled,
               "turns": run.turns, "lasting": run.lasting, "notice": self.notice,
               "since_ms": run.since_ms})
    }
}

/// A turn a `/stop` ended at its next step, which the stop refused (W1).
pub struct StoppedAtStep<'a> {
    pub by: &'a str,
}

impl Fact for StoppedAtStep<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Turn,
            format!(
                "Stopped by {}, the turn ended at its next step, which the stop refused.",
                self.by
            ),
        );
    }
}

/// A turn stopped on a fault, not a failure it reports itself.
pub struct TurnFaulted;

impl Fact for TurnFaulted {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Turn,
            "The turn stopped on an internal error; the log has it.".into(),
        );
    }
}

/// A failed turn, told to its clients (`turn.failed`): the requester also
/// gets the error response; watchers get only this.
pub struct TurnFailureTold<'a> {
    pub failed: &'a theseus_protocol::TurnFailed,
}

impl Fact for TurnFailureTold<'_> {
    const METHOD: Option<&'static str> = Some(notify::TURN_FAILED);

    fn event(&self) -> Option<Event> {
        Some(Event::TurnFailed(self.failed.clone()))
    }
}

/// A background result landed while the turn ran, which the model has not
/// read: the driver takes another turn at once.
pub struct WokenAgain;

impl Fact for WokenAgain {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            "Woken again at once: a background result landed during the turn.".into(),
        );
    }
}

/// What the run of failures decided (theseus-ljr): a retry with backoff, one
/// retry, or the next message.
pub struct RetryDecided<'a> {
    pub run: &'a Failing,
    pub then: Then,
    /// The execution was woken for the retry.
    pub woke: bool,
    /// A result the turn reads itself settled before it failed.
    pub settled: bool,
}

impl Fact for RetryDecided<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let f = self.run;
        match self.then {
            Then::Backoff if self.woke => say.line(
                Session,
                format!(
                    "Woken again: {} passes with time, so the driver retries with its backoff \
                     ({} in a row).",
                    f.class,
                    narrative::count(f.turns as u64, "failed turn", "failed turns")
                ),
            ),
            Then::Retry if self.woke => say.line(
                Session,
                format!(
                    "Woken for one retry: {} will not pass by waiting, but a config or profile \
                     change may have cured it.",
                    f.class
                ),
            ),
            Then::Park => say.line(
                Session,
                format!(
                    "Not retried: {} ({}), so the session waits on its next message, which \
                     retries.",
                    f.class,
                    if f.class == WINDOW_CLASS {
                        "the same request would pass the window again"
                    } else if f.class == crate::turn::compaction::OVERAGE_CLASS {
                        "the same exchange would not fit the window again"
                    } else if self.settled {
                        "it failed again after its retry"
                    } else {
                        "it failed before any provider call returned"
                    }
                ),
            ),
            _ => {}
        }
    }
}
