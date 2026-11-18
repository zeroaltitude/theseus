//! `look.vault`: what the vault holds, by name, for the owner's own turn
//! (issue 26): who the daemon's token is, which vaults it sees, the titles of
//! the items in one, and which setting reads which item.
//!
//! The daemon asks, with its own token and its own time limit
//! (`secrets::look`), and the turn is told what came back. **A value or a
//! note's text is not on any path**: the verbs are a closed set (three lists
//! and one get of an item by the id the program listed), no string of the
//! model's is in the command line, and the answer is rebuilt from named keys
//! (see `secrets::look`). The one thing below an item that is given is the
//! LABEL and kind of each of its fields (`fields_of`), so a reference to one
//! can be written; the field's value is dropped by construction. A failure is
//! told by class.
//!
//! With no token (an install whose secrets come from the environment or from
//! files) it says so and still lists where each `[secrets]` entry is read from.

use std::fmt::Write;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::secrets::look::{
    self as names, Failed, FieldName, ItemId, ItemName, Vault, VaultId, Verb,
};
use crate::secrets::{OpReader, SecretState, Source};
use crate::Core;

/// The parts of the look, in the order they are given. `fields` needs
/// `fields_of`, and says nothing without it.
const PARTS: [&str; 5] = ["account", "vaults", "items", "fields", "settings"];
/// How many item titles a look gives when it is not told, and the most it may.
pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 100;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    what: Option<Vec<String>>,
    #[serde(default)]
    vault: Option<String>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    fields_of: Option<String>,
}

/// What a call asks, checked.
pub(super) struct Ask {
    parts: Vec<&'static str>,
    vault: Option<String>,
    filter: Option<String>,
    limit: usize,
    /// The item whose field labels are wanted, by its title or id.
    fields_of: Option<String>,
}

pub(super) fn ask_of(input: &Value) -> Result<Ask, String> {
    let i: Input = serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
    let mut parts = Vec::new();
    for w in i.what.iter().flatten() {
        let part = PARTS.iter().find(|p| **p == w.trim()).ok_or_else(|| {
            format!(
                "`{w}` is not a part of the look: choose from {}",
                PARTS.join(", ")
            )
        })?;
        if !parts.contains(part) {
            parts.push(*part);
        }
    }
    let fields_of = i
        .fields_of
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if fields_of.is_some() && !parts.contains(&"fields") {
        parts.push("fields");
    }
    if parts.is_empty() {
        parts = PARTS.to_vec();
    }
    parts.sort_by_key(|p| PARTS.iter().position(|x| x == p));
    let trimmed = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Ok(Ask {
        parts,
        vault: trimmed(i.vault),
        filter: trimmed(i.filter),
        limit: i.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        fields_of,
    })
}

impl Ask {
    /// The call in a line, for its plan.
    pub(super) fn summary(&self) -> String {
        let mut s = self.parts.join(", ");
        if let Some(v) = &self.vault {
            s.push_str(&format!(" in {}", names::clean(v)));
        }
        if let Some(f) = &self.filter {
            s.push_str(&format!(" matching {}", names::clean(f)));
        }
        if let Some(i) = &self.fields_of {
            s.push_str(&format!(" (field names of {})", names::clean(i)));
        }
        s
    }
}

/// The tool's input schema.
pub(super) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "what": {
                "type": "array",
                "description": "Which parts: account, vaults, items, fields, settings. Omit for all.",
                "items": {"type": "string", "enum": PARTS}
            },
            "vault": {"type": "string", "description": "Only the items of this vault, by its name or id."},
            "filter": {"type": "string", "description": "Only items whose title contains this."},
            "limit": {"type": "integer", "description": "The most titles or field names to list (default 50, at most 100)."},
            "fields_of": {"type": "string", "description": "An item's title or id: lists the names and kinds of its fields (never a value), so a setting can read one. Narrow it with `vault` if two vaults have the title."}
        },
        "additionalProperties": false
    })
}

/// The vault name in a reference, and the item's: `op://vault/item/field`.
fn vault_and_item(reference: &str) -> Option<(String, String)> {
    let rest = reference.strip_prefix("op://")?;
    let mut segs = rest.split('/');
    let (vault, item) = (segs.next()?, segs.next()?);
    Some((names::clean(vault), names::clean(item)))
}

/// The vault `wanted` means among the ones `op` listed: by id, else by name
/// (any case). The id is the one `op` gave, so no string of the model's
/// reaches the command line.
fn vault_named<'a>(vaults: &'a [Vault], wanted: &str) -> Option<&'a Vault> {
    vaults
        .iter()
        .find(|v| v.id == wanted)
        .or_else(|| vaults.iter().find(|v| v.name.eq_ignore_ascii_case(wanted)))
}

fn said<T>(r: &Result<T, Failed>) -> Option<&'static str> {
    r.as_ref().err().map(|f| f.words())
}

/// Run a call: the words, or why not.
pub(super) async fn run(
    core: Arc<Core>,
    op: Option<Arc<OpReader>>,
    ask: Ask,
) -> Result<String, String> {
    let has = |p: &str| ask.parts.contains(&p);
    let mut out = String::from(
        "The vault, as the daemon sees it: names only, never a value, a field or a note.\n",
    );
    let no_token = op.as_ref().is_none_or(|o| o.has_no_vault());
    let reader = op.as_deref().filter(|o| !o.has_no_vault());
    if no_token {
        let _ = writeln!(out, "No vault session: {}.", Failed::NoVault.words());
    }
    let look = |v: Verb| async move {
        match reader {
            Some(r) => r.look(&v).await,
            None => Err(Failed::NoVault),
        }
    };
    let vaults = match (has("vaults") || ask.vault.is_some() || has("items")) && !no_token {
        true => Some(
            look(Verb::Vaults)
                .await
                .and_then(|r| names::parse_vaults(&r)),
        ),
        false => None,
    };
    if has("account") && !no_token {
        match look(Verb::Whoami)
            .await
            .and_then(|r| names::parse_account(&r))
        {
            Ok(a) => {
                let _ = writeln!(out, "Account: {} ({})", a.host, a.kind);
            }
            Err(f) => {
                let _ = writeln!(out, "Account: not available: {}.", f.words());
            }
        }
    }
    if has("vaults") {
        match &vaults {
            Some(Ok(v)) => {
                let _ = writeln!(out, "Vaults ({}):", v.len());
                for x in v {
                    let _ = writeln!(out, "- {} (id {})", x.name, x.id);
                }
            }
            Some(Err(f)) => {
                let _ = writeln!(out, "Vaults: not available: {}.", f.words());
            }
            None => {}
        }
    }
    let wants_fields = has("fields") && ask.fields_of.is_some();
    let items = match (has("items") || wants_fields) && !no_token {
        true => Some(items_of(&ask, &vaults, &look).await),
        false => None,
    };
    if let (true, Some(items)) = (has("items"), &items) {
        out.push_str(&items_words(&ask, items));
    }
    if let (true, Some(wanted)) = (wants_fields && !no_token, &ask.fields_of) {
        out.push_str(&fields_words(&ask, wanted, &items, reader).await);
    }
    if has("settings") {
        let seen = items.as_ref().and_then(|i| i.as_ref().ok());
        out.push_str(&settings(&core, seen.map(|(all, _)| all.as_slice())));
    }
    Ok(out)
}

/// The items asked for: every one the list gave (for the join with
/// `[secrets]`) and the vault they were asked of.
type Listed = Result<(Vec<ItemName>, Option<String>), String>;

async fn items_of<F, Fut>(
    ask: &Ask,
    vaults: &Option<Result<Vec<Vault>, Failed>>,
    look: &F,
) -> Listed
where
    F: Fn(Verb) -> Fut,
    Fut: std::future::Future<Output = Result<String, Failed>>,
{
    let mut scope = None;
    let mut which = None;
    if let Some(wanted) = &ask.vault {
        let Some(Ok(vs)) = vaults else {
            return Err(vaults
                .as_ref()
                .and_then(said)
                .unwrap_or("the vaults could not be listed")
                .to_string());
        };
        let Some(v) = vault_named(vs, wanted) else {
            let each: Vec<&str> = vs.iter().map(|v| v.name.as_str()).collect();
            return Err(format!(
                "no vault of that name: the vaults are {}",
                each.join(", ")
            ));
        };
        scope = VaultId::parse(&v.id);
        if scope.is_none() {
            return Err(Failed::Unreadable.words().to_string());
        }
        which = Some(v.name.clone());
    }
    let raw = look(Verb::Items(scope))
        .await
        .map_err(|f| f.words().to_string())?;
    let all = names::parse_items(&raw).map_err(|f| f.words().to_string())?;
    Ok((all, which))
}

/// The names and kinds of the fields of the one item `wanted` means (a listed
/// id, else a title in any case): never a value. Two items of one title are
/// not guessed between: their ids are given and the model names one.
async fn fields_words(
    ask: &Ask,
    wanted: &str,
    items: &Option<Listed>,
    reader: Option<&OpReader>,
) -> String {
    let Some(Ok((all, _))) = items else {
        let why = match items {
            Some(Err(why)) => why.as_str(),
            _ => "the items could not be listed",
        };
        return format!("Fields: not available: {why}.\n");
    };
    let mut hit: Vec<&ItemName> = all.iter().filter(|i| i.id == wanted).collect();
    if hit.is_empty() {
        hit = all
            .iter()
            .filter(|i| i.title.eq_ignore_ascii_case(wanted))
            .collect();
    }
    let shown = names::clean(wanted);
    let (item, id) = match hit.as_slice() {
        [] => {
            return format!(
                "Fields: no item of that title or id{}: list the items to see the titles.\n",
                ask.vault
                    .as_deref()
                    .map_or(String::new(), |v| format!(" in {}", names::clean(v)))
            )
        }
        [one] => match ItemId::parse(&one.id) {
            Some(id) => (*one, id),
            None => return format!("Fields: not available: {}.\n", Failed::Unreadable.words()),
        },
        many => {
            let each: Vec<String> = many
                .iter()
                .map(|i| format!("{} (id {}, in {})", i.title, i.id, i.vault))
                .collect();
            return format!(
                "Fields: {} items are titled \"{shown}\": {}. Ask again with the id of the one meant.\n",
                many.len(),
                each.join("; ")
            );
        }
    };
    let Some(reader) = reader else {
        return format!("Fields: not available: {}.\n", Failed::NoVault.words());
    };
    let fields = match reader.look_item(&id).await {
        Ok(text) => names::parse_fields(&text),
        Err(f) => Err(f),
    };
    match fields {
        Ok(f) => fields_list(ask, item, &f),
        Err(f) => format!("Fields: not available: {}.\n", f.words()),
    }
}

/// The fields, one to a line: the section (if any), the label and the kind.
fn fields_list(ask: &Ask, item: &ItemName, fields: &[FieldName]) -> String {
    let mut out = format!(
        "Fields of \"{}\" ({}, in {}): {} of {} shown (cap {}); names and kinds only, never a value:\n",
        item.title,
        item.category,
        item.vault,
        fields.len().min(ask.limit),
        fields.len(),
        ask.limit
    );
    for f in fields.iter().take(ask.limit) {
        match &f.section {
            Some(s) => {
                let _ = writeln!(out, "- {} › {} · {}", s, f.label, f.kind);
            }
            None => {
                let _ = writeln!(out, "- {} · {}", f.label, f.kind);
            }
        }
    }
    out
}

fn items_words(ask: &Ask, items: &Listed) -> String {
    let (all, which) = match items {
        Ok(x) => x,
        Err(why) => return format!("Items: not available: {why}.\n"),
    };
    let filter = ask.filter.as_deref().map(str::to_lowercase);
    let mut hit: Vec<&ItemName> = all
        .iter()
        .filter(|i| {
            filter
                .as_ref()
                .is_none_or(|f| i.title.to_lowercase().contains(f))
        })
        .collect();
    hit.sort_by(|a, b| (&a.vault, &a.title).cmp(&(&b.vault, &b.title)));
    let mut out = format!(
        "Items{}{}: {} of {} shown (cap {}{}):\n",
        which
            .as_deref()
            .map_or(String::new(), |w| format!(" in {w}")),
        ask.filter.as_deref().map_or(String::new(), |f| format!(
            " whose title has \"{}\"",
            names::clean(f)
        )),
        hit.len().min(ask.limit),
        hit.len(),
        ask.limit,
        match hit.len() > ask.limit {
            true => "; narrow it with a vault or part of a title to see the rest",
            false => "",
        }
    );
    for i in hit.iter().take(ask.limit) {
        let _ = writeln!(out, "- {} · {} · in {}", i.title, i.category, i.vault);
    }
    out
}

/// Which setting reads which item, resolved or not. The vault's items are
/// named by the reference in the settings, never by what `op` holds for them;
/// `seen`, when the items were listed, says whether the reference's item is
/// among them.
fn settings(core: &Core, seen: Option<&[ItemName]>) -> String {
    let mut out = String::from("Settings that read a secret (setting: where, state):\n");
    let states = core.secrets.states();
    for (name, reference) in &core.cfg.secrets {
        let state = match states.get(name) {
            Some(SecretState::Ready(_)) => "resolved",
            Some(SecretState::Resolving) => "still resolving",
            Some(SecretState::Failed(_)) => "not resolved",
            None => "unknown",
        };
        let from = match Source::of(reference) {
            Source::Env => "read from the environment".to_string(),
            Source::File => "read from a file".to_string(),
            Source::Vault => match vault_and_item(reference) {
                Some((v, i)) => {
                    let listed = seen.map(|all| {
                        all.iter().any(|x| {
                            x.title.eq_ignore_ascii_case(&i) && x.vault.eq_ignore_ascii_case(&v)
                        })
                    });
                    format!(
                        "item \"{i}\" in vault \"{v}\"{}",
                        match listed {
                            Some(true) => " (in the vault's list)",
                            Some(false) => " (not in the vault's list)",
                            None => "",
                        }
                    )
                }
                None => "an item in the vault".to_string(),
            },
        };
        let _ = writeln!(out, "- secrets.{name}: {from} · {state}");
    }
    out
}

#[cfg(test)]
mod tests;
