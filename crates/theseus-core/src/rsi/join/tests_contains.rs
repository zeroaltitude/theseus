//! The words for the last join when the build that runs came later (issue 20): by hand, after the join's own install
//! did not take, an install of a later commit of main runs the join inside it. The daemon tells from its own clone.

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
    let yes = said(&[joined.clone()], Contained::Yes(later.clone()));
    assert_eq!(yes.install, Install::Running);
    assert_eq!(
        yes.line,
        format!(
            "joined at {}; installed and running (inside the running build, {}, which contains it)",
            &a[..12],
            &later[..12]
        )
    );
    let no = said(&[joined.clone()], Contained::No(later.clone())).line;
    assert!(
        no.contains("not running yet")
            && no.contains(&later[..12])
            && no.contains("does not contain it")
            && no.contains("docs/setup.md"),
        "{no}"
    );
    let unsure = said(
        &[joined.clone()],
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
    let own = said(&[joined.clone()], Contained::Yes(a.clone())).line;
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
    for c in [
        Contained::No(later.clone()),
        Contained::CannotTell("w".into()),
    ] {
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
    r.core.self_join_pass(&r.seams()).await.unwrap();
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
