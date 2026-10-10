//! Theseus kernel. See `docs/the-ship-of-theseus.md`.
//!
//! M0 First light: config and secrets from 1Password, a turn runner with an
//! empty tool list, an Advancer whose only policy is `stop_after_one_loop`.
//! (M0's hook events, defined but never handled, came out in theseus-hco.)
//! M1 Keel: the WAL store. M2 Kernel: executions, actions, completions, the
//! spool, admission, budgets, the harness loop (`theseus-kernel` + `harness`).

pub mod advancer;
pub mod approval;
pub mod arrangement;
pub mod attach;
pub mod aws;
pub mod bench;
pub mod binary;
pub mod blobs;
pub mod books;
pub mod broker;
pub mod bus;
pub mod cancel;
pub mod catalog;
pub mod ceiling;
pub mod cgroup;
pub mod check;
pub mod compiler;
pub mod config;
pub mod config_copy;
pub mod config_gate;
pub mod consolidate;
pub mod context_files;
pub mod context_parts;
pub mod correction;
pub mod cpu;
pub mod crash;
pub mod day_ceiling;
pub mod disk;
pub mod egress;
pub mod extend;
pub mod external;
pub mod fact;
pub mod file_read;
pub mod github;
pub mod glide;
pub mod graph;
pub mod harness;
pub mod import;
pub mod judge;
pub mod learning;
pub mod ledger;
pub mod lsp;
pub mod mcp;
pub mod mcp_server;
pub mod memory_lookup;
pub mod memory_pass;
pub mod narrative;
pub mod node;
pub mod node_cache;
pub mod one_shot;
pub mod ontology;
pub mod outbound;
pub mod outbox;
pub mod parked;
pub mod peer;
pub mod place_warnings;
pub mod places;
pub mod policy;
pub mod provider;
pub mod push;
pub mod reach;
pub mod recall;
pub mod resident;
pub mod restore;
pub mod routing;
pub mod rpc;
pub mod sandbox;
pub mod scrub;
pub mod secrets;
pub mod session;
pub mod signals;
pub mod startup;
pub mod store;
pub mod stub;
pub mod succession;
pub mod sweep;
pub mod task;
pub mod task_graph;
pub mod telemetry;
pub mod tender;
pub mod term;
pub mod tighten;
pub mod toolrun;
pub mod trace;
pub mod turn;
pub mod voice;
pub mod wake;
pub mod web;
pub mod webui;

pub use config::Config;
pub use rpc::Core;

pub const NAME: &str = "theseus";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit the running binary was built from (theseus-9o5n): a constant
/// of the binary that serves, which names it once, before its core is built
/// (theseusd's build script takes it at compile time). The core is a library
/// of that binary, so the binary says it; a core in a test has none.
static COMMIT: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Name the binary's commit, once: a later call changes nothing. An empty
/// commit (a build outside a git checkout) is none.
pub fn set_commit(commit: &'static str) {
    if !commit.is_empty() {
        let _ = COMMIT.set(commit);
    }
}

/// This binary's build, as health and `server.started` name it.
pub fn build() -> theseus_protocol::Build {
    theseus_protocol::Build {
        version: VERSION.into(),
        commit: COMMIT.get().map(|c| (*c).to_string()),
    }
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}

/// When [`new_id`] minted `id`, in Unix milliseconds: a UUIDv7's first 48
/// bits. None for an id of any other shape.
pub fn id_ms(id: &str) -> Option<u64> {
    let (_, tail) = id.rsplit_once('_')?;
    let u = uuid::Uuid::try_parse(tail).ok()?;
    (u.get_version_num() == 7).then(|| (u.as_u128() >> 80) as u64)
}

#[cfg(test)]
mod tests_activation;
#[cfg(test)]
mod tests_activation_arm;
#[cfg(test)]
mod tests_activation_pace;
#[cfg(test)]
mod tests_activation_search;
#[cfg(test)]
mod tests_actor;
#[cfg(test)]
mod tests_approvals;
#[cfg(test)]
mod tests_arrangement;
#[cfg(test)]
mod tests_audit;
#[cfg(test)]
mod tests_backfill;
#[cfg(test)]
mod tests_books;
#[cfg(test)]
mod tests_budget_loop;
#[cfg(test)]
mod tests_budgets;
#[cfg(test)]
mod tests_cancel;
#[cfg(test)]
mod tests_categorize;
#[cfg(test)]
mod tests_ceilings;
#[cfg(test)]
mod tests_check;
#[cfg(test)]
mod tests_check_view;
#[cfg(test)]
mod tests_compaction;
#[cfg(test)]
mod tests_config;
#[cfg(test)]
mod tests_consolidate;
#[cfg(test)]
mod tests_context;
#[cfg(test)]
mod tests_continuations;
#[cfg(test)]
mod tests_continue;
#[cfg(test)]
mod tests_continue_slow;
#[cfg(test)]
mod tests_day_ceiling;
#[cfg(test)]
mod tests_disk_watch;
#[cfg(test)]
mod tests_egress;
#[cfg(test)]
mod tests_explain;
#[cfg(test)]
mod tests_external;
#[cfg(test)]
mod tests_failures;
#[cfg(test)]
mod tests_fallback;
#[cfg(test)]
mod tests_gate_layers;
#[cfg(test)]
mod tests_glide;
#[cfg(test)]
mod tests_grants;
#[cfg(test)]
mod tests_harness;
#[cfg(test)]
mod tests_inbound;
#[cfg(test)]
mod tests_job_handles;
#[cfg(test)]
mod tests_jobs;
#[cfg(test)]
mod tests_judge;
#[cfg(test)]
mod tests_judge_reads;
#[cfg(test)]
mod tests_judge_surfaces;
#[cfg(test)]
mod tests_ladder;
#[cfg(test)]
mod tests_ladder_unread;
#[cfg(test)]
mod tests_layouts;
#[cfg(test)]
mod tests_learn_loop;
#[cfg(test)]
mod tests_learning;
#[cfg(test)]
mod tests_limits_notify;
#[cfg(test)]
mod tests_lsp;
#[cfg(test)]
mod tests_lsp_edits;
#[cfg(test)]
mod tests_m3;
#[cfg(test)]
mod tests_memory_arm;
#[cfg(test)]
mod tests_memory_lookup;
#[cfg(test)]
mod tests_notices;
#[cfg(test)]
mod tests_ontology;
#[cfg(test)]
mod tests_output;
#[cfg(test)]
mod tests_outside_text;
#[cfg(test)]
mod tests_overflow;
#[cfg(test)]
mod tests_people_proposals;
#[cfg(test)]
mod tests_places;
#[cfg(test)]
mod tests_prove;
#[cfg(test)]
mod tests_push;
#[cfg(test)]
mod tests_push_once;
#[cfg(test)]
mod tests_reach;
#[cfg(test)]
mod tests_recall;
#[cfg(test)]
mod tests_recall_deadline;
#[cfg(test)]
mod tests_recall_node;
#[cfg(test)]
mod tests_recall_when;
#[cfg(test)]
mod tests_recall_words;
#[cfg(test)]
mod tests_refused;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_replay;
#[cfg(test)]
mod tests_rerank;
#[cfg(test)]
mod tests_rerank_labels;
#[cfg(test)]
mod tests_rerank_links;
#[cfg(test)]
mod tests_rerank_live;
#[cfg(test)]
mod tests_retention;
#[cfg(test)]
mod tests_route;
#[cfg(test)]
mod tests_route_base;
#[cfg(test)]
mod tests_route_cap;
#[cfg(test)]
mod tests_route_correct;
#[cfg(test)]
mod tests_route_effort;
#[cfg(test)]
mod tests_route_keep;
#[cfg(test)]
mod tests_route_late;
#[cfg(test)]
mod tests_route_measure;
#[cfg(test)]
mod tests_route_model;
#[cfg(test)]
mod tests_route_rows;
#[cfg(test)]
mod tests_route_wait;
#[cfg(test)]
mod tests_sandbox;
#[cfg(test)]
mod tests_security;
#[cfg(test)]
mod tests_sink_backlog;
#[cfg(test)]
mod tests_sink_between;
#[cfg(test)]
mod tests_sink_blobs;
#[cfg(test)]
mod tests_sink_busy;
#[cfg(test)]
mod tests_sink_flush;
#[cfg(test)]
mod tests_sink_off_turn;
#[cfg(test)]
mod tests_situation;
#[cfg(test)]
mod tests_stack;
#[cfg(test)]
mod tests_steps;
#[cfg(test)]
mod tests_stopping;
#[cfg(test)]
mod tests_stub_kinds;
#[cfg(test)]
mod tests_task_claims;
#[cfg(test)]
mod tests_task_graph;
#[cfg(test)]
mod tests_task_layers;
#[cfg(test)]
mod tests_task_wakes;
#[cfg(test)]
mod tests_tasks;
#[cfg(test)]
mod tests_tender;
#[cfg(test)]
mod tests_term;
#[cfg(test)]
mod tests_thinking_writer;
#[cfg(test)]
mod tests_tiering;
#[cfg(test)]
mod tests_turn_reserve;
#[cfg(test)]
mod tests_wakes;
#[cfg(test)]
mod tests_web_offer;
