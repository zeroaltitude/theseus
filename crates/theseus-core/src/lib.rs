//! Theseus kernel. See `docs/the-ship-of-theseus.md`.
//!
//! M0 First light: config and secrets from 1Password, a turn runner with an
//! empty tool list, an Advancer (the turn's today is `UntilNoToolCalls`).
//! (M0's hook events, defined but never handled, came out in theseus-hco.)
//! M1 Keel: the WAL store. M2 Kernel: executions, actions, completions, the
//! spool, admission, budgets, the harness loop (`theseus-kernel` + `harness`).

pub mod advancer;
pub mod approval;
pub mod arrangement;
pub mod ask;
pub mod attach;
pub mod aws;
pub mod bench;
pub mod binary;
pub mod blobs;
pub mod books;
pub mod broker;
pub mod bus;
pub mod cancel;
pub mod capture;
pub mod card;
pub mod catalog;
pub mod category_recall;
pub mod ceiling;
pub mod census;
pub mod cgroup;
pub mod check;
pub mod commitments;
pub mod compiler;
pub mod config;
pub mod config_copy;
pub mod config_gate;
pub mod conns;
pub mod consolidate;
pub mod content;
pub mod content_scene;
pub mod content_wait;
pub mod context_files;
pub mod context_parts;
pub mod correction;
pub mod cpu;
pub mod crash;
pub mod data_gate;
pub mod day_ceiling;
pub mod disk;
pub mod draft;
pub mod draft_view;
pub mod egress;
pub mod engage;
pub mod exits;
pub mod extend;
pub mod external;
pub mod fact;
pub mod features;
pub mod file_read;
pub mod first_request;
pub mod flow;
pub mod forecast;
pub mod fork;
pub mod github;
pub mod glide;
pub mod google;
pub mod graph;
pub mod harness;
pub mod import;
pub mod initiative;
pub mod judge;
pub mod keep_warm;
pub mod keep_warm_threads;
pub mod keep_warm_wait;
pub mod learning;
pub mod ledger;
pub mod ledger_pin;
pub mod lsp;
pub mod mcp;
pub mod mcp_server;
pub mod memory_lookup;
pub mod memory_pass;
pub mod moved;
pub mod narrative;
pub mod native;
pub mod needs_you;
pub mod node;
pub mod node_cache;
pub mod one_shot;
pub mod ontology;
pub mod outbound;
pub mod outbox;
pub mod outputs;
pub mod own_bench;
pub mod owner_note;
pub mod pacer;
pub mod pack_names;
pub mod parked;
pub mod peer;
pub mod peer_proc;
pub mod persona;
pub mod persona_import;
pub mod persona_recall;
pub mod personas;
pub mod place_warnings;
pub mod places;
pub mod policy;
pub mod prefix;
pub mod project;
pub mod project_tool;
pub mod provider;
pub mod push;
pub mod reach;
pub mod recall;
pub mod resident;
pub mod restore;
pub mod retract;
pub mod rewind;
pub mod routine;
pub mod routine_tool;
pub mod routing;
pub mod rpc;
pub mod rsi;
pub mod sandbox;
pub mod scrub;
pub mod secrets;
pub mod self_card;
pub mod session;
pub mod signals;
pub mod split_turns;
pub mod standing;
pub mod standing_proposals;
pub mod startup;
pub mod store;
pub mod stub;
pub mod succession;
pub mod surface;
pub mod sweep;
pub mod task;
pub mod task_fan_out;
pub mod task_graph;
mod task_home;
pub mod task_self;
pub mod task_start;
pub mod task_wait;
pub mod telemetry;
pub mod tender;
pub mod term;
pub mod thread_context;
pub mod thread_flow;
pub mod thread_grow;
pub mod thread_join;
pub mod thread_link_seen;
pub mod thread_read;
pub mod thread_recall;
pub mod thread_route;
pub mod thread_split;
pub mod thread_title;
pub mod threads;
pub mod threads_import;
pub mod tighten;
pub mod tool_set;
pub mod tool_words;
pub mod toolrun;
pub mod tools_defer;
pub mod trace;
pub mod transcripts;
pub mod turn;
pub mod voice;
pub mod wake;
pub mod wake_home;
pub mod wake_tools;
pub mod wake_when;
pub mod warm;
pub mod watch;
pub mod watch_tool;
pub mod web;
pub mod webui;
mod wrapper_lost;

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
mod scratch;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests_accept_batch;
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
mod tests_arrangement_quote;
#[cfg(test)]
mod tests_ask;
#[cfg(test)]
mod tests_audit;
#[cfg(test)]
mod tests_backfill;
#[cfg(test)]
mod tests_bg_gate;
#[cfg(test)]
mod tests_books;
#[cfg(test)]
mod tests_budget_loop;
#[cfg(test)]
mod tests_budgets;
#[cfg(test)]
mod tests_cancel;
#[cfg(test)]
mod tests_capture;
#[cfg(test)]
mod tests_categorize;
#[cfg(test)]
mod tests_ceilings;
#[cfg(test)]
mod tests_census;
#[cfg(test)]
mod tests_check;
#[cfg(test)]
mod tests_check_view;
#[cfg(test)]
mod tests_checkpoint_effort;
#[cfg(test)]
mod tests_cite;
#[cfg(test)]
mod tests_coding_hint;
#[cfg(test)]
mod tests_commitments;
#[cfg(test)]
mod tests_compaction;
#[cfg(test)]
mod tests_config;
#[cfg(test)]
mod tests_consolidate;
#[cfg(test)]
mod tests_content_cut;
#[cfg(test)]
mod tests_content_floor;
#[cfg(test)]
mod tests_content_freeze;
#[cfg(test)]
mod tests_content_gate;
#[cfg(test)]
mod tests_content_kind;
#[cfg(test)]
mod tests_content_late;
#[cfg(test)]
mod tests_content_race;
#[cfg(test)]
mod tests_context;
#[cfg(test)]
mod tests_continuations;
#[cfg(test)]
mod tests_continue;
#[cfg(test)]
mod tests_continue_slow;
#[cfg(test)]
mod tests_data_gate;
#[cfg(test)]
mod tests_day_ceiling;
#[cfg(test)]
mod tests_day_notify;
#[cfg(test)]
mod tests_day_total;
#[cfg(test)]
mod tests_defer;
#[cfg(test)]
mod tests_detour;
#[cfg(test)]
mod tests_disk_watch;
#[cfg(test)]
mod tests_dispatch;
#[cfg(test)]
mod tests_dm_rig;
#[cfg(test)]
mod tests_draft;
#[cfg(test)]
mod tests_draft_first_pass;
#[cfg(test)]
mod tests_draft_hardening;
#[cfg(test)]
mod tests_draft_home;
#[cfg(test)]
mod tests_draft_reach;
#[cfg(test)]
mod tests_effort_beta;
#[cfg(test)]
mod tests_effort_lapse;
#[cfg(test)]
mod tests_effort_sticky;
#[cfg(test)]
mod tests_egress;
#[cfg(test)]
mod tests_end_note;
#[cfg(test)]
mod tests_engage;
#[cfg(test)]
mod tests_explain;
#[cfg(test)]
mod tests_failures;
#[cfg(test)]
mod tests_fallback;
#[cfg(test)]
mod tests_families;
#[cfg(test)]
mod tests_fan_out_cue;
#[cfg(test)]
mod tests_find_sticks;
#[cfg(test)]
mod tests_first_request;
#[cfg(test)]
mod tests_fit;
#[cfg(test)]
mod tests_fork;
#[cfg(test)]
mod tests_fresh_recall;
#[cfg(test)]
mod tests_gate_layers;
#[cfg(test)]
mod tests_glide;
#[cfg(test)]
mod tests_grants;
#[cfg(test)]
mod tests_harness;
#[cfg(test)]
mod tests_hold_compile;
#[cfg(test)]
mod tests_home_size;
#[cfg(test)]
mod tests_horizon;
#[cfg(test)]
mod tests_inbound;
#[cfg(test)]
mod tests_initiative;
#[cfg(test)]
mod tests_injection_tiers;
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
mod tests_keep_warm;
#[cfg(test)]
mod tests_kept;
#[cfg(test)]
mod tests_ladder;
#[cfg(test)]
mod tests_ladder_unread;
#[cfg(test)]
mod tests_layouts;
#[cfg(test)]
mod tests_leaks;
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
mod tests_manifest_harness;
#[cfg(test)]
mod tests_marker;
#[cfg(test)]
mod tests_mcp_memory;
#[cfg(test)]
mod tests_memory_arm;
#[cfg(test)]
mod tests_memory_lookup;
#[cfg(test)]
mod tests_money;
#[cfg(test)]
mod tests_never;
#[cfg(test)]
mod tests_notices;
#[cfg(test)]
mod tests_observability;
#[cfg(test)]
mod tests_offer;
#[cfg(test)]
mod tests_one_turn;
#[cfg(test)]
mod tests_onto;
#[cfg(test)]
mod tests_onto_digest;
#[cfg(test)]
mod tests_onto_judged;
#[cfg(test)]
mod tests_onto_mark;
#[cfg(test)]
mod tests_onto_mint;
#[cfg(test)]
mod tests_onto_names;
#[cfg(test)]
mod tests_onto_other;
#[cfg(test)]
mod tests_onto_sweep;
#[cfg(test)]
mod tests_ontology;
#[cfg(test)]
mod tests_ontology_bind;
#[cfg(test)]
mod tests_ontology_undo;
#[cfg(test)]
mod tests_open_task;
#[cfg(test)]
mod tests_output;
#[cfg(test)]
mod tests_outside_text;
#[cfg(test)]
mod tests_overflow;
#[cfg(test)]
mod tests_owner_note;
#[cfg(test)]
mod tests_people_proposals;
#[cfg(test)]
mod tests_persona;
#[cfg(test)]
mod tests_persona_layer;
#[cfg(test)]
mod tests_persona_recall;
#[cfg(test)]
mod tests_personal_hold;
#[cfg(test)]
mod tests_personal_warn;
#[cfg(test)]
mod tests_personas;
#[cfg(test)]
mod tests_places;
#[cfg(test)]
mod tests_private_authored;
#[cfg(test)]
mod tests_private_view;
#[cfg(test)]
mod tests_proc_cap;
#[cfg(test)]
mod tests_progress;
#[cfg(test)]
mod tests_projects;
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
mod tests_recall_current;
#[cfg(test)]
mod tests_recall_deadline;
#[cfg(test)]
mod tests_recall_neighbour;
#[cfg(test)]
mod tests_recall_node;
#[cfg(test)]
mod tests_recall_precision;
#[cfg(test)]
mod tests_recall_time;
#[cfg(test)]
mod tests_recall_when;
#[cfg(test)]
mod tests_recall_words;
#[cfg(test)]
mod tests_refused;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_reminders;
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
mod tests_retract;
#[cfg(test)]
mod tests_retry;
#[cfg(test)]
mod tests_route;
#[cfg(test)]
mod tests_route_base;
#[cfg(test)]
mod tests_route_cap;
#[cfg(test)]
mod tests_route_correct;
#[cfg(test)]
mod tests_route_data;
#[cfg(test)]
mod tests_route_defer;
#[cfg(test)]
mod tests_route_down;
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
mod tests_route_vocab;
#[cfg(test)]
mod tests_route_wait;
#[cfg(test)]
mod tests_route_warm;
#[cfg(test)]
mod tests_routine_stop;
#[cfg(test)]
mod tests_routine_task;
#[cfg(test)]
mod tests_routine_trust;
#[cfg(test)]
mod tests_routines;
#[cfg(test)]
mod tests_sandbox;
#[cfg(test)]
mod tests_security;
#[cfg(test)]
mod tests_self_budget;
#[cfg(test)]
mod tests_self_card;
#[cfg(test)]
mod tests_self_turns;
#[cfg(test)]
mod tests_send_path;
#[cfg(test)]
mod tests_session_dir;
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
mod tests_standing;
#[cfg(test)]
mod tests_standing_ring;
#[cfg(test)]
mod tests_status_cache;
#[cfg(test)]
mod tests_steer;
#[cfg(test)]
mod tests_steps;
#[cfg(test)]
mod tests_stop_nudge;
#[cfg(test)]
mod tests_stopping;
#[cfg(test)]
mod tests_stub_kinds;
#[cfg(test)]
mod tests_stubs;
#[cfg(test)]
mod tests_task_brief;
#[cfg(test)]
mod tests_task_checkpoint;
#[cfg(test)]
mod tests_task_claims;
#[cfg(test)]
mod tests_task_close;
#[cfg(test)]
mod tests_task_fan_out;
#[cfg(test)]
mod tests_task_graph;
#[cfg(test)]
mod tests_task_join;
#[cfg(test)]
mod tests_task_layers;
#[cfg(test)]
mod tests_task_report;
#[cfg(test)]
mod tests_task_reports_of;
#[cfg(test)]
mod tests_task_search;
#[cfg(test)]
mod tests_task_self;
#[cfg(test)]
mod tests_task_start;
#[cfg(test)]
mod tests_task_wait;
#[cfg(test)]
mod tests_task_wakes;
#[cfg(test)]
mod tests_tasks;
#[cfg(test)]
mod tests_tasks_deferred;
#[cfg(test)]
mod tests_tasks_wake;
#[cfg(test)]
mod tests_tender;
#[cfg(test)]
mod tests_term;
#[cfg(test)]
mod tests_thinking_writer;
#[cfg(test)]
mod tests_tiering;
#[cfg(test)]
mod tests_tool_words;
#[cfg(test)]
mod tests_trim_tokens;
#[cfg(test)]
mod tests_trivial_tools;
#[cfg(test)]
mod tests_turn_checkpoint;
#[cfg(test)]
mod tests_turn_effort;
#[cfg(test)]
mod tests_turn_progress;
#[cfg(test)]
mod tests_turn_reserve;
#[cfg(test)]
mod tests_turn_retry;
#[cfg(test)]
mod tests_unlabel;
#[cfg(test)]
mod tests_wait_cost;
#[cfg(test)]
mod tests_waiting_split;
#[cfg(test)]
mod tests_wakes;
#[cfg(test)]
mod tests_web_offer;
#[cfg(test)]
mod tests_wire_mark;
#[cfg(test)]
mod tests_work;
#[cfg(test)]
mod tests_work_doing;
