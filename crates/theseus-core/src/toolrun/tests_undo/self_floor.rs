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
//!
//! Since issue 57 the owner's own conversation is moved the same way where L1 was seen to run at the
//! daemon's start, and he is told once instead of asked; everything else the floor holds still asks
//! him, and so does his own `[policy.tools]` line for `proc.run`. Which sessions are his, through
//! the real producers of a task, a routine's run and a fork, is `tests_owner_move`. The sessions
//! here are conversations opened with the principal a test names.

use super::self_own::{base, builder, drive, drive_launched, drive_says, jobs_of, nothing, tree};
use super::*;

/// The owner's conversation (the operator principal's), on a host where L1 ran its probe.
fn owners(input: impl FnOnce(&Path) -> Value, tweak: impl FnOnce(&mut Config)) -> (Rig, String) {
    let (r, sid) = drive_says(REVERSIBLE, "operator", "proc_run", input, tweak);
    r.core.tools.sandbox.set_l1_verified(true);
    (r, sid)
}

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

/// The same call from a task's session, a job's and an agent's MCP session is asked, for the same
/// reason the builder's was, and starts nothing. A person's session, the owner's, is not this since
/// issue 57: [`the_owners_own_call_runs_in_the_sandbox_and_he_is_told_once`].
#[tokio::test(flavor = "multi_thread")]
async fn the_same_call_from_any_other_principal_is_asked() {
    for principal in ["task", "job", crate::mcp_server::PRINCIPAL] {
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

/// Issue 57, KEY: the owner's own session's call that names `theseusd` is not asked. It runs in the
/// sandbox, whose view keeps no write (no read-write place: the workspace is an overlay whose writes
/// are scratch) and hides Theseus's store, spool and bindings, and the owner is told once, in words
/// that quote none of the command.
#[tokio::test(flavor = "multi_thread")]
async fn the_owners_own_call_runs_in_the_sandbox_and_he_is_told_once() {
    let (r, sid) = owners(line(""), nothing);
    r.turn(&sid).await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "{:?}", r.results(&sid));
    let view = jobs[0].sandbox.as_ref().expect("bound for L1");
    assert!(view.rw.is_empty(), "no write is kept: {:?}", view.rw);
    let state = base(&r.target).join("state");
    for kept in ["store", "spool", "bindings.toml"] {
        let p = theseus_tools::paths::canonical_best_effort(&state.join(kept));
        assert!(view.hidden.contains(&p), "{kept}: {:?}", view.hidden);
    }
    assert_eq!(
        crate::tests_places::told_notes(&r.core),
        [format!(
            "ℹ️ FYI · Theseus is starting a command that names theseusd, in the sandbox\n\
             conversation `{}` asked for it, and you were not asked\n\
             Theseus's store, settings file and socket are not in the sandbox, and what the command \
             writes in the sandbox is discarded when it ends. To be asked first, set \
             `[policy.tools] \"proc.run\" = \"approve\"`.",
            crate::task::short(&sid)
        )]
    );
}

/// Issue 57's neighbours: for the owner's own session, everything else the floor holds still asks
/// and starts nothing (the 1Password tool alone and beside `theseusd`, the store, the spool, the
/// bindings, his instruction file, the 1Password CLI's folder), and so does the same call with
/// `sandbox: false` or under his own `[policy.tools] "proc.run" = "approve"`. None is told.
#[tokio::test(flavor = "multi_thread")]
async fn what_else_the_floor_holds_still_asks_the_owner() {
    type Aim = fn(&Path) -> String;
    let aims: [(&str, Aim); 7] = [
        ("the 1Password tool beside it", |_| "; op item get x".into()),
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
        ("the 1Password CLI's folder", |_| {
            "; cat ~/.config/op/config".into()
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
        let (r, sid) = owners(call, nothing);
        r.turn(&sid).await;
        let asked = r.asked();
        assert_eq!(asked.len(), 1, "{what}: {:?}", r.results(&sid));
        assert_eq!(asked[0]["floor"], true, "{what}");
        assert!(jobs_of(&sid).is_empty(), "{what}: nothing ran");
        assert!(
            crate::tests_places::told_notes(&r.core).is_empty(),
            "{what}"
        );
    }
    // The 1Password tool on its own, with no `theseusd` at all.
    let op = |t: &Path| {
        let own = tree(t, "sails");
        json!({"command": format!("cd {}; op item get x", own.display()), "cwd": own.to_string_lossy()})
    };
    let (r, sid) = owners(op, nothing);
    r.turn(&sid).await;
    let asked = r.asked();
    assert_eq!(asked.len(), 1, "op alone: {:?}", r.results(&sid));
    assert_eq!(asked[0]["floor"], true, "op alone");
    assert!(jobs_of(&sid).is_empty(), "op alone: nothing ran");

    let explicit = |t: &Path| {
        let mut v = line("")(t);
        v["sandbox"] = json!(false);
        v
    };
    let (r, sid) = owners(explicit, nothing);
    r.turn(&sid).await;
    assert_eq!(r.asked().len(), 1, "sandbox: false: {:?}", r.results(&sid));
    assert_eq!(r.asked()[0]["floor"], true, "sandbox: false");
    assert!(jobs_of(&sid).is_empty());

    let approve = |c: &mut Config| {
        c.policy.tools.insert("proc.run".into(), Posture::Approve);
    };
    let (r, sid) = owners(line(""), approve);
    r.turn(&sid).await;
    let asked = r.asked();
    assert_eq!(asked.len(), 1, "his own line: {:?}", r.results(&sid));
    let reason = asked[0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("[policy.tools]"),
        "his own line is why: {reason}"
    );
    assert!(jobs_of(&sid).is_empty(), "his own line: nothing ran");
    assert!(crate::tests_places::told_notes(&r.core).is_empty());
}

/// Issue 57: where L1 was not seen to run, the owner's call asks as it did before, with the floor's
/// own card, and nothing starts; no note. Two hosts: one whose probe has not run (unknown is no),
/// and a non-root one whose probe ran and failed. The second is real, not set: the unit tests'
/// binary cannot be L1's init, so the daemon's own probe, `/bin/true` started as a job starts,
/// fails here as it would on a host without user namespaces.
#[tokio::test(flavor = "multi_thread")]
async fn where_l1_was_not_seen_to_run_the_owners_call_asks_as_before() {
    for probe in [false, true] {
        let (r, sid) = drive_says(REVERSIBLE, "operator", "proc_run", line(""), nothing);
        let sandbox = &r.core.tools.sandbox;
        assert!(sandbox.refusal().is_none(), "a non-root daemon");
        if probe {
            assert!(!sandbox.verify_l1().await, "the probe failed here");
        }
        assert!(!sandbox.l1_verified(), "probe {probe}");
        r.turn(&sid).await;
        let asked = r.asked();
        assert_eq!(asked.len(), 1, "probe {probe}: {:?}", r.results(&sid));
        assert_eq!(asked[0]["floor"], true, "probe {probe}");
        let reason = asked[0]["reason"].as_str().unwrap();
        assert!(
            reason.contains("`theseusd` is Theseus's own binary"),
            "probe {probe}: the floor's own card: {reason}"
        );
        assert!(jobs_of(&sid).is_empty(), "probe {probe}: nothing ran");
        assert!(
            crate::tests_places::told_notes(&r.core).is_empty(),
            "probe {probe}"
        );
    }
}

/// The note the owner's moved call stages, word for word.
fn moved_note(sid: &str) -> String {
    format!(
        "ℹ️ FYI · Theseus is starting a command that names theseusd, in the sandbox\n\
         conversation `{}` asked for it, and you were not asked\n\
         Theseus's store, settings file and socket are not in the sandbox, and what the command \
         writes in the sandbox is discarded when it ends. To be asked first, set \
         `[policy.tools] \"proc.run\" = \"approve\"`.",
        crate::task::short(sid)
    )
}

/// Issue 57: an L1 launch that fails stays a failure. The unit tests' binary cannot be L1's init,
/// so the moved job fails as it starts; it is not run again outside the sandbox (one job, and that
/// one bound for L1), and the one note staged said only what was true when it was written: that
/// Theseus was starting it.
#[tokio::test(flavor = "multi_thread")]
async fn a_moved_call_whose_l1_launch_fails_is_a_failure_and_its_note_was_true() {
    let (r, sid) = owners(line(""), nothing);
    r.turn(&sid).await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "never again at L0: {:?}", r.results(&sid));
    assert!(jobs[0].sandbox.is_some(), "the one job was L1's");
    let results = r.results(&sid);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].1.contains("could not start in L1"),
        "the launch failed, and says so: {results:?}"
    );
    assert_eq!(crate::tests_places::told_notes(&r.core), [moved_note(&sid)]);
}

/// Every job launched by [`Hangs`], which never ends one.
static HUNG: std::sync::Mutex<Vec<theseus_kernel::job::WrapperArgs>> =
    std::sync::Mutex::new(Vec::new());

/// A launcher whose jobs are still running: it starts nothing and never says one ended.
struct Hangs;

impl crate::toolrun::JobLauncher for Hangs {
    fn launch(
        &self,
        _: &theseus_kernel::spool::Spool,
        args: &theseus_kernel::job::WrapperArgs,
        _: crate::toolrun::JobDone,
    ) -> anyhow::Result<u32> {
        HUNG.lock().unwrap().push(args.clone());
        Ok(std::process::id())
    }
}

/// Issue 57: a moved call still running in the background has its note staged already, and the note
/// says only what is true while it runs: Theseus is running it, and what it writes in the sandbox is
/// discarded when it ends.
#[tokio::test(flavor = "multi_thread")]
async fn a_moved_call_still_running_has_a_note_that_is_true_while_it_runs() {
    let background = |t: &Path| {
        let mut v = line("")(t);
        v["background"] = json!(true);
        v
    };
    let (r, sid) = drive_launched(
        REVERSIBLE,
        "operator",
        "proc_run",
        background,
        nothing,
        Arc::new(Hangs),
    );
    r.core.tools.sandbox.set_l1_verified(true);
    r.turn(&sid).await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    let hung = HUNG.lock().unwrap().clone();
    assert_eq!(hung.len(), 1, "{:?}", r.results(&sid));
    assert!(hung[0].sandbox.is_some(), "bound for L1");
    assert!(r.rows("tool.late_result").is_empty(), "it has not ended");
    assert_eq!(crate::tests_places::told_notes(&r.core), [moved_note(&sid)]);
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

/// The working directory of a call is its own working copy, which was closed: the call fails at
/// launch with words the session can act on, and its ledger row says `tree_gone` (the nightly read
/// files nothing for it). A missing folder inside a working copy that is still open, a directory
/// that was never its own, and the same call from a person's session fail as before and carry no
/// mark, and are never told the working copy was closed.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_in_a_closed_working_copy_fails_marked_and_any_other_missing_directory_does_not() {
    let gone = |t: &Path| json!({"argv": ["true"], "cwd": tree(t, "sails").to_string_lossy()});
    let inside = |t: &Path| json!({"argv": ["true"], "cwd": tree(t, "sails").join("no-such-sub").to_string_lossy()});
    let elsewhere =
        |t: &Path| json!({"argv": ["true"], "cwd": base(t).join("no-such-dir").to_string_lossy()});
    type Case = (&'static str, &'static str, fn(&Path) -> Value, bool, bool);
    // (what, principal, call, whether the tree is removed first, whether it is marked)
    let cases: [Case; 4] = [
        ("its own tree, closed", "self", gone, true, true),
        (
            "a missing folder in a tree still open",
            "self",
            inside,
            false,
            false,
        ),
        (
            "a directory that was never its own",
            "self",
            elsewhere,
            true,
            false,
        ),
        (
            "a person's session, the same path",
            "operator",
            gone,
            true,
            false,
        ),
    ];
    for (what, principal, input, close, marked) in cases {
        let (r, sid) = drive_says(REVERSIBLE, principal, "proc_run", input, nothing);
        if close {
            std::fs::remove_dir_all(tree(&r.target, "sails")).unwrap();
        }
        r.turn(&sid).await;
        let results = r.results(&sid);
        assert_eq!(results.len(), 1, "{what}: {results:?}");
        assert_eq!(results[0].0, "Error", "{what}: {results:?}");
        assert!(
            results[0].1.contains("does not exist"),
            "{what}: {results:?}"
        );
        let said = results[0].1.contains("working copy was closed");
        assert_eq!(said, marked, "{what}: {results:?}");
        let failed = r.rows("action.failed");
        assert_eq!(failed.len(), 1, "{what}: {failed:?}");
        let word = failed[0].get("failure").and_then(Value::as_str);
        assert_eq!(word == Some("tree_gone"), marked, "{what}: {failed:?}");
    }
}
