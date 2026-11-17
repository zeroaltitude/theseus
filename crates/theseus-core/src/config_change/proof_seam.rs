//! Where a card of issue 22 meets the owner's second proof.
//!
//! A change to a sensitive setting, an undo of one, an MCP server attached or
//! removed, and the install of a joined change are never made on the card's
//! yes alone: each asks for a code from the owner's authenticator app too
//! (`owner_proof`). The card is **marked** by the daemon's own record, staged
//! in the frame that asks it ([`Needs::mark`]); the one place an answer
//! becomes a decision (`rpc::proof_gate`) then approves a marked question only
//! on a good code or an open window of the answer's own place. The daemon's
//! write asks once more at the yes ([`at_yes`]): the question must carry the
//! mark its settings call for, so a card asked without one is never written.
//!
//! An ordinary change needs the card alone. A change to who the owner is, to
//! the proof itself, or turning `[self] install` to `"auto"` is marked fresh:
//! a code typed for that change, covered by no window.

use theseus_store::NewRecord;

use super::diff::Setting;
use crate::store::Store;

/// What a card asks of the owner besides the card itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// An ordinary change: the card's yes.
    Card,
    /// A sensitive change: a code, or an open window of the same place.
    Proof,
    /// A code typed for this change alone, whatever window is open.
    Fresh,
}

impl Needs {
    /// What a change of `settings` asks: `sensitive` when it reached a
    /// sensitive setting, or is one of the kinds that always are (an MCP
    /// server's card, the install card).
    pub fn of(sensitive: bool, settings: &[Setting]) -> Self {
        match (needs_fresh(settings), sensitive) {
            (true, _) => Self::Fresh,
            (false, true) => Self::Proof,
            (false, false) => Self::Card,
        }
    }

    /// The record that marks the question `correlation_id`, to be staged in
    /// the frame that asks it. None for a card that asks for no proof.
    pub fn mark(self, correlation_id: &str) -> anyhow::Result<Option<NewRecord>> {
        match self {
            Self::Card => Ok(None),
            Self::Proof => crate::owner_proof::mark(correlation_id, false).map(Some),
            Self::Fresh => crate::owner_proof::mark(correlation_id, true).map(Some),
        }
    }
}

/// The words of a yes that reaches the write on a question the proof never saw.
pub const NOT_ASKED: &str = "this change needs the owner's second proof, and the card was not asked \
     with it, so nothing was written: ask for the change again";

/// The daemon's write, before it writes: the question must carry the mark
/// `needs` calls for. The code itself was judged where the answer became a
/// decision; this is the wall behind it, for a card that was made without a
/// mark.
pub fn at_yes(store: &Store, correlation_id: &str, needs: Needs) -> Result<(), String> {
    match (needs, crate::owner_proof::marked(store, correlation_id)) {
        (Needs::Card, _) | (Needs::Proof, Some(_)) | (Needs::Fresh, Some(true)) => Ok(()),
        _ => Err(NOT_ASKED.into()),
    }
}

/// Whether any setting of a change needs a fresh code, one typed for that
/// change alone and covered by no open window (`config::sensitivity::
/// needs_fresh_proof`). Read from the settings that really change, as the
/// sensitive check is.
pub fn needs_fresh(settings: &[Setting]) -> bool {
    settings
        .iter()
        .any(|s| crate::config::sensitivity::needs_fresh_proof(&s.path, s.after.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_change::diff;

    fn between(before: &str, after: &str) -> Vec<diff::Setting> {
        diff::between(before, after).expect("both parse")
    }

    #[test]
    fn a_change_that_turns_install_to_auto_needs_a_fresh_proof_however_it_is_written() {
        for after in [
            "[self]\ninstall = \"auto\"\n",
            "self = { install = \"auto\" }\n",
        ] {
            assert!(needs_fresh(&between("", after)), "{after}");
        }
        assert!(needs_fresh(&between(
            "[self]\ninstall = \"off\"\n",
            "[self]\ninstall = \"auto\"\nmode = \"on\"\n"
        )));
    }

    #[test]
    fn the_other_self_settings_and_a_move_away_from_auto_do_not() {
        assert!(!needs_fresh(&between("", "[self]\ninstall = \"ask\"\n")));
        assert!(!needs_fresh(&between(
            "[self]\ninstall = \"auto\"\n",
            "[self]\ninstall = \"off\"\n"
        )));
        assert!(!needs_fresh(&between("", "[self]\nmode = \"on\"\n")));
    }

    #[test]
    fn who_the_owner_is_and_the_proof_itself_need_a_fresh_proof() {
        for after in [
            "[places]\nowner = [\"discord:1\"]\n",
            "[server]\nbindings_file = \"elsewhere.toml\"\n",
            "[owner_proof]\nwindow_hours = 0\n",
        ] {
            assert_eq!(
                Needs::of(true, &between("", after)),
                Needs::Fresh,
                "{after}"
            );
        }
        let sensitive_only = between("", "[sandbox]\negress = [\"a.example:443\"]\n");
        assert_eq!(Needs::of(true, &sensitive_only), Needs::Proof);
        assert_eq!(
            Needs::of(false, &between("", "[import.notes]\nenabled = false\n")),
            Needs::Card
        );
    }
}
