//! Keep-warm for threads (theseus-ytt8, T9) through the core, with the fake
//! provider and tokio's paused clock: an active thread's session is read at
//! the interval within `[cache] threads_keep_warm_hours` of its owner's last
//! message and not after, while an unthreaded session goes on to the
//! profile's window; pausing removes it at once and a resume waits for the
//! next message; a restart leaves a paused thread out; the day's line reads
//! the most recently written first and says what it skipped; and the day's
//! numbers come from planted `provider.call` and `keep_warm` rows.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::threads::ThreadNewParams;
use theseus_protocol::SessionKind;

use crate::approval::{Answerer, Surface};
use crate::keep_warm::Outcome;
use crate::tests_keep_warm::{build, one_hour, reads, rows, session, settle, turn, MIN};
use crate::{Config, Core};

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn config(thread_hours: f64) -> Config {
    let mut c = one_hour(24);
    c.cache.threads_keep_warm_hours = thread_hours;
    c
}

fn thread(core: &Core, slug: &str) -> String {
    core.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    core.thread_session(slug, cli()).unwrap().session_id
}

fn read_sessions(core: &Core) -> Vec<String> {
    rows(core, "keep_warm")
        .into_iter()
        .filter_map(|r| r.session_id)
        .collect()
}

/// A thread is read every interval to `[cache] threads_keep_warm_hours` after its
/// owner's message, and none after; a plain conversation on the same profile
/// goes on to the profile's 24 hours.
#[tokio::test]
async fn a_thread_is_kept_warm_for_its_window_and_a_plain_session_for_the_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), config(2.0));
    let sid = thread(&core, "tide-gauge");
    let plain = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "the gauge at the pier").await;
    turn(&core, &plain, "an unrelated chat").await;
    tokio::time::pause();
    core.tend_keep_warm_after_serving();
    // The tender rebuilds the windows 10 s after serving, then reads.
    tokio::time::advance(Duration::from_secs(11)).await;
    settle().await;
    // A minute past each interval, so a read's own few ms never leave one due.
    for _ in 0..6 {
        tokio::time::advance(56 * MIN).await;
        settle().await;
    }
    let got = read_sessions(&core);
    let of = |s: &str| got.iter().filter(|x| *x == s).count();
    // 56 and 112 minutes are inside two hours; 168 and the rest are not.
    assert_eq!(of(&sid), 2, "the thread: two reads in its two hours");
    assert_eq!(of(&plain), 6, "the plain session: every interval");
    assert_eq!(reads(&fake).len(), 8);
    assert!(!core.runner.keep_warm.holds(&sid), "the window is closed");
}

/// Pausing removes the session at once, before any read could find it, and a
/// resume does not put it back: its next owner message does.
#[tokio::test]
async fn pausing_stops_the_reads_at_once_and_a_resume_waits_for_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), config(4.0));
    let sid = thread(&core, "pier-survey");
    turn(&core, &sid, "the survey so far").await;
    tokio::time::pause();
    core.tend_keep_warm_after_serving();
    assert!(core.runner.keep_warm.holds(&sid));
    tokio::time::advance(54 * MIN).await;
    settle().await;
    core.thread_pause("pier-survey", cli()).unwrap();
    assert!(
        !core.runner.keep_warm.holds(&sid),
        "removed by the pause itself, not by a later refresh"
    );
    assert_eq!(core.cache_health().kept, 0);
    tokio::time::advance(2 * MIN).await;
    settle().await;
    assert!(reads(&fake).is_empty(), "no read after the pause");
    core.thread_resume("pier-survey", cli()).unwrap();
    assert!(!core.runner.keep_warm.holds(&sid), "a resume holds nothing");
    tokio::time::advance(60 * MIN).await;
    settle().await;
    assert!(reads(&fake).is_empty(), "none until a message");
    turn(&core, &sid, "back to the survey").await;
    assert!(core.runner.keep_warm.holds(&sid));
    tokio::time::advance(56 * MIN).await;
    settle().await;
    assert_eq!(reads(&fake).len(), 1, "read again after its message");
}

/// The state is also checked at the read: a thread paused with its session
/// still held (the forget missed) is not read, and a done one is forgotten.
#[tokio::test]
async fn a_read_finds_a_paused_or_done_thread_and_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), config(4.0));
    let live = core.live_profile().0;
    let (a, b) = (thread(&core, "paused-one"), thread(&core, "done-one"));
    for s in [&a, &b] {
        turn(&core, s, "a question").await;
    }
    core.thread_pause("paused-one", cli()).unwrap();
    core.thread_done("done-one", cli()).unwrap();
    // Put them back as a missed forget would have left them.
    core.runner.keep_warm.message(&a, true);
    core.runner.keep_warm.message(&b, true);
    for s in [&a, &b] {
        let got = core.runner.keep_warm_once(s, &live).await;
        assert!(
            matches!(&got, Outcome::Forgotten(why) if why.contains("paused") || why.contains("done")),
            "{got:?}"
        );
    }
    assert!(reads(&fake).is_empty());
    assert_eq!(
        (
            core.runner.keep_warm.holds(&a),
            core.runner.keep_warm.holds(&b)
        ),
        (false, false)
    );
}

/// A thread's keep-warm off (`keep_warm_hours = 0`) reads none of its
/// sessions, and leaves the plain ones to the profile.
#[tokio::test]
async fn zero_hours_keeps_no_thread_warm() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), config(0.0));
    let live = core.live_profile().0;
    let sid = thread(&core, "no-warm");
    let plain = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "a question").await;
    turn(&core, &plain, "a chat").await;
    assert!(matches!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Forgotten(_)
    ));
    assert!(matches!(
        core.runner.keep_warm_once(&plain, &live).await,
        Outcome::Read { .. }
    ));
    assert_eq!(reads(&fake).len(), 1);
}

/// A restart rebuilds the windows of the active thread and not the paused.
#[tokio::test]
async fn a_restart_rebuilds_an_active_threads_window_and_not_a_paused_ones() {
    let dir = tempfile::tempdir().unwrap();
    let (live_sid, paused_sid) = {
        let (core, _) = build(dir.path(), config(4.0));
        let a = thread(&core, "active-one");
        let b = thread(&core, "paused-two");
        turn(&core, &a, "first").await;
        turn(&core, &b, "second").await;
        core.thread_pause("paused-two", cli()).unwrap();
        core.store.flush().unwrap();
        (a, b)
    };
    let (core, _) = build(dir.path(), config(4.0));
    let live = core.live_profile().0;
    assert_eq!(core.runner.keep_warm_restore(&live).unwrap(), 1);
    assert!(core.runner.keep_warm.holds(&live_sid));
    assert!(!core.runner.keep_warm.holds(&paused_sid));
}

/// At the day's line the tender reads the thread written last first, skips
/// the rest of the round, says how many, and leaves them held and due again
/// an interval later.
#[tokio::test]
async fn at_the_days_line_the_most_recent_is_first_and_the_rest_are_counted_as_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = config(4.0);
    c.kernel.daily_spend_ceiling_mode = crate::config::DailyCeilingMode::Stop;
    let (core, fake) = build(dir.path(), c);
    let (old, mid, new) = (
        thread(&core, "wrote-first"),
        thread(&core, "wrote-second"),
        thread(&core, "wrote-last"),
    );
    for s in [&old, &mid, &new] {
        turn(s_core(&core), s, "a question").await;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    core.kernel.day_ceiling().set_limit(1);
    let interval = Duration::from_millis(core.cfg.cache.interval_ms());
    tokio::time::pause();
    tokio::time::advance(interval + Duration::from_secs(1)).await;
    assert_eq!(
        core.runner.keep_warm.due(interval),
        vec![new.clone(), mid.clone(), old.clone()],
        "the thread written last is first"
    );
    core.tend_keep_warm_after_serving();
    settle().await;
    tokio::time::advance(interval + Duration::from_secs(1)).await;
    settle().await;
    assert!(reads(&fake).is_empty(), "no call past the line");
    assert!(
        !core.runner.keep_warm.holds(&new),
        "the first was refused and stopped"
    );
    assert!(core.runner.keep_warm.holds(&mid) && core.runner.keep_warm.holds(&old));
    let day = crate::keep_warm::today(&core.kernel);
    assert_eq!(core.runner.keep_warm.skipped_today(&day), 2);
    assert_eq!(core.threads_cache().skipped_today, 2);
}

fn s_core(c: &Arc<Core>) -> &Arc<Core> {
    c
}

fn call_row(sid: &str, ago: u64, first: bool, read: u64, wrote: u64) -> theseus_store::NewRecord {
    let at = theseus_protocol::now_unix_ms() - ago;
    let r = crate::ledger::LedgerRow {
        at_unix_ms: at,
        kind: "provider.call".into(),
        session_id: Some(sid.into()),
        turn_id: None,
        data: json!({"loop": u32::from(!first), "model": "claude-sonnet-5-5",
            "usage": {"input_tokens": 10, "output_tokens": 20,
                "cache_read_input_tokens": read, "cache_creation_input_tokens": wrote,
                "cache_creation_1h_input_tokens": wrote}}),
    };
    theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, &r).unwrap()
}

fn read_row(sid: &str, ago: u64) -> theseus_store::NewRecord {
    let r = crate::ledger::LedgerRow {
        at_unix_ms: theseus_protocol::now_unix_ms() - ago,
        kind: "keep_warm".into(),
        session_id: Some(sid.into()),
        turn_id: None,
        data: json!({"model": "claude-sonnet-5-5", "cost_usd": 0.01,
            "usage": {"input_tokens": 5, "output_tokens": 1, "cache_read_input_tokens": 100_000}}),
    };
    theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, &r).unwrap()
}

/// The numbers, from planted rows of a thread's session (minutes ago): a
/// loop at 160 and the turn at 95 after it (65 minutes: a cold resume that
/// rewrote 90k), a keep-warm read at 60, the turn at 30 (warm, and its
/// turn call alone was 65 minutes back: a rewrite avoided), and the turn at
/// 10 (warm, nothing avoided). `thread show` and health say the same.
#[tokio::test]
async fn the_days_numbers_come_from_the_ledgers_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = build(dir.path(), config(4.0));
    let sid = thread(&core, "numbers");
    let m = 60_000u64;
    // Today's rows only count: the cold turn must fall after local midnight.
    let now = theseus_protocol::now_unix_ms();
    if now - crate::learning::local_midnight(now) < 100 * m {
        return;
    }
    core.store
        .append(&[
            call_row(&sid, 160 * m, false, 100_000, 500),
            call_row(&sid, 95 * m, true, 0, 90_000),
            read_row(&sid, 60 * m),
            call_row(&sid, 30 * m, true, 90_000, 400),
            call_row(&sid, 10 * m, true, 90_400, 300),
        ])
        .unwrap();
    let c = core.threads_cache();
    let t = c.threads.iter().find(|e| e.slug == "numbers").unwrap();
    let s = &t.stats;
    assert_eq!((s.cold_resumes, s.rewrite_tokens), (1, 90_000));
    assert_eq!((s.warm_hits, s.avoided), (2, 1));
    assert_eq!(s.keep_warm_reads, 1);
    assert!((s.keep_warm_usd - 0.01).abs() < 1e-6, "{s:?}");
    assert!(s.rewrite_usd > 0.0 && s.saved_usd > 0.0, "{s:?}");
    assert_eq!(c.total, *s);
    let shown = core.thread_show("numbers", cli()).unwrap();
    assert_eq!(shown.cache.as_ref(), Some(s));
    let h = core.health().threads.unwrap();
    assert_eq!(h.keep_warm_hours, 4.0);
    assert_eq!(h.cache.unwrap().total, *s);
}
