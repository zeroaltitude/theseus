//! Facts (theseus-j6qn, Review 2's C2): one typed fact for each thing the
//! core records, recorded once and projected to every channel that carries
//! it.
//!
//! Before, a site wrote one fact by hand into each of its channels: a ledger
//! row, a notification, a narrative sentence, and a span in the turn's
//! trace. Now the site builds the fact's type and records it, and the type
//! says in one place what each channel gets:
//!
//! - `KIND` and `row`: its ledger row, when it has one;
//! - `METHOD` and `event`: its notification, when it has one;
//! - `narrate`: its sentences, made only when the narrative is on;
//! - `span`: its span in the turn's trace, or attributes on the open span.
//!
//! The recorder, [`Rec`], carries the site's context: whose the fact is (the
//! session, and the turn), who hears its notification (a turn's clients, a
//! session's watchers, or every connection), and where its row goes (a
//! turn's handle holds it for the turn's next frame; any other writes it in
//! a frame now). A fact whose row rides in a frame the site builds itself (a
//! completion's, a kernel transaction's) takes its row as a record
//! ([`Rec::row`]) and announces the rest once that frame is written
//! ([`Rec::announce`]). Recording writes no frame of its own, so a fact
//! lands in the frame it landed in when its site wrote it by hand.
//!
//! The metrics are a projection of the turn's end: `Telemetry::record_turn`
//! and `record_failure` make them from the turn's result and its trace, the
//! spans the facts recorded, where the result lands, once a turn.
//!
//! Every fact is listed in [`FACTS`] with its kind and its method, so a test
//! can list them all. A kind is a `theseus_protocol::LedgerKind`, the
//! registry every row writer in the workspace names.

use serde_json::Value;
use theseus_protocol::{Event, LedgerKind, Message, NarrativePart};
use theseus_store::{kinds, NewRecord};

use crate::bus::{EventSink, SessionBus};
use crate::ledger::LedgerRow;
use crate::narrative::Narrator;
use crate::store::Store;
use crate::trace::Trace;

pub mod answer;
pub mod cancel;
pub mod driver;
pub mod durability;
pub mod index;
pub mod judge;
pub mod mcp;
pub mod ontology;
pub mod place;
pub mod recall;
pub mod sandbox;
pub mod start;
pub mod term;
pub mod tool;
pub mod turn;

/// A fact: what happened, and what each channel says of it.
pub trait Fact {
    /// Its ledger row's kind; `None`, no row.
    const KIND: Option<LedgerKind> = None;
    /// Its notification's method; `None`, no notification.
    const METHOD: Option<&'static str> = None;

    /// The row's data. Read only when `KIND` is set.
    fn row(&self) -> Value {
        Value::Null
    }

    /// The notification, whose method is `METHOD`. Read only when `METHOD`
    /// is set.
    fn event(&self) -> Option<Event> {
        None
    }

    /// Its sentences, each in its part of the narrative. Called only when
    /// narration is on, so nothing is formatted otherwise.
    fn narrate(&self, _say: &mut Say<'_>) {}

    /// Its span in the turn's trace, or its attributes on the span open
    /// now. A turn's recorder calls it (`Turn::record`); a fact recorded
    /// outside a turn has no trace.
    fn span(&self, _trace: &mut Trace) {}
}

/// Who hears a fact's notification.
#[derive(Clone, Copy)]
pub enum To<'a> {
    /// A turn's clients: the connection that asked, and the session's
    /// watchers.
    Sink(&'a EventSink),
    /// A session's watchers.
    Session(&'a SessionBus, &'a str),
    /// Every connection that watches a session, once each.
    Everyone(&'a SessionBus),
    /// No one: the site's facts have no notification.
    Nobody,
}

impl To<'_> {
    /// Send a fact's notification alone: for a fact that is nothing else,
    /// told where no recorder is at hand (a stream's pieces).
    pub fn tell<F: Fact>(self, f: &F) {
        debug_assert!(F::KIND.is_none(), "a fact with a row is recorded, not told");
        if F::METHOD.is_some() {
            if let Some(e) = f.event() {
                debug_assert_eq!(Some(e.method()), F::METHOD, "a fact sends its METHOD");
                self.send(e);
            }
        }
    }

    fn send(self, e: Event) {
        match self {
            To::Sink(sink) => sink.send(e),
            To::Session(bus, session) => bus.publish(session, &Message::from(e), None),
            To::Everyone(bus) => bus.publish_all(&Message::from(e)),
            To::Nobody => debug_assert!(false, "a notification with no one to hear it"),
        }
    }
}

/// A fact's row as a record, for a frame the caller builds, whose session
/// and turn are given.
pub fn row<F: Fact>(f: &F, session: Option<&str>, turn: Option<&str>) -> anyhow::Result<NewRecord> {
    let kind = F::KIND.ok_or_else(|| anyhow::anyhow!("this fact writes no ledger row"))?;
    NewRecord::json(
        kinds::LEDGER,
        None,
        &LedgerRow::new(kind, session, turn, f.row()),
    )
}

/// Where a fact is recorded: its site's context.
#[derive(Clone, Copy)]
pub struct Rec<'a> {
    pub narrator: &'a Narrator,
    /// Whose it is: its row's and its sentences' session and turn.
    pub session: Option<&'a str>,
    pub turn: Option<&'a str>,
    /// Who hears its notification.
    pub to: To<'a>,
    /// Where its row goes: a turn's handle holds it for the turn's next
    /// frame (theseus-qa0); another writes it in a frame now.
    pub store: &'a Store,
}

impl<'a> Rec<'a> {
    /// Record a fact on each of its channels. A row that cannot be written
    /// is logged, not fatal: it is no state transition.
    pub fn record<F: Fact>(&self, f: &F) {
        if F::KIND.is_some() {
            if let Err(e) = self.row(f).and_then(|r| self.store.defer(r)) {
                tracing::warn!(error = %e, "ledger append failed");
            }
        }
        self.announce(f);
    }

    /// The fact's row as a record, for a frame the caller builds; its other
    /// channels follow with `announce`, once that frame is written.
    pub fn row<F: Fact>(&self, f: &F) -> anyhow::Result<NewRecord> {
        row(f, self.session, self.turn)
    }

    /// The fact's notification and its sentences: every channel but its row.
    pub fn announce<F: Fact>(&self, f: &F) {
        if F::METHOD.is_some() {
            if let Some(e) = f.event() {
                debug_assert_eq!(Some(e.method()), F::METHOD, "a fact sends its METHOD");
                self.to.send(e);
            }
        }
        if self.narrator.on() {
            f.narrate(&mut Say { rec: self });
        }
    }

    /// The same context, as another turn's (`None`: the session's own).
    pub fn in_turn(self, turn: Option<&'a str>) -> Self {
        Rec { turn, ..self }
    }
}

/// Where a fact's sentences go: the narrative, as its recorder's session and
/// turn.
pub struct Say<'a> {
    rec: &'a Rec<'a>,
}

impl Say<'_> {
    /// One sentence, in `part`.
    pub fn line(&mut self, part: NarrativePart, text: String) {
        self.rec
            .narrator
            .line(part, self.rec.session, self.rec.turn, text);
    }
}

/// A fact as `FACTS` lists it.
#[derive(Debug, Clone, Copy)]
pub struct Listed {
    pub name: &'static str,
    pub kind: Option<LedgerKind>,
    pub method: Option<&'static str>,
}

/// List every fact: its name, its kind, and its method.
macro_rules! facts {
    ($($t:ty),* $(,)?) => {
        /// Every fact the core records, with its ledger kind and its method.
        pub const FACTS: &[Listed] = &[$(Listed {
            name: stringify!($t),
            kind: <$t as Fact>::KIND,
            method: <$t as Fact>::METHOD,
        }),*];
    };
}

facts![
    turn::TurnStarted<'static>,
    turn::ExecutionOpened<'static>,
    turn::BudgetQuestionSuperseded<'static>,
    turn::TurnRefused<'static>,
    turn::TurnNotRunnable<'static>,
    turn::WokenByInput<'static>,
    turn::SessionResumed<'static>,
    turn::Parked<'static>,
    turn::CaughtUp<'static>,
    turn::Stopped<'static>,
    turn::WakeCameDue<'static>,
    turn::ReportWoke<'static>,
    turn::NoCall,
    turn::ContextFileMissing<'static>,
    turn::LoopOpened,
    turn::ContextCompiled<'static>,
    recall::RecallShadow<'static>,
    place::PlaceViewed<'static>,
    place::Published<'static>,
    turn::LoopStarted<'static>,
    turn::LoopCut<'static>,
    turn::LoopEnded<'static>,
    turn::ModelUnpriced<'static>,
    turn::ModelNotPlanned<'static>,
    turn::ModelCalling<'static>,
    turn::ModelDelta<'static>,
    turn::ModelThinking<'static>,
    turn::ModelAnswered<'static>,
    turn::ProviderCall<'static>,
    turn::ProviderRefused<'static>,
    turn::BudgetAsked<'static>,
    turn::LoopEndedOnBudget<'static>,
    turn::OverLimit<'static>,
    turn::ImageNotShown<'static>,
    turn::ImagesHidden<'static>,
    turn::ContextOverflow<'static>,
    turn::WindowFailed,
    turn::ModelCallFailed<'static>,
    turn::ProviderError<'static>,
    turn::ModelCut<'static>,
    turn::NodeWritten<'static>,
    turn::ContextRecompiled<'static>,
    turn::TurnFailed<'static>,
    turn::TurnBooked<'static>,
    turn::TurnEnded<'static>,
    turn::TurnNext<'static>,
    turn::StoppedAtStep<'static>,
    turn::TurnFaulted,
    turn::TurnFailureTold<'static>,
    turn::WokenAgain,
    turn::RetryDecided<'static>,
    tool::ToolProposed<'static>,
    tool::GateDecided<'static>,
    tool::ToolNotified<'static>,
    tool::CallAsked<'static>,
    tool::UnknownTool<'static>,
    tool::InvalidJson<'static>,
    tool::InvalidInput<'static>,
    tool::ToolStarted<'static>,
    tool::JobStarted<'static>,
    tool::SecretGranted<'static>,
    tool::SecretWithheld<'static>,
    tool::SecretHanded<'static>,
    tool::AwsCalled<'static>,
    tool::AwsSessionMinted<'static>,
    tool::HoldTaken<'static>,
    tool::JobRefused<'static>,
    tool::JobNotStarted<'static>,
    tool::JobStoppedAtLaunch<'static>,
    tool::JobStoppedBelowFloor<'static>,
    tool::ToolEnded<'static>,
    tool::CallSuperseded<'static>,
    tool::CallNeverAsked<'static>,
    tool::ApprovedRunning<'static>,
    tool::ApprovalVoid<'static>,
    tool::AuthorizedResumed<'static>,
    tool::LateResult<'static>,
    tool::TaskStarted<'static>,
    tool::TaskHoldsExternal<'static>,
    tool::WakeSet<'static>,
    answer::CallAnswered<'static>,
    answer::WokenByAnswer,
    answer::BudgetAnswered<'static>,
    answer::SpendReset<'static>,
    answer::ResetDeclined<'static>,
    answer::ActRefused<'static>,
    answer::LimitChanged<'static>,
    answer::QuestionWithdrawn<'static>,
    answer::QuestionExpired<'static>,
    driver::HeartbeatActed<'static>,
    driver::SpooledCompletion<'static>,
    driver::QuestionCancelled<'static>,
    driver::ExecutionCancelled<'static>,
    driver::QuestionStopped<'static>,
    driver::ExecutionStopped<'static>,
    driver::WrapperLost<'static>,
    cancel::CancelVerified<'static>,
    cancel::CancelUnsupported<'static>,
    cancel::CancelUncertain<'static>,
    driver::DriverResumes<'static>,
    index::TenderStarted<'static>,
    index::TenderAdopted,
    index::TenderSettingsChanged,
    index::TenderExited<'static>,
    index::TenderStartFailed<'static>,
    durability::SegmentShipped<'static>,
    sandbox::SandboxStarted<'static>,
    sandbox::SandboxEgress<'static>,
    sandbox::SandboxEgressRefused<'static>,
    start::CrashFound<'static>,
    ontology::CategorySet<'static>,
    ontology::GuidanceSet<'static>,
    ontology::MembershipSet<'static>,
    judge::JudgeCall<'static>,
    judge::JudgePaused<'static>,
    judge::JudgeBlockBooked<'static>,
    judge::JudgeCircuit<'static>,
    judge::JudgeShed,
    mcp::McpStarted,
    mcp::McpReady,
    mcp::McpExited,
    mcp::McpFailed,
    mcp::McpToolsChanged,
    crate::aws::hands::group::Launched<'static>,
    crate::aws::hands::group::Settled<'static>,
    crate::aws::hands::poller::Quarantined<'static>,
    term::TermOpened<'static>,
    term::TermClosed<'static>,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Each fact is listed once, its kind is the registry's, and its method
    /// is a notification's.
    #[test]
    fn every_fact_is_listed_once_with_a_kind_and_a_method_that_exist() {
        let mut names: Vec<&str> = FACTS.iter().map(|f| f.name).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "a fact listed twice");
        for f in FACTS {
            if let Some(k) = f.kind {
                assert_eq!(
                    LedgerKind::parse(k.as_str()),
                    Some(k),
                    "{}: `{k}` is not a registry kind",
                    f.name
                );
            }
            if let Some(m) = f.method {
                assert!(
                    theseus_protocol::notify::ALL.contains(&m),
                    "{}: `{m}` is no notification",
                    f.name
                );
            }
        }
    }
}
