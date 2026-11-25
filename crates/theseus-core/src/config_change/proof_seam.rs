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
//! An ordinary change needs the card alone, and so does a sensitive one whose
//! settings `config::sensitivity::proof_reason` does not name (issue 57: the
//! code is kept for the approval rules, secrets, the sandbox, places, the
//! broker, where Theseus listens, its MCP server, who the owner is, and a
//! raised spend). A change to who the owner is, to the proof itself, or turning
//! `[self] install` to `"auto"` is marked fresh: a code typed for that change,
//! covered by no window.

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
    /// What a change of `settings` asks: fresh when any needs a fresh code, the
    /// proof when any needs the code (`sensitivity::proof_reason`), else the
    /// card alone.
    pub fn of(settings: &[Setting]) -> Self {
        let proof = settings.iter().any(|s| {
            crate::config::sensitivity::proof_reason(&s.path, s.before.as_ref(), s.after.as_ref())
                .is_some()
        });
        match (needs_fresh(settings), proof) {
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
pub const NOT_ASKED: &str =
    "this change needs the owner's second proof, and the card was not asked \
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
    fn the_write_refuses_a_question_the_proof_never_saw() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        // A card that asks for the card alone needs no mark.
        assert_eq!(at_yes(&store, "act_plain", Needs::Card), Ok(()));
        // A question asked with no mark is never written when its settings
        // call for the proof, whatever answered it.
        for needs in [Needs::Proof, Needs::Fresh] {
            assert_eq!(at_yes(&store, "act_bare", needs), Err(NOT_ASKED.into()));
        }
        // Marked for the proof: enough for a sensitive change, not for a fresh one.
        store
            .append(&[
                crate::owner_proof::mark("act_marked", false).unwrap(),
                crate::owner_proof::mark("act_fresh", true).unwrap(),
            ])
            .unwrap();
        assert_eq!(at_yes(&store, "act_marked", Needs::Proof), Ok(()));
        assert_eq!(
            at_yes(&store, "act_marked", Needs::Fresh),
            Err(NOT_ASKED.into())
        );
        assert_eq!(at_yes(&store, "act_fresh", Needs::Fresh), Ok(()));
        assert_eq!(at_yes(&store, "act_fresh", Needs::Proof), Ok(()));
    }

    #[test]
    fn who_the_owner_is_and_the_proof_itself_need_a_fresh_proof() {
        for after in [
            "[places]\nowner = [\"discord:1\"]\n",
            "[discord]\nbindings_file = \"elsewhere.toml\"\n",
            "[server]\nstate_dir = \"/elsewhere\"\n",
            "[owner_proof]\nwindow_hours = 0\n",
        ] {
            assert_eq!(Needs::of(&between("", after)), Needs::Fresh, "{after}");
        }
        let sensitive_only = between("", "[sandbox]\negress = [\"a.example:443\"]\n");
        assert_eq!(Needs::of(&sensitive_only), Needs::Proof);
        assert_eq!(
            Needs::of(&between("", "[import.notes]\nenabled = false\n")),
            Needs::Card
        );
    }

    /// Issue 57: the code is kept for the rules, secrets, the sandbox, places, the broker, where
    /// Theseus listens, its MCP server and a raised spend; every other sensitive setting keeps its
    /// card and needs no code.
    #[test]
    fn the_code_is_kept_for_the_gates_and_money_and_dropped_for_the_rest() {
        for after in [
            "[policy.tools]\n\"proc.run\" = \"open\"\n",
            "[secrets]\nk = \"op://v/i/f\"\n",
            "[places]\npublic = [\"discord:9\"]\n",
            "[broker]\nenabled = true\n",
            "[server]\nport = 9\n",
            "[mcp_server]\nport = 9\n",
            "[google]\ncredentials = \"elsewhere\"\n",
            "[mcp.servers.files]\ncommand = [\"x\"]\n",
            "[mcp.servers.files]\nauth_secret = \"k\"\n",
            "brand_new = 1\n",
        ] {
            assert_eq!(Needs::of(&between("", after)), Needs::Proof, "{after}");
        }
        for after in [
            "[self]\nmode = \"act\"\n",
            "[self_issues]\nrepo = \"x\"\n",
            "[self_fix]\nin_place = true\n",
            "[aws]\nregion = \"x\"\n",
            "[google]\nenabled = true\n",
            "[content_based_security]\ninjection = \"notify\"\n",
            "[judge]\nenabled = false\n",
            "[privacy]\nmodels = [\"x\"]\n",
            "[vault]\nform = true\n",
            "[tools]\nroots = [\"/x\"]\n",
            "[tools]\nagent_processes = [\"x\"]\n",
            "[model]\nsystem = \"x\"\n",
            "[model]\nprovider = \"x\"\n",
            "[providers.x]\napi_base = \"https://x.example\"\n",
        ] {
            assert_eq!(Needs::of(&between("", after)), Needs::Card, "{after}");
        }
    }

    /// Raising a spending ceiling or budget needs the code; lowering one does not; a change that
    /// cannot be read as a lowering keeps it.
    #[test]
    fn only_a_lowered_spend_goes_without_the_code() {
        let usd = |b: &str, a: &str| Needs::of(&between(b, a));
        let five = "[self.budget]\nusd = 5.0\n";
        let three = "[self.budget]\nusd = 3.0\n";
        assert_eq!(usd(five, three), Needs::Card);
        assert_eq!(usd(three, five), Needs::Proof);
        assert_eq!(usd("", five), Needs::Proof, "set where there was none");
        assert_eq!(
            usd(five, ""),
            Needs::Proof,
            "removed: its default may be higher"
        );
        assert_eq!(
            usd(five, "[self.budget]\nusd = \"lots\"\n"),
            Needs::Proof,
            "not a number"
        );
        let ceiling = |v: &str| format!("[kernel]\ndaily_spend_ceiling_usd = {v}\n");
        assert_eq!(usd(&ceiling("50"), &ceiling("40")), Needs::Card);
        assert_eq!(usd(&ceiling("40"), &ceiling("50")), Needs::Proof);
    }
}
