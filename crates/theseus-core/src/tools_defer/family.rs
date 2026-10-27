//! The tool families (theseus-7m7w.2): every tool Theseus offers belongs to
//! one, and a request loads whole families or none of one.
//!
//! A family is a coarser thing than `Tool::family()`, which is the name's
//! first dotted part (`fs`, `git`, `text`): `files` holds those three and
//! `file.read`. The classification is by the tool's wire name, so it needs
//! nothing of the registry and a request's tool definitions are enough; a
//! tool it does not know is in `other`, which always loads, so a new tool is
//! never hidden before someone puts it in a family (a test fails the build
//! when a registered tool is in `other`).

use serde::{Deserialize, Serialize};

/// The family of an unknown tool: always loaded.
pub const OTHER: &str = "other";
/// The family of `tools_find`: loaded wherever it is offered.
pub const TOOLS: &str = "tools";
/// The family of the task tools: deferred in a DM, loaded for a message that asks for a task
/// (`want::asks_for_task`, theseus-gg78p).
pub const TASKS: &str = "tasks";
/// The family of `task_wait` and `task_fan_out`.
pub const HELPERS: &str = "helpers";

/// The families, with the line that says what each holds. An MCP server's
/// tools are the family `mcp:<server>`, outside this table.
pub const FAMILIES: &[(&str, &str)] = &[
    (
        "files",
        "read, search, edit and write files; git history and diffs; read the files people gave you",
    ),
    (
        "commands",
        "run programs and shell commands; read, wait for and stop their background jobs",
    ),
    ("web", "search the web and fetch web pages"),
    (
        "memory",
        "look things up in your memory of past conversations",
    ),
    (
        TASKS,
        "tasks and sub-sessions, and the task graph; background work with a budget is task_create",
    ),
    // `task_wait` and `task_fan_out` (theseus-z9mze.2, .3) are a family of their
    // own, which no place loads and a request to start a task does not: the
    // tasks family loads whole on that cue, and these tools add to the owner's
    // locked DM task flow's request (and the shared channel's, which is at its
    // budget). A search finds them, and a later row (theseus-z9mze.4) gives them a cue.
    (
        HELPERS,
        "wait in this turn for the tasks you started and read their reports; start helpers that each read one area at once",
    ),
    (
        "reminders",
        "wakes and reminders: set one, move it, cancel it",
    ),
    (
        "routines",
        "routines that run on a schedule, and watches that tell you when something changes",
    ),
    ("threads", "read another thread of the conversation"),
    (
        "terminals",
        "interactive programs on a terminal: open, send keys, read the screen, close",
    ),
    (
        "code",
        "language servers: definitions, references, diagnostics, renames",
    ),
    ("channels", "post to and read other places and channels"),
    // The AWS tools are five families, by use (theseus-3msy0): the whole set is 6,869
    // tokens, more than any place can add, and a message that names one tool needs only
    // that tool's family (each is at most 1,300 tokens; `aws::tests` holds the cap).
    (
        "aws",
        "AWS: any API call, the API catalog, and who the account is",
    ),
    ("aws-s3", "AWS S3: list, read and write buckets and objects"),
    (
        "aws-logs",
        "AWS CloudWatch Logs: Insights queries and recent log lines",
    ),
    (
        "aws-review",
        "AWS account review: what Theseus made, what it cost, and CloudTrail's event history",
    ),
    (
        "aws-stacks",
        "AWS CloudFormation stacks: plan, apply, status and delete",
    ),
    (
        "aws-hands",
        "AWS hands: run a command as hands on Lambda or Fargate",
    ),
    // Google Workspace (the Google design, §3): every tool is deferred, in three
    // families by use.
    ("mail", "Gmail: search, read, draft, send and label mail"),
    ("calendar", "Google Calendar: read events, create, change and cancel them"),
    (
        "docs",
        "Google Drive, Docs, Sheets and Slides: find, read, write and share files; any Google API call",
    ),
    (
        "ask",
        "ask the owner a question with options, and wait for the answer",
    ),
    (
        "projects",
        "long-term projects: attach this conversation to one",
    ),
    (
        "plan",
        "plan mode: put a plan to the owner for approval, and mark its steps as you do them",
    ),
    ("extensions", "propose a new extension of your own tools"),
    ("self", "propose and inspect changes to Theseus itself"),
];

/// What `mcp:<server>` holds.
pub fn mcp_summary(server: &str) -> String {
    format!("the tools of the {server} MCP server")
}

/// The family of a tool, by its wire name.
pub fn of_wire(wire: &str) -> String {
    if let Some(rest) = wire.strip_prefix("mcp__") {
        let server = rest.split("__").next().unwrap_or(rest);
        return format!("mcp:{server}");
    }
    // A loaded flow is a family of its own (theseus-pw1q.21.4).
    if let Some(name) = wire.strip_prefix(crate::flow::tool::WIRE_PREFIX) {
        return format!("flow:{name}");
    }
    let family = match wire {
        "tools_find" => TOOLS,
        "file_read" => "files",
        "memory_lookup" => "memory",
        "proc_run" => "commands",
        "owner_ask" => "ask",
        "task_wait" | "task_fan_out" => HELPERS,
        _ => match wire.split('_').next().unwrap_or("") {
            "fs" | "git" | "text" => "files",
            "job" => "commands",
            "web" | "http" => "web",
            "task" => "tasks",
            "wake" => "reminders",
            "routine" | "watch" => "routines",
            "thread" => "threads",
            "project" => "projects",
            "plan" => "plan",
            "term" => "terminals",
            "lsp" => "code",
            "channel" => "channels",
            "aws" => aws_family(wire),
            "gmail" => "mail",
            "calendar" => "calendar",
            "drive" | "sheets" | "slides" | "google" => "docs",
            "extend" => "extensions",
            "self" => "self",
            _ => OTHER,
        },
    };
    family.to_string()
}

/// The family of an AWS tool, by use: the generic call, the catalog and the identity are `aws`.
fn aws_family(wire: &str) -> &'static str {
    match wire {
        w if w.starts_with("aws_s3_") => "aws-s3",
        w if w.starts_with("aws_logs_") => "aws-logs",
        "aws_inventory" | "aws_cost" | "aws_trail" => "aws-review",
        w if w.starts_with("aws_stack_") => "aws-stacks",
        w if w.starts_with("aws_hands_") => "aws-hands",
        _ => "aws",
    }
}

/// The summary line of a family: the table's, an MCP server's, or none.
pub fn summary(family: &str) -> Option<String> {
    if let Some(server) = family.strip_prefix("mcp:") {
        return Some(mcp_summary(server));
    }
    if let Some(name) = family.strip_prefix("flow:") {
        return Some(format!(
            "the frozen flow {name}: a fixed sequence of steps over your own tools"
        ));
    }
    FAMILIES
        .iter()
        .find(|(n, _)| *n == family)
        .map(|(_, s)| (*s).to_string())
}

/// Whether a family may be hidden at all: `other` and `tools` never are.
pub fn hideable(family: &str) -> bool {
    family != OTHER && family != TOOLS
}

/// The kinds of place a session speaks in, which the loaded set is kept for.
/// Which one a turn is in is a fact of the daemon and the session, never of
/// who runs it: a headless daemon (`--stdio`) is `Headless` whatever it is
/// asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// The CLI, the terminal UI and the web UI.
    Cli,
    /// A direct message.
    Dm,
    /// A channel of a group place.
    Channel,
    /// A task's session.
    Task,
    /// A daemon with one client and no place of its own (`--stdio`).
    Headless,
}

impl PlaceKind {
    pub const ALL: [PlaceKind; 5] = [
        PlaceKind::Cli,
        PlaceKind::Dm,
        PlaceKind::Channel,
        PlaceKind::Task,
        PlaceKind::Headless,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PlaceKind::Cli => "cli",
            PlaceKind::Dm => "dm",
            PlaceKind::Channel => "channel",
            PlaceKind::Task => "task",
            PlaceKind::Headless => "headless",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The kind of a turn: its session's kind, where its words go (the
    /// outbox's target, `discord:dm:…`), and whether the daemon is headless.
    pub fn of(task: bool, target: Option<&str>, headless: bool) -> Self {
        match (task, target) {
            (true, _) => PlaceKind::Task,
            (false, Some(t)) if t.contains(":dm:") => PlaceKind::Dm,
            (false, Some(_)) => PlaceKind::Channel,
            (false, None) if headless => PlaceKind::Headless,
            (false, None) => PlaceKind::Cli,
        }
    }

    /// The most tokens of tool definitions a request in this kind of place
    /// may carry when a message's words load more families for a model
    /// without tool search (theseus-jpjcd): the owner's first-request budget
    /// of the kind (`first_request`), never a second set of numbers.
    pub fn tools_budget_tokens(self) -> u64 {
        use crate::first_request as fr;
        match self {
            PlaceKind::Cli | PlaceKind::Task => fr::CLI_TASK_TOKENS,
            PlaceKind::Dm => fr::DM_TOKENS,
            PlaceKind::Channel => fr::CHANNEL_TOKENS,
            PlaceKind::Headless => fr::HEADLESS_TOKENS,
        }
    }

    /// The families this kind of place loads before the ledger says
    /// anything, beyond `[tools.defer] always_loaded` (committed: the
    /// role's needs, never a harness's or a benchmark's). The `aws` families, the
    /// extensions, `self` and the MCP servers are deferred everywhere until a
    /// place's own use loads them.
    pub fn defaults(self) -> &'static [&'static str] {
        match self {
            // A coding session at a terminal: memory, with the files and
            // commands every place loads. The tasks are the largest family
            // (about 6,000 tokens of 15,500), and a coding session's use of
            // them is what the ledger learns. The language servers (`code`,
            // seven tools, about 1,860 tokens) are deferred in every place
            // (theseus-ykc7o, the owner's "Go for it!" of 2026-10-15): no
            // `lsp_*` call in 30 days across every session (install #30's
            // census), and with them the owner's cli read 9,510 tokens of
            // its 8,000; a search loads them, and the ledger's use puts them
            // back where a place calls them.
            PlaceKind::Cli => &["memory"],
            // A person's message: memory, reminders, the web. Posting
            // to and reading other places is deferred (theseus-to0xw): a
            // direct message answers in itself, and the family is 855 tokens
            // of the DM's 15,000 budget; a search loads it, and the ledger's
            // use puts it back where a DM calls it in 1 of 10 turns. Routines,
            // watches and threads are deferred the same way (2026-10-15, the
            // owner's "Go for it!"): `routine_create`, `watch` and
            // `thread_read` had no call in 30 days across every session
            // (install #30's census), 1,167 tokens of the DM's request.
            //
            // The tasks themselves are deferred there (theseus-rwpbg.3, the owner's rule of
            // 2026-10-14): `task_create`, `task_update`, `task_close`, `task_claim` and
            // `task_split` had no call in 30 days, and with them the DM's request read 13,290
            // tokens; `wake_at` had one, so the reminders stay. A shared channel keeps both.
            PlaceKind::Dm => &["memory", "reminders", "web"],
            PlaceKind::Channel => &["memory", "tasks", "reminders", "web", "channels"],
            // A task's session works: the files and commands. Its wakes and
            // sub-tasks, and the language servers, load once its place kind
            // has used them.
            PlaceKind::Task => &[],
            // One client and one run: the work itself, with terminals for
            // programs that want a screen.
            PlaceKind::Headless => &["terminals"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_fall_into_their_families() {
        for (wire, want) in [
            ("fs_read", "files"),
            ("git_diff", "files"),
            ("text_diff", "files"),
            ("file_read", "files"),
            ("proc_run", "commands"),
            ("plan_present", "plan"),
            ("plan_step", "plan"),
            ("job_wait", "commands"),
            ("http_fetch", "web"),
            ("web_search", "web"),
            ("memory_lookup", "memory"),
            ("task_create", "tasks"),
            ("task_wait", "helpers"),
            ("task_fan_out", "helpers"),
            ("wake_at", "reminders"),
            ("wake_move", "reminders"),
            ("wake_cancel", "reminders"),
            ("routine_create", "routines"),
            ("watch", "routines"),
            ("project_attach", "projects"),
            ("thread_read", "threads"),
            ("term_open", "terminals"),
            ("lsp_definition", "code"),
            ("channel_post", "channels"),
            ("aws_call", "aws"),
            ("aws_describe", "aws"),
            ("aws_whoami", "aws"),
            ("aws_s3_list", "aws-s3"),
            ("aws_s3_put", "aws-s3"),
            ("aws_logs_tail", "aws-logs"),
            ("aws_cost", "aws-review"),
            ("aws_trail", "aws-review"),
            ("aws_inventory", "aws-review"),
            ("aws_stack_plan", "aws-stacks"),
            ("aws_stack_delete", "aws-stacks"),
            ("aws_hands_run", "aws-hands"),
            ("extend_propose", "extensions"),
            ("self_propose_change", "self"),
            ("mcp__docs__search", "mcp:docs"),
            ("tools_find", TOOLS),
            ("something_new", OTHER),
        ] {
            assert_eq!(of_wire(wire), want, "{wire}");
        }
    }

    #[test]
    fn every_table_family_has_a_summary_and_other_has_none() {
        for (n, _) in FAMILIES {
            assert!(summary(n).is_some());
        }
        assert!(summary(OTHER).is_none());
        assert_eq!(summary("mcp:docs").unwrap(), mcp_summary("docs"));
    }

    #[test]
    fn a_places_kind_is_read_from_its_target_and_the_daemon() {
        assert_eq!(PlaceKind::of(false, None, false), PlaceKind::Cli);
        assert_eq!(PlaceKind::of(false, None, true), PlaceKind::Headless);
        assert_eq!(
            PlaceKind::of(false, Some("discord:dm:1"), false),
            PlaceKind::Dm
        );
        assert_eq!(
            PlaceKind::of(false, Some("discord:channel:2"), true),
            PlaceKind::Channel
        );
        assert_eq!(
            PlaceKind::of(true, Some("discord:dm:1"), true),
            PlaceKind::Task
        );
        for k in PlaceKind::ALL {
            assert_eq!(PlaceKind::parse(k.as_str()), Some(k));
        }
    }

    /// The helpers family (`task_wait`, `task_fan_out`) is in no place's defaults, so it is deferred
    /// everywhere until something admits it (theseus-z9mze.4); the other families' defaults are as
    /// they were.
    #[test]
    fn the_helpers_family_is_deferred_by_default_in_every_place() {
        for k in PlaceKind::ALL {
            assert!(!k.defaults().contains(&HELPERS), "{}", k.as_str());
        }
        let was: [(PlaceKind, &[&str]); 5] = [
            (PlaceKind::Cli, &["memory"]),
            (PlaceKind::Dm, &["memory", "reminders", "web"]),
            (
                PlaceKind::Channel,
                &["memory", "tasks", "reminders", "web", "channels"],
            ),
            (PlaceKind::Task, &[]),
            (PlaceKind::Headless, &["terminals"]),
        ];
        for (k, d) in was {
            assert_eq!(k.defaults(), d, "{}", k.as_str());
        }
    }

    /// The committed defaults name families that exist, and no harness or
    /// benchmark (the task's rule: a headless run has the defaults of its
    /// place kind, nothing keyed on what is running it).
    #[test]
    fn the_defaults_name_real_families_and_no_benchmark() {
        for k in PlaceKind::ALL {
            for f in k.defaults() {
                assert!(summary(f).is_some(), "{} defaults name {f}", k.as_str());
            }
        }
        // Every file of the feature, but a test module's own words.
        let files = [
            include_str!("family.rs"),
            include_str!("select.rs"),
            include_str!("board.rs"),
            include_str!("find.rs"),
            include_str!("list.rs"),
            include_str!("../tools_defer.rs"),
            include_str!("../turn/defer_step.rs"),
            include_str!("../config/defer.rs"),
        ];
        // The template's own section.
        let template = include_str!("../../config/theseus.example.toml");
        let section = template
            .split("[tools.defer]")
            .nth(1)
            .and_then(|r| r.split("\n[").next())
            .unwrap();
        let banned = [
            "terminal-bench",
            "terminal_bench",
            "harbor",
            "swe-bench",
            "tbench",
        ];
        let bodies = files
            .iter()
            .map(|src| src.split("#[cfg(test)]").next().unwrap())
            .chain([section]);
        for body in bodies {
            let body = body.to_lowercase();
            for w in banned {
                assert!(!body.contains(w), "a file of the feature names {w}");
            }
        }
    }
}
