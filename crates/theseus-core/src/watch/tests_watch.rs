//! Watches through the whole core (theseus-f2r2b): waiting on the world with
//! no model. A file watch is met within one interval of the change and says
//! so; twenty unchanged checks write no frame; a secret a check read is
//! scrubbed from every row, state and message; three failures in a row
//! inform once; a watch gives up at its expiry; after a restart each open
//! watch checks once and its baseline holds; the cap refuses one more; and
//! none of it calls a model.
//!
//! A check is run the way the driver's tick runs it (`look`: observe, then
//! `watch_finish`), so a test decides when an interval has passed.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{Message, Request, WakeNoticed};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::provider::FakeProvider;
use crate::store::Store;
pub(crate) use crate::test_support::{rows, until_within as until};
use crate::{Config, Core};

pub(super) const DM: &str = "dm:42";
pub(super) const TARGET: &str = "discord:dm:42";

pub(super) struct Life {
    pub(super) core: Arc<Core>,
    pub(super) model: Arc<FakeProvider>,
    events: tokio::sync::mpsc::UnboundedReceiver<Message>,
    driver: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for Life {
    fn drop(&mut self) {
        if let Some(d) = &self.driver {
            d.abort();
        }
    }
}

/// One life of the daemon over the store in `dir`, with a DM bound; the
/// driver runs when `drive` says so.
pub(super) fn life(dir: &Path, drive: bool, tweak: impl FnOnce(&mut Config)) -> Life {
    life_knowing(dir, drive, tweak, crate::scrub::Scrubber::default())
}

/// `life`, its core's scrubber knowing the values it was given.
fn life_knowing(
    dir: &Path,
    drive: bool,
    tweak: impl FnOnce(&mut Config),
    scrubber: crate::scrub::Scrubber,
) -> Life {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = crate::policy::Posture::Notify;
    tweak(&mut cfg);
    let store = Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts {
        scrubber: Arc::new(scrubber),
        ..crate::rpc::Parts::for_tests(cfg, model.clone(), store)
    })
    .unwrap();
    core.bind_places(vec![crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let (tx, events) = tokio::sync::mpsc::unbounded_channel();
    *core.bus.tap.lock().unwrap() = Some(tx);
    let driver = drive.then(|| tokio::spawn(crate::harness::drive(core.clone())));
    Life {
        core,
        model,
        events,
        driver,
    }
}

impl Life {
    /// End this life: the driver stops and the core is let go.
    pub(super) async fn stop(mut self) -> Arc<Core> {
        if let Some(d) = self.driver.take() {
            d.abort();
            let _ = d.await;
        }
        self.core.clone()
    }
}

pub(super) fn dm_session(core: &Arc<Core>) -> String {
    let sid = core
        .open_session(theseus_protocol::SessionOpenParams {
            dir: None,
            kind: None,
            label: None,
            opened_from: None,
            persona: None,
        })
        .unwrap()
        .session_id;
    core.outbox.bind_place(DM, &sid).unwrap();
    sid
}

pub(super) async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(
        sr,
        sw,
        Client::new("sock#1", Surface::Cli),
    ));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r.result.ok_or_else(|| r.error.unwrap().message);
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

/// `watch.create` for a file, from the CLI.
async fn watch_file(core: &Arc<Core>, path: &Path, until: &str, extra: Value) -> Value {
    let mut p = json!({"title": "the deploy landed?", "check": {"kind": "file", "path": path},
                       "until": until});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    rpc(core, "watch.create", p).await.unwrap()
}

/// One check, the way the driver's tick runs it: observe, then finish.
pub(super) async fn look(core: &Arc<Core>, wake_id: &str) {
    let e = core
        .watches
        .get(wake_id)
        .expect("the watch is on the board");
    let mut rows = Vec::new();
    let result = crate::watch::check::observe(&core.watch_tools(), &e, &mut rows).await;
    let result = core.watch_seen(wake_id, &rows, result);
    let (c, id) = (core.clone(), wake_id.to_string());
    tokio::task::spawn_blocking(move || c.watch_finish(&id, result))
        .await
        .unwrap();
}

fn ledger_len(core: &Core) -> usize {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .len()
}

/// The notices the outbox holds: (target, text), each text past its head,
/// which every one carries (theseus-x8b9k).
pub(super) fn posts(core: &Core) -> Vec<(String, String)> {
    core.kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .map(|a| {
            let text = crate::outbox::body_of(&a)["text"].as_str().unwrap_or("");
            assert!(crate::owner_note::is_headed(text), "{text}");
            let body = text.splitn(3, '\n').nth(2).unwrap_or("");
            (crate::outbox::target_of(&a).to_string(), body.to_string())
        })
        .collect()
}

fn noticed(l: &mut Life) -> Vec<WakeNoticed> {
    let mut out = vec![];
    while let Ok(m) = l.events.try_recv() {
        if let Message::Notification(n) = m {
            if n.method == "wake.noticed" {
                out.push(serde_json::from_value(n.params).unwrap());
            }
        }
    }
    out
}

fn pending(core: &Core) -> usize {
    core.kernel.open_watches().unwrap().len()
}

/// The point of the row, in real time: the driver's own tick finds a file
/// that came to say MERGED, within one interval of the change; the owner
/// hears it as an Interrupt with the value and what the wait cost; the watch
/// is gone; and no model was called.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_watch_is_met_within_one_interval_of_the_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = life(dir.path(), true, |_| {});
    let dm = dm_session(&l.core);
    let file = dir.path().join("pr");
    let made = watch_file(&l.core, &file, "equals MERGED", json!({"every": "1m"})).await;
    assert_eq!(made["home"], true, "{made}");
    assert_eq!(
        made["watch"]["session_id"], dm,
        "it lives where the DM runs"
    );
    assert_eq!(made["watch"]["kind"], "file");
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    // A minute is the floor for a person; the test shortens the board's own
    // interval so real time shows the rule.
    l.core.watches.with(&id, |e| e.spec.every_ms = 700).unwrap();
    until("its first look", 10, || {
        l.core.watches.get(&id).is_some_and(|e| e.checks >= 1)
    })
    .await;
    assert!(
        posts(&l.core).is_empty(),
        "nothing yet: the file is not there"
    );
    std::fs::write(&file, "MERGED\n").unwrap();
    let t0 = Instant::now();
    until("the notice", 10, || !posts(&l.core).is_empty()).await;
    assert!(
        t0.elapsed() < Duration::from_millis(700 + 500 + 1_500),
        "within an interval and a tick of the change, not {:?}",
        t0.elapsed()
    );
    let (target, text) = posts(&l.core).remove(0);
    assert_eq!(target, TARGET);
    assert!(
        text.starts_with("👁 the deploy landed → MERGED (after ") && text.ends_with("checks, $0)"),
        "{text}"
    );
    assert_eq!(pending(&l.core), 0, "met: the watch is gone");
    assert_eq!(rows(&l.core, "watch.met").len(), 1);
    assert!(l.core.watches.get(&id).is_none());
    let n = noticed(&mut l);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].purpose, "watch.met");
    assert!(l.model.requests().is_empty(), "no model call, at all");
    // Met watches stay met: nothing more is written.
    let before = ledger_len(&l.core);
    tokio::time::sleep(Duration::from_millis(1_300)).await;
    assert_eq!(ledger_len(&l.core), before);
    assert_eq!(posts(&l.core).len(), 1);
}

/// FAST and quiet: twenty checks of an unchanged value write no frame. The
/// first value and each change are frames; the count rides the next one.
#[tokio::test(flavor = "multi_thread")]
async fn twenty_unchanged_checks_write_no_frame() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), false, |_| {});
    dm_session(&l.core);
    let file = dir.path().join("state");
    std::fs::write(&file, "OPEN").unwrap();
    let made = watch_file(&l.core, &file, "equals MERGED", json!({})).await;
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    look(&l.core, &id).await;
    assert_eq!(
        rows(&l.core, "watch.checked").len(),
        1,
        "the first value is news"
    );
    let rows_before = ledger_len(&l.core);
    for _ in 0..20 {
        let c = l.core.clone();
        let id = id.clone();
        let e = c.watches.get(&id).unwrap();
        let result = crate::watch::check::observe(&c.watch_tools(), &e, &mut Vec::new()).await;
        // On this thread, with no await between the reads: the frames it wrote.
        let b = theseus_store::frames_written_here();
        c.watch_finish(&id, result);
        assert_eq!(
            theseus_store::frames_written_here(),
            b,
            "a check of a value it knows"
        );
    }
    assert_eq!(ledger_len(&l.core), rows_before, "no row either");
    let e = l.core.watches.get(&id).unwrap();
    assert_eq!(e.checks, 21, "the checks were counted in memory");
    // A different value is a frame, and carries the count so far.
    std::fs::write(&file, "DRAFT").unwrap();
    look(&l.core, &id).await;
    let checked = rows(&l.core, "watch.checked");
    assert_eq!(checked.len(), 2);
    assert_eq!(checked[1]["checks"], 22);
    assert_eq!(checked[1]["value"], "DRAFT");
    // The list shows the value and the checks.
    let list = rpc(&l.core, "watch.list", json!({})).await.unwrap();
    assert_eq!(list["watches"][0]["value"], "DRAFT");
    assert_eq!(list["watches"][0]["checks"], 22);
    assert!(l.model.requests().is_empty());
}

/// A watch's value, row and state are what the check read: a value the vault
/// holds is in them as written (the record is raw; the courier that posts
/// the message substitutes it on its way out).
#[tokio::test(flavor = "multi_thread")]
async fn a_secret_in_a_checks_output_is_the_watchs_value_as_read() {
    let dir = tempfile::tempdir().unwrap();
    let token = "vault-held-ghp-a1B2c3D4e5a1B2c3D4e5";
    let scrubber = crate::scrub::Scrubber::with_values(vec![(token.into(), "gh_token".into())]);
    let l = life_knowing(dir.path(), false, |_| {}, scrubber);
    dm_session(&l.core);
    let file = dir.path().join("log");
    std::fs::write(&file, format!("token {token} seen")).unwrap();
    let made = watch_file(&l.core, &file, "contains seen", json!({})).await;
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    look(&l.core, &id).await;
    let (_, text) = posts(&l.core).remove(0);
    let met = serde_json::to_string(&rows(&l.core, "watch.met")).unwrap();
    let state = serde_json::to_string(&l.core.kernel.watch_state(&id).unwrap()).unwrap();
    assert!(met.contains(token), "the row: {met}");
    assert!(state.contains(token), "the state: {state}");
    assert!(text.contains(token), "the message: {text}");
    assert!(!format!("{met}{state}{text}").contains("[redacted:"));
}

/// Three failures in a row inform once, and a longer pause follows each.
#[tokio::test(flavor = "multi_thread")]
async fn three_failures_in_a_row_inform_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = life(dir.path(), false, |_| {});
    dm_session(&l.core);
    // A directory cannot be read as a file.
    let made = watch_file(&l.core, dir.path(), "equals MERGED", json!({})).await;
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    for _ in 0..2 {
        look(&l.core, &id).await;
    }
    assert!(posts(&l.core).is_empty(), "two failures are not news");
    let a = l.core.watches.get(&id).unwrap();
    assert!(
        a.next_ms > l.core.kernel.now_ms() + 4 * 60_000,
        "backing off: {}",
        a.next_ms
    );
    look(&l.core, &id).await;
    let p = posts(&l.core);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(
        p[0].1.contains("keeps failing") && p[0].1.contains("Is a directory"),
        "{}",
        p[0].1
    );
    assert_eq!(rows(&l.core, "watch.failing").len(), 1);
    for _ in 0..5 {
        look(&l.core, &id).await;
    }
    assert_eq!(posts(&l.core).len(), 1, "told once");
    assert_eq!(rows(&l.core, "watch.failing").len(), 1);
    assert_eq!(pending(&l.core), 1, "it goes on trying");
    let n = noticed(&mut l);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].purpose, "watch.failing", "an Inform, not an Interrupt");
    assert_eq!(
        theseus_protocol::notices::wake_urgency(&n[0].purpose),
        theseus_protocol::notices::Urgency::Inform
    );
}

/// At its expiry an unmet watch says it gave up, with the last value.
#[tokio::test(flavor = "multi_thread")]
async fn a_watch_that_reaches_its_expiry_gives_up_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = life(dir.path(), false, |_| {});
    dm_session(&l.core);
    let file = dir.path().join("state");
    std::fs::write(&file, "OPEN").unwrap();
    let made = watch_file(
        &l.core,
        &file,
        "equals MERGED",
        json!({"for_at_most": "1h"}),
    )
    .await;
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    look(&l.core, &id).await;
    assert!(made["watch"]["expires_at_ms"].as_u64().unwrap() > l.core.kernel.now_ms());
    // The hour passes.
    l.core
        .watches
        .with(&id, |e| e.spec.expires_ms = l.core.kernel.now_ms())
        .unwrap();
    look(&l.core, &id).await;
    let p = posts(&l.core);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(
        p[0].1.contains("gave up on \"the deploy landed?\"")
            && p[0].1.contains("the last value was OPEN"),
        "{}",
        p[0].1
    );
    assert_eq!(rows(&l.core, "watch.expired").len(), 1);
    assert_eq!(pending(&l.core), 0);
    let n = noticed(&mut l);
    assert_eq!(n[0].purpose, "watch.expired");
    assert_eq!(
        theseus_protocol::notices::wake_urgency(&n[0].purpose),
        theseus_protocol::notices::Urgency::Inform
    );
}

/// After a restart each open watch checks once, from the state its last
/// frame left: `changes` keeps its baseline, the count goes on, and the same
/// value is quiet.
#[tokio::test(flavor = "multi_thread")]
async fn after_a_restart_each_open_watch_checks_once_and_the_baseline_holds() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state");
    std::fs::write(&file, "OPEN").unwrap();
    let id = {
        let l = life(dir.path(), false, |_| {});
        dm_session(&l.core);
        let made = watch_file(&l.core, &file, "changes", json!({})).await;
        let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
        for _ in 0..3 {
            look(&l.core, &id).await;
        }
        assert_eq!(rows(&l.core, "watch.checked").len(), 1);
        let core = l.stop().await;
        drop(core);
        id
    };
    // The next life: the board starts empty, and the first ticks fill it.
    let l = life(dir.path(), false, |_| {});
    assert!(l.core.watches.get(&id).is_none());
    l.core.watch_tick();
    until("the load", 10, || l.core.watches.get(&id).is_some()).await;
    let e = l.core.watches.get(&id).unwrap();
    assert_eq!(
        (e.first.as_deref(), e.checks),
        (Some("OPEN"), 1),
        "from the last frame"
    );
    let before = ledger_len(&l.core);
    l.core.watch_tick();
    until("its one check", 10, || {
        l.core
            .watches
            .get(&id)
            .is_some_and(|e| e.checks == 2 && !e.running)
    })
    .await;
    assert_eq!(ledger_len(&l.core), before, "the same value is still quiet");
    assert!(
        l.core.watches.get(&id).unwrap().next_ms > l.core.kernel.now_ms() + 4 * 60_000,
        "and then it waits its interval"
    );
    l.core.watch_tick();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        l.core.watches.get(&id).unwrap().checks,
        2,
        "once, not every tick"
    );
    std::fs::write(&file, "MERGED").unwrap();
    look(&l.core, &id).await;
    assert_eq!(
        rows(&l.core, "watch.met").len(),
        1,
        "changes held its baseline over the restart"
    );
}

/// `[kernel] max_watches` bounds the daemon, with words.
#[tokio::test(flavor = "multi_thread")]
async fn the_cap_refuses_one_more_watch() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), false, |c| c.kernel.max_watches = 2);
    dm_session(&l.core);
    let file = dir.path().join("f");
    for _ in 0..2 {
        watch_file(&l.core, &file, "exists", json!({})).await;
    }
    let e = rpc(
        &l.core,
        "watch.create",
        json!({"title": "one more?", "check": {"kind": "file", "path": file}, "until": "exists"}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("pending watches, the most it may (2)"), "{e}");
    assert_eq!(pending(&l.core), 2);
    // A cancel makes room, by the wake's own verb.
    let list = rpc(&l.core, "watch.list", json!({})).await.unwrap();
    let id = list["watches"][0]["wake_id"].as_str().unwrap();
    rpc(&l.core, "wake.cancel", json!({"wake": id}))
        .await
        .unwrap();
    watch_file(&l.core, &file, "exists", json!({})).await;
}

/// The owner's bounds, with words: a minute at the least, 30 days at the
/// most, and a job's shell never sets one.
#[tokio::test(flavor = "multi_thread")]
async fn the_bounds_and_the_owners_act_are_enforced_on_the_wire() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), false, |_| {});
    dm_session(&l.core);
    let file = dir.path().join("f");
    let bad = |extra: Value| {
        let l = &l;
        let file = file.clone();
        async move {
            let mut p =
                json!({"title": "t?", "check": {"kind": "file", "path": file}, "until": "exists"});
            for (k, v) in extra.as_object().unwrap().clone() {
                p[k] = v;
            }
            rpc(&l.core, "watch.create", p).await.unwrap_err()
        }
    };
    assert!(bad(json!({"every": "30m", "for_at_most": "31d"}))
        .await
        .contains("30 days"));
    assert!(bad(json!({"every": "0m"})).await.contains("span"));
    assert!(bad(json!({"until": "soon"}))
        .await
        .contains("not a condition"));
    assert_eq!(pending(&l.core), 0, "nothing was made");
    let ok = watch_file(&l.core, &file, "exists", json!({"every": "1m"})).await;
    assert_eq!(ok["watch"]["every"], "1m");
}
