//! The words for the last join when the build that runs came later (issue 20): by hand, after the join's own install
//! did not take, an install of a later commit of main runs the join inside it. The daemon tells from its own clone.

use crate::rsi::join::tests_publish::PassYes as _;
use serde_json::json;
use theseus_protocol::rsi::SelfMode;

use super::tests::{git, rig, PROVED, SONNET};
use super::words::{last_join_in, Contained, Install};
use crate::ledger::LedgerRow;

fn row(kind: &str, data: serde_json::Value) -> LedgerRow {
    LedgerRow::named(kind, None, None, data)
}

/// A commit on top of `parent` in `repo`, with `parent`'s tree.
fn child_of(repo: &std::path::Path, parent: &str) -> String {
    let tree = git(repo, &["rev-parse", &format!("{parent}^{{tree}}")]);
    git(
        repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit-tree",
            &tree,
            "-p",
            parent,
            "-m",
            "a later landing",
        ],
    )
}

/// The three answers, mapped to the words, whatever the join's own install rows say.
#[test]
fn the_words_follow_whether_the_running_build_contains_the_join() {
    let (a, later) = ("a".repeat(40), "c".repeat(40));
    let joined = row("self.joined", json!({"merge": a}));
    let said = |rows: &[LedgerRow], c: Contained| last_join_in(rows, &|_| Some(c.clone())).unwrap();
    // Nothing says the install finished: a later build that contains it makes it run; one that does not, does not.
    let yes = said(std::slice::from_ref(&joined), Contained::Yes(later.clone()));
    assert_eq!(yes.install, Install::Running);
    assert_eq!(
        yes.line,
        format!(
            "joined at {}; installed and running (inside the running build, {}, which contains it)",
            &a[..12],
            &later[..12]
        )
    );
    let no = said(std::slice::from_ref(&joined), Contained::No(later.clone())).line;
    assert!(
        no.contains("not running yet")
            && no.contains(&later[..12])
            && no.contains("does not contain it")
            && no.contains("docs/setup.md"),
        "{no}"
    );
    let unsure = said(
        std::slice::from_ref(&joined),
        Contained::CannotTell("it has not fetched".into()),
    )
    .line;
    assert_eq!(
        unsure,
        format!(
            "joined at {}; not confirmed running: cannot tell whether the running build contains it (it has not fetched)",
            &a[..12]
        )
    );
    // The join's own build is the running one: the plain words.
    let own = said(std::slice::from_ref(&joined), Contained::Yes(a.clone())).line;
    assert!(own.ends_with("installed and running"), "{own}");
    // An install that did not take, then a hand install of a later build: it runs. Where it still does not contain
    // it, or cannot tell, the install's own words (why, and the one command) stand.
    let failed = row(
        "self.root_install",
        json!({"merge": a, "step": "rolled_back", "why": "the new build is not what is running", "numbers": {"step": "sudo install x"}}),
    );
    let rows = [joined.clone(), failed];
    assert_eq!(
        said(&rows, Contained::Yes(later.clone())).install,
        Install::Running
    );
    for c in [Contained::No(later), Contained::CannotTell("w".into())] {
        let j = said(&rows, c);
        assert_eq!(j.install, Install::DidNotTake);
        assert!(
            j.line.contains("did not take") && j.line.contains("sudo install x"),
            "{}",
            j.line
        );
    }
    // A row that says it runs is not asked about.
    let running = [joined, row("self.installed", json!({"merge": a}))];
    let j = last_join_in(&running, &|_| panic!("asked")).unwrap();
    assert_eq!(j.install, Install::Running);
}

/// The daemon's own clone decides: a descendant of the join contains it, an ancestor does not, a commit the clone
/// lacks cannot be told, and a build that names no commit cannot either.
#[tokio::test]
async fn the_daemon_tells_from_its_own_clone_whether_the_running_build_has_the_join() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    let merge = r.upstream_main();
    let before = git(&r.f.upstream, &["rev-parse", &format!("{merge}^1")]);
    // Main moves on, and the daemon's clone fetches it.
    let later = child_of(&r.f.upstream, &merge);
    git(&r.f.upstream, &["update-ref", "refs/heads/main", &later]);
    git(&r.f.place.repo, &["fetch", "-q", "origin"]);

    let j = r.core.self_last_join_as(Some(&later)).unwrap();
    assert_eq!(j.install, Install::Running, "{}", j.line);
    assert!(
        j.line.contains("installed and running") && j.line.contains(&later[..12]),
        "{}",
        j.line
    );
    // Not told as a hand install's dirt either: `health` does not call it not running.
    assert_eq!(r.core.self_joined_not_running(), None);

    let j = r.core.self_last_join_as(Some(&before)).unwrap();
    assert_eq!(j.install, Install::Unconfirmed);
    assert!(
        j.line.contains("does not contain it") && j.line.contains(&before[..12]),
        "{}",
        j.line
    );

    let absent = "9".repeat(40);
    let j = r.core.self_last_join_as(Some(&absent)).unwrap();
    assert!(
        j.line.contains("cannot tell") && j.line.contains("has not fetched"),
        "{}",
        j.line
    );

    let j = r.core.self_last_join_as(None).unwrap();
    assert!(
        j.line.contains("cannot tell") && j.line.contains("does not name its commit"),
        "{}",
        j.line
    );
}

/// Issue 41: after an install from outside the loop the clone lacks the running build's commit. Before the start's look
/// the answer says plainly that it has not looked yet; the start's one fetch finds the commit and the answer is told; a
/// commit the remote does not have is said so after the look, and a clone that has the commit fetches nothing.
#[tokio::test]
async fn the_start_looks_for_the_running_builds_commit_before_it_answers() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    let merge = r.upstream_main();
    // Built and installed from outside the loop: main moved on upstream, and the clone has not fetched it.
    let later = child_of(&r.f.upstream, &merge);
    git(&r.f.upstream, &["update-ref", "refs/heads/main", &later]);
    let j = r.core.self_last_join_as(Some(&later)).unwrap();
    assert_eq!(j.install, Install::Unconfirmed, "{}", j.line);
    assert!(j.line.contains("has not looked yet"), "{}", j.line);
    assert!(r.core.fetch_running_as(Some(&later)), "the start fetches");
    let j = r.core.self_last_join_as(Some(&later)).unwrap();
    assert_eq!(j.install, Install::Running, "{}", j.line);
    assert!(
        !r.core.fetch_running_as(Some(&later)),
        "a clone that has the commit fetches nothing"
    );
    let absent = "8".repeat(40);
    assert!(r.core.fetch_running_as(Some(&absent)));
    let j = r.core.self_last_join_as(Some(&absent)).unwrap();
    assert!(
        j.line.contains("fetched main since it started") && j.line.contains("still does not have"),
        "{}",
        j.line
    );
}

// ---- a join that was undone is never said to run ---------------------------------------------------

/// A revert is a later commit that has the join as an ancestor, so a build that holds the revert "contains" the join
/// by ancestry; the words read the revert's own row and never say a reverted join runs.
#[test]
fn a_reverted_join_is_never_said_to_run() {
    let (a, revert, running) = ("a".repeat(40), "d".repeat(40), "e".repeat(40));
    let joined = row("self.joined", json!({"merge": a}));
    for (kind, by) in [
        ("self.reverted", "the loop's own revert after a red signal"),
        ("self.vetoed", "the owner's revert"),
    ] {
        let undone = row(kind, json!({"merge": a, "revert": revert}));
        // Even with a row that says the join's own install ran: it was undone.
        let rows = [
            joined.clone(),
            row("self.installed", json!({"merge": a})),
            undone.clone(),
        ];
        // The question is about the REVERT's commit, and the join's own is never asked.
        let said = |c: Contained| {
            last_join_in(&rows, &|asked| {
                assert_eq!(
                    asked, revert,
                    "{kind}: asked about the join, not its revert"
                );
                Some(c.clone())
            })
            .unwrap()
        };
        let yes = said(Contained::Yes(running.clone()));
        assert_eq!(yes.install, Install::Reverted, "{kind}");
        assert!(
            yes.line.contains(by)
                && yes.line.contains("contains that revert")
                && yes.line.contains("not what runs")
                && !yes.line.contains("installed and running"),
            "{}",
            yes.line
        );
        let no = said(Contained::No(running.clone())).line;
        assert!(
            no.contains("does not contain that revert yet")
                && !no.contains("installed and running"),
            "{no}"
        );
        let unsure = said(Contained::CannotTell("it has not fetched".into())).line;
        assert!(
            unsure.contains(
                "cannot tell whether the running build contains that revert (it has not fetched)"
            ),
            "{unsure}"
        );
        // Nothing asked (the plain words): still said to be undone.
        let plain = last_join_in(&[joined.clone(), undone], &|_| None).unwrap();
        assert_eq!(plain.install, Install::Reverted);
        assert!(
            plain.line.contains(&format!("reverted on main by {by}")),
            "{}",
            plain.line
        );
    }
}

/// The reviewer's probe, on a real clone: the join, a revert of it on top (main's tree from before the join), main
/// moved there and fetched, and the running build the revert. With the row of either kind it says reverted, not running.
#[tokio::test]
async fn a_revert_of_the_join_in_the_running_build_is_said_as_a_revert() {
    for kind in ["self.reverted", "self.vetoed"] {
        let r = rig(SelfMode::Act);
        r.seed("trim-the-sails", PROVED, SONNET);
        r.core.self_join_pass_yes(&r.seams()).await.unwrap();
        let merge = r.upstream_main();
        let before = git(&r.f.upstream, &["rev-parse", &format!("{merge}^1")]);
        // The revert: a commit on the join with the tree from before it.
        let tree = git(&r.f.upstream, &["rev-parse", &format!("{before}^{{tree}}")]);
        let revert = git(
            &r.f.upstream,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "commit-tree",
                &tree,
                "-p",
                &merge,
                "-m",
                "Revert the join",
            ],
        );
        git(&r.f.upstream, &["update-ref", "refs/heads/main", &revert]);
        git(&r.f.place.repo, &["fetch", "-q", "origin"]);
        // Premise: by ancestry alone the build with the revert contains the join, and would read as running.
        let j = r.core.self_last_join_as(Some(&revert)).unwrap();
        assert_eq!(j.install, Install::Running, "premise: {}", j.line);
        r.core
            .store
            .append_ledger(&LedgerRow::named(
                kind,
                None,
                None,
                json!({"merge": merge, "revert": revert}),
            ))
            .unwrap();
        let j = r.core.self_last_join_as(Some(&revert)).unwrap();
        assert_eq!(j.install, Install::Reverted, "{kind}: {}", j.line);
        assert!(
            j.line.contains("contains that revert") && !j.line.contains("installed and running"),
            "{}",
            j.line
        );
        assert_eq!(r.core.self_joined_not_running(), None);
        // A build from before the revert may still run the change: said so, not "running".
        let j = r.core.self_last_join_as(Some(&merge)).unwrap();
        assert!(
            j.line.contains("does not contain that revert yet"),
            "{}",
            j.line
        );
    }
}

// ---- what a failed `merge-base` reads as -------------------------------------------------------------

fn finished(raw: i32) -> anyhow::Result<std::process::Output> {
    use std::os::unix::process::ExitStatusExt;
    Ok(std::process::Output {
        status: std::process::ExitStatus::from_raw(raw),
        stdout: Vec::new(),
        stderr: b"fatal: not a valid object name".to_vec(),
    })
}

/// Only exit 0 is yes and only exit 1 is no. Any other code, a signal, or a command that did not run cannot tell.
#[test]
fn only_exit_zero_is_a_yes_and_only_exit_one_is_a_no() {
    use super::contains::classify;
    let run = "f".repeat(40);
    assert_eq!(classify(finished(0), &run), Contained::Yes(run.clone()));
    assert_eq!(classify(finished(1 << 8), &run), Contained::No(run.clone()));
    for (what, result) in [
        ("exit 128", finished(128 << 8)),
        ("exit 2", finished(2 << 8)),
        ("killed by SIGKILL", finished(9)),
        ("dumped core on SIGSEGV", finished(11 | 0x80)),
        ("could not run", Err(anyhow::anyhow!("git could not start"))),
    ] {
        match classify(result, &run) {
            Contained::CannotTell(why) => assert!(!why.is_empty(), "{what}"),
            other => panic!("{what} read as {other:?}"),
        }
    }
}
