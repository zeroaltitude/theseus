//! A settings act that runs with no card, and tells the owner (issue 57).
//!
//! The owner's rule: a card only for what is dangerous and cannot be undone.
//! So an ordinary change (`config.propose`, and `config.undo` of one), a
//! restart (`daemon.restart`) and removing an MCP server (`mcp.detach`) follow
//! the tool's own setting, as any tool does. At `open` or `notify` the act is
//! written by the same yes path a card's yes takes (`answer.rs`,
//! `write_change`), its row says `told`, and the owner gets one FYI note: what
//! was done, in which conversation, and how to undo it. At `approve` (set, or
//! tightened by "should have asked") the card is asked as it always was.
//!
//! A card is still asked, whatever the setting, when:
//!
//! - the change reaches a sensitive setting, gives a vault item, or has a value
//!   too long for a line (the caller keeps today's path whole);
//! - the act needs a restart and this turn has already restarted Theseus with
//!   no card since the owner's last message ([`restart_spent`]): one restart
//!   with no card per message of the owner's, so a turn cannot restart in a loop;
//! - a change written since this daemon started, and so not yet live, sends a
//!   secret or names an address ([`going_live`]): the restart would make it
//!   live, and a secret sent cannot be called back. The card says which.

use serde_json::{json, Value};
use theseus_store::{kinds, NewRecord};

use super::answer::{changed_row, undone_row, Asked};
use super::{edit, history, write};
use crate::config::sensitivity;
use crate::rpc::Core;
use crate::toolrun::{ToolRuntime, TurnCtx};

/// Who approved a change written with no card, as its row and its marker say.
pub const BY: &str = "told";
/// The key, after the session's id, of the owner's message a restart with no
/// card was spent on in that session.
const SPENT_PREFIX: &str = "config.restart.told.";

/// The tool's own setting, in its words (`enforcement = notify`), when it lets
/// the act run with no card; none when it asks (`approve`, set or tightened).
pub(crate) fn untold(rt: &ToolRuntime, tool: &str) -> Option<String> {
    let now = rt.posture_now(tool);
    (now.posture != crate::policy::Posture::Approve).then_some(now.setting)
}

/// The owner's last message in this conversation, as the store numbers it.
fn last_word(tc: &TurnCtx<'_>) -> Option<u64> {
    let nodes = tc.store.session_nodes(tc.session_id).ok()?;
    nodes
        .iter()
        .rev()
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .map(|(seq, _)| *seq)
}

/// Whether this conversation has already restarted Theseus with no card since
/// the owner's last message (or has no message of the owner's at all): then the
/// next restart asks. Else the record that spends the owner's message on this
/// restart, to be written with it.
pub(crate) fn restart_spent(tc: &TurnCtx<'_>) -> Result<NewRecord, String> {
    let word = last_word(tc).ok_or("no message of the owner's in this conversation")?;
    let key = format!("{SPENT_PREFIX}{}", tc.session_id);
    if tc.store.get_meta::<u64>(&key).ok().flatten() == Some(word) {
        return Err("this conversation already restarted Theseus with no card since your last \
                    message"
            .into());
    }
    NewRecord::json(kinds::META, Some(&key), &word).map_err(|e| format!("{e:#}"))
}

/// Whether a setting sends a secret or names where something is sent: a
/// `[secrets]` entry, a key that reads a secret, an address, a host or a base URL.
fn reaches(path: &[String]) -> bool {
    path.first().is_some_and(|t| t == "secrets")
        || path.iter().skip(1).any(|k| {
            let lower = k.to_ascii_lowercase();
            matches!(
                sensitivity::key_reason(k),
                Some(sensitivity::SECRET_REASON | sensitivity::ADDRESS_REASON)
            ) || lower == "egress"
                || lower.contains("host")
        })
}

/// The settings written since this daemon started, so not live until it
/// restarts (`sensitivity::LIVE` is empty), that send a secret or name an
/// address ([`reaches`]): each as its dotted path. A prompt file is live at
/// once, and a plain restart writes no setting.
pub(crate) fn going_live(store: &crate::store::Store, started_ms: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in history::rows(store) {
        let d = &r.data;
        let written = r.kind == theseus_protocol::LedgerKind::ConfigChanged.as_str()
            && d["source"] == "owner"
            && d["restart_only"] != true
            && !d["file"].is_object()
            && r.at_unix_ms >= started_ms;
        if !written {
            continue;
        }
        for p in history::paths_of(d).into_iter().filter(|p| reaches(p)) {
            let dotted = edit::dotted(&p);
            if !out.contains(&dotted) {
                out.push(dotted);
            }
        }
    }
    out
}

/// What a card says of the settings a restart makes live ([`going_live`]):
/// empty when there are none.
pub fn going_live_line(paths: &[String]) -> String {
    match paths.is_empty() {
        true => String::new(),
        false => format!(
            "Written since Theseus last started, and live from this start on: {}. Each sends a \
             secret or names where Theseus sends things, and a secret sent cannot be called back, \
             so this restart asks you.\n",
            paths.join(", ")
        ),
    }
}

/// Where the act was asked, as the note says it.
fn asked_in(args: &Value) -> String {
    match args["asked_by"]["place"].as_str() {
        Some(p) => format!("asked in {p}"),
        None => "asked in a conversation in the terminal or the web UI".into(),
    }
}

/// The note's trigger line: where it was asked, and the setting that let it
/// run with no card.
fn trigger(args: &Value, setting: &str, tool: &str) -> String {
    format!("{}; {tool} ran with no card ({setting})", asked_in(args))
}

/// The note for a change written with no card (`number` is its number in the
/// history), and its undo.
pub fn change_note(
    args: &Value,
    settings: &[super::diff::Setting],
    number: u32,
    (tool, setting): (&str, &str),
    outside_text: bool,
) -> String {
    let mut body = String::new();
    for s in settings {
        body.push_str(&format!(
            "- {}: {} → {}\n",
            edit::dotted(&s.path),
            super::diff::words(s.before.as_ref()),
            super::diff::words(s.after.as_ref()),
        ));
    }
    if args["restart"] == true {
        body.push_str("Theseus restarts itself to apply it.\n");
    }
    if outside_text {
        body.push_str(
            "This conversation had read text from outside since your last message (a page, a \
             search or a server).\n",
        );
    }
    body.push_str(&format!(
        "To undo it, ask Theseus to undo change #{number}.\n"
    ));
    let title = match args["undoes"]["number"].as_u64() {
        Some(n) => format!("Undid settings change #{n}"),
        None => format!(
            "Changed {}",
            crate::narrative::count(settings.len() as u64, "setting", "settings")
        ),
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &title,
        &trigger(args, setting, tool),
        &body,
    )
}

/// The note for a restart run with no card.
pub fn restart_note(args: &Value, setting: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        "Restarting Theseus",
        &trigger(args, setting, super::RESTART),
        &format!(
            "It waits up to {} s for a running turn to end, and a turn it cuts resumes \
             afterwards. Its settings do not change, so there is nothing to undo.\n",
            super::RESTART_WAIT_SECS
        ),
    )
}

/// The note for a server removed with no card.
pub fn detach_note(args: &Value, name: &str, setting: &str) -> String {
    let when = match args["restart"] == true {
        true => "Theseus started it from the settings file, so it stops at a restart, which \
                 Theseus does now.",
        false => "It stopped, and its tools are gone from your next message.",
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("Removed the MCP server {name}"),
        &trigger(args, setting, super::mcp_attach::DETACH),
        &format!(
            "{when}\nTo have it back, ask Theseus to attach it again: attaching a server asks \
             you, as it always has.\n"
        ),
    )
}

/// An act written with no card: the arguments its card would have carried, the
/// tool's setting that let it run, and what to say once it is written (given
/// the change's number in the history: the tool's result and the note).
pub(crate) struct Told<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub args: &'a Value,
    /// Who approved it, as its row says: [`BY`] for an act the tool's setting
    /// let run, the presser for an Undo pressed on a note.
    pub by: &'a str,
    /// What let it run: the tool's setting, or where the Undo was pressed.
    pub setting: &'a str,
    /// Written with the act: the record a call run again reads, the restart's
    /// spend.
    pub records: Vec<NewRecord>,
}

impl Core {
    /// Write an act with no card through the yes path (`write_change`), its
    /// rows in one append, then the note to the owner (none for an Undo
    /// pressed: its own note says it), then the server live or the restart.
    /// `words` gives the tool's result and the note from the change's number.
    /// The result's record is written with the rows, so a call run again after
    /// the restart answers with it.
    pub(crate) fn write_told(
        &self,
        t: Told<'_>,
        words: impl FnOnce(u32) -> (String, Option<String>),
    ) -> Result<String, String> {
        let q = Asked {
            id: t.id,
            session_id: t.session_id,
            args: t.args,
        };
        let applied = self.write_change(q, t.by, t.setting)?;
        let (said, note) = words(applied.previous);
        let who = &t.args["asked_by"];
        let rows = (|| -> anyhow::Result<Vec<NewRecord>> {
            let mut rows = t.records;
            rows.push(NewRecord::json(
                kinds::META,
                Some(&super::call_key(t.id)),
                &said,
            )?);
            rows.push(NewRecord::json(
                kinds::LEDGER,
                None,
                &changed_row(q, &applied, who, t.by, t.setting),
            )?);
            rows.extend(undone_row(q, true, t.args, who, t.by, t.setting)?);
            Ok(rows)
        })()
        .map_err(|e| format!("the change was written, but its record was not: {e:#}"))?;
        let live = self
            .mcp_prepare(t.args, t.by, t.setting, t.session_id)
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
        if let Some(note) = note {
            if let Err(e) = self.outbox.to_operator(
                Some(t.session_id),
                json!({"kind": "notice", "text": note, "announce": true}),
            ) {
                tracing::warn!(error = %format!("{e:#}"), "the owner's note about a settings act was not written");
            }
        }
        if let Some(l) = live {
            self.mcp_apply(l);
        }
        if let Some(p) = applied.pending.as_ref() {
            self.restart_for_change(p, std::time::Duration::from_secs(super::RESTART_WAIT_SECS));
        }
        Ok(said)
    }
}

mod press;

#[cfg(test)]
mod tests;
