//! The web apps' protocol types, generated from these (theseus-0g4; Eddie's
//! call, 2026-10-01): one file per type in `web/src/protocol.gen/`, and
//! `index.ts` exporting them all. `web/src/protocol.ts` re-exports them, so the
//! Observatory and the cockpit import what they always did.
//!
//! The test writes the files, and the gate fails when they differ from the
//! commit, as it does for `web/dist`: a Rust type changed without its
//! TypeScript fails the gate. Large integers are `number`, since the wire is
//! JSON and `JSON.parse` reads numbers. A `serde_json::Value` field says its
//! TypeScript type itself (`#[ts(type = "...")]`).
//!
//! The place: the cockpit already imports `web/src/protocol.ts` through its
//! `@protocol` alias, so a directory beside it serves both apps with no new
//! path, and `.gen` says nobody edits it.

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/protocol.gen")
}

/// The types this crate declares, by its sources: (name, derives `TS`).
fn declared() -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for src in [
        include_str!("lib.rs"),
        include_str!("events.rs"),
        include_str!("gate.rs"),
        include_str!("index.rs"),
        include_str!("label.rs"),
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
fn the_web_apps_types_are_generated_from_the_rust_ones() {
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
        Id, Request, Notification, RpcError, Response, Message, HealthResult, SpoolStatus,
        SpoolSweep, DiskStatus, BinaryStatus, StoreStatus, CrashStatus, WebStatus, SecretsStatus,
        ContextStatus, ConfigStatus,
        ConfigRestart, SecretFailed, StartupPhase, Tightening, PolicyTightenParams,
        PolicyUntightenParams, TightenResult, ExternalText, ExternalTextInfo, PolicyTrustParams,
        TrustResult, ApprovalStatus, ApprovalChannel, DiscordOrigin, BindingStatus, OutboxStatus,
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
        ConfirmResolved,
        NodeWritten, PolicyNotified, Asker, ApprovalRefused, Access, Resource, Plan, Proposal,
        Notice, GateDecision, GateResult, GateRecord, Level, Attention, WaitingOn, PendingConfirm,
        ExecutionView, ExecutionsWatchParams, ExecutionsWatchResult, SessionListParams, PushStatus,
        WaitUntil, SessionWaitParams, SessionWaitResult, EventsLost, ToolClass, AwsPlan, AwsStatus,
        AwsAccountStatus, TenderStatus, Integrity, Readers, Label, Audience, Withheld, InPlay, LabelsHealth, PlaceAudience,
        Warrant, LabelGraduateParams, GraduateResult, HeldPosts,
        index::IndexQueryParams, index::IndexWeights, index::IndexFilters, index::IndexSourceRank,
        index::IndexHit, index::IndexTimings, index::IndexLag, index::IndexQueryResult,
        index::IndexStamp, index::IndexEmbedTask, index::IndexNeighboursParams,
        index::IndexNeighbour, index::IndexNeighboursResult, index::IndexEmbedParams,
        index::IndexEmbedResult, index::IndexForgetParams, index::IndexChunkRef,
        index::IndexForgetResult, index::IndexWarmResult, index::IndexBackfill, index::IndexStatus,
        index::IndexVectorStatus, index::IndexCompactions, index::IndexReembed,
        index::IndexEmbedStats, index::IndexRebuildResult, index::IndexHealth,
        sandbox::SandboxHealth, sandbox::SandboxProbe, cancel::CancelVerdict, cancel::CancelCount,
        sandbox::SandboxUsage, sandbox::JobUsage, bench::BenchHistoryParams,
        bench::BenchHistoryResult, bench::BenchRun, bench::BenchPhase,
        SecretRequested, cred::CredRequests, cred::HarnessOnly,
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
