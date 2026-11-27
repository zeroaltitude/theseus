//! The daemon's check of an item and a field, before a card names them (issue
//! 26): the vault is one the account sees, the item is one item, the field is
//! one field, and the value can be read. Yes or no; never the value.
//!
//! **The reference is built here, from what the vault listed, and never taken
//! from the model.** The model names a vault, an item and a field in words; the
//! daemon finds each in the vault's own lists, by name or by the id the vault
//! gave, and writes `op://vault/item/field` from the names it found (or from
//! their ids where a name has a character a reference cannot carry). So the
//! card names what the daemon will read, a title that looks like a reference
//! finds nothing, two items of one title are not guessed between, and a `/`, a
//! `?` or a `#` in a name cannot make the reference mean another thing.
//!
//! The last step reads the built reference once, as the daemon will at start:
//! the one proof that the item and field are there, the account can read them
//! and the field holds something. The card form drops what it read; the run form
//! (issue 64) keeps it, as [`Read`], for the one run it is given to. A failure is
//! told by class, never in the program's own words.

use crate::secrets::look::{
    class_of, parse_fields, parse_items, parse_vaults, Failed, FieldName, ItemId, ItemName, Vault,
    VaultId, Verb,
};
use crate::secrets::{OpReader, Secret, SecretRef};

/// What was found, in the names the owner would recognise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub vault: String,
    pub item: String,
    pub field: String,
    pub section: Option<String>,
    /// The reference the daemon built, which is what a start will read.
    pub reference: String,
    /// Whether any part of it is written as an id, not a name.
    pub by_id: bool,
}

/// What a vault the account cannot see is told, with what the owner can do.
pub(crate) fn not_seen(wanted: &str, seen: &[Vault]) -> String {
    let each: Vec<&str> = seen.iter().map(|v| v.name.as_str()).collect();
    format!(
        "Theseus's vault account does not see a vault called \"{}\"; it sees {}. The owner can \
         grant the account that vault, or move the item to one it sees (the vault's maker does \
         not let a service account into a personal vault).",
        crate::secrets::look::clean(wanted),
        match each.is_empty() {
            true => "no vault".to_string(),
            false => each.join(", "),
        }
    )
}

/// Whether a name can be written in a reference as it is: the characters the
/// vault's own references allow in a name (letters, digits, spaces, `-`, `_`
/// and `.`), at most 64 of them, with no space at either end. Anything else
/// (a `/`, a `#`, a `?`, a quote, a line break) is written by its id.
pub(crate) fn writable(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 64
        && name == name.trim()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
}

/// The segment for one thing: its name when that can be written and no sibling
/// has it too (the vault's program matches names ignoring case, and needs the
/// id when a name is shared), else its id.
fn segment(name: &str, id: &str, unique: bool, by_id: &mut bool) -> Option<String> {
    if unique && writable(name) {
        return Some(name.to_string());
    }
    if writable(id) {
        *by_id = true;
        return Some(id.to_string());
    }
    None
}

fn said(f: Failed) -> String {
    f.words().to_string()
}

/// A read's failure, by class: what the program said is not returned.
fn read_failure(e: &anyhow::Error) -> String {
    let text = format!("{e:#}");
    if text.contains("empty value") {
        return "the field is empty, so there is nothing for Theseus to read there".into();
    }
    if text.contains("did not answer") {
        return said(Failed::TimedOut);
    }
    said(class_of(text.as_bytes()))
}

/// The one item `wanted` means among the vault's `items`: by the id the vault
/// gave, else by title in any case. `Ok(None)` when none has it; two of one
/// title are not guessed between, and the ids are told.
pub(crate) fn find_item<'a>(
    items: &'a [ItemName],
    vault: &str,
    wanted: &str,
) -> Result<Option<&'a ItemName>, String> {
    let mut hit: Vec<&ItemName> = items.iter().filter(|i| i.id == wanted).collect();
    if hit.is_empty() {
        hit = items
            .iter()
            .filter(|i| i.title.eq_ignore_ascii_case(wanted))
            .collect();
    }
    match hit.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        many => {
            let each: Vec<String> = many
                .iter()
                .map(|i| format!("{} (id {})", i.title, i.id))
                .collect();
            Err(format!(
                "{} items in \"{vault}\" are titled \"{}\": {}. Ask again with the id of the one meant.",
                many.len(),
                crate::secrets::look::clean(wanted),
                each.join("; ")
            ))
        }
    }
}

/// What the run form's check found: the names, the value it read, and the vault's ids for the
/// vault and the item.
pub(crate) struct Read {
    pub checked: Checked,
    pub value: Secret,
    pub vault_id: String,
    pub item_id: String,
}

/// Check what the model named and build the reference, or say why not, in
/// words for the model and the owner.
pub async fn check(
    reader: &OpReader,
    vault: &str,
    item: &str,
    field: &str,
    section: Option<&str>,
) -> Result<Checked, String> {
    // The proof: the daemon reads it as a start will, and keeps nothing.
    reading(reader, vault, item, field, section)
        .await
        .map(|r| r.checked)
}

/// [`check`], keeping what it read.
pub(crate) async fn reading(
    reader: &OpReader,
    vault: &str,
    item: &str,
    field: &str,
    section: Option<&str>,
) -> Result<Read, String> {
    let list = |v: Verb| async move { reader.look(&v).await };
    let vaults = parse_vaults(&list(Verb::Vaults).await.map_err(said)?).map_err(said)?;
    let Some(v) = crate::look::vault_named(&vaults, vault) else {
        return Err(not_seen(vault, &vaults));
    };
    let scope = VaultId::parse(&v.id).ok_or_else(|| said(Failed::Unreadable))?;
    let items =
        parse_items(&list(Verb::Items(Some(scope.clone()))).await.map_err(said)?).map_err(said)?;
    let Some(found) = find_item(&items, &v.name, item)? else {
        return Err(format!(
            "the vault \"{}\" has no item of that title: look at its items first",
            v.name
        ));
    };
    let item_id = ItemId::parse(&found.id).ok_or_else(|| said(Failed::Unreadable))?;
    let fields = match reader.look_item(&scope, &item_id).await {
        Ok(text) => parse_fields(&text).map_err(said)?,
        Err(f) => return Err(said(f)),
    };
    let field = pick_field(&fields, field, section, &found.title)?;
    let (reference, by_id) = build_reference(&vaults, v, &items, found, &fields, field)?;
    let parsed = SecretRef::parse(&reference).map_err(|_| said(Failed::Unreadable))?;
    let value = match reader.read(&parsed).await {
        Ok(value) => value,
        Err(e) => return Err(read_failure(&e)),
    };
    Ok(Read {
        checked: Checked {
            vault: v.name.clone(),
            item: found.title.clone(),
            field: field.label.clone(),
            section: field.section.clone(),
            reference,
            by_id,
        },
        value,
        vault_id: v.id.clone(),
        item_id: found.id.clone(),
    })
}

/// The reference for the one field found: each part written by its name, or by
/// its id where the name is not one exactly one sibling answers to (the vault's
/// program matches names ignoring case, and is not assumed to look in a field's
/// own section first). And whether any part is an id.
fn build_reference(
    vaults: &[Vault],
    v: &Vault,
    items: &[ItemName],
    found: &ItemName,
    fields: &[FieldName],
    field: &FieldName,
) -> Result<(String, bool), String> {
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let mut by_id = false;
    let mut parts = vec![
        segment(
            &v.name,
            &v.id,
            vaults.iter().filter(|x| same(&x.name, &v.name)).count() == 1,
            &mut by_id,
        ),
        segment(
            &found.title,
            &found.id,
            items
                .iter()
                .filter(|x| same(&x.title, &found.title))
                .count()
                == 1,
            &mut by_id,
        ),
    ];
    let unnamed = || {
        "two fields or sections of that item answer to the same name (a label or an id, in any \
         case), or a name has a character a reference cannot carry, and the vault gave no other \
         plain name for it: the owner renames one in the vault"
            .to_string()
    };
    // The section, when the field has one: written by its label, or its id, only
    // when exactly one section answers to it.
    let sections: Vec<(&str, &str)> = fields
        .iter()
        .filter_map(|f| Some((f.section.as_deref()?, f.section_id.as_deref()?)))
        .collect();
    if let Some(s) = &field.section {
        let sid = field.section_id.as_deref().unwrap_or_default();
        let one = |c: &str| {
            let hits: std::collections::BTreeSet<&str> = sections
                .iter()
                .filter(|(l, i)| same(l, c) || same(i, c))
                .map(|(_, i)| *i)
                .collect();
            writable(c) && hits.len() == 1
        };
        match (one(s), one(sid)) {
            (true, _) => parts.push(Some(s.clone())),
            (false, true) => {
                by_id = true;
                parts.push(Some(sid.to_string()));
            }
            _ => return Err(unnamed()),
        }
    }
    // The field: written by its label, or its id, only when exactly one field
    // answers to it among the fields the reference reaches (the section's when a
    // section is written, else every field of the item, since the vault's program
    // is not assumed to look in the sectionless ones first).
    let reached: Vec<&FieldName> = fields
        .iter()
        .filter(|f| field.section.is_none() || f.section_id == field.section_id)
        .collect();
    let one = |c: &str| {
        writable(c)
            && reached
                .iter()
                .filter(|f| same(&f.label, c) || same(&f.id, c))
                .count()
                == 1
    };
    match (one(&field.label), one(&field.id)) {
        (true, _) => parts.push(Some(field.label.clone())),
        (false, true) => {
            by_id = true;
            parts.push(Some(field.id.clone()));
        }
        _ => return Err(unnamed()),
    }
    let Some(parts) = parts.into_iter().collect::<Option<Vec<String>>>() else {
        return Err(
            "a name in that path has a character a reference cannot carry (a slash, a question \
             mark or a hash) and the vault gave no plain id for it: the owner renames it in \
             the vault"
                .into(),
        );
    };
    let reference = format!("op://{}", parts.join("/"));
    Ok((reference, by_id))
}

/// The one field the model meant: by its label (any case), and its section when
/// the label is in more than one. Not guessed between.
fn pick_field<'a>(
    fields: &'a [FieldName],
    label: &str,
    section: Option<&str>,
    title: &str,
) -> Result<&'a FieldName, String> {
    let named: Vec<&FieldName> = fields
        .iter()
        .filter(|f| f.label.eq_ignore_ascii_case(label) || f.id == label)
        .filter(|f| {
            section.is_none_or(|s| {
                f.section
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(s))
            })
        })
        .collect();
    // An id given exactly is the one field that has it, even when another's label is the same word.
    let exact: Vec<&FieldName> = named.iter().copied().filter(|f| f.id == label).collect();
    let named = match (named.len(), exact.len()) {
        (n, 1) if n > 1 => exact,
        _ => named,
    };
    match named.as_slice() {
        [one] => Ok(one),
        [] => {
            let each: Vec<String> = fields.iter().take(20).map(|f| f.label.clone()).collect();
            Err(format!(
                "\"{title}\" has no field called \"{}\"{}: its fields are {}",
                crate::secrets::look::clean(label),
                section.map_or(String::new(), |s| format!(
                    " in a section called \"{}\"",
                    crate::secrets::look::clean(s)
                )),
                each.join(", ")
            ))
        }
        many => {
            let each: Vec<String> = many
                .iter()
                .map(|f| match &f.section {
                    Some(s) => format!("section \"{s}\""),
                    None => "no section".to_string(),
                })
                .collect();
            Err(format!(
                "\"{title}\" has {} fields called \"{}\" ({}): ask again with the section of the one meant",
                many.len(),
                crate::secrets::look::clean(label),
                each.join(", ")
            ))
        }
    }
}
