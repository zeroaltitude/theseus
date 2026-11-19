//! The write of a keep, what it is told as, and the row it leaves (issue 26).
//!
//! The value is in this file for the length of one call: `perform` takes it by
//! reference, hands it to the vault's program on **stdin** (`OpReader::keep_field`),
//! reads the field back and compares by digest, and returns an [`Outcome`] that
//! holds no value. What the owner is told, and what the ledger row says, are
//! built from the target's names and the outcome's class and nothing else.

use serde_json::{json, Value};

use super::target::{State, Target};
use crate::secrets::look::Failed;
use crate::secrets::write::KeepStop;
use crate::secrets::{OpReader, Secret};

/// The longest a value may be: a key is not a document.
pub const MAX_VALUE: usize = 4_096;

/// How a keep ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Written, and read back as written.
    Kept,
    /// Nothing was written.
    NotWritten(Failed),
    /// The vault took it and reading it back failed: the item may exist.
    NotChecked(Failed),
    /// The vault took it and it read back as something else.
    Differs,
    /// The target is not what the card showed: nothing was written.
    Changed,
}

impl Outcome {
    /// The row's `outcome`: a closed set of words.
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Kept => "kept",
            Outcome::NotWritten(_) => "not_written",
            Outcome::NotChecked(_) => "unverified",
            Outcome::Differs => "differs",
            Outcome::Changed => "changed",
        }
    }

    pub fn written(self) -> bool {
        self == Outcome::Kept
    }
}

/// A value that may be kept, trimmed: or why not, before anything is asked of
/// the vault.
pub fn checked(raw: &str) -> Result<Secret, String> {
    let v = raw.trim();
    if v.is_empty() {
        return Err("no value was given: type it where only you see it".into());
    }
    if v.len() > MAX_VALUE || v.contains('\0') {
        return Err(format!(
            "that is not a secret's value (a key is at most {MAX_VALUE} bytes, and no document): \
             nothing was written"
        ));
    }
    Ok(Secret::new(v.to_string()))
}

/// What reads the field back: by the ids the vault gave for an item that is
/// there, by the title for one that is not yet.
fn reference(t: &Target) -> String {
    match (&t.item_id, &t.field_id) {
        (Some(item), Some(field)) => format!("op://{}/{item}/{field}", t.vault_id),
        (Some(item), None) => format!("op://{}/{item}/{}", t.vault_id, t.field),
        (None, _) => format!("op://{}/{}/{}", t.vault_id, t.title, t.field),
    }
}

/// Write `value` where `t` says and read it back. The caller has resolved `t`
/// just now (a card's answer checks it is still what the card showed first).
pub async fn perform(reader: &OpReader, t: &Target, value: &Secret) -> Outcome {
    let item = t.item_id.as_deref().unwrap_or(&t.title);
    let field_id = t.field_id.as_deref().unwrap_or(&t.field);
    match reader
        .keep_field(
            (&t.vault_id, item, t.item_id.is_some()),
            (field_id, &t.field),
            &reference(t),
            value,
        )
        .await
    {
        Ok(()) => Outcome::Kept,
        Err(KeepStop::NotWritten(f)) => Outcome::NotWritten(f),
        Err(KeepStop::NotChecked(f)) => Outcome::NotChecked(f),
        Err(KeepStop::Differs) => Outcome::Differs,
    }
}

/// What the owner is told, by name: where it is, or what to check.
pub fn words(t: &Target, o: Outcome) -> String {
    let at = format!(
        "the vault \"{}\", item \"{}\", field \"{}\"",
        t.vault, t.title, t.field
    );
    match o {
        Outcome::Kept => format!(
            "Kept in {at}: it reads back as written.{} Ask Theseus to use it by that name.",
            match t.state {
                State::Replaces => " It replaced the value that was there.",
                _ => "",
            }
        ),
        Outcome::NotWritten(f) => format!(
            "Nothing was kept in {at}: {}. {}",
            f.words(),
            match f {
                // A killed program may have been done before it was killed.
                Failed::TimedOut =>
                    "The vault may have been changed anyway: check the item before asking again.",
                _ => "The vault was not changed.",
            }
        ),
        Outcome::NotChecked(f) => format!(
            "The vault accepted a value for {at}, but reading it back failed ({}). The item \
             exists or was changed: check it in the vault before relying on it.",
            f.words()
        ),
        Outcome::Differs => format!(
            "The vault accepted a value for {at}, but it did NOT read back as written. The item \
             exists or was changed: check it in the vault before relying on it."
        ),
        Outcome::Changed => format!(
            "{at} is not what the card showed any more (the item or the field changed in the \
             vault), so nothing was written. Ask for the keep again."
        ),
    }
}

/// The ledger row `vault.keep`: who, where, which target by name, and how it
/// ended. Never the value, and not its digest.
pub fn row(t: &Target, o: Outcome, by: &str, via: &str, question: Option<&str>) -> Value {
    let mut row = json!({
        "outcome": o.name(), "state": t.state.as_str(),
        "vault": t.vault, "title": t.title, "field": t.field,
        "protected": t.protected, "approved_by": by, "via": via,
    });
    if let Some(q) = question {
        row["question"] = q.into();
    }
    row
}
