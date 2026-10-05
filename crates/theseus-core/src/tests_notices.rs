//! `security.v3`'s live notices (step 24's notices, theseus-0j2.13), against
//! the fake Jev with scripted scores: an open call v3 is at least 90% sure
//! was risky posts one notice after it started, and nothing else does; a
//! slow Jev never delays the call; the brake (`security.v1`'s rules) stops
//! them for the rest of the day at the 31st notice or the third `noise`
//! label, survives a restart, and lifts at the next local day; and the
//! config's switches each keep v3 in shadow.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::learning::JudgeLabelParams;

use crate::config::{JudgePackConfig, PackMode};
use crate::judge::gate::{judgment_id, SECURITY_CANDIDATE};
use crate::judge::notice::notice_key;
use crate::ledger::LedgerRow;
use crate::outbox::{body_of, kind_of, OPERATOR_TARGET};
use crate::policy::Posture;
use crate::provider::Provider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_security::{
    a_hold, board, rig_with, security_rows, session, the_call, turn, until_judged, Heard, RunsOnce,
};
use crate::Config;

/// `proc.run` at `posture`: `open` runs it unasked.
fn posture(p: Posture) -> impl FnOnce(&mut Config) {
    move |c: &mut Config| {
        c.policy.tools.insert("proc.run".into(), p);
    }
}

/// A model that runs `echo hi` once per turn.
fn echoes() -> Arc<dyn Provider> {
    Arc::new(RunsOnce {
        argv: vec!["echo".into(), "hi".into()],
    })
}

/// The judge's notices: `tool.notified` rows with `by: judge`.
fn notices(store: &Store) -> Vec<LedgerRow> {
    security_rows(store, "tool.notified")
        .into_iter()
        .filter(|r| r.data["by"] == "judge")
        .collect()
}

/// The notices' pauses: `judge.paused` rows with `what: notices`.
fn pauses(store: &Store) -> Vec<LedgerRow> {
    security_rows(store, "judge.paused")
        .into_iter()
        .filter(|r| r.data["what"] == "notices")
        .collect()
}

/// The posts to the owner of `kind`, written and not yet delivered (no
/// binding runs here), by their bodies.
fn posts(core: &Core, kind: &str) -> Vec<Value> {
    core.outbox
        .open_for(OPERATOR_TARGET)
        .iter()
        .filter(|a| kind_of(a) == kind)
        .map(|a| body_of(a).clone())
        .collect()
}

/// Wait, on the runtime's timer, until `done` holds.
async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !done() {
        assert!(t0.elapsed() < Duration::from_secs(20), "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Let whatever the gate's tasks still do land: the judgments are written
/// before a notice is posted, and a notice is posted within its task.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

/// An open call v3 scores `risky` 0.95 posts one notice, after
/// `tool.started`, with v3's percent, the tool, the plan's summary, the
/// reasons and the judgment's id: the post to the owner and its
/// `tool.notified` row in one frame, and `judge.noticed` to the turn's
/// clients. v3's judgment is recorded, and its trace marked, `live`. At 0.89
/// it posts none, whatever `steered` says.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_call_jev_is_sure_was_risky_posts_one_notice_after_it_started() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.95));
    jev.script("exfiltrates", Jev::Noul(0.92));
    jev.script("beyond_ask", Jev::Noul(0.71));
    jev.script("destructive", Jev::Noul(0.10));
    let r = rig_with(echoes(), Some(&jev), posture(Posture::Open)).await;
    let sid = session(&r.core, None);
    let heard: Heard = Arc::default();
    let res = turn(&r.core, &sid, "say hi", Some(&heard)).await;
    assert!(res.awaiting_confirm.is_none(), "it ran unasked");
    until("a notice", || notices(&r.core.store).len() == 1).await;
    until("judge.noticed", || {
        heard
            .lock()
            .unwrap()
            .iter()
            .any(|(_, m, _)| m == "judge.noticed")
    })
    .await;
    settle().await;
    let (corr, _) = the_call(&r.core, &sid, "proc.run");
    let id = judgment_id(SECURITY_CANDIDATE, &corr);
    let rows = notices(&r.core.store);
    assert_eq!(rows.len(), 1, "one notice: {rows:?}");
    let n = &rows[0].data;
    assert_eq!(
        (
            n["judgment"].as_str(),
            n["tool"].as_str(),
            n["percent"].as_u64()
        ),
        (Some(id.as_str()), Some("proc.run"), Some(95))
    );
    assert_eq!(n["id"], notice_key(&id).as_str());
    assert_eq!(
        n["reasons"],
        json!(["sends data out 92%", "beyond the ask 71%"])
    );
    assert_eq!(n["correlation_id"], corr.as_str());
    assert!(
        n["summary"].as_str().is_some_and(|s| s.contains("echo")),
        "{n}"
    );
    assert_eq!(rows[0].session_id.as_deref(), Some(sid.as_str()));
    // Its post to the owner, which holds what the notice says.
    let p = posts(&r.core, "jev_notice");
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(
        (p[0]["judgment"].as_str(), p[0]["percent"].as_u64()),
        (Some(id.as_str()), Some(95))
    );
    assert!(n["post"].as_str().is_some_and(|p| !p.is_empty()), "{n}");
    // After the call started, to the turn's clients.
    let h = heard.lock().unwrap().clone();
    let started = h.iter().position(|(_, m, _)| m == "tool.started").unwrap();
    let noticed = h.iter().position(|(_, m, _)| m == "judge.noticed").unwrap();
    assert!(started < noticed, "after tool.started");
    assert!(h[noticed].0 >= h[started].0);
    let told: theseus_protocol::judge::JudgeNoticed =
        serde_json::from_value(h[noticed].2.clone()).unwrap();
    assert_eq!(
        told.line(),
        "Jev: 95% risky (sends data out 92%, beyond the ask 71%)"
    );
    // v3 is live; v1 stays in shadow.
    let judged = until_judged(&r.core.store, 2).await;
    let mode = |pack: &str| {
        judged
            .iter()
            .find(|j| j.data["pack"] == pack)
            .map(|j| j.data["mode"].clone())
    };
    assert_eq!(mode(SECURITY_CANDIDATE), Some(json!("live")));
    assert_eq!(mode("security.v1"), Some(json!("shadow")));
    let h = r.core.health().judge.unwrap();
    assert_eq!(h.notices, "on");
    assert!(
        h.packs
            .contains(&"security.v3: live (owner: decision of 2026-10-04)".to_string()),
        "{:?}",
        h.packs
    );
    // The notice counts on the ladder: one `notice` event today (26a).
    let day = r.core.runner.judge.today();
    let scope = crate::judge::ladder::events_scope("security", &day);
    let events = || {
        r.core
            .store
            .scope_after(&scope, 0)
            .unwrap()
            .into_iter()
            .map(|x| x.decode::<LedgerRow>().unwrap().data["event"].clone())
            .collect::<Vec<Value>>()
    };
    until("the notice's event", || !events().is_empty()).await;
    assert_eq!(events(), [json!({"event": "notice", "day": day})]);

    // Just under the act band: no notice, though `steered` is sure. Only
    // `risky` decides a notice.
    jev.script("risky", Jev::Noul(0.89));
    jev.script("steered", Jev::Noul(0.99));
    let quiet = session(&r.core, None);
    turn(&r.core, &quiet, "say hi again", None).await;
    until_judged(&r.core.store, 4).await;
    settle().await;
    assert_eq!(notices(&r.core.store).len(), 1, "0.89 posts none");
}

/// Nothing new posts for a call that had its say: a `notify` call (its own
/// notice and score), an `approve` call, and an open call the hold made
/// wait, each scored 0.95.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_that_had_its_say_posts_nothing_new() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.95));
    for (p, held) in [
        (Posture::Notify, false),
        (Posture::Approve, false),
        (Posture::Open, true),
    ] {
        let r = rig_with(echoes(), Some(&jev), posture(p)).await;
        let sid = session(&r.core, held.then(a_hold));
        let res = turn(&r.core, &sid, "say hi", None).await;
        assert_eq!(res.awaiting_confirm.is_some(), p != Posture::Notify || held);
        until_judged(&r.core.store, 2).await;
        settle().await;
        assert!(notices(&r.core.store).is_empty(), "{p:?} held {held}");
        assert!(posts(&r.core, "jev_notice").is_empty(), "{p:?} held {held}");
    }
}

/// A skipped, failed or shed v3 judgment posts nothing: Jev down.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_judgment_posts_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.99));
    jev.set_mode(FakeMode::Down);
    let r = rig_with(echoes(), Some(&jev), posture(Posture::Open)).await;
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "say hi", None).await;
    let rows = until_judged(&r.core.store, 2).await;
    assert!(
        rows.iter()
            .all(|j| j.data["outcome"]["outcome"] == "failed"),
        "{rows:?}"
    );
    settle().await;
    assert!(notices(&r.core.store).is_empty());
}

/// When `tool.started` comes after the turn began, with notices on or off.
async fn started_after(jev: &FakeJev, on: bool) -> Duration {
    let r = rig_with(echoes(), Some(jev), |c| {
        c.policy.tools.insert("proc.run".into(), Posture::Open);
        c.judge.total_secs = 6;
        c.judge.packs.insert(
            SECURITY_CANDIDATE.into(),
            JudgePackConfig {
                notices: Some(on),
                ..Default::default()
            },
        );
    })
    .await;
    let sid = session(&r.core, None);
    let heard: Heard = Arc::default();
    let t0 = Instant::now();
    turn(&r.core, &sid, "say hi", Some(&heard)).await;
    let started = heard
        .lock()
        .unwrap()
        .iter()
        .find(|(_, m, _)| m == "tool.started")
        .map(|(at, _, _)| at.duration_since(t0))
        .expect("tool.started");
    started
}

/// With Jev slow (5 s) and sure the call is risky, `tool.started` comes as
/// fast with notices on as with them off: the call never waits on v3.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_jev_slow_a_noticed_call_starts_as_fast_as_with_notices_off() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.99));
    jev.set_mode(FakeMode::Slow(Duration::from_secs(5)));
    let off = started_after(&jev, false).await;
    let on = started_after(&jev, true).await;
    assert!(
        on < off + Duration::from_millis(1500),
        "on {on:?}, off {off:?}"
    );
    assert!(on < Duration::from_secs(3), "{on:?}");
}

/// Flagged open calls, one turn each, in fresh sessions.
async fn flag(core: &Arc<Core>, n: usize) {
    for i in 0..n {
        let sid = session(core, None);
        turn(core, &sid, &format!("say hi {i}"), None).await;
    }
}

/// The brake: the 31st flagged call of a day trips it, so its notice is not
/// posted, and one pause notice and its row appear (health says so); the
/// next calls post nothing; the next local day's first flagged call posts
/// again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_31st_notice_of_a_day_trips_the_brake_until_the_next_day() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.97));
    let r = rig_with(echoes(), Some(&jev), posture(Posture::Open)).await;
    let c = &r.core;
    flag(c, 30).await;
    until("30 notices", || notices(&c.store).len() == 30).await;
    assert!(pauses(&c.store).is_empty());
    flag(c, 1).await;
    until("the pause", || pauses(&c.store).len() == 1).await;
    flag(c, 2).await;
    until_judged(&c.store, 66).await;
    settle().await;
    assert_eq!(
        notices(&c.store).len(),
        30,
        "the 31st and after are not posted"
    );
    let p = pauses(&c.store);
    assert_eq!(p.len(), 1, "{p:?}");
    let d = &p[0].data;
    assert_eq!(
        (d["pack"].as_str(), d["rule"].as_str(), d["short"].as_str()),
        (
            Some(SECURITY_CANDIDATE),
            Some("notices_per_day"),
            Some("31 notices today")
        )
    );
    let said = posts(c, "jev_paused");
    assert_eq!(said.len(), 1, "one pause notice");
    assert!(
        said[0]["text"]
            .as_str()
            .unwrap()
            .contains("paused until tomorrow: 31 notices today"),
        "{}",
        said[0]
    );
    let h = c.health().judge.unwrap();
    assert!(h.notices.starts_with("paused until "), "{}", h.notices);
    assert!(h.notices.ends_with(": 31 notices today"), "{}", h.notices);
    assert!(!h.paused, "the budget's pause is the budget's alone");
    // The next local day: the first flagged call posts again. The ladder
    // reads the pause as security's day brake by its own clock, so it moves
    // too (26a).
    c.runner.judge.brake().skew(26 * 3600 * 1000);
    let next = theseus_protocol::now_unix_ms() + 26 * 3600 * 1000;
    c.runner.judge.ladder().set_clock(Arc::new(move || next));
    flag(c, 1).await;
    until("a notice the next day", || notices(&c.store).len() == 31).await;
    assert_eq!(c.health().judge.unwrap().notices, "on");
}

/// The owner's label on a judgment, as the CLI gives it.
fn label(core: &Core, judgment: &str, word: &str) -> anyhow::Result<()> {
    let p = JudgeLabelParams {
        judgment: judgment.into(),
        question: None,
        label: json!(word),
        note: None,
        discord: None,
    };
    core.judge_label(&p, "cli").map(|_| ())
}

/// Three `noise` labels on v3's judgments trip the brake on the third, from
/// any surface: a `right` does not count, and a noise on v1's does not.
/// A labeled notice's post says so. A restart mid-pause keeps it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_noise_labels_trip_the_brake_and_a_restart_keeps_it() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.95));
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let server = Arc::new(crate::web::tests::serve().await);
    // One store across both runs.
    let core = on_store(dir.path(), &jev, &server, work.path());
    flag(&core, 4).await;
    until("4 notices", || notices(&core.store).len() == 4).await;
    let judged: Vec<String> = notices(&core.store)
        .iter()
        .map(|n| n.data["judgment"].as_str().unwrap().to_string())
        .collect();
    label(&core, &judged[0], "right").unwrap();
    label(&core, &judged[0], "noise").unwrap();
    label(&core, &judged[1], "noise").unwrap();
    assert!(pauses(&core.store).is_empty(), "two noise labels");
    // A labeled notice's post says the label and who gave it.
    let labeled = posts(&core, "jev_labeled");
    assert_eq!(labeled.len(), 3, "{labeled:?}");
    assert_eq!(labeled[1]["label"], "noise");
    assert_eq!(labeled[1]["judgment"], judged[0].as_str());
    label(&core, &judged[2], "noise").unwrap();
    let p = pauses(&core.store);
    assert_eq!(p.len(), 1, "the third trips it");
    assert_eq!(p[0].data["rule"], "labels_per_day");
    assert_eq!(p[0].data["short"], "3 labeled noise today");
    let said = posts(&core, "jev_paused");
    assert_eq!(said.len(), 1);
    assert!(said[0]["text"]
        .as_str()
        .unwrap()
        .contains("paused until tomorrow: 3 labeled noise today"));
    flag(&core, 1).await;
    until_judged(&core.store, 10).await;
    settle().await;
    assert_eq!(notices(&core.store).len(), 4, "paused: nothing posts");
    // A restart mid-pause: still paused, in health before any judgment,
    // and at the first flagged call.
    drop(core);
    let core = on_store(dir.path(), &jev, &server, work.path());
    let h = core.health().judge.unwrap();
    assert!(h.notices.contains("3 labeled noise today"), "{}", h.notices);
    flag(&core, 1).await;
    until_judged(&core.store, 12).await;
    settle().await;
    assert_eq!(
        notices(&core.store).len(),
        4,
        "still paused after a restart"
    );
    assert_eq!(pauses(&core.store).len(), 1, "and said once");
}

/// A core on the store in `dir`, the judge at `jev`, `proc.run` open.
fn on_store(
    dir: &std::path::Path,
    jev: &FakeJev,
    server: &Arc<crate::web::tests::Server>,
    root: &std::path::Path,
) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.insert("proc.run".into(), Posture::Open);
    cfg.judge.enabled = true;
    cfg.judge.api_base = jev.base();
    cfg.judge.connect_secs = 1;
    cfg.judge.total_secs = 2;
    cfg.validate().unwrap();
    let store = Store::open(&dir.join("store")).unwrap();
    let mut p = Parts {
        toollets: crate::web::tests::web(server.port, Default::default(), true).tools(),
        ..Parts::for_tests(cfg, echoes(), store)
    };
    p.secrets = board();
    Core::build(p).unwrap()
}

/// The config's switches each keep v3 in shadow: `notices = false`, the
/// pack's `mode = "shadow"`, and `max_mode = "shadow"` post nothing, and
/// v3's judgments are recorded in shadow.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_switch_keeps_v3_in_shadow_and_posts_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.99));
    let line = |notices: Option<bool>, mode: Option<PackMode>| {
        move |c: &mut Config| {
            c.policy.tools.insert("proc.run".into(), Posture::Open);
            c.judge.packs.insert(
                SECURITY_CANDIDATE.into(),
                JudgePackConfig {
                    notices,
                    mode,
                    ..Default::default()
                },
            );
        }
    };
    let capped = |c: &mut Config| {
        c.policy.tools.insert("proc.run".into(), Posture::Open);
        c.judge.max_mode = PackMode::Shadow;
    };
    let rigs = [
        rig_with(echoes(), Some(&jev), line(Some(false), None)).await,
        rig_with(echoes(), Some(&jev), line(None, Some(PackMode::Shadow))).await,
        rig_with(echoes(), Some(&jev), capped).await,
    ];
    for (i, r) in rigs.iter().enumerate() {
        let sid = session(&r.core, None);
        turn(&r.core, &sid, "say hi", None).await;
        let judged = until_judged(&r.core.store, 2).await;
        settle().await;
        assert!(notices(&r.core.store).is_empty(), "switch {i}");
        assert!(posts(&r.core, "jev_notice").is_empty(), "switch {i}");
        let v3 = judged
            .iter()
            .find(|j| j.data["pack"] == SECURITY_CANDIDATE)
            .unwrap();
        assert_eq!(v3.data["mode"], "shadow", "switch {i}");
        let h = r.core.health().judge.unwrap();
        assert_eq!(h.notices, "off", "switch {i}");
        // Shadow: with the ladder's adoption under the config's ceiling once
        // a judged point read it (`max_mode = "shadow"` reads it nowhere).
        assert!(
            h.packs.iter().any(|l| l == "security.v3: shadow"
                || l.starts_with("security.v3: shadow (the config's ceiling; ")),
            "{:?}",
            h.packs
        );
    }
}

/// A run's first `noise` label counts once: the brake reads its day from the
/// store at a run's first use, and the store holds the label just written
/// (review R3's join fix; seen live: two labels tripped the brake, and its
/// row said three).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_runs_first_noise_label_counts_once() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.95));
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let server = Arc::new(crate::web::tests::serve().await);
    let core = on_store(dir.path(), &jev, &server, work.path());
    flag(&core, 3).await;
    until("3 notices", || notices(&core.store).len() == 3).await;
    let judged: Vec<String> = notices(&core.store)
        .iter()
        .map(|n| n.data["judgment"].as_str().unwrap().to_string())
        .collect();
    // A restart: the new run has read nothing of the day yet.
    drop(core);
    let core = on_store(dir.path(), &jev, &server, work.path());
    label(&core, &judged[0], "noise").unwrap();
    label(&core, &judged[1], "noise").unwrap();
    assert!(
        pauses(&core.store).is_empty(),
        "two noise labels pause nothing"
    );
    label(&core, &judged[2], "noise").unwrap();
    let p = pauses(&core.store);
    assert_eq!(p.len(), 1, "the third trips it");
    assert_eq!(p[0].data["short"], "3 labeled noise today");
}

/// A ladder rollback of security.v3 stops its notices (batch 5's join,
/// theseus-9j7x): an open call v3 scores 0.95 risky is judged in shadow and
/// posts nothing, and health says the pack is rolled back and the notices
/// off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ladder_rollback_of_v3_stops_its_notices() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.95));
    let r = rig_with(echoes(), Some(&jev), posture(Posture::Open)).await;
    r.core
        .pack_rollback(
            &theseus_protocol::packs::PackRollbackParams {
                pack: SECURITY_CANDIDATE.into(),
                why: None,
            },
            "cli",
        )
        .unwrap();
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "say hi", None).await;
    let judged = until_judged(&r.core.store, 2).await;
    settle().await;
    assert!(notices(&r.core.store).is_empty(), "no notice");
    assert!(posts(&r.core, "jev_notice").is_empty(), "no post");
    let v3 = judged
        .iter()
        .find(|j| j.data["pack"] == SECURITY_CANDIDATE)
        .unwrap();
    assert_eq!(v3.data["mode"], "shadow");
    let h = r.core.health().judge.unwrap();
    assert_eq!(h.notices, "off");
    assert!(
        h.packs
            .contains(&"security.v3: rolled back (owner: the owner rolled it back)".to_string()),
        "{:?}",
        h.packs
    );
}
