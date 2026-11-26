//! The Undo on the note of a told `vault.use` (issue 54): the entry the use added removed again,
//! for the owner's press, with no turn. `told.undo` has already judged the press (the owner, from
//! a private place) and judges no one again here.
//!
//! A told use only ever adds a new name that nothing reads (`classify::Case::Inert`), so its undo
//! removes exactly that entry: the undo `config.undo` plans for the change (`undo::plan`), written
//! by the settings' one writer (`answer::one_write`), with a `config.changed` row and the
//! `config.undone` row a card's yes to an undo writes, so the history says it was undone. It runs
//! only while the settings file is exactly what the use wrote: if the settings changed since, the
//! use has moved on, nothing is written, and the note says the owner asks in a conversation, where
//! a card shows what an undo would overwrite.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use super::super::{answer, diff, edit, history, read_base, undo, write};
use crate::approval::Answerer;
use crate::config_copy::sha256;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

impl Core {
    /// Remove the entry the told use that was settings change `number` added, for `who`'s press:
    /// true when it was removed, false when the settings changed since (nothing is written).
    pub(crate) fn vault_use_put_back(&self, number: u32, who: &Answerer) -> Result<bool, String> {
        let w = &self.tools.config_change;
        let file = w
            .file
            .clone()
            .ok_or("the settings are not a file this daemon can change")?;
        let _one = answer::one_write(w)?;
        let rows = history::rows(&self.store);
        let made = rows
            .iter()
            .find(|r| {
                r.kind == LedgerKind::ConfigChanged.as_str()
                    && r.data["previous_copy"].as_u64() == Some(u64::from(number))
            })
            .filter(|r| r.data["told"] == true && r.data["via"] == super::USE)
            .ok_or_else(|| format!("change #{number} is not a use of a secret you were told of"))?;
        let session = made.session_id.clone().unwrap_or_default();
        let base = read_base(&self.tools)?;
        if made.data["digest_after"].as_str() != Some(sha256(&base).as_str()) {
            return Ok(false);
        }
        let list = history::entries(&rows, &w.state, w.file.as_deref(), history::SHOWN);
        let planned = undo::plan(&list, Some(number), &base)?;
        let candidate = edit::apply(&base, &planned.changes)?;
        crate::Config::parse(&candidate)
            .map_err(|e| format!("the settings with that undo do not load: {e:#}"))?;
        let settings = diff::between(&base, &candidate)?;
        let (copy, _) = write::keep_previous(&w.state, &base)
            .map_err(|e| format!("the settings could not be kept, so nothing was put back: {e}"))?;
        write::write_config(&file, &candidate)
            .map_err(|e| format!("the settings could not be written: {e}"))?;
        let id = format!("told.undo.{number}.{}", theseus_protocol::now_unix_ms());
        let paths: Vec<String> = settings.iter().map(|s| edit::dotted(&s.path)).collect();
        let asked_by = json!({
            "session_id": session,
            "place": format!("the Undo on change #{number}'s note"),
        });
        let (by, via) = (who.label.clone(), who.via());
        let changed = LedgerRow::new(
            LedgerKind::ConfigChanged,
            Some(&session),
            None,
            json!({
                "source": "owner", "question": id, "paths": paths, "asked_by": asked_by,
                "approved_by": by, "via": via,
                "digest_before": sha256(&base), "digest_after": sha256(&candidate),
                "previous_copy": copy, "restart": false, "restart_only": false,
                "file": Value::Null, "told": true,
            }),
        );
        let undone = undo::undone_row(
            &session,
            &id,
            &planned.undoes.args(),
            &json!({"paths": paths}),
            &asked_by,
            &by,
            &via,
        );
        let records = [changed, undone]
            .iter()
            .map(|r| NewRecord::json(kinds::LEDGER, None, r))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| {
                format!("the settings were put back, and their record was not made: {e}")
            })?;
        self.store.append(&records).map_err(|e| {
            format!("the settings were put back, and their record was not kept: {e}")
        })?;
        Ok(true)
    }

    /// Whether the told use that was settings change `number` was undone: a `config.undone` row
    /// names it. What `told.undo` reads before it runs the Undo again after a crash.
    pub(crate) fn vault_use_put_back_done(&self, number: u32) -> bool {
        history::rows(&self.store).iter().any(|r| {
            r.kind == LedgerKind::ConfigUndone.as_str()
                && r.data["undone"].as_u64() == Some(u64::from(number))
        })
    }
}
