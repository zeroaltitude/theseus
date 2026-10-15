//! The first request's budget in tokens, as sent (theseus-to0xw).
//!
//! A fresh session's first request is counted for a kind of place the way the
//! provider bills it: the deferred tools at their constant and the search tool
//! at its size (`ProviderRequest::census`), at the model's figures. It is the
//! number `theseus census --place <kind>` prints as the request's total, and
//! it is deterministic: nothing is sent and no network is read. It runs in the
//! bench and in `census`, never on a turn's path.
//!
//! The owner's rule for what loads: tools are deferred by default, and one is
//! always loaded only with ledger evidence of use in about 1 of 5 sessions of
//! that place. [`ALWAYS_LOADED`] names every tool a place loads in its first
//! request, each with its reason, and a test fails a tool that is loaded and
//! not named there: adding a tool to a place's loaded set is a decision with
//! its evidence written beside it.

use anyhow::Result;
use serde_json::Value;

use crate::catalog::TokenRates;
use crate::tools_defer::{PlaceKind, FIND, SEARCH_TOOL};

/// The most tokens a cli or task place's first request may be, as sent.
pub const CLI_TASK_TOKENS: u64 = 8_000;
/// The most in the owner's DM place.
pub const DM_TOKENS: u64 = 15_000;
/// A shared channel's and a headless run's: today's figure (2026-10-15, the
/// default config) and a tenth over, to the hundred; never above the DM's.
pub const CHANNEL_TOKENS: u64 = 7_900;
pub const HEADLESS_TOKENS: u64 = 8_600;

/// The budget of a kind of place: its census name (`cli`, `dm`, `channel`,
/// `task`, `headless`). `harness` has none: it is the benchmark's profile.
pub fn budget(place: &str) -> Option<u64> {
    match PlaceKind::parse(place)? {
        PlaceKind::Cli | PlaceKind::Task => Some(CLI_TASK_TOKENS),
        PlaceKind::Dm => Some(DM_TOKENS),
        PlaceKind::Channel => Some(CHANNEL_TOKENS),
        PlaceKind::Headless => Some(HEADLESS_TOKENS),
    }
}

/// What a fresh session's first request is on a kind of place.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstRequest {
    /// The whole request, in tokens as sent.
    pub tokens: u64,
    /// The tools it loads: not deferred, and neither the search tool nor `tools_find`.
    pub loaded: Vec<String>,
}

impl crate::rpc::Core {
    /// The first request of a fresh session on `place`, counted as the census counts it.
    pub fn first_request(&self, place: &str) -> Result<FirstRequest> {
        let (live, _) = self.live_profile();
        let next = self.runner.census_request(None, place, &live)?;
        let rates = TokenRates::of(&next.target.model);
        let tokens = crate::census::census_parts(&next.compiled.request, &next, rates).total_tokens;
        let loaded = next
            .compiled
            .request
            .tools
            .iter()
            .filter(|t| t.get("defer_loading") != Some(&Value::Bool(true)))
            .filter_map(|t| t["name"].as_str())
            .filter(|n| *n != SEARCH_TOOL && *n != FIND)
            .map(str::to_string)
            .collect();
        Ok(FirstRequest { tokens, loaded })
    }
}

/// The tools a place loads, each with the place kinds that load it and why.
/// The reason is the evidence: where the ledger shows the use it says so;
/// where it has not been read yet, it says that the entry is a committed
/// default (`tools_defer::PlaceKind::defaults`, `[tools.defer] always_loaded`)
/// the ledger's use column confirms or removes.
pub const ALWAYS_LOADED: &[(&str, &[&str], &str)] = &[
    (
        "fs_read",
        &["cli", "dm", "task", "headless"],
        "files: the work itself; always_loaded by default",
    ),
    ("fs_list", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("fs_glob", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("fs_grep", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("fs_edit", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("fs_patch", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("fs_write", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("git_diff", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("git_log", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    ("text_diff", &["cli", "dm", "task", "headless"], "files: always_loaded by default"),
    (
        "file_read",
        &["cli", "dm", "task", "headless"],
        "files: reads what people gave the model; always_loaded by default",
    ),
    (
        "proc_run",
        &["cli", "dm", "task", "headless"],
        "commands: the work itself; always_loaded by default",
    ),
    ("job_read", &["cli", "dm", "task", "headless"], "commands: reads a proc_run's job; always_loaded"),
    ("job_wait", &["cli", "dm", "task", "headless"], "commands: waits for a proc_run's job; always_loaded"),
    ("job_stop", &["cli", "dm", "task", "headless"], "commands: stops a proc_run's job; always_loaded"),
    (
        "memory_lookup",
        &["cli", "dm"],
        "memory: a committed default of cli and dm (PlaceKind::defaults); the use column of `theseus census` confirms it",
    ),
    (
        "term_open",
        &["headless"],
        "terminals: a committed default of headless runs (programs that want a screen)",
    ),
    ("term_send", &["headless"], "terminals: headless default"),
    ("term_read", &["headless"], "terminals: headless default"),
    ("term_close", &["headless"], "terminals: headless default"),
    (
        "task_create",
        &["dm", "channel"],
        "tasks: a committed default of dm and channel (PlaceKind::defaults): a person's message asks for work to be done later",
    ),
    ("task_update", &["dm", "channel"], "tasks: dm and channel default"),
    ("task_close", &["dm", "channel"], "tasks: dm and channel default"),
    ("task_claim", &["dm", "channel"], "tasks: dm and channel default"),
    ("task_split", &["dm", "channel"], "tasks: dm and channel default"),
    (
        "wake_at",
        &["dm", "channel"],
        "tasks: reminders are a person's ask; dm and channel default",
    ),
    ("wake_move", &["dm"], "tasks: dm default"),
    ("wake_cancel", &["dm"], "tasks: dm default"),
    ("watch", &["dm"], "tasks: dm default"),
    ("routine_create", &["dm"], "tasks: dm default"),
    ("thread_read", &["dm"], "tasks: dm default"),
    (
        "http_fetch",
        &["dm", "channel"],
        "web: a committed default of dm (PlaceKind::defaults); a shared channel's public tool",
    ),
    (
        "channel_post",
        &["channel"],
        "channels: a shared channel is offered the public tools alone, and none is deferred; a DM defers them",
    ),
    ("channel_read", &["channel"], "channels: a shared channel's public tool"),
];

/// Whether `tool` is named for `place` in [`ALWAYS_LOADED`].
pub fn allowed(place: &str, tool: &str) -> bool {
    ALWAYS_LOADED
        .iter()
        .any(|(t, places, _)| *t == tool && places.contains(&place))
}
