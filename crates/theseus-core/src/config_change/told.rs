//! Removing an MCP server that was attached live runs with no card, and tells
//! the owner (issue 57).
//!
//! The owner's rule: a card only for what is dangerous and cannot be undone.
//! A server attached by asking stops at once when it is removed, with no
//! restart, and attaching it again asks as it always has. So `mcp.detach` of
//! such a server follows the tool's own setting, as any tool does. At `open`
//! or `notify` it is written by the same yes path a card's yes takes
//! (`answer.rs`, `write_change`), its row says `told`, and the owner gets one
//! FYI note: what was removed, in which conversation, and how to have it back.
//! At `approve` (set, or tightened by "should have asked") the card is asked
//! as it always was. A server started from the settings file stops only at a
//! restart, so its removal keeps its card whatever the setting.

use serde_json::{json, Value};
use theseus_store::{kinds, NewRecord};

use super::answer::{changed_row, Asked};
use super::write;
use crate::rpc::Core;
use crate::toolrun::ToolRuntime;

/// Who approved a change written with no card, as its row and its marker say.
pub const BY: &str = "told";

/// The tool's own setting, in its words (`enforcement = notify`), when it lets
/// the act run with no card; none when it asks (`approve`, set or tightened).
pub(crate) fn untold(rt: &ToolRuntime, tool: &str) -> Option<String> {
    let now = rt.posture_now(tool);
    (now.posture != crate::policy::Posture::Approve).then_some(now.setting)
}

/// Where the act was asked, as the note says it.
fn asked_in(args: &Value) -> String {
    match args["asked_by"]["place"].as_str() {
        Some(p) => format!("asked in {p}"),
        None => "asked in a conversation in the terminal or the web UI".into(),
    }
}

/// The note for a server removed with no card: it stopped with no restart.
pub fn detach_note(args: &Value, name: &str, setting: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("Removed the MCP server {name}"),
        &format!(
            "{}; {} ran with no card ({setting})",
            asked_in(args),
            super::mcp_attach::DETACH
        ),
        "It stopped, and its tools are gone from your next message.\nTo have it back, ask \
         Theseus to attach it again: attaching a server asks you, as it always has.\n",
    )
}

/// An act written with no card: the arguments its card would have carried, and
/// the tool's setting that let it run.
pub(crate) struct Told<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub args: &'a Value,
    pub setting: &'a str,
}

impl Core {
    /// Write an act with no card through the yes path (`write_change`), its
    /// rows in one append, then the note to the owner, then the server let go.
    /// The tool's result (`said`) is written with the rows, so a call run again
    /// answers with it.
    pub(crate) fn write_told(
        &self,
        t: Told<'_>,
        said: String,
        note: String,
    ) -> Result<String, String> {
        let q = Asked {
            id: t.id,
            session_id: t.session_id,
            args: t.args,
        };
        let applied = self.write_change(q, BY, t.setting)?;
        let who = &t.args["asked_by"];
        let rows = (|| -> anyhow::Result<Vec<NewRecord>> {
            Ok(vec![
                NewRecord::json(kinds::META, Some(&super::call_key(t.id)), &said)?,
                NewRecord::json(
                    kinds::LEDGER,
                    None,
                    &changed_row(q, &applied, who, BY, t.setting),
                )?,
            ])
        })()
        .map_err(|e| format!("the change was written, but its record was not: {e:#}"))?;
        let live = self
            .mcp_prepare(t.args, BY, t.setting, t.session_id)
            .map_err(|e| format!("{e:#}"))?;
        let mut rows = rows;
        if let Some(l) = &live {
            rows.push(l.record().map_err(|e| format!("{e:#}"))?);
        }
        self.store
            .append(&rows)
            .map_err(|e| format!("the change was written, but its record was not: {e:#}"))?;
        if let Some(sha) = t.args["candidate_sha256"].as_str() {
            write::drop_candidate(&self.tools.config_change.state, sha);
        }
        // To the notices place when one is bound (`outbox::ANNOUNCE_KEY`).
        if let Err(e) = self.outbox.to_operator(
            Some(t.session_id),
            json!({"kind": "notice", "text": note, "announce": true}),
        ) {
            tracing::warn!(error = %format!("{e:#}"), "the owner's note about a settings act was not written");
        }
        if let Some(l) = live {
            self.mcp_apply(l);
        }
        Ok(said)
    }
}

#[cfg(test)]
mod tests;
