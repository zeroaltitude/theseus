//! A use of a vault item that is told, not asked (issue 54): only a NEW secret name that nothing
//! reads (`classify::Case::Inert`). Nothing reads it, so it is inert at any restart; every other
//! use is a card (`classify.rs`). The owner, 2026-11-25: "using a secret must not ask for 2fa",
//! and "the approve is really just for 'i think something truly dangerous is happening that
//! can't be reversed if I do it without permission'".
//!
//! The daemon writes the entry by the settings' one writer (`answer::one_write`: the lock a card's
//! yes holds, never while a restart is under way, and the base digest checked under it): the
//! previous file kept as a numbered copy, the file written whole, a `config.changed` row. It tells
//! the owner in the shared told note (`owner_note::told`), whose Undo removes exactly that entry
//! (`put_back.rs`). The note names the vault, the item and the field, so it is a `told_dm`: it
//! stays in his DM and never goes to a notices place.

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

/// The note's title, its second line (the conversation) and its body: what was kept under which
/// name, that nothing reads it, and that a restart does not change that. `told::stage_dm` adds the
/// Undo. Names and references only, never the value.
pub(crate) fn note_parts(
    title: Option<&str>,
    used: &Chosen,
    number: u32,
) -> (&'static str, String, String) {
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
    (
        "Theseus kept a vault secret's name in its settings",
        format!(
            "{}, at your word, without a card:",
            crate::judge::onto::words::in_conversation(title)
        ),
        format!(
            "The item \"{}\", field \"{}\"{section}, in the vault \"{}\", is kept under the new name \
             `{}`, read from {}.{by_id} Nothing reads it: no setting is given it, so no restart \
             sends it anywhere. Giving it to a setting asks you on a card. Its value was read once \
             to check it and is shown nowhere.\nIts Undo removes exactly this entry; \
             `theseus config history` lists it as change #{number}.",
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
    crate::Config::parse(&candidate)
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
    let number = {
        // The settings' one writer, as a card's yes: the base re-read and checked under it.
        let _one = super::super::answer::one_write(w)?;
        let now = std::fs::read_to_string(&file)
            .map_err(|e| format!("the settings file cannot be read: {e}"))?;
        if sha256(&now) != sha256(base) {
            return Err(
                "the settings file changed while the vault was read, so nothing was written: \
                 ask again"
                    .into(),
            );
        }
        let (number, _) = write::keep_previous(&w.state, &now).map_err(|e| {
            format!("the previous settings could not be kept, so nothing was written: {e}")
        })?;
        write::write_config(&file, &candidate)
            .map_err(|e| format!("the settings could not be written: {e}"))?;
        number
    };
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
            "digest_before": sha256(base), "digest_after": sha256(&candidate),
            "previous_copy": number, "restart": false, "restart_only": false,
            "file": Value::Null, "told": true, "secret": used.args(),
        }),
    );
    let rec = NewRecord::json(kinds::LEDGER, None, &row)
        .map_err(|e| format!("the change was written, and its record could not be made: {e}"))?;
    if let Err(e) = tc.store.defer(rec) {
        tracing::warn!(error = %format!("{e:#}"), "vault.use: the change's row was not kept");
    }
    let title = tc
        .store
        .get_session::<crate::session::SessionRecord>(tc.session_id)
        .ok()
        .flatten()
        .and_then(|r| r.label.filter(|l| !l.is_empty()).or(r.title));
    let (head, by, body) = note_parts(title.as_deref(), used, number);
    let told = tell(tc, (head, &by, &body), number);
    let said = format!(
        "Done{}: `secrets.{}` now reads the vault's item \"{}\", field \"{}\", and nothing reads \
         it. Giving it to a setting is a card the owner answers. The value went to no turn, note or \
         record. This is settings change {number}; the note's Undo, or `config.undo`, puts it back.",
        match told {
            true => ", and the owner is told (no card)",
            false => " (no card); the owner's note could not be staged",
        },
        used.name,
        used.checked.item,
        used.checked.field,
    );
    Ok((said, json!({"settings": settings.len(), "told": told, "change": number})))
}

/// Stage the use's note in the owner's DM, the shared told note whose Undo removes the entry
/// (`put_back.rs`). Whether it was staged; one that was not is logged, and the use stands.
fn tell(tc: &TurnCtx<'_>, (head, by, body): (&str, &str, &str), number: u32) -> bool {
    let undo = crate::owner_note::told::Undo::VaultUseUndo(number);
    match crate::owner_note::told::stage_dm(tc.outbox, Some(tc.session_id), head, by, body, &undo) {
        Ok((post, records)) => {
            for r in records {
                if let Err(e) = tc.store.defer(r) {
                    tracing::warn!(error = %format!("{e:#}"), "vault.use: the note's record was not kept");
                }
            }
            tc.posts.lock().unwrap().push(post);
            true
        }
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "vault.use: the note was not staged");
            false
        }
    }
}
