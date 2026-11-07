//! Two card writers at once (theseus-kxb8d): the interleavings that reverted
//! an answer or lost `posted_ms` before the one card lock.
//!
//! Every card writer is a read-modify-write of `card.proposal.<id>`: it reads
//! the proposals, changes one, and appends it whole. The window is the gap
//! between a writer's read and its append, and the two tests that stage it
//! deterministically hold the caller's read still — which is exactly what the
//! real answer path does, since `card_open` reads the proposal and
//! `card_settle` writes it back after `standing_add` has run in between.
//!
//! A third test runs the two writers on real threads against each other, as
//! `kernel-sim --p-race` races a thread against turns. kernel-sim itself
//! races kernel executions, not card proposals, so it is a regression check
//! here and not the instrument: these are.

use std::sync::Arc;

use theseus_protocol::{CardAsk, ProposalState};

use super::state;
use super::tests_dm::{live, morning, put, read, rig};

/// Writer A reads the proposal, writer B posts it to the DM and stamps
/// `posted_ms`, then writer A appends its answer. Before the lock, A wrote
/// back the clone it had read and the post time was gone: the question looked
/// unposted and the day could give it a second post. Now A rebuilds its answer
/// from the proposal as it stands, so the answer and the post both hold.
#[tokio::test]
async fn an_answer_built_before_a_post_keeps_the_post_time() {
    let r = rig(|_| {});
    put(&r, &live(1..=1));
    let now = morning();

    // Writer A's read: what `card_open` hands `card_settle`.
    let held = r.core.card_open("cdp_00000001").await.unwrap();
    assert_eq!(held.posted_ms, None);

    // Writer B, in the window: the DM post marks it.
    assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 1);
    assert_eq!(read(&r)[0].posted_ms, Some(now));

    // Writer A appends, from the read it took before B ran.
    let settled = r
        .core
        .card_settle(&held, "decline", None, "owner", "cli")
        .await
        .unwrap();

    assert_eq!(settled.state, ProposalState::Declined, "the answer holds");
    let after = read(&r);
    assert_eq!(
        after[0].posted_ms,
        Some(now),
        "the post time survives the answer written over it"
    );
    assert_eq!(after[0].state, ProposalState::Declined);
}

/// Two answers built from the same read. Before the lock the second silently
/// wrote over the first, so a Keep became a Decline and the standing record
/// the Keep had made was left with nothing pointing at it. Now the second is
/// refused by name and nothing is written.
#[tokio::test]
async fn a_second_answer_from_the_same_read_does_not_revert_the_first() {
    let r = rig(|_| {});
    put(&r, &live(1..=1));

    // Both writers read the same open proposal.
    let held = r.core.card_open("cdp_00000001").await.unwrap();
    assert_eq!(held.state, ProposalState::Open);

    // The first answer settles it.
    let first = r
        .core
        .card_settle(&held, "decline", None, "owner", "cli")
        .await
        .unwrap();
    assert_eq!(first.state, ProposalState::Declined);
    let answered_ms = read(&r)[0].answered_ms;
    assert!(answered_ms.is_some());

    // The second, from the read it took before the first ran, is refused.
    let again = r
        .core
        .card_settle(&held, "keep", None, "owner", "cli")
        .await;
    let why = format!(
        "{:#}",
        again.expect_err("the second answer must be refused")
    );
    assert!(
        why.contains("already declined"),
        "the refusal says what it is now: {why}"
    );

    let after = read(&r);
    assert_eq!(after.len(), 1);
    assert_eq!(
        after[0].state,
        ProposalState::Declined,
        "the first answer is not reverted"
    );
    assert_eq!(
        after[0].answered_ms, answered_ms,
        "and its frame is the one that was written"
    );
}

/// Expiry and an answer from the same read: the same class the review named
/// (`card_expire` against `card_settle`). The expiry runs first here, so the
/// answer is refused rather than resurrecting the question.
#[tokio::test]
async fn an_answer_does_not_undo_an_expiry_that_landed_first() {
    let r = rig(|_| {});
    let mut p = live(1..=1);
    p[0].expires_ms = morning() + 1;
    put(&r, &p);

    let held = r.core.card_open("cdp_00000001").await.unwrap();
    assert_eq!(held.state, ProposalState::Open);

    assert_eq!(r.core.card_expire(morning() + 2).await.unwrap(), 1);
    assert_eq!(read(&r)[0].answered_by.as_deref(), Some("expiry"));

    let again = r
        .core
        .card_settle(&held, "keep", None, "owner", "cli")
        .await;
    assert!(again.is_err(), "the answer is refused, not written");
    let after = read(&r);
    assert_eq!(after[0].state, ProposalState::Declined);
    assert_eq!(
        after[0].answered_by.as_deref(),
        Some("expiry"),
        "the expiry's frame stands"
    );
}

/// The two writers on real threads, racing. Whatever the interleaving, the
/// store must never hold a proposal that is answered and unposted when the
/// post went out, and never an open proposal that was answered: one of the
/// two writers wins outright and the other is refused or finds nothing due.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_two_writers_racing_never_lose_one_of_the_two_writes() {
    for round in 0..40u64 {
        let r = rig(|_| {});
        put(&r, &live(1..=1));
        let now = morning();
        let held = r.core.card_open("cdp_00000001").await.unwrap();

        let a = Arc::clone(&r.core);
        let b = Arc::clone(&r.core);
        let post = tokio::spawn(async move { b.card_dm_tick(now).await });
        let answer =
            tokio::spawn(
                async move { a.card_settle(&held, "decline", None, "owner", "cli").await },
            );
        let (posted, settled) = (post.await.unwrap(), answer.await.unwrap());

        let after = read(&r);
        assert_eq!(after.len(), 1, "round {round}");
        let p = &after[0];
        if settled.is_ok() {
            assert_eq!(p.state, ProposalState::Declined, "round {round}");
        }
        if posted.as_ref().is_ok_and(|n| *n == 1) {
            assert_eq!(
                p.posted_ms,
                Some(now),
                "round {round}: the post went out, so its mark must be in the store"
            );
        }
        // Never answered-but-unmarked after a post, and never open after an
        // answer: the two outcomes the race used to produce.
        assert!(
            !(p.state == ProposalState::Open && p.answered_ms.is_some()),
            "round {round}: an answered proposal was reverted to open"
        );
    }
}

/// The lock is not reentrant, and nothing inside a card frame takes it again:
/// a settle whose proposal rests on an observation writes that observation's
/// move in the same frame, through `obs_moves`, which reads the store
/// directly rather than opening a second section.
#[tokio::test]
async fn a_settle_that_moves_an_observation_writes_one_frame() {
    let r = rig(|_| {});
    put(&r, &live(1..=1));
    let held = r.core.card_open("cdp_00000001").await.unwrap();
    assert_eq!(held.ask, CardAsk::Add);
    assert_eq!(held.observations.len(), 1);
    let settled = r
        .core
        .card_settle(&held, "decline", None, "owner", "cli")
        .await
        .unwrap();
    assert_eq!(settled.state, ProposalState::Declined);
    assert!(state::proposals(&r.core.store).is_ok());
}
