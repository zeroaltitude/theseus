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
    AwsCalled = "aws.called",
    AwsHandsLaunched = "aws.hands.launched",
    AwsHandsSettled = "aws.hands.settled",
    AwsHourAlert = "aws.hour.alert",
    AwsReaperFailed = "aws.reaper.failed",
    AwsSessionMinted = "aws.session.minted",
    AwsTrailChecked = "aws.trail.checked",
    BudgetAsked = "budget.asked",
    BudgetCarved = "budget.carved",
    BudgetLimitChanged = "budget.limit_changed",
    BudgetMigrated = "budget.migrated",
    BudgetOverLimit = "budget.over_limit",
    BudgetReopened = "budget.reopened",
    BudgetReset = "budget.reset",
    CompletionDuplicate = "completion.duplicate",
    CompletionLateAfterCancel = "completion.late_after_cancel",
    CompletionQuarantined = "completion.quarantined",
    ConfigChanged = "config.changed",
    ConfigHeld = "config.held",
    ConfigInvalid = "config.invalid",
    ConfigUnreachable = "config.unreachable",
    ContextCompiled = "context.compiled",
    ContextCompacted = "context.compacted",
    ContextFileMissing = "context.file_missing",
    ContextOverflow = "context.overflow",
    ContextOverage = "context.overage",
    ContextRecompileRequested = "context.recompile_requested",
    ContextRecompiled = "context.recompiled",
    DiscordBound = "discord.bound",
    DiscordCommand = "discord.command",
    DiscordConfirm = "discord.confirm",
    DiscordDisconnected = "discord.disconnected",
    DiscordError = "discord.error",
    DiscordIgnored = "discord.ignored",
    DiscordMessageIn = "discord.message.in",
    DiscordMessageOut = "discord.message.out",
    DiscordReady = "discord.ready",
    DiscordTighten = "discord.tighten",
    DriverStarted = "driver.started",
    DurabilityShipped = "durability.shipped",
    ExecutionBlocked = "execution.blocked",
    ExecutionCancelled = "execution.cancelled",
    ExecutionComplete = "execution.complete",
    ExecutionFailed = "execution.failed",
    ExecutionInterrupted = "execution.interrupted",
    ExecutionOpened = "execution.opened",
    ExecutionQueued = "execution.queued",
    ExecutionResultsConsumed = "execution.results_consumed",
    ExecutionRunning = "execution.running",
    ExecutionStopped = "execution.stopped",
    ExecutionWaiting = "execution.waiting",
    ExtendAcked = "extend.acked",
    ExtendDeclined = "extend.declined",
    ExtendProposed = "extend.proposed",
    ExtendTested = "extend.tested",
    ImageNotShown = "image.not_shown",
    IndexTender = "index.tender",
    JobNotStarted = "job.not_started",
    JobRefused = "job.refused",
    JobStoppedAtLaunch = "job.stopped_at_launch",
    JobStoppedBelowFloor = "job.stopped_below_floor",
    JobWrapperLost = "job.wrapper_lost",
    JudgeBlockBooked = "judge.block_booked",
    JudgeCall = "judge.call",
    JudgeCircuit = "judge.circuit",
    JudgeLabel = "judge.label",
    JudgePaused = "judge.paused",
    JudgeReplay = "judge.replay",
    JudgeReport = "judge.report",
    JudgeResumed = "judge.resumed",
    JudgeShed = "judge.shed",
    LoopEnded = "loop.ended",
    LoopStarted = "loop.started",
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
    MemoryLabel = "memory.label",
    OntologyCategory = "ontology.category",
    OntologyGuidance = "ontology.guidance",
    OntologyMembership = "ontology.membership",
    PlacePublished = "place.published",
    PlaceViewed = "place.viewed",
    PolicyTightened = "policy.tightened",
    PolicyUntightened = "policy.untightened",
    ProfileChanged = "profile.changed",
    ProviderCall = "provider.call",
    ProviderCut = "provider.cut",
    ProviderError = "provider.error",
    ProviderRefusal = "provider.refusal",
    RecallRan = "recall.ran",
    RecallShadow = "recall.shadow",
    Reconcile = "reconcile",
    SandboxEgress = "sandbox.egress",
    SandboxEgressRefused = "sandbox.egress_refused",
    SandboxStarted = "sandbox.started",
    SecretGranted = "secret.granted",
    SecretWithheld = "secret.withheld",
    SecretsFailed = "secrets.failed",
    SecretsResolved = "secrets.resolved",
    ServerCrashed = "server.crashed",
    ServerServing = "server.serving",
    ServerStarted = "server.started",
    ServerStopping = "server.stopping",
    SessionExternalRead = "session.external_read",
    SessionOpened = "session.opened",
    SessionTrusted = "session.trusted",
    SpeechSynthesized = "speech.synthesized",
    SpeechTranscribed = "speech.transcribed",
    SpoolSwept = "spool.swept",
    StartupStep = "startup.step",
    StoreCorrupt = "store.corrupt",
    StoreIndexReplaced = "store.index_replaced",
    StoreRestored = "store.restored",
    TaskArranged = "task.arranged",
    TaskArrangementRefused = "task.arrangement_refused",
    TaskChangeAccepted = "task.change_accepted",
    TaskChangeDeclined = "task.change_declined",
    TaskChangeProposed = "task.change_proposed",
    TaskClosed = "task.closed",
    TaskCreated = "task.created",
    TaskEnded = "task.ended",
    TaskReportWake = "task.report_wake",
    TaskReportsRead = "task.reports_read",
    TaskSplit = "task.split",
    TaskStaleRefused = "task.stale_refused",
    TaskUpdated = "task.updated",
    TermClosed = "term.closed",
    TermOpened = "term.opened",
    ToolConfirmRequested = "tool.confirm_requested",
    ToolInvalidInput = "tool.invalid_input",
    ToolJobStarted = "tool.job_started",
    ToolLateResult = "tool.late_result",
    ToolNotified = "tool.notified",
    TurnEnded = "turn.ended",
    TurnFailed = "turn.failed",
    TurnNext = "turn.next",
    TurnRefused = "turn.refused",
    TurnStarted = "turn.started",
    TurnTrace = "turn.trace",
    VoiceBargeIn = "voice.barge_in",
    VoiceFailed = "voice.failed",
    VoiceJoined = "voice.joined",
    VoiceLeft = "voice.left",
    VoiceUnlisted = "voice.unlisted",
    WakeCancelled = "wake.cancelled",
    WakeEnded = "wake.ended",
    WakeFired = "wake.fired",
    WakeSet = "wake.set",
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
