//! The one-way floor's decision, pure (`p5kln-location-facts` piece 4; the
//! owner, 2026-11-13 12:22, S25-1).
//!
//! These state a call and its resolved words and read the answer back, with no
//! tree, no probe and no model. The floor ON THE LIVE GATE PATH is
//! `toolrun/tests_undo.rs`, which is where the card's five whole-core tests
//! live; this file is the precedence table underneath them, where a word can
//! be stated that a fixture would have to be built to produce.

use theseus_judge::builders::{Recovery, Target, UndoInput};

use super::*;

/// A plain `fs.write` of one target, with the word stated.
fn write_of(tool: &str, access: &str, recovery: Recovery) -> (UndoCall, UndoInput) {
    let path = "/w/notes.txt";
    let call = UndoCall {
        session_id: "ses_1".into(),
        turn_id: "turn_1".into(),
        tool: tool.into(),
        class: "write".into(),
        argv: vec![],
        steps: vec![],
        targets: vec![(path.into(), access.into())],
        url: None,
        other_args: None,
        cwd: None,
        is_program: false,
        l1: false,
        held: Default::default(),
    };
    let input = UndoInput {
        tool: tool.into(),
        class: "write".into(),
        posture: String::new(),
        argv: vec![],
        steps: vec![],
        targets: vec![Target {
            path: path.into(),
            access: access.into(),
            recovery,
        }],
        url: None,
        other_args: None,
        cwd: None,
        cwd_kind: None,
        sandbox: None,
        remote_keeps: None,
        terminal_tail: None,
    };
    (call, input)
}

fn floored(tool: &str, recovery: Recovery) -> bool {
    let (c, i) = write_of(tool, "write", recovery);
    forces_hold(&c, &i)
}

/// **The three words the owner named, and only those three.** The whole
/// precedence in one table, so a word added to `Recovery` later lands as a
/// compile error in `nothing_holds` and as a missing line here.
#[test]
fn the_floor_holds_exactly_the_three_words_the_owner_named() {
    for (word, held) in [
        (Recovery::Untracked, true),
        (Recovery::OutsideRepo, true),
        (Recovery::TrackedDirty, true),
        (Recovery::TrackedClean, false),
        (Recovery::ReadThisSession, false),
        (Recovery::Regenerable, false),
        (Recovery::Absent, false),
        (Recovery::Unknown, false),
    ] {
        assert_eq!(
            floored("fs.write", word),
            held,
            "{}: the floor must {}hold it",
            word.as_str(),
            if held { "" } else { "not " }
        );
    }
}

/// Every tool in the table is floored on the same word, and the three file
/// tools are the whole table: a word that holds `fs.write` holds `fs.edit` and
/// `fs.patch` too, because the rule is about the call's whole effect and not
/// about which of the three wrote it.
#[test]
fn the_floor_covers_every_plain_file_tool_in_the_table() {
    for p in PLAIN_FILE_TOOLS {
        assert!(
            floored(p.tool, Recovery::Untracked),
            "{} is in the table and must be floored",
            p.tool
        );
        assert!(
            !floored(p.tool, Recovery::ReadThisSession),
            "{} is still not floored where a copy is held",
            p.tool
        );
    }
}

/// **The owner's own boundary: shell commands, sends and everything else stay
/// with the model's judgment.** Named tools, not a shape, because a tool not in
/// the table is one whose whole effect code does not know.
#[test]
fn a_tool_the_table_does_not_name_is_never_floored() {
    for tool in [
        "proc.run",
        "fs.read",
        "fs.glob",
        "net.fetch",
        "term.send",
        "mcp.somebody_elses_writer",
    ] {
        assert!(
            !floored(tool, Recovery::Untracked),
            "{tool} is not a plain file write: the model decides it"
        );
    }
}

/// A call of a table tool that carries an argv, steps, a url or a program
/// backend is not a plain file write, whatever it is called: the structural
/// test sits UNDER the table, so a tool that grew a second effect stops being
/// floored rather than goes on being floored for the wrong reason.
#[test]
fn a_table_tool_whose_call_carries_more_than_its_targets_is_not_floored() {
    let base = || write_of("fs.write", "write", Recovery::Untracked);
    assert!(
        forces_hold(&base().0, &base().1),
        "the plain call is floored"
    );

    let mut argv = base();
    argv.0.argv = vec!["rm".into(), "-rf".into()];
    assert!(
        !forces_hold(&argv.0, &argv.1),
        "an argv is not a file target"
    );

    let mut steps = base();
    steps.0.steps = vec![vec!["rm".into()]];
    assert!(
        !forces_hold(&steps.0, &steps.1),
        "steps are not file targets"
    );

    let mut url = base();
    url.0.url = Some("https://example.invalid/".into());
    assert!(!forces_hold(&url.0, &url.1), "a url is not a file target");

    let mut program = base();
    program.0.is_program = true;
    assert!(
        !forces_hold(&program.0, &program.1),
        "a program's whole effect is not the paths it names"
    );
}

/// Only a target the call would CHANGE counts. A plain file call that merely
/// reads a path loses nothing from it, so an unrecoverable word on a `read`
/// target must not hold the call.
#[test]
fn a_target_the_call_only_reads_does_not_floor_it() {
    assert!(
        floored("fs.write", Recovery::Untracked),
        "the same word on a `write` target is the positive count"
    );
    let (c, i) = write_of("fs.write", "read", Recovery::Untracked);
    assert!(
        !forces_hold(&c, &i),
        "nothing is lost from a path the call does not write"
    );
}

/// One unrecoverable target out of several is enough: `fs.patch` names every
/// file its headers do, and a patch that would lose one of them loses it.
#[test]
fn one_unrecoverable_target_among_several_floors_the_call() {
    let (mut c, mut i) = write_of("fs.patch", "write", Recovery::TrackedClean);
    assert!(!forces_hold(&c, &i), "one recoverable target: not floored");
    c.targets.push(("/w/other.txt".into(), "write".into()));
    i.targets.push(Target {
        path: "/w/other.txt".into(),
        access: "write".into(),
        recovery: Recovery::Untracked,
    });
    assert!(
        forces_hold(&c, &i),
        "the patch would lose the second file, so the call is held"
    );
}

/// A call that names no target at all cannot be floored: there is nothing the
/// floor is about.
#[test]
fn a_call_with_no_targets_is_not_floored() {
    let (mut c, mut i) = write_of("fs.write", "write", Recovery::Untracked);
    c.targets.clear();
    i.targets.clear();
    assert!(!forces_hold(&c, &i));
}

/// The table is closed and every line says what the tool's whole effect is —
/// the condition the owner set for the floor applying at all. A line added
/// without one fails here.
#[test]
fn the_table_is_closed_and_every_line_says_its_whole_effect() {
    assert_eq!(
        PLAIN_FILE_TOOLS.len(),
        3,
        "growing this table is a decision with a reviewer: {:?}",
        PLAIN_FILE_TOOLS.iter().map(|p| p.tool).collect::<Vec<_>>()
    );
    for p in PLAIN_FILE_TOOLS {
        assert!(
            !p.effect.trim().is_empty(),
            "{}: a line must say what its whole effect is",
            p.tool
        );
        assert_eq!(whole_effect_of(p.tool), Some(p.effect));
    }
    assert_eq!(whole_effect_of("proc.run"), None);
}
