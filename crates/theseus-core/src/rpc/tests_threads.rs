//! Threads (theseus-nhew) through the core: a thread made, listed, shown,
//! edited, paused, resumed, finished and reopened, each one frame with its
//! ledger row; the board kept in memory and read again at a restart; a shared
//! place that can neither address, list nor bind one; a thread's session
//! superseded when it is replaced, the thread following; two threads' turns
//! that overlap in time; and the daily ceiling that covers all of them.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::method;
use theseus_protocol::threads::{
    ThreadEditParams, ThreadListParams, ThreadNewParams, ThreadState, WorkingSet,
};
use theseus_protocol::TurnSubmitParams;

use super::tests::test_core;
use super::{Core, Parts};
use crate::approval::{Answerer, Client, Surface};
use crate::ledger::LedgerRow;
use crate::places::{BoundPlace, PlaceClass};
use crate::provider::FakeProvider;
use crate::session::SessionRecord;

const LAB: u64 = 314_159_265_358_979_323;
const STRANGER: u64 = 777;

fn core() -> Arc<Core> {
    test_core("the tide is out")
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
    }
}

/// A person in a shared channel of the binding's.
fn stranger() -> Answerer {
    Answerer {
        label: format!("discord:{STRANGER}"),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: STRANGER.to_string(),
            channel_id: LAB.to_string(),
            guild_id: Some("7".into()),
        }),
    }
}

fn bind_a_shared_lab(core: &Core) {
    core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
}

fn new(core: &Core, slug: &str) -> theseus_protocol::threads::ThreadRow {
    core.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap()
}

fn rows(core: &Core, kind: &str) -> Vec<LedgerRow> {
    core.store
        .ledger_tail::<LedgerRow>(2000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

/// The frames `f` wrote on this thread: the writes are synchronous.
fn frames<T>(f: impl FnOnce() -> T) -> (u64, T) {
    let before = theseus_store::frames_written_here();
    let out = f();
    (theseus_store::frames_written_here() - before, out)
}

/// A second core over `c`'s store, as a restarted daemon's.
fn restarted(c: &Core) -> Arc<Core> {
    let model = Arc::new(FakeProvider::default());
    Core::build(Parts::for_tests((*c.cfg).clone(), model, c.store.clone())).unwrap()
}

/// Every edit of a thread is one frame holding its record and its ledger row,
/// and an edit that changes nothing writes nothing.
#[test]
fn each_edit_of_a_thread_is_one_frame_with_its_row() {
    let c = core();
    let (n, made) = frames(|| new(&c, "tide-gauges"));
    assert_eq!(n, 1, "thread.new");
    assert_eq!(made.thread.state, ThreadState::Active);
    assert_eq!(made.thread.title, "Title of tide-gauges");
    assert_eq!(rows(&c, "thread.created").len(), 1);
    assert_eq!(rows(&c, "thread.created")[0].data["slug"], "tide-gauges");

    let edit = |p: ThreadEditParams| c.thread_edit(&p, cli());
    let (n, r) = frames(|| {
        edit(ThreadEditParams {
            slug: "tide-gauges".into(),
            goal: Some("Recalibrate   gauge 4".into()),
            topics: Some(vec!["topic:tides".into(), "topic:tides".into()]),
            working_set: Some(WorkingSet {
                repos: vec!["gauge-log".into()],
                ..Default::default()
            }),
            card: Some(vec!["gauge 4 drifts".into(), "".into()]),
            persona: Some("surveyor".into()),
            ..Default::default()
        })
        .unwrap()
    });
    assert_eq!(n, 1, "thread.edit");
    assert_eq!(r.thread.goal, "Recalibrate gauge 4");
    assert_eq!(r.thread.topics, ["topic:tides"]);
    assert_eq!(r.thread.card, ["gauge 4 drifts"]);
    assert_eq!(r.thread.persona.as_deref(), Some("surveyor"));
    let edited = rows(&c, "thread.edited");
    assert_eq!(edited.len(), 1);
    assert_eq!(
        edited[0].data["changed"],
        serde_json::json!(["goal", "topics", "working_set", "card", "persona"])
    );
    // The same words again change nothing: no frame, no row.
    let (n, _) = frames(|| {
        edit(ThreadEditParams {
            slug: "tide-gauges".into(),
            goal: Some("Recalibrate gauge 4".into()),
            ..Default::default()
        })
        .unwrap()
    });
    assert_eq!((n, rows(&c, "thread.edited").len()), (0, 1));
    // An empty persona clears it.
    let r = edit(ThreadEditParams {
        slug: "tide-gauges".into(),
        persona: Some(String::new()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(r.thread.persona, None);

    for (name, kind, state) in [
        (method::THREAD_PAUSE, "thread.paused", ThreadState::Paused),
        (method::THREAD_RESUME, "thread.resumed", ThreadState::Active),
        (method::THREAD_DONE, "thread.done", ThreadState::Done),
        (
            method::THREAD_REOPEN,
            "thread.reopened",
            ThreadState::Active,
        ),
    ] {
        let (n, r) = frames(|| move_thread(&c, name, "tide-gauges"));
        assert_eq!(n, 1, "{name}");
        assert_eq!(r.thread.state, state, "{name}");
        let got = rows(&c, kind);
        assert_eq!(got.len(), 1, "{kind}");
        assert_eq!(got[0].data["slug"], "tide-gauges");
        assert_eq!(got[0].data["state"], state.as_str());
    }
    // A move to the state it is in writes nothing.
    let (n, _) = frames(|| move_thread(&c, method::THREAD_RESUME, "tide-gauges"));
    assert_eq!((n, rows(&c, "thread.resumed").len()), (0, 1));
}

/// One of the four moves, by its method.
fn move_thread(c: &Core, name: &str, slug: &str) -> theseus_protocol::threads::ThreadRow {
    match name {
        method::THREAD_PAUSE => c.thread_pause(slug, cli()),
        method::THREAD_RESUME => c.thread_resume(slug, cli()),
        method::THREAD_DONE => c.thread_done(slug, cli()),
        _ => c.thread_reopen(slug, cli()),
    }
    .unwrap()
}

/// One `turn.submit` over a connection from the CLI, as the CLI sends it:
/// the response, as JSON.
async fn ask(
    core: Arc<Core>,
    slug: Option<&str>,
    session: Option<&str>,
    input: &str,
) -> serde_json::Value {
    use theseus_protocol::{Id, Message, Request};
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.serve_connection(sr, sw, Client::new("cli", Surface::Cli)));
    let (cr, mut cw) = tokio::io::split(client);
    let req = Request::new(
        Id::Num(1),
        method::TURN_SUBMIT,
        TurnSubmitParams {
            dir: None,
            carried: false,
            prompt: None,
            session_id: session.map(str::to_string),
            input: input.into(),
            profile: None,
            provider: None,
            model: None,
            author: None,
            attachments: vec![],
            reply_to: None,
            opened_from: None,
            thread: slug.map(str::to_string),
            speaking: None,
        },
    );
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
            break serde_json::to_value(r).unwrap();
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

fn session_of(resp: &serde_json::Value) -> String {
    resp["result"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no session in {resp}"))
        .to_string()
}

fn rec(core: &Core, id: &str) -> SessionRecord {
    core.store.get_session(id).unwrap().unwrap()
}

/// A thread's turns run in the session its place names, opened by the first
/// and reused by the next; the session posts nowhere, as a CLI session's.
#[tokio::test]
async fn a_message_to_a_thread_opens_its_session_once_and_the_next_reuses_it() {
    let c = core();
    new(&c, "tides");
    let first = ask(c.clone(), Some("tides"), None, "hello").await;
    let sid = session_of(&first);
    assert_eq!(
        c.outbox.place_session("thread:tides").unwrap(),
        Some(sid.clone())
    );
    assert_eq!(rec(&c, &sid).label.as_deref(), Some("thread:tides"));
    assert!(
        c.outbox.target(&sid).is_none(),
        "a thread's session has no Discord target"
    );
    let again = ask(c.clone(), Some("tides"), None, "and again").await;
    assert_eq!(session_of(&again), sid);
    assert_eq!(rec(&c, &sid).turns, 2);
    let board = c.threads();
    assert_eq!(
        board.of_session(&sid).map(|t| t.slug.as_str()),
        Some("tides")
    );
    assert_eq!(
        board.get("tides").unwrap().session_id.as_deref(),
        Some(sid.as_str())
    );
    // Its list shows the session, and today's dollars as a number.
    let list = c.thread_list(&ThreadListParams::default(), cli()).unwrap();
    assert_eq!(list.threads.len(), 1);
    assert_eq!(
        list.threads[0].thread.session_id.as_deref(),
        Some(sid.as_str())
    );
    assert!(list.threads[0].thread.last_active_ms >= rec(&c, &sid).last_active_ms);
    // A turn names a session or a thread, not both; a missing thread is not found.
    let both = ask(c.clone(), Some("tides"), Some(&sid), "x").await;
    assert_eq!(
        both["error"]["code"],
        theseus_protocol::error_code::INVALID_PARAMS
    );
    let none = ask(c.clone(), Some("nowhere"), None, "x").await;
    assert_eq!(
        none["error"]["code"],
        theseus_protocol::error_code::NOT_FOUND
    );
}

/// A paused or finished thread takes no message, and says how to change that.
#[tokio::test]
async fn a_paused_or_finished_thread_takes_no_message() {
    let c = core();
    new(&c, "tides");
    c.thread_pause("tides", cli()).unwrap();
    let r = ask(c.clone(), Some("tides"), None, "hello").await;
    assert_eq!(r["error"]["code"], theseus_protocol::error_code::REFUSED);
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("thread resume"),
        "{r}"
    );
    assert!(c.outbox.place_session("thread:tides").unwrap().is_none());
    c.thread_resume("tides", cli()).unwrap();
    assert!(ask(c.clone(), Some("tides"), None, "hello").await["result"].is_object());
    c.thread_done("tides", cli()).unwrap();
    let r = ask(c.clone(), Some("tides"), None, "hello").await;
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("thread reopen"),
        "{r}"
    );
    // A finished thread is reopened, never resumed.
    assert!(c.thread_resume("tides", cli()).is_err());
    assert_eq!(c.threads().get("tides").unwrap().state, ThreadState::Done);
}

/// A restart reads the board back: the record, and the session its place
/// runs on.
#[test]
fn a_restart_reloads_the_board_and_the_session_each_thread_runs_on() {
    let c = core();
    new(&c, "tides");
    new(&c, "moons");
    c.thread_pause("moons", cli()).unwrap();
    let sid = c.thread_session("tides", cli()).unwrap().session_id;
    let again = restarted(&c);
    let board = again.threads();
    assert_eq!(board.len(), 2);
    assert_eq!(
        board.of_session(&sid).map(|t| t.slug.as_str()),
        Some("tides")
    );
    assert_eq!(board.get("moons").unwrap().state, ThreadState::Paused);
    assert_eq!(board.get("tides").unwrap().title, "Title of tides");
    assert_eq!(
        again.thread_session("tides", cli()).unwrap().session_id,
        sid
    );
}

/// A shared place cannot make, list, show, edit or address a thread, and
/// opens no session for one; a thread's place is private whichever way it is
/// spelled, and the router's candidates for a shared place are none.
#[test]
fn a_shared_place_can_neither_address_list_nor_bind_a_thread() {
    use theseus_protocol::error_code::REFUSED;
    let c = core();
    bind_a_shared_lab(&c);
    new(&c, "tides");
    let refused = |e: super::server::RpcFailure| assert_eq!(e.code, REFUSED, "{}", e.message);
    refused(
        c.thread_list(&ThreadListParams::default(), stranger())
            .unwrap_err(),
    );
    refused(c.thread_show("tides", stranger()).unwrap_err());
    refused(
        c.thread_new(
            &ThreadNewParams {
                slug: "sneaky".into(),
                ..Default::default()
            },
            stranger(),
        )
        .unwrap_err(),
    );
    refused(c.thread_pause("tides", stranger()).unwrap_err());
    refused(c.thread_session("tides", stranger()).unwrap_err());
    assert!(
        c.outbox.place_session("thread:tides").unwrap().is_none(),
        "no place was bound"
    );
    assert_eq!(c.threads().get("tides").unwrap().state, ThreadState::Active);
    assert!(c.threads().get("sneaky").is_none());
    // The owner's own DM, bound private, may.
    let class = |t: &str| c.runner.place_rule.class(&c.cfg, Some(t));
    assert_eq!(class("thread:tides"), PlaceClass::Private);
    assert_eq!(class("discord:thread:tides"), PlaceClass::Private);
    assert_eq!(class(&format!("discord:channel:{LAB}")), PlaceClass::Shared);
    let board = c.threads();
    assert!(board.live(PlaceClass::Shared).is_empty());
    assert_eq!(board.live(PlaceClass::Private).len(), 1);
}

/// A thread's session replaced (retired by the owner, as `/new` retires a
/// place's) is superseded by the next message's session, and the thread
/// follows it.
#[test]
fn a_new_session_in_a_thread_supersedes_the_old_one_and_the_thread_follows() {
    use theseus_protocol::sessions::RetiredReason;
    let c = core();
    new(&c, "tides");
    let s1 = c.thread_session("tides", cli()).unwrap().session_id;
    assert_eq!(
        c.thread_session("tides", cli()).unwrap().session_id,
        s1,
        "reused while it lives"
    );
    c.retire_session(&s1, &cli()).unwrap();
    let s2 = c.thread_session("tides", cli()).unwrap().session_id;
    assert_ne!(s1, s2);
    let old = rec(&c, &s1);
    assert_eq!(
        old.superseded_by.as_ref().map(|l| l.session_id.as_str()),
        Some(s2.as_str())
    );
    assert_eq!(
        old.retired.as_ref().map(|r| r.reason),
        Some(RetiredReason::ByHand)
    );
    assert_eq!(
        rec(&c, &s2)
            .supersedes
            .as_ref()
            .map(|l| l.session_id.as_str()),
        Some(s1.as_str())
    );
    let board = c.threads();
    assert_eq!(
        board.of_session(&s2).map(|t| t.slug.as_str()),
        Some("tides")
    );
    assert!(
        board.of_session(&s1).is_none(),
        "the old session serves no thread"
    );
    assert_eq!(
        board.get("tides").unwrap().session_id.as_deref(),
        Some(s2.as_str())
    );
    assert_eq!(
        c.outbox.place_session("thread:tides").unwrap().as_deref(),
        Some(s2.as_str())
    );
    let sup = rows(&c, "session.superseded");
    assert_eq!(sup.len(), 1);
    assert_eq!(sup[0].data["place"], "thread:tides");
}

/// Finishing a thread retires its session and leaves its card in it as a
/// note, in the row too; reopening it lets the next message open a successor.
#[test]
fn finishing_a_thread_retires_its_session_and_leaves_its_card() {
    use crate::node::Body;
    let c = core();
    new(&c, "tides");
    c.thread_edit(
        &ThreadEditParams {
            slug: "tides".into(),
            goal: Some("Recalibrate gauge 4".into()),
            card: Some(vec!["gauge 4 drifts 2 mm a week".into()]),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let s1 = c.thread_session("tides", cli()).unwrap().session_id;
    c.thread_done("tides", cli()).unwrap();
    assert!(c.session_retired(&s1));
    let nodes = c.store.session_nodes(&s1).unwrap();
    let note = nodes
        .iter()
        .rev()
        .find_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("a note in the session");
    assert!(
        note.contains("Thread tides (Title of tides) is done"),
        "{note}"
    );
    assert!(
        note.contains("gauge 4 drifts 2 mm a week") && note.contains("Recalibrate gauge 4"),
        "{note}"
    );
    let done = rows(&c, "thread.done");
    assert_eq!(
        done[0].data["card"],
        serde_json::json!(["gauge 4 drifts 2 mm a week"])
    );
    assert_eq!(done[0].data["session"], s1);
    c.thread_reopen("tides", cli()).unwrap();
    let s2 = c.thread_session("tides", cli()).unwrap().session_id;
    assert_ne!(s1, s2);
    assert_eq!(
        rec(&c, &s1)
            .superseded_by
            .as_ref()
            .map(|l| l.session_id.as_str()),
        Some(s2.as_str())
    );
}

/// `[threads] max_live` bounds the open threads, `enabled = false` refuses
/// them, and a bad slug or too many topics are refused with words.
#[test]
fn the_config_bounds_the_threads_and_a_bad_slug_is_refused() {
    let mut cfg = crate::Config::example();
    cfg.threads.max_live = 2;
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let c = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap();
    new(&c, "a");
    new(&c, "b");
    let e = c
        .thread_new(
            &ThreadNewParams {
                slug: "c".into(),
                ..Default::default()
            },
            cli(),
        )
        .unwrap_err();
    assert!(e.message.contains("max_live"), "{}", e.message);
    c.thread_done("a", cli()).unwrap();
    new(&c, "c");
    for bad in ["", "Tides", "-x", "a b"] {
        let e = c
            .thread_new(
                &ThreadNewParams {
                    slug: bad.into(),
                    ..Default::default()
                },
                cli(),
            )
            .unwrap_err();
        assert!(e.message.contains("is not a slug"), "{bad}: {}", e.message);
    }
    let e = c
        .thread_new(
            &ThreadNewParams {
                slug: "d".into(),
                topics: Some(vec!["a".into(), "b".into(), "c".into(), "d".into()]),
                ..Default::default()
            },
            cli(),
        )
        .unwrap_err();
    assert!(e.message.contains("at most 3 topics"), "{}", e.message);
    // The same slug twice: it exists.
    let e = c
        .thread_new(
            &ThreadNewParams {
                slug: "b".into(),
                ..Default::default()
            },
            cli(),
        )
        .unwrap_err();
    assert!(e.message.contains("exists"), "{}", e.message);
    // Off, nothing is made.
    let mut cfg = crate::Config::example();
    cfg.threads.enabled = false;
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let off = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap();
    let e = off
        .thread_new(
            &ThreadNewParams {
                slug: "a".into(),
                ..Default::default()
            },
            cli(),
        )
        .unwrap_err();
    assert!(e.message.contains("threads are off"), "{}", e.message);
}

/// A turn in a thread reads the board in memory: a record written behind
/// its back (the thread paused) changes nothing until a restart reads the
/// store, which is the only time it does.
#[test]
fn a_turn_in_a_thread_reads_the_board_in_memory() {
    let c = core();
    new(&c, "tides");
    let mut t = c.threads().get("tides").cloned().unwrap();
    t.state = ThreadState::Paused;
    c.store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some("thread:tides"),
            &t,
        )
        .unwrap()])
        .unwrap();
    c.thread_session("tides", cli())
        .expect("the board in memory still has it active");
    let again = restarted(&c);
    let e = again.thread_session("tides", cli()).unwrap_err();
    assert!(e.message.contains("is paused"), "{}", e.message);
}

/// Two threads' turns overlap in time: each model call takes 3 s on the
/// paused clock, and both answer within 3 s of each other's start, not 6.
#[tokio::test(start_paused = true)]
async fn two_threads_turns_overlap_in_time() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider {
        reply: "slow".into(),
        delay_ms: 3_000,
        ..Default::default()
    });
    let c = Core::build(Parts::for_tests(crate::Config::example(), model, store)).unwrap();
    new(&c, "tides");
    new(&c, "moons");
    let t0 = tokio::time::Instant::now();
    let (a, b) = tokio::join!(
        ask(c.clone(), Some("tides"), None, "hello"),
        ask(c.clone(), Some("moons"), None, "hello")
    );
    let took = t0.elapsed();
    assert!(
        a["result"].is_object() && b["result"].is_object(),
        "{a} {b}"
    );
    assert_ne!(session_of(&a), session_of(&b));
    assert!(took >= Duration::from_secs(3), "{took:?}");
    assert!(
        took < Duration::from_millis(5_500),
        "the turns ran one after the other: {took:?}"
    );
    let list = c.thread_list(&ThreadListParams::default(), cli()).unwrap();
    assert!(list.threads.iter().all(|t| t.thread.session_id.is_some()));
}

/// The daily ceiling is the daemon's, not a thread's: what one thread spent
/// today stops another's call (the code is unchanged; this holds it).
#[tokio::test]
async fn the_daily_ceiling_covers_every_thread() {
    use crate::tests_day_ceiling::{billed, build, turn_in};
    use crate::turn::TurnError;
    let dir = tempfile::tempdir().unwrap();
    let spent = {
        let (core, _) = build(dir.path(), vec![billed("one")], |_| {});
        new(&core, "tides");
        new(&core, "moons");
        let sa = core.thread_session("tides", cli()).unwrap().session_id;
        turn_in(&core, &sa, "hello").await.unwrap();
        let c = core.kernel.day_ceiling();
        let t = c.today(c.now());
        assert!(t.spent > 0);
        t.spent
    };
    let ceiling = theseus_kernel::micros_to_usd(spent + 1);
    let (core, fake) = build(dir.path(), vec![billed("two")], |c| {
        c.kernel.daily_spend_ceiling_usd = ceiling;
    });
    let sb = core.thread_session("moons", cli()).unwrap().session_id;
    let res = turn_in(&core, &sb, "hi").await;
    assert_eq!(
        res.unwrap_err().downcast_ref::<TurnError>().unwrap().class,
        "daily_ceiling",
        "another thread's turn is stopped by the day's spend"
    );
    assert!(fake.requests().is_empty(), "no call was made");
    // And what the day cost shows on the thread that spent it.
    let list = core
        .thread_list(&ThreadListParams::default(), cli())
        .unwrap();
    let tides = list
        .threads
        .iter()
        .find(|t| t.thread.slug == "tides")
        .unwrap();
    assert!(tides.today_usd > 0.0, "{tides:?}");
}
