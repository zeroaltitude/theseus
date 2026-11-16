//! `rm`'s grammar, which is all the code reads of a shell command: the cases that are read, and
//! each shape that is not. The whole-core cases are `toolrun/tests_undo/self_own.rs`.

use serde_json::{json, Value};
use theseus_tools::proc::Run;

use super::*;

fn operands(argv: &[&str], input: Value) -> Option<Vec<String>> {
    let plan = Plan {
        argv: Some(argv.iter().map(ToString::to_string).collect()),
        ..Plan::default()
    };
    rm_operands(|| true, &Run::default(), &plan, &input)
        .map(|o| o.into_iter().map(String::from).collect())
}

fn read(argv: &[&str]) -> Option<Vec<String>> {
    operands(argv, json!({}))
}

#[test]
fn rm_with_its_known_flags_gives_its_operands() {
    assert_eq!(read(&["rm", "a"]), Some(vec!["a".into()]));
    assert_eq!(
        read(&["rm", "-rf", "a", "b/c"]),
        Some(vec!["a".into(), "b/c".into()])
    );
    assert_eq!(read(&["rm", "-r", "-f", "-v", "a"]), Some(vec!["a".into()]));
    assert_eq!(
        read(&["rm", "--recursive", "--force", "a"]),
        Some(vec!["a".into()])
    );
    assert_eq!(read(&["/usr/bin/rm", "-Rfd", "a"]), Some(vec!["a".into()]));
    // After `--` a dash is part of a name, and the flags have ended.
    assert_eq!(
        read(&["rm", "-f", "--", "-rf", "--force"]),
        Some(vec!["-rf".into(), "--force".into()])
    );
    // A lone dash is a file of that name.
    assert_eq!(read(&["rm", "-"]), Some(vec!["-".into()]));
}

#[test]
fn what_is_not_read_gives_nothing() {
    for (what, argv) in [
        ("no operand", vec!["rm", "-rf"]),
        ("an empty operand", vec!["rm", ""]),
        ("a flag it does not know", vec!["rm", "-rfi", "a"]),
        (
            "a long flag it does not know",
            vec!["rm", "--one-file-system", "a"],
        ),
        (
            "the root guard off",
            vec!["rm", "--no-preserve-root", "-rf", "a"],
        ),
        ("a relative program", vec!["./rm", "-rf", "a"]),
        (
            "another program",
            vec!["git", "worktree", "remove", "--force", "a"],
        ),
        ("a shell", vec!["bash", "-c", "rm -rf a"]),
        ("no program", vec![]),
    ] {
        assert_eq!(read(&argv), None, "{what}");
    }
}

#[test]
fn a_call_that_sets_an_environment_is_not_read() {
    let argv = ["rm", "-rf", "a"];
    assert!(
        operands(&argv, json!({"env": {}})).is_some(),
        "an empty one is none"
    );
    for env in [
        json!({"PATH": "/tmp"}),
        json!({"A": "b"}),
        json!(null),
        json!("PATH=/tmp"),
    ] {
        assert_eq!(operands(&argv, json!({ "env": env })), None, "{env}");
    }
}

#[test]
fn only_proc_run_is_read() {
    let plan = Plan {
        argv: Some(vec!["rm".into(), "a".into()]),
        ..Plan::default()
    };
    let other = theseus_tools::fs::WriteFile;
    assert_eq!(rm_operands(|| true, &other, &plan, &json!({})), None);
}

#[test]
fn a_bare_rm_is_read_only_when_it_is_the_systems() {
    let plan = Plan {
        argv: Some(vec!["rm".into(), "a".into()]),
        ..Plan::default()
    };
    let run = Run::default();
    assert!(rm_operands(|| true, &run, &plan, &json!({})).is_some());
    assert_eq!(rm_operands(|| false, &run, &plan, &json!({})), None);
    // The system's own names need no look at the PATH.
    let plan = Plan {
        argv: Some(vec!["/usr/bin/rm".into(), "a".into()]),
        ..Plan::default()
    };
    assert!(rm_operands(|| false, &run, &plan, &json!({})).is_some());
}
