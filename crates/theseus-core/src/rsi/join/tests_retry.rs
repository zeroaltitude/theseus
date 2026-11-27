//! A change its join holds at the owner's card is checked again on his `self retry` (issue 20):
//! the join runs every check on it from the first, a clean change joins, one the join still
//! stops is held again with a new card, a head already on main is closed as joined by hand, and
//! none of it is a try. Nothing but his word lifts the hold.

use crate::rsi::join::tests_publish::PassYes as _;
use theseus_protocol::rsi::{
    SelfFileParams, SelfMode, SelfPipelineParams, SelfRetryParams, SelfRetryResult,
};
use theseus_protocol::tasks::{TaskRecord, TaskState};

use super::tests::{git, rig, Rig, PROVED, SONNET};
use crate::approval::{Answerer, Refusal, Surface};
use crate::rsi::backlog::attempts;

/// Main's keel guard, as the real one reads its range: `erosion.txt` in what the branch changed,
/// while `marker` exists. Taking the marker away is a false alarm that was fixed.
fn guard_on_main(r: &Rig, marker: &std::path::Path) {
    let guard = "import os, subprocess, sys\n\
        a = sys.argv\n\
        repo, base, head = (a[a.index(k) + 1] for k in ('--repo', '--base', '--head'))\n\
        if os.path.exists('@MARKER@'):\n\
        \x20   out = subprocess.run(['git', 'diff', '--name-only', base, head], cwd=repo, capture_output=True, text=True).stdout.split()\n\
        \x20   if 'erosion.txt' in out:\n\
        \x20       print('tests/fix.sh:1: test-removed: a -> b')\n\
        \x20       sys.exit(1)\n"
        .replace("@MARKER@", &marker.to_string_lossy());
    let root = r.f.place.state.parent().unwrap().to_path_buf();
    let other = root.join("guard");
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("scripts/keel-guard.py"), guard).unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "the guard"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    // The branches seeded next are cut from main with this guard on it.
    git(&r.f.place.repo, &["fetch", "-q", "origin"]);
}

fn eroding() -> Vec<(&'static str, &'static str)> {
    let mut files = PROVED.to_vec();
    files.push(("erosion.txt", "gone\n"));
    files
}

impl Rig {
    /// The owner joins a branch by hand: its head is pushed to main, a fast-forward.
    fn joined_by_hand(&self, head: &str) {
        git(
            &self.f.place.repo,
            &["push", "-q", "origin", &format!("{head}:refs/heads/main")],
        );
    }
}

/// An item on the backlog, as the builder left it: in progress, its change built. The owner's
/// own filing makes the item, so it has the backlog's parent, as the retry reads it.
pub(super) fn item(r: &Rig) -> String {
    let filed = r
        .core
        .self_file(
            &SelfFileParams {
                finding: "The sails are not trimmed before the wind, so the ship loses speed."
                    .into(),
                title: Some("Trim the sails".into()),
                ..Default::default()
            },
            &owner(),
        )
        .unwrap();
    let mut t = crate::task_graph::get(&r.core.store, &filed.item)
        .unwrap()
        .unwrap();
    t.state = TaskState::InProgress;
    t.session = Some("ses_builder".into());
    t.version += 1;
    t.evidence.push(session_row("ses_builder", 1));
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    filed.item
}

/// The evidence a build's start leaves, at `at_ms`.
fn session_row(session: &str, at_ms: u64) -> theseus_protocol::tasks::TaskEvidence {
    theseus_protocol::tasks::TaskEvidence {
        node: None,
        identity: format!("session:{session}"),
        note: None,
        by: "self-backlog".into(),
        at_ms,
    }
}

pub(super) fn task(r: &Rig, id: &str) -> TaskRecord {
    crate::task_graph::get(&r.core.store, id).unwrap().unwrap()
}

pub(super) fn owner() -> Answerer {
    Answerer::from("the CLI")
}

pub(super) fn retry(
    r: &Rig,
    id: &str,
    who: &Answerer,
    from_job: Option<&str>,
) -> anyhow::Result<SelfRetryResult> {
    r.core.self_retry(
        &SelfRetryParams {
            item: id.into(),
            from_job: from_job.map(str::to_string),
            ..Default::default()
        },
        who,
    )
}

pub(super) fn held(r: &Rig) -> Vec<theseus_protocol::rsi::SelfPipelineItem> {
    r.core
        .self_pipeline(&SelfPipelineParams { week: false })
        .unwrap()
        .held
}

/// A change held at the keel by a guard's finding that was a false alarm: the owner's retry
/// sends it through the join again, and it joins as any accepted branch does. It was never
/// built or reviewed again, and it cost the item no try.
#[tokio::test]
async fn a_held_change_that_is_now_clean_joins_on_the_owners_retry() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let marker = r.f.place.state.parent().unwrap().join("alarm");
    std::fs::write(&marker, "false alarm").unwrap();
    guard_on_main(&r, &marker);
    let (branch, head) = r.seed(&id, &eroding(), SONNET);
    let main = r.upstream_main();
    let seams = r.seams();
    let one = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(
        one.stopped,
        [(branch.clone(), "keel".to_string())],
        "{one:?}"
    );
    assert_eq!(r.posts().len(), 1);
    let tries = attempts::tries(&task(&r, &id), 0);
    // Held for the owner, and the pipeline's line says how to check it again.
    let line = held(&r);
    assert_eq!(line.len(), 1, "{line:?}");
    let why = line[0].why.as_deref().unwrap();
    assert!(
        why.starts_with(&format!(
            "To have Theseus check it again from the first check, run `theseus self retry {id}`"
        )),
        "{why}"
    );
    // The false alarm is fixed. Nothing looks at the change again of itself, any number of passes
    // and any restart later.
    std::fs::remove_file(&marker).unwrap();
    for _ in 0..2 {
        let again = r.core.self_join_pass_yes(&seams).await.unwrap();
        assert!(
            again.stopped.is_empty() && again.joined.is_empty(),
            "{again:?}"
        );
    }
    assert!(r.rows("self.join.reopened").is_empty());
    assert_eq!(r.upstream_main(), main);
    assert_eq!(held(&r).len(), 1, "still held");
    // The owner's word.
    let said = retry(&r, &id, &owner(), None).unwrap();
    assert!(said.rechecked && said.rank == 0, "{said:?}");
    assert_eq!(held(&r).len(), 0, "no longer held");
    let two = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(two.joined.len(), 1, "{two:?}");
    assert!(two.reviewed.is_empty(), "no new review: {two:?}");
    assert_eq!(r.rows("self.review.verdict").len(), 1);
    assert_ne!(r.upstream_main(), main);
    assert_eq!(
        git(&r.f.upstream, &["show", "refs/heads/main:erosion.txt"]),
        "gone"
    );
    assert_eq!(r.rows("self.joined")[0]["head"], head.as_str());
    // Not a try, and not the item's retry: its attempts stand as they were.
    let t = task(&r, &id);
    assert_eq!(attempts::tries(&t, 0), tries);
    assert_eq!(
        attempts::last_retry([&t]),
        0,
        "the count did not start again"
    );
    assert_eq!(t.state, TaskState::InProgress);
    assert!(t.evidence.iter().any(|e| e.identity == "rechecked:owner"));
    assert!(!t.evidence.iter().any(|e| e.identity == "retried:owner"));
    assert_eq!(r.rows("self.join.reopened").len(), 1);
}

/// KEY: the retry never joins what the join would not. A change that still touches the keel is
/// held again, with a second card, and nothing reaches main, however often the owner asks.
#[tokio::test]
async fn a_held_change_that_still_hits_the_keel_is_held_again_with_a_second_card() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let mut files = PROVED.to_vec();
    files.push(("scripts/long-files.txt", "crates/x.rs 9999\n"));
    let (branch, _) = r.seed(&id, &files, SONNET);
    let main = r.upstream_main();
    let seams = r.seams();
    let one = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(
        one.stopped,
        [(branch.clone(), "keel".to_string())],
        "{one:?}"
    );
    let tries = attempts::tries(&task(&r, &id), 0);
    for card in 2..=3 {
        retry(&r, &id, &owner(), None).unwrap();
        let again = r.core.self_join_pass_yes(&seams).await.unwrap();
        assert_eq!(
            again.stopped,
            [(branch.clone(), "keel".to_string())],
            "{again:?}"
        );
        assert!(again.joined.is_empty(), "{again:?}");
        let posts = r.posts();
        assert_eq!(posts.len(), card, "a card each time: {posts:?}");
        assert!(
            posts[card - 1].contains("scripts/long-files.txt")
                && posts[card - 1].contains(&format!("theseus self retry {id}")),
            "{}",
            posts[card - 1]
        );
        assert_eq!(r.upstream_main(), main, "nothing joined");
        assert_eq!(held(&r).len(), 1, "held again");
    }
    // Held again means it waits: a pass alone does nothing more.
    let idle = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(
        idle.stopped.is_empty() && idle.joined.is_empty(),
        "{idle:?}"
    );
    assert_eq!(r.posts().len(), 3);
    assert_eq!(attempts::tries(&task(&r, &id), 0), tries, "no try");
    assert_eq!(task(&r, &id).state, TaskState::InProgress);
}

/// A change the owner joined by hand: its head is on main, so the retry closes the item as done,
/// its cause is not forbidden, and no card is posted.
#[tokio::test]
async fn a_held_change_already_on_main_is_closed_as_joined_by_hand() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let marker = r.f.place.state.parent().unwrap().join("alarm");
    std::fs::write(&marker, "").unwrap();
    guard_on_main(&r, &marker);
    let (branch, head) = r.seed(&id, &eroding(), SONNET);
    let seams = r.seams();
    r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(held(&r).len(), 1);
    let tries = attempts::tries(&task(&r, &id), 0);
    // The owner joins it by hand.
    r.joined_by_hand(&head);
    let still = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(still.stopped.is_empty(), "held until he says: {still:?}");
    retry(&r, &id, &owner(), None).unwrap();
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(
        pass.stopped,
        [(branch.clone(), "merge".to_string())],
        "{pass:?}"
    );
    assert!(
        pass.joined.is_empty() && pass.reviewed.is_empty(),
        "{pass:?}"
    );
    let t = task(&r, &id);
    assert_eq!(t.state, TaskState::Done);
    assert!(
        t.evidence
            .iter()
            .any(|e| e.identity.starts_with(&format!("joined-by-hand:{branch}@"))),
        "{:?}",
        t.evidence
    );
    assert_eq!(attempts::tries(&t, 0), tries);
    assert!(held(&r).is_empty(), "it left Held for the owner");
    assert!(r.rows("self.joined").is_empty(), "Theseus joined nothing");
    assert_eq!(r.upstream_main(), head);
    assert_eq!(r.posts().len(), 1, "no card for a change that is on main");
    // Its cause is not forbidden: nothing was dropped.
    let list = r
        .core
        .self_backlog(&theseus_protocol::rsi::SelfBacklogParams { all: true })
        .unwrap();
    assert!(list.dropped.is_empty(), "{:?}", list.dropped);
    // And it stays closed.
    let after = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(
        after.stopped.is_empty() && after.joined.is_empty(),
        "{after:?}"
    );
}

/// A head found on main that no retry lifted a hold from is held as it was: only the owner's
/// word closes an item, and nothing of it is the loop's own.
#[tokio::test]
async fn a_head_on_main_with_no_retry_is_not_closed_by_the_loop() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let (branch, head) = r.seed(&id, PROVED, SONNET);
    // Accepted, and waiting for its join when the owner puts it on main by hand.
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfReviewVerdict {
            item: id.clone(),
            branch: branch.clone(),
            head: head.clone(),
            reviewer: "fable (claude-fable-5-2)".into(),
            model: "claude-fable-5-2".into(),
            asked: "claude-fable-5-2".into(),
            verdict: "accept".into(),
            planted_revert: "failed".into(),
            proof_check: None,
            reasons: vec!["the stand-in accepts".into()],
            cost_usd: 0.5,
        });
    r.joined_by_hand(&head);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "merge".to_string())], "{pass:?}");
    assert_eq!(task(&r, &id).state, TaskState::InProgress);
    assert_eq!(held(&r).len(), 1);
}

/// The merge is of the head the review passed, never of the branch's tip now: a branch moved
/// after its review is not what a retry joins.
#[tokio::test]
async fn a_branch_moved_after_its_review_is_not_what_a_retry_joins() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let marker = r.f.place.state.parent().unwrap().join("alarm");
    std::fs::write(&marker, "").unwrap();
    guard_on_main(&r, &marker);
    let (branch, head) = r.seed(&id, &eroding(), SONNET);
    let seams = r.seams();
    r.core.self_join_pass_yes(&seams).await.unwrap();
    // The branch moves on after its review: a commit the review never saw.
    let repo = &r.f.place.repo;
    let wt = r.f.place.state.parent().unwrap().join("mover-wt");
    git(
        repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            wt.to_str().unwrap(),
            &head,
        ],
    );
    std::fs::write(wt.join("unreviewed.txt"), "never reviewed\n").unwrap();
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-q", "-m", "after the review"]);
    let moved = git(&wt, &["rev-parse", "HEAD"]);
    git(
        repo,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
    git(repo, &["branch", "-f", &branch, &moved]);
    // The alarm is over and the owner asks: the reviewed head joins, and what came after does not.
    std::fs::remove_file(&marker).unwrap();
    retry(&r, &id, &owner(), None).unwrap();
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let on_main = git(
        &r.f.upstream,
        &["ls-tree", "-r", "--name-only", "refs/heads/main"],
    );
    assert!(on_main.contains("erosion.txt"), "{on_main}");
    assert!(!on_main.contains("unreviewed.txt"), "{on_main}");
}

/// Only the owner's word: a job's shell, and a surface that is never the owner's, are refused as
/// they are for any retry; nothing is written and the change stays held.
#[tokio::test]
async fn a_retry_by_anyone_but_the_owner_is_refused_as_today() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let mut files = PROVED.to_vec();
    files.push(("scripts/long-files.txt", "crates/x.rs 9999\n"));
    r.seed(&id, &files, SONNET);
    let seams = r.seams();
    r.core.self_join_pass_yes(&seams).await.unwrap();
    let before = task(&r, &id);
    let job = retry(&r, &id, &owner(), Some("ses_job")).unwrap_err();
    assert!(job.downcast_ref::<Refusal>().is_some(), "{job:#}");
    let mut mcp = owner();
    mcp.surface = Surface::Mcp;
    let other = retry(&r, &id, &mcp, None).unwrap_err();
    assert!(other.downcast_ref::<Refusal>().is_some(), "{other:#}");
    assert!(r.rows("self.join.reopened").is_empty());
    assert_eq!(
        task(&r, &id).version,
        before.version,
        "the item is untouched"
    );
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(
        pass.stopped.is_empty() && pass.joined.is_empty(),
        "{pass:?}"
    );
    assert_eq!(held(&r).len(), 1, "still held");
}

/// The refusal for an item no join holds stays, and now names the third case; a second retry on
/// a change that is no longer held is refused the same way.
#[tokio::test]
async fn an_item_no_join_holds_is_refused_in_words_that_name_the_held_case() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let e = retry(&r, &id, &owner(), None).unwrap_err();
    let said = format!("{e}");
    assert!(
        said.contains("is in_progress")
            && said.contains("held at your card by its join")
            && said.contains("this one is not held"),
        "{said}"
    );
    assert!(r.rows("self.join.reopened").is_empty());
    // Held, retried, and retried again before the pass has run: the second finds it let back in.
    let mut files = PROVED.to_vec();
    files.push(("scripts/long-files.txt", "crates/x.rs 9999\n"));
    r.seed(&id, &files, SONNET);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    retry(&r, &id, &owner(), None).unwrap();
    assert!(retry(&r, &id, &owner(), None).is_err());
    assert_eq!(r.rows("self.join.reopened").len(), 1);
}

/// A head the item was sent back from is not the item's change: the sealed hold returns the item
/// to its builder, and once the rebuild has started the old head is not offered, not reopened and
/// not measured again (the first review's finding). The item is left as it is.
#[tokio::test]
async fn a_head_its_item_was_sent_back_from_is_not_reopened_during_the_rebuild() {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    let mut files = PROVED.to_vec();
    files.push(("scripts/long-files.txt", "crates/x.rs 9999\n"));
    r.seed(&id, &files, SONNET);
    let seams = r.seams();
    r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(held(&r)[0]
        .why
        .as_deref()
        .unwrap()
        .contains("theseus self retry"));
    // The item goes back to its builder and a new build starts, as after a sealed hold: the
    // item is in progress again under a new session, and nothing is green since.
    let mut t = task(&r, &id);
    t.session = Some("ses_rebuild".into());
    t.version += 1;
    t.evidence.push(session_row(
        "ses_rebuild",
        theseus_protocol::now_unix_ms() + 1,
    ));
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    let before = task(&r, &id);
    let said = retry(&r, &id, &owner(), None).unwrap_err();
    assert!(format!("{said}").contains("this one is not held"), "{said}");
    assert!(r.rows("self.join.reopened").is_empty());
    let after = task(&r, &id);
    assert_eq!(
        (after.version, after.state, after.session.clone()),
        (
            before.version,
            TaskState::InProgress,
            Some("ses_rebuild".into())
        )
    );
    // And the old head is not offered to the owner as a change to check again.
    let why = held(&r)
        .first()
        .and_then(|i| i.why.clone())
        .unwrap_or_default();
    assert!(!why.contains("theseus self retry"), "{why}");
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(
        pass.stopped.is_empty() && pass.joined.is_empty(),
        "{pass:?}"
    );
}
