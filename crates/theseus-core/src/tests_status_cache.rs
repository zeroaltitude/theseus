//! The tender's status, read once a second and shared (theseus-id8d): health
//! calls inside a second make one read of the tender's `index.status`, and
//! callers that come while it is out take its answer; a tender that stops
//! answering is shown by its last status, `stale` once that is old.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::config::IndexConfig;
use crate::tender::{HEALTH_DEADLINE, STALE_AFTER, STATUS_DEADLINE, STATUS_EVERY};
use crate::tests_tender::{settle, stand_in, supervisor_with, FakeOs};

/// Two health calls inside a second, and five more at once, make one status
/// read on the tender's socket; a call after the second asks again.
#[tokio::test]
async fn health_calls_inside_a_second_make_one_status_read() {
    let dir = tempfile::tempdir().unwrap();
    let asked = stand_in(dir.path(), Arc::default());
    let os = Arc::new(FakeOs::default());
    let every = Duration::from_millis(400);
    let (t, _) = supervisor_with(
        IndexConfig::default(),
        dir.path(),
        os,
        Duration::ZERO,
        every,
    );
    let task = tokio::spawn(t.clone().run());
    settle().await;
    let began = std::time::Instant::now();
    let first = t.health(HEALTH_DEADLINE).await;
    let second = t.health(STATUS_DEADLINE).await;
    let many: Vec<_> = (0..5)
        .map(|_| {
            let t = t.clone();
            tokio::spawn(async move { t.health(HEALTH_DEADLINE).await })
        })
        .collect();
    for h in many {
        assert_eq!(h.await.unwrap().state, "ready");
    }
    let inside = began.elapsed() < every;
    assert_eq!(
        (first.state.as_str(), second.state.as_str()),
        ("ready", "ready")
    );
    assert_eq!(second.status.map(|s| s.documents), Some(3));
    assert_eq!(second.why, None, "a fresh read says nothing more");
    if inside {
        assert_eq!(asked.load(Ordering::SeqCst), 1, "one read on the socket");
        assert_eq!(t.status_reads(), 1);
    }
    tokio::time::sleep(every).await;
    assert_eq!(t.health(HEALTH_DEADLINE).await.state, "ready");
    if inside {
        assert_eq!(asked.load(Ordering::SeqCst), 2, "a read a window");
    }
    assert!(asked.load(Ordering::SeqCst) >= 2);
    task.abort();
}

/// The daemon's own window is a second.
#[test]
fn one_read_serves_a_second_and_stale_is_minutes() {
    assert_eq!(STATUS_EVERY, Duration::from_secs(1));
    assert!(STALE_AFTER >= Duration::from_secs(60) && STALE_AFTER <= Duration::from_secs(600));
}

/// A tender that stops answering: health waits its deadline, shows the last
/// status and how old it is, and once that is past `STALE_AFTER` says
/// `stale` plainly; the cached failure answers the next call in the window
/// at once, with the same words.
#[tokio::test]
async fn a_tender_that_does_not_answer_gives_its_last_status_marked_stale() {
    let dir = tempfile::tempdir().unwrap();
    let hang = Arc::new(AtomicBool::new(false));
    let asked = stand_in(dir.path(), hang.clone());
    let os = Arc::new(FakeOs::default());
    let every = Duration::from_millis(300);
    let (t, _) = supervisor_with(
        IndexConfig::default(),
        dir.path(),
        os,
        Duration::ZERO,
        every,
    );
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(t.health(HEALTH_DEADLINE).await.state, "ready");
    hang.store(true, Ordering::SeqCst);
    tokio::time::sleep(every).await;
    let h = t.health(HEALTH_DEADLINE).await;
    assert_eq!(h.state, "ready", "its last answer");
    let why = h.why.unwrap();
    assert!(
        why.starts_with("its socket did not answer (no answer within 100 ms): its status as of "),
        "{why}"
    );

    t.age_status(STALE_AFTER.as_millis() as u64 + 60_000);
    let reads = asked.load(Ordering::SeqCst);
    let t0 = std::time::Instant::now();
    let h = t.health(HEALTH_DEADLINE).await;
    let quick = t0.elapsed() < every;
    assert_eq!(h.status.as_ref().map(|s| s.documents), Some(3));
    let why = h.why.unwrap();
    assert!(
        why.starts_with("stale: its status is 4 min old, and its socket did not answer ("),
        "{why}"
    );
    if quick {
        assert_eq!(
            asked.load(Ordering::SeqCst),
            reads,
            "the cached failure answers"
        );
    }
    tokio::time::sleep(every).await;
    let h = t.health(HEALTH_DEADLINE).await;
    assert!(
        h.why.unwrap().starts_with("stale: "),
        "asked again, still stale"
    );
    task.abort();
}
