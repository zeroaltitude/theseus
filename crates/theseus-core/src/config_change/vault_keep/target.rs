//! What a keep is about, found by the daemon in the vault's own lists (issue
//! 26): the vault, the item and the field, as the owner will see them on the
//! card, and whether the keep makes a new item, adds a field to one, or
//! REPLACES a value. The turn names things in words; nothing it wrote reaches
//! the program's command line (the vault and item are named to it by the ids it
//! listed, as `look.vault` does).

use serde_json::{json, Value};

use crate::config_change::vault_use::check::{find_item, not_seen, writable};
use crate::secrets::look::{
    parse_fields, parse_items, parse_vaults, Failed, FieldName, ItemId, ItemName, Permission,
    Vault, VaultId, Verb,
};
use crate::secrets::{OpReader, SecretRef, Source};

/// What the keep does to the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No item has that title: a new one is made.
    New,
    /// The item is there and has no such field: a field is added to it.
    Adds,
    /// The field is there: its value is REPLACED.
    Replaces,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::New => "new",
            State::Adds => "adds",
            State::Replaces => "replaces",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [State::New, State::Adds, State::Replaces]
            .into_iter()
            .find(|x| x.as_str() == s)
    }
}

/// The one place a secret is kept, by name and by the ids the vault gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub vault: String,
    pub vault_id: String,
    pub title: String,
    /// None for an item that does not exist yet.
    pub item_id: Option<String>,
    pub field: String,
    /// None for a field that does not exist yet.
    pub field_id: Option<String>,
    pub state: State,
    /// Theseus's own settings read this item (a `[secrets]` entry, or the
    /// settings note): a turn may not replace it, the owner does from their terminal.
    pub protected: bool,
}

impl Target {
    /// What the question keeps: names and ids, never a value.
    pub fn args(&self) -> Value {
        json!({
            "vault": self.vault, "vault_id": self.vault_id,
            "title": self.title, "item_id": self.item_id,
            "field": self.field, "field_id": self.field_id,
            "state": self.state.as_str(), "protected": self.protected,
        })
    }

    /// The target a question recorded, or none for anything else.
    pub fn of_args(keep: &Value) -> Option<Self> {
        let text = |k: &str| keep[k].as_str().map(str::to_string);
        Some(Self {
            vault: text("vault")?,
            vault_id: text("vault_id")?,
            title: text("title")?,
            item_id: text("item_id"),
            field: text("field")?,
            field_id: text("field_id"),
            state: State::parse(keep["state"].as_str()?)?,
            protected: keep["protected"].as_bool().unwrap_or(true),
        })
    }

    /// Whether an existing value is replaced: asked for a fresh code every time.
    pub fn replaces(&self) -> bool {
        self.state == State::Replaces
    }
}

/// What reads from the vault in Theseus's own settings: each `[secrets]` entry
/// that is a vault reference, in the settings running and in the file as
/// written, and the settings note's.
pub fn readers(core: &crate::Core) -> Vec<String> {
    let note = core.config_gate.reference();
    // The settings file as written, too: a use written there but not live until the next start
    // is read then (issue 54).
    let written: Vec<String> = core
        .tools
        .config_change
        .file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| crate::Config::parse(&t).ok())
        .map(|(c, _)| c.secrets.into_values().collect())
        .unwrap_or_default();
    core.cfg
        .secrets
        .values()
        .cloned()
        .chain(written)
        .chain(Some(note))
        .filter(|r| Source::of(r) == Source::Vault && r.starts_with("op://"))
        .collect()
}

/// Whether one of `refs` names this vault and item, by name or by id.
fn is_read(refs: &[String], v: &Vault, item: &ItemName) -> bool {
    let same = |a: &str, b: &str, id: &str| a.eq_ignore_ascii_case(b) || a == id;
    refs.iter()
        .filter_map(|r| SecretRef::parse(r).ok())
        .any(|r| same(&r.vault, &v.name, &v.id) && same(&r.item, &item.title, &item.id))
}

fn said(f: Failed) -> String {
    f.words().to_string()
}

/// Whether the daemon's account may write in `v`: the vault's own program is
/// asked which vaults hold `p` for it, each time (nothing is remembered, so a
/// permission taken away since a card was put is found at the answer). An
/// answer that cannot be had is no yes.
async fn may_write(reader: &OpReader, v: &Vault, p: Permission) -> Result<(), String> {
    let held = parse_vaults(&reader.look(&Verb::VaultsWith(p)).await.map_err(|f| {
        format!(
            "could not check whether Theseus may write in the vault \"{}\": {}",
            v.name,
            f.words()
        )
    })?)
    .map_err(said)?;
    match held.iter().any(|h| h.id == v.id) {
        true => Ok(()),
        false => Err(cannot_write(&v.name, p)),
    }
}

/// How the words begin when the account may not write in a vault.
pub const CANNOT_WRITE: &str = "Theseus's vault account may not";

/// Whether the account may still write where `t` says, asked of the vault's
/// program now: the words that say so when it may not.
pub async fn still_writable(reader: &OpReader, t: &Target) -> Result<(), String> {
    let v = Vault {
        id: t.vault_id.clone(),
        name: t.vault.clone(),
    };
    let p = match t.state {
        State::New => Permission::CreateItems,
        _ => Permission::EditItems,
    };
    may_write(reader, &v, p).await
}

/// What the owner is told when the account may not write in a vault, and what
/// they can do.
pub fn cannot_write(vault: &str, p: Permission) -> String {
    let may = match p {
        Permission::CreateItems => "make a new item",
        Permission::EditItems => "change an item",
    };
    format!(
        "{CANNOT_WRITE} {may} in the vault \"{vault}\". Nothing was \
         kept. Give the account write access to that vault in the vault's own \
         settings, or choose another vault"
    )
}

/// Find the target of a keep: what the names mean in the vault, or why they
/// mean nothing. A vault the account cannot see is told with what the owner
/// can do; two items of one title are not guessed between.
pub async fn resolve(
    reader: &OpReader,
    refs: &[String],
    (vault, title, field): (&str, &str, &str),
) -> Result<Target, String> {
    for (what, name) in [("vault", vault), ("item", title), ("field", field)] {
        // A name begins with a letter or a digit: never a flag's dash, a path's dot or a space.
        if !writable(name) || !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return Err(format!(
                "the {what} name has a character a name here cannot carry (it begins with a \
                 letter or a digit, then letters, digits, spaces, `-`, `_` and `.`, at most 64): \
                 give it plainly"
            ));
        }
    }
    let vaults = parse_vaults(&reader.look(&Verb::Vaults).await.map_err(said)?).map_err(said)?;
    let Some(v) = crate::look::vault_named(&vaults, vault) else {
        return Err(not_seen(vault, &vaults));
    };
    let scope = VaultId::parse(&v.id).ok_or_else(|| said(Failed::Unreadable))?;
    let items = parse_items(
        &reader
            .look(&Verb::Items(Some(scope.clone())))
            .await
            .map_err(said)?,
    )
    .map_err(said)?;
    let mut target = Target {
        vault: v.name.clone(),
        vault_id: v.id.clone(),
        title: title.to_string(),
        item_id: None,
        field: field.to_string(),
        field_id: None,
        state: State::New,
        protected: false,
    };
    let Some(item) = find_item(&items, &v.name, title)? else {
        may_write(reader, v, Permission::CreateItems).await?;
        return Ok(target);
    };
    may_write(reader, v, Permission::EditItems).await?;
    target.protected = is_read(refs, v, item);
    target.title = item.title.clone();
    target.item_id = Some(item.id.clone());
    let id = ItemId::parse(&item.id).ok_or_else(|| said(Failed::Unreadable))?;
    let fields = match reader.look_item(&scope, &id).await {
        Ok(text) => parse_fields(&text).map_err(said)?,
        Err(f) => return Err(said(f)),
    };
    let named: Vec<&FieldName> = fields
        .iter()
        .filter(|f| f.label.eq_ignore_ascii_case(field) || f.id.eq_ignore_ascii_case(field))
        .collect();
    match named.as_slice() {
        [] => target.state = State::Adds,
        [one] => {
            if one.kind != "concealed" || one.section.is_some() {
                return Err(format!(
                    "\"{}\" has a field called \"{field}\" that is {}: Theseus keeps a secret in \
                     a concealed field with no section, so it will not write there. Name another \
                     field, or change that one in the vault",
                    item.title,
                    match one.section.is_some() {
                        true => "in a section".to_string(),
                        false => format!("a {} field", one.kind),
                    }
                ));
            }
            target.field = one.label.clone();
            target.field_id = Some(one.id.clone());
            target.state = State::Replaces;
        }
        many => {
            return Err(format!(
                "\"{}\" has {} fields called \"{field}\": Theseus will not guess which one to \
                 replace; the owner renames one in the vault",
                item.title,
                many.len()
            ))
        }
    }
    Ok(target)
}
