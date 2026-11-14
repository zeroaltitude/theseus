//! `theseus self set-aside` and `bring-back` (issue 6, condition 6): their words, and the one
//! thing a command must never do, which is say a set-aside worked when it dropped the item.

use clap::Parser;
use theseus_protocol::rsi::SelfDropResult;

use super::backlog::aside_words;
use super::SelfCmd;

fn result(changed: bool, set_aside: bool) -> SelfDropResult {
    SelfDropResult {
        item: "tsk_sails".into(),
        title: "Trim the sails".into(),
        cause: "turn.failed:timeout".into(),
        changed,
        set_aside,
    }
}

/// KEY (the first review's F6): an answer without `set_aside` is a daemon older than this command,
/// which read the request as a drop; the command says so and fails, and never says nothing was
/// forbidden.
#[test]
fn an_older_daemons_drop_is_never_reported_as_a_set_aside() {
    let said = aside_words(&result(true, false)).unwrap_err().to_string();
    assert!(said.contains("DROPPED tsk_sails"), "{said}");
    assert!(said.contains("never filed again"), "{said}");
    assert!(!said.contains("nothing is forbidden"), "{said}");
}

#[test]
fn a_set_aside_says_nothing_is_forbidden_and_how_to_bring_it_back() {
    let said = aside_words(&result(true, true)).unwrap();
    assert!(said.contains("nothing is forbidden"), "{said}");
    assert!(
        said.contains("`theseus self bring-back tsk_sails`"),
        "{said}"
    );
    assert_eq!(
        aside_words(&result(false, true)).unwrap(),
        "tsk_sails (Trim the sails) was set aside already."
    );
}

/// The two verbs parse: `set-aside <item> <why words>`, and `bring-back` is `retry`.
#[test]
fn the_verbs_parse() {
    let line = "theseus self set-aside tsk_sails it keeps failing the same way";
    match crate::Cli::try_parse_from(line.split_whitespace())
        .unwrap()
        .cmd
    {
        crate::Cmd::SelfImprove {
            cmd: SelfCmd::SetAside { item, why },
        } => {
            assert_eq!(item, "tsk_sails");
            assert_eq!(why.join(" "), "it keeps failing the same way");
        }
        other => panic!("{other:?}"),
    }
    match crate::Cli::try_parse_from(["theseus", "self", "bring-back", "tsk_sails"])
        .unwrap()
        .cmd
    {
        crate::Cmd::SelfImprove {
            cmd: SelfCmd::Retry { item },
        } => assert_eq!(item, "tsk_sails"),
        other => panic!("{other:?}"),
    }
}
