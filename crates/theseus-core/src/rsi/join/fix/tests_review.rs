//! The fix at review, played through the whole self-join (issue 36), over `join::tests`' rig with
//! the fix's own stand-in gate: a review that answers `fix` has the repair made, gated, approved by
//! the builder's model and pushed; the next pass runs the planted revert and a fresh review on the
//! fixed head, which joins on an accept, and is rejected on a second `fix`.

use crate::rsi::join::tests_publish::PassYes as _;
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
/// the code commits it, the gate runs again, the builder's model accepts it, and the branch moves
/// to the new head in `[self] repo`, never pushed (issue 53), with its tree closed. The next pass
/// runs the planted revert on that head and a fresh review, told of the fix and offered none; its
/// accept asks the owner, and on his yes the fixed head joins.
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
            &["for-each-ref", "--format=%(refname)", "refs/heads/"]
        ),
        "refs/heads/main",
        "the fixed branch is not pushed"
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
    assert_eq!(two.parked, [(branch.clone(), fixed.clone())], "{two:?}");
    assert!(
        two.joined.is_empty(),
        "nothing joins before the owner's yes"
    );
    assert_eq!(crate::rsi::join::tests_publish::owner_says_yes(&r.core), 1);
    let three = r.core.self_join_pass(&s).await.unwrap();
    let merge = &three.joined[0].1;
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
    r.core.self_join_pass_yes(&s).await.unwrap();
    let two = r.core.self_join_pass_yes(&s).await.unwrap();
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
        .self_join_pass_yes(&seams(&review, &fixer))
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
        .self_join_pass_yes(&seams(&review, &fixer))
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
            proof_check: None,
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

// ---------------------------------------------------------------- round 2 (review B2, the join)

/// The review's `fix` verdict on a head, as a pass writes it before its fix.
fn asked_fix(r: &Rig, branch: &str, head: &str) {
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfReviewVerdict {
            item: "trim".into(),
            branch: branch.into(),
            head: head.into(),
            reviewer: "fable (claude-fable-5-2)".into(),
            model: "claude-fable-5-2".into(),
            asked: "claude-fable-5-2".into(),
            verdict: "fix".into(),
            planted_revert: "failed".into(),
            proof_check: None,
            reasons: vec!["rename it".into()],
            cost_usd: 0.0,
        });
}

/// Review round 1, B2 at review: a restart cut a fix at review short after the fixer's commit
/// was made in the reopened tree. The next pass ends it, closes that tree (so nothing builds on
/// it), returns the item as a reject does, and the view says no fix runs; the item's next attempt
/// starts from before the fix.
#[tokio::test]
async fn a_fix_at_review_cut_short_by_a_restart_is_ended_and_its_tree_closed() {
    let r = rig(|_| {});
    let (branch, head) = r.seed("trim", PROVED, SONNET);
    item(&r, "trim");
    asked_fix(&r, &branch, &head);
    let m = crate::fact::rsi_fix::Moved::new("trim", "given to a fixer")
        .with("branch", branch.clone())
        .with("head", head.clone())
        .with("point", "review")
        .with("fixer", "fable (claude-fable-5-2)");
    r.core.rec(None).record(&crate::fact::rsi_fix::Started(m));
    let t = crate::rsi::tree::reopen(&r.f.place, "trim", &branch, &head).unwrap();
    std::fs::write(t.dir.join("note.txt"), "the fixer's\n").unwrap();
    git(&t.dir, &["add", "-A"]);
    git(&t.dir, &["commit", "-q", "-m", "self: fix at review"]);
    let review = scripted(&[]);
    let fixer = StandIn::writing(&[]);
    let one = r
        .core
        .self_join_pass_yes(&seams(&review, &fixer))
        .await
        .unwrap();
    assert!(one.joined.is_empty(), "{one:?}");
    assert_eq!(fixer.fixed(), 0, "never made twice");
    let none = r.rows("self.fix.none").pop().unwrap();
    assert!(none["why"].as_str().unwrap().contains("restart"), "{none}");
    assert!(!t.dir.exists(), "the reopened tree is closed");
    assert_eq!(task(&r, "trim").state, TaskState::Proposed);
    assert_eq!(
        git(
            &r.f.place.repo,
            &["rev-parse", &format!("refs/heads/{branch}^")]
        ),
        head,
        "the fixer's commit is on the branch, and the tree that held it is gone"
    );
    let view = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert!(
        view.reviewing
            .iter()
            .chain(&view.ready)
            .all(|e| e.fixing.is_none()),
        "{view:?}"
    );
}

/// The one rule at review, whatever path led to the head: a head that holds a fixer's commit no
/// other model approved is rejected before any reviewer is asked.
#[tokio::test]
async fn a_head_with_an_unapproved_fixers_commit_is_not_reviewed() {
    let r = rig(|_| {});
    let (branch, head) = r.seed("trim", PROVED, SONNET);
    item(&r, "trim");
    let session = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .find(|(_, row)| row.kind == "self.branch.built")
        .and_then(|(_, row)| row.session_id)
        .unwrap();
    // The seeded head was judged before: only the head with the fixer's commit is up now.
    let v = crate::fact::rsi_join::SelfReviewVerdict {
        item: "trim".into(),
        branch: branch.clone(),
        head: head.clone(),
        reviewer: "fable (claude-fable-5-2)".into(),
        model: "claude-fable-5-2".into(),
        asked: "claude-fable-5-2".into(),
        verdict: "reject".into(),
        planted_revert: "failed".into(),
        proof_check: None,
        reasons: vec!["an earlier reading".into()],
        cost_usd: 0.0,
    };
    r.core.rec(None).record(&v);
    let t = crate::rsi::tree::reopen(&r.f.place, "trim", &branch, &head).unwrap();
    std::fs::write(t.dir.join("note.txt"), "the fixer's\n").unwrap();
    git(&t.dir, &["add", "-A"]);
    let msg = format!("self: fix\n\n{} review\n", super::TRAILER);
    git(&t.dir, &["commit", "-q", "-m", &msg]);
    let fixed = git(&t.dir, &["rev-parse", "HEAD"]);
    crate::rsi::tree::close(&r.f.place, &t).unwrap();
    r.core
        .rec(Some(&session))
        .record(&crate::fact::rsi_pr::SelfBranchBuilt {
            item: "trim",
            issue: "theseus-sail1",
            branch: &branch,
            head: Some(&fixed),
            gate: "green",
            report: None,
            cost_usd: 0.0,
            stopped_at: None,
            why: "Trim the sails before the wind.",
            numbers: serde_json::Value::Null,
        });
    let review = scripted(&["accept"]);
    let fixer = StandIn::writing(&[]);
    let one = r
        .core
        .self_join_pass_yes(&seams(&review, &fixer))
        .await
        .unwrap();
    assert!(
        one.reviewed
            .contains(&(branch.clone(), "reject".to_string())),
        "{one:?}"
    );
    assert!(one.joined.is_empty(), "{one:?}");
    let v = r.rows("self.review.verdict").pop().unwrap();
    assert!(
        v["reasons"][0]
            .as_str()
            .unwrap()
            .contains("no other model approved"),
        "{v}"
    );
    assert!(
        review.asks.lock().unwrap().iter().all(|a| a.head != fixed),
        "never asked"
    );
}

/// A join main moves under says so as it happens (a row, and the joining queue's words), and the
/// joined row says how many times its gate ran.
#[tokio::test]
async fn a_join_main_moved_under_says_how_often_its_gate_ran() {
    let r = crate::rsi::join::tests::rig(SelfMode::Act);
    let (branch, head) = r.seed("trim", PROVED, SONNET);
    let (root, up) = (
        r.f.place.state.parent().unwrap().to_path_buf(),
        r.f.upstream.clone(),
    );
    let seams = Seams {
        before_push: Some(Arc::new(move |attempt| {
            if attempt == 1 {
                let other = root.join("mover");
                git(
                    &root,
                    &["clone", "-q", up.to_str().unwrap(), other.to_str().unwrap()],
                );
                std::fs::write(other.join("elsewhere.txt"), "elsewhere\n").unwrap();
                git(&other, &["add", "-A"]);
                git(&other, &["commit", "-q", "-m", "elsewhere"]);
                git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
            }
        })),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let again = r.rows("self.join.again");
    assert_eq!(again.len(), 1, "{again:?}");
    assert_eq!(
        (again[0]["branch"].as_str(), again[0]["runs"].as_u64()),
        (Some(branch.as_str()), Some(1))
    );
    let joined = &r.rows("self.joined")[0];
    assert!(
        joined["what"]
            .as_str()
            .unwrap()
            .ends_with("its gate ran 2 times: main moved under it"),
        "{joined}"
    );
    // While it waits, the joining queue says it.
    let r = crate::rsi::join::tests::rig(SelfMode::Act);
    let (branch, head2) = r.seed("trim", PROVED, SONNET);
    let _ = head;
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfReviewVerdict {
            item: "trim".into(),
            branch: branch.clone(),
            head: head2.clone(),
            reviewer: "fable (claude-fable-5-2)".into(),
            model: "claude-fable-5-2".into(),
            asked: "claude-fable-5-2".into(),
            verdict: "accept".into(),
            planted_revert: "failed".into(),
            proof_check: None,
            reasons: vec![],
            cost_usd: 0.0,
        });
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfJoinAgain {
            item: "trim".into(),
            branch,
            head: head2,
            runs: 2,
        });
    let view = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(
        view.joining[0].why.as_deref(),
        Some("its gate has run 2 times: main moved under it before its push")
    );
}

// ---------------------------------------------------------------- round 3 (item 2)

/// A reviewer whose turn never ends, stopped at a short limit by the real `within`.
struct PastLimit;

impl crate::rsi::join::review::Reviewer for PastLimit {
    fn review<'a>(
        &'a self,
        core: &'a Arc<crate::rpc::Core>,
        _ask: &'a crate::rsi::join::review::Ask,
    ) -> crate::rsi::join::review::Fut<'a> {
        Box::pin(async move {
            let never =
                std::future::pending::<anyhow::Result<theseus_protocol::TurnSubmitResult>>();
            let limit = Duration::from_millis(30);
            super::turns::within(core, "ses_review", None, limit, never).await?;
            unreachable!("a pending turn never ends")
        })
    }
}

/// Item 2: a review stopped at its limit is an outcome of its own, said in plain words, and it is
/// not asked again for ever: the third such stop in a row holds the change for the owner with its
/// card, the hold the review already has for a failure that asking again does not clear.
#[tokio::test]
async fn a_review_stopped_at_its_limit_three_times_is_held_for_the_owner() {
    let r = rig(|_| {});
    let (branch, _) = r.seed("trim", PROVED, SONNET);
    item(&r, "trim");
    let s = Seams {
        reviewer: Arc::new(PastLimit),
        ..seams(&scripted(&[]), &StandIn::writing(&[]))
    };
    for _ in 0..4 {
        r.core.self_join_pass_yes(&s).await.unwrap();
    }
    let stops: Vec<_> = r
        .rows("self.join.stopped")
        .into_iter()
        .filter(|d| d["phase"] == "review")
        .collect();
    assert_eq!(
        stops.len(),
        3,
        "asked three times, never a fourth: {stops:?}"
    );
    assert!(
        stops[0]["why"]
            .as_str()
            .unwrap()
            .contains("ran past its limit"),
        "{}",
        stops[0]
    );
    assert_eq!(stops[0]["numbers"]["class"], super::turns::LIMIT_CLASS);
    assert_eq!(stops[2]["numbers"]["held"], true, "{}", stops[2]);
    let posts = r.posts();
    assert!(
        posts
            .iter()
            .any(|p| p.contains("failed the same way 3 times")),
        "{posts:?}"
    );
    let view = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert!(
        view.held
            .iter()
            .any(|e| e.branch.as_deref() == Some(branch.as_str())),
        "{view:?}"
    );
}
