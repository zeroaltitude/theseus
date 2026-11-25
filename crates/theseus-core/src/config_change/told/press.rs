//! The Undo on a settings change's note (issue 57): the change put back for
//! the owner's press, with no turn. It is `config.undo` with no card: the undo
//! it plans (`undo::plan`), written by the same yes path a card's yes takes
//! ([`Core::write_told`], `write_change`), with the same rows. Only an undo that
//! `config.undo` would itself write with no card: one that reaches a sensitive
//! setting, has a value too long for a line, would make live at its restart a
//! written secret or address, or puts back a prompt file is not done here, and
//! the owner is told to ask in a conversation, where its card is put. A press
//! is the owner's own, so the bound on a turn's restarts does not apply.

use serde_json::json;
use theseus_protocol::LedgerKind;

use super::{going_live, Told};
use crate::config::sensitivity;
use crate::config_change::{diff, edit, history, read_base, sensitive, undo, write};
use crate::rpc::Core;

impl Core {
    /// Put back settings change `number` for an Undo pressed on its note, by
    /// `by` through `via`, in the conversation that made the change: what was
    /// done, or why nothing was.
    pub(crate) fn undo_change_pressed(
        &self,
        number: u32,
        by: &str,
        via: &str,
    ) -> Result<String, String> {
        let ask =
            format!("ask Theseus in a conversation to undo change #{number}, and it asks you");
        let w = &self.tools.config_change;
        let base = read_base(&self.tools)?;
        let rows = history::rows(&self.store);
        let list = history::entries(&rows, &w.state, w.file.as_deref(), history::SHOWN);
        let session = rows
            .iter()
            .find(|r| {
                r.kind == LedgerKind::ConfigChanged.as_str()
                    && r.data["previous_copy"].as_u64() == Some(u64::from(number))
            })
            .and_then(|r| r.session_id.clone())
            .unwrap_or_default();
        if undo::target(&list, Some(number))?.file.is_some() {
            return Err(format!("change #{number} changed a prompt file: {ask}"));
        }
        let planned = undo::plan(&list, Some(number), &base)?;
        let candidate = edit::apply(&base, &planned.changes)?;
        crate::Config::parse(&candidate)
            .map_err(|e| format!("the settings with that undo do not load: {e:#}"))?;
        let settings = diff::between(&base, &candidate)?;
        let asks = !sensitive(&planned.changes, &settings).is_empty()
            || settings.iter().any(diff::too_long)
            || !going_live(&self.store, w.started_ms).is_empty();
        if asks {
            return Err(format!(
                "putting change #{number} back is a change Theseus asks you about: {ask}"
            ));
        }
        let digest = write::keep_candidate(&w.state, &candidate)
            .map_err(|e| format!("the undo could not be kept, so nothing was written: {e}"))?;
        let args = json!({
            "base_sha256": crate::config_copy::sha256(&base),
            "candidate_sha256": digest,
            "paths": settings.iter().map(|s| edit::dotted(&s.path)).collect::<Vec<_>>(),
            "restart": settings.iter().any(|s| sensitivity::needs_restart(&s.path)),
            "undoes": planned.undoes.args(),
            "asked_by": {
                "session_id": &session,
                "place": format!("the Undo on change #{number}'s note"),
            },
        });
        let id = format!("told.undo.{number}.{}", theseus_protocol::now_unix_ms());
        let t = Told {
            id: &id,
            session_id: &session,
            args: &args,
            by,
            setting: via,
            records: vec![],
        };
        self.write_told(t, |n| {
            let said = format!("Put back settings change #{number}: the undo is change #{n}.");
            (said, None)
        })
    }
}

#[cfg(test)]
mod tests;
