//! Which settings are sensitive, and which take effect at once (issue 22).
//!
//! The one place that says it. A change to a sensitive setting is the kind
//! that governs what Theseus may do, so it is never made without a card
//! (`config_change`), and some need the owner's code as well ([`proof_reason`],
//! issue 57): what needs approval, where each secret is read
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
pub const SENSITIVE_TABLES: [(&str, &str); 17] = [
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
    (
        "self_fix",
        "self-improvement: whether a fix is made in place, and how large it may be",
    ),
    ("aws", "AWS accounts, roles and budgets"),
    ("google", "Google access: credentials and trusted senders"),
    ("broker", "the secrets broker"),
    ("server", "where Theseus listens and keeps its state"),
    ("content_based_security", "how outside text is handled"),
    ("judge", "the judge that checks risky acts, and its spend"),
    ("privacy", "which models and paths may see private text"),
    (
        "owner_proof",
        "the second proof itself: how long a code covers",
    ),
    (
        "vault",
        "where a secret's value may be typed: a form in a chat passes it through the chat service",
    ),
];

/// A top-level table that is ordinary, apart from its keys that
/// [`key_reason`] names. The config's own tables, not a list of what is safe
/// to guess: each was read for what a change to it could do.
pub const ORDINARY_TABLES: [&str; 32] = [
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
    // The memory feature's switch between its modes: no folder, secret or spend.
    "memory.mode",
];

/// Why a key that holds an address is sensitive (`key_reason`).
pub const ADDRESS_REASON: &str = "an address Theseus listens on or sends to";
/// Why a key that names where a secret is read from is sensitive (`key_reason`).
pub const SECRET_REASON: &str = "where a secret is read from";
/// Why a key that names the owner is sensitive (`key_reason`).
pub const OWNER_REASON: &str = "who the owner is";
/// Why a spending ceiling or budget is sensitive (`key_reason`).
pub const SPEND_REASON: &str = "a spending ceiling or budget";
/// Why a key of the sandbox's kind is sensitive (`key_reason`).
pub const SANDBOX_REASON: &str = "the sandbox, what may run, and the hosts it may reach";

/// Whether a key (lower case) is a spending ceiling or budget.
fn spend_key(k: &str) -> bool {
    ["usd", "budget", "ceiling", "spend", "runaway"]
        .iter()
        .any(|s| k.contains(s))
}

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
        return Some(SECRET_REASON);
    }
    if has("owner") {
        return Some(OWNER_REASON);
    }
    if is("system")
        || has("prompt")
        || is("card")
        || is("end_note")
        || has("persona")
        || has("instruction")
    {
        return Some(PROMPT_REASON);
    }
    if has("agent") || is("provider") || ends("_provider") || has("not_") {
        return Some("which processes count as agents, which provider sees the owner's text, or what Theseus is told never to keep");
    }
    if spend_key(&k) {
        return Some(SPEND_REASON);
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
        return Some(FOLDER_REASON);
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
        return Some(SANDBOX_REASON);
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
        return Some(ADDRESS_REASON);
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

/// Whether a change of `path` to `after` needs a code typed for it alone,
/// whatever window is open (`needs_fresh_proof`): a setting that decides who
/// the owner is or what the second proof is, or that widens what Theseus may
/// do to the machine without asking again.
///
/// - who the owner is: `[places] owner` and the bindings file's path,
///   `[discord] bindings_file` (the file names the person of each DM), and
///   `[server] state_dir` (the bindings file and the proof's own file are read
///   from it, so moving it can change both);
/// - the second proof itself: any key of `[owner_proof]`;
/// - `[self] install` becoming `"auto"`: it installs every joined change on a
///   root-owned layout with no card. The older `true` is not it (it never
///   installs into a root-owned directory by itself), and a removal reads as
///   the default, `"ask"`, which is not widening.
///
/// Each of the first two is fresh whatever it becomes, a removal included.
pub fn needs_fresh_proof(path: &[String], after: Option<&serde_json::Value>) -> bool {
    let at = |p: &[&str]| path.len() >= p.len() && path.iter().zip(p).all(|(a, b)| a == b);
    at(&["places", "owner"])
        || at(&["server", "state_dir"])
        || at(&["discord", "bindings_file"])
        || at(&["owner_proof"])
        || (at(&["self", "install"])
            && path.len() == 2
            && after.is_some_and(|v| v.as_str() == Some("auto")))
}

/// Why a key holds text put into every prompt (`key_reason`).
pub const PROMPT_REASON: &str = "text that is put into every prompt, or the persona that speaks";
/// Why a key names a folder or a file Theseus may touch (`key_reason`).
pub const FOLDER_REASON: &str = "a folder or file Theseus may touch";
/// Why a catalog price is kept for the code ([`proof_reason`]).
pub const PRICE_REASON: &str = "a price that decides what a model call is charged or reserved";

// The second reason table (issue 57): which sensitive settings' cards ALSO take the owner's
// authenticator code. Every sensitive setting keeps its card; these lists say only where the
// code is kept or dropped. Each group is one list, so a later decision flips one group. A
// setting in no list keeps what main does for it ([`reason`]: sensitive keeps the code), so no
// unlisted key is weaker than main.

/// (K) The code is KEPT, as the owner's yes named: the approval rules, where secrets come from,
/// the sandbox, places, the broker, where Theseus listens and keeps its state, its own MCP
/// server, and the second proof itself.
pub const KEPT_TABLES: [&str; 8] = [
    "policy",
    "secrets",
    "sandbox",
    "places",
    "broker",
    "server",
    "mcp_server",
    "owner_proof",
];

/// (K) The code is KEPT, in any table, for a key of these kinds: where a secret is read from,
/// who the owner is, and the sandbox's kind (what may run and the hosts it may reach).
pub const KEPT_KEYS: [&str; 3] = [SECRET_REASON, OWNER_REASON, SANDBOX_REASON];

/// (K) The catalog's keys that price a call (with every `*_per_mtok`): the code is KEPT in
/// either direction, since a lower price lets more calls through the same dollar limits.
pub const PRICE_KEYS: [&str; 3] = ["long_prompt", "off_peak", "bytes_per_token"];

/// A group the owner decided by his word (2026-11-25, "I'll take your recommendation"): each
/// keeps the code while it is in [`KEEPS`], and takes the card's yes alone when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// (G1) KEPT by the owner's word: every key whose value is an address that conversations,
    /// messages or a credential are sent to ([`ADDRESS_REASON`]), and which provider a model
    /// uses (`provider`, `*_provider`).
    Addresses,
    /// (G2) KEPT by the owner's word: the tables that decide or guard later gates
    /// ([`GATE_TABLES`]).
    Gates,
    /// (G3) DROPPED by the owner's word: self-improvement, the AWS and Google accounts, the
    /// vault ([`ACCOUNT_TABLES`]), the folders and files Theseus may touch ([`FOLDER_REASON`]),
    /// and which processes count as agents. The global exceptions still win inside it: a key
    /// that reads a secret, who the owner is, a sandbox-like key, any spending key, and
    /// `self.install = "auto"`.
    SelfAndAccounts,
}

/// The groups whose code is KEPT, by the owner's word: G1 and G2. A group not in this list
/// (G3) takes the card's yes alone; each is one entry to flip.
pub const KEEPS: [Group; 2] = [Group::Addresses, Group::Gates];

/// (G2) The tables that decide or guard later gates: KEPT by the owner's word.
pub const GATE_TABLES: [&str; 3] = ["judge", "privacy", "content_based_security"];

/// (G3) Self-improvement, the AWS and Google accounts, and the vault: DROPPED by the owner's
/// word.
pub const ACCOUNT_TABLES: [&str; 6] = ["self", "self_issues", "self_fix", "aws", "google", "vault"];

/// (D) The code is DROPPED (the card's yes is enough) for prompt text ([`PROMPT_REASON`]),
/// as the owner's yes named it,
/// including a long prompt value. Prompt files, removing an MCP server and attaching one with
/// no secret, env, address or egress take their own cards with no code
/// (`config_change::prompt_file`, `config_change::mcp_attach::needs`); so does tightening one
/// of [`TIGHTENS`].
pub const DROPPED_KEYS: [&str; 1] = [PROMPT_REASON];

/// (D) Whether `path` is prompt TEXT: a key of [`DROPPED_KEYS`]'s kind that holds the text
/// itself (`system`, `end_note`, a persona's `card`, an instruction), not one that chooses a
/// persona or sizes one (`default_persona`, `persona_max_tokens`), which keep what main does.
pub fn prompt_text(path: &[String]) -> bool {
    let persona = path
        .last()
        .is_some_and(|k| k.to_ascii_lowercase().contains("persona"));
    !persona && reason(path).is_some_and(|r| DROPPED_KEYS.contains(&r.as_str()))
}

/// The group a setting is in, when it is in one ([`Group`]).
pub fn group(path: &[String]) -> Option<Group> {
    let first = path.first()?.as_str();
    let keys = || path.iter().skip(1);
    let provider = |k: &String| {
        let k = k.to_ascii_lowercase();
        k == "provider" || k.ends_with("_provider")
    };
    if keys().any(|k| key_reason(k) == Some(ADDRESS_REASON) || provider(k)) {
        return Some(Group::Addresses);
    }
    if GATE_TABLES.contains(&first) {
        return Some(Group::Gates);
    }
    let agent = |k: &String| k.to_ascii_lowercase().contains("agent");
    if ACCOUNT_TABLES.contains(&first)
        || keys().any(|k| key_reason(k) == Some(FOLDER_REASON) || agent(k))
    {
        return Some(Group::SelfAndAccounts);
    }
    None
}

/// Whether `path` prices a model call (`catalog.<model>.<a price>`), or names a catalog row
/// whole.
fn price(path: &[String]) -> bool {
    path.first().is_some_and(|t| t == "catalog")
        && (path.len() < 3
            || path
                .iter()
                .skip(2)
                .any(|k| k.ends_with("_per_mtok") || PRICE_KEYS.contains(&k.as_str())))
}

/// Why a change of `path` from `before` to `after` needs the owner's authenticator code on
/// top of the card's yes (issue 57), or `None` when the card alone is enough. In order, the
/// strictest first: the fresh proof ([`needs_fresh_proof`]); (K) [`KEPT_TABLES`], a price,
/// [`KEPT_KEYS`], a spending limit not made tighter ([`tightened`]); a group, by [`KEEPS`];
/// (D) [`DROPPED_KEYS`]; and else what main does ([`reason`]).
pub fn proof_reason(
    path: &[String],
    before: Option<&serde_json::Value>,
    after: Option<&serde_json::Value>,
) -> Option<String> {
    if needs_fresh_proof(path, after) {
        return Some("who the owner is, or the second proof itself".into());
    }
    let first = path.first()?.as_str();
    if KEPT_TABLES.contains(&first) {
        return reason(path).or_else(|| Some(format!("[{first}]")));
    }
    if price(path) {
        return Some(PRICE_REASON.into());
    }
    let keys = || path.iter().skip(1);
    if let Some(why) = keys().find_map(|k| key_reason(k).filter(|w| KEPT_KEYS.contains(w))) {
        return Some(why.into());
    }
    if keys().any(|k| spend_key(&k.to_ascii_lowercase())) {
        return match tightened(path, before, after) {
            true => None,
            false => Some(format!("{SPEND_REASON}, not made tighter")),
        };
    }
    // A group keeps the code while it is in KEEPS, and takes the card alone when it is not.
    if let Some(g) = group(path) {
        return match KEEPS.contains(&g) {
            true => reason(path).or_else(|| Some(format!("{g:?}"))),
            false => None,
        };
    }
    match prompt_text(path) {
        true => None,
        false => reason(path),
    }
}

/// What zero means for a spending limit whose direction is known ([`TIGHTENS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Dollars, which the kernel holds as micro-dollars (`theseus_kernel::usd_to_micros`,
    /// rounded to the nearest); zero micro-dollars is no limit at all.
    Dollars,
    /// A whole multiple of a session's limit; zero is no limit at all.
    Multiple,
}

/// The spending limits whose direction is known, by what the kernel holds for each
/// (`KernelSection::to_kernel_config`): for these, and only these, a change that makes the
/// limit strictly tighter in the kernel's own units needs the card alone. Every other spending
/// key needs the code in either direction until its meaning is written here.
pub const TIGHTENS: [(&str, &str, Unit); 5] = [
    ("kernel", "spend_limit_usd", Unit::Dollars),
    ("kernel", "daily_spend_ceiling_usd", Unit::Dollars),
    ("kernel", "daily_runaway_usd", Unit::Dollars),
    ("kernel", "runaway_usd_per_10min", Unit::Dollars),
    ("kernel", "spend_limit_hard_multiple", Unit::Multiple),
];

/// A limit's value as the kernel will use it, with no limit as `u64::MAX`; `None` when it is
/// not a value of its unit.
pub fn effective(unit: Unit, v: &serde_json::Value) -> Option<u64> {
    let held = match unit {
        Unit::Dollars => theseus_kernel::usd_to_micros(v.as_f64()?),
        Unit::Multiple => v.as_u64()?,
    };
    Some(match held {
        0 => u64::MAX,
        n => n,
    })
}

/// Whether a change makes a spending limit of [`TIGHTENS`] strictly tighter in the kernel's
/// own units: both sides values of its unit, and the new limit strictly lower, where zero (or
/// a value that converts to zero micro-dollars) is no limit at all. Any other key, a removal,
/// a value set where there was none: no.
fn tightened(
    path: &[String],
    before: Option<&serde_json::Value>,
    after: Option<&serde_json::Value>,
) -> bool {
    let [table, key] = path else {
        return false;
    };
    let known = TIGHTENS
        .iter()
        .find(|(t, k, _)| *t == table.as_str() && *k == key.as_str());
    let (Some((_, _, unit)), Some(b), Some(a)) = (known, before, after) else {
        return false;
    };
    match (effective(*unit, b), effective(*unit, a)) {
        (Some(b), Some(a)) => a < b,
        _ => false,
    }
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
            "discord.bindings_file",
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
        for s in ["import.notes.enabled", "tui.theme", "memory.enabled"] {
            assert_eq!(reason(&p(s)), None, "{s} must be ordinary");
        }
    }

    #[test]
    fn a_table_nobody_classified_is_sensitive() {
        let why = reason(&p("brand_new.thing")).expect("unclassified is sensitive");
        assert!(why.contains("not been classified"), "{why}");
    }

    #[test]
    fn who_the_owner_is_and_the_proof_itself_are_fresh_whatever_they_become() {
        let v = |s: &str| Some(serde_json::json!(s));
        for path in [
            "places.owner",
            "places.owner.discord",
            "discord.bindings_file",
            "server.state_dir",
            "owner_proof.window_hours",
            "owner_proof",
        ] {
            for after in [v("x"), None] {
                assert!(
                    needs_fresh_proof(&p(path), after.as_ref()),
                    "{path} {after:?}"
                );
            }
            assert!(is_sensitive(&p(path)), "{path} is also sensitive");
        }
        // Their neighbours are sensitive, not fresh.
        for path in ["places.public", "server.port", "mcp_server.port"] {
            assert!(!needs_fresh_proof(&p(path), v("x").as_ref()), "{path}");
        }
    }

    #[test]
    fn only_turning_install_to_auto_needs_a_fresh_proof() {
        let v = |s: &str| Some(serde_json::json!(s));
        let install = p("self.install");
        assert!(needs_fresh_proof(&install, v("auto").as_ref()));
        for after in [
            v("ask"),
            v("off"),
            Some(serde_json::json!(true)),
            Some(serde_json::json!(false)),
            None,
        ] {
            assert!(!needs_fresh_proof(&install, after.as_ref()), "{after:?}");
        }
        assert!(!needs_fresh_proof(&p("self.mode"), v("auto").as_ref()));
        assert!(!needs_fresh_proof(&p("policy.install"), v("auto").as_ref()));
        // Still sensitive on its own: the fresh proof is on top of the second one.
        assert!(is_sensitive(&install));
    }

    #[test]
    fn nothing_is_live_so_every_change_needs_a_restart() {
        assert!(needs_restart(&p("import.notes.enabled")));
        assert!(needs_restart(&p("self.budget.usd")));
    }
}
