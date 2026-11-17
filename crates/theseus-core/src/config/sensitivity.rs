//! Which settings are sensitive, and which take effect at once (issue 22).
//!
//! The one place that says it. A change to a sensitive setting is the kind
//! that governs what Theseus may do, so it is never made on a card's yes
//! alone (`config_change`): what needs approval, where each secret is read
//! from, the sandbox and the hosts it allows, the folders Theseus may touch,
//! who the owner is and which places are private, every spending ceiling and
//! budget, Theseus's own MCP server, self-improvement's mode and sealed sets,
//! and where the config and the state live.
//!
//! A path is a list of keys: `["self", "budget", "usd"]`. Three lists decide:
//!
//! - [`SENSITIVE_TABLES`]: a whole top-level table, whatever is in it;
//! - [`ORDINARY_TABLES`]: a top-level table that is ordinary unless one of its
//!   keys is sensitive by [`key_reason`] (a folder, a secret, an address, an
//!   owner, a spend);
//! - neither: **unclassified**, and treated as sensitive, so a table added to
//!   the config without a decision is refused until someone makes one. The
//!   test `every_table_and_every_risky_key_is_classified` walks the config
//!   type and fails on it.
//!
//! A change is judged by the settings that really change, not by the paths
//! the proposal named (`config_change::diff`): a table written whole, an array
//! index, or a renamed key reaches the same keys, and each is classified.

/// A top-level table every key of which is sensitive, and why.
pub const SENSITIVE_TABLES: [(&str, &str); 14] = [
    ("policy", "what needs approval"),
    ("secrets", "where secrets come from"),
    ("sandbox", "the sandbox and the hosts it allows"),
    ("places", "who the owner is and which places are private"),
    ("mcp_server", "Theseus's own MCP server"),
    ("self", "self-improvement: its mode, budget and sealed sets"),
    (
        "self_issues",
        "self-improvement: where it files and what it spends",
    ),
    ("aws", "AWS accounts, roles and budgets"),
    ("google", "Google access: credentials and trusted senders"),
    ("broker", "the secrets broker"),
    ("server", "where Theseus listens and keeps its state"),
    ("content_based_security", "how outside text is handled"),
    ("judge", "the judge that checks risky acts, and its spend"),
    ("privacy", "which models and paths may see private text"),
];

/// A top-level table that is ordinary, apart from its keys that
/// [`key_reason`] names. The config's own tables, not a list of what is safe
/// to guess: each was read for what a change to it could do.
pub const ORDINARY_TABLES: [&str; 33] = [
    "narrative",
    "model",
    "profiles",
    "context",
    "personas",
    "providers",
    "github",
    "web",
    "tui",
    "telemetry",
    "kernel",
    "catalog",
    "tools",
    "discord",
    "voice",
    "index",
    "memory",
    "routing",
    "mcp",
    "lsp",
    "sessions",
    "routines",
    "route_bench",
    "persona_import",
    "project",
    "projects",
    "standing",
    "threads",
    "initiative",
    "watch_for",
    "cockpit",
    "prefix",
    "notify",
];

/// More top-level tables, ordinary the same way (a second list only so each
/// stays under a line of the array's length that rustfmt keeps readable).
pub const ORDINARY_TABLES_MORE: [&str; 5] = ["cache", "people", "card", "ontology", "import"];

/// Keys that read as risky by name in the template and are not: each is a
/// count, a size, a switch or a model's name, written as the template's path
/// with `*` for a name the owner chooses. `tests_sensitivity` walks the
/// template and fails on a risky-looking key that is neither sensitive nor
/// named here, so an excuse is a decision somebody wrote down.
pub const ORDINARY_KEYS: &[&str] = &[
    // Counts and sizes.
    "discord.task_channel_cap",
    "memory.recall_floor",
    "memory.session_recall_cap_tokens",
    "threads.base_distance",
    // The memory feature's switch between its modes: no folder, secret or spend.
    "memory.mode",
];

/// What the settings' own words say about a key, whatever its table: the
/// reason it is sensitive, or `None`.
pub fn key_reason(key: &str) -> Option<&'static str> {
    let k = key.to_ascii_lowercase();
    let has = |s: &str| k.contains(s);
    let ends = |s: &str| k.ends_with(s);
    let is = |s: &str| k == s;
    if ends("_secret")
        || ends("_secrets")
        || is("secrets")
        || is("env")
        || is("credentials")
        || is("token")
        || is("key")
        || has("access_key")
        || has("refresh_token")
        || has("token_file")
        || has("password")
    {
        return Some("where a secret is read from");
    }
    if has("owner") {
        return Some("who the owner is");
    }
    if is("system")
        || has("prompt")
        || is("card")
        || is("end_note")
        || has("persona")
        || has("instruction")
    {
        return Some("text that is put into every prompt, or the persona that speaks");
    }
    if has("agent") || is("provider") || ends("_provider") || has("not_") {
        return Some("which processes count as agents, which provider sees the owner's text, or what Theseus is told never to keep");
    }
    if has("usd") || has("budget") || has("ceiling") || has("spend") || has("runaway") {
        return Some("a spending ceiling or budget");
    }
    if is("roots")
        || is("projects_dir")
        || is("bindings_file")
        || ends("_dir")
        || ends("_dirs")
        || ends("_path")
        || ends("_paths")
        || ends("_root")
        || ends("_roots")
        || ends("_file")
        || is("read")
        || is("files")
        || is("frozen")
        || is("tree")
        || is("repo")
    {
        return Some("a folder or file Theseus may touch");
    }
    if is("sandbox")
        || is("cwd")
        || is("socket")
        || is("egress")
        || is("command")
        || is("proc_env")
        || is("path_prepend")
        || has("allow")
        || has("argv")
        || is("posture")
        || has("posture_")
        || is("enforcement")
    {
        return Some("the sandbox, what may run, and the hosts it may reach");
    }
    if is("url")
        || ends("_url")
        || is("endpoint")
        || ends("_endpoint")
        || is("api_base")
        || has("proxy")
        || ends("_origin")
        || is("bind")
        || is("origin")
        || is("port")
        || has("headers")
    {
        return Some("an address Theseus listens on or sends to");
    }
    if ends("_channel") || is("channel") || has("forum") || is("task_home") || is("thread") {
        return Some("where Theseus posts and reads");
    }
    if has("trust") || has("guild") || has("private") || has("public") || has("external") {
        return Some(
            "which places and senders are trusted, and whether outside text keeps its label",
        );
    }
    None
}

/// Why a change to `path` is sensitive, or `None` when it is ordinary. A path
/// in a table nobody classified is sensitive, and says so.
pub fn reason(path: &[String]) -> Option<String> {
    let first = path.first()?.as_str();
    if let Some((_, why)) = SENSITIVE_TABLES.iter().find(|(t, _)| *t == first) {
        return Some((*why).to_string());
    }
    let known = ORDINARY_TABLES.contains(&first) || ORDINARY_TABLES_MORE.contains(&first);
    if !known {
        return Some(format!(
            "[{first}] has not been classified as ordinary or sensitive, so it is treated as sensitive"
        ));
    }
    path.iter()
        .skip(1)
        .find_map(|k| key_reason(k).map(str::to_string))
}

/// Whether a change to `path` is sensitive.
pub fn is_sensitive(path: &[String]) -> bool {
    reason(path).is_some()
}

/// The settings that take effect without a restart: where the code already
/// applies one live. None: Theseus reads its config once, at the start, so
/// every approved change restarts it. A setting joins this list only with the
/// code that applies it live, and a test per entry.
pub const LIVE: [&str; 0] = [];

/// Whether a change to `path` needs a restart to take effect.
pub fn needs_restart(path: &[String]) -> bool {
    let dotted = path.join(".");
    !LIVE
        .iter()
        .any(|l| dotted == *l || dotted.starts_with(&format!("{l}.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<String> {
        s.split('.').map(str::to_string).collect()
    }

    #[test]
    fn the_sensitive_things_the_issue_names_are_sensitive() {
        for s in [
            "policy.tools",
            "secrets.model_key",
            "sandbox.egress",
            "mcp.servers.files.sandbox",
            "mcp.servers.files.egress",
            "mcp.servers.files.env",
            "mcp.servers.files.auth_secret",
            "mcp.servers.files.command",
            "tools.roots",
            "tools.projects_dir",
            "places.owner",
            "places.public",
            "server.bindings_file",
            "server.state_dir",
            "mcp_server.port",
            "self.mode",
            "self.sealed",
            "self.budget.usd",
            "kernel.daily_spend_ceiling_usd",
            "kernel.spend_limit_usd",
            "discord.rest_proxy",
            "discord.gateway_proxy",
            "mcp.servers.docs.external",
            "memory.include_external",
            "model.system",
            "profiles.sonnet.system",
            "personas.theseus.card",
            "context.default_persona",
            "tools.cwd",
            "tools.agent_processes",
            "discord.forum_channel",
            "discord.task_home",
            "threads.discord_channel",
            "ontology.not_topics",
            "people.not_people",
            "model.provider",
            "profiles.sonnet.provider",
        ] {
            assert!(is_sensitive(&p(s)), "{s} must be sensitive");
        }
    }

    #[test]
    fn ordinary_settings_are_ordinary() {
        for s in [
            "import.notes.enabled",
            "narrative",
            "tui.theme",
            "memory.enabled",
        ] {
            assert_eq!(reason(&p(s)), None, "{s} must be ordinary");
        }
    }

    #[test]
    fn a_table_nobody_classified_is_sensitive() {
        let why = reason(&p("brand_new.thing")).expect("unclassified is sensitive");
        assert!(why.contains("not been classified"), "{why}");
    }

    #[test]
    fn nothing_is_live_so_every_change_needs_a_restart() {
        assert!(needs_restart(&p("import.notes.enabled")));
        assert!(needs_restart(&p("self.budget.usd")));
    }
}
