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
    ActionCancelled = "action.cancelled",
    ActionConfirmAnswered = "action.confirm_answered",
    ActionConfirmed = "action.confirmed",
    ActionDeclined = "action.declined",
    ActionDispatched = "action.dispatched",
    ActionFailed = "action.failed",
    ActionOutcomeUnknown = "action.outcome_unknown",
    ActionPlanned = "action.planned",
    ActionResolved = "action.resolved",
    ActionSucceeded = "action.succeeded",
    ApprovalChannelChecked = "approval.channel_checked",
    ApprovalRefused = "approval.refused",
    AwsCalled = "aws.called",
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
    ConfigConfirmed = "config.confirmed",
    ConfigHeld = "config.held",
    ConfigInvalid = "config.invalid",
    ConfigUnreachable = "config.unreachable",
    ContextCompiled = "context.compiled",
    ContextFileMissing = "context.file_missing",
    ContextOverflow = "context.overflow",
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
    ImageNotShown = "image.not_shown",
    IndexTender = "index.tender",
    JobNotStarted = "job.not_started",
    JobRefused = "job.refused",
    JobStoppedAtLaunch = "job.stopped_at_launch",
    JobStoppedBelowFloor = "job.stopped_below_floor",
    JobWrapperLost = "job.wrapper_lost",
    LoopEnded = "loop.ended",
    LoopStarted = "loop.started",
    PolicyTightened = "policy.tightened",
    PolicyUntightened = "policy.untightened",
    ProfileChanged = "profile.changed",
    ProviderCall = "provider.call",
    ProviderCut = "provider.cut",
    ProviderError = "provider.error",
    ProviderRefusal = "provider.refusal",
    Reconcile = "reconcile",
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
    SpoolSwept = "spool.swept",
    StartupStep = "startup.step",
    StoreCorrupt = "store.corrupt",
    StoreIndexReplaced = "store.index_replaced",
    StoreRestored = "store.restored",
    TaskEnded = "task.ended",
    TaskReportWake = "task.report_wake",
    TaskReportsRead = "task.reports_read",
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
    WakeCancelled = "wake.cancelled",
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
