//! `vault.use`'s run form (issue 64): the owner's own turn names a vault item for one variable
//! of one run of one program, and nothing stays. No card, no code, no settings, no restart.
//!
//! - **The wall is the card form's** (`wall.rs`), and a routine never has the tool.
//! - **It asks first** at the gate, before anything is read, when the turn has read text from
//!   outside since the owner's last message (at both `outside` settings), when the session is
//!   flagged, or when the owner's own message did not begin the turn (a wake, a task's report, a
//!   note the harness wrote): [`ask_first`]. Approved, it runs as below.
//! - **The program** is resolved when the form is called: a bare name on the daemon's PATH, as
//!   a configured grant's is, or a path with a `/` in the turn's working directory, to its
//!   canonical file. A shell, an interpreter or a wrapper is refused by name. A script run by its
//!   own first line is not: the interpreter that line names then reads the variable.
//! - **The item** is found and read as the card form's is (`check.rs`), and the value is kept,
//!   not read twice. An item Theseus itself runs on is not given ([`runs_on`]): by what reads its
//!   name in the settings, and by value. It goes by the card form.
//! - **The value** is held on the board from the moment it is read, so every exit masks it, and
//!   the broker gives it to the next run of that file in this turn by its own argv
//!   (`broker/once.rs`). The run's stored result is scrubbed of it.
//! - **The owner is told** in one FYI note: the item, the field and the vault by the names the
//!   vault listed, the program's file name, the variable and the conversation's short id; none
//!   of the model's words.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::parse;

use super::check::{self, Read};
use super::USE;
use crate::broker::OnceGrant;
use crate::node::{Body, Origin};
use crate::secrets::SecretRef;
use crate::toolrun::{ToolRuntime, TurnCtx};

/// The answer for an item Theseus itself runs on.
pub const RUNS_ON: &str = "this item is a key Theseus itself runs on, so it is not given to a \
     program for one run: use `vault.use` with `name` (and `also`) instead, which puts it to the \
     owner on a card, as today";

/// The hold's words when the owner's own message did not begin the turn.
pub const NOT_BEGUN: &str = "this turn was not begun by your own message (a wake, a task's \
     report or a note began it)";

/// The answer when outside text reached the conversation after the gate judged this call.
pub const AGAIN: &str = "text from outside reached this conversation after this call was \
     checked, so nothing was read from the vault: call `vault.use` again, on its own, and the \
     owner is asked first";

/// The answer for a variable that steers how a program is found or started ([`steers`]).
pub const STEERS: &str = "`run.variable` may not be PATH, a loader's variable (LD_*, DYLD_*), a \
     shell's start file (BASH_ENV, ENV) or a name Theseus keeps for itself (OP_*, AWS_*, \
     ANTHROPIC*, THESEUS*): choose another name for the variable the program reads";

/// The answer for a value too short to be masked.
pub const SHORT: &str = "the field's value is too short for Theseus to mask wherever it could \
     be shown (under 8 characters), so it is not given to a program";

/// What the run form asks to do, as the hold says it.
const WHAT: &str = "give a key from Theseus's vault to a program for one run";

/// The longest a variable's name may be.
const MAX_VARIABLE: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    vault: String,
    item: String,
    field: String,
    #[serde(default)]
    section: Option<String>,
    run: Run,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    program: String,
    variable: String,
}

/// Whether a call is the run form.
pub(crate) fn is_run(input: &Value) -> bool {
    input.get("run").is_some()
}

/// The run form's plan: what it gives to which program.
pub(super) fn plan(input: &Value) -> Result<Plan, String> {
    let i: Input = parse(input)?;
    let clean = crate::secrets::look::clean;
    Ok(Plan {
        summary: format!(
            "use the vault's item {} as {} for one run of {}",
            clean(&i.item),
            clean(&i.run.variable),
            clean(&i.run.program)
        ),
        ..Default::default()
    })
}

/// The gate's hold on the run form, at both `outside` settings: outside text since the owner's
/// last message, the session's flag, or a turn the owner's own message did not begin.
pub(crate) fn ask_first(tc: &TurnCtx<'_>, plan: Plan) -> Plan {
    ask_first_for(tc, WHAT, &|_| false, plan)
}

/// [`ask_first`] for the call that asks to do `what`, with the results `skip` names not counted
/// as outside text (`http.request`'s earlier answers from the same host with the same item).
pub(crate) fn ask_first_for(
    tc: &TurnCtx<'_>,
    what: &str,
    skip: &dyn Fn(&crate::node::Node) -> bool,
    mut plan: Plan,
) -> Plan {
    if let Some(said) = hold_reason_for(tc, what, skip) {
        plan.authority = Some(said);
    }
    plan
}

/// Why the run form is put to the owner first, or None: outside text since the owner's own last
/// message ([`outside_since_owners_message`]), the session's flag, or a turn the owner's own
/// typed input did not begin ([`begun_by_owner`]). Read at the gate, and again when the call
/// runs (`run`).
fn hold_reason(tc: &TurnCtx<'_>) -> Option<String> {
    hold_reason_for(tc, WHAT, &|_| false)
}

/// [`hold_reason`] for the call that asks to do `what`, with the results `skip` names not
/// counted as outside text.
pub(super) fn hold_reason_for(
    tc: &TurnCtx<'_>,
    what: &str,
    skip: &dyn Fn(&crate::node::Node) -> bool,
) -> Option<String> {
    let outside = match tc.store.session_nodes(tc.session_id) {
        Ok(nodes) => outside_since_owners_message(&nodes, skip),
        Err(_) => true,
    };
    let (_, steered) = crate::config_change::outside_text(tc);
    let way = crate::config_change::flag_way(tc);
    crate::config_change::hold_words(what, outside, steered, &way)
        .or_else(|| (!begun_by_owner(tc)).then(|| format!("{what}: {NOT_BEGUN}")))
}

/// Whether any result after the owner's own last message is marked as outside text. The owner's
/// message is the latest message of his that is not the harness's own words (`content::
/// harness_own`: its clock, its progress, a helper's fixed note), so such a note does not end the
/// window; a result whose call the owner approved with "Approve and clear" counts as his message,
/// as `hold::span` counts it.
fn outside_since_owners_message(
    nodes: &[(u64, crate::node::Node)],
    skip: &dyn Fn(&crate::node::Node) -> bool,
) -> bool {
    let at = nodes.iter().rposition(|(_, n)| match &n.body {
        Body::UserMessage { .. } => {
            n.origin == Origin::Operator
                && !crate::content::harness_own(n.origin, n.author.as_deref())
        }
        Body::ToolResult { meta, .. } => {
            meta[crate::config_change::hold::CLEARED].as_bool() == Some(true)
        }
        _ => false,
    });
    let from = match at {
        Some(i) if matches!(nodes[i].1.body, Body::UserMessage { .. }) => i + 1,
        Some(i) => i,
        None => 0,
    };
    nodes[from..]
        .iter()
        .any(|(_, n)| crate::external::is_outside(n) && !skip(n))
}

/// Whether the owner's own typed input began this turn: the latest message of the session that
/// is not the harness's own words is a person's, written by this turn as its opening input, and
/// this turn's submission was judged the owner's own (`EventSink::owner`, from
/// `Core::owner_input`: the owner, in a private place, and not a job's or a prompt's). A wake's,
/// a task's report, any other note the harness relays, a later continuation (a job that ended)
/// and a job's or an agent's submission are not. A session whose history cannot be read is not.
fn begun_by_owner(tc: &TurnCtx<'_>) -> bool {
    if !tc.sink.owner {
        return false;
    }
    let Ok(nodes) = tc.store.session_nodes(tc.session_id) else {
        return false;
    };
    let Some(n) = began(&nodes) else {
        return false;
    };
    // The turn's opening input, not a message steered into it.
    let opening = nodes.iter().map(|(_, m)| m).find(|m| {
        m.turn_id.as_deref() == Some(tc.turn_id)
            && matches!(m.body, Body::UserMessage { .. })
            && !crate::content::harness_own(m.origin, m.author.as_deref())
    });
    n.origin == Origin::Operator
        && n.turn_id.as_deref() == Some(tc.turn_id)
        && opening.is_some_and(|m| m.id == n.id)
}

/// The message that began the latest turn of `nodes`: the latest that is not the harness's own
/// words. A continuation an answer to a card began has none of its own.
fn began(nodes: &[(u64, crate::node::Node)]) -> Option<&crate::node::Node> {
    nodes.iter().rev().map(|(_, n)| n).find(|n| {
        matches!(n.body, Body::UserMessage { .. })
            && !crate::content::harness_own(n.origin, n.author.as_deref())
    })
}

/// A turn of `session` ended: its one-run grants end too, except one whose program a run waiting
/// on the owner's answer names (a `proc.run` the gate asked about), which the continuation his
/// answer begins may still take.
pub(crate) fn turn_ended(rt: &ToolRuntime, kernel: &theseus_kernel::Kernel, session: &str) {
    if !rt.broker.has_once(session) {
        return;
    }
    let waiting: Vec<String> = kernel
        .pending_confirms()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.session_id == session && a.tool == "proc.run")
        .filter_map(|a| {
            let first = a.proposal?.args.pointer("/argv/0")?.as_str()?.to_string();
            let name = std::path::Path::new(&first)
                .file_name()?
                .to_str()?
                .to_string();
            Some(name)
        })
        .collect();
    rt.broker.once_keep_only(session, &waiting);
}

/// A call of the turn `tc`: the session's one-run grants that another turn left end.
pub(crate) fn end_other_turns(rt: &ToolRuntime, tc: &TurnCtx<'_>) {
    if rt.broker.has_once(tc.session_id) {
        rt.broker.once_turn(tc.session_id, &turn_of(tc));
    }
}

/// [`turn_of`] when the session has a one-run grant to take; empty, read from nothing, when not.
pub(crate) fn held_turn(rt: &ToolRuntime, tc: &TurnCtx<'_>) -> String {
    match rt.broker.has_once(tc.session_id) {
        true => turn_of(tc),
        false => String::new(),
    }
}

/// The turn a one-run grant is given in and taken in: the id of the message that began it, or
/// the turn's own id when none can be read.
pub(crate) fn turn_of(tc: &TurnCtx<'_>) -> String {
    tc.store
        .session_nodes(tc.session_id)
        .ok()
        .and_then(|nodes| began(&nodes).map(|n| n.id.clone()))
        .unwrap_or_else(|| tc.turn_id.to_string())
}

/// Whether the owner approved the call `correlation_id` itself, on its card.
pub(super) fn approved(tc: &TurnCtx<'_>, correlation_id: &str) -> bool {
    tc.kernel
        .action(correlation_id)
        .ok()
        .flatten()
        .is_some_and(|a| a.confirm.is_some())
}

/// A variable's name: letters, digits and `_`, not starting with a digit, as the settings take a
/// program's.
fn valid_variable(v: &str) -> bool {
    v.len() <= MAX_VARIABLE
        && v.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a variable steers how a program is found, loaded or started, or is one Theseus keeps
/// for itself: `PATH` (which a job's call may not set either), a dynamic loader's (`LD_*`,
/// `DYLD_*`), a shell's start file (`BASH_ENV`, `ENV`, which a script's own first line may run),
/// and the prefixes a job's call may not set (`OP_*`, `AWS_*`, `ANTHROPIC*`, `THESEUS*`). The
/// key's own names (`*_TOKEN`, `*_KEY`) are what the form is for, and stay allowed.
fn steers(v: &str) -> bool {
    let n = v.to_ascii_uppercase();
    matches!(n.as_str(), "PATH" | "BASH_ENV" | "ENV")
        || ["LD_", "DYLD_", "OP_", "AWS_", "ANTHROPIC", "THESEUS"]
            .iter()
            .any(|p| n.starts_with(p))
}

/// The program's file, or why it is not one a variable may be given to.
fn program_file(rt: &ToolRuntime, tc: &TurnCtx<'_>, program: &str) -> Result<PathBuf, String> {
    let cwd = rt.ctx_in(tc).cwd.clone();
    let Some(file) = rt.broker.resolve_once(program, &cwd) else {
        return Err(match program.contains('/') {
            true => format!(
                "`{}` is not an executable file in the working directory",
                crate::secrets::look::clean(program)
            ),
            false => format!(
                "`{}` is not a program on Theseus's PATH: name a script by its path, as `./{}`",
                crate::secrets::look::clean(program),
                crate::secrets::look::clean(program)
            ),
        });
    };
    let name = file
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    if let Some(what) = crate::broker::launcher(name) {
        return Err(format!(
            "`{}` is {what}, which runs whatever it is given, so the variable would reach that \
             too: name the program or script itself, run by its own argv",
            file.display()
        ));
    }
    Ok(file)
}

/// Run the run form for the call `correlation_id` of the turn `tc`; the wall has passed. An
/// error is the result the model reads.
pub(super) async fn run(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    let i: Input = parse(input)?;
    if crate::config_change::card::reason_line(&i.reason).is_empty() {
        return Err("give a reason in the owner's words, one line".into());
    }
    let variable = i.run.variable.trim().to_string();
    if !valid_variable(&variable) {
        return Err(format!(
            "`run.variable` is an environment variable's name: letters, digits and `_`, not \
             starting with a digit, at most {MAX_VARIABLE} (for example `LANTERN_TOKEN`)"
        ));
    }
    if steers(&variable) {
        return Err(STEERS.into());
    }
    // Outside text that came after the gate judged this call (an earlier call of its response):
    // asked for, unless the owner approved this very call.
    if hold_reason(tc).is_some() && !approved(tc, correlation_id) {
        return Err(AGAIN.into());
    }
    let file = program_file(rt, tc, i.run.program.trim())?;
    let Some(reader) = rt.look.reader().filter(|r| !r.has_no_vault()) else {
        return Err(
            "this Theseus has no vault session (its secrets come from the environment or from \
             files), so it cannot use a vault item"
                .into(),
        );
    };
    let read = check::reading(
        &reader,
        &i.vault,
        &i.item,
        &i.field,
        i.section
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .await?;
    if runs_on(rt, &read)? {
        return Err(RUNS_ON.into());
    }
    // A value the scrubber cannot tell from chance is masked at no exit.
    if !crate::scrub::protectable(read.value.expose()) {
        return Err(SHORT.into());
    }
    // Remembered by name before the value can reach a program, so every start masks it again.
    super::used::remember(tc.store, &read.checked)?;
    let Read { checked, value, .. } = read;
    let program = file
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string();
    rt.broker.grant_once(
        OnceGrant {
            session: tc.session_id.into(),
            turn: turn_of(tc),
            file: file.clone(),
            variable: variable.clone(),
            handle: format!("{}{correlation_id}", crate::broker::ONCE_HANDLE),
            vault: [
                checked.vault.clone(),
                checked.item.clone(),
                checked.field.clone(),
            ],
        },
        value,
    );
    let short = crate::narrative::short(tc.session_id);
    let note = crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        "A key from the vault, for one run",
        &format!(
            "In conversation {short}, Theseus made the vault item \"{}\" (field \"{}\", in the \
             vault \"{}\") ready for one run of {program}, as {variable}.",
            checked.item, checked.field, checked.vault
        ),
        "The grant ends with that run or with the turn, and nothing was written to the settings. \
         Theseus remembers which items it has used this way, by name only, and reads them again at \
         each start so that it keeps masking their values; the next use reads the vault again. To \
         stop it being used at all, take the item out of what Theseus's vault account can see.",
    );
    crate::config_change::hold::tell_owner(tc, &note, USE);
    Ok((
        format!(
            "{variable} is ready for one run of {program} in this turn: run it by its own argv, \
             as [\"{}\", …], with no `env` and not in the background ({}). The value is not \
             shown and never passes through you; nothing was written to the settings.",
            crate::secrets::look::clean(i.run.program.trim()),
            file.display()
        ),
        json!({"once": true, "program": program, "variable": variable}),
    ))
}

/// Whether the item read is a key Theseus itself runs on: its reference is one a name Theseus
/// reads is read from (in the settings running or as written, or the settings note's), or its
/// value is one the board holds for Theseus now (such a name's, or any value held at run time).
/// A name only a program's grant or an attached server's variable reads is not Theseus's own.
pub(super) fn runs_on(rt: &ToolRuntime, read: &Read) -> Result<bool, String> {
    let Some(core) = rt.jobs.core() else {
        return Err(
            "Theseus cannot read its own settings now, so it cannot tell whether \
                    this item is a key it runs on"
                .into(),
        );
    };
    let written = core
        .tools
        .config_change
        .file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| crate::Config::parse(&t).ok())
        .map(|(c, _)| c);
    let mut refs = vec![core.config_gate.reference()];
    let mut names = Vec::new();
    for cfg in std::iter::once(&*core.cfg).chain(written.as_ref()) {
        for (name, reference) in &cfg.secrets {
            if own(cfg, name) {
                refs.push(reference.clone());
                names.push(name.clone());
            }
        }
    }
    let c = &read.checked;
    let same = |a: &str, b: &str, id: &str| a.eq_ignore_ascii_case(b) || a == id;
    let by_ref = refs
        .iter()
        .filter_map(|r| SecretRef::parse(r).ok())
        .any(|r| same(&r.vault, &c.vault, &read.vault_id) && same(&r.item, &c.item, &read.item_id));
    // By value: every value the board holds that Theseus runs on: a configured name Theseus reads,
    // and every value held at run time (a signed-in server's token, a consent's, a value a call
    // returned), but not a name only a program's grant reads, and not the run form's own holds.
    let configured: std::collections::BTreeSet<&String> = std::iter::once(&*core.cfg)
        .chain(written.as_ref())
        .flat_map(|c| c.secrets.keys())
        .collect();
    let board = core.secrets.states();
    let by_value = board.iter().any(|(name, state)| {
        let crate::secrets::SecretState::Ready(v) = state else {
            return false;
        };
        let theirs = match configured.contains(name) {
            true => names.contains(name),
            false => {
                !name.starts_with(crate::broker::ONCE_HANDLE)
                    && !name.starts_with(super::used::HANDLE)
            }
        };
        theirs && v.expose() == read.value.expose()
    });
    Ok(by_ref || by_value)
}

/// Whether Theseus itself reads the secret `name` in `cfg`: a reader that names it by default,
/// or any setting set to it that is not a program's variable or an attached server's.
fn own(cfg: &crate::Config, name: &str) -> bool {
    if super::default_readers(cfg).iter().any(|(f, _)| f == name) {
        return true;
    }
    let Ok(v) = serde_json::to_value(cfg) else {
        return true;
    };
    let mut leaves = std::collections::BTreeMap::new();
    crate::config_change::diff::flatten(&v, &mut Vec::new(), &mut leaves);
    leaves.iter().any(|(path, value)| {
        let reads = match value {
            Value::String(s) => s == name,
            Value::Array(a) => a.iter().any(|x| x.as_str() == Some(name)),
            _ => false,
        };
        let p: Vec<&str> = path.iter().map(String::as_str).collect();
        let program = matches!(
            p.as_slice(),
            ["secrets", ..] | ["broker", "programs", _, "env", _] | ["mcp", "servers", _, "env", _]
        );
        reads && !program
    })
}
