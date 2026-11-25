//! Attach an MCP server by asking, and take it away again (issue 22, line 4;
//! `docs/design/mcp-attach.md`).
//!
//! The owner names a server in a private place: a command to run, or an
//! address. `mcp.attach` then, in this order:
//!
//! 1. refuses unless the caller is the owner (`wall.rs`). Attaching can give a
//!    server secrets and a network, so its card is always marked as needing
//!    the second proof (`proof_seam.rs`): a code, or an open window of the
//!    same place. Nothing has started by then;
//! 2. **tries the server once**, in the sandbox (L1, whatever the owner asked
//!    for it to run in), with no secret and no network beyond the hosts asked
//!    for, and lists its tools. A server over HTTP is asked for its list
//!    without its key. A trial that fails is shown, and the owner may still
//!    say yes;
//! 3. builds the candidate with [`super::edit`] (one new table,
//!    `[mcp.servers.<name>]`) and loads it exactly as a start loads it, and
//!    puts **one card** to the owner built from the table as it will be
//!    written (`card.rs`), never from the model's words;
//! 4. on the owner's yes, the daemon writes the table through
//!    `config_change`'s own write (one change, the previous copy kept,
//!    `config.changed`) and **loads the server live** (`live.rs`): its tools are
//!    offered from the next turn, with no restart.
//!
//! `mcp.detach` is the same road backwards: a card, the daemon removes the
//! table, and the server is unloaded live; one the daemon started from the
//! file at its start stops at a restart, which the card says beforehand.
//!
//! The yes is `config.ack`, as `config.propose`'s: the question's arguments
//! carry an `mcp` marker, which `answer.rs` checks against the settings that
//! really change. A secret's value is never here: a table names a `[secrets]`
//! entry, and the trial is given none.

pub mod card;
pub mod live;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::Proposal;
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::{diff, edit, proof_seam, wall, write, ACK, RESTART_WAIT_SECS};
use card::Facts;

use crate::config::mcp::{server_name_ok, McpSandbox};
use crate::config::McpServerConfig;
use crate::mcp::McpBoard;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub const ATTACH: &str = "mcp.attach";
pub const DETACH: &str = "mcp.detach";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 2] = [ATTACH, DETACH];
/// The longest a trial takes: its start, and a little over for the list.
pub const TRIAL_SECS: u64 = 30;
/// The most arguments a command may have, and the longest one.
const MAX_ARGS: usize = 64;
const MAX_ARG_BYTES: usize = 4_096;
/// The most tools, secrets or hosts a proposal names.
const MAX_LIST: usize = 64;

/// Whether `tool` is run by this module's async functions.
pub fn is_async(tool: &str) -> bool {
    NAMES.contains(&tool) || tool == super::vault_use::USE || tool == super::vault_keep::KEEP
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachInput {
    name: String,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    auth_secret: Option<String>,
    #[serde(default)]
    read: Vec<String>,
    #[serde(default)]
    sandbox: Option<String>,
    #[serde(default)]
    egress: Vec<String>,
    #[serde(default)]
    external: Option<bool>,
    #[serde(default)]
    reason: Option<String>,
    /// Where the model found the server's command or address (`source.rs`).
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetachInput {
    name: String,
    #[serde(default)]
    reason: Option<String>,
}

/// What an attach asks for, checked: the table to write, the server as the
/// owner asked for it (shown when the settings cannot load it yet), and the
/// server as the trial runs it.
struct Spec {
    name: String,
    table: Value,
    wanted: McpServerConfig,
    trial: McpServerConfig,
    reason: String,
    source: Option<String>,
}

/// What an attach names, checked before anything is built: one transport, and
/// only the settings that transport takes.
fn check(i: &AttachInput) -> Result<(), String> {
    match (i.command.is_empty(), &i.url) {
        (true, None) => {
            return Err(
                "name what to attach: `command` (a program and its arguments, as a list) or `url`"
                    .into(),
            )
        }
        (false, Some(_)) => {
            return Err("a server has one transport: `command` or `url`, not both".into())
        }
        (false, None) => {
            if i.command.len() > MAX_ARGS || i.command.iter().any(|a| a.len() > MAX_ARG_BYTES) {
                return Err(format!(
                    "a command has at most {MAX_ARGS} arguments of {MAX_ARG_BYTES} bytes"
                ));
            }
            if i.command[0].trim().is_empty() {
                return Err("the command names no program".into());
            }
            if i.auth_secret.is_some() {
                return Err(
                    "`auth_secret` is for a server over HTTP; a program gets its secrets in `env`"
                        .into(),
                );
            }
        }
        (true, Some(u)) => {
            if !(u.starts_with("https://") || u.starts_with("http://")) {
                return Err(format!("{u:?} is not an http:// or https:// address"));
            }
            let authority = u.split_once("://").map_or("", |(_, r)| r);
            if authority
                .split(['/', '?', '#'])
                .next()
                .is_some_and(|a| a.contains('@'))
            {
                return Err(
                    "the address carries a name or key before an `@`: a key is a secret, \
                            so name it in `auth_secret` and leave it out of the address"
                        .into(),
                );
            }
            if !i.env.is_empty() {
                return Err("`env` is for a program Theseus starts; a server over HTTP gets its key from `auth_secret`".into());
            }
            if i.sandbox.as_deref() == Some("l1") || !i.egress.is_empty() {
                return Err("`sandbox` and `egress` are for a program Theseus starts; a server over HTTP runs elsewhere".into());
            }
        }
    }
    if [i.env.len(), i.read.len(), i.egress.len()]
        .iter()
        .any(|n| *n > MAX_LIST)
    {
        return Err(format!(
            "name at most {MAX_LIST} secrets, read-only tools or hosts"
        ));
    }
    Ok(())
}

/// The table to write, from the server as the owner asked for it.
fn table_of(wanted: &McpServerConfig) -> Value {
    let mut table = serde_json::Map::new();
    match &wanted.url {
        Some(u) => {
            table.insert("url".into(), json!(u));
            if let Some(s) = &wanted.auth_secret {
                table.insert("auth_secret".into(), json!(s));
            }
        }
        None => {
            table.insert("command".into(), json!(wanted.command));
            if !wanted.env.is_empty() {
                table.insert("env".into(), json!(wanted.env));
            }
            table.insert(
                "sandbox".into(),
                json!(match wanted.sandbox {
                    McpSandbox::L1 => "l1",
                    McpSandbox::L0 => "l0",
                }),
            );
            if !wanted.egress.is_empty() {
                table.insert("egress".into(), json!(wanted.egress));
            }
        }
    }
    if !wanted.read.is_empty() {
        table.insert("read".into(), json!(wanted.read));
    }
    if !wanted.external {
        table.insert("external".into(), json!(false));
    }
    Value::Object(table)
}

impl Spec {
    fn of(i: &AttachInput) -> Result<Self, String> {
        if !server_name_ok(&i.name) || i.name.starts_with(crate::extend::SERVER_PREFIX) {
            return Err(format!(
                "{:?} is not a server's name: letters, digits, `_` and `-`, no `__`, at most 32 \
                 characters, and not starting with `{}`",
                i.name,
                crate::extend::SERVER_PREFIX
            ));
        }
        check(i)?;
        let sandbox = match (i.sandbox.as_deref(), i.url.is_some()) {
            (Some("l0"), _) | (None, true) => McpSandbox::L0,
            (Some("l1"), _) | (None, false) => McpSandbox::L1,
            (Some(other), _) => return Err(format!("sandbox is `l1` or `l0`, not {other:?}")),
        };
        let wanted = McpServerConfig {
            command: i.command.clone(),
            env: i.env.clone(),
            url: i.url.clone(),
            auth_secret: i.auth_secret.clone(),
            read: i.read.clone(),
            sandbox,
            egress: i.egress.clone(),
            frozen: None,
            external: i.external.unwrap_or(true),
            enabled: true,
            start_timeout_secs: TRIAL_SECS,
            call_timeout_secs: 110,
        };
        // What will run must be shown whole on the card, or there is no card.
        card::fits(&wanted)?;
        // The trial: in the sandbox whatever the owner asked for, with the
        // hosts asked for only when the server will run in the sandbox, and no
        // secret at all.
        let trial = McpServerConfig {
            env: BTreeMap::new(),
            auth_secret: None,
            read: Vec::new(),
            sandbox: match wanted.url {
                Some(_) => McpSandbox::L0,
                None => McpSandbox::L1,
            },
            egress: match wanted.sandbox {
                McpSandbox::L1 => wanted.egress.clone(),
                McpSandbox::L0 => Vec::new(),
            },
            ..wanted.clone()
        };
        Ok(Spec {
            name: i.name.clone(),
            table: table_of(&wanted),
            wanted,
            trial,
            reason: super::card::reason_line(i.reason.as_deref().unwrap_or_default()),
            source: super::source::clean(i.source.as_deref()),
        })
    }

    fn path(&self) -> Vec<String> {
        server_path(&self.name)
    }
}

/// `mcp.servers.<name>` as a path.
pub(crate) fn server_path(name: &str) -> Vec<String> {
    vec!["mcp".into(), "servers".into(), name.into()]
}

/// `mcp.attach`.
pub struct Attach;

impl Tool for Attach {
    fn name(&self) -> &str {
        ATTACH
    }

    fn deadline(&self) -> Option<Duration> {
        Some(Duration::from_secs(TRIAL_SECS + 30))
    }

    fn description(&self) -> &str {
        "Attach an MCP server the owner names: a program to run (`command`, the program and each \
         argument as a list, never a shell line) or an address (`url`). Theseus tries it once in \
         the sandbox with no secret, lists its tools, and shows the owner one card with what will \
         run, each tool, which tools are read-only (only those you name in `read`), which secrets \
         it is given by name, and which hosts it may reach. Only the owner's yes attaches it, \
         with a second proof, and its tools are offered from the next message with no restart. \
         A secret a server needs is named by its `[secrets]` entry (`env`: variable to secret \
         name; `auth_secret` for HTTP): never a value. A command or address its maker publishes may be \
         looked up with the web tools rather than asked for: give it with a `source`, never a guess. \
         Offered only to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "What to call the server: letters, digits, `_` and `-`."},
                "command": {"type": "array", "items": {"type": "string"}, "description": "A program Theseus starts, over stdio: the program, then each argument as its own entry."},
                "url": {"type": "string", "description": "Instead of command: the address of a server over HTTP."},
                "env": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Environment variable to the NAME of a [secrets] entry. Never a value."},
                "auth_secret": {"type": "string", "description": "For url: the NAME of the [secrets] entry that is its key."},
                "read": {"type": "array", "items": {"type": "string"}, "description": "The tools the owner says are read-only. Every other tool asks first."},
                "sandbox": {"type": "string", "enum": ["l1", "l0"], "description": "l1 (the default) runs it in the sandbox; l0 outside it, only on the owner's word."},
                "egress": {"type": "array", "items": {"type": "string"}, "description": "Hosts (host:port) a sandboxed server may reach. Empty: no network."},
                "external": {"type": "boolean", "description": "true (the default): its results are marked as outside text. false only on the owner's explicit word: the card says so on a line of its own."},
                "reason": {"type": "string", "description": "One line, in the owner's words: why they want it."},
                "source": {"type": "string", "description": "For a published command or address: a page's address and what it says, one line. The card shows it as your word, not a check."}
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a restart, it answers with what it asked.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: AttachInput = parse(input)?;
        Spec::of(&i)?;
        Ok(Plan {
            summary: format!("propose attaching the MCP server {}", i.name),
            ..Default::default()
        })
    }
}

/// `mcp.detach`.
pub struct Detach;

impl Tool for Detach {
    fn name(&self) -> &str {
        DETACH
    }

    fn description(&self) -> &str {
        "Take away an MCP server that is attached, on the owner's word. Shows the owner one card \
         with what stops and when; only their yes, with a second proof, removes it, and a server \
         that was attached by asking stops at once. To change a server, detach it and attach it \
         again. Offered only to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The server's name."},
                "reason": {"type": "string", "description": "One line, in the owner's words: why they want it gone."}
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: DetachInput = parse(input)?;
        Ok(Plan {
            summary: format!("propose removing the MCP server {}", i.name),
            ..Default::default()
        })
    }
}

/// The wall's words for this tool: the same wall, named for it, and one wall
/// more: a message that came through an agent's process (Claude Code, Codex,
/// anything `peer_proc` names) is not the owner's own word for this.
fn refusal(tc: &TurnCtx<'_>, tool: &str) -> Option<String> {
    if let Some(why) = wall::refusal(tc) {
        return Some(why.replacen("config.propose", tool, 1));
    }
    // The agent behind this turn's input: on the sink, and, for a turn that
    // goes on after an approval or a wait, as the input path kept it.
    let kept = tc
        .store
        .get_meta::<String>(&crate::rpc::agent_input_key(tc.session_id))
        .ok()
        .flatten();
    tc.sink.agent.clone().or(kept).as_ref().map(|agent| {
        format!(
            "{tool} is the owner's own act, and this message came through an agent's process \
             ({agent}): it is not offered. The owner asks in their own words, from a DM, their \
             own terminal or the web UI"
        )
    })
}

/// Whether `a` is a card of this module: a `config.ack` question that carries
/// an `mcp` marker.
pub(crate) fn is_card(a: &theseus_kernel::Action) -> bool {
    a.proposal
        .as_ref()
        .is_some_and(|p| is_card_of(&a.tool, &p.args))
}

/// [`is_card`] for a question as `confirm.list` shows it.
pub(crate) fn is_card_of(tool: &str, input: &Value) -> bool {
    tool == ACK && input.get("mcp").is_some()
}

/// Who asked, as the answer's ledger row records it.
fn asked_by(tc: &TurnCtx<'_>) -> Value {
    json!({
        "session_id": tc.session_id,
        "execution_id": tc.execution_id,
        "turn_id": tc.turn_id,
        "place": tc.outbox.target(tc.session_id),
    })
}

/// Whether the turn has read outside text, or is steered by it: one flag for
/// the card's line.
fn outside_text(tc: &TurnCtx<'_>) -> bool {
    let (outside, steered) = super::outside_text(tc);
    outside || steered
}

/// What the file and the running board say before anything is tried: the file
/// as it is, its settings, and where it is.
struct Ground {
    base: String,
    config: crate::Config,
    board: Arc<McpBoard>,
}

fn ground(rt: &ToolRuntime) -> Result<Ground, String> {
    let file = rt.config_change.file.clone().ok_or(
        "Theseus's settings are not a file this daemon can change (they are read from the vault, \
         or were not given as a file): the owner edits them where they live",
    )?;
    let base = std::fs::read_to_string(&file)
        .map_err(|e| format!("the settings file cannot be read: {e}"))?;
    let (config, _) = crate::Config::parse(&base).map_err(|e| {
        format!("the settings file as it is now does not load, so nothing is built on it: {e:#}")
    })?;
    let board = rt
        .extend
        .board()
        .ok_or("the MCP board is not running, so nothing can be tried")?;
    Ok(Ground {
        base,
        config,
        board,
    })
}

/// The server tried once: in the board's own trial, a task a cancel reaches.
async fn try_once(
    rt: &ToolRuntime,
    board: &Arc<McpBoard>,
    spec: &Spec,
    correlation_id: &str,
) -> Result<card::Tried, String> {
    let (b, name, cfg) = (
        board.clone(),
        format!("try-{}", spec.name),
        spec.trial.clone(),
    );
    let mut task = tokio::spawn(async move {
        match b.trial(&name, &cfg).await {
            Ok(t) => card::Tried {
                tools: t.tools.clone(),
                error: None,
            },
            Err(e) => card::Tried {
                tools: Vec::new(),
                error: Some(e),
            },
        }
    });
    let _stoppable = rt.stops.track(correlation_id, task.abort_handle());
    match tokio::time::timeout(Duration::from_secs(TRIAL_SECS + 10), &mut task).await {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(e)) if e.is_cancelled() => Err(crate::extend::STOPPED.into()),
        Ok(Err(e)) => Err(format!("the trial failed: {e}")),
        Err(_) => {
            task.abort();
            Err("the trial ran past its time".into())
        }
    }
}

/// The question's proposal and its record.
fn put(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    correlation_id: &str,
    (question, reason): (&str, &str),
    candidate: (&str, &str, &[diff::Setting]),
    restart: bool,
    marker: Value,
) -> Result<(String, Value), String> {
    let (base, digest, settings) = candidate;
    let proposal = Proposal {
        tool: ACK.into(),
        args: json!({
            "question": question,
            "settled": super::words::mcp(&marker, restart),
            "base_sha256": crate::config_copy::sha256(base),
            "candidate_sha256": digest,
            "paths": settings.iter().map(|s| edit::dotted(&s.path)).collect::<Vec<_>>(),
            "restart": restart,
            "reason": reason,
            "outside_text": outside_text(tc),
            "asked_by": asked_by(tc),
            "mcp": marker,
        }),
        resource: Some("config".into()),
        policy_context: json!({}),
    };
    let said = format!(
        "Asked the owner:\n{question}Nothing changes until they say yes, from a private place, \
         with the second proof; the daemon then writes the settings itself. If they decline, or \
         do not answer in time, nothing is written."
    );
    super::ask(
        rt,
        tc,
        proposal,
        correlation_id,
        &said,
        proof_seam::Needs::Proof,
    )?;
    Ok((
        said,
        json!({"settings": settings.len(), "candidate": digest}),
    ))
}

/// Settings that change anything but the server's own table are not this tool's.
fn only_the_server(name: &str, settings: &[diff::Setting]) -> Result<(), String> {
    let at = server_path(name);
    match !settings.is_empty() && settings.iter().all(|s| s.path.starts_with(&at)) {
        true => Ok(()),
        false => {
            Err("that would change more than the server's own table: edit it with settings".into())
        }
    }
}

/// Run `mcp.attach` for the call `correlation_id` of the turn `tc`. An error is
/// the result the model reads.
pub async fn attach(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = refusal(tc, ATTACH) {
        return Err(why);
    }
    // Run again after a restart, it answers with what the first run asked.
    if let Ok(done) = super::proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let i: AttachInput = parse(input)?;
    let spec = Spec::of(&i)?;
    let g = ground(rt)?;
    if g.config.mcp.servers.contains_key(&spec.name) || g.board.has_server(&spec.name) {
        return Err(format!(
            "an MCP server named {} is already attached: detach it first, then attach it again",
            spec.name
        ));
    }
    let secrets: Vec<&String> = spec.wanted.secrets().collect();
    let missing: Vec<String> = secrets
        .iter()
        .filter(|s| !g.config.secrets.contains_key(**s))
        .map(|s| (*s).clone())
        .collect();
    let unloaded: Vec<String> = secrets
        .iter()
        .filter(|s| g.config.secrets.contains_key(**s) && !g.board.knows_secret(s))
        .map(|s| (*s).clone())
        .collect();
    let blocked = !missing.is_empty() || !unloaded.is_empty();
    // The candidate, before anything starts: one that does not load makes no
    // trial and no card.
    let built = match blocked {
        true => None,
        false => {
            let change = edit::Change::Set(spec.path(), spec.table.clone());
            let candidate = edit::apply(&g.base, &[change])?;
            let (loaded, _) = crate::Config::parse(&candidate)
                .map_err(|e| format!("the settings with that server do not load: {e:#}"))?;
            let server = loaded
                .mcp
                .servers
                .get(&spec.name)
                .cloned()
                .ok_or("the settings with that server do not hold it")?;
            let settings = diff::between(&g.base, &candidate)?;
            only_the_server(&spec.name, &settings)?;
            Some((candidate, server, settings))
        }
    };
    let tried = try_once(rt, &g.board, &spec, correlation_id).await?;
    if tried.error.is_none() {
        let list = crate::mcp::StoredList {
            digest: crate::mcp::digest(&tried.tools),
            tools: tried.tools.clone(),
        };
        g.board.keep_listed(&spec.name, list);
    } else {
        // An earlier trial's list is not this server's.
        g.board.take_listed(&spec.name);
    }
    let outside = outside_text(tc);
    let facts = |server| Facts {
        name: &spec.name,
        server,
        tried: &tried,
        outside_text: outside,
        reason: &spec.reason,
        missing: &missing,
        unloaded: &unloaded,
        source: spec.source.as_deref(),
    };
    let Some((candidate, server, settings)) = built else {
        return Ok((
            card::blocked(&facts(&spec.wanted))?,
            json!({"asked": false, "missing": missing, "unloaded": unloaded}),
        ));
    };
    let question = card::attach(&facts(&server))?;
    let digest = write::keep_candidate(&rt.config_change.state, &candidate)
        .map_err(|e| format!("the candidate could not be kept for the card: {e}"))?;
    let marker = json!({
        "op": "attach",
        "name": spec.name,
        "listed": tried.error.is_none(),
        "tools": tried.tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
    });
    put(
        rt,
        tc,
        correlation_id,
        (&question, &spec.reason),
        (&g.base, &digest, &settings),
        false,
        marker,
    )
}

/// Run `mcp.detach` for the call `correlation_id` of the turn `tc`.
pub fn detach(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = refusal(tc, DETACH) {
        return Err(why);
    }
    if let Ok(done) = super::proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let i: DetachInput = parse(input)?;
    let g = ground(rt)?;
    let Some(server) = g.config.mcp.servers.get(&i.name).cloned() else {
        return Err(format!(
            "no MCP server named {} is in the settings, so there is nothing to remove",
            i.name
        ));
    };
    let candidate = edit::apply(&g.base, &[edit::Change::Remove(server_path(&i.name))])?;
    crate::Config::parse(&candidate)
        .map_err(|e| format!("the settings without that server do not load: {e:#}"))?;
    let settings = diff::between(&g.base, &candidate)?;
    only_the_server(&i.name, &settings)?;
    // A server this daemon started from the file stops at a restart; one it
    // loaded by asking stops now.
    let restart = g.board.is_configured(&i.name);
    let reason = super::card::reason_line(i.reason.as_deref().unwrap_or_default());
    let question = card::detach(&card::Detach {
        name: &i.name,
        server: &server,
        tools: &g.board.offered_tools(&i.name),
        restart,
        wait_secs: RESTART_WAIT_SECS,
        outside_text: outside_text(tc),
        reason: &reason,
    });
    let digest = write::keep_candidate(&rt.config_change.state, &candidate)
        .map_err(|e| format!("the candidate could not be kept for the card: {e}"))?;
    let marker = json!({"op": "detach", "name": i.name});
    put(
        rt,
        tc,
        correlation_id,
        (&question, &reason),
        (&g.base, &digest, &settings),
        restart,
        marker,
    )
}

/// The yes's check, for a question that carries an `mcp` marker: the settings
/// that really change must be the server's own table and nothing else, only
/// added for an attach and only removed for a detach. `Ok(true)` when the
/// question is one of these, whose change is always sensitive.
pub(crate) fn at_yes(args: &Value, settings: &[diff::Setting]) -> Result<bool, String> {
    let Some(m) = args.get("mcp") else {
        return Ok(false);
    };
    let name = m["name"].as_str().unwrap_or_default();
    only_the_server(name, settings)?;
    let held = match m["op"].as_str() {
        Some("attach") => settings
            .iter()
            .all(|s| s.before.is_none() && s.after.is_some()),
        Some("detach") => settings
            .iter()
            .all(|s| s.after.is_none() && s.before.is_some()),
        _ => false,
    };
    match held {
        true => Ok(true),
        false => {
            Err("the change is no longer the one the card showed, so nothing was written".into())
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_proof;
#[cfg(test)]
mod tests_told;
