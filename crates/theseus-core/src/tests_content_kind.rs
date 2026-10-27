//! Whose words a message is, for the injection question (theseus-9ymz0, the
//! review's J2): the operator's own only when the place is private **and** the
//! author is an owner. A guest in a channel bound private, or in a trusted
//! guild, is not. The rig is `tests_recall`'s: the owner's DM, `#den` (a guild
//! channel bound private) and `#quay` (shared).

use crate::config::MemoryMode;
use crate::content::ItemKind;
use crate::tests_recall::{rig, session, DEN, OWNER, QUAY};

#[tokio::test]
async fn the_operator_is_an_owner_in_a_private_place_never_the_place_alone() {
    let r = rig(MemoryMode::Off);
    let c = &r.core;
    let cli = session(c, None, &[]);
    let dm = session(c, Some(&format!("dm:{OWNER}")), &[]);
    let den = session(c, Some(&format!("channel:{DEN}")), &[]);
    let quay = session(c, Some(&format!("channel:{QUAY}")), &[]);
    let kind = |sid: &str, owner: bool| c.runner.message_kind(sid, owner);
    // The owner's own words, wherever the place is private: never asked.
    for (name, sid) in [("the CLI", &cli), ("the owner's DM", &dm), ("#den", &den)] {
        assert_eq!(kind(sid, true), ItemKind::OperatorMessage, "{name}");
    }
    // A guest in the same private places is asked: the place does not decide.
    for (name, sid) in [("the CLI", &cli), ("the owner's DM", &dm), ("#den", &den)] {
        assert_eq!(kind(sid, false), ItemKind::PersonMessage, "{name}");
    }
    // A shared place asks whoever speaks.
    for owner in [true, false] {
        assert_eq!(kind(&quay, owner), ItemKind::PersonMessage, "#quay {owner}");
    }
}
