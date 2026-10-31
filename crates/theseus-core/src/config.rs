//! Configuration. A TOML document: a local file, or a 1Password item, so a
//! deployment is reconstructible from the vault, named by its `op://`
//! reference in `--config` or `THESEUS_CONFIG`. Secret fields are
//! `op://vault/item/field` references, never values.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::places::PlacesConfig;
use crate::secrets::{OpReader, SecretRef};

mod aws;
mod context;
mod google;
pub(crate) mod google_calendars;
pub use context::ContextConfig;
mod cache;
mod inherit;
pub use cache::{CacheConfig, CacheTtl};
pub mod calendar;
pub mod discord;
mod index;
pub use index::IndexConfig;
pub mod card;
mod cockpit;
pub mod content_based_security;
mod data_floor;
pub(crate) mod import;
mod judge;
mod limits;
mod lookup;
pub(crate) mod lsp;
pub(crate) mod memory;
mod notify;
pub mod ontology;
mod outputs;
pub(crate) mod people;
pub mod persona;
pub mod prefix;
pub mod privacy;
mod projects;
pub(crate) mod route;
mod routines;
pub(crate) mod routing;
mod routing_checkpoints;
pub mod self_improve;
pub mod self_mint;
mod self_sealed;
pub use persona::{Initiative, PersonaConfig, PersonaView, DEFAULT_NAME};
mod initiative;
mod persona_settings;
mod renamed;
mod sessions;
mod standing;
mod threads;
mod wake_limits;
pub use cockpit::CockpitConfig;
pub use projects::ProjectsConfig;
pub use routines::RoutinesConfig;
mod watches;
pub use initiative::InitiativeConfig;
pub use notify::NotifyConfig;
pub use persona_settings::PersonaSettings;
pub use prefix::PrefixConfig;
pub use route::RouteConfig;
pub use sessions::SessionsConfig;
pub use standing::StandingConfig;
pub use threads::{PersonaEngage, ThreadsConfig};
pub use watches::WatchesConfig;
pub mod defer;
mod sparse;
pub mod term;
mod web;
pub use aws::{
    default_hourly_alert_usd, default_runaway_factor, AwsAccountConfig, AwsConfig,
    AwsCredentialNames, HandsNetwork,
};
pub use google::{GoogleConfig, GoogleCredentialNames};
pub use google_calendars::{primary_spec, CalendarEntry, CalendarSpec, CalendarTable};
pub use judge::{JudgeConfig, JudgePackConfig, PackMode, SignalsConfig};
pub use limits::{
    default_fan_in_chars, default_fan_out_max_wait_secs, default_fan_out_wait_secs,
    default_helper_floor_usd, default_helper_max_output_tokens, default_job_wait_beat_secs,
    default_job_wait_max_secs, default_job_wait_status, default_parallel_reads,
    default_parallel_runs, default_parallel_writes, DailyCeilingMode, MaxLoopsMode, SpendLimitMode,
};
pub use web::{WebToolsConfig, WEB_TIMEOUT_MAX_SECS};
// Where the config comes from when nothing names one (theseus-5aqz).
pub use lookup::{find_config, Lookup, DEFAULT_CONFIG, NO_CONFIG, SYSTEM_CONFIG};
pub use lsp::{LspConfig, LspServerConfig};
pub use memory::{MemoryConfig, MemoryMode};
pub use routing::{RoutingConfig, RoutingMode};
pub use sparse::{sparse_note, SPARSE_HEADER};
pub mod mcp;
pub mod mcp_server;
pub mod tui;
pub use mcp::{McpConfig, McpServerConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Narrate every step of the session/turn/loop/model-call structure into
    /// the web UI's The Narrative tab. Absent or false: no line is made and
    /// the tab never shows. First, because a TOML key after a table is that
    /// table's.
    #[serde(default)]
    pub narrative: bool,
    #[serde(default)]
    pub model: ModelConfig,
    /// Named model profiles. The implicit `default` profile is built from `[model]`.
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
    /// `[context]`: the system level of context files, which every session
    /// gets, and the persona in play (theseus-c48).
    #[serde(default, skip_serializing_if = "ContextConfig::is_empty")]
    pub context: ContextConfig,
    /// `[personas.<name>]`: each persona's context files, added after the
    /// system level's while it is in play (theseus-c48).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub personas: BTreeMap<String, PersonaConfig>,
    /// Additional Anthropic-Messages-compatible endpoints by name (e.g. `zai`).
    /// The implicit `anthropic` provider comes from `[model]` unless overridden here.
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    /// name → where its value comes from: an `op://` reference into the vault
    /// (recommended), or `env:NAME` or `file:PATH` from outside it
    /// (theseus-n88g.1). They resolve in the background while the daemon
    /// serves; a consumer waits for its own secret and fails closed if it did
    /// not resolve, and a failed one is fetched again (theseus-qa0).
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    /// `[broker]`: which program gets which secret, in which variable, and
    /// each secret's posture (theseus-dcy). Empty until a grant is added.
    #[serde(default, skip_serializing_if = "BrokerConfig::is_empty")]
    pub broker: BrokerConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub github: GitHubConfig,
    #[serde(default)]
    pub web: WebConfig,
    /// `[tui]`: how the terminal clients ping the owner (theseus-f0bi9).
    #[serde(default)]
    pub tui: tui::TuiConfig,
    #[serde(default)]
    pub telemetry: crate::telemetry::TelemetryConfig,
    #[serde(default)]
    pub kernel: KernelSection,
    /// `[catalog."<model>"]`, changes to the code's model table on purpose: a
    /// table over a built-in model replaces the fields it names, and a
    /// complete one adds a model. The template holds none (theseus-vwar):
    /// every other model keeps the code's figures, which `theseus catalog`
    /// lists, and a table that copies them changes nothing.
    #[serde(default)]
    pub catalog: BTreeMap<String, crate::catalog::CatalogRow>,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    /// `[voice]` (rows 77 and 78): speech in a voice channel, in `crate::voice`.
    #[serde(default, skip_serializing_if = "crate::voice::VoiceConfig::is_default")]
    pub voice: crate::voice::VoiceConfig,
    /// `[aws.accounts.<id>]`: the AWS accounts Theseus owns (AWS design
    /// §3.5). None: no AWS tool, and nothing AWS runs.
    #[serde(default, skip_serializing_if = "AwsConfig::is_empty")]
    pub aws: AwsConfig,
    /// `[google]`: Google Workspace (the Google design, §2.1). No `account`: no
    /// Google tool, and nothing Google runs.
    #[serde(default, skip_serializing_if = "GoogleConfig::is_empty")]
    pub google: GoogleConfig,
    /// `[index]`: the index tender (roadmap row 51).
    #[serde(default)]
    pub index: IndexConfig,
    /// `[memory]`: recall (M6 step 30a), off by default, in `config/memory.rs`.
    #[serde(default, skip_serializing_if = "MemoryConfig::is_default")]
    pub memory: MemoryConfig,
    /// `[routing]`: Jev's model per interaction mode (M5 25e), in `config/routing.rs`.
    #[serde(default, skip_serializing_if = "RoutingConfig::is_default")]
    pub routing: RoutingConfig,
    /// `[privacy]`: the one list of sensitive names (theseus-oejh3), `config/privacy.rs`.
    #[serde(default, skip_serializing_if = "privacy::PrivacyConfig::is_default")]
    pub privacy: privacy::PrivacyConfig,
    /// `[content_based_security]`: what a content flag does, and the switch for models of restricted origin
    /// (theseus-9ymz0, theseus-gyjyo.2), `config/content_based_security.rs`.
    #[serde(
        default,
        skip_serializing_if = "content_based_security::ContentBasedSecurityConfig::is_default"
    )]
    pub content_based_security: content_based_security::ContentBasedSecurityConfig,
    /// `[mcp.servers.<name>]`: the MCP servers whose tools turns are offered
    /// (M7 36b), in `config::mcp`.
    #[serde(default, skip_serializing_if = "McpConfig::is_empty")]
    pub mcp: McpConfig,
    /// `[mcp_server]` (step 41b): Theseus's own MCP server, off by default.
    #[serde(
        default,
        skip_serializing_if = "mcp_server::McpServerConfig::is_default"
    )]
    pub mcp_server: mcp_server::McpServerConfig,
    /// `[lsp]`: the language-server board and its tools (L2), off by default, in `config/lsp.rs`.
    #[serde(default, skip_serializing_if = "LspConfig::is_default")]
    pub lsp: LspConfig,
    /// `[sessions]`: the state windows and the re-title (theseus-emqx).
    #[serde(default, skip_serializing_if = "SessionsConfig::is_default")]
    pub sessions: SessionsConfig,
    /// `[routines]`: recurring work, a run a task (theseus-pvm5c), `config/routines.rs`.
    #[serde(default, skip_serializing_if = "RoutinesConfig::is_default")]
    pub routines: RoutinesConfig,
    /// `[route_bench]`: your own benchmark's sampler (theseus-pw1q.12), `config/route.rs`.
    #[serde(default, skip_serializing_if = "RouteConfig::is_default")]
    pub route_bench: RouteConfig,
    /// `[persona_import]`: the agent import and the recall boost (theseus-a8n7), `config/persona_settings.rs`.
    #[serde(default, skip_serializing_if = "PersonaSettings::is_default")]
    pub persona_import: PersonaSettings,
    /// `[projects]`: long-term work (theseus-n5ui2), `config/projects.rs`.
    #[serde(default, skip_serializing_if = "ProjectsConfig::is_default")]
    pub projects: ProjectsConfig,
    /// `[standing]`: what the owner's words become, `config/standing.rs` (theseus-slvl.5).
    #[serde(default, skip_serializing_if = "StandingConfig::is_default")]
    pub standing: StandingConfig,
    /// `[threads]`: threads on virtual places (theseus-nhew), `config/threads.rs`.
    #[serde(default, skip_serializing_if = "ThreadsConfig::is_default")]
    pub threads: ThreadsConfig,
    /// `[initiative]`: what Theseus says first, in its persona's voice (theseus-slvl.9), `config/initiative.rs`.
    #[serde(default, skip_serializing_if = "InitiativeConfig::is_default")]
    pub initiative: InitiativeConfig,
    /// `[watch_for]`: checks with no model until a condition holds (theseus-f2r2b), `config/watches.rs`.
    #[serde(default, skip_serializing_if = "WatchesConfig::is_default")]
    pub watch_for: WatchesConfig,
    /// `[cockpit]`: the link a reply's footer carries (theseus-x9swu), `config/cockpit.rs`.
    #[serde(default, skip_serializing_if = "CockpitConfig::is_default")]
    pub cockpit: CockpitConfig,
    /// `[prefix]`: the prefix library (theseus-rr15), `config/prefix.rs`.
    #[serde(default, skip_serializing_if = "PrefixConfig::is_default")]
    pub prefix: PrefixConfig,
    /// `[notify]`: when an open question pings the owner's DM (theseus-j7cje), `config/notify.rs`.
    #[serde(default, skip_serializing_if = "NotifyConfig::is_default")]
    pub notify: NotifyConfig,
    /// `[cache]`: the keep-warm read and the cold rewrite's stubs (theseus-ezeg).
    #[serde(default, skip_serializing_if = "CacheConfig::is_default")]
    pub cache: CacheConfig,
    /// `[people]`: people proposed from text (theseus-wy7y), `config/people.rs`.
    #[serde(default, skip_serializing_if = "people::PeopleConfig::is_default")]
    pub people: people::PeopleConfig,
    /// `[card]`: the owner card proposed from the imported history
    /// (theseus-slvl.6), `config/card.rs`.
    #[serde(default, skip_serializing_if = "card::CardConfig::is_default")]
    pub card: card::CardConfig,
    /// `[ontology]`: Jev files each message's session into the ontology
    /// (theseus-xzxqv), `config/ontology.rs`.
    #[serde(default, skip_serializing_if = "ontology::OntologyConfig::is_default")]
    pub ontology: ontology::OntologyConfig,
    /// `[import]`: the nightly OpenClaw sync (theseus-pjmj7), off until `[import.openclaw] state_dir` is set,
    /// in `config/import.rs`.
    #[serde(default, skip_serializing_if = "import::ImportConfig::is_default")]
    pub import: import::ImportConfig,
    /// `[self]`: self-improvement, off by default (theseus-pw1q), in `config/self_improve.rs`.
    #[serde(default, rename = "self")]
    #[serde(skip_serializing_if = "self_improve::SelfImproveConfig::is_default")]
    pub self_improve: self_improve::SelfImproveConfig,
    /// `[sandbox]`: L1 for `proc.run` (M4 17b), in `crate::sandbox`.
    #[serde(default)]
    pub sandbox: crate::sandbox::SandboxConfig,
    /// `[judge]`: Jev's judgments, in shadow (M5 23a), in `config/judge.rs`.
    #[serde(default)]
    pub judge: JudgeConfig,
    /// `[places]` (the place rule).
    #[serde(default)]
    #[serde(skip_serializing_if = "PlacesConfig::is_empty")]
    pub places: PlacesConfig,
    /// The 1Password token file this daemon was pointed at (`--op-token-file` or
    /// `THESEUS_OP_TOKEN_FILE`): set at startup, never from the TOML; the floor keeps it (theseus-8az).
    #[serde(skip)]
    pub op_token_file: Option<PathBuf>,
    /// The last-known-good copy of the vault's config note (theseus-2fo): set at startup when the
    /// config is `op://`, never read from the TOML. The floor keeps it, as it keeps the token file.
    #[serde(skip)]
    pub config_copy: Option<PathBuf>,
    /// Every profile and provider, the implicit `default` and `anthropic`
    /// that `[model]` names included: resolved once, when the document is read.
    #[serde(skip)]
    resolved: Resolved,
}

#[derive(Debug, Clone, Default)]
struct Resolved {
    profiles: BTreeMap<String, ProfileConfig>,
    providers: BTreeMap<String, ProviderConfig>,
}

/// `[discord]`: the Discord binding (M3). It connects only when the token
/// secret resolves and the bindings file exists; otherwise health reports it
/// unconfigured and nothing else changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Name of the `[secrets]` entry holding the bot token.
    #[serde(default = "default_discord_token_secret")]
    pub token_secret: String,
    /// Which Discord places Theseus lives in and who may drive it there (spec P5).
    /// Relative paths resolve in the state dir, beside the store that remembers
    /// each place's session, so a scratch instance never binds by accident.
    #[serde(default = "default_bindings_file")]
    pub bindings_file: String,
    /// How often a streaming reply is edited; Discord rate-limits edits per channel.
    #[serde(default = "default_edit_interval_ms")]
    pub edit_interval_ms: u64,
    /// Post each notified call's own embed, edited with its outcome. Off (the
    /// default), the call's line in the loop's tool message carries the notice;
    /// the ledger row and the web UI's notice are the same either way.
    #[serde(default)]
    pub notice_embeds: bool,
    /// The kinds of message that post silent (theseus-l1y1): none by default.
    #[serde(default)]
    pub silent: Vec<discord::Category>,
    /// At most one ping per place in this many seconds; 0, the default, is off.
    #[serde(default)]
    pub ping_window_secs: u64,
    /// The task board as one pinned Components V2 message that follows the
    /// work engine's tasks and never pings (theseus-2j2ka); off, or on a bot
    /// that cannot send V2, the plain-text board of the task records.
    #[serde(default = "default_true")]
    pub live_board: bool,
    /// A private text channel for each multi-step or long task, under a
    /// category the bot makes in the one guild the bindings file trusts
    /// (theseus-bh3t.1); a task's steering happens there. With no trusted
    /// guild, or more than one, nothing is made.
    #[serde(default = "default_true")]
    pub task_channels: bool,
    /// A working task with no channel yet gets one this many minutes after it
    /// began.
    #[serde(default = "discord::default_task_channel_after_minutes")]
    pub task_channel_after_minutes: u64,
    /// At most this many task channels open at once; a task past it goes on
    /// as it did before channels.
    #[serde(default = "discord::default_task_channel_cap")]
    pub task_channel_cap: u64,
    /// A finished task's channel is deleted this many days after it ended.
    #[serde(default = "discord::default_task_channel_keep_days")]
    pub task_channel_keep_days: u64,
    /// The bot's presence reads the counts of what needs you and what runs
    /// (`🔴 1 needs you · ⚙️ 3 running`); everyone who can see the bot sees it.
    #[serde(default = "default_true")]
    pub status_line: bool,
    /// Task states drawn with the bot's own animated emojis, uploaded once;
    /// off, or while Discord refuses them, the Unicode ones (theseus-ss36w).
    #[serde(default = "default_true")]
    pub app_emojis: bool,
    /// The local times of day (`"HH:MM"`) a digest of what happened is posted
    /// to the owner's DM, only when something did; empty is none.
    #[serde(default = "discord::default_digest_at")]
    pub digest_at: Vec<String>,
    /// A local stand-in for Discord's REST API, `host:port` over plain http
    /// (twilight's proxy base): tests and scratch daemons only, so that no
    /// request and no token leaves the machine (theseus-q4v).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_proxy: Option<String>,
    /// The same for the gateway: a `ws://host:port` URL connected instead of
    /// Discord's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_proxy: Option<String>,
    /// One private forum in the trusted guild that is the home of every task:
    /// a post each (theseus-bh3t.6). Off: no forum is read, made or posted to.
    #[serde(default = "default_true")]
    pub forum: bool,
    /// The forum, by Discord id (15 to 21 digits) or by name; made when no
    /// forum of that name exists.
    #[serde(default = "discord::default_forum_channel")]
    pub forum_channel: String,
    /// One private text channel in the trusted guild that is the dashboard of
    /// the owner's tasks: the pinned board, every report and one pointer a
    /// task to its post (theseus-xlxp4). A name or a Discord id; empty is off.
    #[serde(default = "discord::default_task_home")]
    pub task_home: String,
    /// `[[discord.thread]]`: a channel the bindings file names, run as a
    /// thread's place (theseus-sjzm); none by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thread: Vec<discord::ThreadBind>,
    /// `[discord.calendar]`: the agenda as the guild's scheduled events; off by default.
    #[serde(default)]
    pub calendar: calendar::CalendarConfig,
}

fn default_discord_token_secret() -> String {
    "discord_bot_token".into()
}
fn default_bindings_file() -> String {
    "bindings.toml".into()
}
fn default_edit_interval_ms() -> u64 {
    1200
}

impl DiscordConfig {
    pub fn bindings_path(&self, state_dir: &std::path::Path) -> PathBuf {
        let p = expand(&self.bindings_file);
        if p.is_absolute() {
            p
        } else {
            state_dir.join(p)
        }
    }
}

/// `[tools]`: where toollets may work and how much they may return (§3.23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The operator's projects directory: the workspace tools work in, and
    /// where relative paths resolve. No built-in default; without it (and
    /// without `roots`) every path is outside the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects_dir: Option<String>,
    /// More workspace roots beside `projects_dir`. A read or a program run
    /// outside every root waits for approval; a write there takes its tool's
    /// posture (theseus-ewi).
    #[serde(default)]
    pub roots: Vec<String>,
    /// Where relative paths resolve and programs run by default (default: `projects_dir`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Paths that wait for approval, even under a root.
    #[serde(default = "default_approve_paths")]
    pub approve_paths: Vec<String>,
    /// Characters of a tool result the model sees (head and tail kept).
    #[serde(default = "default_result_max_chars")]
    pub result_max_chars: usize,
    #[serde(default = "default_max_read_bytes")]
    pub max_read_bytes: usize,
    /// The largest file a surface accepts whole (theseus-c9l6): a Discord
    /// attachment, `theseus ask --attach`. It is kept in the store's blobs,
    /// and a PDF is read for the model. The default, 32 MiB, is the
    /// provider's limit on a request that carries a PDF.
    #[serde(default = "default_max_attachment_bytes")]
    pub max_attachment_bytes: u64,
    /// The most of one recording `file.read` hears (theseus-c9l6): Deepgram
    /// bills by the minute (about $0.26 an hour on nova-3), and a longer
    /// recording is heard to here, which its transcript says.
    #[serde(default = "default_transcribe_max_minutes")]
    pub transcribe_max_minutes: u64,
    /// Entries a listing, glob, or grep returns.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    /// How long a turn waits for `proc.run` before the job continues in the background.
    #[serde(default = "default_proc_sync_secs")]
    pub proc_sync_secs: u64,
    /// The longest one `job_wait` waits (theseus-7m7w.3).
    #[serde(default = "default_job_wait_max_secs")]
    pub job_wait_max_secs: u64,
    /// A `job_wait` on a job that printed nothing for this long returns, with
    /// what it saw (theseus-53ujg.6); 0 waits as long as asked.
    #[serde(default = "default_job_wait_beat_secs")]
    pub job_wait_beat_secs: u64,
    /// At each beat, one status line to the session's place (theseus-5d204).
    #[serde(default = "default_job_wait_status")]
    pub job_wait_status: bool,
    /// What `task_wait` waits when the call names no time (theseus-z9mze.2).
    #[serde(default = "default_fan_out_wait_secs")]
    pub fan_out_wait_secs: u64,
    /// The longest one `task_wait` waits, however long the model asks.
    #[serde(default = "default_fan_out_max_wait_secs")]
    pub fan_out_max_wait_secs: u64,
    /// The most characters of reports one wait brings into the turn at once.
    #[serde(default = "default_fan_in_chars")]
    pub fan_in_chars: usize,
    /// The least a `task_fan_out` helper's share may be, in dollars; never less
    /// than twice the worst case one of its calls reserves (theseus-z9mze.3).
    #[serde(default = "default_helper_floor_usd")]
    pub helper_floor_usd: f64,
    /// The most output tokens one call of a helper may ask for.
    #[serde(default = "default_helper_max_output_tokens")]
    pub helper_max_output_tokens: u32,
    #[serde(default = "default_parallel_runs")]
    pub parallel_runs: usize,
    #[serde(default = "default_parallel_reads")]
    pub parallel_reads: usize,
    #[serde(default = "default_parallel_writes")]
    pub parallel_writes: usize,
    #[serde(default = "default_proc_timeout_secs")]
    pub proc_timeout_secs: u64,
    #[serde(default = "default_proc_timeout_max_secs")]
    pub proc_timeout_max_secs: u64,
    /// Environment variables `proc.run` passes through from the daemon (nothing else).
    #[serde(default = "default_proc_env")]
    pub proc_env: Vec<String>,
    /// The most a job's raw output file keeps (theseus-102). Past it the job runs on, what it prints is counted
    /// and dropped, and its result says so. The runtime reads only the file's last 4 MiB.
    #[serde(default = "default_job_output_max_bytes")]
    pub job_output_max_bytes: u64,
    /// A capped result's whole output, kept in `<state>/outputs/` (theseus-v73m),
    /// is deleted after this many days, at its session's retirement, or,
    /// oldest first, while all of them pass `outputs_max_bytes`.
    #[serde(default = "outputs::keep_days")]
    pub outputs_keep_days: u64,
    #[serde(default = "outputs::max_bytes")]
    pub outputs_max_bytes: u64,
    /// Each L0 job's cgroup's `pids.max` (theseus-a5nv), where the daemon's
    /// cgroup is delegated: its processes and threads at once. 0: no cap.
    #[serde(default = "default_job_pids_max")]
    pub job_pids_max: u64,
    /// `[tools.web]`: `http.fetch` and `web.search` (DD5).
    #[serde(default)]
    pub web: WebToolsConfig,
    #[serde(default)]
    pub term: term::TermToolsConfig,
    /// `[tools.defer]`: which tool families a request loads (theseus-7m7w.2).
    #[serde(default)]
    pub defer: defer::DeferConfig,
}

fn default_approve_paths() -> Vec<String> {
    [
        "~/.ssh",
        "~/.gnupg",
        "~/.aws",
        "~/.config/op",
        "~/.openclaw-1password-service-token",
        "~/.theseus",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
fn default_result_max_chars() -> usize {
    30_000
}
fn default_max_read_bytes() -> usize {
    262_144
}
fn default_max_attachment_bytes() -> u64 {
    theseus_files::MAX_FILE_BYTES
}
fn default_transcribe_max_minutes() -> u64 {
    60
}
fn default_max_entries() -> usize {
    500
}
fn default_proc_sync_secs() -> u64 {
    60
}
fn default_proc_timeout_secs() -> u64 {
    3600
}
fn default_proc_timeout_max_secs() -> u64 {
    3600
}
fn default_job_output_max_bytes() -> u64 {
    theseus_kernel::job::DEFAULT_OUTPUT_MAX_BYTES
}
fn default_job_pids_max() -> u64 {
    theseus_kernel::cgroup::DEFAULT_PIDS_MAX
}
/// The smallest cap a job's output file may have: a page.
const MIN_JOB_OUTPUT_MAX_BYTES: u64 = 4096;
fn default_proc_env() -> Vec<String> {
    [
        "PATH",
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "TERM",
        "TZ",
        "CARGO_HOME",
        "RUSTUP_HOME",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            projects_dir: None,
            roots: vec![],
            cwd: None,
            approve_paths: default_approve_paths(),
            result_max_chars: default_result_max_chars(),
            max_read_bytes: default_max_read_bytes(),
            max_attachment_bytes: default_max_attachment_bytes(),
            transcribe_max_minutes: default_transcribe_max_minutes(),
            max_entries: default_max_entries(),
            proc_sync_secs: default_proc_sync_secs(),
            job_wait_max_secs: default_job_wait_max_secs(),
            job_wait_beat_secs: default_job_wait_beat_secs(),
            job_wait_status: default_job_wait_status(),
            fan_out_wait_secs: default_fan_out_wait_secs(),
            fan_out_max_wait_secs: default_fan_out_max_wait_secs(),
            fan_in_chars: default_fan_in_chars(),
            helper_floor_usd: default_helper_floor_usd(),
            helper_max_output_tokens: default_helper_max_output_tokens(),
            parallel_runs: default_parallel_runs(),
            parallel_reads: default_parallel_reads(),
            parallel_writes: default_parallel_writes(),
            proc_timeout_secs: default_proc_timeout_secs(),
            proc_timeout_max_secs: default_proc_timeout_max_secs(),
            proc_env: default_proc_env(),
            job_output_max_bytes: default_job_output_max_bytes(),
            job_pids_max: default_job_pids_max(),
            outputs_keep_days: outputs::keep_days(),
            outputs_max_bytes: outputs::max_bytes(),
            web: WebToolsConfig::default(),
            term: term::TermToolsConfig::default(),
            defer: defer::DeferConfig::default(),
        }
    }
}

/// `[broker]` (theseus-dcy): the secret broker's grants, and each secret's
/// posture.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerConfig {
    /// `[broker.programs.<program>]`: what a program gets when a job runs it
    /// by its own argv.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub programs: BTreeMap<String, ProgramGrant>,
    /// `[broker.secrets.<name>]`: a secret's posture.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretPosture>,
}

impl BrokerConfig {
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty() && self.secrets.is_empty()
    }
}

/// One program's grant: each environment variable it gets, with the name of
/// the `[secrets]` entry whose value it holds; or, for a program that calls
/// AWS (`aws`), an account whose short-lived job session it gets at launch,
/// under the guards, and never the key (AWS design §3.5).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramGrant {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_account: Option<String>,
}

/// A secret's posture: a call given the secret runs at no looser a posture.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretPosture {
    #[serde(default = "default_secret_posture")]
    pub posture: crate::policy::Posture,
}

fn default_secret_posture() -> crate::policy::Posture {
    crate::policy::Posture::Notify
}

/// `[policy]`: the posture every tool and MCP inherits, the per-tool and
/// per-MCP exceptions, and the operator's explicit argv lists (§3.9; see
/// `policy` for the order they apply in).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    /// The posture every tool and MCP inherits: open | notify | approve.
    #[serde(default)]
    pub enforcement: crate::policy::Posture,
    /// `proc.run` argv prefixes that run (open) when every path argument is
    /// inside the roots. A prefix covers any arguments after it.
    #[serde(default = "default_allow_argv")]
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that wait for approval.
    #[serde(default = "default_approve_argv")]
    pub approve_argv: Vec<Vec<String>>,
    /// Per-tool postures by canonical name, e.g. `"proc.run" = "approve"`.
    #[serde(default)]
    pub tools: BTreeMap<String, crate::policy::Posture>,
    /// Per-MCP postures: `"server"` (every tool from that server) or
    /// `"server/tool"` (one tool), for tools named `mcp:<server>/<tool>`.
    #[serde(default)]
    pub mcp: BTreeMap<String, crate::policy::Posture>,
    /// Programs that host an agent (theseus-5rvss): a CLI answer from a
    /// process under one, or from one with no controlling terminal, is
    /// recorded as that agent's (`peer_proc`), and an agent never answers a
    /// question that spends money or changes AWS. Matched by file name, in
    /// any case.
    #[serde(default = "default_agent_processes")]
    pub agent_processes: Vec<String>,
    /// A URL whose host is a private address (theseus-7gir.20): `ask`, the
    /// default, waits for approval (DD5); `open` judges it as any other, for a
    /// run with no operator to ask (the bench profile).
    #[serde(default)]
    pub private_addresses: crate::web::net::PrivateAddresses,
    /// `[policy.aws]` (AWS design §3.9): an AWS call's posture when its tool
    /// has no `[policy.tools]` line: `"service:Operation"`, then `"service"`,
    /// then its class's (`read`), then `enforcement`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aws: BTreeMap<String, crate::policy::Posture>,
    /// `[policy.earned]`, retired: earned autonomy is gone. A config of an
    /// earlier build still has the section; it is read, ignored, and warned
    /// of (`Config::parse`), never written back.
    #[serde(default, skip_serializing)]
    pub earned: Option<toml::Table>,
}

fn argvs(v: &[&[&str]]) -> Vec<Vec<String>> {
    v.iter()
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .collect()
}
fn default_allow_argv() -> Vec<Vec<String>> {
    // No git: its config and attributes can run programs (fsmonitor, textconv,
    // external diff), and the native git.diff / git.log toollets read history
    // without the git binary.
    argvs(&[&["ls"], &["pwd"]])
}
fn default_approve_argv() -> Vec<Vec<String>> {
    // No `op` or `theseusd`: the floor asks for those whatever the lists say.
    argvs(&[
        &["sudo"],
        &["su"],
        &["doas"],
        &["rm", "-rf", "/"],
        &["mkfs"],
        &["dd"],
        &["shutdown"],
        &["reboot"],
    ])
}
fn default_agent_processes() -> Vec<String> {
    ["claude", "codex", "gemini", "aider", "opencode", "openclaw"]
        .map(String::from)
        .to_vec()
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            enforcement: Default::default(),
            allow_argv: default_allow_argv(),
            approve_argv: default_approve_argv(),
            tools: BTreeMap::new(),
            mcp: BTreeMap::new(),
            agent_processes: default_agent_processes(),
            private_addresses: Default::default(),
            earned: None,
            aws: BTreeMap::new(),
        }
    }
}

/// `[kernel]`: the durable kernel's knobs (spec §3.2a, §3.15, §3.16; M2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelSection {
    /// How many executions may hold a turn at once.
    #[serde(default = "default_admission_ceiling")]
    pub admission_ceiling: u32,
    /// How long admission may go without admitting any turn before a
    /// waiting turn fails (theseus-4g0l): a queue that moves fails none.
    #[serde(default = "default_admission_stuck_secs")]
    pub admission_stuck_secs: u64,
    /// Each session's spend limit in US dollars (theseus-0sg). What reaching
    /// it does is `spend_limit_mode`: a notice, or the question (theseus-usei).
    #[serde(default = "limits::default_spend_limit_usd")]
    pub spend_limit_usd: f64,
    #[serde(default, skip_serializing_if = "SpendLimitMode::is_default")]
    pub spend_limit_mode: SpendLimitMode,
    /// Under notify, the multiple of the limit at which calls stop anyway
    /// (theseus-jse4); 0 never.
    #[serde(default = "limits::default_hard_multiple")]
    pub spend_limit_hard_multiple: u64,
    /// The daemon's day ceiling over every model call (theseus-kp20).
    #[serde(default = "limits::default_daily_ceiling_usd")]
    pub daily_spend_ceiling_usd: f64,
    /// What reaching it does (theseus-9e02): a warning, or the stop.
    #[serde(default, skip_serializing_if = "DailyCeilingMode::is_default")]
    pub daily_spend_ceiling_mode: DailyCeilingMode,
    /// Under notify, where the day stops (theseus-9e02); 0 never.
    #[serde(default = "limits::default_daily_runaway_usd")]
    pub daily_runaway_usd: f64,
    /// Under notify, a warning at the ceiling and every this much after it.
    #[serde(default = "limits::default_daily_notice_step_usd")]
    pub daily_notice_step_usd: f64,
    /// A session pauses, and asks to resume, when its settled spend in ten
    /// minutes reaches this; 0 never.
    #[serde(default = "limits::default_runaway_per_10min_usd")]
    pub runaway_usd_per_10min: f64,
    /// Heartbeat reconciler cadence.
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
    /// Deadline for an action whose tool declares none.
    #[serde(default = "default_deadline_secs")]
    pub default_deadline_secs: u64,
    /// How long a call's question waits before the owner is told, once, and
    /// again each further life: it stays pending, never expired (theseus-v2dnc).
    /// A proposed extension's ack and a pack's promotion still expire.
    #[serde(default = "default_confirm_ttl_secs")]
    pub confirm_ttl_secs: u64,
    /// The shortest span a repeating wake may take, in minutes (37a): a
    /// floor on a runaway series' turns. At least 1.
    #[serde(default = "default_min_repeat_minutes")]
    pub min_repeat_minutes: u64,
    /// A task claim's lease, in minutes (39b): the holder's edits renew it.
    /// At least 1.
    #[serde(default = "default_task_lease_minutes")]
    pub task_lease_minutes: u64,
    /// The furthest ahead a wake, a reminder or a deferred task may be set, in
    /// days (theseus-8ux6x): 365.
    #[serde(default = "wake_limits::default_max_ahead_days")]
    pub max_wake_ahead_days: u64,
    /// Check-ins one conversation may hold at once (`wake.at`): 20.
    #[serde(default = "wake_limits::default_max_wakes")]
    pub max_wakes: usize,
    /// Reminders the daemon may hold at once, over every conversation: 100.
    #[serde(default = "wake_limits::default_max_reminders")]
    pub max_reminders: usize,
    /// Routines and watches, declared for their rows: 50 and 20.
    #[serde(default = "wake_limits::default_max_watches")]
    pub max_watches: usize,
    /// A person's message queued behind a running turn interrupts that turn's
    /// waits and joins it at its next loop (theseus-qu1lc); off, it waits for
    /// the turn to end and runs as the next one.
    #[serde(default = "default_true")]
    pub steer_queued: bool,
    /// How many levels of tasks may sit below a conversation: a task starts
    /// tasks until this depth (theseus-tecj5, the owner's "lets start with 10
    /// deep, and observe"). 1 to 32.
    #[serde(default = "default_task_depth")]
    pub task_depth: u32,
}

fn default_task_depth() -> u32 {
    theseus_kernel::task_depth::DEFAULT_DEPTH_LIMIT
}

fn default_admission_ceiling() -> u32 {
    8
}
fn default_admission_stuck_secs() -> u64 {
    600
}
fn default_heartbeat_secs() -> u64 {
    60
}
fn default_deadline_secs() -> u64 {
    600
}
fn default_confirm_ttl_secs() -> u64 {
    900
}
fn default_min_repeat_minutes() -> u64 {
    5
}
fn default_task_lease_minutes() -> u64 {
    30
}

impl Default for KernelSection {
    fn default() -> Self {
        Self {
            admission_ceiling: default_admission_ceiling(),
            admission_stuck_secs: default_admission_stuck_secs(),
            spend_limit_usd: limits::default_spend_limit_usd(),
            spend_limit_mode: SpendLimitMode::default(),
            spend_limit_hard_multiple: limits::default_hard_multiple(),
            daily_spend_ceiling_usd: limits::default_daily_ceiling_usd(),
            daily_spend_ceiling_mode: DailyCeilingMode::default(),
            daily_runaway_usd: limits::default_daily_runaway_usd(),
            daily_notice_step_usd: limits::default_daily_notice_step_usd(),
            runaway_usd_per_10min: limits::default_runaway_per_10min_usd(),
            heartbeat_secs: default_heartbeat_secs(),
            default_deadline_secs: default_deadline_secs(),
            confirm_ttl_secs: default_confirm_ttl_secs(),
            min_repeat_minutes: default_min_repeat_minutes(),
            task_lease_minutes: default_task_lease_minutes(),
            max_wake_ahead_days: wake_limits::default_max_ahead_days(),
            max_wakes: wake_limits::default_max_wakes(),
            max_reminders: wake_limits::default_max_reminders(),
            max_watches: wake_limits::default_max_watches(),
            steer_queued: true,
            task_depth: default_task_depth(),
        }
    }
}

impl KernelSection {
    /// The admission queue's stuck window: at least a second.
    pub fn admission_stuck(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.admission_stuck_secs.max(1))
    }

    /// A task claim's lease, in milliseconds: at least a minute.
    pub fn task_lease_ms(&self) -> u64 {
        self.task_lease_minutes.max(1) * 60_000
    }

    pub fn to_kernel_config(&self) -> theseus_kernel::KernelConfig {
        theseus_kernel::KernelConfig {
            admission_ceiling: self.admission_ceiling.max(1),
            default_deadline_ms: self.default_deadline_secs * 1000,
            spend_limit_micros: theseus_kernel::usd_to_micros(self.spend_limit_usd),
            spend_limit_notify: self.spend_limit_mode == SpendLimitMode::Notify,
            confirm_ttl_ms: self.confirm_ttl_secs * 1000,
            heartbeat_ms: self.heartbeat_secs.max(1) * 1000,
            fault_after_startup_step: None,
            min_repeat_ms: self.min_repeat_minutes.max(1) * 60_000,
            daily_ceiling_micros: theseus_kernel::usd_to_micros(self.daily_spend_ceiling_usd),
            spend_hard_multiple: self.spend_limit_hard_multiple,
            daily_notify: self.daily_spend_ceiling_mode == DailyCeilingMode::Notify,
            daily_runaway_micros: theseus_kernel::usd_to_micros(self.daily_runaway_usd),
            daily_step_micros: theseus_kernel::usd_to_micros(self.daily_notice_step_usd),
            burn_line_micros: theseus_kernel::usd_to_micros(self.runaway_usd_per_10min),
            wakes: wake_limits::limits(self),
            task_depth_limit: self
                .task_depth
                .clamp(1, theseus_kernel::task_depth::DEPTH_LIMIT_MAX),
            // The system's zone.
            ..Default::default()
        }
    }
}

/// The localhost web UI. Bound to loopback only; no auth yet (spec §3.14).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// A loopback address (`127.0.0.1` or `::1`): anything else fails to
    /// load until the web UI has auth (theseus-2fo).
    #[serde(default = "default_web_bind")]
    pub bind: String,
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// For UI development only (theseus-zab): the Vite dev page's origin
    /// (`npm run dev` in `cockpit/`), such as `http://127.0.0.1:5174`. The
    /// cockpit's page opens `/ws` straight, with that `Origin`, which the UI
    /// otherwise refuses; a dev server's proxy would pass the page's `Host`
    /// too. While set, a `/ws` upgrade from exactly this
    /// origin is served too, counted in health, and ledgered
    /// (`web.dev_origin`); every other route still answers only the UI's own
    /// address. Off by default. `http://`, `localhost` or a loopback address,
    /// and a port, else the config fails to load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_origin: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_web_bind() -> String {
    "127.0.0.1".into()
}
fn default_web_port() -> u16 {
    7433
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: default_web_bind(),
            port: default_web_port(),
            dev_origin: None,
        }
    }
}

/// A dev page's origin as a browser sends it, on this machine
/// (theseus-zab): `http://`, then `localhost` or a loopback address (an
/// IPv6 one in brackets), then a port. Nothing else: no path, no user, no
/// other host.
pub fn dev_origin_ok(origin: &str) -> bool {
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => match rest.split_once("]:") {
            Some((host, port)) => (host, port),
            None => return false,
        },
        None => match authority.rsplit_once(':') {
            Some((host, _)) if host.contains(':') => return false,
            Some(split) => split,
            None => return false,
        },
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    loopback && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|p| p > 0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    /// Name of the entry in `[secrets]` holding the GitHub token; checked at startup.
    #[serde(default = "default_github_secret")]
    pub token_secret: String,
    /// Warn when the token expires within this many days.
    #[serde(default = "default_warn_days")]
    pub warn_days: i64,
}

fn default_github_secret() -> String {
    "github_token".into()
}
fn default_warn_days() -> i64 {
    30
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            token_secret: default_github_secret(),
            warn_days: default_warn_days(),
        }
    }
}

/// A model endpoint speaking the Anthropic Messages API. The first-party API
/// is one; Z.ai's GLM series exposes the same protocol at another URL, so a
/// second provider is a table entry, not code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub api_base: String,
    /// Name of the entry in `[secrets]` holding this provider's key.
    pub api_key_secret: String,
    #[serde(default)]
    pub kind: ProviderKind,
    #[serde(default)]
    pub timeouts: Option<crate::provider::Timeouts>,
    /// The operator's word for where this host's models come from, for a
    /// host Theseus does not know (a company proxy to Anthropic). Without
    /// it an unknown host is `unknown`, which is restricted; on a China-origin
    /// host only `cn` is accepted.
    #[serde(default)]
    pub origin: Option<crate::catalog::ModelOrigin>,
}

/// A provider's wire protocol. Only the Anthropic Messages API exists today.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    AnthropicMessages,
}

/// A named way of running turns: which provider, which model, how long an
/// answer may be, and what system prompt. Exactly one profile is **live** at
/// a time; the live one can be switched over the protocol and the switch
/// persists. Turns may name a profile explicitly; later, sessions and tasks
/// will carry their own override.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    #[serde(default = "default_provider_name")]
    pub provider: String,
    pub model: String,
    /// Cap on tokens the model may *generate* per call (the Messages API's
    /// `max_tokens`). Not an input limit; input is whatever the compiler
    /// assembles. Omitted: the model's ceiling from the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub system: Option<String>,
    /// For models that accept effort. Omitted: the model's default (Opus 5.5:
    /// medium; others: high).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// `effort` stands whatever Jev's route.v3 answers (theseus-qe3v).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub effort_fixed: bool,
    #[serde(default)]
    pub thinking_display: ThinkingDisplay,
    /// Tool loops per turn before a notice, or the turn's end: `on_max_loops`.
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
    #[serde(default, skip_serializing_if = "MaxLoopsMode::is_default")]
    pub on_max_loops: MaxLoopsMode,
    /// Server-side refusal fallbacks where the model supports them.
    #[serde(default = "default_true")]
    pub refusal_fallbacks: bool,
    /// How long the provider keeps the profile's cached prefixes (theseus-ev1);
    /// none: `[cache]`'s ttl, as `keep_warm_hours` (theseus-ezeg, `config/cache.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_ttl: Option<CacheTtl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_warm_hours: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

/// What thinking blocks carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingDisplay {
    #[default]
    Summarized,
    Omitted,
    Updates,
}

fn default_max_loops() -> u32 {
    40
}

impl ProfileConfig {
    /// The output cap in force: configured, else the catalog ceiling, else 16384.
    pub fn effective_max_tokens(&self, catalog: &crate::catalog::Catalog) -> u32 {
        self.max_output_tokens
            .or_else(|| catalog.get(&self.model).map(|e| e.max_output_tokens))
            .unwrap_or(16_384)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// Name of the profile that is live at startup (a key of `[profiles]`,
    /// or "default" for the implicit profile built from this section).
    /// A persisted runtime switch (`profile.use`) takes precedence, until `profile.release`.
    #[serde(default = "default_profile_name")]
    pub live: String,
    /// Default provider name: a key of `[providers]`, or "anthropic" for the implicit one.
    #[serde(default = "default_provider_name")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// Output cap per call for the implicit default profile; see `ProfileConfig`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(default)]
    pub thinking_display: ThinkingDisplay,
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
    #[serde(default, skip_serializing_if = "MaxLoopsMode::is_default")]
    pub on_max_loops: MaxLoopsMode,
    #[serde(default = "default_true")]
    pub refusal_fallbacks: bool,
    /// A turn that would end leaving new files its commands made beside its edits, or with files
    /// edited since its last command, gets one factual harness note and one more loop
    /// (theseus-53ujg.1). Off: no note; the files lines of results stay.
    #[serde(default = "default_true")]
    pub end_note: bool,
    #[serde(default = "default_api_base")]
    pub api_base: String,
    /// Name of the entry in `[secrets]` holding the Anthropic key.
    #[serde(default = "default_key_name")]
    pub api_key_secret: String,
    #[serde(default)]
    pub timeouts: crate::provider::Timeouts,
    #[serde(default)]
    pub retries: Retries,
}

/// `[model.retries]` (theseus-7gir.21): a call that fails with a class that
/// passes with time is made again inside its turn, after a backoff that
/// doubles each time, at most `transient` times. 2, the default: a 429 or a
/// 503 is made again inside its turn, where the model can still answer it. 0:
/// none; the turn fails and the session's driver retries it later
/// (theseus-ljr). A headless run ends with its turn, so it has no later retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retries {
    #[serde(default)]
    pub transient: u32,
    /// The wait before the first retry.
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,
    /// The longest one wait.
    #[serde(default = "default_backoff_max_ms")]
    pub backoff_max_ms: u64,
    /// A refused request is made once more on its model's fallback, the
    /// catalog's `refusal_fallback_model` (theseus-7gir.18). On by default.
    #[serde(default = "default_true")]
    pub refusal: bool,
    /// A refusal of a category that is not a genuine decline is made once
    /// more on the same model before the fallback answers (theseus-keuap): a
    /// refusal is often incidental, and the fallback is a weaker model. At
    /// most one retry per refused request and three per turn. On by default;
    /// `refusal = false` turns it off with the fallback.
    #[serde(default = "default_true")]
    pub refusal_retry: bool,
    /// The refusal categories that are a genuine decline (theseus-wf482). The
    /// fallback answers the refused request and the next goes back to the
    /// routed model; a refusal with one of these categories keeps the turn on
    /// the fallback instead, so a safety decline never bounces back to the
    /// model that declined it. Any other refusal does so only at its third in
    /// a row.
    #[serde(default = "default_hard_decline_categories")]
    pub hard_decline_categories: Vec<String>,
}

fn default_hard_decline_categories() -> Vec<String> {
    ["cyber", "bio", "general_harms"].map(String::from).into()
}
fn default_backoff_ms() -> u64 {
    2_000
}
fn default_backoff_max_ms() -> u64 {
    30_000
}

impl Default for Retries {
    fn default() -> Self {
        Self {
            transient: 2,
            backoff_ms: default_backoff_ms(),
            backoff_max_ms: default_backoff_max_ms(),
            refusal: true,
            refusal_retry: true,
            hard_decline_categories: default_hard_decline_categories(),
        }
    }
}

impl Retries {
    /// The wait before retry `n`, the first being 0.
    pub fn wait(&self, n: u32) -> std::time::Duration {
        let doubled = self.backoff_ms.saturating_mul(1 << n.min(30));
        std::time::Duration::from_millis(doubled.min(self.backoff_max_ms))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
    /// Retired (route.v10, theseus-h66v4: the China-origin draft is gone):
    /// read, warned of (`Config::parse`), ignored and never written back.
    #[serde(default, skip_serializing)]
    pub drafts_dir: Option<String>,
    #[serde(default = "default_socket")]
    pub socket: String,
    /// Index engine behind the WAL: "redb", the only one (theseus-0g4). Still
    /// accepted so a config that names it loads; any other value is refused.
    #[serde(default)]
    pub store_engine: theseus_store::Engine,
    /// How long a clean stop waits, from its start, for the posts it has
    /// already sent to settle (theseus-pfv). §9 holds a clean stop to 100 ms.
    /// A post still in flight then stays dispatched, and the next start sends
    /// it again under the same nonce. 0 waits for none.
    #[serde(default = "default_stop_grace_ms")]
    pub stop_grace_ms: u64,
    /// Health warns when the filesystem under the state dir has less free
    /// space than this, in MB (theseus-102). 0 never warns.
    #[serde(default = "default_disk_warn_mb")]
    pub disk_warn_mb: u64,
    /// A job is refused, with its reason, when the filesystem under the state
    /// dir has less free space than this, in MB (theseus-102), so the store
    /// keeps room to write. 0 refuses none.
    #[serde(default = "default_disk_floor_mb")]
    pub disk_floor_mb: u64,
    #[serde(default)]
    pub max_connections: u64,
}

fn default_stop_grace_ms() -> u64 {
    50
}

/// Five floors: health says the disk is low while there is still room to
/// act before jobs are refused. One build's target dir can take several GB.
fn default_disk_warn_mb() -> u64 {
    5 * 1024
}

/// Room, once new jobs are refused, for what already runs to finish and be
/// written down: eight turns at once (the admission ceiling), each with a
/// job at its 64 MiB output cap, is 512 MiB, beside a whole 64 MiB WAL
/// segment and the index's checkpoint. It protects the store, whose appends
/// fail on a full disk, the rows that would say so among them.
fn default_disk_floor_mb() -> u64 {
    1024
}

/// The longest grace a clean stop may give the posts in flight.
const MAX_STOP_GRACE_MS: u64 = 10_000;

fn default_model() -> String {
    "claude-sonnet-5-5".into()
}
fn default_provider_name() -> String {
    "anthropic".into()
}
fn default_profile_name() -> String {
    "default".into()
}
fn default_api_base() -> String {
    "https://api.anthropic.com".into()
}
fn default_key_name() -> String {
    "anthropic_api_key".into()
}
/// Where state lives unless `[server].state_dir` or `--state-dir` says
/// otherwise; the config copy is found here before any config is read.
pub const DEFAULT_STATE_DIR: &str = "~/.theseus";

fn default_state_dir() -> String {
    DEFAULT_STATE_DIR.into()
}
fn default_socket() -> String {
    "~/.theseus/theseus.sock".into()
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            live: default_profile_name(),
            provider: default_provider_name(),
            model: default_model(),
            max_output_tokens: None,
            system: None,
            effort: None,
            thinking_display: ThinkingDisplay::Summarized,
            max_loops: default_max_loops(),
            on_max_loops: MaxLoopsMode::default(),
            refusal_fallbacks: true,
            end_note: true,
            api_base: default_api_base(),
            api_key_secret: default_key_name(),
            timeouts: Default::default(),
            retries: Default::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            state_dir: default_state_dir(),
            drafts_dir: None,
            socket: default_socket(),
            store_engine: theseus_store::Engine::Redb,
            stop_grace_ms: default_stop_grace_ms(),
            disk_warn_mb: default_disk_warn_mb(),
            disk_floor_mb: default_disk_floor_mb(),
            max_connections: 0,
        }
    }
}

impl Config {
    /// `source` is either an `op://` reference or a filesystem path.
    pub async fn load(source: &str, op: &OpReader) -> Result<Self> {
        Ok(Self::load_text(source, op).await?.0)
    }

    /// `load`, and the document's text as it was read: the vault's note is
    /// kept as the last-known-good copy (theseus-2fo).
    pub async fn load_text(source: &str, op: &OpReader) -> Result<(Self, String)> {
        let text = if source.starts_with("op://") {
            let r = SecretRef::parse(source)?;
            let secret = op
                .read(&r)
                .await
                .with_context(|| format!("reading config from {source}"))?;
            secret.expose().to_string()
        } else {
            let path = expand(source);
            std::fs::read_to_string(&path)
                .with_context(|| format!("reading config file {}", path.display()))?
        };
        let (cfg, warnings) =
            Self::parse(&text).with_context(|| format!("loading config {source}"))?;
        for w in &warnings {
            tracing::warn!("{w}");
        }
        Ok((cfg, text))
    }

    /// A field set in this config that could carry a credential, by name
    /// (theseus-2fo): a URL with a user, a password, or a query. The loader
    /// refuses a value in `[secrets]`; every other field is a reference, a
    /// name, an id, a number, a path, a posture, an argv prefix, or prose for
    /// the model. A note with such a URL is never kept as a copy.
    pub fn credential_in_url(&self) -> Option<String> {
        let mut urls = vec![("model.api_base".to_string(), self.model.api_base.as_str())];
        for (name, p) in &self.providers {
            urls.push((format!("providers.{name}.api_base"), p.api_base.as_str()));
        }
        if let Some(e) = self.telemetry.endpoint() {
            urls.push(("telemetry.otlp_endpoint".to_string(), e));
        }
        urls.into_iter()
            .find(|(_, u)| match reqwest::Url::parse(u) {
                Ok(u) => !u.username().is_empty() || u.password().is_some() || u.query().is_some(),
                Err(_) => u.contains('@') || u.contains('?'),
            })
            .map(|(key, _)| key)
    }

    /// Parse and validate a config document. The warnings name what it sets
    /// that does nothing; the caller logs them at startup.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>)> {
        let cfg = Self::from_toml(text).context("parsing config TOML")?;
        cfg.validate()?;
        let mut warnings = Vec::new();
        warnings.extend(cfg.persona_warnings());
        let retired = cfg.routing.retired();
        if !retired.is_empty() {
            warnings.push(format!(
                "{} retired in 1.0.5 (Jev chooses the model and its effort): read and ignored, \
                 remove {}",
                retired.join(", "),
                match retired.len() {
                    1 => "it",
                    _ => "them",
                }
            ));
        }
        if cfg.server.drafts_dir.is_some() {
            warnings.push(
                "[server] drafts_dir is retired: the China-origin draft is gone, so the key \
                 does nothing; remove it"
                    .into(),
            );
        }
        if cfg.policy.earned.is_some() {
            warnings.push(
                "[policy.earned] is retired: earned autonomy and its weekly card are gone, so the \
                 section does nothing; remove it"
                    .into(),
            );
        }
        Ok((cfg, warnings))
    }

    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub fn validate(&self) -> Result<()> {
        self.tui.validate()?;
        for (name, r) in &self.secrets {
            if crate::secrets::is_local(r) {
                crate::secrets::check_local(r).with_context(|| format!("secrets.{name}"))?;
                continue;
            }
            SecretRef::parse(r).with_context(|| {
                format!("secrets.{name} is not a valid op:// reference, env:NAME, or file:PATH")
            })?;
        }
        if !self.secrets.contains_key(&self.model.api_key_secret) {
            anyhow::bail!(
                "model.api_key_secret = {:?} has no matching entry under [secrets]",
                self.model.api_key_secret
            );
        }
        for (name, p) in &self.providers {
            if !self.secrets.contains_key(&p.api_key_secret) {
                anyhow::bail!(
                    "providers.{name}.api_key_secret = {:?} has no matching entry under [secrets]",
                    p.api_key_secret
                );
            }
            if p.origin
                .is_some_and(|o| o != crate::catalog::ModelOrigin::Cn)
                && crate::catalog::is_cn_endpoint(&p.api_base)
            {
                anyhow::bail!(
                    "providers.{name}.origin: {} is a China-origin endpoint, so its origin can only be \
                     \"cn\"",
                    p.api_base
                );
            }
        }
        if !self.all_providers().contains_key(&self.model.provider) {
            anyhow::bail!(
                "model.provider = {:?} is not the implicit \"anthropic\" provider nor a key of [providers]",
                self.model.provider
            );
        }
        self.tools.defer.check().map_err(anyhow::Error::msg)?;
        if self.server.stop_grace_ms > MAX_STOP_GRACE_MS {
            anyhow::bail!(
                "server.stop_grace_ms = {} is over {MAX_STOP_GRACE_MS}: a clean stop waits at most \
                 10 s for the posts in flight",
                self.server.stop_grace_ms
            );
        }
        if self.server.disk_warn_mb > 0 && self.server.disk_floor_mb > self.server.disk_warn_mb {
            anyhow::bail!(
                "server.disk_floor_mb = {} is above server.disk_warn_mb = {}: health would never \
                 warn before jobs are refused",
                self.server.disk_floor_mb,
                self.server.disk_warn_mb
            );
        }
        if self.tools.job_output_max_bytes < MIN_JOB_OUTPUT_MAX_BYTES {
            anyhow::bail!(
                "tools.job_output_max_bytes = {} is under {MIN_JOB_OUTPUT_MAX_BYTES}: a job's output \
                 file keeps at least a page",
                self.tools.job_output_max_bytes
            );
        }
        self.sandbox.validate()?;
        self.self_improve.validate(self)?;
        self.judge.validate(&self.secrets)?;
        let providers = self.all_providers();
        for (name, prof) in &self.profiles {
            if !providers.contains_key(&prof.provider) {
                anyhow::bail!(
                    "profiles.{name}.provider = {:?} is not a configured provider",
                    prof.provider
                );
            }
        }
        if let Some(h) = &self.telemetry.headers_secret {
            if !self.secrets.contains_key(h) {
                anyhow::bail!(
                    "telemetry.headers_secret = {h:?} has no matching entry under [secrets]"
                );
            }
        }
        for (name, prof) in self.all_profiles() {
            if prof.max_loops == 0 {
                anyhow::bail!("profiles.{name}.max_loops must be at least 1");
            }
        }
        // The search key's secret may be absent: web.search is then not
        // offered, and health says why (theseus-4o4c).
        let web = &self.tools.web;
        if !(1..=WEB_TIMEOUT_MAX_SECS).contains(&web.timeout_secs) {
            anyhow::bail!(
                "tools.web.timeout_secs must be 1 to {WEB_TIMEOUT_MAX_SECS}: every in-process call ends by 120 s"
            );
        }
        if web.max_bytes == 0 {
            anyhow::bail!("tools.web.max_bytes must be at least 1");
        }
        if web.search_key_secret.is_empty() {
            anyhow::bail!("tools.web.search_key_secret must name a [secrets] entry");
        }
        // Paths only: the files are read when a turn compiles, never here.
        let files = std::iter::once(("context.files".to_string(), &self.context.files)).chain(
            self.personas
                .iter()
                .map(|(name, p)| (format!("personas.{name}.files"), &p.files)),
        );
        for (key, p) in &self.personas {
            p.validate(key)?;
        }
        for (key, list) in files {
            if let Some(f) = list
                .iter()
                .map(crate::context_files::ContextEntry::path)
                .find(|f| !(f.starts_with('/') || f.starts_with("~/")))
            {
                anyhow::bail!("{key} entry {f:?} must be an absolute path or start with ~/");
            }
        }
        self.validate_places()?;
        self.memory.validate()?;
        self.routing.validate()?;
        self.lsp.validate()?;
        self.sessions.validate()?;
        self.threads.validate()?;
        self.persona_import.validate()?;
        self.context.validate()?;
        self.cockpit.validate()?;
        self.routines.validate()?;
        self.watch_for.validate()?;
        self.initiative.validate()?;
        discord::ThreadBind::validate_all(&self.discord.thread)?;
        discord::digest_slots(&self.discord.digest_at)?;
        self.discord.calendar.validate()?;
        discord::validate_forum_channel(&self.discord.forum_channel)?;
        discord::validate_task_home(&self.discord.task_home)?;
        self.route_bench.validate()?;
        self.prefix.validate()?;
        self.notify.validate()?;
        self.projects.validate()?;
        self.cache.validate(self)?;
        self.people.validate()?;
        self.card.validate()?;
        self.ontology.validate()?;
        self.validate_data_floor()?;
        self.content_based_security.validate()?;
        self.privacy.validate()?;
        self.import.validate()?;
        self.validate_voice()?;
        self.validate_mcp_server()?;
        if let Some(name) = &self.context.default_persona {
            if !self.personas.contains_key(name) {
                anyhow::bail!(
                    "context.default_persona = {name:?} is not a persona: {}",
                    match self.personas.keys().cloned().collect::<Vec<_>>().join(", ") {
                        known if known.is_empty() => "no [personas.<name>] table is defined".into(),
                        known => format!("the personas are {known}"),
                    }
                );
            }
        }
        for (program, grant) in &self.broker.programs {
            if program.is_empty() || program.contains('/') || program.contains(char::is_whitespace)
            {
                anyhow::bail!(
                    "broker.programs.\"{program}\" must be a program's name as argv[0] gives it, \
                     found on PATH, with no '/'"
                );
            }
            if let Some(what) = crate::broker::launcher(program) {
                anyhow::bail!(
                    "broker.programs.{program}: a secret granted to a program reaches only that \
                     program, never one it can be made to run, and {program} is {what}, whose \
                     job is to run what the call names (theseus-txvt). Grant the secret to the \
                     program {program} would run, and have the job run that one directly"
                );
            }
            if grant.env.is_empty() && grant.aws_account.is_none() {
                anyhow::bail!(
                    "broker.programs.{program} grants nothing: name a variable and its [secrets] \
                     entry, as env = {{ GH_TOKEN = \"github_token\" }}, or an AWS account whose \
                     job session it gets, as aws_account = \"111122223333\""
                );
            }
            if let Some(id) = &grant.aws_account {
                if !self.aws.accounts.contains_key(id) {
                    anyhow::bail!(
                        "broker.programs.{program}.aws_account = {id:?} is not an account under \
                         [aws.accounts]"
                    );
                }
            }
            for (var, secret) in &grant.env {
                let name = var.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                    && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !name {
                    anyhow::bail!(
                        "broker.programs.{program}.env: {var:?} is not an environment variable's name"
                    );
                }
                if !self.secrets.contains_key(secret) {
                    anyhow::bail!(
                        "broker.programs.{program}.env.{var} = {secret:?} has no matching entry \
                         under [secrets]"
                    );
                }
            }
        }
        for name in self.broker.secrets.keys() {
            if !self.secrets.contains_key(name) {
                anyhow::bail!("broker.secrets.{name} has no matching entry under [secrets]");
            }
        }
        for (k, argv) in self
            .policy
            .allow_argv
            .iter()
            .chain(&self.policy.approve_argv)
            .enumerate()
        {
            if argv.is_empty() {
                anyhow::bail!("policy argv entry {k} is empty");
            }
        }
        let registry = theseus_tools::default_registry();
        let mcp_key = |k: &str| {
            let parts: Vec<&str> = k.split('/').collect();
            parts.len() <= 2 && parts.iter().all(|p| !p.is_empty())
        };
        // The core's own tools beside the toollets: the web's, a task's, a
        // wake's, and AWS's.
        let core_tools = || {
            crate::web::NAMES
                .into_iter()
                .chain(crate::task::NAMES)
                .chain(crate::task_graph::tools::NAMES)
                .chain(crate::wake::NAMES)
                .chain(crate::routine_tool::NAMES)
                .chain(crate::project_tool::NAMES)
                .chain(crate::glide::NAMES)
                .chain(crate::extend::NAMES)
                .chain(crate::aws::NAMES)
                .chain(crate::google::NAMES)
                .chain(crate::term::NAMES)
                .chain(crate::lsp::NAMES)
                .chain(crate::memory_lookup::NAMES)
                .chain(crate::thread_read::NAMES)
                .chain(crate::rsi::pr::NAMES)
                .chain(crate::rsi::evidence::NAMES)
        };
        for name in self.policy.tools.keys() {
            let known = match name.strip_prefix(crate::policy::MCP_PREFIX) {
                Some(rest) => rest.contains('/') && mcp_key(rest),
                None => registry.get(name).is_some() || core_tools().any(|t| t == name),
            };
            if !known {
                let names: Vec<&str> = registry
                    .all()
                    .map(|t| t.name())
                    .chain(core_tools())
                    .collect();
                anyhow::bail!(
                    "policy.tools.\"{name}\" is not a tool (the tools: {}; an MCP tool is \"mcp:<server>/<tool>\")",
                    names.join(", ")
                );
            }
        }
        for key in self.policy.mcp.keys() {
            if !mcp_key(key) {
                anyhow::bail!("policy.mcp.\"{key}\" must be \"server\" or \"server/tool\"");
            }
        }
        if !self.all_profiles().contains_key(&self.model.live) {
            anyhow::bail!(
                "model.live = {:?} is not the implicit \"default\" profile nor a key of [profiles]",
                self.model.live
            );
        }
        limits::check_dollars(&self.kernel)?;
        limits::check_job_wait(&self.tools)?;
        let builtin = crate::catalog::Catalog::builtin();
        for (id, row) in &self.catalog {
            for (key, price) in row.prices() {
                if price.is_some_and(|p| !p.is_finite() || p < 0.0) {
                    anyhow::bail!(
                        "catalog.\"{id}\".{key} must be a price in US dollars per million tokens, \
                         zero or more"
                    );
                }
            }
            if let Some(l) = &row.long_prompt {
                let prices = [
                    l.input_per_mtok,
                    l.output_per_mtok,
                    l.cache_read_per_mtok,
                    l.cache_write_per_mtok,
                    l.cache_write_1h_per_mtok,
                ];
                if l.above_tokens == 0 || prices.iter().any(|p| !p.is_finite() || *p < 0.0) {
                    anyhow::bail!(
                        "catalog.\"{id}\".long_prompt needs above_tokens above zero and its five \
                         prices in US dollars per million tokens, zero or more"
                    );
                }
            }
            if row
                .origin
                .is_some_and(|o| o != crate::catalog::ModelOrigin::Cn)
                && builtin
                    .get(id)
                    .is_some_and(|b| b.origin == crate::catalog::ModelOrigin::Cn)
            {
                anyhow::bail!(
                    "catalog.\"{id}\".origin: a China-origin model's origin is not configurable"
                );
            }
            if let Err(missing) = row.over(builtin.get(id)) {
                anyhow::bail!(
                    "catalog.\"{id}\" is not a built-in model, so its table needs {}",
                    missing.join(", ")
                );
            }
        }
        // A refusal's fallback is another priced model of the same provider
        // (theseus-7gir.18), so its call reserves and settles as any does.
        let catalog = crate::catalog::Catalog::with_overrides(&self.catalog);
        for (id, e) in &catalog.entries {
            let Some(to) = &e.refusal_fallback_model else {
                continue;
            };
            if to == id || catalog.get(to).is_none_or(|f| f.provider != e.provider) {
                anyhow::bail!(
                    "catalog.\"{id}\".refusal_fallback_model = {to:?} must name another model of \
                     the catalog that {} serves",
                    e.provider
                );
            }
        }
        // The web UI has no auth yet (§3.14): whoever reaches it is the
        // operator, so it listens on loopback only (theseus-2fo).
        let bind = &self.web.bind;
        if !bind
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
        {
            anyhow::bail!(
                "web.bind = {bind:?} is not a loopback address: the web UI has no auth yet \
                 (spec §3.14), so it listens only on 127.0.0.1 or ::1"
            );
        }
        // The dev page's origin is served beside the UI's own: it must be a
        // page on this machine (theseus-zab).
        if let Some(o) = &self.web.dev_origin {
            if !dev_origin_ok(o) {
                anyhow::bail!(
                    "web.dev_origin = {o:?} is not a dev page's origin on this machine: it must be \
                     http://, then localhost or a loopback address, then a port, and nothing more \
                     (for the Vite dev server, http://localhost:5173)"
                );
            }
        }
        self.validate_aws()?;
        self.validate_google()?;
        self.validate_mcp()
    }

    /// Deserialize a document and resolve its implicit profile and provider.
    fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        let mut cfg: Config = toml::from_str(text).map_err(|e| match renamed::explain(text) {
            Some(said) => <toml::de::Error as serde::de::Error>::custom(said),
            None => e,
        })?;
        let m = &cfg.model;
        let doc: toml::Table = text.parse()?;
        let mut profiles = inherit::profiles(&doc)?;
        profiles
            .entry("default".into())
            .or_insert_with(|| ProfileConfig {
                provider: m.provider.clone(),
                model: m.model.clone(),
                max_output_tokens: m.max_output_tokens,
                system: m.system.clone(),
                effort: m.effort,
                effort_fixed: false,
                thinking_display: m.thinking_display,
                max_loops: m.max_loops,
                on_max_loops: m.on_max_loops,
                refusal_fallbacks: m.refusal_fallbacks,
                cache_ttl: Some(cfg.cache.ttl),
                keep_warm_hours: Some(cfg.cache.keep_warm_hours),
            });
        cache::inherit(&mut profiles, &cfg.cache);
        let mut providers = cfg.providers.clone();
        providers
            .entry("anthropic".into())
            .or_insert_with(|| ProviderConfig {
                api_base: m.api_base.clone(),
                api_key_secret: m.api_key_secret.clone(),
                kind: ProviderKind::AnthropicMessages,
                timeouts: None,
                origin: None,
            });
        cfg.resolved = Resolved {
            profiles,
            providers,
        };
        // An `enabled` the config leaves out is settled by the Jev key's
        // entry (theseus-is8t).
        let secrets = cfg.secrets.clone();
        cfg.judge.resolve(&secrets);
        cfg.cockpit.resolve(cfg.web.port, cfg.web.enabled);
        Ok(cfg)
    }

    /// A test's `[model]` served from a China-origin endpoint, after the
    /// providers were resolved (theseus-oejh3).
    #[cfg(test)]
    pub(crate) fn serve_default_from_cn(&mut self) {
        let p = self.resolved.providers.get_mut("anthropic");
        p.expect("the implicit provider").api_base = "https://api.z.ai/api/anthropic".into();
    }

    /// A test's `[model]` served from a host the operator vouches is of EU
    /// origin (theseus-xs5ah).
    #[cfg(test)]
    pub(crate) fn serve_default_from_eu(&mut self) {
        let p = self.resolved.providers.get_mut("anthropic");
        let p = p.expect("the implicit provider");
        p.api_base = "https://api.example.eu/anthropic".into();
        p.origin = Some(crate::catalog::ModelOrigin::Eu);
    }

    /// A test whose GLM profiles were only a second, cheaper model
    /// (theseus-a1wvq.3): the `zai` provider served from a host vouched `us`,
    /// and `glm` and `glm53` on Claude Haiku 5.5, so the data floor lets them
    /// run. The profiles' names, their provider and its fake stay.
    #[cfg(test)]
    pub(crate) fn serve_glm_unrestricted(&mut self) {
        let z = self
            .resolved
            .providers
            .get_mut("zai")
            .expect("the zai provider");
        z.api_base = "https://llm-proxy.example.com/anthropic".into();
        z.origin = Some(crate::catalog::ModelOrigin::Us);
        for name in ["glm", "glm53"] {
            let p = self.resolved.profiles.get_mut(name).expect("the profile");
            p.model = UNRESTRICTED_SECOND.into();
        }
    }

    /// A test's profile `name`, a copy of `like` (theseus-a1wvq.8's
    /// `dsflash`, GLM's model on the `zai` fake).
    #[cfg(test)]
    pub(crate) fn copy_profile(&mut self, name: &str, like: &str) {
        let p = self.resolved.profiles[like].clone();
        self.resolved.profiles.insert(name.into(), p);
    }

    /// A test's `dsflash` profile: Flash on the `deepseek` provider, whose key
    /// is a real-looking `op://` reference (the template's is a placeholder).
    #[cfg(test)]
    pub(crate) fn serve_deepseek_flash(&mut self) {
        self.copy_profile("dsflash", "sonnet");
        let p = self.resolved.profiles.get_mut("dsflash").expect("copied");
        p.provider = "deepseek".into();
        p.model = "deepseek-flash".into();
        self.name_every_provider_key();
    }

    /// A test's providers' keys named by real-looking `op://` references: the
    /// template's are placeholders, and `provider_without_key` reads a
    /// placeholder as no key, so a pick on one is a fallback. A test that
    /// expects a pick to run (a job's, a reviewer's) calls this.
    #[cfg(test)]
    pub(crate) fn name_every_provider_key(&mut self) {
        for k in ["anthropic_api_key", "zai_api_key", "deepseek_api_key"] {
            self.secrets
                .insert(k.into(), "op://test-vault/item/notesPlain".into());
        }
    }

    /// A test's config without profile `name`.
    #[cfg(test)]
    pub(crate) fn remove_profile(&mut self, name: &str) {
        self.resolved.profiles.remove(name);
    }

    /// A test's `glm` profile on `model` (US origin, on the `zai` fake), a
    /// second provider with a model of its own (theseus-a1wvq.10).
    #[cfg(test)]
    pub(crate) fn serve_glm_as(&mut self, model: &str) {
        self.serve_glm_unrestricted();
        self.resolved
            .profiles
            .get_mut("glm")
            .expect("the profile")
            .model = model.into();
    }

    /// A test's `zai` provider vouched of `origin`.
    #[cfg(test)]
    pub(crate) fn vouch_zai(&mut self, origin: crate::catalog::ModelOrigin) {
        let z = self
            .resolved
            .providers
            .get_mut("zai")
            .expect("the zai provider");
        z.origin = Some(origin);
    }

    /// Every profile by name, with the implicit `default` synthesized from
    /// `[model]` unless `[profiles.default]` overrides it.
    pub fn all_profiles(&self) -> &BTreeMap<String, ProfileConfig> {
        &self.resolved.profiles
    }

    /// A profile by name, or the error that names every configured one.
    pub fn profile(&self, name: &str) -> anyhow::Result<&ProfileConfig> {
        self.resolved.profiles.get(name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown profile {name:?}; configured: {}",
                self.resolved
                    .profiles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    }

    /// The context files a session gets while `persona` is in play, in the
    /// order its system carries them: the system level, then the
    /// persona's. `engaging`: a message here can engage another persona
    /// (`TurnRunner::engages`), so every live persona's soul is carried.
    pub fn context_paths(
        &self,
        book: &crate::personas::Book,
        persona: Option<&str>,
        engaging: bool,
    ) -> Vec<crate::context_files::ContextPath> {
        use crate::context_files::ContextPath;
        let system = self.context.files.iter().map(|p| ContextPath {
            path: p.path().into(),
            persona: None,
            public: p.readers() == crate::context_files::ContextReaders::Public,
        });
        // Where a message can engage another persona, every persona's soul
        // is in the request (the persona layer), so a message's persona is
        // a line in its tail and a switch changes no byte of the prefix
        // (theseus-c1uz). Elsewhere (a cli, task or headless session, whose
        // persona is fixed) only the persona in play is carried, and a change
        // of it is a `system_changed` recompile (theseus-nsp9l.1).
        let own = book
            .live()
            .filter(|(name, _)| engaging || Some(name.as_str()) == persona)
            .flat_map(|(name, p)| {
                p.files.iter().map(|f| ContextPath {
                    path: f.path().into(),
                    persona: Some(name.clone()),
                    public: f.readers() == crate::context_files::ContextReaders::Public,
                })
            });
        system.chain(own).collect()
    }

    /// A profile's model origin (theseus-a1wvq): its provider's endpoint, its
    /// model and the catalog's row for it, by [`crate::catalog::origin_of`].
    /// A profile or provider that does not resolve is `unknown`.
    pub fn profile_origin(
        &self,
        profile: &str,
        catalog: &crate::catalog::Catalog,
    ) -> crate::catalog::ModelOrigin {
        let Some(p) = self.all_profiles().get(profile) else {
            return crate::catalog::ModelOrigin::Unknown;
        };
        let Some(prov) = self.all_providers().get(&p.provider) else {
            return crate::catalog::ModelOrigin::Unknown;
        };
        crate::catalog::origin_of(&prov.api_base, prov.origin, &p.model, catalog.get(&p.model))
    }

    /// Whether a profile may be sent private data (theseus-xs5ah), by
    /// [`crate::catalog::cleared_for_sensitive`]; a profile or provider that
    /// does not resolve is not.
    pub fn profile_cleared_for_sensitive(
        &self,
        profile: &str,
        catalog: &crate::catalog::Catalog,
    ) -> bool {
        let Some(p) = self.all_profiles().get(profile) else {
            return false;
        };
        let Some(prov) = self.all_providers().get(&p.provider) else {
            return false;
        };
        crate::catalog::cleared_for_sensitive(
            &prov.api_base,
            prov.origin,
            &p.model,
            catalog.get(&p.model),
        )
    }

    /// Every provider by name, with the implicit `anthropic` one synthesized
    /// from `[model]` unless `[providers.anthropic]` overrides it.
    pub fn all_providers(&self) -> &BTreeMap<String, ProviderConfig> {
        &self.resolved.providers
    }

    /// Why a pick on `provider` cannot run for want of a key, in the words
    /// `theseus why` prints (`deepseek: no key`), or `None` when the provider
    /// is declared and its secret names a reference that could resolve. A
    /// reference still holding the template's placeholder (`<your vault>`) names
    /// no item and is no key: a pick on it is a fallback at pick time, not a call
    /// that fails at dispatch. A structural check only: no vault is asked, and
    /// whether a real reference resolves is the secret board's (`route_profiles`
    /// reads its state). The words carry no reference.
    pub fn provider_without_key(&self, provider: &str) -> Option<String> {
        let named = |secret: &str| {
            self.secrets
                .get(secret)
                .is_some_and(|r| !r.contains(['<', '>']))
        };
        match self.all_providers().get(provider) {
            Some(p) if named(&p.api_key_secret) => None,
            _ => Some(format!("{provider}: no key")),
        }
    }

    pub fn state_dir(&self) -> PathBuf {
        expand(&self.server.state_dir)
    }
    pub fn socket_path(&self) -> PathBuf {
        expand(&self.server.socket)
    }

    /// The annotated configuration template, with every parameter documented.
    /// Tested to parse, validate, and to parse with every comment un-commented.
    pub const EXAMPLE_TOML: &'static str = include_str!("../config/theseus.example.toml");

    /// The template, parsed. `example-config` prints `EXAMPLE_TOML` itself so
    /// the comments survive.
    pub fn example() -> Self {
        Self::from_toml(Self::EXAMPLE_TOML).expect("the bundled example config parses")
    }
}

/// The model a test's GLM profiles run once `serve_glm_unrestricted` moves
/// them.
#[cfg(test)]
pub(crate) const UNRESTRICTED_SECOND: &str = "claude-haiku-5-5";

/// A configured path with `~` and environment variables read as a shell
/// reads them (review 2's consideration 5, in place of `shellexpand`, whose
/// dependencies are MPL-2.0).
pub fn expand(p: &str) -> PathBuf {
    PathBuf::from(expand_with(p, |name| std::env::var(name).ok()))
}

/// A command's path argument with a leading `~` as `$HOME`, and nothing
/// else, as `shellexpand::tilde` read it for the gate. The arguments reach
/// the program unexpanded (no shell), so a `$VAR` stays as written: the
/// relative path the program will open.
pub fn expand_home(p: &str) -> PathBuf {
    let (home, rest) = tilde(p, || std::env::var("HOME").ok());
    PathBuf::from(home + rest)
}

/// `p`'s leading `~` (alone, or before `/`) as `home`, and the rest. Without
/// a home, or before a user's name (`~user`), it is left as written.
fn tilde(p: &str, home: impl FnOnce() -> Option<String>) -> (String, &str) {
    if let Some(after) = p.strip_prefix('~') {
        if after.is_empty() || after.starts_with('/') {
            if let Some(h) = home().filter(|h| !h.is_empty()) {
                return (h, after);
            }
        }
    }
    (String::new(), p)
}

/// `p` with a leading `~` (alone, or before `/`) as `$HOME`, and each `$NAME`
/// or `${NAME}` as that variable's value, from `var`. What does not resolve
/// is left as written: a variable that is not set, `~user`, a `$` before
/// anything but a name, and `~` when `HOME` is not set.
pub fn expand_with(p: &str, var: impl Fn(&str) -> Option<String>) -> String {
    let (mut out, mut rest) = tilde(p, || var("HOME"));
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, len) = match after.strip_prefix('{') {
            Some(b) => match b.find('}') {
                Some(end) => (&b[..end], end + 2),
                None => ("", 0),
            },
            None => {
                let end = after.find(|c| !name_char(c)).unwrap_or(after.len());
                (&after[..end], end)
            }
        };
        let named = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(name_char);
        match named.then(|| var(name)).flatten() {
            Some(value) => out.push_str(&value),
            None => out.push_str(&rest[at..at + 1 + len]),
        }
        rest = &after[len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_card;
#[cfg(test)]
mod tests_content_based_security;
#[cfg(test)]
mod tests_deepseek;
#[cfg(test)]
mod tests_fresh;
#[cfg(test)]
mod tests_google;
#[cfg(test)]
mod tests_inherit;
#[cfg(test)]
mod tests_moonshot;
#[cfg(test)]
mod tests_ontology;
#[cfg(test)]
mod tests_persona;
