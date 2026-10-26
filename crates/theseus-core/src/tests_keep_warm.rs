//! The keep-warm read through the core (theseus-ezeg), with the fake
//! provider and tokio's paused clock: a 1-hour conversation is read at the
//! interval and not past its window; the read's prefix is the next turn's
//! first bytes; none during a turn, for a task, in a shared place, past the
//! day ceiling (refused and said once), from a stopping daemon, or under the
//! prefix floor; a new message starts the window again; a restart rebuilds
//! the windows; health lists each profile's TTL and the reads.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::SessionKind;

use crate::bus::EventSink;
use crate::keep_warm::{Outcome, LINE, MAX_TOKENS};
use crate::provider::{FakeProvider, ProviderRequest};
use crate::session::SessionRecord;
use crate::store::Store;
pub(crate) use crate::test_support::ledger_rows_5k as rows;
use crate::turn::TurnRequest;
use crate::{Config, Core};

pub(crate) const MIN: Duration = Duration::from_secs(60);

/// The template with a 1-hour `[cache] ttl`, so every profile that says none
/// (the live one, sonnet, among them) caches for an hour, kept warm for
/// `hours` after a message.
pub(crate) fn one_hour(hours: u32) -> Config {
    let text = Config::EXAMPLE_TOML
        .replace("\n[cache]\n", "\n[cache]\nttl = \"1h\"\n")
        .replace(
            "keep_warm_hours = 24\n",
            &format!("keep_warm_hours = {hours}\n"),
        );
    let mut c = Config::parse(&text).unwrap().0;
    c.cache.keep_warm_min_tokens = 0;
    c
}

pub(crate) fn build(dir: &std::path::Path, c: Config) -> (Arc<Core>, Arc<FakeProvider>) {
    let fake = Arc::new(FakeProvider::default());
    (build_with(dir, c, fake.clone()), fake)
}

fn build_with(dir: &std::path::Path, c: Config, fake: Arc<FakeProvider>) -> Arc<Core> {
    let mut c = c;
    c.server.state_dir = dir.to_string_lossy().into_owned();
    c.tools.roots = vec![];
    let store = Store::open(&dir.join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(c, fake, store)).unwrap()
}

/// A conversation, posting to `place` when given (unbound: the CLI's).
pub(crate) fn session(core: &Core, kind: SessionKind, place: Option<&str>) -> String {
    let r = SessionRecord::new(kind, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

/// A person's message and its turn, as `turn.submit` makes them.
pub(crate) async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
    core.runner.keep_warm.message(sid, true);
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

pub(crate) fn reads(fake: &FakeProvider) -> Vec<ProviderRequest> {
    fake.requests()
        .into_iter()
        .filter(|r| r.max_tokens == MAX_TOKENS)
        .collect()
}

/// Let the tender run what is due: a few turns of the runtime, and the
/// blocking pool's work, on the paused clock.
pub(crate) async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(1)).await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

/// A 1-hour session is read once `keep_warm_minutes` after its turn, again
/// each interval after that read, and never past `keep_warm_hours` after
/// its message; each read is a `keep_warm` row with its usage and cost,
/// booked to the session and counted on the day.
#[tokio::test]
async fn a_one_hour_session_is_read_at_the_interval_and_none_after_its_window() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), one_hour(12));
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "the long conversation").await;
    tokio::time::pause();
    core.tend_keep_warm_after_serving();
    tokio::time::advance(54 * MIN).await;
    settle().await;
    assert!(reads(&fake).is_empty(), "nothing before the interval");
    tokio::time::advance(MIN + Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(reads(&fake).len(), 1, "one read at 55 minutes");
    let row = &rows(&core, "keep_warm")[0];
    assert_eq!(row.session_id.as_deref(), Some(sid.as_str()));
    assert_eq!(row.data["model"], "claude-sonnet-5-5");
    assert!(
        row.data["cost_usd"].as_f64().unwrap() > 0.0,
        "{:?}",
        row.data
    );
    assert!(row.data["usage"]["input_tokens"].as_u64().unwrap() > 0);
    let spent: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let e = core
        .kernel
        .execution(spent.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    let booked = theseus_kernel::usd_to_micros(row.data["cost_usd"].as_f64().unwrap());
    assert!(e.budget.spent_micros >= booked, "booked to the session");
    // Each interval after the read, to the window's end (12 hours after the
    // message, under Sonnet 5.5's 18.3-hour break-even): 13 reads in all, and
    // none after.
    for _ in 0..30 {
        tokio::time::advance(55 * MIN).await;
        settle().await;
    }
    assert_eq!(reads(&fake).len(), 13, "every 55 minutes for 12 hours");
    assert_eq!(rows(&core, "keep_warm").len(), 13);
    tokio::time::advance(10 * 55 * MIN).await;
    settle().await;
    assert_eq!(reads(&fake).len(), 13, "none after the window");
    let h = core.cache_health();
    assert_eq!((h.kept, h.reads_today > 0), (0, true));
}

/// The read is the next turn's request up to its new message, byte for
/// byte (system, tools, thinking and every message before it), then the
/// keep-warm line, at the smallest output.
#[tokio::test]
async fn the_reads_prefix_is_the_next_turns_first_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), one_hour(24));
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "first, a question").await;
    turn(&core, &sid, "then another").await;
    let live = core.live_profile().0;
    assert!(matches!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Read { .. }
    ));
    turn(&core, &sid, "and the next message").await;
    let all = fake.requests();
    let read = reads(&fake).pop().unwrap();
    let next = all.last().unwrap();
    assert_eq!(read.max_tokens, MAX_TOKENS);
    let n = read.messages.len() - 1;
    assert_eq!(
        read.messages[n],
        serde_json::json!({"role": "user", "content": [{"type": "text", "text": LINE}]})
    );
    let bytes = |v: &[Value]| serde_json::to_string(v).unwrap();
    assert_eq!(bytes(&read.system), bytes(&next.system), "the system");
    assert_eq!(bytes(&read.tools), bytes(&next.tools), "the tools");
    assert_eq!(
        bytes(&read.messages[..n]),
        bytes(&next.messages[..n]),
        "the messages before the new one"
    );
    assert_eq!(
        (&read.thinking, &read.cache_control, &read.model),
        (&next.thinking, &next.cache_control, &next.model)
    );
    assert!(next.messages.len() > n);
}

/// No read while a turn of the session runs: the read waits, and the
/// turn's end notes the session's call again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn none_during_a_turn() {
    let dir = tempfile::tempdir().unwrap();
    let slow = Arc::new(FakeProvider {
        delay_ms: 400,
        ..FakeProvider::default()
    });
    let core = build_with(dir.path(), one_hour(24), slow.clone());
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "first").await;
    let c2 = core.clone();
    let s2 = sid.clone();
    let running = tokio::spawn(async move { turn(&c2, &s2, "a turn the read waits for").await });
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.unwrap();
    for _ in 0..200 {
        let e = core.kernel.execution(&exec).unwrap().unwrap();
        if e.state == theseus_kernel::ExecState::Running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let live = core.live_profile().0;
    assert_eq!(core.runner.keep_warm_once(&sid, &live).await, Outcome::Busy);
    running.await.unwrap();
    assert!(reads(&slow).is_empty(), "nothing sent while it ran");
}

/// Never a task's session, nor a conversation in a shared place, nor one
/// whose prefix is under the floor: each is forgotten, and nothing is sent.
#[tokio::test]
async fn none_for_a_task_a_shared_place_or_a_small_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), one_hour(24));
    let live = core.live_profile().0;
    let shared = session(&core, SessionKind::Conversation, Some("channel:4242"));
    turn(&core, &shared, "in a guild channel nobody bound private").await;
    let task = session(&core, SessionKind::Task, None);
    core.runner.keep_warm.message(&task, true);
    for sid in [&shared, &task] {
        let got = core.runner.keep_warm_once(sid, &live).await;
        assert!(matches!(got, Outcome::Forgotten(_)), "{sid}: {got:?}");
    }
    let mut c = one_hour(24);
    c.cache.keep_warm_min_tokens = 30_000;
    let dir2 = tempfile::tempdir().unwrap();
    let (small, fake2) = build(dir2.path(), c);
    let sid = session(&small, SessionKind::Conversation, None);
    turn(&small, &sid, "a short conversation").await;
    let got = small.runner.keep_warm_once(&sid, &live).await;
    assert_eq!(
        got,
        Outcome::Forgotten("its prefix is under [cache] keep_warm_min_tokens".into())
    );
    assert!(reads(&fake).is_empty() && reads(&fake2).is_empty());
    // A 5-minute profile is none of the keep-warm's either.
    let dir3 = tempfile::tempdir().unwrap();
    let mut c = Config::example();
    c.cache.keep_warm_min_tokens = 0;
    let (five, fake3) = build(dir3.path(), c);
    let sid = session(&five, SessionKind::Conversation, None);
    turn(&five, &sid, "on five minutes").await;
    let got = five.runner.keep_warm_once(&sid, &live).await;
    assert!(matches!(got, Outcome::Forgotten(_)), "{got:?}");
    assert!(reads(&fake3).is_empty());
}

/// Past the day ceiling the read is refused before any call: the day's
/// `spend.ceiling` row says it once, and the session's reads stop until its
/// next message, which starts them again.
#[tokio::test]
async fn none_past_the_daily_ceiling_and_said_once() {
    let dir = tempfile::tempdir().unwrap();
    // The ceiling's stop mode (theseus-9e02): under the default, notify, it warns and refuses nothing.
    let mut c = one_hour(24);
    c.kernel.daily_spend_ceiling_mode = crate::config::DailyCeilingMode::Stop;
    let (core, fake) = build(dir.path(), c);
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "before the ceiling").await;
    core.kernel.day_ceiling().set_limit(1);
    let live = core.live_profile().0;
    let got = core.runner.keep_warm_once(&sid, &live).await;
    assert!(
        matches!(&got, Outcome::Stopped(why) if why.contains("daily ceiling")),
        "{got:?}"
    );
    assert!(reads(&fake).is_empty(), "no call past the ceiling");
    assert_eq!(rows(&core, "spend.ceiling").len(), 1);
    assert!(rows(&core, "keep_warm").is_empty());
    let interval = Duration::from_millis(core.cfg.cache.interval_ms());
    assert_eq!(core.runner.keep_warm.next_due(interval), None, "stopped");
    assert_eq!(core.cache_health().stopped, 1);
    // A message lifts the stop.
    core.runner.keep_warm.message(&sid, true);
    assert!(core.runner.keep_warm.next_due(interval).is_some());
}

/// A limit that notifies lets a read go past the session's limit, but not
/// past its hard multiple (theseus-jse4): a session already at ten times its
/// limit is refused before any call, and its reads stop.
#[tokio::test]
async fn none_past_the_hard_multiple_of_a_notifying_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    c.kernel.spend_limit_usd = 1.0;
    c.kernel.spend_limit_mode = crate::config::SpendLimitMode::Notify;
    let (core, fake) = build(dir.path(), c);
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "before the stop").await;
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.unwrap();
    core.kernel
        .book_spend(
            &exec,
            10_000_000,
            theseus_protocol::LedgerKind::KeepWarm,
            serde_json::json!({}),
        )
        .unwrap();
    let live = core.live_profile().0;
    let got = core.runner.keep_warm_once(&sid, &live).await;
    assert!(
        matches!(&got, Outcome::Stopped(why) if why.contains("hard stop")),
        "{got:?}"
    );
    assert!(reads(&fake).is_empty(), "no call past the hard stop");
    assert_eq!(core.cache_health().stopped, 1);
}

/// A stopping daemon sends none, and its tender ends.
#[tokio::test]
async fn none_from_a_stopping_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), one_hour(24));
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "before the stop").await;
    core.outbox.stop_sending();
    let live = core.live_profile().0;
    assert_eq!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Stopping
    );
    tokio::time::pause();
    core.tend_keep_warm_after_serving();
    tokio::time::advance(3 * 55 * MIN).await;
    settle().await;
    assert!(reads(&fake).is_empty());
}

/// A new message starts the window again: a read past `keep_warm_hours`
/// after the first message, inside it after the second, goes.
#[tokio::test]
async fn a_new_message_starts_the_window_again() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), one_hour(2));
    let live = core.live_profile().0;
    let sid = session(&core, SessionKind::Conversation, None);
    tokio::time::pause();
    turn(&core, &sid, "first").await;
    tokio::time::advance(121 * MIN).await;
    assert_eq!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Forgotten("its window closed".into())
    );
    turn(&core, &sid, "again").await;
    tokio::time::advance(110 * MIN).await;
    turn(&core, &sid, "and again, near the window's end").await;
    tokio::time::advance(60 * MIN).await;
    assert!(matches!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Read { .. }
    ));
    assert_eq!(reads(&fake).len(), 1);
}

/// A restart rebuilds the windows from the store: the session's newest
/// message and its last activity, nothing stored for it.
#[tokio::test]
async fn a_restart_rebuilds_the_windows_from_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let sid = {
        let (core, _) = build(dir.path(), one_hour(24));
        let sid = session(&core, SessionKind::Conversation, None);
        turn(&core, &sid, "before the restart").await;
        let shared = session(&core, SessionKind::Conversation, Some("channel:4242"));
        turn(&core, &shared, "a guild channel's").await;
        core.store.flush().unwrap();
        sid
    };
    let (core, fake) = build(dir.path(), one_hour(24));
    let live = core.live_profile().0;
    assert_eq!(core.runner.keep_warm_restore(&live).unwrap(), 1);
    let interval = Duration::from_millis(core.cfg.cache.interval_ms());
    assert!(core.runner.keep_warm.next_due(interval).is_some());
    assert!(matches!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Read { .. }
    ));
    assert_eq!(reads(&fake).len(), 1);
}

/// Health's `cache`: each profile's TTL, inherited or its own, and the
/// keep-warm's counts after a read.
#[tokio::test]
async fn health_lists_each_profiles_ttl_and_the_reads() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = build(dir.path(), one_hour(24));
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "a question").await;
    let live = core.live_profile().0;
    core.runner.keep_warm_once(&sid, &live).await;
    let h = core.health().cache.unwrap();
    let of = |name: &str| h.profiles.iter().find(|p| p.profile == name).unwrap();
    assert_eq!(
        (of("sonnet").ttl.as_str(), of("sonnet").inherited),
        ("1h", true)
    );
    assert_eq!(
        (of("haiku").ttl.as_str(), of("haiku").inherited),
        ("1h", false)
    );
    assert_eq!(
        (of("default").ttl.as_str(), of("default").inherited),
        ("1h", false)
    );
    assert_eq!(of("sonnet").keep_warm_hours, 24.0);
    assert_eq!((h.kept, h.stopped, h.reads_today), (1, 0, 1));
    assert!(h.usd_today > 0.0);
    assert_eq!(h.keep_warm_minutes, 55.0);
}

/// With deferred tools (theseus-7m7w.2), the read sends the tools the
/// session's requests send, deferred as they are, so its prefix is still the
/// next turn's: a read that sent the whole catalog would write a cache no
/// turn reads.
#[tokio::test]
async fn the_reads_prefix_holds_with_tools_deferred() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeProvider::default());
    let mut c = one_hour(24);
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.roots = vec![];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(c, fake.clone(), store);
    parts.cfg.tools.defer.enabled = true;
    let core = Core::build(parts).unwrap();
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "first, a question").await;
    turn(&core, &sid, "then another").await;
    let live = core.live_profile().0;
    assert!(matches!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Read { .. }
    ));
    turn(&core, &sid, "and the next message").await;
    let all = fake.requests();
    let read = reads(&fake).pop().unwrap();
    let next = all.last().unwrap();
    assert!(
        next.tools
            .iter()
            .any(|t| t.get("defer_loading") == Some(&Value::Bool(true))),
        "the session's requests defer some tools"
    );
    let bytes = |v: &[Value]| serde_json::to_string(v).unwrap();
    assert_eq!(bytes(&read.tools), bytes(&next.tools), "the tools");
    assert_eq!(bytes(&read.system), bytes(&next.system), "the system");
    let n = read.messages.len() - 1;
    assert_eq!(
        bytes(&read.messages[..n]),
        bytes(&next.messages[..n]),
        "the messages before the new one"
    );
}

/// Reads stop at the model's break-even (theseus-rwpbg.3, M3): a 1-hour write costs `write / read`
/// reads, a read every 55 minutes, so past that many windows the reads cost more than the rewrite
/// they avoid. Sonnet 5.5 (4 against 0.20 a million) breaks even at 18.3 hours (36.7 while its cache reads were $0.10), under a 72-hour window.
#[tokio::test]
async fn keep_warm_stops_at_break_even() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _fake) = build(dir.path(), one_hour(72));
    let live = core.live_profile().0;
    let entry = core.catalog.get("claude-sonnet-5-5").unwrap();
    let cap = crate::keep_warm::break_even_hours(entry, 55.0).unwrap();
    assert!((cap - 18.33).abs() < 0.01, "{cap}");
    let sid = session(&core, SessionKind::Conversation, None);
    tokio::time::pause();
    turn(&core, &sid, "first").await;
    tokio::time::advance(17 * 60 * MIN).await;
    assert!(matches!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Read { .. }
    ));
    tokio::time::advance(2 * 60 * MIN).await;
    assert_eq!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Forgotten("its window closed".into())
    );
}

/// The caps per catalog model, printed for the report (`--nocapture`); a priced model has one.
#[test]
fn break_even_per_catalog_model() {
    for (name, e) in &crate::catalog::Catalog::builtin().entries {
        let cap = crate::keep_warm::break_even_hours(e, 55.0);
        eprintln!(
            "break-even {name}: {cap:?} h (read {}, write 1h {})",
            e.cache_read_per_mtok, e.cache_write_1h_per_mtok
        );
        assert_eq!(
            cap.is_some(),
            e.cache_read_per_mtok > 0.0 && e.cache_write_1h_per_mtok > 0.0
        );
    }
}

/// A keep-warm read carries no pass (theseus-a1wvq.10): a session whose last
/// target is of restricted origin is forgotten, with the reason, and nothing
/// is sent or refused at the chokepoint; a session of US origin beside it is
/// still read.
#[tokio::test]
async fn a_restricted_target_is_never_kept_warm() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    c.copy_profile("dsflash", "glm");
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.roots = vec![];
    let (fake, zai) = (
        Arc::new(FakeProvider::default()),
        Arc::new(FakeProvider::default()),
    );
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(c, fake.clone(), store);
    parts.providers.insert("zai".into(), zai.clone());
    let core = Core::build(parts).unwrap();
    let (restricted, plain) = (
        session(&core, SessionKind::Conversation, None),
        session(&core, SessionKind::Conversation, None),
    );
    turn(&core, &restricted, "the first conversation").await;
    turn(&core, &plain, "the second conversation").await;
    let (live, _) = core.live_profile();
    let mut rec: SessionRecord = core.store.get_session(&restricted).unwrap().unwrap();
    let t = core
        .runner
        .resolve_target(&live, Some("dsflash"), None, None)
        .unwrap();
    rec.last_target = Some(crate::session::TargetRef {
        profile: t.profile,
        provider: t.provider,
        model: t.model,
    });
    core.store.put_session(&restricted, &rec).unwrap();

    let out = core.runner.keep_warm_once(&restricted, &live).await;
    assert!(
        matches!(&out, Outcome::Forgotten(w) if w.contains("is of restricted origin")),
        "{out:?}"
    );
    assert!(zai.requests().is_empty(), "nothing was sent to it");
    assert!(reads(&fake).is_empty());
    assert!(crate::tests_judge::kinds(&core.store, "data_gate.blocked").is_empty());

    let out = core.runner.keep_warm_once(&plain, &live).await;
    assert!(matches!(out, Outcome::Read { .. }), "{out:?}");
    assert_eq!(reads(&fake).len(), 1);
}

/// A turn's end drops the room it recorded for a task wait, so a session's
/// entry does not outlive its turn (the stack review's N10).
#[test]
fn a_turns_end_forgets_the_room_a_wait_recorded() {
    let k = crate::keep_warm::KeepWarm::default();
    k.set_room("ses_room", 40_000, 200_000);
    assert_eq!(k.room("ses_room"), Some((40_000, 200_000)));
    k.turn_ended("ses_room");
    assert_eq!(k.room("ses_room"), None);
}
