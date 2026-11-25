//! A use of a vault item that is told, not asked (issue 54; the owner, 2026-11-25: "using a secret
//! must not ask for 2fa", and "the approve is really just for 'i think something truly dangerous is
//! happening that can't be reversed if I do it without permission'").
//!
//! When every setting the use changes is the secret's own entry (`secrets.<name>`, new or
//! re-pointed) or a setting outside the sensitive tables that comes to read it (its value is the
//! name), the daemon writes the change itself, by the path a card's yes takes (the previous file
//! kept as a numbered copy, the file written whole, a `config.changed` row), and tells the owner
//! in one FYI note: the item and field, the name, what reads it, that it takes effect at the next
//! restart, and the undo, which puts back each setting the use touched from that kept copy.
//!
//! Any other setting in `also` (a provider's address beside its key, a setting that has nothing to
//! do with the secret, a sensitive table's reader) would let a steered turn choose where the value
//! goes or loosen something else, so that use keeps the card it had, with its second proof where
//! the setting is sensitive. No restart is started here: every setting needs one
//! (`sensitivity::LIVE` is empty), and only the owner starts it.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use super::super::{diff, edit, write};
use super::Chosen;
use crate::config_copy::sha256;
use crate::ledger::LedgerRow;
use crate::toolrun::{ToolRuntime, TurnCtx};

/// Who approved a told use, in its `config.changed` row and the history.
pub const TOLD: &str = "told: the owner's own turn";

/// Whether a use that changes `settings` is told: each is the secret's own entry, or a reader of it
/// ([`ours`]).
pub fn tells(settings: &[diff::Setting], name: &str) -> bool {
    !settings.is_empty() && settings.iter().all(|s| ours(s, name))
}

/// Whether `s` is the secret `name`'s own entry, or a setting of an ordinary table that comes to
/// read it and is sensitive only because it names where a secret is read from (a provider's
/// `api_key_secret`). Its second proof is not asked for either; anything else keeps its own.
pub fn ours(s: &diff::Setting, name: &str) -> bool {
    let own = s.path.len() == 2 && s.path[0] == "secrets" && s.path[1] == name;
    let reads = match s.after.as_ref() {
        Some(Value::String(v)) => v == name,
        Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some(name)),
        _ => false,
    };
    let names_a_secret = matches!(
        crate::config::sensitivity::reason(&s.path).as_deref(),
        None | Some(SECRET_KEY)
    );
    own || (reads && names_a_secret)
}

/// What `sensitivity::key_reason` says of a key that names where a secret is read from.
const SECRET_KEY: &str = "where a secret is read from";

/// What a restart makes live of the secrets told uses wrote since this start, in words: each name
/// the settings file still points where its use pointed it, its reference, and what reads it.
/// Names and references only.
pub(crate) fn waiting(w: &super::super::Where) -> Vec<String> {
    let told = w.told.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let Some(file) = w.file.as_ref().filter(|_| !told.is_empty()) else {
        return Vec::new();
    };
    let Some((written, _)) = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| crate::Config::parse(&t).ok())
    else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (name, reference) in told.iter().rev() {
        if !seen.insert(name.clone()) || written.secrets.get(name) != Some(reference) {
            continue;
        }
        let readers = super::given_to(&written, name);
        out.push(match readers.is_empty() {
            true => format!("the secret `{name}` is read from {reference}, and nothing reads it yet"),
            false => format!(
                "the secret `{name}` is read from {reference} by {}",
                readers.join(" and ")
            ),
        });
    }
    out.reverse();
    out
}

/// The note: FYI-headed, the conversation, what now reads where (and, for a re-pointed name, what
/// it read before), that a restart is needed, and the undo. Names and references only, never the
/// value.
pub(crate) fn note_text(
    title: Option<&str>,
    used: &Chosen,
    before: Option<&str>,
    readers: &[String],
    number: u32,
) -> String {
    let c = &used.checked;
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
    let given = match readers.is_empty() {
        true => "Nothing is given it yet.".to_string(),
        false => format!("Given to: {}.", readers.join("; ")),
    };
    let was = before.map_or(String::new(), |b| {
        format!(" It was re-pointed: until now `{}` was read from {b}.", used.name)
    });
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        match before.is_some() {
            true => "Theseus pointed a secret at another vault item",
            false => "Theseus put a secret from the vault to use",
        },
        &format!(
            "{}, at your word, without a card:",
            crate::judge::onto::words::in_conversation(title)
        ),
        &format!(
            "The item \"{}\", field \"{}\"{section}, in the vault \"{}\", is kept under the name \
             `{}`, read from {}.{was}{by_id} {given} Its value was read once to check it and is shown \
             nowhere.\nIt takes effect when Theseus next restarts; nothing restarts it but you.\n\
             Undo: ask Theseus to undo settings change {number} (`config.undo`), which puts back \
             each setting this changed; `theseus config history` lists it.",
            c.item, c.field, c.vault, used.name, c.reference
        ),
    )
}

/// Write the use and tell the owner. The settings file must still be `base`.
pub(super) fn put_told(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    correlation_id: &str,
    base: &str,
    changes: &[edit::Change],
    used: &Chosen,
) -> Result<(String, Value), String> {
    let w = &rt.config_change;
    let candidate = edit::apply(base, changes)?;
    let (loaded, _) = crate::Config::parse(&candidate)
        .map_err(|e| format!("the settings with that change do not load: {e:#}"))?;
    let settings = diff::between(base, &candidate)?;
    if settings.is_empty() {
        return Err("that would change nothing: the file already has those values".into());
    }
    super::refuse_other_references(&settings, Some(used))?;
    let file = w
        .file
        .clone()
        .ok_or("the settings are not a file this daemon can change")?;
    let now = std::fs::read_to_string(&file)
        .map_err(|e| format!("the settings file cannot be read: {e}"))?;
    if sha256(&now) != sha256(base) {
        return Err(
            "the settings file changed while the vault was read, so nothing was written: ask \
             again"
                .into(),
        );
    }
    let before = crate::config_change::history::doc_of(base)
        .and_then(|d| {
            crate::config_change::history::value_at(&d, &["secrets".into(), used.name.clone()])
        })
        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string));
    let (number, _) = write::keep_previous(&w.state, &now).map_err(|e| {
        format!("the previous settings could not be kept, so nothing was written: {e}")
    })?;
    write::write_config(&file, &candidate)
        .map_err(|e| format!("the settings could not be written: {e}"))?;
    let paths: Vec<String> = settings.iter().map(|s| edit::dotted(&s.path)).collect();
    let row = LedgerRow::new(
        LedgerKind::ConfigChanged,
        Some(tc.session_id),
        None,
        json!({
            "source": "owner", "question": correlation_id, "paths": paths,
            "asked_by": {
                "session_id": tc.session_id,
                "execution_id": tc.execution_id,
                "turn_id": tc.turn_id,
                "place": tc.outbox.target(tc.session_id),
            },
            "approved_by": TOLD, "via": super::USE,
            "digest_before": sha256(&now), "digest_after": sha256(&candidate),
            "previous_copy": number, "restart": false, "restart_only": false,
            "file": Value::Null, "told": true, "secret": used.args(),
        }),
    );
    let rec = NewRecord::json(kinds::LEDGER, None, &row)
        .map_err(|e| format!("the change was written, and its record could not be made: {e}"))?;
    if let Err(e) = tc.store.defer(rec) {
        tracing::warn!(error = %format!("{e:#}"), "vault.use: the change's row was not kept");
    }
    let readers = super::given_to(&loaded, &used.name);
    let title = tc
        .store
        .get_session::<crate::session::SessionRecord>(tc.session_id)
        .ok()
        .flatten()
        .and_then(|r| r.label.filter(|l| !l.is_empty()).or(r.title));
    w.told
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((used.name.clone(), used.checked.reference.clone()));
    let text = note_text(title.as_deref(), used, before.as_deref(), &readers, number);
    crate::config_change::hold::tell_owner(tc, &text, super::USE);
    let said = format!(
        "Done, and the owner is told (no card): `secrets.{}` now reads the vault's item \"{}\", \
         field \"{}\"; {}. It takes effect when Theseus next restarts, which only the owner starts \
         (`daemon.restart` asks them). The value went to no turn, note or record. This is \
         settings change {number}; `config.undo` puts it back.",
        used.name,
        used.checked.item,
        used.checked.field,
        match readers.is_empty() {
            true => "nothing reads it yet".to_string(),
            false => format!("read by {}", readers.join("; ")),
        }
    );
    Ok((said, json!({"settings": settings.len(), "told": true, "change": number})))
}
