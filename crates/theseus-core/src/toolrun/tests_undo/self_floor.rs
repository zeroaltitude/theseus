//! A self builder's test run of the daemon crate is not stopped at the owner (issue 20), through a whole
//! core. On 2026-11-17 an unattended build ran `cargo test -p theseusd` in its own working copy with no
//! `sandbox` key; the floor read the package name after `-p` as the program `theseusd`, held the call
//! for the owner, and the build parked. The call is now bound for the builder's sandbox, where the
//! floor is not in the view, and everything else the floor holds is held as it was.
//!
//! The commands here end in `true`, which is harmless wherever it runs: what a test reads is whether
//! the owner was asked (a `tool.confirm_requested` row, which names the floor and the reason) and which
//! class the job was started in (`jobs_of`: a sandboxed job carries its view). A pair makes each claim:
//! the call of a self builder runs, and the same call from another session, or one that names anything
//! else the floor holds, is asked.

use super::self_own::{base, builder, drive, drive_says, jobs_of, nothing, tree};
use super::*;

/// The call that parked the build, with `extra` at its end: a shell line in its own tree whose one tie
/// to the floor is the package name after `-p` (a program position to the floor).
fn line(extra: &'static str) -> impl FnOnce(&Path) -> Value {
    move |t| {
        let own = tree(t, "sails");
        json!({
            "command": format!("cd {}; true -p theseusd {extra}", own.display()),
            "cwd": own.to_string_lossy(),
        })
    }
}

impl Rig {
    /// The owner's questions the one call made.
    fn asked(&self) -> Vec<Value> {
        self.rows("tool.confirm_requested")
    }
}

/// KEY: the reproduced call is not asked, and its job is the builder's sandbox view with the
/// builder's one read-write folder.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_builders_test_run_of_the_daemon_crate_runs_in_the_sandbox_and_asks_nobody() {
    let (r, sid) = drive_says(REVERSIBLE, "self", "proc_run", line(""), nothing);
    r.turn(&sid).await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "{:?}", r.results(&sid));
    let view = jobs[0].sandbox.as_ref().expect("bound for L1");
    assert_eq!(
        view.rw,
        [builder(&r.target)],
        "the build folder, and no other"
    );
}

/// The same call from a person's session, a task's and a job's is asked, for the same reason the
/// builder's was, and starts nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_call_from_any_other_principal_is_asked() {
    for principal in ["operator", "task", "job"] {
        let (r, sid) = drive_says(REVERSIBLE, principal, "proc_run", line(""), nothing);
        r.turn(&sid).await;
        let asked = r.asked();
        assert_eq!(asked.len(), 1, "{principal}: {:?}", r.results(&sid));
        assert_eq!(asked[0]["floor"], true, "{principal}");
        let reason = asked[0]["reason"].as_str().unwrap();
        assert!(
            reason.contains("`theseusd` is Theseus's own binary"),
            "{principal}: the program rule is why: {reason}"
        );
        assert!(jobs_of(&sid).is_empty(), "{principal}: nothing ran");
    }
}

/// A self session that is no builder (its item's tree is not open), and a builder that asks for no
/// sandbox in so many words, are asked: nothing is moved for either.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_session_with_no_open_tree_or_an_explicit_no_sandbox_is_asked() {
    let (r, sid) = drive_says(REVERSIBLE, "self", "proc_run", line(""), nothing);
    std::fs::remove_dir_all(tree(&r.target, "sails").join(".git")).unwrap();
    r.turn(&sid).await;
    assert_eq!(r.asked().len(), 1, "no open tree: {:?}", r.results(&sid));
    assert!(jobs_of(&sid).is_empty());

    let explicit = |t: &Path| {
        let mut v = line("")(t);
        v["sandbox"] = json!(false);
        v
    };
    let (r, sid) = drive_says(REVERSIBLE, "self", "proc_run", explicit, nothing);
    r.turn(&sid).await;
    assert_eq!(r.asked().len(), 1, "sandbox: false: {:?}", r.results(&sid));
    assert!(jobs_of(&sid).is_empty());
}

/// Everything else the floor holds stays held for a self builder in its own tree: another floored
/// program beside the package name, the store, the bindings, the owner's instruction file and the
/// socket named by any word of the line. None of them is moved to the sandbox, and none starts.
#[tokio::test(flavor = "multi_thread")]
async fn what_else_the_floor_holds_is_still_held_for_a_self_builder() {
    type Aim = fn(&Path) -> String;
    let aims: [(&str, Aim); 6] = [
        ("another floored program", |_| "; op item get x".into()),
        ("the store", |t| {
            format!("; cat {}/state/store/x", base(t).display())
        }),
        ("the spool", |t| {
            format!("; cat {}/state/spool/x", base(t).display())
        }),
        ("the bindings", |t| {
            format!("; cat {}/state/bindings.toml", base(t).display())
        }),
        ("the owner's instructions", |t| {
            format!(
                "; cat {}/state/{}",
                base(t).display(),
                crate::project_context::USER_FILE
            )
        }),
        ("the store as an option's value", |t| {
            format!(" --manifest-path={}/state/store/x", base(t).display())
        }),
    ];
    for (what, aim) in aims {
        let call = move |t: &Path| {
            let mut v = line("")(t);
            let more = aim(t);
            let cmd = v["command"].as_str().unwrap().to_string();
            v["command"] = json!(format!("{cmd}{more}"));
            v
        };
        let (r, sid) = drive_says(REVERSIBLE, "self", "proc_run", call, nothing);
        r.turn(&sid).await;
        let asked = r.asked();
        assert_eq!(asked.len(), 1, "{what}: {:?}", r.results(&sid));
        assert_eq!(asked[0]["floor"], true, "{what}");
        assert!(jobs_of(&sid).is_empty(), "{what}: nothing ran");
    }
}

/// The state directory itself is not the floor: a self builder's L0 line that names its own tree and
/// no floored program was never asked, and still runs at L0 (no sandbox), as it did. This is what the
/// incident's own tree path was NOT: the tie to the floor was the program name.
#[tokio::test(flavor = "multi_thread")]
async fn naming_its_own_tree_alone_is_no_tie_to_the_floor() {
    let own = |t: &Path| {
        let own = tree(t, "sails");
        json!({"command": format!("cd {}; true", own.display()), "cwd": own.to_string_lossy()})
    };
    let (r, sid) = drive_says(REVERSIBLE, "self", "proc_run", own, nothing);
    r.turn(&sid).await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "{:?}", r.results(&sid));
    assert!(jobs[0].sandbox.is_none(), "a plain line stays where it was");
}

/// The reversibility layer reads the class the gate chose: a moved call that removes a file of the
/// working copy is a sandboxed removal, which keeps nothing, so it is not held on a "reshapeable"
/// answer (the same shell line at L0 is).
#[tokio::test(flavor = "multi_thread")]
async fn a_moved_call_that_removes_a_file_of_the_tree_is_judged_as_the_sandboxed_call_it_is() {
    let rm = |t: &Path| {
        let mut v = line("")(t);
        let cmd = v["command"].as_str().unwrap().to_string();
        let file = tree(t, "sails").join("notes.txt");
        v["command"] = json!(format!("{cmd}; rm -f {}", file.display()));
        v
    };
    let (r, sid) = drive("self", "proc_run", rm, nothing);
    r.turn(&sid).await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    assert!(jobs_of(&sid)[0].sandbox.is_some());
}
