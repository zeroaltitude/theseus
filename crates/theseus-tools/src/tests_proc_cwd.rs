//! A working directory that is not there is refused when the call is
//! planned, as invalid input the model reads, not admitted and then failed as
//! a harness fault: the latter wrote a 0 ms `action.failed:harness:proc.run`
//! row with no reason.

use serde_json::{json, Value};

use crate::proc::Run;
use crate::{Tool, ToolCtx};

#[test]
fn a_missing_working_directory_is_refused_when_planned() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("sub")).unwrap();
    let c = ToolCtx::for_tests(d.path());
    let err = |i: Value| Run::default().plan(&i, &c).unwrap_err();
    let one = err(json!({"argv": ["ls"], "cwd": "nowhere"}));
    assert!(
        one.contains("working directory") && one.contains("does not exist"),
        "{one}"
    );
    let step =
        err(json!({"steps": [{"argv": ["ls"], "cwd": "sub"}, {"argv": ["ls"], "cwd": "nowhere"}]}));
    assert!(step.starts_with("step 2: working directory"), "{step}");
    let inherited = err(json!({"cwd": "nowhere", "steps": [{"argv": ["ls"]}]}));
    assert!(
        inherited.starts_with("step 1: working directory"),
        "{inherited}"
    );
    assert!(Run::default()
        .plan(&json!({"argv": ["ls"], "cwd": "sub"}), &c)
        .is_ok());
}

/// The session's own directory, gone, is not the model's input: the call
/// plans, and the launch is what finds the directory missing.
#[test]
fn the_sessions_own_missing_directory_plans_and_fails_at_launch() {
    let d = tempfile::tempdir().unwrap();
    let gone = ToolCtx::for_tests(&d.path().join("deleted"));
    assert!(Run::default()
        .plan(&json!({"command": "ls"}), &gone)
        .is_ok());
    assert!(Run::default()
        .job(&json!({"command": "ls"}), &gone)
        .is_err());
}
