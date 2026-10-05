//! The cockpit's protocol types, generated from these (theseus-0g4; Eddie's
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
        include_str!("check.rs"),
        include_str!("tasks.rs"),
        include_str!("events.rs"),
        include_str!("gate.rs"),
        include_str!("index.rs"),
        include_str!("mcp.rs"),
        include_str!("extend.rs"),
        include_str!("memory.rs"),
        include_str!("learning.rs"),
        include_str!("judge_runs.rs"),
        include_str!("ontology.rs"),
        include_str!("places.rs"),
        include_str!("push.rs"),
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
        ContextStatus, ConfigStatus, NodeCacheHealth,
        ConfigRestart, SecretFailed, SecretSource, StartupPhase, Tightening, PolicyTightenParams,
        PolicyUntightenParams, TightenResult, ExternalText, ExternalTextInfo, PolicyTrustParams,
        TrustResult, DiscordOrigin, BindingStatus, OutboxStatus,
        PlaceStatus, TelemetryStatus, KernelStatus, ChildrenStatus, GrantStatus,
        ExecutionInfo,
        BudgetInfo, ExecutionListResult, ActionInfo, ActionListParams, ActionListResult,
        ExecutionCancelParams, ExecutionCancelResult, ExecutionStopParams, ExecutionStopResult,
        TaskInfo, TaskListParams, TaskListResult, TaskCancelParams, TaskCancelResult, WakeInfo,
        WakeListParams, WakeListResult, WakeCancelParams, WakeCancelResult, TaskRef, SessionKind,
        SessionOpenParams, SessionInfo, SessionListResult, TurnSubmitParams, Attachment,
        ProfileInfo, ProfileListResult, ProfileUseParams, ProfileChanged, Span, Usage,
        TurnSubmitResult, LedgerTailParams, LedgerEntry, LedgerTailResult, SessionRef,
        SessionHistoryParams, NodeInfo, SessionHistoryResult, SessionRecompileParams,
        NodeListParams, NodeListResult, NodeReachParams, ReachCompilation, ReachExposure,
        ReachDescendant, ReachTotals, NodeReachResult, CompilationListParams, CompilationInfo,
        CompilationListResult, CatalogModel, CatalogListResult, ConfirmRequest, BudgetAsk,
        ConfirmListResult, ActionConfirmParams, ActionConfirmResult, ToolInfo, ToolListResult,
        ProviderErrorData, TurnStarted, TurnFailed, LoopStarted, ModelDelta, LoopEnded,
        NarrativePart, NarrativeLine, NarrativeWatchResult, ToolProposed, ContextFileRef,
        CacheSummary, EstimateSummary, CensusSummary, ContextCompiled, ToolStarted, ToolEnded,
        ConfirmResolved, NodeWritten, PolicyNotified, Access, Resource, Plan, Proposal,
        Notice, GateDecision, GateResult, GateRecord, Level, Attention, WaitingOn, PendingConfirm,
        ExecutionView, ExecutionsWatchParams, ExecutionsWatchResult, SessionListParams, PushStatus,
        WaitUntil, SessionWaitParams, SessionWaitResult, EventsLost, ToolClass, AwsPlan, AwsStatus,
        AwsAccountStatus, TenderStatus, PlaceClass, PlacesHealth, PlaceInfo, PlacePublishParams, PublishResult,
        HandsListParams, HandsGroupInfo, HandsListResult,
        BudgetListResult, BudgetRow, BudgetResetInfo, BudgetQuestionInfo, BudgetTotals, JudgeDayBudget,
        PolicyExplainParams, PolicyExplainResult, PlaceExplain, ToolExplain, ExplainLayer, ExplainCondition,
        OntologyListParams, OntologyListResult, OntologyKind, OntologyCategory, OntologyGuidance,
        OntologyMembership, OntologyCategoryAddParams, OntologyGuidanceSetParams,
        OntologyMembershipSetParams, OntologyMembershipResult, OntologyProposalsParams,
        OntologyProposalsResult, OntologyProposal, OntologyProposalAcceptParams,
        OntologyProposalRejectParams, OntologyProposalAnswered, TasksHealth, ParkedTask,
        index::IndexQueryParams, index::IndexWeights, index::IndexFilters, index::IndexSourceRank,
        index::IndexHit, index::IndexTimings, index::IndexLag, index::IndexQueryResult,
        index::IndexStamp, index::IndexEmbedTask, index::IndexNeighboursParams,
        index::IndexNeighbour, index::IndexNeighboursResult, index::IndexEmbedParams,
        index::IndexEmbedResult, index::IndexEntitiesParams, index::IndexEntitiesResult,
        index::IndexForgetParams, index::IndexChunkRef,
        index::IndexForgetResult, index::IndexWarmResult, index::IndexBackfill, index::IndexStatus,
        index::IndexVectorStatus, index::IndexCompactions, index::IndexReembed,
        index::IndexEmbedStats, index::IndexRebuildResult, index::IndexHealth,
        memory::MemorySearchParams, memory::MemoryRecallsParams, memory::MemoryRecallsResult,
        memory::RecallManifest, memory::RecallRerank, memory::RecallActivation, memory::MemoryHealth, memory::AdjacencyHealth, memory::RecallItem, memory::RecallDrop, memory::RecallTimings,
        memory::BudgetReport, memory::BudgetDrop, memory::BudgetRange, memory::BudgetOverage,
        memory::MemoryLabelParams, memory::MemoryLabelResult, memory::RecallRetention, memory::MemoryConsolidateParams, memory::MemoryConsolidateResult, memory::SynthesisReport,
        sandbox::SandboxHealth, judge::JudgeHealth, judge::JudgeScored, judge::JudgeNoticed, judge::JudgeListParams, judge::JudgeListResult,
        judge::JudgeGetParams, judge::JudgeGetResult, learning::JudgeLabelParams, learning::JudgeLabelResult, packs::PackListResult, packs::PackInfo, packs::PackModeRow, packs::HoldoutBounds, packs::PackPromoteParams, packs::PackPromoteResult, packs::PackRollbackParams,
        learning::LearningReportParams, learning::LearningReport, judge_runs::JudgeReplayParams, judge_runs::ReplayLeftOut, judge_runs::ReplayJudgment, judge_runs::ReplayClassFell, judge_runs::ReplayEvalSide,
        judge_runs::ReplayEval, judge_runs::JudgeReplayResult, judge_runs::JudgeAuditParams, judge_runs::JudgeAuditResult, judge_runs::JudgeBackfillParams, judge_runs::JudgeBackfillResult, judge_runs::JudgeLearnParams, judge_runs::LearnClass, judge_runs::LearnQuestion, judge_runs::LearnThreshold, judge_runs::JudgeProposal, sandbox::SandboxLaunch, cancel::CancelVerdict, cancel::CancelCount,
        signals::CompileSignal, route::TurnRoute, route::TurnFallback, sandbox::SandboxUsage, sandbox::RunningJob, lsp::LspServerStatus,
        bench::BenchHistoryParams, bench::BenchHistoryResult, bench::BenchRun, bench::BenchPhase, cred::HarnessOnly,
        AwsBudgetStatus, AwsGuardDutyStatus, AwsBootstrapParams, AwsBootstrapStack,
        AwsBootstrapResult, AwsConfirmAlertsParams, AwsConfirmAlertsResult, TaskArrangement,
        ArrangementPiece, TaskCheck, CheckPiece, CheckOverlap,
        mcp::McpServerStatus, mcp::McpToolInfo, mcp::McpListResult,
        mcp::McpRestartParams, mcp::McpRestartResult, mcp::McpPromptArgument,
        mcp::McpPromptInfo, mcp::McpPromptListParams, mcp::McpPromptListResult, mcp::McpPromptRef,
        mcp_server::McpServerHealth, mcp_server::McpServerRefusals,
        extend::ExtendInfo, extend::ExtendListResult, extend::ExtendHealth,
        extend::ExtendLoadedInfo, extend::ExtensionRevokeParams, extend::ExtensionRevokeResult,
        tasks::TaskState, tasks::TaskOrigin, tasks::TaskEvidence, tasks::TaskProposal,
        tasks::TaskRecord, tasks::TaskGetParams, tasks::TaskGetResult, tasks::TaskChanged,
        tasks::TaskViewSummary, tasks::TaskClaim, tasks::TaskChange,
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
