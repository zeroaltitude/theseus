//! The pace of a post made again, a call that waits out its backoff, and the
//! owner's delete kept across a restart.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::time::Instant;

use super::remake::{remake_wait, MAX_REMAKES};
use super::tests::{shared_over, worker_on, worker_over_the_fake, FORUM};
use super::{Seen, Worker, RETRY};
use crate::runtime::voice_status::tests::view;

const TASK: &str = "exe_01J000000000000000task01";
const WAKE: &str = "wak_01J000000000000000wake01";

fn tell(w: &mut Worker, id: &str, kind: &str, state: &str, title: &str) {
    w.see(Seen {
        views: vec![view(id, kind, state, title, None)],
        private: true,
        guild: None,
        links: HashMap::new(),
        home: None,
        homed: HashSet::new(),
        parent: None,
    });
}

/// The forum is the default one, where scheduled items have their posts.
fn default_forum(shared: &crate::runtime::Shared) {
    shared.task_posts.forums.lock().unwrap()[0].default = true;
}

/// The forum's posts that stand, by id.
fn live(r: &crate::tests_gateway::Rig) -> Vec<u64> {
    r.fake
        .threads()
        .into_iter()
        .filter(|t| t.parent == FORUM)
        .map(|t| t.id)
        .collect()
}

/// How many forum posts have been made, deleted or not.
fn made(r: &crate::tests_gateway::Rig) -> usize {
    r.fake
        .seen()
        .iter()
        .filter(|s| s.method == "POST" && s.outcome == "thread")
        .count()
}

/// How long to hold before the next pass: what `drive` said, and at most as
/// long as the longest backoff a test has to sit out.
async fn sit(held: Option<Instant>) {
    let by = held.map_or(Duration::from_millis(50), |t| {
        t.saturating_duration_since(Instant::now())
    });
    tokio::time::sleep(by.min(Duration::from_millis(1300)) + Duration::from_millis(20)).await;
}

/// The wait before a post found gone is made again: none the first time, then
/// 2 s, 4 s, doubling to the ceiling the other retries have.
#[test]
fn a_post_found_gone_again_and_again_waits_two_seconds_then_double_up_to_the_ceiling() {
    let first = Duration::from_secs(2);
    let waits: Vec<u64> = (1..=8)
        .map(|n| remake_wait(n, first, RETRY).as_secs())
        .collect();
    assert_eq!(waits, [0, 2, 4, 8, 16, 32, 60, 60]);
}

/// A forum that answers 404 to every post as soon as it is made (a rule or
/// another bot that removes each new post): the item's post is made again a
/// handful of times, each later one after a longer wait, and then given up with
/// one line, instead of a post a second for ever.
#[tokio::test]
async fn a_post_that_is_gone_as_soon_as_it_is_made_is_made_again_at_a_pace_and_then_given_up() {
    let (r, mut w) = worker_over_the_fake().await;
    let key = (TASK.to_string(), FORUM);
    r.fake.delete_new_posts(true);
    tell(&mut w, TASK, "task", "running", "Chart the reef");
    w.drive().await;
    assert_eq!(made(&r), 1, "a first post");
    let mut gone = 0;
    let mut at_once = None;
    for round in 0..40 {
        // Its card has something to say, so the call on the dead post fails.
        tell(
            &mut w,
            TASK,
            "task",
            "running",
            &format!("Chart the reef {round}"),
        );
        let had = w.slots.contains_key(&key);
        let held = w.drive().await;
        if had && !w.slots.contains_key(&key) {
            // The post was found gone and forgotten in this pass.
            gone += 1;
            let before = made(&r);
            match w.remake_gate(&key) {
                None => assert_eq!(gone, 1, "only the first is made again at once"),
                Some(Some(wait)) => {
                    assert!(gone >= 2 && wait > Duration::ZERO, "gone {gone}: {wait:?}");
                    // Looked at again inside the wait: nothing is made.
                    w.drive().await;
                    assert_eq!(made(&r), before, "gone {gone}: made again inside its wait");
                }
                Some(None) => assert!(gone > MAX_REMAKES, "given up too soon, at {gone}"),
            }
            at_once.get_or_insert(w.remake_gate(&key).is_none());
        }
        sit(held).await;
    }
    assert_eq!(at_once, Some(true), "the first remake was not at once");
    assert_eq!(
        made(&r),
        MAX_REMAKES as usize + 1,
        "the first post and {MAX_REMAKES} made again, no more"
    );
    assert_eq!(gone, MAX_REMAKES + 1);
    let said = r
        .ledger("discord.error")
        .into_iter()
        .filter(|row| row.to_string().contains("no more are made"))
        .count();
    assert_eq!(said, 1, "said once in the ledger");
    let health = format!("{:?}", w.shared.board.st.lock().unwrap().last_error);
    assert!(
        health.contains("no more are made") || health.contains("task forum"),
        "{health}"
    );
    assert!(live(&r).is_empty() && w.shared.task_posts.posts.lock().unwrap().is_empty());
}

/// The owner's ordinary case is not slowed: a post that stood a while and is
/// then deleted is made again at once, however often that happens.
#[tokio::test]
async fn a_post_the_owner_deleted_after_it_stood_is_made_again_at_once_every_time() {
    let (r, mut w) = worker_over_the_fake().await;
    // What a post must stand for its going to be the owner's: shorter than the
    // wait a failed call has before the post is read (`every.retry`).
    w.every.stood = Duration::from_millis(100);
    let key = (TASK.to_string(), FORUM);
    tell(&mut w, TASK, "task", "running", "Chart the reef");
    w.drive().await;
    for round in 0..3 {
        let post = live(&r)[0];
        r.fake.delete_thread(post);
        tell(
            &mut w,
            TASK,
            "task",
            "running",
            &format!("Chart the reef {round}"),
        );
        let mut held = w.drive().await;
        while w.slots.contains_key(&key) {
            sit(held).await;
            held = w.drive().await;
        }
        assert!(w.remake_gate(&key).is_none(), "round {round}: it waits");
        w.drive().await;
        let now = live(&r);
        assert_eq!(now.len(), 1, "round {round}: one post stands: {now:?}");
        assert_ne!(now[0], post, "round {round}: a new one");
    }
}

/// A call that failed waits out its backoff before the post is read: five
/// passes inside the second make no read of it, and the next after it makes one.
#[tokio::test]
async fn a_failed_call_on_a_post_waits_out_its_backoff_before_the_post_is_read() {
    let (r, mut w) = worker_over_the_fake().await;
    tell(&mut w, TASK, "task", "running", "Chart the reef");
    w.drive().await;
    let post = live(&r)[0];
    r.fake.delete_thread(post);
    let reads = || {
        r.fake
            .seen()
            .iter()
            .filter(|s| s.method == "GET" && s.path == format!("/channels/{post}"))
            .count()
    };
    // Past the card's own pace, so the edit is tried at once.
    tokio::time::sleep(Duration::from_millis(100)).await;
    tell(&mut w, TASK, "task", "running", "Chart the whole reef");
    w.drive().await;
    assert!(w.failed.contains_key(&(TASK.to_string(), FORUM)));
    for _ in 0..5 {
        w.drive().await;
    }
    assert_eq!(reads(), 0, "read before its backoff was out");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    w.drive().await;
    assert_eq!(reads(), 1, "read once its backoff was out");
}

/// A scheduled item that is over, whose post the owner deleted, stays without a
/// post after the daemon is killed and started again.
#[tokio::test]
async fn an_ended_items_post_the_owner_deleted_is_not_made_again_after_a_restart() {
    let (r, mut w) = worker_over_the_fake().await;
    default_forum(&w.shared);
    tell(&mut w, WAKE, "wake", "running", "tide check");
    w.drive().await;
    let post = live(&r)[0];
    r.fake.delete_thread(post);
    // It ends: the card has something to say, the call fails, the post is read.
    tell(&mut w, WAKE, "wake", "done", "tide check");
    let mut held = w.drive().await;
    for _ in 0..8 {
        if w.slots.is_empty() {
            break;
        }
        sit(held).await;
        held = w.drive().await;
    }
    assert!(live(&r).is_empty(), "none made again in this run");
    assert!(w.deleted.contains_key(&(WAKE.to_string(), FORUM)));

    // The daemon is killed: nothing of the worker or its lists is in memory,
    // the store is what the last run left. The item is still told, over.
    drop(w);
    let shared = shared_over(&r);
    default_forum(&shared);
    shared.task_posts.load(&shared.core.store);
    let mut w = worker_on(shared);
    tell(&mut w, WAKE, "wake", "done", "tide check");
    for _ in 0..3 {
        w.drive().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(live(&r).is_empty(), "made again after a restart");
    assert_eq!(made(&r), 1, "only the first post was ever made");

    // The item stands again: a post is made, and the mark is gone for good.
    tell(&mut w, WAKE, "wake", "running", "tide check");
    w.drive().await;
    assert_eq!(live(&r).len(), 1, "a standing item has its post");
    let shared = shared_over(&r);
    let w = worker_on(shared);
    assert!(w.deleted.is_empty(), "the mark leaves when the item stands");
}

/// A post whose starter message the owner deleted while the thread stands: its
/// card can never be edited again, so the item is given a post of its own, at the
/// pace of any post found gone, and the old row is not kept.
#[tokio::test]
async fn a_post_whose_starter_message_was_deleted_is_made_again_for_a_standing_item() {
    let (r, mut w) = worker_over_the_fake().await;
    let key = (TASK.to_string(), FORUM);
    tell(&mut w, TASK, "task", "running", "Chart the reef");
    w.drive().await;
    let first = live(&r)[0];
    r.fake.delete_starter_message(first);
    tokio::time::sleep(Duration::from_millis(100)).await;
    for round in 0..8 {
        tell(
            &mut w,
            TASK,
            "task",
            "running",
            &format!("Chart the reef {round}"),
        );
        let held = w.drive().await;
        if made(&r) > 1 {
            break;
        }
        sit(held).await;
    }
    let now = live(&r);
    assert_eq!(made(&r), 2, "a second post: {now:?}");
    let rows = w.shared.task_posts.posts.lock().unwrap().clone();
    assert_eq!(rows.len(), 1, "one row: {rows:?}");
    assert_ne!(rows[0].post, first, "the row is the new post's");
    assert!(w.slots.contains_key(&key));
}
