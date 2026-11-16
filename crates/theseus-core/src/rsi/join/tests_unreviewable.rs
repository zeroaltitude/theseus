//! A join that cannot be reviewed does not repeat without end (issue 20): three identical stops
//! for a reason that will not clear hold the change for the owner with one card, and the owner's
//! retry has it asked again; a reason that clears is tried again, after a wait that grows.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use theseus_protocol::rsi::SelfMode;

use super::review::{Ask, Fut, Reviewer};
use super::tests::{rig, Rig, PROVED, SONNET};
use super::tests_retry::{held, item, owner, retry, task};
use super::Seams;
use crate::rpc::Core;
use crate::rsi::backlog::attempts;
use crate::turn::TurnError;

/// A reviewer whose turn fails the way the newest model's did: its class and whether it is
/// transient are the turn's own, and its cause is the provider's words.
struct Failing {
    class: &'static str,
    transient: bool,
    asked: AtomicUsize,
}

impl Failing {
    fn with(class: &'static str, transient: bool) -> Arc<Failing> {
        Arc::new(Failing {
            class,
            transient,
            asked: AtomicUsize::new(0),
        })
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

impl Reviewer for Failing {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, _ask: &'a Ask) -> Fut<'a> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let e = TurnError {
            class: self.class.into(),
            transient: self.transient,
            usage_unknown: false,
            turn_id: "turn_review".into(),
            session_id: "ses_review".into(),
            elapsed_ms: 1,
            trace: None,
            usage: Default::default(),
            cost_usd: None,
            tool_calls: 1,
            source: anyhow::anyhow!("the provider said: messages.1.content.0: Invalid signature"),
        };
        Box::pin(async move { Err(anyhow::Error::new(e)) })
    }
}

/// A rig with one finished change and a reviewer that fails as `reviewer` does.
fn rig_with_failing(class: &'static str, transient: bool) -> (Rig, String, Arc<Failing>, Seams) {
    let r = rig(SelfMode::Act);
    let id = item(&r);
    r.seed(&id, PROVED, SONNET);
    let reviewer = Failing::with(class, transient);
    let seams = Seams {
        reviewer: reviewer.clone(),
        ..r.seams()
    };
    (r, id, reviewer, seams)
}

/// KEY: the review fails the same way, for a reason that will not clear, three passes in a row:
/// the third holds the change for the owner with one card that says what failed and how to check
/// it again; no pass after it asks again; and it is no try of the item.
#[tokio::test]
async fn three_identical_stops_for_a_reason_that_will_not_clear_hold_the_change() {
    let (r, id, reviewer, seams) = rig_with_failing("invalid_request", false);
    let tries = attempts::tries(&task(&r, &id), 0);
    for pass in 1..=2 {
        let p = r.core.self_join_pass(&seams).await.unwrap();
        assert_eq!(p.stopped.len(), 1, "pass {pass}: {p:?}");
        assert_eq!(reviewer.asked(), pass);
        assert!(r.posts().is_empty(), "no card before the third");
        assert!(held(&r).is_empty());
    }
    let third = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(third.stopped.len(), 1, "{third:?}");
    assert_eq!(reviewer.asked(), 3);
    let cards = r.posts();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(cards[0].contains("could not be reviewed"), "{}", cards[0]);
    assert!(cards[0].contains("the same way 3 times"), "{}", cards[0]);
    assert!(
        cards[0].contains(&format!("`theseus self retry {id}`")),
        "{}",
        cards[0]
    );
    let line = held(&r);
    assert_eq!(line.len(), 1, "{line:?}");
    assert!(
        line[0].why.as_deref().unwrap().starts_with(&format!(
            "To have Theseus check it again from the first check, run `theseus self retry {id}`"
        )),
        "{:?}",
        line[0].why
    );
    // Nothing asks again, however many passes follow.
    for _ in 0..3 {
        let again = r.core.self_join_pass(&seams).await.unwrap();
        assert!(
            again.stopped.is_empty() && again.reviewed.is_empty(),
            "{again:?}"
        );
    }
    assert_eq!((reviewer.asked(), r.posts().len()), (3, 1));
    // It was never a try of the item.
    assert_eq!(attempts::tries(&task(&r, &id), 0), tries);
}

/// The owner's retry has it asked again, and the count starts over: it takes three more
/// identical stops to hold it again.
#[tokio::test]
async fn the_owners_retry_starts_the_count_over() {
    let (r, id, reviewer, seams) = rig_with_failing("invalid_request", false);
    for _ in 0..3 {
        r.core.self_join_pass(&seams).await.unwrap();
    }
    assert_eq!(held(&r).len(), 1);
    retry(&r, &id, &owner(), None).unwrap();
    for pass in 4..=5 {
        r.core.self_join_pass(&seams).await.unwrap();
        assert_eq!(reviewer.asked(), pass);
        assert!(held(&r).is_empty(), "pass {pass}");
    }
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(
        (reviewer.asked(), held(&r).len(), r.posts().len()),
        (6, 1, 2)
    );
}

/// A reason that clears is never held: the review is tried again, after a wait that doubles each
/// time, and it is asked once a pass when the wait is over.
#[tokio::test]
async fn a_transient_reason_is_tried_again_after_a_wait_that_grows() {
    let _clock = crate::test_support::pin_mid_period();
    let (r, _id, reviewer, seams) = rig_with_failing("overloaded", true);
    let minutes = |m: u64| {
        theseus_protocol::time::move_clock_to(theseus_protocol::now_unix_ms() + m * 60_000)
    };
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(reviewer.asked(), 1);
    // The next pass comes at once, before any wait: not asked.
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(reviewer.asked(), 1, "a wait follows the first stop");
    // One interval on: asked. The second wait is twice as long.
    minutes(15);
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(reviewer.asked(), 2);
    minutes(15);
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(
        reviewer.asked(),
        2,
        "the second wait is longer than one interval"
    );
    minutes(15);
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(reviewer.asked(), 3);
    // A long way on, it is asked again, and never held, however many stops there were.
    for _ in 0..8 {
        minutes(5 * 60);
        r.core.self_join_pass(&seams).await.unwrap();
    }
    assert_eq!(reviewer.asked(), 11);
    assert!(held(&r).is_empty());
    assert!(r.posts().is_empty(), "no card for a reason that clears");
}

/// A different failure ends the run: two of one class and one of another hold nothing.
#[tokio::test]
async fn only_identical_stops_in_a_row_count() {
    let (r, _id, reviewer, seams) = rig_with_failing("invalid_request", false);
    r.core.self_join_pass(&seams).await.unwrap();
    r.core.self_join_pass(&seams).await.unwrap();
    let other = Failing::with("auth", false);
    let seams = Seams {
        reviewer: other.clone(),
        ..seams
    };
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!((reviewer.asked(), other.asked()), (2, 1));
    assert!(held(&r).is_empty());
    assert!(r.posts().is_empty());
}
