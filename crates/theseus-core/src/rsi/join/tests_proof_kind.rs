//! How a change was proved reaches the review that follows: a failing test as a sentence, a
//! build that only broke as a paragraph that asks the reviewer to find the assertion. Through the
//! whole join over the throwaway repository, with stand-ins for the test runs and the reviewer.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use theseus_protocol::rsi::SelfMode;

use super::planted::{Outcome, Ran, Tests};
use super::review::{self, Ask, Fut, Review, Reviewer};
use super::tests::{rig, PROVED, SONNET};
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

/// Accepts, and keeps every ask it was given.
#[derive(Default)]
struct Keeps(Mutex<Vec<Ask>>);

impl Reviewer for Keeps {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a> {
        self.0.lock().unwrap().push(ask.clone());
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        Box::pin(async {
            Ok(Review {
                accept: true,
                reasons: vec!["the stand-in accepts".into()],
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
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let keeps = Arc::new(Keeps::default());
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
}

/// A build that only broke: the record says `build-broke`, and the reviewer's ask has the
/// paragraph that says what that shows and what it must therefore find.
#[tokio::test]
async fn a_broken_build_reaches_the_reviewer_as_a_paragraph_asking_for_the_assertion() {
    let (pass, asks, rows) = joined(&[Ran::Passed, Ran::BuildFailed]).await;
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
    ] {
        assert!(said.contains(want), "{want}: {said}");
    }
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
    let (_, asks, _) = joined(&[Ran::Passed, Ran::BuildFailed]).await;
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
        assert_eq!(o.proof_note(), None);
    }
}
