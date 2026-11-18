//! What a proposal's reply says it checked, and what else is waiting (issue 22).
//!
//! A turn that proposes a change used to be left to guess two things the daemon
//! knows: whether the new value was a valid one (it had loaded the whole settings
//! with it, so a value that does not load never reaches a card), and whether an
//! earlier change was still waiting (it reported one from the conversation's
//! memory that had been approved and undone hours before). So the reply says
//! both, from the daemon's own records.
//!
//! **What "waiting" counts.** A settings question that is planned and not yet
//! past its life: a change, an undo, a restart, an attached or removed server, a
//! prompt file or an install, from any place (a question asked from another place
//! is named as such). It does not count a tool's approval, a call the floor
//! holds, a spend limit or any other kind of question, and the reply says so.
//! The question this proposal makes is not yet asked when the reply is built.

use theseus_kernel::Action;

use super::words;
use crate::toolrun::TurnCtx;

/// The most waiting questions the reply names; the rest are counted.
const NAMED: usize = 5;

/// What a settings proposal says it checked.
pub(crate) const SETTINGS_CHECKED: &str =
    "Checked: the daemon loaded all of Theseus's settings with this change, the value's meaning \
     included, and they are valid (a value that does not load is refused before any card), so \
     there is nothing to tell the owner about whether the value is valid.";

/// What a prompt file proposal says it checked.
pub(crate) const FILE_CHECKED: &str = "Checked: the daemon confirmed the file may be changed and the new text fits a prompt file. It \
     changes no setting.";

/// The settings questions waiting for the owner now, besides this proposal's own,
/// as the reply's last lines. A question that cannot be listed is said so, never
/// read as none.
pub(crate) fn waiting(tc: &TurnCtx<'_>) -> String {
    let Ok(pending) = tc.kernel.pending_confirms() else {
        return "Other settings questions waiting for the owner: could not be read, so do not say \
                whether any is."
            .into();
    };
    let here = tc.outbox.target(tc.session_id);
    let now = theseus_protocol::now_unix_ms();
    let lines: Vec<String> = pending
        .iter()
        .filter(|a| {
            words::is_settings_action(a) && now < a.planned_at_ms.saturating_add(tc.confirm_ttl_ms)
        })
        .filter_map(|a| line(a, here.as_deref()))
        .collect();
    let scope = "This counts settings questions of every kind (changes, undos, restarts, servers, \
                 prompt files, installs), from any place; not tool approvals or other questions.";
    match lines.as_slice() {
        [] => format!("No other settings question is waiting for the owner. {scope}"),
        _ => {
            let mut out = String::from("Other settings questions waiting for the owner:\n");
            for l in lines.iter().take(NAMED) {
                out.push_str(&format!("- {l}\n"));
            }
            if lines.len() > NAMED {
                out.push_str(&format!("- and {} more\n", lines.len() - NAMED));
            }
            out.push_str(scope);
            out
        }
    }
}

/// One waiting question in one line: its words, and where it was asked when
/// that is somewhere other than here.
fn line(a: &Action, here: Option<&str>) -> Option<String> {
    let args = &a.proposal.as_ref()?.args;
    let what = words::plain_of(args)?.what;
    Some(tagged(what, args["asked_by"]["place"].as_str(), here))
}

/// `what`, said to be from another place when both places are known and differ.
fn tagged(what: String, there: Option<&str>, here: Option<&str>) -> String {
    match (there, here) {
        (Some(t), Some(h)) if t != h => format!("{what} (asked from another place)"),
        _ => what,
    }
}

#[cfg(test)]
mod tests;
