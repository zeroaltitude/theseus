//! A self build's own test run waits for no one (issue 6, condition 1): an L1 call whose
//! every place is inside its own tree runs with a notice, though the owner's word on
//! `proc.run` holds L1 calls at approve; one that reaches out of the tree is still asked.

use super::*;

fn pinned() -> Rig {
    held(|cfg| {
        // The owner's own line for the universal shell.
        cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
    })
}

/// KEY: `cargo test` from the tree, in the sandbox, is not asked about.
#[tokio::test]
async fn a_test_run_in_its_own_sandbox_is_not_held_for_the_owner() {
    let r = pinned();
    let (sid, tree) = task(&r, "sails");
    r.turn(
        &sid,
        "proc_run",
        json!({"argv": ["echo", "tested"], "cwd": tree, "sandbox": true}),
    )
    .await;
    assert!(!asked(&r, &sid), "a run in its own tree, in L1");
}

/// The hold stays for a run that is not in the tree, in the same sandbox.
#[tokio::test]
async fn a_run_outside_its_tree_is_still_held() {
    let r = pinned();
    let (sid, _tree) = task(&r, "sails");
    let (_other_sid, other) = task(&r, "tides");
    r.turn(
        &sid,
        "proc_run",
        json!({"argv": ["echo", "no"], "cwd": other, "sandbox": true}),
    )
    .await;
    assert!(asked(&r, &sid), "another task's tree is not its own");
}

/// And for a session that is not a self session's.
#[tokio::test]
async fn a_session_that_is_not_a_self_task_is_still_held() {
    let r = pinned();
    let (_sid, tree) = task(&r, "sails");
    let sid = r.session("someone");
    r.turn(
        &sid,
        "proc_run",
        json!({"argv": ["echo", "no"], "cwd": tree, "sandbox": true}),
    )
    .await;
    assert!(asked(&r, &sid), "only a self task's own tree is open");
}
