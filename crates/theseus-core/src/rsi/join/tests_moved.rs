//! A finished change that no longer merges is sent back to its builder (issue 20): main is moved
//! under a green branch, and before its review is spent the branch goes back with main merged
//! into a working copy of it, the builder's resolution joins, and none of it is a failed try.

use crate::rsi::join::tests_publish::PassYes as _;
use theseus_protocol::rsi::SelfMode;
use theseus_protocol::tasks::{TaskRecord, TaskState};
use theseus_protocol::SessionKind;

use super::tests::{git, rig, Rig, PROVED, SONNET};
use super::tests_notify::{conflicting, item, main_moves};
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::rsi::backlog::attempts;
use crate::rsi::tree;
use crate::session::{SessionRecord, TargetRef};

fn task(r: &Rig) -> TaskRecord {
    crate::task_graph::get(&r.core.store, "tsk_sails")
        .unwrap()
        .unwrap()
}

/// What the builder does once its tree holds main: resolves the clash and commits through the
/// tool's own commit, then leaves a green row for the branch it is on, as `self.propose_change`
/// does. The new branch and its head.
fn resolved(r: &Rig) -> (String, String) {
    let place = &r.f.place;
    let t = tree::find(place, "tsk_sails").expect("the item's tree is open");
    std::fs::write(t.dir.join("same.txt"), "mine and theirs\n").unwrap();
    let head = tree::commit(place, &t, "self: resolve what main moved", "main moved")
        .unwrap()
        .expect("the resolution is a commit");
    let mut s = SessionRecord::new(SessionKind::Conversation, None);
    s.last_target = Some(TargetRef {
        profile: "builder".into(),
        provider: "anthropic".into(),
        model: SONNET.into(),
    });
    r.core.store.put_session(&s.session_id, &s).unwrap();
    r.core.rec(Some(&s.session_id)).record(&SelfBranchBuilt {
        item: "tsk_sails",
        issue: "theseus-sail1",
        branch: &t.branch,
        head: Some(&head),
        gate: "green",
        report: None,
        cost_usd: 1.0,
        stopped_at: None,
        why: "Trim the sails before the wind.",
        numbers: serde_json::Value::Null,
    });
    // A green branch closes its tree.
    tree::close(place, &t).unwrap();
    (t.branch, head)
}

/// KEY: main moves under a finished change. Its review is not spent; the item is `proposed` with
/// no try against it; its tree is open on the branch's own commits, with main merged in and the
/// clash left in the file; and the owner is told once.
#[tokio::test]
async fn main_moving_under_a_finished_change_sends_it_back_with_main_in_its_tree() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 0);
    let (branch, head) = r.seed("tsk_sails", &conflicting(), SONNET);
    main_moves(&r);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch.clone(), "merge".to_string())]);
    assert!(pass.reviewed.is_empty(), "no review is spent: {pass:?}");
    assert!(r.rows("self.review.verdict").is_empty());
    assert_eq!(r.upstream_main(), main, "nothing joined");
    let t = task(&r);
    assert_eq!(t.state, TaskState::Proposed, "ready again, held for no one");
    assert!(
        attempts::tries(&t, 0).is_empty(),
        "no try: {:?}",
        t.evidence
    );
    let ids: Vec<&str> = t.evidence.iter().map(|e| e.identity.as_str()).collect();
    assert!(ids.iter().any(|i| i.starts_with("conflict:")), "{ids:?}");
    // Its tree, on a branch of its own that holds the finished change.
    let tr = tree::find(&r.f.place, "tsk_sails").expect("the builder's tree is open");
    assert_eq!(tr.branch, format!("{branch}-2"));
    assert!(ids.contains(&format!("main-merged:{}", tr.branch).as_str()));
    assert_eq!(git(&tr.dir, &["rev-parse", "HEAD"]), head);
    assert!(git(&tr.dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).len() == 40);
    let clash = std::fs::read_to_string(tr.dir.join("same.txt")).unwrap();
    assert!(
        clash.contains("<<<<<<<") && clash.contains(">>>>>>>"),
        "{clash}"
    );
    assert!(tr.target.is_dir(), "its build folder is there");
    let stopped = &r.rows("self.join.stopped")[0];
    assert_eq!(stopped["numbers"]["conflict"], true, "{stopped}");
    assert_eq!(stopped["numbers"]["main_merged"], true, "{stopped}");
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        posts[0].contains("with its builder") && posts[0].contains("same.txt"),
        "{}",
        posts[0]
    );
    let again = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert!(
        again.stopped.is_empty() && again.reviewed.is_empty(),
        "{again:?}"
    );
    assert_eq!(r.posts().len(), 1, "that head is not tried again");
}

/// The builder's brief says what happened, once, for the start that follows, and no longer after
/// the item has started again.
#[tokio::test]
async fn the_builder_is_told_main_moved_for_the_start_that_follows() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 0);
    r.seed("tsk_sails", &conflicting(), SONNET);
    main_moves(&r);
    assert!(crate::rsi::backlog::brief::main_moved(&task(&r)).is_none());
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    let mut t = task(&r);
    let told = crate::rsi::backlog::brief::main_moved(&t).expect("told");
    assert!(
        told.contains("same.txt") && told.contains("not a failed try"),
        "{told}"
    );
    t.evidence.push(theseus_protocol::tasks::TaskEvidence {
        node: None,
        identity: "session:ses_next".into(),
        note: None,
        by: "self-backlog".into(),
        at_ms: 5,
    });
    assert!(crate::rsi::backlog::brief::main_moved(&t).is_none());
}

/// The whole way round: the builder resolves what clashes, the check's green row lands on its
/// new branch, and that branch is reviewed and joined, a merge of main and the change, with the
/// item's tries still at none.
#[tokio::test]
async fn the_resolution_is_reviewed_and_joined_and_none_of_it_was_a_try() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 0);
    r.seed("tsk_sails", &conflicting(), SONNET);
    main_moves(&r);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    let (branch, head) = resolved(&r);
    let parents = git(
        &r.f.place.repo,
        &["rev-list", "--parents", "-n", "1", &head],
    );
    assert_eq!(
        parents.split(' ').count(),
        3,
        "the commit is the merge: {parents}"
    );
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(
        pass.reviewed,
        [(branch.clone(), "accept".to_string())],
        "{pass:?}"
    );
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let merged = git(&r.f.upstream, &["show", "refs/heads/main:same.txt"]);
    assert_eq!(merged, "mine and theirs");
    assert!(attempts::tries(&task(&r), 0).is_empty());
}

/// The bound on repeats stands: an item that has conflicted twice already is failed at its third,
/// with no working copy made, and the stop says why.
#[tokio::test]
async fn the_bound_on_repeats_fails_the_item_and_the_row_says_why() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 2);
    r.seed("tsk_sails", &conflicting(), SONNET);
    main_moves(&r);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(task(&r).state, TaskState::Failed);
    assert!(
        tree::find(&r.f.place, "tsk_sails").is_none(),
        "no tree for a dropped item"
    );
    let stopped = &r.rows("self.join.stopped")[0];
    assert_eq!(stopped["numbers"]["dropped"], true, "{stopped}");
    assert_eq!(stopped["numbers"]["main_merged"], false, "{stopped}");
    assert!(
        stopped["why"]
            .as_str()
            .unwrap()
            .contains("conflicted with main 3 times"),
        "{stopped}"
    );
}

/// A branch that still merges is reviewed as before, and no tree is made for it.
#[tokio::test]
async fn a_branch_that_still_merges_is_reviewed_as_before() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 0);
    let (branch, _) = r.seed("tsk_sails", PROVED, SONNET);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed, [(branch, "accept".to_string())], "{pass:?}");
    assert!(tree::find(&r.f.place, "tsk_sails").is_none());
}
