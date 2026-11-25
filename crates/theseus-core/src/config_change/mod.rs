//! The owner asks Theseus to change a setting, and the daemon writes it
//! (issue 22; `docs/design/config-change.md` has the whole design).
//!
//! The config file stays on the floor (`toolrun/guarded.rs`): no tool writes
//! it, and this adds none. What it adds is a way for the DAEMON to change it,
//! in its own code, after the owner has seen exactly what changes and said yes.
//!
//! 1. **The tool** (`config.propose`, here): the owner's own turn names
//!    settings and new values (or a removal) and a one-line reason. Never
//!    text to write. It changes nothing by itself. Who may call it is
//!    `wall.rs`: the owner, alone in a private place, in their own session.
//! 2. **The candidate** (`edit.rs`): the file as it is on disk now with the
//!    named changes made on its own text, comments and order kept, then
//!    loaded and checked exactly as a start loads it (`Config::parse`). One
//!    that does not load makes no card; the loader's words are the result.
//! 3. **The card** (`diff.rs`, `card.rs`): built from the two parsed configs,
//!    each setting's value before and after in plain words, whether it needs
//!    a restart, and, when the turn has read outside text, a line that says
//!    so. The model's only words on it are its one-line reason, shown as its.
//! 4. **The yes** (`answer.rs`): `action.confirm`, judged by the place rule,
//!    on a question of its own (`config.ack`), as an extension's ack is. The
//!    daemon re-reads the file; one that changed since the card was made is
//!    refused and nothing is written. Otherwise it keeps the previous text as
//!    a numbered copy and writes the candidate in one step (`write.rs`).
//! 5. **The sensitive list** (`config/sensitivity.rs`): a change to a setting
//!    on it is refused before a card is made, until the second proof is built
//!    in (`proof_seam.rs`).
//! 6. **The history and the undo** (`history.rs`, `undo.rs`;
//!    `docs/design/config-undo.md`): `config.history` lists the last changes
//!    from the ledger and the kept copies, and `config.undo` puts one change's
//!    settings back through `put`, the same path a proposal takes.

pub mod answer;
pub mod card;
pub mod diff;
pub mod edit;
pub mod history;
pub mod long_value;
pub mod mcp_attach;
pub mod prompt_file;
pub mod proof_seam;
pub mod restart;
pub mod show;
pub mod shown;
pub mod source;
pub mod undo;
pub mod vault_keep;
pub mod vault_use;
pub mod waiting;
pub mod wall;
pub mod whole;
pub mod words;
pub mod write;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{Action, Proposal, RetryClass};
use theseus_protocol::ConfirmRequest;
use theseus_store::{kinds, NewRecord};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::config::sensitivity;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub const PROPOSE: &str = "config.propose";
/// The owner's plain word: restart Theseus, with no change to its settings.
pub const RESTART: &str = "daemon.restart";
/// The question the owner's yes answers: a planned action of this tool.
pub const ACK: &str = "config.ack";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 7] = [
    PROPOSE,
    RESTART,
    history::HISTORY,
    undo::UNDO,
    show::SHOW,
    vault_use::USE,
    vault_keep::KEEP,
];
/// The most settings one proposal may name.
pub const MAX_CHANGES: usize = 20;
/// The longest a value a proposal gives may be, as JSON: a setting, not a
/// document.
pub const MAX_VALUE_BYTES: usize = 4_096;
/// How long a restart waits for a running turn to end before it cuts it, in
/// seconds. Said on the card.
pub const RESTART_WAIT_SECS: u64 = 120;
/// The key under which a proposal's result is kept, so a call run again after
/// a restart finds what the first run asked.
const CALL_PREFIX: &str = "config.change.call.";

/// Where the config file is, and the state directory the copies are kept in.
/// `file` is none when the daemon's config is not a file it can write (a vault
/// note, or a test): then nothing can be proposed. The core, held weakly once
/// built, is what a restart that waits for a turn is spawned from.
#[derive(Debug, Clone, Default)]
pub struct Where {
    pub file: Option<PathBuf>,
    pub state: PathBuf,
    core: Arc<OnceLock<Weak<crate::rpc::Core>>>,
    /// Held across a yes's re-read, keep and write, so two yeses never both
    /// pass the digest check before either writes.
    pub(crate) writes: Arc<std::sync::Mutex<()>>,
}

impl Where {
    pub fn new(file: Option<PathBuf>, state: PathBuf) -> Self {
        Self {
            file,
            state,
            core: Arc::default(),
            writes: Arc::default(),
        }
    }

    /// The core, once built: held weakly, as every board holds it.
    pub fn attach(&self, core: &Arc<crate::rpc::Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    pub(crate) fn core(&self) -> Option<Arc<crate::rpc::Core>> {
        self.core.get().and_then(Weak::upgrade)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    path: String,
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    remove: bool,
    /// Where the model found a fact its maker publishes (`source.rs`).
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    changes: Vec<Item>,
    reason: String,
}

fn input_of(input: &Value) -> Result<(Vec<edit::Change>, Vec<source::Sourced>, String), String> {
    let i: Input = parse(input)?;
    if i.changes.is_empty() || i.changes.len() > MAX_CHANGES {
        return Err(format!(
            "name between 1 and {MAX_CHANGES} settings to change"
        ));
    }
    let reason = card::reason_line(&i.reason);
    if reason.is_empty() {
        return Err("give a reason in the owner's words, one line".into());
    }
    let (changes, sources) = changes_of(i.changes)?;
    Ok((changes, sources, reason))
}

/// The edits a list of items names, each checked as a proposal checks it, and
/// the sources the model gave for them.
fn changes_of(items: Vec<Item>) -> Result<(Vec<edit::Change>, Vec<source::Sourced>), String> {
    let mut changes = Vec::new();
    let mut sources = Vec::new();
    for c in items {
        let path = edit::parse_path(&c.path)?;
        sources.extend(source::of(&path, c.source.as_deref()));
        changes.push(match (c.value, c.remove) {
            (Some(v), false) => {
                if v.to_string().len() > MAX_VALUE_BYTES {
                    return Err(format!(
                        "the value of {} is longer than a card can show whole, and larger than a \
                         setting ({MAX_VALUE_BYTES} bytes): name the setting, not a document",
                        c.path
                    ));
                }
                edit::Change::Set(path, v)
            }
            (None, true) => edit::Change::Remove(path),
            _ => {
                return Err(format!(
                    "{} needs either a value or remove = true, and not both",
                    c.path
                ))
            }
        });
    }
    Ok((changes, sources))
}

/// Register the tools, only where Theseus's settings are a file the daemon
/// can change (issue 22): a config read from the vault, and every default test
/// config, has none, so the tools are in no catalog there and no request
/// carries them.
pub fn register(cfg: &crate::Config, r: &mut theseus_tools::Registry) {
    if cfg.config_file.is_some() {
        r.register(Arc::new(Propose));
        r.register(Arc::new(RestartTool));
        r.register(Arc::new(history::History));
        r.register(Arc::new(show::Show));
        r.register(Arc::new(vault_use::VaultUse));
        r.register(Arc::new(vault_keep::VaultKeep));
        r.register(Arc::new(undo::Undo));
        r.register(Arc::new(mcp_attach::Attach));
        r.register(Arc::new(mcp_attach::Detach));
        r.register(Arc::new(prompt_file::Propose));
        r.register(Arc::new(prompt_file::Read));
    }
}

/// `config.propose`.
pub struct Propose;

impl Tool for Propose {
    fn name(&self) -> &str {
        PROPOSE
    }

    fn description(&self) -> &str {
        "Change a Theseus setting (a budget, a cap, a switch, a server), on the owner's word: the only way to change one. \
         The settings file is never opened, listed or searched, and the shell is not the way. Name each setting by \
         its path (`self.budget.usd`, `import.notes.enabled`, or a whole table such as `mcp.servers.<name>`) with \
         its new value, or `remove: true`, and give a one-line reason in the owner's words. Never write TOML or text \
         for the file. The result says each setting's value before, set or default, so no look is needed first \
         (`config.show` reads other settings). Theseus checks that the settings still load, then shows the owner one \
         card with each value before and after; only their yes changes anything, and the daemon writes it itself. \
         A setting that governs what Theseus may do (approvals, secrets, the sandbox, folders, who the owner is, \
         spending) needs a second proof as well. A secret in the vault is used by `vault.use`, never by writing \
         an `op://` reference here. A fact its maker publishes (a model's exact name, a provider's address and \
         protocol) may be looked up with the web tools rather than asked for: give it with a `source`, never a \
         guess. The owner still confirms, on the card, where a secret is sent. Offered only to the owner, in a \
         private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "changes": {
                    "type": "array",
                    "description": "The settings to change, at most 20.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "The setting's path, keys joined by dots; quote a key that has a dot, as providers.\"my.host\"."},
                            "value": {"description": "Its new value: a switch, a number, text, a list, or a whole table."},
                            "remove": {"type": "boolean", "description": "Remove the setting, so it takes its default. Instead of a value."},
                            "source": {"type": "string", "description": "For a published fact: a page's address and what it says, one line. The card shows it as your word, not a check."}
                        },
                        "required": ["path"],
                        "additionalProperties": false
                    }
                },
                "reason": {"type": "string", "description": "One line, in the owner's words: why they want it."}
            },
            "required": ["changes", "reason"],
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
        let (changes, _, _) = input_of(input)?;
        let paths: Vec<String> = changes.iter().map(|c| edit::dotted(c.path())).collect();
        Ok(Plan {
            summary: format!("propose changing {}", paths.join(", ")),
            ..Default::default()
        })
    }
}

/// Whether this turn has read outside text since the owner's last message,
/// and whether Theseus's own check flagged what it read as steering it. A
/// session whose history cannot be read counts as having read some.
pub(crate) fn outside_text(tc: &TurnCtx<'_>) -> (bool, bool) {
    let outside = match tc.store.session_nodes(tc.session_id) {
        Ok(nodes) => outside_since_owner(&nodes),
        Err(_) => true,
    };
    let steered = matches!(
        tc.store.get_session::<crate::session::SessionRecord>(tc.session_id),
        Ok(Some(s)) if s.data.as_ref().is_some_and(|d| d.frozen())
    );
    (outside, steered)
}

/// The hold's words when outside text alone holds the call.
pub const OUTSIDE_HOLD: &str = "this conversation has read text from outside since your last \
     message; this ends with your next message";

/// The hold's words when the session's flag holds the call, before the way to clear it.
pub const FLAG_HOLD: &str = "Theseus's check flagged text in this conversation as trying to \
     steer it; it asks each time until you clear the flag";

/// Why a call that reads Theseus's own state is held, in the words of its cause (`what` is
/// what the call asks to do): the session's flag, which asks every time until the owner clears
/// it (`way`, from [`flag_way`]); else outside text, which ends with the owner's next message.
/// None when neither holds. Each sentence fits whole in what a card prints of it
/// (`needs_you::rule_clause`, cut at 200 characters by the Discord card).
pub fn hold_words(what: &str, outside: bool, steered: bool, way: &str) -> Option<String> {
    match (outside, steered) {
        (_, true) => Some(format!("{what}: {FLAG_HOLD}: {way}")),
        (true, false) => Some(format!("{what}: {OUTSIDE_HOLD}")),
        (false, false) => None,
    }
}

/// How the owner clears the session's flag, where this conversation is.
pub(crate) fn flag_way(tc: &TurnCtx<'_>) -> String {
    let place = tc.outbox.try_target(tc.session_id).ok().flatten();
    flag_way_in(place.as_deref(), tc.session_id)
}

/// [`flag_way`], on the facts it reads: in the owner's Discord DM, `/trust` there or Remove on
/// the flag's notice, which only that place has; anywhere else, the command that works from
/// every terminal, naming the session by its last four characters.
pub fn flag_way_in(place: Option<&str>, session_id: &str) -> String {
    match place {
        Some(p) if p.starts_with("discord:dm:") => {
            "/trust in the DM, or Remove on its ⚑ notice".into()
        }
        _ => {
            let n = session_id.chars().count();
            let tail: String = session_id.chars().skip(n.saturating_sub(4)).collect();
            format!("`theseus content remove {tail} injection`")
        }
    }
}

/// Whether a rule's clause is a hold's words ([`hold_words`]).
pub(crate) fn is_hold_words(clause: &str) -> bool {
    clause.contains(OUTSIDE_HOLD) || clause.contains(FLAG_HOLD)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartInput {
    reason: String,
}

/// `daemon.restart`: the owner's plain word to restart Theseus.
pub struct RestartTool;

impl Tool for RestartTool {
    fn name(&self) -> &str {
        RESTART
    }

    fn description(&self) -> &str {
        "Ask to restart Theseus, on the owner's word, with no change to its settings. The owner \
         sees one card that says what a restart cuts (a running turn, a job, the \
         self-improvement loop's build); only their yes restarts it, and Theseus then tells them \
         it is back and how long it took. Offered only to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {"type": "string", "description": "One line, in the owner's words: why they want it restarted."}
            },
            "required": ["reason"],
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
        let _: RestartInput = parse(input)?;
        Ok(Plan {
            summary: "ask to restart Theseus".into(),
            ..Default::default()
        })
    }
}

/// Run `daemon.restart`: put one card to the owner, which says what a restart
/// cuts. The yes restarts by the same road as a change does, with a marker so
/// the owner is told when it is back.
pub fn restart(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(wall_why) = wall::refusal(tc) {
        return Err(wall_why.into());
    }
    if let Ok(done) = proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let i: RestartInput = parse(input)?;
    rt.config_change.file.as_ref().ok_or(
        "this daemon cannot restart itself and come back checked (its settings are not a file it \
         keeps a copy of)",
    )?;
    let reason = card::reason_line(&i.reason);
    if reason.is_empty() {
        return Err("give a reason in the owner's words, one line".into());
    }
    let (outside, steered) = outside_text(tc);
    let question = card::restart_question(&card::RestartFacts {
        // The asking turn is one of them, and ends as this returns.
        running: rt.admission.load().0.saturating_sub(1),
        // Whatever is parked on a question to the owner, a self build included.
        waiting: tc
            .kernel
            .pending_confirms()
            .map_or(0, |v| u32::try_from(v.len()).unwrap_or(u32::MAX)),
        outside_text: outside || steered,
        steered,
        reason: &reason,
        wait_secs: RESTART_WAIT_SECS,
    });
    let proposal = Proposal {
        tool: ACK.into(),
        args: json!({
            "question": question, "restart_only": true, "paths": Vec::<String>::new(),
            "restart": true, "reason": reason, "outside_text": outside || steered,
            "settled": words::RESTART,
            "asked_by": {
                "session_id": tc.session_id, "execution_id": tc.execution_id,
                "turn_id": tc.turn_id, "place": tc.outbox.target(tc.session_id),
            },
        }),
        resource: Some("daemon".into()),
        policy_context: json!({}),
    };
    let said = format!(
        "Asked the owner:\n{question}Nothing restarts until they say yes, from a private place."
    );
    // A plain restart changes no setting: its own card, and the card's yes alone.
    ask(
        rt,
        tc,
        proposal,
        correlation_id,
        &said,
        proof_seam::Needs::Card,
    )?;
    Ok((said, json!({})))
}

/// Whether any result after the owner's last message is marked as outside
/// text.
pub(crate) fn outside_since_owner(nodes: &[(u64, crate::node::Node)]) -> bool {
    let last_said = nodes
        .iter()
        .rposition(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }));
    nodes[last_said.map_or(0, |i| i + 1)..]
        .iter()
        .any(|(_, n)| crate::external::is_outside(n))
}

/// The settings of a change that are sensitive: each path and why. Both what
/// the proposal named and what really changes are read, so a table written
/// whole, an array or a renamed key is classified as the settings it reaches.
pub(crate) fn sensitive(
    changes: &[edit::Change],
    settings: &[diff::Setting],
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let named = changes.iter().map(|c| c.path());
    for p in named.chain(settings.iter().map(|s| s.path.as_slice())) {
        if let Some(why) = sensitivity::reason(p) {
            let d = edit::dotted(p);
            if !out.iter().any(|(x, _)| *x == d) {
                out.push((d, why));
            }
        }
    }
    out
}

/// Run `config.propose` for the call `correlation_id` of the turn `tc`: build
/// the candidate, check it loads, and ask. An error is the result the model
/// reads.
pub fn propose(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = wall::refusal(tc) {
        return Err(why.into());
    }
    // Run again after a restart, it answers with what the first run asked.
    if let Ok(done) = proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let (changes, sources, reason) = input_of(input)?;
    let base = read_base(rt)?;
    let extra = Extra {
        sources: &sources,
        ..Extra::default()
    };
    put_using(rt, tc, correlation_id, &base, &changes, &reason, extra)
}

/// The settings file as it is on disk, which must load as it is: nothing is
/// built on a file that does not.
pub(crate) fn read_base(rt: &ToolRuntime) -> Result<String, String> {
    let file = rt.config_change.file.clone().ok_or(
        "Theseus's settings are not a file this daemon can change (they are read from the vault, or \
         were not given as a file): the owner edits them where they live",
    )?;
    let base = std::fs::read_to_string(&file)
        .map_err(|e| format!("the settings file cannot be read: {e}"))?;
    crate::Config::parse(&base).map(drop).map_err(|e| {
        format!("the settings file as it is now does not load, so nothing is built on it: {e:#}")
    })?;
    Ok(base)
}

/// Put `changes` to the file `base` to the owner as one card: the candidate,
/// its check, the sensitive list and the proof seam, the card, the question.
/// `config.undo` comes here, and `config.propose` to [`put_using`], so
/// an undo is held to everything a proposal is (`undoes` is only what an undo
/// adds to the card and to the question's arguments).
pub(crate) fn put(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    correlation_id: &str,
    base: &str,
    changes: &[edit::Change],
    reason: &str,
    undoes: Option<&undo::Undoes>,
) -> Result<(String, Value), String> {
    let extra = Extra {
        undoes,
        ..Extra::default()
    };
    put_using(rt, tc, correlation_id, base, changes, reason, extra)
}

/// What a change carries beyond its settings: the undo it is, or the vault item
/// it gives to use.
#[derive(Default)]
pub(crate) struct Extra<'a> {
    pub undoes: Option<&'a undo::Undoes>,
    pub secret: Option<&'a vault_use::Chosen>,
    /// What the model said of where it found a value (`source.rs`).
    pub sources: &'a [source::Sourced],
}

/// [`put`], for a change that also gives a vault item to use (`vault.use`):
/// `secret` is what the daemon checked and built, and it is the only vault
/// reference the change may write. Any other change that writes one (a
/// proposal's, an `also` of the tool's) is refused, so every reference on a
/// card was built by the daemon from names it found (`vault_use`).
pub(crate) fn put_using(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    correlation_id: &str,
    base: &str,
    changes: &[edit::Change],
    reason: &str,
    extra: Extra<'_>,
) -> Result<(String, Value), String> {
    let Extra {
        undoes,
        secret,
        sources,
    } = extra;
    let w = &rt.config_change;
    let candidate = edit::apply(base, changes)?;
    let (loaded, _) = crate::Config::parse(&candidate)
        .map_err(|e| format!("the settings with that change do not load: {e:#}"))?;
    let settings = diff::between(base, &candidate)?;
    if settings.is_empty() {
        return Err("that would change nothing: the file already has those values".into());
    }
    // An undo puts back what an earlier card showed; nothing else writes a
    // vault reference but the tool that checked it.
    if undoes.is_none() {
        vault_use::refuse_other_references(&settings, secret)?;
    }
    let long = too_big(&settings)?;
    let needs = proof_seam::Needs::of(!sensitive(changes, &settings).is_empty(), &settings);
    let (outside, steered) = outside_text(tc);
    let facts = card::Facts {
        settings: &settings,
        outside_text: outside || steered,
        steered,
        reason,
        wait_secs: RESTART_WAIT_SECS,
        sources,
    };
    let question = match (undoes, secret) {
        (Some(u), _) => {
            let now = theseus_protocol::now_unix_ms();
            card::question_undoing(u.number, &undo::preface(u, now), &facts)
        }
        (None, Some(s)) => card::question_using(&s.preface(&loaded, &settings), &facts),
        (None, None) => card::question(&facts),
    };
    let digest = write::keep_candidate(&w.state, &candidate)
        .map_err(|e| format!("the candidate could not be kept for the card: {e}"))?;
    // A card with a long value has a whole text to read before the yes: the
    // file as it was is kept beside the candidate (`whole.rs`).
    if long > 0 {
        write::keep_text(&w.state, write::Kept::SettingsBase, base)
            .map_err(|e| format!("the settings could not be kept for the card: {e}"))?;
    }
    let settled = words::change(&settings, undoes.map(|u| u.number));
    let mut args = json!({
        "question": question,
        "settled": match secret {
            Some(s) => format!("{}: {settled}", s.settled(&loaded)),
            None => settled,
        },
        "base_sha256": crate::config_copy::sha256(base),
        "candidate_sha256": digest,
        "paths": settings.iter().map(|s| edit::dotted(&s.path)).collect::<Vec<_>>(),
        "restart": settings.iter().any(|s| sensitivity::needs_restart(&s.path)),
        "undoes": undoes.map(undo::Undoes::args),
        "needs_fresh_proof": needs == proof_seam::Needs::Fresh,
        "reason": reason,
        "outside_text": outside || steered,
        "asked_by": {
            "session_id": tc.session_id,
            "execution_id": tc.execution_id,
            "turn_id": tc.turn_id,
            "place": tc.outbox.target(tc.session_id),
        },
    });
    if long > 0 {
        args["whole"] = json!(true);
        args["base_kept"] = json!(true);
    }
    if let Some(s) = secret {
        args["secret"] = s.args();
    }
    let proposal = Proposal {
        tool: ACK.into(),
        args,
        resource: Some("config".into()),
        policy_context: json!({}),
    };
    let said = format!(
        "Asked the owner:\n{question}Nothing changes until they say yes, from a private place{}; \
         the daemon then writes the file itself. If they decline, or do not answer in time, \
         nothing is written.\n{}\n{}\n{}",
        match needs {
            proof_seam::Needs::Card => "",
            _ => ", with the second proof",
        },
        waiting::SETTINGS_CHECKED,
        waiting::waiting(tc),
        show::before(base, &settings)
    );
    ask(rt, tc, proposal, correlation_id, &said, needs)?;
    Ok((
        said,
        json!({"settings": settings.len(), "candidate": digest}),
    ))
}

/// What cannot be shown whole is refused first, in its own words; else how many
/// values are too long for a line. A value too long for a line is not refused:
/// it has its own form on the card (`long_value.rs`), with its whole text
/// readable before the yes.
fn too_big(settings: &[diff::Setting]) -> Result<usize, String> {
    let short: Vec<diff::Setting> = settings
        .iter()
        .filter(|s| !diff::too_long(s))
        .cloned()
        .collect();
    if let Some(why) = card::too_much(&short) {
        return Err(why);
    }
    let long = settings.len() - short.len();
    if long > long_value::MAX_LONG {
        return Err(format!(
            "that changes {long} long values, and one card shows at most {} of them, each with \
             its changed lines: ask for fewer at a time",
            long_value::MAX_LONG
        ));
    }
    Ok(long)
}

/// The question, its card, and the record that answers a call run again, in
/// one frame on the proposing execution.
pub(crate) fn ask(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    proposal: Proposal,
    correlation_id: &str,
    said: &str,
    needs: proof_seam::Needs,
) -> Result<(), String> {
    let target = tc.outbox.target(tc.session_id);
    let mut card = None;
    let a = tc
        .kernel
        .plan_confirm_with(tc.guard, &proposal, RetryClass::NonRepeatable, None, |a| {
            let mut records = vec![NewRecord::json(
                kinds::META,
                Some(&format!("{CALL_PREFIX}{correlation_id}")),
                &said,
            )?];
            // A card that asks for the second proof is marked by the daemon's
            // own record, in the frame that asks it.
            records.extend(needs.mark(&a.correlation_id)?);
            if let Some(target) = &target {
                let (post, more) = tc.outbox.stage(
                    tc.session_id,
                    tc.execution_id,
                    target,
                    json!({"kind": "card", "question": a.correlation_id, "node": null}),
                )?;
                records.extend(more);
                card = Some(post);
            }
            Ok(records)
        })
        .map_err(|e| format!("the question was not asked: {e:#}"))?;
    if let Some(post) = card {
        tc.outbox.posted(&post);
    }
    // The card is the question: the owner's words that began this turn are not
    // also offered or kept as a standing record (issue 22).
    if let Some(core) = rt.config_change.core() {
        if let Some(node) = core.turn_input(tc.turn_id) {
            core.settled_by_act(tc.session_id, &node);
        }
    }
    if let Ok(Some(session)) = tc
        .store
        .get_session::<crate::session::SessionRecord>(tc.session_id)
    {
        let mut request = confirm_request(&a, &session, tc.confirm_ttl_ms);
        request.needs_proof = needs != proof_seam::Needs::Card;
        request.needs_fresh_proof = needs == proof_seam::Needs::Fresh;
        tc.record(&crate::fact::tool::CallAsked { request: &request });
    }
    Ok(())
}

/// A proposal run again after a restart: what the first run asked, else why
/// it must be proposed again.
pub fn proposed_by_call(tc: &TurnCtx<'_>, correlation_id: &str) -> Result<(String, Value), String> {
    match tc
        .store
        .get_meta::<String>(&format!("{CALL_PREFIX}{correlation_id}"))
    {
        Ok(Some(said)) => Ok((said, json!({}))),
        _ => Err(
            "the daemon restarted before this proposal was put to the owner, so nothing was \
                  asked: propose it again"
                .into(),
        ),
    }
}

/// The ack's question as the owner sees it: its card, `confirm.list`.
pub fn confirm_request(
    a: &Action,
    session: &crate::session::SessionRecord,
    ttl_ms: u64,
) -> ConfirmRequest {
    let args = a
        .proposal
        .as_ref()
        .map(|p| p.args.clone())
        .unwrap_or_default();
    let plain = words::plain_of(&args);
    let keep = vault_keep::is_card(&args);
    let mut request = crate::rpc::offer(ConfirmRequest {
        correlation_id: a.correlation_id.clone(),
        session_id: session.session_id.clone(),
        execution_id: a.execution_id.clone(),
        turn_id: None,
        tool: ACK.into(),
        reason: args["question"].as_str().unwrap_or_default().to_string(),
        input: args,
        resource: a.resource.clone(),
        by: crate::turn::OPERATOR.into(),
        requested_at_ms: a.planned_at_ms,
        expires_at_ms: a.planned_at_ms + ttl_ms,
        floor: false,
        budget: None,
        task: crate::task::task_ref(session),
        change: None,
        options: Vec::new(),
        may_answer: None,
        preview: None,
        grant_offer: None,
        plain,
        // `confirm.list` reads the proof's marks onto it; the card the asker
        // posts says it in `ask`.
        needs_proof: false,
        needs_fresh_proof: false,
        proof: None,
    });
    // A secret to keep takes a value, which a yes does not carry, and no place's buttons do
    // either (the cockpit has a field, the terminal a hidden prompt, Discord nothing yet): the
    // daemon states it by offering only the decline, so no surface can draw an Approve that
    // could only be refused (issue 29).
    if keep {
        request.options.retain(|o| o.id == "decline");
    }
    request
}

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
pub(crate) mod tests_answer;
#[cfg(test)]
pub(crate) mod tests_hold;
#[cfg(test)]
mod tests_look;
#[cfg(test)]
mod tests_look_card;
#[cfg(test)]
mod tests_look_fields;
#[cfg(test)]
mod tests_ontology_look;
#[cfg(test)]
mod tests_own_command;
#[cfg(test)]
mod tests_press;
#[cfg(test)]
mod tests_profile;
#[cfg(test)]
mod tests_profile_asked;
#[cfg(test)]
mod tests_profile_ended;
#[cfg(test)]
mod tests_profile_names;
#[cfg(test)]
mod tests_profile_notes;
#[cfg(test)]
mod tests_profile_run;
#[cfg(test)]
mod tests_profile_run_more;
#[cfg(test)]
mod tests_proof;
#[cfg(test)]
mod tests_source;
#[cfg(test)]
mod tests_undo;
#[cfg(test)]
mod tests_withdrawn;
#[cfg(test)]
mod tests_work;
#[cfg(test)]
mod tests_work_backfill;
#[cfg(test)]
mod tests_work_global;
#[cfg(test)]
mod tests_work_notes;
#[cfg(test)]
mod tests_work_rest;
