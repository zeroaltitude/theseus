//! The seam for the second proof (issue 22, line 1 step 6).
//!
//! A change to a sensitive setting is never made on the card's yes alone: it
//! also needs a second proof, a code from the owner's authenticator app,
//! built on its own branch (`owner_proof`). Until that module is on main,
//! this is the one function the change path asks, and it answers "not
//! installed", so such a change is refused before a card is made, and again
//! on a yes (`answer.rs`), whatever else happened. The later commit that wires
//! the proof in replaces the body of [`required_and_given`] with a call to
//! `owner_proof::required_and_given`, and nothing else here moves.

use theseus_kernel::Action;

/// What the second proof is, for a question and its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proof {
    /// The proof is not built into this daemon: a sensitive change is refused.
    NotInstalled,
}

/// The words a refused sensitive change says.
pub const NOT_INSTALLED: &str = "this setting needs the second proof, which is not installed yet";

/// Whether the second proof is required for `question` and what was given
/// with its answer `via`. Until the proof is installed: not installed.
pub fn required_and_given(_question: &Action, _via: &str) -> Proof {
    Proof::NotInstalled
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
}

/// Whether any setting of a change needs a fresh code, one typed for that
/// change alone and covered by no open window (`config::sensitivity::
/// needs_fresh_proof`; today `[self] install` becoming `"auto"`). Read from
/// the settings that really change, as the sensitive check is. The wiring
/// passes it to `owner_proof::mark(correlation_id, fresh)` where the card is
/// asked, and again where a yes is judged.
pub fn needs_fresh(settings: &[super::diff::Setting]) -> bool {
    settings
        .iter()
        .any(|s| crate::config::sensitivity::needs_fresh_proof(&s.path, s.after.as_ref()))
}

/// Whether a sensitive change may go to the owner at all: not while the
/// proof is not installed.
pub fn installed() -> bool {
    false
}
