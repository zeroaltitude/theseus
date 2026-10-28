//! The cockpit's protocol types, generated from these (theseus-0g4; the owner's
//! call, 2026-10-01): one file per type in `cockpit/src/protocol.gen/`, and
//! `index.ts` exporting them all. `cockpit/src/protocol.ts` re-exports them
//! beside the client, and the cockpit imports it as `@protocol`.
//!
//! The test writes the files, and the gate fails when they differ from the
//! commit: a Rust type changed without its TypeScript fails the gate. Large
//! integers are `number`, since the wire is JSON and `JSON.parse` reads
//! numbers. A `serde_json::Value` field says its TypeScript type itself
//! (`#[ts(type = "...")]`).
//!
//! The place: the directory sat in `web/src/` until the Observatory retired,
//! and moved into the cockpit byte for byte (theseus-vm3n.6). `.gen` says
//! nobody edits it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ts_rs::{Config, TS};

use crate::*;

/// Every wire type, exported with its dependencies.
macro_rules! export {
    ($cfg:expr; $($t:ty),* $(,)?) => {
        $(<$t as TS>::export_all($cfg).unwrap_or_else(|e| panic!("{}: {e}", stringify!($t)));)*
    };
}

fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cockpit/src/protocol.gen")
}

/// The types this crate declares, by its sources: (name, derives `TS`).
fn declared() -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for src in [
        include_str!("lib.rs"),
        include_str!("arrangement.rs"),
        include_str!("census.rs"),
        include_str!("check.rs"),
        include_str!("cite.rs"),
        include_str!("context.rs"),
        include_str!("tasks.rs"),
        include_str!("events.rs"),
        include_str!("gate.rs"),
        include_str!("index.rs"),
        include_str!("inspect.rs"),
        include_str!("mcp.rs"),
        include_str!("extend.rs"),
        include_str!("memory.rs"),
        include_str!("learning.rs"),
        include_str!("judge_runs.rs"),
        include_str!("ontology.rs"),
        include_str!("places.rs"),
        include_str!("push.rs"),
        include_str!("standing.rs"),
        include_str!("work.rs"),
        include_str!("retry.rs"),
        include_str!("notices.rs"),
        include_str!("work_watch.rs"),
        include_str!("job_follow.rs"),
        include_str!("nodes.rs"),
        include_str!("thread_placed.rs"),
    ] {
        let lines: Vec<&str> = src.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let Some(rest) = l
                .strip_prefix("pub struct ")
                .or_else(|| l.strip_prefix("pub enum "))
            else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let ts = lines[..i]
                .iter()
                .rev()
                .take_while(|a| a.starts_with("#[") || a.starts_with("///"))
                .any(|a| a.contains("ts_rs::TS"));
            out.push((name, ts));
        }
    }
    out
}

#[test]
fn the_cockpits_types_are_generated_from_the_rust_ones() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    // A type that went away takes its file with it.
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "ts") {
            std::fs::remove_file(p).unwrap();
        }
    }
    let cfg = Config::new().with_large_int("number").with_out_dir(&dir);
    export! { &cfg;
        Id, Request, Notification, RpcError, Response, Message, HealthResult, Build, SpoolStatus,
        SpoolSweep, DiskStatus, BinaryStatus, StoreStatus, CrashStatus, WebStatus, SecretsStatus,
        ContextStatus, ConfigStatus, NodeCacheHealth, PromptCacheHealth, ProfileTtl,
        ConfigRestart, SecretFailed, SecretSource, StartupPhase, Tightening, PolicyTightenParams,
        PolicyUntightenParams, TightenResult,
        content::LabelView, content::ContentUnlabelParams, content::UnlabelResult, content::LabelRaised, notice::WaitingSplit, DiscordOrigin, BindingStatus, OutboxStatus,
        PlaceStatus, TelemetryStatus, KernelStatus, ChildrenStatus, GrantStatus,
        ExecutionInfo,
        BudgetInfo, ExecutionListResult, ExecutionListParams, ActionInfo, ActionListParams, ActionListResult,
        ExecutionCancelParams, ExecutionCancelResult, ExecutionStopParams, ExecutionStopResult, ExecutionDismissParams, ExecutionDismissResult, TaskRetryParams, TaskRetryResult, task_pause::TaskPauseParams, task_pause::TaskPauseResult, task_not_this::TaskNotThisParams, task_not_this::TaskNotThisResult, ops::DataMark, ops::SessionProfileParams, ops::SessionProfileResult, ops::TaskRehomeParams, ops::TaskRehomeResult, ops::TaskRescheduleParams, ops::TaskRescheduleResult,
        TaskInfo, TaskListParams, TaskListResult, tasks::TaskDetail, TaskCancelParams, TaskCancelResult, WakeInfo,
        WakeListParams, WakeListResult, WakeCancelParams, WakeCancelResult, wakes::WakeCreateParams, wakes::WakeEditResult, wakes::WakeSnoozeParams, wakes::WakePauseParams, wakes::WakeNameParams, wakes::WakeDoneResult, wakes::WakeAnswer, wakes::WakeNoticed, watches::WatchCheck, watches::WatchThen, watches::WatchCreateParams, watches::WatchInfo, watches::WatchCreateResult, watches::WatchListParams, watches::WatchListResult, TaskRef, SessionKind, task_start::TaskStartParams, task_start::TaskStartResult, task_start::ReportTo, task_start::Expect,
        SessionOpenParams, SessionInfo, SessionListResult, TurnSubmitParams, Attachment, sessions::SessionRetireParams, sessions::SessionReopenParams,
        ProfileInfo, ProfileListResult, ProfileUseParams, ProfileChanged, Span, Usage,
        TurnSubmitResult, LedgerTailParams, LedgerEntry, LedgerTailResult, LedgerWindowParams, LedgerWindowResult, LedgerVerifyParams, LedgerVerifyResult, LedgerBreak, LedgerHead, SessionRef,
        SessionHistoryParams, NodeInfo, SessionHistoryResult, SessionRecompileParams,
        NodeListParams, NodeListResult, NodeReachParams, ReachCompilation, ReachExposure,
        ReachDescendant, ReachTotals, NodeReachResult, CompilationListParams, CompilationInfo,
        CompilationListResult, CatalogModel, CatalogListResult, ConfirmRequest, plain::PlainWords, BudgetAsk,
        ConfirmListResult, ActionConfirmParams, ActionConfirmResult, ToolInfo, ToolListResult, defer::DeferListResult, defer::DeferPlace, defer::DeferFamily, defer::DeferHealth,
        ProviderErrorData, TurnStarted, TurnFailed, LoopStarted, ModelDelta, LoopEnded,
        NarrativePart, NarrativeLine, NarrativeWatchResult, ToolProposed, ContextFileRef,
        CacheSummary, StubbedFile, EstimateSummary, CensusSummary, ContextCompiled, Situation, ToolStarted, ToolEnded, ToolDiff, ConfirmPreview,
        ConfirmResolved, NodeWritten, PolicyNotified, Access, Resource, Plan, Proposal,
        Notice, GateDecision, GateResult, GateRecord, Level, Attention, WaitingOn, PendingConfirm,
        ExecutionView, ExecutionsWatchParams, ExecutionsWatchResult, ShortFailed, StatusShortResult, SessionListParams, PushStatus, ConnectionsHealth,
        WaitUntil, SessionWaitParams, SessionWaitResult, EventsLost, ToolClass, AwsPlan, AwsStatus,
        AwsAccountStatus, TenderStatus, PlaceClass, PlacesHealth, PlaceInfo, PlacePublishParams, PublishResult,
        HandsListParams, HandsGroupInfo, HandsListResult,
        BudgetListResult, BudgetRow, BudgetResetInfo, BudgetQuestionInfo, BudgetTotals, JudgeDayBudget, DayCeilingBudget,
        PolicyExplainParams, PolicyExplainResult, PlaceExplain, ToolExplain, ExplainLayer, ExplainCondition,
        OntologyListParams, OntologyListResult, OntologyKind, OntologyCategory, OntologyGuidance,
        OntologyMembership, OntologyCategoryAddParams, OntologyGuidanceSetParams,
        OntologyMembershipSetParams, OntologyMembershipResult, OntologyProposalsParams,
        OntologyProposalsResult, OntologyProposal, ProposedPerson, OntologyProposalAcceptParams,
        OntologyProposalRejectParams, OntologyProposalAnswered, OntologyPersonMergeParams, OntologyPersonMerged, OntologyProposalAcceptAllParams, OntologyProposalAcceptAllResult, OntologyPersonProposals, OntologyProposalRejectAllParams, OntologyProposalRejectAllResult, OntologyProposalNeverParams, OntologyProposalNeverResult, OntologyNever, OntologyNeverListParams, OntologyNeverListResult, OntologyNeverUndoParams, OntologyNeverUndoResult, OntologyPersonNotSameParams, OntologyPersonNotSame, OntologyPersonNotSameListParams, OntologyPersonNotSameListResult, OntologyPersonCandidatesParams, OntologyPersonCandidate, OntologyPersonCandidatesResult, OntologyUndoParams, OntologyUndoResult, OntologyKindAddParams, OntologyKindRetireParams, OntologyKindResult, judge::JudgeLatencyParams, judge::JudgeLatencyDay, judge::JudgeLatencyResult, TasksHealth, ParkedTask,
        index::IndexQueryParams, index::IndexWeights, index::IndexFilters, index::IndexSourceRank,
        index::IndexHit, index::IndexTimings, index::IndexLag, index::IndexQueryResult,
        index::IndexStamp, index::IndexEmbedTask, index::IndexNeighboursParams,
        index::IndexNeighbour, index::IndexNeighboursResult, index::IndexEmbedParams,
        index::IndexEmbedResult, index::IndexEntitiesParams, index::IndexEntitiesResult,
        index::IndexForgetParams, index::IndexChunkRef,
        index::IndexForgetResult, index::IndexWarmResult, index::IndexBackfill, index::IndexStatus,
        index::IndexVectorStatus, index::IndexCompactions, index::IndexReembed, index::IndexQueryGate,
        index::IndexEmbedStats, index::IndexRebuildResult, index::IndexHealth, index::IndexPriority,
        memory::MemorySearchParams, memory::MemoryLookupParams, memory::MemoryLookupResult, memory::MemoryHit, memory::MemoryCitation, memory::MemoryRecallsParams, memory::MemoryRecallsResult,
        memory::RecallManifest, memory::RecallRerank, memory::RecallActivation, memory::MemoryHealth, memory::RecallOutcomes, memory::AdjacencyHealth, memory::RecallItem, memory::RecallRelevance, memory::RecallDrop, memory::RecallTimings, memory::RecallWhen, resident::ResidentHealth, warm::WarmHealth, warm::WarmStamp, warm::SessionTypingParams, warm::SessionTypingResult, resident::HeapHealth, resident::TrimHealth, resident::CacheHealth, cite::Citation, cite::DiscordSource, cite::MemoryCitesParams, cite::MemoryCitesResult,
        memory::BudgetReport, memory::BudgetDrop, memory::BudgetRange, memory::BudgetOverage,
        memory::MemoryLabelParams, memory::MemoryLabelResult, retract::FactRetractParams, retract::FactRetractResult, retract::RetractExposure, memory::RecallRetention, memory::MemoryConsolidateParams, memory::MemoryConsolidateResult, memory::SynthesisReport,
        sandbox::SandboxHealth, judge::JudgeHealth, judge::JudgeScored, judge::JudgeNoticed, judge::JudgeListParams, judge::JudgeListResult,
        judge::JudgeGetParams, judge::JudgeGetResult, learning::JudgeLabelParams, learning::JudgeLabelResult, packs::PackListResult, packs::PackInfo, packs::PackModeRow, packs::HoldoutBounds, packs::PackPromoteParams, packs::PackPromoteResult, packs::PackRollbackParams, import::ImportLine, import::ImportEpisodesParams, import::ImportRejected, import::ImportEpisodesResult, import::ImportEraseParams, import::ImportEraseResult, import::ImportTagInfo, import::ImportListResult, import::HealthImported, import_sync::ImportTouched, import_sync::ImportSyncPlan, import_sync::ImportSyncRecord, import_sync::SyncPeople, import_sync::HealthSync, import_sync::ImportSyncResult, import::ImportTopicsParams, import::ImportTopicsResult, import::ImportPeopleParams, import::ImportPeopleResult, import::PeopleProposeReport, books::BooksListParams, books::BookInfo, books::BooksListResult, books::BooksPageParams, books::BookPlace, books::BookEpisode, books::BookFacet, books::BookFacets, books::BooksPageResult, import::ImportSessionsParams, import::ImportedEpisode, import::ImportFacet, import::ImportFacets, import::ImportSessionsResult, context::ContextExplainParams, context::ContextPart, context::ContextTurn, context::ContextSource, context::ContextExplainResult, context::DetourPart, context::ContextDetoured, context::ContextDetour, census::ContextCensusParams, census::CensusPart, census::CensusBlock, census::CensusTool, census::CensusExact, census::ContextCensusResult, inspect::TurnInspectParams, inspect::InspectPrompt, inspect::InspectTotals, inspect::InspectRow, inspect::InspectAnswer, inspect::InspectRedacted, inspect::InspectCall, inspect::InspectLoop, inspect::TurnInspectResult,
        learning::LearningReportParams, learning::LearningReport, judge_runs::JudgeReplayParams, judge_runs::ReplayLeftOut, judge_runs::ReplayJudgment, judge_runs::ReplayClassFell, judge_runs::ReplayEvalSide,
        judge_runs::ReplayEval, judge_runs::OntoKindScore, judge_runs::UndoRate, judge_runs::OntoReplayReport, judge_runs::JudgeReplayResult, judge_runs::JudgeAuditParams, judge_runs::JudgeAuditResult, judge_runs::JudgeBackfillParams, judge_runs::JudgeBackfillResult, judge_runs::JudgeLearnParams, judge_runs::LearnClass, judge_runs::LearnQuestion, judge_runs::LearnThreshold, judge_runs::JudgeProposal, judge_runs::JudgeProveParams, judge_runs::JudgeProveResult, judge_runs::ClassifyQuality, sandbox::SandboxLaunch, cancel::CancelVerdict, cancel::CancelCount,
        signals::CompileSignal, route::TurnRoute, route::RoutePersona, route::TurnData, route::TurnFallback, route::ToolsRaised, route::RaisedFamily, nudge::TurnNudge, files_left::FilesLeft, files_left::PathList, files_left::EndNote, nudge::SessionNudgeParams, nudge::SessionNudgeResult, route::RouteCorrectParams, route::RouteCorrectResult, route::RouteCorrectionsResult, route::RouteCorrectionInfo, sandbox::SandboxUsage, sandbox::RunningJob, lsp::LspServerStatus,
        bench::BenchHistoryParams, bench::BenchHistoryResult, bench::BenchRun, bench::BenchPhase, cred::HarnessOnly,
        AwsBudgetStatus, AwsGuardDutyStatus, AwsBootstrapParams, AwsBootstrapStack,
        AwsBootstrapResult, AwsConfirmAlertsParams, AwsConfirmAlertsResult, TaskArrangement,
        ArrangementPiece, TaskCheck, CheckPiece, CheckOverlap,
        mcp::McpServerStatus, mcp::McpToolInfo, mcp::McpListResult,
        mcp::McpRestartParams, mcp::McpRestartResult, mcp::McpPromptArgument,
        mcp::McpPromptInfo, mcp::McpPromptListParams, mcp::McpPromptListResult, mcp::McpPromptRef,
        mcp_server::McpServerHealth, mcp_server::McpServerRefusals,
        extend::ExtendInfo, extend::ExtendListResult, extend::ExtendHealth, extend::ExtendUsage, extend::ExtendListParams, extend::ExtendHandsInfo,
        extend::ExtendLoadedInfo, extend::ExtensionRevokeParams, extend::ExtensionRevokeResult, grants::GrantOffer, grants::GrantAnswer, modes::SessionMode, modes::PlanState, modes::PlanItem, modes::SessionModeParams, modes::SessionModeResult, modes::SessionModeChanged, rsi::SelfMode, rsi::SelfState, rsi::SealedState, rsi::SelfHaltParams, rsi::SelfResumeParams, rsi::SelfSwitchResult, rsi::SelfLogParams, rsi::SelfLogRow, rsi::SelfLogResult, rsi::SelfDigestParams, rsi::SelfDigestResult, rsi::SelfExamsParams, rsi::SelfExamsResult, rsi::SelfExamFamily, rsi::SelfExamToday, rsi::SelfBranchesParams, rsi::SelfBranch, rsi::SelfBranchesResult, rsi::SelfBacklogParams, rsi::SelfBacklogItem, rsi::SelfBacklogResult, rsi::SelfDropParams, rsi::SelfDropResult, rsi::SelfMoveParams, rsi::SelfMoveResult, rsi::SelfRetryParams, rsi::SelfRetryResult, rsi::SelfBudgetView, rsi::SelfBranchSpend, features::Features, rsi::SelfChanged, rsi::SelfNarrowParams, rsi::SelfNarrowItem, rsi::SelfNarrowResult, rsi::SelfRevertParams, rsi::SelfRevertResult, rsi::SelfPipelineParams, rsi::SelfPipelineItem, rsi::SelfPipelineRoom, rsi::SelfPipelineDay, rsi::SelfPipelineWeek, rsi::SelfPipelineResult, patterns::SelfPatternsParams, patterns::SelfPatternRow, patterns::SelfPatternsResult,
        tasks::TaskState, tasks::TaskOrigin, tasks::TaskEvidence, tasks::TaskProposal,
        tasks::TaskRecord, tasks::TaskChanged,
        tasks::TaskViewSummary, tasks::TaskClaim, tasks::TaskChange, work::WorkKind, answer::WorkAnswerParams, answer::ConfirmListParams, answer::WorkAnswerResult, work::WorkState, work::WorkView, work::DoingKind, work::Doing, work::Progress, work::QuestionKind, work::WorkRef, work::Question, work::Style, work::AnswerOption, work::Rollup, work::Eta, work::Cost, work::WorkReport, work::Woke, fork::SessionForkParams, fork::SessionForkResult, fork::ForkOrigin, fork::Inherits, fork::ReachFork, standing::StandingKind, standing::StandingScope, standing::StandingRecord, standing::StandingInfo, standing::StandingListParams, standing::StandingListResult, standing::StandingAddParams, standing::StandingRevokeParams, standing::StandingChanged, threads::ThreadState, threads::WorkingSet, threads::Seal, threads::ThreadInfo, threads::ThreadRow, threads::ThreadNewParams, threads::ThreadListParams, threads::ThreadListResult, threads::ThreadNameParams, threads::ThreadEditParams, notices::Urgency, notices::Why, notices::Notice, notices::Finished, notices::Rules, notices::Viewer, notices::Delivery, WorkWatchParams, WorkWatchResult, WorkUnwatchResult, WorkChanged, ThreadGroup, routines::CatchUp, routines::Speak, routines::RoutineSpec, routines::RunRow, routines::RoutineInfo, routines::RoutineCreateParams, routines::RoutineListResult, routines::RoutineNameParams, routines::RoutineRunsParams, routines::RoutineRunsResult, routines::RoutinePauseParams, routines::RoutineUpdateParams, routines::RoutineEditResult, threads::Speaking, threads::ThreadChanged, threads_import::ThreadImportParams, threads_import::ThreadCandidate, threads_import::ThreadApplied, threads_import::ThreadSkipped, threads_import::ThreadImportResult, threads::ThreadSealParams, threads::ThreadCrossed, threads::ThreadSealResult, thread_route::RouteHow, thread_route::ThreadTag, thread_route::AskOption, thread_route::ThreadAsk, thread_route::ThreadAnswerParams, thread_route::ThreadAnswerResult, thread_route::ThreadMoveParams, thread_route::ThreadMoveResult, thread_placed::ThreadPlaced, job_follow::WorkPeekParams, job_follow::WorkPeekResult, job_follow::JobStopParams, job_follow::JobStopResult, job_follow::ActionOutputParams, job_follow::ActionOutputResult, job_follow::ActionOutputChunk, job_follow::ActionOutputEnd, job_follow::ActionUnfollowParams, job_follow::ActionUnfollowResult, own_bench::TurnReplayParams, own_bench::ReplayReply, own_bench::ReplayVerdict, own_bench::TurnReplayResult, own_bench::RouteReportParams, own_bench::RouteCell, own_bench::DataCell, own_bench::GateCount, own_bench::DraftCell, own_bench::RouteReportResult, forecast::WorkForecast, forecast::ForecastCalibrationParams, forecast::CalibrationClass, forecast::CalibrationWindow, forecast::ForecastCalibrationResult, rewind::FilesChangesParams, rewind::FileChange, rewind::RewindPoint, rewind::FilesChangesResult, rewind::FilesRestoreParams, rewind::RestoredFile, rewind::FilesRestoreResult, persona::PersonaListParams, persona::PersonaListResult, persona::PersonaInfo, persona::PersonaFile, persona::PersonaTally, persona::PersonaPlace, thread_cache::ThreadCacheStats, thread_cache::ThreadCacheEntry, thread_cache::ThreadsCache, thread_cache::ThreadsHealth, threads::ThreadMergeParams, threads::ThreadMergeResult, threads::ThreadSplitParams, threads::ThreadSplitResult, threads::ThreadSplitPlan, threads::ThreadUnsplitParams, threads::ThreadUnsplitResult, threads::ThreadUnclaimParams, threads::ThreadUnclaimResult, threads::ThreadLinkParams, threads::SplitEstimate, own_bench::TestRun, own_bench::WholeCall, own_bench::WholeSame, own_bench::WholeReplay, prefix::PrefixReportParams, prefix::PrefixReport, prefix::FalseFit, work::horizon::Series, work::horizon::RoutineLast, work::horizon::RunCounts, work::horizon::Month, work::horizon::RoutineView, work::horizon::WatchLast, work::horizon::WatchView, work::horizon::Tally, work::horizon::NextDue, work::horizon::ProjectView, work::horizon::Horizon, work::horizon::LocalDay, work::horizon::AgendaParams, work::horizon::AgendaWhat, work::horizon::AgendaLine, work::horizon::AgendaResult, voice_pick::VoiceShowParams, voice_pick::VoiceShowResult, voice_pick::VoiceAuditionParams, voice_pick::AuditionClip, voice_pick::AuditionFailure, voice_pick::VoiceAuditionResult, initiative::Initiative, initiative::InitiativeRateParams, initiative::InitiativeRateResult, initiative::InitiativeWeekParams, initiative::InitiativeWeekResult, card::ProposalState, card::CardSource, card::CardProposal, card::CardProposeParams, card::CardProposeResult, card::CardListParams, card::CardListResult, card::CardAnswerParams, card::CardAnswerResult, card::CardDoneParams, card::CardDoneResult, card::CardAsk, card::ObservationKind, card::ObservationState, card::CardObservation, card::CardObservedParams, card::CardObservedResult, persona_import::PersonaImportParams, persona_import::ImportedFile, persona_import::OverLimit, persona_import::ProposedStanding, persona_import::PersonaImportResult, persona_import::PersonaCardConfirmParams, persona_import::PersonaCardConfirmResult, persona_write::PersonaAddParams, persona_write::PersonaSoulParams, persona_write::PersonaSetParams, persona_write::PersonaKeyParams, persona_write::PersonaWriteResult, persona_write::PersonaSoulReadParams, persona_write::PersonaSoulBackup, persona_write::PersonaSoulReadResult, standing_proposals::StandingProposal, standing_proposals::StandingProposalsParams, standing_proposals::StandingProposalsResult, standing_proposals::StandingProposalAnswer, standing_proposals::StandingProposalAnswered, tasks::TaskKind, projects::ProjectProgress, projects::ProjectSliceSummary, projects::ProjectInfo, projects::MilestoneSpec, projects::ProjectCreateParams, projects::ProjectCreateResult, projects::ProjectListParams, projects::ProjectListResult, projects::ProjectShowParams, projects::ProjectShowResult, projects::ProjectAttachParams, projects::ProjectAttachResult, standing::StandingAsk, standing::StandingNoticed, standing::StandingAnswerParams, standing::StandingAnswerResult, WorkDigestParams, WorkDigestResult, DigestKind, DigestLine, DigestMore, digest_google::DigestGoogle, google::GoogleAuthBeginParams, google::GoogleAuthBeginResult, google::GoogleAuthFinishParams, google::GoogleAuthFinishResult, google::GoogleRevokeParams, google::GoogleRevokeResult, google::GoogleStatusParams, google::GoogleStatus, calendar::CalendarWindowParams, calendar::CalendarStatus, calendar::CalendarEvent, calendar::CalendarWindow, calendar::CalendarWindowResult, calendar::CalendarChanged,
    }
    let files: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|f| f.ends_with(".ts"))
        .collect();
    for (name, ts) in declared() {
        assert!(ts, "{name} is a wire type: derive TS on it (theseus-0g4)");
        assert!(
            files.contains(&format!("{name}.ts")),
            "{name} has no TypeScript: add it to the list above"
        );
    }
    let mut index = String::from(
        "// The protocol's types, generated from crates/theseus-protocol (theseus-0g4):\n\
         // `cargo nextest run --workspace -E 'package(theseus-protocol)'` writes them.\n\
         // Do not edit.\n",
    );
    for f in &files {
        index.push_str(&format!(
            "export type * from './{}';\n",
            f.trim_end_matches(".ts")
        ));
    }
    std::fs::write(dir.join("index.ts"), index).unwrap();
}
