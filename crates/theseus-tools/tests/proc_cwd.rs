//! A working directory the call names that is not there is refused when the
//! call is planned: invalid input, which the model reads and answers, and not
//! a call admitted and then failed as a fault of the harness. That failure
//! was a 0 ms `action.failed:harness:proc.run` row, and the item filed for it
//! could not say why.
//!
//! This is a test target of its own, not a module that `lib.rs` declares: the
//! planted revert undoes every non-test file a branch changes, and a
//! declared module goes with them, leaving tests that do not run and so pass.

use serde_json::{json, Value};
use theseus_tools::proc::Run;
use theseus_tools::{Tool, ToolCtx};

/// What planning `input` is refused with.
fn refusal(c: &ToolCtx, input: Value) -> String {
    Run::default().plan(&input, c).unwrap_err()
}

#[test]
fn a_call_naming_a_directory_that_is_not_there_is_refused_when_planned() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("sub")).unwrap();
    let c = ToolCtx::for_tests(d.path());
    for (input, missing) in [
        (json!({"argv": ["ls"], "cwd": "nowhere"}), "nowhere"),
        (json!({"command": "ls", "cwd": "nowhere"}), "nowhere"),
        (
            json!({"argv": ["ls"], "cwd": "nowhere", "background": true}),
            "nowhere",
        ),
        (json!({"argv": ["ls"], "cwd": "sub/nowhere"}), "sub/nowhere"),
    ] {
        let e = refusal(&c, input.clone());
        assert!(
            e.starts_with("working directory ") && e.ends_with(" does not exist"),
            "{input}: {e}"
        );
        let path = c.cwd.join(missing).display().to_string();
        assert!(e.contains(&path), "it names the directory, {path}: {e}");
    }
    // A directory that is there plans, named or not.
    for input in [
        json!({"argv": ["ls"], "cwd": "sub"}),
        json!({"argv": ["ls"], "cwd": "."}),
        json!({"argv": ["ls"]}),
    ] {
        assert!(Run::default().plan(&input, &c).is_ok(), "{input}");
    }
}

#[test]
fn a_batch_with_a_step_in_a_directory_that_is_not_there_is_refused_naming_the_step() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("sub")).unwrap();
    let c = ToolCtx::for_tests(d.path());
    let second = refusal(
        &c,
        json!({"steps": [{"argv": ["ls"], "cwd": "sub"}, {"argv": ["ls"], "cwd": "nowhere"}]}),
    );
    assert!(
        second.starts_with("step 2: working directory ") && second.ends_with(" does not exist"),
        "{second}"
    );
    // A step that names none runs in the call's.
    let inherited = refusal(&c, json!({"cwd": "nowhere", "steps": [{"argv": ["ls"]}]}));
    assert!(
        inherited.starts_with("step 1: working directory "),
        "{inherited}"
    );
    // With every directory there, the batch plans.
    let there = json!({"cwd": "sub", "steps": [{"argv": ["ls"]}, {"argv": ["ls"], "cwd": "."}]});
    assert!(Run::default().plan(&there, &c).is_ok());
}

/// The session's own directory, when it is gone, is not what the model wrote:
/// the call plans, as it did, and the launch is what finds the directory
/// missing. (Refusing it at the gate too is a change for its own item: tests
/// of the gate plan calls in a session directory that was never made.)
#[test]
fn the_sessions_own_missing_directory_plans_and_is_refused_at_the_launch() {
    let d = tempfile::tempdir().unwrap();
    let gone = ToolCtx::for_tests(&d.path().join("deleted"));
    let call = json!({"command": "ls"});
    assert!(Run::default().plan(&call, &gone).is_ok());
    let e = Run::default().jobs(&call, &gone).unwrap_err();
    assert!(e.contains("does not exist"), "{e}");
}

/// A directory can go between the gate and the launch (a tree removed under
/// its session): the launch asks again, for a call and for each of a
/// batch's steps, and a call planned after it goes is refused at the gate.
#[test]
fn a_directory_that_goes_after_the_plan_is_refused_at_the_launch() {
    let d = tempfile::tempdir().unwrap();
    let work = d.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let c = ToolCtx::for_tests(d.path());
    let call = json!({"argv": ["ls"], "cwd": "work"});
    let batch = json!({"steps": [{"argv": ["ls"], "cwd": "work"}]});
    assert!(Run::default().plan(&call, &c).is_ok());
    assert!(Run::default().plan(&batch, &c).is_ok());
    assert!(Run::default().jobs(&call, &c).is_ok());
    std::fs::remove_dir(&work).unwrap();
    let e = Run::default().jobs(&call, &c).unwrap_err();
    assert!(e.contains("does not exist"), "{e}");
    let e = Run::default().jobs(&batch, &c).unwrap_err();
    assert!(e.starts_with("step 1: "), "{e}");
    assert!(Run::default().plan(&call, &c).is_err());
}
