//! Every command of a self-principal session runs in L1 (issue 53, round 6): its own `sandbox: false` included, a
//! helper task's included. From there a `git push` to the self repository's remote sends nothing, and a write to the
//! self repository's git configuration, a tag in it, or git's user-level config does not reach what the daemon's git
//! reads. A run of `git` or `gh` the broker would hand a credential is refused.

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

/// (a) and (d): a self session's `git push` to the self remote, asked for at L0 with `sandbox: false`, runs in L1
/// and sends nothing; so does its helper task's.
#[tokio::test]
async fn a_self_sessions_push_runs_in_l1_and_sends_nothing() {
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
        assert!(ran_in_l1(&r, s), "{who}: the command ran in L1");
        assert_eq!(remote_refs(&r), before, "{who}: nothing reached the remote");
    }
}

/// (b): from a self session's shell, a write to the self repository's git config, a tag in it, and a write to
/// git's user-level config do not reach what the daemon's git reads.
#[tokio::test]
async fn a_self_sessions_git_config_writes_do_not_reach_the_daemons_git() {
    let r = held(|cfg| cfg.policy.enforcement = Posture::Open);
    let (sid, tree) = task(&r, "sails");
    let repo = r.f.place.repo.clone();
    let config_before = std::fs::read(repo.join(".git").join("config")).ok();
    let script = format!(
        "git -C '{repo}' config remote.origin.pushurl /tmp/elsewhere; git -C '{repo}' tag around HEAD; \
         git config --global url./tmp/elsewhere.pushInsteadOf '{up}'; exit 0",
        repo = repo.display(),
        up = r.f.upstream.display()
    );
    r.turn(
        &sid,
        "proc_run",
        json!({"argv": ["sh", "-c", script], "cwd": tree}),
    )
    .await;
    assert!(ran_in_l1(&r, &sid), "the command ran in L1");
    let place = &r.f.place;
    let pushurl = place
        .git(&repo, &["config", "--get", "remote.origin.pushurl"])
        .unwrap();
    assert!(
        String::from_utf8_lossy(&pushurl.stdout).trim().is_empty(),
        "no pushurl reached the repository"
    );
    let tags = place.git_ok(&repo, &["tag", "--list"]).unwrap();
    assert!(tags.is_empty(), "no tag: {tags}");
    let rewrite = place
        .git(&repo, &["config", "--get-regexp", "pushinsteadof"])
        .unwrap();
    assert!(
        String::from_utf8_lossy(&rewrite.stdout).trim().is_empty(),
        "no user-level rewrite reached the daemon's git"
    );
    assert_eq!(
        std::fs::read(repo.join(".git").join("config")).ok(),
        config_before
    );
}

/// (c): from a self session's shell the daemon's state, where its store and its socket are, is not there to reach.
#[tokio::test]
async fn a_self_sessions_shell_cannot_reach_the_daemons_state() {
    let r = held(|cfg| cfg.policy.enforcement = Posture::Open);
    let (sid, tree) = task(&r, "sails");
    let store = r.f.place.state.join("store");
    assert!(store.exists());
    let probe = tree.join("probe.txt");
    let script = format!(
        "if [ -e '{}' ]; then echo SEEN; else echo UNSEEN; fi > '{}'",
        store.display(),
        probe.display()
    );
    r.turn(
        &sid,
        "proc_run",
        json!({"argv": ["sh", "-c", script], "cwd": tree}),
    )
    .await;
    assert!(ran_in_l1(&r, &sid), "the command ran in L1");
    let seen = std::fs::read_to_string(&probe).unwrap_or_default();
    assert_eq!(
        seen.trim(),
        "UNSEEN",
        "the store (and the socket beside it) is hidden: {seen}"
    );
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
