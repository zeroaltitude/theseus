//! Every command of a self-principal session is bound to L1 (issue 53, round 6), its own `sandbox: false` included,
//! a helper task's included, and never runs at L0. This rig cannot start L1's init, so what L1 itself hides is not
//! observed here: `theseusd/tests/sandbox.rs` and `theseusd/tests/self_build.rs` run the real L1 for that. A run of
//! `git` or `gh` the broker would hand a credential is refused.

use super::*;

/// The self repository's remote's branches.
fn remote_refs(r: &Rig) -> String {
    super::super::upstream_refs(&r.f)
}

/// Whether `sid` started an L1 job: its `sandbox.started` row.
fn ran_in_l1(r: &Rig, sid: &str) -> bool {
    let rows = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap();
    rows.iter()
        .any(|(_, row)| row.kind == "sandbox.started" && row.session_id.as_deref() == Some(sid))
}

/// A helper task of `parent`'s task: its own session, its principal the self loop's.
fn helper_of(r: &Rig, parent: &str, id: &str) -> String {
    let sid = r.session("self");
    let t = TaskRecord {
        id: id.into(),
        version: 1,
        title: id.into(),
        state: TaskState::InProgress,
        session: Some(sid.clone()),
        origin: TaskOrigin {
            session: parent.into(),
            principal: "self".into(),
            ..Default::default()
        },
        ..TaskRecord::default()
    };
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    sid
}

/// A repository in `dir` with one commit, and the command that pushes it to the self remote's main.
fn pushing(r: &Rig, dir: &std::path::Path) -> Vec<String> {
    let repo = dir.join("work");
    std::fs::create_dir_all(&repo).unwrap();
    let g = |args: &[&str]| crate::rsi::tests_tree::git(&repo, args);
    g(&["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("leak.txt"), "SECRET pushed around the card\n").unwrap();
    g(&["add", "-A"]);
    g(&[
        "-c",
        "user.name=x",
        "-c",
        "user.email=x@example.invalid",
        "commit",
        "-q",
        "-m",
        "leak",
    ]);
    let script = format!(
        "cd '{}' && git push -q '{}' HEAD:refs/heads/around-the-card; git push -q origin HEAD:refs/heads/around-the-card; exit 0",
        repo.display(),
        r.f.upstream.display()
    );
    vec!["sh".into(), "-c".into(), script]
}

/// (a) and (d): a self session's `git push` to the self remote, asked for at L0 with `sandbox: false`, is bound to
/// L1 and does not run at L0, so nothing reaches the remote; so is its helper task's.
#[tokio::test]
async fn a_self_sessions_push_asked_at_l0_is_bound_to_l1_and_sends_nothing() {
    let r = held(|cfg| cfg.policy.enforcement = Posture::Open);
    let (sid, tree) = task(&r, "sails");
    let helper = helper_of(&r, &sid, "sails-helper");
    let before = remote_refs(&r);
    for (who, s) in [("builder", &sid), ("helper", &helper)] {
        let argv = pushing(&r, &tree.join(who));
        r.turn(
            s,
            "proc_run",
            json!({"argv": argv, "cwd": tree, "sandbox": false}),
        )
        .await;
        assert!(ran_in_l1(&r, s), "{who}: the command was bound to L1");
        assert_eq!(remote_refs(&r), before, "{who}: nothing reached the remote");
    }
}

/// A self session's `git` that the broker would hand a credential is refused in fixed words, and nothing runs.
#[tokio::test]
async fn a_credentialed_git_is_refused_to_a_self_session() {
    let r = held(|cfg| {
        cfg.policy.enforcement = Posture::Open;
        let grant = crate::config::ProgramGrant {
            env: [("GIT_TOKEN".to_string(), "a-token".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        cfg.broker.programs.insert("git".into(), grant);
    });
    let (sid, tree) = task(&r, "sails");
    let said = r
        .turn(
            &sid,
            "proc_run",
            json!({"argv": ["git", "push", "origin", "HEAD"], "cwd": tree}),
        )
        .await;
    assert!(
        said.contains(crate::rsi::own_tree::CREDENTIAL_REFUSAL),
        "{said}"
    );
    assert!(!ran_in_l1(&r, &sid), "nothing ran");
}
