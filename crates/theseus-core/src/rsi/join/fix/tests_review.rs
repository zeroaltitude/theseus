//! The fix at review, played through the whole self-join (issue 36), over `join::tests`' rig with
//! the fix's own stand-in gate: a review that answers `fix` has the repair made, gated, approved by
//! the builder's model and pushed; the next pass runs the planted revert and a fresh review on the
//! fixed head, which joins on an accept, and is rejected on a second `fix`.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::rsi::{SelfMode, SelfPipelineParams};
use theseus_protocol::tasks::{TaskOrigin, TaskRecord, TaskState};

use super::tests::{scripted, ScriptedReview, StandIn};
use super::tests_gate::GATE;
use crate::rsi::join::tests::Scripts;
use crate::rsi::join::tests::{git, rig_on, Rig, PROVED, SONNET};
use crate::rsi::join::Seams;
use crate::rsi::tests_tree::fixture_with;

fn rig(edit: impl FnOnce(&mut crate::Config)) -> Rig {
    rig_on(fixture_with(GATE), SelfMode::Act, edit, |_| {})
}

fn seams(review: &Arc<ScriptedReview>, fixer: &Arc<StandIn>) -> Seams {
    Seams {
        reviewer: review.clone(),
        tests: Arc::new(Scripts),
        fixer: fixer.clone(),
        lock_every: Duration::from_millis(50),
        lock_wait: Duration::from_secs(60),
        ..Seams::default()
    }
}

/// The item of the seeded branch, in progress, as the backlog leaves a built one.
fn item(r: &Rig, id: &str) {
    let t = TaskRecord {
        id: id.into(),
        version: 1,
        title: "Trim the sails".into(),
        state: TaskState::InProgress,
        origin: TaskOrigin {
            session: "self:backlog".into(),
            principal: "self".into(),
            ..Default::default()
        },
        ..TaskRecord::default()
    };
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
}

fn task(r: &Rig, id: &str) -> TaskRecord {
    crate::task_graph::get(&r.core.store, id).unwrap().unwrap()
}

/// The review asks for a fix: the fixer (the reviewer's profile) repairs it in the item's tree,
/// the code commits it, the gate runs again, the builder's model accepts it, and the branch is
/// pushed at the new head with its tree closed. The next pass runs the planted revert on that head
/// and a fresh review, told of the fix and offered none; its accept joins the fixed head.
#[tokio::test]
async fn a_reviews_fix_is_made_gated_proved_reviewed_afresh_and_joined() {
    let r = rig(|_| {});
    let (branch, head) = r.seed("trim", PROVED, SONNET);
    item(&r, "trim");
    let review = scripted(&["fix", "accept"]);
    let fixer = StandIn::writing(&[("note.txt", "named plainly\n")]);
    let s = seams(&review, &fixer);
    let one = r.core.self_join_pass(&s).await.unwrap();
    assert_eq!(
        one.reviewed,
        [(branch.clone(), "fix".to_string())],
        "{one:?}"
    );
    assert!(one.joined.is_empty());
    let first = review.asks.lock().unwrap()[0].clone();
    assert_eq!(first.offer_fix, Some((60, 6)));
    assert_eq!(first.fixed, None);
    // The fixer was given the review's reasons, in the item's own tree.
    let ask = fixer.fixes.lock().unwrap()[0].clone();
    assert_eq!(ask.target.profile, "fable");
    assert!(
        ask.failure.contains("name the note's file plainly"),
        "{}",
        ask.failure
    );
    assert!(ask.tree.ends_with("/trim"), "{}", ask.tree);
    let v = r.rows("self.fix.verdict").pop().unwrap();
    assert_eq!(
        (v["point"].as_str(), v["verdict"].as_str()),
        (Some("review"), Some("accept"))
    );
    assert!(
        v["approver"]
            .as_str()
            .unwrap()
            .ends_with("(claude-sonnet-5-5)"),
        "{v}"
    );
    let fixed = v["head"].as_str().unwrap().to_string();
    assert_ne!(fixed, head);
    assert_eq!(
        git(
            &r.f.place.repo,
            &["rev-parse", &format!("refs/heads/{branch}^")]
        ),
        head
    );
    assert_eq!(
        git(
            &r.f.upstream,
            &["rev-parse", &format!("refs/heads/{branch}")]
        ),
        fixed,
        "pushed at the fixed head"
    );
    assert!(
        !r.f.place.trees().join("trim").exists(),
        "the tree is closed"
    );
    // The branch's report is frozen, so the fix has its own beside it.
    let report =
        r.f.place
            .reports()
            .join(format!("{}-fix-review.md", branch.replace('/', "-")));
    let report = std::fs::read_to_string(report).unwrap();
    for want in [
        "Fixed by: fable (claude-fable-5-2)",
        "1 lines in 1 files",
        "claude-sonnet-5-5)",
    ] {
        assert!(report.contains(want), "{want}: {report}");
    }
    // Between the passes the fixed head waits for its review, as any green head does.
    let view = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(view.reviewing.len(), 1, "{view:?}");
    assert_eq!(view.reviewing[0].fixing, None);
    // The next pass: the planted revert on the fixed head, a fresh review told of the fix.
    let two = r.core.self_join_pass(&s).await.unwrap();
    assert_eq!(
        two.reviewed,
        [(branch.clone(), "accept".to_string())],
        "{two:?}"
    );
    let second = review.asks.lock().unwrap()[1].clone();
    assert_eq!(second.head, fixed);
    assert_eq!(second.offer_fix, None);
    let told = second.fixed.unwrap();
    assert!(
        told.contains("claude-sonnet-5-5)") && told.contains("by fable"),
        "{told}"
    );
    let verdicts = r.rows("self.review.verdict");
    assert_eq!(verdicts[1]["head"], fixed.as_str());
    assert_eq!(verdicts[1]["planted_revert"], "failed", "proved again");
    let merge = &two.joined[0].1;
    let parents = git(&r.f.upstream, &["rev-list", "--parents", "-n", "1", merge]);
    assert!(
        parents.ends_with(&fixed),
        "the fixed head joined: {parents}"
    );
}

/// After a fix at review, a second `fix` is a reject: the item returns, and no second fix is made.
#[tokio::test]
async fn a_second_fix_at_review_is_a_reject() {
    let r = rig(|_| {});
    let (branch, _) = r.seed("trim", PROVED, SONNET);
    item(&r, "trim");
    let review = scripted(&["fix", "fix"]);
    let fixer = StandIn::writing(&[("note.txt", "named plainly\n")]);
    let s = seams(&review, &fixer);
    r.core.self_join_pass(&s).await.unwrap();
    let two = r.core.self_join_pass(&s).await.unwrap();
    assert_eq!(two.reviewed, [(branch, "reject".to_string())], "{two:?}");
    let v = r.rows("self.review.verdict").pop().unwrap();
    assert!(
        v["reasons"][0].as_str().unwrap().contains("second fix"),
        "{v}"
    );
    assert_eq!(fixer.fixed(), 1, "no second fix");
    assert_eq!(
        task(&r, "trim").state,
        TaskState::Proposed,
        "back as after a reject"
    );
}

/// The fix at review is gated again: a fix the gate fails is not approved, and the item returns
/// as after a reject, saying a fix was tried.
#[tokio::test]
async fn a_fix_at_review_the_gate_fails_returns_the_item() {
    let r = rig(|_| {});
    let (branch, _) = r.seed("trim", PROVED, SONNET);
    item(&r, "trim");
    let review = scripted(&["fix"]);
    let fixer = StandIn::writing(&[("broken.txt", "x\n")]);
    let one = r
        .core
        .self_join_pass(&seams(&review, &fixer))
        .await
        .unwrap();
    assert_eq!(
        one.reviewed,
        [
            (branch.clone(), "fix".to_string()),
            (branch, "fix-failed".to_string())
        ],
        "{one:?}"
    );
    assert_eq!(fixer.approved(), 0);
    let none = r.rows("self.fix.none").pop().unwrap();
    assert!(
        none["why"].as_str().unwrap().contains("red again"),
        "{none}"
    );
    let t = task(&r, "trim");
    assert_eq!(t.state, TaskState::Proposed);
    let said = t.evidence.last().unwrap();
    assert!(
        said.identity.starts_with("review:"),
        "a reject's word: {said:?}"
    );
    assert!(
        said.note.as_deref().unwrap().contains("red again"),
        "{said:?}"
    );
    assert!(
        !r.f.place.trees().join("trim").exists(),
        "the tree is closed"
    );
}

/// With `[self_fix] mode = "off"` a review's `fix` is a reject, as any other word is, and nothing
/// of a fix runs.
#[tokio::test]
async fn with_fix_off_a_reviews_fix_is_a_reject() {
    let r = rig(|c| c.self_fix.mode = crate::config::self_fix::FixMode::Off);
    let (branch, _) = r.seed("trim", PROVED, SONNET);
    let review = scripted(&["fix"]);
    let fixer = StandIn::writing(&[("note.txt", "x\n")]);
    let one = r
        .core
        .self_join_pass(&seams(&review, &fixer))
        .await
        .unwrap();
    assert_eq!(one.reviewed, [(branch, "reject".to_string())], "{one:?}");
    assert_eq!(review.asks.lock().unwrap()[0].offer_fix, None);
    assert_eq!(fixer.fixed(), 0);
    assert!(r.rows("self.fix.started").is_empty());
}

/// While a fix at review is under way, the pipeline's view names the branch as being fixed.
#[tokio::test]
async fn a_branch_being_fixed_is_named_so_in_the_view() {
    let r = rig(|_| {});
    let (branch, head) = r.seed("trim", PROVED, SONNET);
    item(&r, "trim");
    // The review's word and the fix's start, as a pass writes them before the fixer answers.
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfReviewVerdict {
            item: "trim".into(),
            branch: branch.clone(),
            head: head.clone(),
            reviewer: "fable (claude-fable-5-2)".into(),
            model: "claude-fable-5-2".into(),
            asked: "claude-fable-5-2".into(),
            verdict: "fix".into(),
            planted_revert: "failed".into(),
            reasons: vec!["rename it".into()],
            cost_usd: 0.0,
        });
    let m = crate::fact::rsi_fix::Moved::new("trim", "given to a fixer")
        .with("branch", branch)
        .with("head", head)
        .with("point", "review")
        .with("fixer", "fable (claude-fable-5-2)");
    r.core.rec(None).record(&crate::fact::rsi_fix::Started(m));
    let view = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(view.reviewing.len(), 1, "{view:?}");
    assert_eq!(
        view.reviewing[0].fixing.as_deref(),
        Some("at review, by fable (claude-fable-5-2)")
    );
    // `self log` says it in plain words.
    let log = r
        .core
        .self_log(&theseus_protocol::rsi::SelfLogParams::default())
        .unwrap();
    let row = log
        .rows
        .iter()
        .find(|l| l.kind == "self.fix.started")
        .unwrap();
    assert_eq!(row.what, "given to a fixer");
}
