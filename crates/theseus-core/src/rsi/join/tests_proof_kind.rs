//! How a change was proved reaches the review that follows: a failing test as a sentence, a
//! build that only broke as a paragraph that asks the reviewer to check it by an experiment in its
//! scratch, and how the reviewer says it did that (`proof_check`). Also the proof step's stop that
//! repeats, held for the owner. Through the whole join over the throwaway repository, with
//! stand-ins for the test runs and the reviewer.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use theseus_protocol::rsi::SelfMode;

use super::planted::{Outcome, Ran, Tests};
use super::review::{self, Ask, Fut, Review, Reviewer};
use super::tests::{rig, Rig, PROVED, SONNET};
use super::Seams;
use crate::rsi::tree::{Place, Tree};
use crate::Core;

/// Says what each run of the tests comes to, in turn.
struct Runs(Mutex<VecDeque<Ran>>);

impl Tests for Runs {
    fn run(&self, _p: &Place, _t: &Tree, _tests: &[String]) -> anyhow::Result<(Ran, String)> {
        let ran = self.0.lock().unwrap().pop_front().unwrap();
        Ok((ran, "tail".into()))
    }
}

/// Answers `accept` with `reasons`, and keeps every ask it was given.
struct Keeps(Mutex<Vec<Ask>>, bool, Vec<String>);

impl Keeps {
    fn saying(accept: bool, reasons: &[&str]) -> Arc<Self> {
        let reasons = reasons.iter().map(|r| r.to_string()).collect();
        Arc::new(Keeps(Mutex::default(), accept, reasons))
    }
}

impl Reviewer for Keeps {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a> {
        self.0.lock().unwrap().push(ask.clone());
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        let (accept, reasons) = (self.1, self.2.clone());
        Box::pin(async move {
            Ok(Review {
                accept,
                reasons,
                cost_usd: 0.0,
                session: None,
                profile,
                model,
            })
        })
    }
}

/// A proved branch through one join pass with the two runs `runs`: the pass, the asks the reviewer
/// had, and the verdict rows.
async fn joined(runs: &[Ran]) -> (super::Pass, Vec<Ask>, Vec<serde_json::Value>) {
    joined_saying(runs, true, &["the stand-in accepts"]).await
}

/// [`joined`], with the reviewer's answer.
async fn joined_saying(
    runs: &[Ran],
    accept: bool,
    reasons: &[&str],
) -> (super::Pass, Vec<Ask>, Vec<serde_json::Value>) {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let keeps = Keeps::saying(accept, reasons);
    let seams = Seams {
        reviewer: keeps.clone(),
        tests: Arc::new(Runs(Mutex::new(runs.iter().copied().collect()))),
        ..r.seams()
    };
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    let asks = keeps.0.lock().unwrap().clone();
    (pass, asks, r.rows("self.review.verdict"))
}

/// A test that failed without the fix: the reviewer is told so in one plain sentence, with the
/// test names, and the record says `failed`.
#[tokio::test]
async fn a_failing_test_reaches_the_reviewer_as_one_sentence() {
    let (pass, asks, rows) = joined(&[Ran::Passed, Ran::Failed]).await;
    assert_eq!(pass.reviewed.len(), 1, "{pass:?}");
    assert_eq!(rows[0]["planted_revert"], "failed");
    assert_eq!(asks.len(), 1);
    let said = review::prompt(&asks[0]);
    assert_eq!(
        asks[0].proof,
        "How it was proved: with the fix undone, at least one of its new or changed tests failed \
         (tests/fix.sh).\n\n"
    );
    assert!(said.contains(&asks[0].proof), "{said}");
    assert!(!said.contains("no longer compile"), "{said}");
    for not in ["CHANGED:", "CHECKED BY READING ONLY:", "scratch checkout"] {
        assert!(!said.contains(not), "{not}: {said}");
    }
    assert!(rows[0].get("proof_check").is_none(), "{:?}", rows[0]);
}

/// A build that only broke: the record says `build-broke`, and the reviewer's ask has the
/// paragraph that says what that shows and what it must therefore find.
#[tokio::test]
async fn a_broken_build_reaches_the_reviewer_as_a_paragraph_asking_for_the_assertion() {
    let (pass, asks, rows) = joined_saying(&[Ran::Passed, Ran::BuildFailed], true, RAN_IT).await;
    assert_eq!(pass.reviewed.len(), 1, "{pass:?}");
    assert_eq!(rows[0]["planted_revert"], "build-broke");
    assert_eq!(rows[0]["verdict"], "accept");
    assert_eq!(asks.len(), 1);
    let said = review::prompt(&asks[0]);
    for want in [
        "the tests no longer compile (tests/fix.sh)",
        "a test mentions the fix and not that it checks what the fix does",
        "find, in the diff, at least one new or changed test whose assertion would fail if the \
         fix's behaviour were wrong",
        "name that test and that assertion in your reasons",
        "If there is none, reject, and say so.",
        // The experiment, in the scratch the ask names and nowhere else.
        "check it by an experiment, not only by reading",
        &format!("{}/tree", asks[0].scratch),
        "keep the fix's new names so that everything still compiles",
        "make the fix's behaviour wrong by the smallest change that does it",
        "Make the change with a command",
        "see at least one of them FAIL on its assertion",
        "The experiment is not the branch",
        "changes only files under",
        "nothing is committed, pushed or written anywhere else",
        "`CHANGED:` what you changed",
        "`FAILED:` which test failed and on which assertion",
        "its tests do not check what the fix does",
        "`CHECKED BY READING ONLY:`",
        "does not count",
    ] {
        assert!(said.contains(want), "{want}: {said}");
    }
}

const RAN_IT: &[&str] = &[
    "CHANGED: made `f` return 1 in the scratch copy",
    "FAILED: the_fix_holds, on its assert_eq of the return value",
];

/// The reply says it ran the experiment: the row says `running`.
#[tokio::test]
async fn a_reply_that_ran_the_experiment_is_recorded_as_running() {
    let (pass, _, rows) = joined_saying(&[Ran::Passed, Ran::BuildFailed], true, RAN_IT).await;
    assert_eq!(
        pass.reviewed,
        [(pass.reviewed[0].0.clone(), "accept".to_string())]
    );
    assert_eq!(rows[0]["planted_revert"], "build-broke");
    assert_eq!(rows[0]["proof_check"], "running", "{:?}", rows[0]);
}

/// The reply says it could only read: the row says `reading`, and the accept stands.
#[tokio::test]
async fn a_reply_that_could_only_read_is_recorded_as_reading() {
    let reasons = [
        "CHECKED BY READING ONLY: no way to break the behaviour without breaking the build",
        "the_fix_holds asserts the return value",
    ];
    let (pass, _, rows) = joined_saying(&[Ran::Passed, Ran::BuildFailed], true, &reasons).await;
    assert_eq!(pass.reviewed[0].1, "accept");
    assert_eq!(rows[0]["verdict"], "accept");
    assert_eq!(rows[0]["proof_check"], "reading", "{:?}", rows[0]);
}

/// An accept of a build-broke proof that says neither is not an accept: a reject that says why,
/// with the reply's own reasons after it, and no `proof_check`.
#[tokio::test]
async fn a_build_broke_accept_that_says_how_it_checked_nothing_is_a_reject() {
    let (pass, _, rows) =
        joined_saying(&[Ran::Passed, Ran::BuildFailed], true, &["it looks fine"]).await;
    assert_eq!(pass.reviewed[0].1, "reject");
    assert_eq!(rows[0]["verdict"], "reject");
    assert_eq!(rows[0]["planted_revert"], "build-broke");
    assert!(rows[0].get("proof_check").is_none(), "{:?}", rows[0]);
    let first = rows[0]["reasons"][0].as_str().unwrap();
    assert!(
        first.starts_with("the review did not say how it checked the proof"),
        "{first}"
    );
    assert_eq!(rows[0]["reasons"][1], "it looks fine");
}

/// A reject that says how it checked records it too; an accept of a proof that was a failing test
/// needs no such words and records none.
#[tokio::test]
async fn only_a_build_broke_proof_needs_and_records_how_it_was_checked() {
    let (_, _, rows) = joined_saying(&[Ran::Passed, Ran::BuildFailed], false, RAN_IT).await;
    assert_eq!(
        (&rows[0]["verdict"], &rows[0]["proof_check"]),
        (&"reject".into(), &"running".into())
    );
    let (_, _, rows) = joined_saying(&[Ran::Passed, Ran::Failed], true, &["it looks fine"]).await;
    assert_eq!(rows[0]["verdict"], "accept");
    assert!(rows[0].get("proof_check").is_none(), "{:?}", rows[0]);
    // And the words are read the way the verdict line is: a case, a space do not matter.
    assert_eq!(
        super::proof_check::how(&["  checked by reading only: no time".into()]),
        Some("reading")
    );
    assert_eq!(
        super::proof_check::how(&["changed: x".into(), "failed: y".into()]),
        Some("running")
    );
    assert_eq!(super::proof_check::how(&["changed: x".into()]), None);
}

/// An end that is neither a failing test nor a failing build proves nothing and rejects nothing:
/// the step stops (and is tried again), no verdict is written and no reviewer is paid.
#[tokio::test]
async fn any_other_end_stops_and_neither_proves_nor_rejects() {
    let (pass, asks, rows) = joined(&[Ran::Passed, Ran::Other(Some(106))]).await;
    assert_eq!(pass.stopped.len(), 1, "{pass:?}");
    assert_eq!(pass.stopped[0].1, "planted");
    assert!(pass.reviewed.is_empty(), "{pass:?}");
    assert!(rows.is_empty(), "{rows:?}");
    assert!(asks.is_empty());
}

/// The paragraph is not part of the claim, so the cut of a long diff never reaches it; and a
/// change proved some other way is told nothing.
#[tokio::test]
async fn the_proof_is_told_after_the_diff_whatever_its_length() {
    let (_, asks, _) = joined_saying(&[Ran::Passed, Ran::BuildFailed], true, RAN_IT).await;
    let mut ask = asks[0].clone();
    ask.diff = "+x\n".repeat(review::DIFF_CHARS);
    let said = review::prompt(&ask);
    let (cut, at, who) = (
        said.find("[the diff is cut here")
            .expect("the diff was cut"),
        said.find("How it was proved").unwrap(),
        said.find("You are the independent reviewer").unwrap(),
    );
    assert!(cut < at && at < who, "{cut} {at} {who}");
    ask.proof = String::new();
    assert!(!review::prompt(&ask).contains("How it was proved"));
    for o in [Outcome::NoTests, Outcome::Passed { tests: vec![] }] {
        assert_eq!(o.proof_note("/s"), None);
    }
}

/// The runs of `passes` passes of a head whose run with the fix undone ends in an exit that says
/// nothing (106, an incomplete run): the tests on the head, then with the fix undone.
fn runs_that_stop(passes: usize) -> Arc<Runs> {
    let runs = (0..passes).flat_map(|_| [Ran::Passed, Ran::Other(Some(106))]);
    Arc::new(Runs(Mutex::new(runs.collect())))
}

/// The heads held by the proof step's stops.
fn held_stops(r: &Rig) -> usize {
    let stops = r.rows("self.join.stopped");
    stops
        .iter()
        .filter(|d| d["numbers"]["held"] == true)
        .count()
}

/// KEY: the same head stopping the same way three times in a row is held for the owner with one
/// card in plain words (what stopped, how many times, the exit), as a review that keeps failing is;
/// and no pass after it runs the tests again.
#[tokio::test]
async fn a_proof_step_that_stops_the_same_way_three_times_holds_the_head() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let runs = runs_that_stop(10);
    let seams = Seams {
        reviewer: Keeps::saying(true, &["x"]),
        tests: runs.clone(),
        ..r.seams()
    };
    for pass in 1..=2 {
        let p = r.core.self_join_pass(&seams).await.unwrap();
        assert_eq!(p.stopped.len(), 1, "pass {pass}: {p:?}");
        assert!(r.posts().is_empty(), "no card before the third");
        assert_eq!(held_stops(&r), 0, "pass {pass}");
    }
    let third = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(third.stopped.len(), 1, "{third:?}");
    let cards = r.posts();
    assert_eq!(cards.len(), 1, "{cards:?}");
    for want in [
        "could not be checked",
        "the same way 3 times",
        "with the fix undone exited with code 106",
        "`theseus self retry",
    ] {
        assert!(cards[0].contains(want), "{want}: {}", cards[0]);
    }
    assert_eq!(held_stops(&r), 1);
    let last = r.rows("self.join.stopped").pop().unwrap();
    assert_eq!(last["numbers"]["class"], "second-run-exit-106");
    assert_eq!(last["numbers"]["repeats"], 3);
    assert!(
        last["why"]
            .as_str()
            .unwrap()
            .contains("could not answer the same way 3 times"),
        "{last}"
    );
    // Nothing runs the tests again, however many passes follow.
    let left = runs.0.lock().unwrap().len();
    for _ in 0..3 {
        let again = r.core.self_join_pass(&seams).await.unwrap();
        assert!(
            again.stopped.is_empty() && again.reviewed.is_empty(),
            "{again:?}"
        );
    }
    assert_eq!((runs.0.lock().unwrap().len(), r.posts().len()), (left, 1));
}

/// A different head starts the count again: the second head is held on its own third stop, with
/// its own card, and not before.
#[tokio::test]
async fn another_head_starts_the_count_again() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    r.seed("mend-the-mast", PROVED, SONNET);
    let seams = Seams {
        reviewer: Keeps::saying(true, &["x"]),
        tests: runs_that_stop(10),
        ..r.seams()
    };
    for _ in 1..=3 {
        r.core.self_join_pass(&seams).await.unwrap();
    }
    assert_eq!((held_stops(&r), r.posts().len()), (1, 1));
    // The fourth pass is the other head's first stop: not held, no card.
    let fourth = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(fourth.stopped.len(), 1, "{fourth:?}");
    assert_eq!((held_stops(&r), r.posts().len()), (1, 1));
    for _ in 5..=6 {
        r.core.self_join_pass(&seams).await.unwrap();
    }
    assert_eq!((held_stops(&r), r.posts().len()), (2, 2));
}

/// A stop that is no exit of the tests (git could not start, say) clears by itself: it is tried
/// again every pass, as it always was, and never held.
struct Breaks;

impl Tests for Breaks {
    fn run(&self, _p: &Place, _t: &Tree, _tests: &[String]) -> anyhow::Result<(Ran, String)> {
        anyhow::bail!("git could not start")
    }
}

#[tokio::test]
async fn a_stop_that_is_no_exit_of_the_tests_is_never_held() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let seams = Seams {
        reviewer: Keeps::saying(true, &["x"]),
        tests: Arc::new(Breaks),
        ..r.seams()
    };
    for pass in 1..=5 {
        let p = r.core.self_join_pass(&seams).await.unwrap();
        assert_eq!(p.stopped.len(), 1, "pass {pass}: {p:?}");
    }
    assert_eq!((held_stops(&r), r.posts().len()), (0, 0));
}
