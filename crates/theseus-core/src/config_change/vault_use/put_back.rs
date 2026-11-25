//! The Undo on the note of a told `vault.use` (issue 54): the wiring before the use put back
//! exactly, for the owner's press, with no turn. `told.undo` has already judged the press (the
//! owner, from a private place) and judges no one again here.
//!
//! It is the undo `config.undo` plans (`undo::plan`): each setting the use touched back to its
//! value before it, the secret's own entry removed again or pointed back at the item it read,
//! and nothing else. It is written by the path the use itself took (the previous file kept as a
//! numbered copy, the file written whole), with a `config.changed` row and the `config.undone`
//! row a card's yes to an undo writes, so the history says it was undone. Only the change of a
//! told use is put back here; one whose settings were changed again since is not, and the owner
//! is told to ask in a conversation, where its card shows what it would overwrite. Like the use,
//! it takes effect at the next restart, which only the owner starts.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use super::super::{diff, edit, history, read_base, undo, write};
use crate::approval::Answerer;
use crate::config_copy::sha256;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

impl Core {
    /// Put back the told use that was settings change `number`, for `who`'s press.
    pub(crate) fn vault_use_put_back(&self, number: u32, who: &Answerer) -> Result<(), String> {
        let ask = format!("ask Theseus in a conversation to undo change #{number}, and it asks you");
        let w = &self.tools.config_change;
        let _one = w
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let list = history::entries(&rows, &w.state, w.file.as_deref(), history::SHOWN);
        let planned = undo::plan(&list, Some(number), &base)?;
        if !planned.undoes.since.is_empty() {
            return Err(format!(
                "a setting change #{number} made was changed again since, so putting it back would \
                 overwrite that: {ask}"
            ));
        }
        let candidate = edit::apply(&base, &planned.changes)?;
        crate::Config::parse(&candidate)
            .map_err(|e| format!("the settings with that undo do not load: {e:#}"))?;
        let settings = diff::between(&base, &candidate)?;
        let file = w
            .file
            .clone()
            .ok_or("the settings are not a file this daemon can change")?;
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
            .map_err(|e| format!("the settings were put back, and their record was not made: {e}"))?;
        self.store
            .append(&records)
            .map_err(|e| format!("the settings were put back, and their record was not kept: {e}"))?;
        Ok(())
    }
}
