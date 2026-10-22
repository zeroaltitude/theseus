//! A place's home that grows past a size warns the owner once (theseus-jg1eg, the follow-up of
//! theseus-vydhq: a one-line DM cost 282k request tokens before its home was split by hand).
//! The measure is the home's request as the compile counts it, the bound is `[threads]
//! working_tokens` (the tokens a thread's request stays within), and the owner's DM hears it once
//! with the size and `theseus thread split home@<place>`. Nothing is split. The marker is a META
//! row by the home's session, so a restart keeps it, a successor home starts unwarned, and a home
//! that fell back under the bound is armed again.

use crate::session::SessionRecord;
use crate::tests_day_ceiling::{billed, bind_dm, build, posts, rig, turn, turn_in};
use crate::turn::home_size_step::{KEY_PREFIX, KIND};
use crate::Core;
use theseus_protocol::SessionKind;

const PLACE: &str = "dm:5150";

/// A bound low enough that a long message passes it, and a message that long.
fn low(c: &mut crate::Config) {
    c.threads.working_tokens = 2_000;
}

fn long() -> String {
    "the design chat goes on and on. ".repeat(1_000)
}

fn marker(core: &Core, sid: &str) -> Option<serde_json::Value> {
    core.store
        .get_meta::<serde_json::Value>(&format!("{KEY_PREFIX}{sid}"))
        .unwrap()
}

/// A home past the bound: the owner's DM gets one post naming the size, the bound, the split
/// command and its undo, and the home is split by nothing.
#[tokio::test]
async fn a_home_past_the_bound_warns_the_owner_once_with_the_size_and_the_command() {
    let r = rig(vec![billed("one"), billed("two")], low);
    bind_dm(&r.core);
    let (sid, res) = turn(&r.core, &long()).await;
    res.unwrap();
    let p = posts(&r.core, KIND);
    assert_eq!(p.len(), 1, "{p:?}");
    let m = marker(&r.core, &sid).expect("the marker");
    assert_eq!(m["warned"], true);
    let tokens = m["tokens"].as_u64().unwrap();
    assert!(tokens > 2_000, "{m}");
    let text = p[0]["text"].as_str().unwrap();
    for want in [
        crate::narrative::thousands(tokens),
        "2,000".to_string(),
        format!("theseus thread split home@discord:{PLACE}"),
        "theseus thread unsplit".to_string(),
        "--dry-run".to_string(),
    ] {
        assert!(text.contains(&want), "{want}: {text}");
    }
    // A second turn in the same home says nothing more.
    turn_in(&r.core, &sid, &long()).await.unwrap();
    assert_eq!(r.fake.requests().len(), 2, "both turns were made");
    assert_eq!(posts(&r.core, KIND).len(), 1);
}

/// A home under the bound is not warned, and leaves no marker.
#[tokio::test]
async fn a_home_under_the_bound_is_not_warned() {
    let r = rig(vec![billed("one")], |_| {});
    bind_dm(&r.core);
    let (sid, res) = turn(&r.core, "hello").await;
    res.unwrap();
    assert!(posts(&r.core, KIND).is_empty());
    assert!(marker(&r.core, &sid).is_none());
}

/// A restart keeps it: the marker is a row, so the home that was told is not told again.
#[tokio::test]
async fn a_restart_does_not_warn_a_home_that_was_told() {
    let dir = tempfile::tempdir().unwrap();
    let sid = {
        let (core, _fake) = build(dir.path(), vec![billed("one")], low);
        bind_dm(&core);
        let (sid, res) = turn(&core, &long()).await;
        res.unwrap();
        assert_eq!(posts(&core, KIND).len(), 1);
        sid
    };
    let (core, fake) = build(dir.path(), vec![billed("two")], low);
    bind_dm(&core);
    turn_in(&core, &sid, &long()).await.unwrap();
    assert_eq!(fake.requests().len(), 1, "the turn was made");
    assert_eq!(
        posts(&core, KIND).len(),
        1,
        "no second post after the restart"
    );
}

/// A new home starts unwarned: the place moves to a successor session (`bind_place`), and it is
/// told when it grows past the bound, however the old one was.
#[tokio::test]
async fn a_successor_home_starts_unwarned() {
    let r = rig(vec![billed("one"), billed("two")], low);
    bind_dm(&r.core);
    let (first, res) = turn(&r.core, &long()).await;
    res.unwrap();
    let (second, res) = turn(&r.core, &long()).await;
    res.unwrap();
    assert_ne!(first, second);
    assert_eq!(posts(&r.core, KIND).len(), 2, "one for each home");
    assert_eq!(marker(&r.core, &second).unwrap()["warned"], true);
}

/// A home that fell back under the bound (a split took its history out) is armed again, and is
/// told once more when it grows back.
#[tokio::test]
async fn a_home_that_fell_under_the_bound_is_armed_again() {
    let dir = tempfile::tempdir().unwrap();
    let sid = {
        let (core, _fake) = build(dir.path(), vec![billed("one")], low);
        bind_dm(&core);
        let (sid, res) = turn(&core, &long()).await;
        res.unwrap();
        sid
    };
    // Under a higher bound the same home's turn is small enough: the marker is cleared.
    {
        let (core, _fake) = build(dir.path(), vec![billed("two")], |c| {
            c.threads.working_tokens = 500_000;
        });
        bind_dm(&core);
        turn_in(&core, &sid, "short").await.unwrap();
        assert_eq!(marker(&core, &sid).unwrap()["warned"], false);
        assert_eq!(posts(&core, KIND).len(), 1);
    }
    let (core, _fake) = build(dir.path(), vec![billed("three")], low);
    bind_dm(&core);
    turn_in(&core, &sid, &long()).await.unwrap();
    assert_eq!(
        posts(&core, KIND).len(),
        2,
        "it grew back and is told again"
    );
    assert_eq!(marker(&core, &sid).unwrap()["warned"], true);
}

/// With no DM bound nobody can be told: nothing is posted and nothing is marked, so the home is
/// told when one is bound.
#[tokio::test]
async fn with_no_owner_dm_nothing_is_marked() {
    let r = rig(vec![billed("one"), billed("two")], low);
    let (sid, res) = turn(&r.core, &long()).await;
    res.unwrap();
    assert!(posts(&r.core, KIND).is_empty());
    assert!(marker(&r.core, &sid).is_none());
    bind_dm(&r.core);
    turn_in(&r.core, &sid, &long()).await.unwrap();
    assert_eq!(posts(&r.core, KIND).len(), 1);
}

/// Only a place's own session is a home: a session no place runs on (the CLI's) is not warned,
/// nor is a thread's session that answers in the place (`Outbox::show_at`), whose target is the
/// place's though the place does not run on it.
#[tokio::test]
async fn a_session_that_is_no_places_home_is_not_warned() {
    let r = rig(vec![billed("one"), billed("two")], low);
    bind_dm(&r.core);
    let (home, res) = turn(&r.core, "hello").await;
    res.unwrap();
    // The bound is low, so the home itself is told; the sessions below add nothing to that.
    assert_eq!(posts(&r.core, KIND).len(), 1);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    turn_in(&r.core, &rec.session_id, &long()).await.unwrap();
    assert_eq!(posts(&r.core, KIND).len(), 1, "no place runs on it");
    assert!(marker(&r.core, &rec.session_id).is_none());
    // A thread's session answers where its last message came from: the DM, which is not its home.
    let thread = SessionRecord::new(SessionKind::Conversation, None);
    r.core
        .store
        .put_session(&thread.session_id, &thread)
        .unwrap();
    r.core
        .outbox
        .show_at(&thread.session_id, Some(&format!("discord:{PLACE}")))
        .unwrap();
    assert!(r.core.outbox.target(&thread.session_id).is_some());
    turn_in(&r.core, &thread.session_id, &long()).await.unwrap();
    assert_eq!(posts(&r.core, KIND).len(), 1, "the thread is no home");
    assert!(marker(&r.core, &thread.session_id).is_none());
    assert_ne!(home, thread.session_id);
}

/// A turn routing moves compiles twice at its first loop (the first compile is dropped and the
/// moved one made): the home is weighed once, so the owner's DM hears it once, not twice (the
/// join fix of integ/harness: the marker waits for the frame, so the second weigh still read
/// "not told").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turn_past_the_bound_warns_the_owner_once() {
    use crate::tests_route::{mode, rig as routed, turn as routed_turn};
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = routed(Some(&jev), 2, |c| {
        c.threads.enabled = true;
        low(c);
    });
    bind_dm(&r.core);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    let res = routed_turn(&r.core, Some(&rec.session_id), &long(), None).await;
    assert_eq!(res.profile, "opus", "routing moved the turn");
    assert_eq!(posts(&r.core, KIND).len(), 1, "{:?}", posts(&r.core, KIND));
    assert_eq!(marker(&r.core, &rec.session_id).unwrap()["warned"], true);
}
