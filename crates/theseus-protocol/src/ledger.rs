//! The ledger's kinds (theseus-j6qn, C2): every kind a row is written
//! under, by the kernel, the core, a binding, or a tool beside the daemon,
//! as one registry. A writer names a variant, so a test lists every kind,
//! and a misspelt kind fails the build instead of starting a new one.
//!
//! A row's `kind` stays a string on the wire (`LedgerEntry::kind`) and in
//! the store, so a row of a kind this build does not know (an older build's,
//! a newer one's) still reads, and a query names a kind by its string. A
//! kind renamed after rows were stored under its old name is in
//! `LedgerKind::RENAMED`, and a query for either name reads both.

macro_rules! ledger_kinds {
    ($($variant:ident = $name:literal,)*) => {
        /// A ledger row's kind.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum LedgerKind {
            $($variant,)*
        }

        impl LedgerKind {
            /// Every kind, in the registry's order.
            pub const ALL: &[LedgerKind] = &[$(LedgerKind::$variant,)*];

            /// Its name, as rows are written under it.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(LedgerKind::$variant => $name,)*
                }
            }

            /// The kind a name is, if this build writes it. An old name
            /// (`RENAMED`) is the kind it became.
            pub fn parse(name: &str) -> Option<LedgerKind> {
                let name = Self::RENAMED
                    .iter()
                    .find(|(_, before)| *before == name)
                    .map_or(name, |(now, _)| now.as_str());
                match name {
                    $($name => Some(LedgerKind::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

ledger_kinds! {
    ActionAuthorized = "action.authorized",
    ActionCancel = "action.cancel",
    ActionCancelUncertain = "action.cancel_uncertain",
    ActionCancelUnsupported = "action.cancel_unsupported",
    ActionCancelVerified = "action.cancel_verified",
    ActionCancelled = "action.cancelled",
    ActionConfirmAnswered = "action.confirm_answered",
    ActionConfirmed = "action.confirmed",
    ActionDeclined = "action.declined",
    ActionDispatched = "action.dispatched",
    ActionExpired = "action.expired",
    ActionFailed = "action.failed",
    ActionOutcomeUnknown = "action.outcome_unknown",
    ActionPlanned = "action.planned",
    ActionResolved = "action.resolved",
    ActionSucceeded = "action.succeeded",
    ApprovalRefused = "approval.refused",
    AwsBudgetReconciled = "aws.budget.reconciled",
    AwsMonthWarning = "aws.month_warning",
    AwsCalled = "aws.called",
    AwsHandsLaunched = "aws.hands.launched",
    AwsHandsSettled = "aws.hands.settled",
    AwsHourAlert = "aws.hour.alert",
    AwsReaperFailed = "aws.reaper.failed",
    AwsRunaway = "aws.runaway",
    AwsSessionMinted = "aws.session.minted",
    AwsTrailChecked = "aws.trail.checked",
    BudgetAsked = "budget.asked",
    BudgetCarved = "budget.carved",
    BudgetLimitChanged = "budget.limit_changed",
    BudgetOverLimit = "budget.over_limit",
    BudgetReached = "budget.reached",
    BudgetReopened = "budget.reopened",
    BudgetReset = "budget.reset",
    CardAnswered = "card.answered",
    CardObserved = "card.observed",
    CardProposed = "card.proposed",
    CompletionDuplicate = "completion.duplicate",
    CompletionLateAfterCancel = "completion.late_after_cancel",
    CompletionQuarantined = "completion.quarantined",
    ConfigChanged = "config.changed",
    ConfigHeld = "config.held",
    ConfigInvalid = "config.invalid",
    ConfigUnreachable = "config.unreachable",
    ContentJudged = "content.judged",
    ContentLabelled = "content.labelled",
    ContentRemoved = "content.removed",
    ContentUnjudged = "content.unjudged",
    ContextCompiled = "context.compiled",
    ContextDetoured = "context.detoured",
    ContextCompacted = "context.compacted",
    ContextCleared = "context.cleared",
    ContextFileMissing = "context.file_missing",
    ContextOverflow = "context.overflow",
    ContextOverage = "context.overage",
    ContextRecompileRequested = "context.recompile_requested",
    ContextRecompiled = "context.recompiled",
    ContextUnadmitted = "context.unadmitted",
    DataGateBlocked = "data_gate.blocked",
    DiscordBound = "discord.bound",
    DiscordCommand = "discord.command",
    DiscordConfirm = "discord.confirm",
    DiscordDisconnected = "discord.disconnected",
    DiscordError = "discord.error",
    DiscordIgnored = "discord.ignored",
    DiscordLabel = "discord.label",
    DiscordMessageIn = "discord.message.in",
    DiscordMessageOut = "discord.message.out",
    DiscordReady = "discord.ready",
    DiscordTighten = "discord.tighten",
    DiskBelowFloor = "disk.below_floor",
    DiskLow = "disk.low",
    DiskOk = "disk.ok",
    DriverStarted = "driver.started",
    DurabilityShipped = "durability.shipped",
    ExecutionBlocked = "execution.blocked",
    ExecutionCancelled = "execution.cancelled",
    ExecutionComplete = "execution.complete",
    ExecutionDismissed = "execution.dismissed",
    ExecutionFailed = "execution.failed",
    ExecutionInterrupted = "execution.interrupted",
    ExecutionOpened = "execution.opened",
    ExecutionQueued = "execution.queued",
    ExecutionResultsConsumed = "execution.results_consumed",
    ExecutionRunning = "execution.running",
    ExecutionStopped = "execution.stopped",
    ExecutionStoppedAll = "execution.stopped_all",
    ExecutionWaiting = "execution.waiting",
    ExtendAcked = "extend.acked",
    ExtendDeclined = "extend.declined",
    ExtendHandsLaunched = "extend.hands.launched",
    ExtendHandsRefused = "extend.hands.refused",
    ExtendHandsSettled = "extend.hands.settled",
    ExtendLoaded = "extend.loaded",
    ExtendProposed = "extend.proposed",
    ExtendRevoked = "extend.revoked",
    ExtendTested = "extend.tested",
    FactCorrected = "fact.corrected",
    FactRetracted = "fact.retracted",
    FanOutCue = "fan_out.cue",
    FileRead = "file.read",
    FilesRestored = "files.restored",
    FlowLoaded = "flow.loaded",
    FlowRan = "flow.ran",
    FlowRetired = "flow.retired",
    GlidePosted = "glide.posted",
    GlideRead = "glide.read",
    ImageNotShown = "image.not_shown",
    ImportBatch = "import.batch",
    ImportErased = "import.erased",
    ImportErasing = "import.erasing",
    ImportPeople = "import.people",
    ImportSynced = "import.synced",
    ImportTopics = "import.topics",
    IndexRebuild = "index.rebuild",
    IndexTender = "index.tender",
    InitiativeRated = "initiative.rated",
    InitiativeSpoke = "initiative.spoke",
    JobNotStarted = "job.not_started",
    JobRefused = "job.refused",
    JobStoppedAtLaunch = "job.stopped_at_launch",
    JobStoppedBelowFloor = "job.stopped_below_floor",
    JobWrapperGone = "job.wrapper_gone",
    JobWrapperLost = "job.wrapper_lost",
    JudgeAudit = "judge.audit",
    JudgeBackfill = "judge.backfill",
    JudgeBlockBooked = "judge.block_booked",
    JudgeCall = "judge.call",
    JudgeCircuit = "judge.circuit",
    JudgeLabel = "judge.label",
    JudgePaused = "judge.paused",
    JudgeProposal = "judge.proposal",
    JudgeReplay = "judge.replay",
    JudgeReport = "judge.report",
    JudgeResumed = "judge.resumed",
    GoogleAuthLapsed = "google.auth_lapsed",
    GoogleAuthorized = "google.authorized",
    GoogleCalled = "google.called",
    GoogleRevoked = "google.revoked",
    JudgeShed = "judge.shed",
    KeepWarm = "keep_warm",
    LedgerChainStarted = "ledger.chain_started",
    LedgerHistoryPinned = "ledger.history_pinned",
    LoopCapReached = "loop.cap_reached",
    LoopEnded = "loop.ended",
    LoopStarted = "loop.started",
    RouteDecided = "route.decided",
    RoutineChanged = "routine.changed",
    RouteCorrected = "route.corrected",
    RouteReplayJudged = "route.replay.judged",
    LspFailed = "lsp.failed",
    LspReady = "lsp.ready",
    LspStarted = "lsp.started",
    LspStopped = "lsp.stopped",
    McpExited = "mcp.exited",
    McpFailed = "mcp.failed",
    McpPromptChanged = "mcp.prompt_changed",
    McpReady = "mcp.ready",
    McpServerCall = "mcp_server.call",
    McpServerRefused = "mcp_server.refused",
    McpStarted = "mcp.started",
    McpToolsChanged = "mcp.tools_changed",
    MemoryArm = "memory.arm",
    MemoryCited = "memory.cited",
    MemoryGated = "memory.gated",
    MemoryLabel = "memory.label",
    MemoryLabeled = "memory.labeled",
    MemoryRead = "memory.read",
    MemoryUsed = "memory.used",
    OntoFiled = "onto.filed",
    OntoNamed = "onto.named",
    OntologyCategory = "ontology.category",
    OntologyGuidance = "ontology.guidance",
    OntologyKind = "ontology.kind",
    OntologyMembership = "ontology.membership",
    OntologyMerged = "ontology.merged",
    OntologyUndo = "ontology.undo",
    PackEvent = "pack.event",
    PackMode = "pack.mode",
    PackVersion = "pack.version",
    PeopleExtracted = "people.extracted",
    PeopleSwept = "people.swept",
    PersonaChanged = "persona.changed",
    PersonaEngaged = "persona.engaged",
    PersonaImported = "persona.imported",
    PersonaMigrated = "persona.migrated",
    PersonaSoul = "persona.soul",
    PlacePublished = "place.published",
    PlaceViewed = "place.viewed",
    PlaceWarned = "place.warned",
    PolicyTightened = "policy.tightened",
    PolicyUntightened = "policy.untightened",
    PrefixPicked = "prefix.picked",
    PrivacyWarned = "privacy.warned",
    ProfileChanged = "profile.changed",
    PromiseKept = "promise.kept",
    PromiseUnkept = "promise.unkept",
    ProviderCall = "provider.call",
    ProviderCut = "provider.cut",
    ProviderEffortRefused = "provider.effort_refused",
    ProviderError = "provider.error",
    ProviderFallback = "provider.fallback",
    ProviderRefusal = "provider.refusal",
    ProviderRoutedBack = "provider.routed_back",
    RecallRan = "recall.ran",
    RecallShadow = "recall.shadow",
    Reconcile = "reconcile",
    RequestRedacted = "request.redacted",
    SandboxEgress = "sandbox.egress",
    SandboxEgressRefused = "sandbox.egress_refused",
    SandboxStarted = "sandbox.started",
    SecretGranted = "secret.granted",
    SecretWithheld = "secret.withheld",
    SecretsFailed = "secrets.failed",
    SecretsResolved = "secrets.resolved",
    SelfBudgetReserved = "self.budget.reserved",
    SelfBudgetSettled = "self.budget.settled",
    SelfBudgetStopped = "self.budget.stopped",
    SelfExamCounted = "self.exam.counted",
    SelfBacklogDropped = "self.backlog.dropped",
    SelfBacklogFiled = "self.backlog.filed",
    SelfBacklogRanked = "self.backlog.ranked",
    SelfBacklogStarted = "self.backlog.started",
    SelfBranchBuilt = "self.branch.built",
    SelfExamPass = "self.exam.pass",
    SelfGraduateProposed = "self.graduate.proposed",
    SelfMineRan = "self.mine.ran",
    SelfMintValidated = "self.mint.validated",
    SelfMintDeployed = "self.mint.deployed",
    SelfMintRetired = "self.mint.retired",
    SelfSealedScored = "self.sealed.scored",
    SelfSealedChecked = "self.sealed.checked",
    SelfSealedDrift = "self.sealed.drift",
    SelfExamRejected = "self.exam.rejected",
    SelfInstalled = "self.installed",
    SelfJoinStopped = "self.join.stopped",
    SelfJoined = "self.joined",
    SelfReviewVerdict = "self.review.verdict",
    SelfHalted = "self.halted",
    SelfNarrowApplied = "self.narrow.applied",
    SelfNarrowDeclined = "self.narrow.declined",
    SelfNarrowProposed = "self.narrow.proposed",
    SelfNarrowRed = "self.narrow.red",
    SelfNarrowReverted = "self.narrow.reverted",
    SelfResumed = "self.resumed",
    SelfReverted = "self.reverted",
    SelfVetoed = "self.vetoed",
    SelfPipelineStarted = "self.pipeline.started",
    SelfPipelineHeld = "self.pipeline.held",
    SelfPipelineReturned = "self.pipeline.returned",
    SelfPipelineParked = "self.pipeline.parked",
    SelfPipelineQueued = "self.pipeline.queued",
    SelfPipelineCleared = "self.pipeline.cleared",
    SelfPipelineEnded = "self.pipeline.ended",
    ServerCrashed = "server.crashed",
    ServerServing = "server.serving",
    ServerStarted = "server.started",
    ServerStopping = "server.stopping",
    SessionForked = "session.forked",
    SessionMode = "session.mode",
    SessionOpened = "session.opened",
    SessionProfileSet = "session.profile_set",
    SessionReopened = "session.reopened",
    SessionRetired = "session.retired",
    SessionSuperseded = "session.superseded",
    SpeechSynthesized = "speech.synthesized",
    SpeechTranscribed = "speech.transcribed",
    SpendCeiling = "spend.ceiling",
    SpendDayWarning = "spend.day_warning",
    SpoolSwept = "spool.swept",
    StandingAdded = "standing.added",
    StandingAnswered = "standing.answered",
    StandingAsked = "standing.asked",
    StandingCandidate = "standing.candidate",
    StandingCaptured = "standing.captured",
    StandingProposalAnswered = "standing.proposal_answered",
    StandingRepeated = "standing.repeated",
    StandingRevoked = "standing.revoked",
    StartupStep = "startup.step",
    ThreadCreated = "thread.created",
    ThreadDone = "thread.done",
    ThreadEdited = "thread.edited",
    ThreadImported = "thread.imported",
    ThreadMerged = "thread.merged",
    ThreadPaused = "thread.paused",
    ThreadReopened = "thread.reopened",
    ThreadResumed = "thread.resumed",
    ThreadSealDismissed = "thread.seal_dismissed",
    ThreadSealProposed = "thread.seal_proposed",
    ThreadSealed = "thread.sealed",
    ThreadUnsealed = "thread.unsealed",
    ThreadLinked = "thread.linked",
    ThreadUnlinked = "thread.unlinked",
    ThreadLinkProposed = "thread.link_proposed",
    ThreadRouted = "thread.routed",
    ThreadAsked = "thread.asked",
    ThreadAnswered = "thread.answered",
    ThreadMoved = "thread.moved",
    ThreadMisrouteHeld = "thread.misroute_held",
    ThreadSplit = "thread.split",
    ThreadUnsplit = "thread.unsplit",
    ThreadUnmerged = "thread.unmerged",
    ThreadClaimed = "thread.claimed",
    ThreadUnclaimed = "thread.unclaimed",
    SynthesisChecked = "synthesis.checked",
    SynthesisProposed = "synthesis.proposed",
    SynthesisScored = "synthesis.scored",
    StoreCorrupt = "store.corrupt",
    StoreIndexReplaced = "store.index_replaced",
    StoreRestored = "store.restored",
    TaskArranged = "task.arranged",
    TaskArrangementRefused = "task.arrangement_refused",
    TaskChangeAccepted = "task.change_accepted",
    TaskChangeDeclined = "task.change_declined",
    TaskChangeExpired = "task.change_expired",
    TaskChangeProposed = "task.change_proposed",
    TaskCheckOpened = "task.check_opened",
    TaskCheckRefused = "task.check_refused",
    TaskClaimed = "task.claimed",
    TaskClosed = "task.closed",
    TaskCreated = "task.created",
    TaskDraft = "task.draft",
    TaskEnded = "task.ended",
    TaskLeaseExpired = "task.lease_expired",
    TaskPaused = "task.paused",
    TaskRehomed = "task.rehomed",
    TaskReportWake = "task.report_wake",
    TaskReportsRead = "task.reports_read",
    TaskRescheduled = "task.rescheduled",
    TaskResumed = "task.resumed",
    TaskRetried = "task.retried",
    TaskSplit = "task.split",
    TaskStaleRefused = "task.stale_refused",
    TaskStartedByOwner = "task.started_by_owner",
    TaskUpdated = "task.updated",
    TaskWaitEnded = "task.wait.ended",
    TaskWaitStarted = "task.wait.started",
    TermClosed = "term.closed",
    TermLeft = "term.left",
    TermOpened = "term.opened",
    ToolConfirmRequested = "tool.confirm_requested",
    ToolInvalidInput = "tool.invalid_input",
    ToolJobStarted = "tool.job_started",
    ToolLateResult = "tool.late_result",
    ToolNotified = "tool.notified",
    ToolsMissed = "tools.missed",
    ToolsSearched = "tools.searched",
    ToolsUsed = "tools.used",
    TurnEndNoted = "turn.end_noted",
    TurnEnded = "turn.ended",
    TurnFailed = "turn.failed",
    TurnNext = "turn.next",
    TurnNudged = "turn.nudged",
    TurnProgress = "turn.progress",
    TurnRefused = "turn.refused",
    TurnReplayed = "turn.replayed",
    TurnStarted = "turn.started",
    TurnSteered = "turn.steered",
    TurnTrace = "turn.trace",
    VoiceBargeIn = "voice.barge_in",
    VoiceCut = "voice.cut",
    VoiceDeaf = "voice.deaf",
    VoiceFailed = "voice.failed",
    VoiceJoined = "voice.joined",
    VoiceLeft = "voice.left",
    VoiceRejoined = "voice.rejoined",
    VoiceResumed = "voice.resumed",
    VoiceUnlisted = "voice.unlisted",
    WakeCancelled = "wake.cancelled",
    WakeDone = "wake.done",
    WakeEnded = "wake.ended",
    WakeFired = "wake.fired",
    WakeMoved = "wake.moved",
    WakePaused = "wake.paused",
    WakeResumed = "wake.resumed",
    WakeSet = "wake.set",
    WakeSkipped = "wake.skipped",
    WakeSnoozed = "wake.snoozed",
    WatchChecked = "watch.checked",
    WatchExpired = "watch.expired",
    WatchFailing = "watch.failing",
    WatchMet = "watch.met",
    WebDevOrigin = "web.dev_origin",
    WebRefused = "web.refused",
}

impl LedgerKind {
    /// Kinds renamed after rows were stored under the old name, as (now,
    /// before). A query for either name reads both.
    pub const RENAMED: &[(LedgerKind, &str)] = &[(LedgerKind::ActionDeclined, "action.denied")];
}

impl std::fmt::Display for LedgerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for LedgerKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

// ---------------------------------------------------------------- reading the ledger

/// `ledger.tail`'s params: the newest `n` rows, or the first `n` after a position.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerTailParams {
    #[serde(default)]
    pub n: Option<usize>,
    /// Only rows of this kind (e.g. "turn.ended", "provider.error"). A renamed
    /// kind also reads the rows stored under its old name.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Only rows after this WAL position, and the first `n` of them, where
    /// without it the read is the newest `n` (theseus-xo0m). A walk over the
    /// whole ledger passes 0, then each answer's `next`; a poll passes the
    /// last position it has. A read, as the rest of `ledger.tail` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub after: Option<u64>,
    /// Only rows before this WAL position: a page back from the newest, the
    /// newest `n` of them (theseus-vm3n.5). A walk back passes each answer's
    /// `older`. Positions never move, so rows written meanwhile neither
    /// repeat a row nor skip one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub before: Option<u64>,
    /// Only rows written at this time or after (unix ms), and with
    /// `until_ms` at it or before. A row counts at its kind's clock in the
    /// store, the newest time a row had been written at, so a host clock
    /// that steps back never splits a window; each row still says its own
    /// `at_unix_ms`. Read through the store's index of time, not a scan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerEntry {
    pub position: u64,
    pub at_unix_ms: u64,
    pub kind: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerTailResult {
    pub rows: Vec<LedgerEntry>,
    pub total: u64,
    /// With `after`: the `after` for the next page while more rows may
    /// follow (the last row read, whether or not a filter kept it); absent
    /// at the ledger's end, and without `after`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<u64>,
    /// With `before`: the `before` for the next page back while older rows
    /// match (the first row of this page); absent once none do, and
    /// without `before`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub older: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each kind has one name, of a kind's form, and its name parses back to
    /// it; an old name parses to the kind it became, and an unknown one to
    /// none.
    #[test]
    fn every_kind_has_one_name_that_parses_back() {
        let mut names: Vec<&str> = LedgerKind::ALL.iter().map(|k| k.as_str()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "a name given to two kinds");
        for k in LedgerKind::ALL {
            let name = k.as_str();
            assert!(
                name.split('.')
                    .all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '_')),
                "`{name}` is not a kind's form"
            );
            assert_eq!(LedgerKind::parse(name), Some(*k));
        }
        assert_eq!(
            LedgerKind::parse("action.denied"),
            Some(LedgerKind::ActionDeclined)
        );
        assert_eq!(LedgerKind::parse("no.such_kind"), None);
    }
}
