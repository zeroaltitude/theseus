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
pub mod arrangement;
pub mod cancel;
pub mod capture;
pub mod card;
pub mod check;
pub mod compaction;
pub mod content;
pub mod data;
pub mod detour;
pub mod disk;
pub mod driver;
pub mod durability;
pub mod extend;
pub mod extend_hands;
pub mod fan_out;
pub mod flow;
pub mod google;
pub mod import;
pub mod index;
pub mod initiative;
pub mod judge;
pub mod judge_runs;
pub mod ladder;
pub mod limits;
pub mod lsp;
pub mod mcp;
pub mod memory;
pub mod modes;
pub mod onto;
pub mod ontology;
pub mod ops;
pub mod own_bench;
pub mod people;
pub mod persona;
pub mod persona_import;
pub mod place;
pub mod prefix;
pub mod privacy;
pub mod progress;
pub mod recall;
pub mod retract;
pub mod retry;
pub mod route;
pub mod rsi;
pub mod rsi_backlog;
pub mod rsi_budget;
pub mod rsi_exams;
pub mod rsi_graduate;
pub mod rsi_join;
pub mod rsi_mine;
pub mod rsi_mint;
pub mod rsi_narrow;
pub mod rsi_pipeline;
pub mod rsi_pr;
pub mod rsi_revert;
pub mod rsi_sealed;
pub mod sandbox;
pub mod situation;
pub mod standing;
pub mod start;
pub mod steer;
pub mod synthesis;
pub mod task_graph;
pub mod task_pause;
pub mod task_start;
pub mod term;
pub mod thread;
pub mod thread_import;
pub mod thread_route;
pub mod thread_seal;
pub mod tool;
pub mod tools_defer;
pub mod turn;
pub mod wake;
pub mod watch;

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
            if let Some(mut e) = f.event() {
                debug_assert_eq!(Some(e.method()), F::METHOD, "a fact sends its METHOD");
                // A task's change names the turn that made it.
                if let Event::TaskChanged(c) = &mut e {
                    c.turn_id = self.turn.map(str::to_string);
                }
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
    own_bench::TurnReplayed<'static>,
    own_bench::RouteReplayJudged<'static>,
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
    wake::WakeFired<'static>,
    watch::WatchEnded<'static>,
    wake::RoutineNoticed<'static>,
    turn::ReportWoke<'static>,
    turn::NoCall,
    turn::ContextFileMissing<'static>,
    turn::LoopOpened,
    turn::ContextCompiled<'static>,
    compaction::Compacted<'static>,
    compaction::Overage<'static>,
    compaction::Cleared<'static>,
    recall::RecallShadow<'static>,
    recall::RecallRan<'static>,
    recall::ArmAssigned<'static>,
    recall::Labeled<'static>,
    retract::Retracted<'static>,
    retract::Corrected<'static>,
    memory::MemoryLabeled<'static>,
    memory::MemoryGated<'static>,
    memory::MemoryUsed<'static>,
    memory::MemoryRead<'static>,
    memory::MemoryCited<'static>,
    place::PlaceViewed<'static>,
    place::PlaceWarned<'static>,
    privacy::PrivacyWarned<'static>,
    content::ContentJudged<'static>,
    content::ContentLabelled<'static>,
    content::ContentRemoved<'static>,
    content::ContentUnjudged<'static>,
    data::DataGateBlocked<'static>,
    data::RequestRedacted<'static>,
    place::Published<'static>,
    place::GlidePosted<'static>,
    place::GlideRead<'static>,
    turn::LoopStarted<'static>,
    detour::Detoured<'static>,
    route::RouteDecided<'static>,
    route::RouteCorrected<'static>,
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
    turn::FellBack<'static>,
    turn::TurnNudged<'static>,
    turn::TurnEndNoted<'static>,
    progress::TurnProgress<'static>,
    steer::TurnSteered<'static>,
    fan_out::FanOutCue<'static>,
    fan_out::TaskWaitStarted<'static>,
    fan_out::TaskWaitEnded<'static>,
    turn::EffortRefused<'static>,
    turn::BudgetAsked<'static>,
    turn::LoopEndedOnBudget<'static>,
    limits::SpendReached<'static>,
    limits::LoopsReached<'static>,
    limits::DayCeilingReached<'static>,
    limits::DayCeilingWarned<'static>,
    turn::OverLimit<'static>,
    turn::ImageNotShown<'static>,
    turn::ImagesHidden<'static>,
    turn::ContextOverflow<'static>,
    turn::WindowFailed,
    turn::ModelCallFailed<'static>,
    turn::ProviderError<'static>,
    turn::ModelRetried<'static>,
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
    google::GoogleCalled<'static>,
    google::GoogleAuthorized<'static>,
    google::GoogleAuthLapsed<'static>,
    google::GoogleRevoked<'static>,
    tool::JobRefused<'static>,
    tool::JobNotStarted<'static>,
    tool::JobStoppedAtLaunch<'static>,
    tool::JobStoppedBelowFloor<'static>,
    tool::ToolEnded<'static>,
    tool::CallSuperseded<'static>,
    tool::CallNeverAsked<'static>,
    tool::CallNotAsked<'static>,
    tool::ApprovedRunning<'static>,
    tool::ApprovalVoid<'static>,
    tool::AuthorizedResumed<'static>,
    tool::LateResult<'static>,
    tool::TaskStarted<'static>,
    task_start::TaskStartedByOwner<'static>,
    arrangement::TaskArranged<'static>,
    arrangement::TaskArrangementRefused<'static>,
    check::TaskCheckOpened<'static>,
    check::TaskCheckRefused<'static>,
    task_graph::TaskCreated<'static>,
    task_graph::TaskUpdated<'static>,
    task_graph::TaskSplit<'static>,
    task_graph::TaskClosed<'static>,
    task_graph::TaskChangeProposed<'static>,
    task_graph::TaskChangeAccepted<'static>,
    task_graph::TaskChangeDeclined<'static>,
    task_graph::TaskChangeExpired<'static>,
    task_graph::TaskStaleRefused<'static>,
    task_graph::TaskClaimed<'static>,
    task_graph::TaskLeaseExpired<'static>,
    tool::WakeSet<'static>,
    tool::WakeCancelled<'static>,
    tool::WakeMoved<'static>,
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
    disk::DiskLow<'static>,
    disk::DiskBelowFloor<'static>,
    disk::DiskOk<'static>,
    driver::SpooledCompletion<'static>,
    driver::QuestionCancelled<'static>,
    driver::ExecutionCancelled<'static>,
    driver::QuestionStopped<'static>,
    driver::ExecutionStopped<'static>,
    driver::ExecutionStoppedAll<'static>,
    driver::WrapperLost<'static>,
    driver::WrapperGone<'static>,
    cancel::CancelVerified<'static>,
    cancel::CancelUnsupported<'static>,
    cancel::CancelUncertain<'static>,
    driver::DriverResumes<'static>,
    index::TenderStarted<'static>,
    index::TenderAdopted,
    index::TenderSettingsChanged,
    index::TenderExited<'static>,
    index::TenderStartFailed<'static>,
    index::Rebuild<'static>,
    durability::SegmentShipped<'static>,
    lsp::LspStarted<'static>,
    lsp::LspReady<'static>,
    lsp::LspStopped<'static>,
    lsp::LspFailed<'static>,
    sandbox::SandboxStarted<'static>,
    sandbox::SandboxEgress<'static>,
    sandbox::SandboxEgressRefused<'static>,
    start::CrashFound<'static>,
    ontology::CategorySet<'static>,
    ontology::GuidanceSet<'static>,
    ontology::MembershipSet<'static>,
    ontology::KindSet<'static>,
    judge::JudgeCall<'static>,
    judge::JudgePaused<'static>,
    judge::JudgeResumed<'static>,
    judge::JudgeBlockBooked<'static>,
    judge::JudgeCircuit<'static>,
    judge::JudgeShed,
    judge::JudgeScored<'static>,
    judge::JudgeNotified<'static>,
    judge::JudgeNoticesPaused<'static>,
    judge::JudgeLabel<'static>,
    judge::ProposalLabel<'static>,
    judge::JudgeReport<'static>,
    judge_runs::JudgeReplayed<'static>,
    judge_runs::JudgeAudited<'static>,
    judge_runs::PackVersioned<'static>,
    judge_runs::JudgeProposed<'static>,
    judge_runs::JudgeBackfilled<'static>,
    synthesis::SynthesisProposed<'static>,
    synthesis::SynthesisChecked<'static>,
    synthesis::SynthesisScored<'static>,
    import::ImportBatch<'static>,
    import::ImportErased<'static>,
    import::ImportErasing<'static>,
    import::ImportSynced<'static>,
    import::ImportTopics<'static>,
    import::ImportPeople<'static>,
    people::PeopleExtracted<'static>,
    people::PeopleSwept<'static>,
    ontology::PersonMerged<'static>,
    ontology::OntologyUndo<'static>,
    onto::OntoFiled<'static>,
    onto::OntoNamed<'static>,
    ladder::PackModeSet<'static>,
    ladder::PackEventLanded<'static>,
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
    term::TermLeft<'static>,
    mcp::McpPromptChanged,
    crate::aws::hands::watch::HourAlert<'static>,
    crate::aws::month_warn::MonthWarned<'static>,
    crate::aws::hands::watch::ReaperFailed<'static>,
    extend::ExtendProposed<'static>,
    extend::ExtendTested<'static>,
    extend::ExtendAcked<'static>,
    extend::ExtendDeclined<'static>,
    extend::ExtendLoaded<'static>,
    extend::ExtendRevoked<'static>,
    flow::FlowLoaded<'static>,
    flow::FlowRetired<'static>,
    flow::FlowRan<'static>,
    retry::FailureDismissed<'static>,
    retry::TaskRetried<'static>,
    ops::SessionProfileSet<'static>,
    ops::TaskRehomed<'static>,
    ops::TaskRescheduled<'static>,
    task_pause::TaskPaused<'static>,
    task_pause::TaskResumed<'static>,
    rsi::SelfHalted<'static>,
    rsi::SelfResumed<'static>,
    rsi_budget::BudgetReserved<'static>,
    rsi_budget::BudgetSettled<'static>,
    rsi_budget::BudgetStopped<'static>,
    rsi_sealed::SelfExamCounted<'static>,
    rsi_sealed::SelfSealedScored<'static>,
    rsi_sealed::SelfSealedChecked<'static>,
    rsi_sealed::SelfSealedDrift<'static>,
    rsi_backlog::BacklogFiled,
    rsi_backlog::BacklogRanked,
    rsi_backlog::BacklogStarted,
    rsi_backlog::BacklogDropped,
    rsi_graduate::GraduateProposed,
    rsi_mine::MineRan,
    rsi_mint::MintValidated,
    rsi_mint::MintDeployed,
    rsi_mint::MintRetired,
    rsi_pr::SelfBranchBuilt<'static>,
    rsi_exams::ExamCounted<'static>,
    rsi_exams::ExamRejected<'static>,
    rsi_exams::ExamPass,
    rsi_join::SelfReviewVerdict,
    rsi_join::SelfJoined,
    rsi_join::SelfInstalled,
    rsi_join::SelfJoinStopped,
    rsi_narrow::NarrowProposed,
    rsi_narrow::NarrowApplied,
    rsi_narrow::NarrowDeclined,
    rsi_narrow::NarrowRed,
    rsi_narrow::NarrowReverted,
    persona::PersonaChanged<'static>,
    persona::PersonaMigrated<'static>,
    persona::PersonaSoul<'static>,
    rsi_revert::SelfReverted,
    rsi_revert::SelfVetoed,
    rsi_pipeline::Started,
    rsi_pipeline::Held,
    rsi_pipeline::Returned,
    rsi_pipeline::Parked,
    rsi_pipeline::Queued,
    rsi_pipeline::Cleared,
    rsi_pipeline::Ended,
    prefix::PrefixPicked<'static>,
    persona::PersonaEngaged<'static>,
    initiative::PromiseKept<'static>,
    initiative::PromiseUnkept<'static>,
    initiative::InitiativeSpoke<'static>,
    initiative::InitiativeRated<'static>,
    card::CardProposed<'static>,
    card::CardAnswered<'static>,
    card::CardObserved<'static>,
    persona_import::PersonaImported<'static>,
    persona_import::StandingProposalAnswered<'static>,
    standing::StandingAdded<'static>,
    modes::ModeChanged<'static>,
    modes::PlanMoved<'static>,
    standing::StandingRevoked<'static>,
    standing::StandingRepeated<'static>,
    capture::StandingCaptured<'static>,
    capture::StandingAsked<'static>,
    capture::StandingAnswered<'static>,
    capture::StandingCandidate<'static>,
    capture::StandingRepeated<'static>,
    thread::ThreadCreated<'static>,
    thread::ThreadEdited<'static>,
    thread::ThreadPaused<'static>,
    thread::ThreadResumed<'static>,
    thread::ThreadDone<'static>,
    thread::ThreadReopened<'static>,
    thread::ThreadOpened<'static>,
    thread_import::ThreadImported<'static>,
    thread_seal::ThreadSealed<'static>,
    thread_seal::ThreadUnsealed<'static>,
    thread_seal::ThreadSealProposed<'static>,
    thread_seal::ThreadSealDismissed<'static>,
    thread_route::ThreadRouted<'static>,
    thread_route::ThreadAsked<'static>,
    thread_route::ThreadAnswered<'static>,
    thread_route::ThreadMoved<'static>,
    thread_route::MisrouteHeld<'static>,
    tools_defer::ToolsUsed<'static>,
    tools_defer::ToolsSearched<'static>,
    tools_defer::ToolsMissed<'static>,
    thread::ThreadMerged<'static>,
    thread::ThreadUnmerged<'static>,
    thread::ThreadSplit<'static>,
    thread::ThreadUnsplit<'static>,
    thread::ThreadClaimed<'static>,
    thread::ThreadUnclaimed<'static>,
    thread::ThreadLinked<'static>,
    thread::ThreadUnlinked<'static>,
    thread::ThreadLinkProposed<'static>,
    extend_hands::HandsLaunched<'static>,
    extend_hands::HandsSettled<'static>,
    extend_hands::HandsRefused<'static>,
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
