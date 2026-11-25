//! `vault.use`: the owner's own turn puts a secret that is in the vault to use,
//! by its name (issue 26, line 1), on the one settings card.
//!
//! The turn names a vault, an item and a field in words, a name of its choosing
//! for the secret, and the other settings that should read it. The DAEMON finds
//! the item and the field in the vault's own lists with its own token, builds
//! the reference itself, reads it once as a start will to prove the account can
//! (`check.rs`: yes or no, never the value), and then puts the whole change to
//! the owner as ONE card (`put_using`, the path every settings change takes: the
//! sensitive list, the second proof, the restart). The card names the item and
//! field as the owner would recognise them, says the daemon read it and what
//! it is given. **The value passes through no turn, no card and no record.**
//!
//! **A vault reference is never written by a turn.** `config.propose` refuses a
//! change that writes one (`refuse_other_references`): the card of a reference a
//! model wrote shows a string the owner must read as a path, and a title with a
//! slash, a query or a lookalike of a reference would make it name one thing
//! while the vault reads another. Here the reference is built from names the
//! vault listed, so the card and the start read the same thing.
//!
//! Who may call it, and the outside-text hold, are the settings tools' and the
//! looks' (`wall.rs`, [`ask_if_outside`]): the owner's own turn in a private
//! place, asked first at the gate when the turn has read text from outside since
//! the owner's last message, never a job, a task, the self loop, an MCP session
//! or a shared place. The design: `docs/design/vault-use.md`.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::{parse, Backend, Retry, Tool, ToolClass, ToolCtx};

use super::{diff, edit};
use crate::secrets::Source;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub(crate) mod check;
#[cfg(test)]
mod tests;

pub use check::Checked;

pub const USE: &str = "vault.use";
/// The longest a secret's name may be.
const MAX_NAME: usize = 48;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    vault: String,
    item: String,
    field: String,
    #[serde(default)]
    section: Option<String>,
    name: String,
    #[serde(default)]
    also: Vec<super::Item>,
    reason: String,
}

/// What the daemon checked, and the name the secret is kept under: what the
/// card and the question's record are built from.
#[derive(Debug, Clone)]
pub struct Chosen {
    pub checked: Checked,
    pub name: String,
}

impl Chosen {
    /// What the card says before the settings: the item and the field as the
    /// owner would recognise them, that the daemon read it, and what is given
    /// the secret.
    pub fn preface(&self, cfg: &crate::Config, settings: &[diff::Setting]) -> String {
        let c = &self.checked;
        let section = c
            .section
            .as_deref()
            .map_or(String::new(), |s| format!(", in the section \"{s}\""));
        let by_id = match c.by_id {
            true => {
                " Part of it is written by the vault's own ids, because a name is shared or has a \
                 character a reference cannot carry."
            }
            false => "",
        };
        let others = unrelated(settings, &self.name, cfg);
        let also = match others.is_empty() {
            true => String::new(),
            false => format!(
                "It also changes settings that do not read the secret: {}.\n",
                others.join(", ")
            ),
        };
        format!(
            "The secret: the item \"{}\", field \"{}\"{section}, in the vault \"{}\". Theseus read it \
             just now to check: the item and the field are there, its account can read them, and \
             the field holds something. Its value is not shown here, and never goes to the model, \
             a record or this card.\nIt is kept under the name `{}`, read from {}.{by_id}\n{}\n{also}",
            c.item,
            c.field,
            c.vault,
            self.name,
            c.reference,
            given(&given_to(cfg, &self.name))
        )
    }

    /// The change in one line, for the question's settled words.
    pub fn settled(&self, cfg: &crate::Config) -> String {
        let to = given_to(cfg, &self.name);
        let given = match to.is_empty() {
            true => "not given to anything yet".to_string(),
            false => format!("given to {}", to.join("; ")),
        };
        format!(
            "use the vault's item \"{}\" (field \"{}\") as `{}`, {given}",
            self.checked.item, self.checked.field, self.name
        )
    }

    /// What the question's record keeps: names only.
    pub fn args(&self) -> Value {
        json!({
            "name": self.name,
            "vault": self.checked.vault,
            "item": self.checked.item,
            "field": self.checked.field,
            "checked": true,
        })
    }
}

fn given(to: &[String]) -> String {
    match to.is_empty() {
        true => "Nothing is given it yet: this only keeps where it is read from.".into(),
        false => format!("Given to: {}.", to.join("; ")),
    }
}

/// What reads the secret `name` in `cfg`, in words: every setting whose value is
/// the name, in the loaded settings (defaults included), outside the `[secrets]`
/// table itself.
pub(crate) fn given_to(cfg: &crate::Config, name: &str) -> Vec<String> {
    let Ok(v) = serde_json::to_value(cfg) else {
        return Vec::new();
    };
    let mut leaves = std::collections::BTreeMap::new();
    diff::flatten(&v, &mut Vec::new(), &mut leaves);
    let mut out = Vec::new();
    for (path, value) in &leaves {
        let reads = match value {
            Value::String(s) => s == name,
            Value::Array(a) => a.iter().any(|x| x.as_str() == Some(name)),
            _ => false,
        };
        if reads && path.first().map(String::as_str) != Some("secrets") {
            out.push(describe(path));
        }
    }
    out
}

/// The settings of a change that have nothing to do with the secret: not its
/// own entry, not a setting that reads it, and not a sibling of one (the
/// provider's address beside its key). A card names them, so a friendly heading
/// cannot hide a change of another kind.
fn unrelated(settings: &[diff::Setting], name: &str, cfg: &crate::Config) -> Vec<String> {
    let Ok(v) = serde_json::to_value(cfg) else {
        return Vec::new();
    };
    let mut leaves = std::collections::BTreeMap::new();
    diff::flatten(&v, &mut Vec::new(), &mut leaves);
    let tables: Vec<Vec<String>> = leaves
        .iter()
        .filter(|(p, v)| {
            p.first().map(String::as_str) != Some("secrets")
                && match v {
                    Value::String(s) => s == name,
                    Value::Array(a) => a.iter().any(|x| x.as_str() == Some(name)),
                    _ => false,
                }
        })
        .map(|(p, _)| p[..p.len() - 1].to_vec())
        .collect();
    settings
        .iter()
        .filter(|s| !(s.path.len() == 2 && s.path[0] == "secrets" && s.path[1] == name))
        .filter(|s| !tables.iter().any(|t| s.path.starts_with(t)))
        .map(|s| format!("`{}`", edit::dotted(&s.path)))
        .collect()
}

/// A reading setting, by what it is.
fn describe(path: &[String]) -> String {
    let p: Vec<&str> = path.iter().map(String::as_str).collect();
    match p.as_slice() {
        ["providers", name, "api_key_secret"] => format!("the provider `{name}` (its key)"),
        ["model", "api_key_secret"] => "the main model (its key)".into(),
        ["broker", "programs", prog, "env", var] => {
            format!("the program `{prog}` (its variable `{var}`)")
        }
        ["mcp", "servers", s, "auth_secret"] => format!("the attached server `{s}` (its key)"),
        ["mcp", "servers", s, "env", var] => {
            format!("the attached server `{s}` (its variable `{var}`)")
        }
        _ => format!("the setting `{}`", edit::dotted(path)),
    }
}

/// Why a change writes a vault reference that the daemon did not build, or
/// `Ok`: a setting under `[secrets]` that is not `env:` or `file:`, other than
/// the one the tool checked.
pub(crate) fn refuse_other_references(
    settings: &[diff::Setting],
    allowed: Option<&Chosen>,
) -> Result<(), String> {
    for s in settings {
        if s.path.first().map(String::as_str) != Some("secrets") {
            continue;
        }
        let Some(Value::String(after)) = &s.after else {
            continue;
        };
        if Source::of(after) != Source::Vault {
            continue;
        }
        let ok = allowed.is_some_and(|u| {
            s.path.len() == 2 && s.path[1] == u.name && *after == u.checked.reference
        });
        if !ok {
            return Err(format!(
                "a vault reference is not written as text: it would name one thing on the card \
                 and could be read as another ({}). Use `{USE}` with the vault, the item and \
                 the field in words: Theseus finds them, checks it can read them, and builds \
                 the reference itself",
                edit::dotted(&s.path)
            ));
        }
    }
    Ok(())
}

/// A name for a `[secrets]` entry: letters, digits and `_`, starting with a
/// letter, at most [`MAX_NAME`] of them.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.len() <= MAX_NAME
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Run `vault.use` for the call `correlation_id` of the turn `tc`. An error is
/// the result the model reads.
pub async fn run(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = super::wall::refusal_for(tc, USE) {
        return Err(why);
    }
    // Run again after a restart, it answers with what the first run asked.
    if let Ok(done) = super::proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let i: Input = parse(input)?;
    if !valid_name(&i.name) {
        return Err(format!(
            "`name` is the secret's name in the settings: letters, digits and `_`, starting with \
             a letter, at most {MAX_NAME} (for example `lantern_key`)"
        ));
    }
    if i.also.len() >= super::MAX_CHANGES {
        return Err(format!(
            "name at most {} other settings to change",
            super::MAX_CHANGES - 1
        ));
    }
    let reason = super::card::reason_line(&i.reason);
    if reason.is_empty() {
        return Err("give a reason in the owner's words, one line".into());
    }
    let (also, sources) = super::changes_of(i.also)?;
    if also
        .iter()
        .any(|c| c.path().first().map(String::as_str) == Some("secrets"))
    {
        return Err(format!(
            "`{USE}` writes the secret's entry itself: `also` is for the settings that read it, \
             not for `secrets`"
        ));
    }
    let Some(reader) = rt.look.reader().filter(|r| !r.has_no_vault()) else {
        return Err(
            "this Theseus has no vault session (its secrets come from the environment or from \
             files), so it cannot use a vault item: name the secret with `env:NAME` or \
             `file:PATH` in the settings instead"
                .into(),
        );
    };
    let base = super::read_base(rt)?;
    let checked = check::check(
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
    let used = Chosen {
        checked,
        name: i.name,
    };
    let mut changes = vec![edit::Change::Set(
        vec!["secrets".into(), used.name.clone()],
        Value::String(used.checked.reference.clone()),
    )];
    changes.extend(also);
    super::put_using(
        rt,
        tc,
        correlation_id,
        &base,
        &changes,
        &reason,
        super::Extra {
            secret: Some(&used),
            sources: &sources,
            ..super::Extra::default()
        },
    )
}

/// The gate's hold on a use that is not free: when the turn has read text from
/// outside since the owner's last message, the call is put to the owner before
/// anything is read from the vault, as a look at the vault's names is.
pub(crate) fn ask_if_outside(tc: &TurnCtx<'_>, name: &str, plan: Plan) -> Plan {
    if name != USE && name != super::vault_keep::KEEP {
        return plan;
    }
    let (outside, steered) = super::outside_text(tc);
    held(name, outside, steered, &super::flag_way(tc), plan)
}

/// [`ask_if_outside`], on the facts it reads: each cause in its own words, with `way` to clear
/// the session's flag.
fn held(name: &str, outside: bool, steered: bool, way: &str, mut plan: Plan) -> Plan {
    let what = match name {
        USE => "look in Theseus's vault to use a secret",
        super::vault_keep::KEEP => "ask to keep a secret in Theseus's vault",
        _ => return plan,
    };
    if let Some(said) = super::hold_words(what, outside, steered, way) {
        plan.authority = Some(said);
    }
    plan
}

/// `vault.use`.
pub struct VaultUse;

impl Tool for VaultUse {
    fn name(&self) -> &str {
        USE
    }

    fn description(&self) -> &str {
        "Use a secret that is in Theseus's vault, by its name: the vault, the item and the field (`look.vault` with \
         `fields_of` lists them), a `name` of your choosing for it in the settings, and in `also` the other \
         settings that should read that name (a provider's `api_key_secret`, a program's \
         `broker.programs.<p>.env.<VAR>`, a server's `auth_secret`), written as for `config.propose`. The daemon \
         finds the item and field, checks that its account can read them, builds the reference itself, and shows \
         the owner ONE card that says so; only their yes (and the second proof) changes anything. A secret is \
         used by its name: its value is never asked for, never shown, and never passes through you. Never write an \
         `op://` reference in `config.propose`, and the vault's program in a shell has no session by design. If the \
         account cannot see a vault, the result says what the owner can do. A fact the provider publishes (its \
         address, its protocol) may be looked up with the web tools rather than asked for: put it in `also` with \
         a `source`, never a guess. The owner still confirms, on the card, where the secret is sent. Offered only \
         to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vault": {"type": "string", "description": "The vault's name or id, as `look.vault` lists it."},
                "item": {"type": "string", "description": "The item's title or id."},
                "field": {"type": "string", "description": "The field's label, as `look.vault` lists it with `fields_of`."},
                "section": {"type": "string", "description": "The field's section, only when two fields share the label."},
                "name": {"type": "string", "description": "The secret's name in the settings: letters, digits and `_`, e.g. `lantern_key`."},
                "also": {
                    "type": "array",
                    "description": "Other settings to change on the same card, at most 19, each {path, value} as for `config.propose`; the value that reads the secret is its `name`.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "value": {},
                            "remove": {"type": "boolean"},
                            "source": {"type": "string", "description": "A published fact's source: a page's address and what it says. Shown on the card as your word."}
                        },
                        "required": ["path"],
                        "additionalProperties": false
                    }
                },
                "reason": {"type": "string", "description": "One line, in the owner's words: why they want it."}
            },
            "required": ["vault", "item", "field", "name", "reason"],
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
        let i: Input = parse(input)?;
        Ok(Plan {
            summary: format!(
                "use the vault's item {} as `{}`",
                crate::secrets::look::clean(&i.item),
                crate::secrets::look::clean(&i.name)
            ),
            ..Default::default()
        })
    }
}
