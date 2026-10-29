//! The future on the work board, through whole cores (theseus-nuhsz; horizon
//! HZ8): a reminder, a routine, a watch and a deferred task are views with
//! their times; a routine's run is its child and a failed run needs you until
//! it runs well; a met watch ends done; `work.agenda` expands a series by the
//! kernel's own arithmetic and skips what is paused; and `status.short` says
//! the next reminder and the routines that failed. The shapes of the
//! `work.changed` frames a routine's run and a met watch make are a golden
//! (`THESEUS_GOLDEN=write` rewrites it).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::notices::Notice;
use theseus_protocol::work::horizon::{AgendaResult, AGENDA_MAX};
use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::{notify, Message, Request, WorkChanged};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::approval::{Client, Surface};
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderError, ProviderFuture, ProviderRequest, Scripted,
};
use crate::push::WorkScope;
use crate::test_support::{board_caught_up, until_within as until};
use crate::{Config, Core};

const TARGET: &str = "discord:dm:42";
const BRIEF: &str = "Read the tide table and say when the next high tide is.";
const HOUR: u64 = 3_600_000;

/// A stand-in model: `Tide at 06:12.`, or a 400 while `fail` is set.
struct Model {
    fail: AtomicBool,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let answer = if self.fail.load(Ordering::SeqCst) {
                Scripted::Fail(ProviderError::InvalidRequest {
                    status: 400,
                    message: "invalid_request_error: the feed was down".into(),
                })
            } else {
                Scripted::text("Tide at 06:12.")
            };
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    driver: tokio::task::JoinHandle<()>,
    dir: tempfile::TempDir,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = crate::policy::Posture::Notify;
    cfg.kernel.min_repeat_minutes = 1;
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        fail: AtomicBool::new(false),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let driver = tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        driver,
        dir,
    }
}

async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(1 << 20);
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

/// What a `work.watch` connection holds and has heard.
struct Watcher {
    rx: UnboundedReceiver<Message>,
    changed: Vec<WorkChanged>,
    notices: Vec<Notice>,
}

impl Watcher {
    async fn start(core: &Arc<Core>) -> Self {
        core.push.ensure(core).await.unwrap();
        let (tx, rx) = unbounded_channel();
        core.bus.watch_work("w", tx, WorkScope::default(), false);
        Self {
            rx,
            changed: Vec::new(),
            notices: Vec::new(),
        }
    }

    fn drain(&mut self) {
        while let Ok(Message::Notification(n)) = self.rx.try_recv() {
            match n.method.as_str() {
                notify::WORK_CHANGED => {
                    self.changed.push(serde_json::from_value(n.params).unwrap())
                }
                notify::WORK_NOTICE => self.notices.push(serde_json::from_value(n.params).unwrap()),
                _ => {}
            }
        }
    }

    fn held(&mut self) -> BTreeMap<String, WorkView> {
        self.drain();
        let mut out: BTreeMap<String, WorkView> = BTreeMap::new();
        for c in &self.changed {
            for v in &c.views {
                if out.get(&v.id).is_none_or(|o| v.position > o.position) {
                    out.insert(v.id.clone(), v.clone());
                }
            }
            for g in &c.gone {
                out.remove(g);
            }
        }
        out
    }
}

fn of_kind(held: &BTreeMap<String, WorkView>, kind: WorkKind) -> Vec<WorkView> {
    held.values().filter(|v| v.kind == kind).cloned().collect()
}

async fn made_routine(core: &Arc<Core>, schedule: &str) -> Value {
    rpc(
        core,
        "routine.create",
        json!({"title": "Tide check", "schedule": schedule, "brief": BRIEF}),
    )
    .await
    .unwrap()["routine"]
        .clone()
}

/// The routine's wake, set to fire in a second.
async fn due_soon(core: &Arc<Core>) {
    let wakes = rpc(core, "wake.list", json!({})).await.unwrap();
    let w = wakes["wakes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["purpose"] == "routine")
        .expect("the routine's wake");
    rpc(
        core,
        "wake.snooze",
        json!({"wake": w["short"], "when": "1s"}),
    )
    .await
    .unwrap();
}

async fn agenda(core: &Arc<Core>, from: u64, to: u64) -> AgendaResult {
    serde_json::from_value(
        rpc(core, "work.agenda", json!({"from_ms": from, "to_ms": to}))
            .await
            .unwrap(),
    )
    .unwrap()
}

/// The board's views the way a late `work.watch` reads them.
fn snapshot(core: &Arc<Core>) -> Vec<WorkView> {
    core.push.work_snapshot(0, &WorkScope::default()).1
}

/// A reminder, a routine, a watch and a deferred task are each a view with
/// their time, and a client that watches late gets them with the snapshot even
/// though no root they sit under works.
#[tokio::test(flavor = "multi_thread")]
async fn the_future_is_on_the_board_with_its_times() {
    let r = rig();
    let now = theseus_protocol::now_unix_ms();
    rpc(
        &r.core,
        "wake.create",
        json!({"when": "in 2h", "note": "Call the harbour master"}),
    )
    .await
    .unwrap();
    rpc(
        &r.core,
        "wake.create",
        json!({"when": "in 3h", "note": "Water the ferns", "every": "1d", "days": ["mon", "fri"]}),
    )
    .await
    .unwrap();
    let routine = made_routine(&r.core, "weekdays 08:00").await;
    let file = r.dir.path().join("pr");
    let watch = rpc(
        &r.core,
        "watch.create",
        json!({"title": "PR 123 merged?", "check": {"kind": "file", "path": file},
               "until": "equals MERGED", "every": "1m"}),
    )
    .await
    .unwrap();
    let deferred = rpc(
        &r.core,
        "task.start",
        json!({"brief": "Draft the release notes", "title": "Release notes", "start_when": "in 5h"}),
    )
    .await
    .unwrap();
    // Nothing is working; the snapshot still holds the future, read from the
    // store by the seed when the board is first asked for.
    r.core.push.ensure(&r.core).await.unwrap();
    let views = snapshot(&r.core);
    let by_title = |t: &str| {
        views
            .iter()
            .find(|v| v.title == t)
            .unwrap_or_else(|| panic!("{t}: {views:#?}"))
    };
    let one_shot = by_title("Call the harbour master");
    assert_eq!(one_shot.kind, WorkKind::Wake);
    assert_eq!(one_shot.purpose.as_deref(), Some("remind"));
    let at = one_shot.next_at_ms.expect("its time");
    assert!(
        (now + 2 * HOUR - 60_000..now + 2 * HOUR + 60_000).contains(&at),
        "{at}"
    );
    assert!(one_shot.series.is_none() && !one_shot.paused);
    let series = by_title("Water the ferns")
        .series
        .clone()
        .expect("a series");
    assert_eq!(series.every.as_deref(), Some("1d"));
    assert_eq!(series.days, ["mon", "fri"]);
    assert_eq!(series.occurrence, 1);
    let rt = by_title("Tide check");
    assert_eq!(rt.kind, WorkKind::Routine);
    assert_eq!(rt.state, WorkState::Ready);
    let rv = rt.routine.as_ref().expect("the routine's part");
    assert_eq!(rv.schedule, "weekdays 08:00");
    assert_eq!(rv.deliver_to, "dm");
    assert_eq!(rv.month.limit_usd, 10.0);
    assert!(rt.next_at_ms.is_some() && rt.series.is_some());
    assert_eq!(rt.session_id.as_deref(), routine["id"].as_str());
    let wt = by_title("PR 123 merged?");
    assert_eq!(wt.kind, WorkKind::Watch);
    assert_eq!(wt.state, WorkState::Waiting);
    let wv = wt.watch.as_ref().expect("the watch's part");
    assert_eq!(wv.until, "equals MERGED");
    assert!(wv.check.ends_with("/pr"), "{}", wv.check);
    assert!(wv.expires_at_ms.is_some());
    assert_eq!(watch["watch"]["kind"], "file");
    let task = views
        .iter()
        .find(|v| v.kind == WorkKind::Task && v.next_at_ms.is_some())
        .expect("the deferred task");
    assert!((now + 5 * HOUR - 60_000..now + 5 * HOUR + 60_000).contains(&task.next_at_ms.unwrap()));
    let _ = deferred;
}

/// A routine's run hangs from the routine; a run that fails leaves the routine
/// needing you, with run now, pause and details to answer with, until a run
/// goes well; the failed run says nothing of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_run_needs_you_until_the_next_run_goes_well() {
    let r = rig();
    let mut w = Watcher::start(&r.core).await;
    r.model.fail.store(true, Ordering::SeqCst);
    let info = made_routine(&r.core, "every 1m").await;
    let id = info["id"].as_str().unwrap().to_string();
    until("the routine is on the board", 10, || {
        !of_kind(&w.held(), WorkKind::Routine).is_empty()
    })
    .await;
    due_soon(&r.core).await;
    until("the routine needs you", 30, || {
        of_kind(&w.held(), WorkKind::Routine)
            .iter()
            .any(|v| v.state == WorkState::NeedsYou)
    })
    .await;
    let held = w.held();
    let routine = of_kind(&held, WorkKind::Routine).remove(0);
    let last = routine
        .routine
        .as_ref()
        .unwrap()
        .last
        .as_ref()
        .expect("a last run");
    assert!(
        matches!(last.outcome.as_str(), "failed" | "blocked"),
        "{last:?}"
    );
    assert_eq!(routine.routine.as_ref().unwrap().runs_30d.failed, 1);
    assert!(
        routine.attention.label.starts_with("failed"),
        "{}",
        routine.attention.label
    );
    let run = of_kind(&held, WorkKind::Task).remove(0);
    assert_eq!(
        run.parent.as_deref(),
        Some(routine.id.as_str()),
        "the run is its child"
    );
    w.drain();
    let ours: Vec<&Notice> = w
        .notices
        .iter()
        .filter(|n| n.work.id == routine.id || n.work.id == run.id)
        .collect();
    assert_eq!(
        ours.len(),
        1,
        "the routine speaks once for its run: {ours:#?}"
    );
    assert_eq!(ours[0].work.id, routine.id);
    let answers: Vec<&str> = ours[0].answers.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(answers, ["run_now", "pause", "details"]);
    // Paused: waiting, and no longer needing you.
    rpc(&r.core, "routine.pause", json!({"id": id}))
        .await
        .unwrap();
    until("waiting", 10, || {
        of_kind(&w.held(), WorkKind::Routine)[0].state == WorkState::Waiting
    })
    .await;
    assert!(of_kind(&w.held(), WorkKind::Routine)[0].paused);
    rpc(&r.core, "routine.resume", json!({"id": id}))
        .await
        .unwrap();
    // The feed is back: run now, and the routine is ready again.
    r.model.fail.store(false, Ordering::SeqCst);
    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    until("ready again", 30, || {
        let v = of_kind(&w.held(), WorkKind::Routine).remove(0);
        v.state == WorkState::Ready && v.routine.as_ref().is_some_and(|x| x.runs_30d.ok == 1)
    })
    .await;
}

/// A met watch ends done with its last value; `status.short` counts what is
/// ahead and what failed.
#[tokio::test(flavor = "multi_thread")]
async fn a_met_watch_ends_done_and_status_short_counts_the_future() {
    let r = rig();
    let mut w = Watcher::start(&r.core).await;
    let file = r.dir.path().join("pr");
    let made = rpc(
        &r.core,
        "watch.create",
        json!({"title": "PR 123 merged?", "check": {"kind": "file", "path": file},
               "until": "equals MERGED", "every": "1m"}),
    )
    .await
    .unwrap();
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    r.core.watches.with(&id, |e| e.spec.every_ms = 700).unwrap();
    rpc(
        &r.core,
        "wake.create",
        json!({"when": "in 1h", "note": "Stretch"}),
    )
    .await
    .unwrap();
    // The board is a projection the push applies after the frame commits
    // (theseus-hanu), so the reminder is counted once it has: awaited as the
    // views below are, never read the instant `wake.create` answers.
    until("the reminder ahead", 10, || {
        r.core.push.status_short().next_due_ms.is_some()
    })
    .await;
    let short = r.core.push.status_short();
    assert!(short.next_due_ms.is_some(), "{short:?}");
    assert_eq!(short.routines_failed, 0);
    std::fs::write(&file, "OPEN\n").unwrap();
    until("its value", 10, || {
        of_kind(&w.held(), WorkKind::Watch)
            .first()
            .and_then(|v| v.watch.as_ref()?.last.clone())
            .is_some_and(|l| l.value == "OPEN")
    })
    .await;
    std::fs::write(&file, "MERGED\n").unwrap();
    until("met", 10, || {
        of_kind(&w.held(), WorkKind::Watch)
            .first()
            .is_some_and(|v| v.state == WorkState::Done)
    })
    .await;
    let done = of_kind(&w.held(), WorkKind::Watch).remove(0);
    assert_eq!(
        done.watch.as_ref().unwrap().last.as_ref().unwrap().value,
        "MERGED"
    );
    assert!(done.state.ended() && done.ended_at_ms.is_some());
    // Done work is not the future.
    assert!(!snapshot(&r.core).iter().any(|v| v.kind == WorkKind::Watch));
}

/// A run-count stop is a fact the daemon keeps, so the agenda serves only the
/// occurrences that will run: the stop's runs less the runs opened, however the
/// window sits (theseus-97b8c, review H-4). A client draws what it is served.
#[tokio::test(flavor = "multi_thread")]
async fn the_agenda_serves_no_occurrence_past_a_run_count_stop() {
    let r = rig();
    let now = theseus_protocol::now_unix_ms();
    let made = rpc(
        &r.core,
        "routine.create",
        json!({"title": "Tide check", "schedule": "every 1h", "brief": BRIEF,
               "stop": "after 3 runs"}),
    )
    .await
    .unwrap()["routine"]
        .clone();
    assert_eq!(made["stop"], "after 3 runs", "{made}");
    let routine_lines = |a: &AgendaResult| -> Vec<(u64, Option<u32>)> {
        a.lines
            .iter()
            .filter(|l| l.kind == WorkKind::Routine)
            .map(|l| (l.at_ms, l.occurrence))
            .collect()
    };
    // A day of hourly runs, and three of them are all that will ever run.
    let a = agenda(&r.core, now, now + 24 * HOUR).await;
    let all = routine_lines(&a);
    assert_eq!(all.len(), 3, "three runs and no more: {all:?}");
    assert!(!a.more);
    // A window that opens after the first is still cut at the stop, not at the
    // window: the occurrences it skips over used the runs up.
    let later = agenda(&r.core, now + HOUR + HOUR / 2, now + 24 * HOUR).await;
    assert_eq!(
        routine_lines(&later),
        all[1..].to_vec(),
        "the second and the third, which are what is left"
    );
    // And past the last there is nothing.
    let after = agenda(&r.core, all[2].0 + 1, now + 24 * HOUR).await;
    assert!(routine_lines(&after).is_empty(), "{after:?}");
}

/// `work.agenda`: a weekday series is only on weekdays and numbered; a pause
/// takes its occurrences out; a minute series is cut at 200 lines and says so;
/// a window of nothing, or backwards, is refused.
#[tokio::test(flavor = "multi_thread")]
async fn the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused() {
    let r = rig();
    let now = theseus_protocol::now_unix_ms();
    let routine = made_routine(&r.core, "weekdays 08:00").await;
    let id = routine["id"].as_str().unwrap().to_string();
    rpc(
        &r.core,
        "wake.create",
        json!({"when": "in 2h", "note": "Once"}),
    )
    .await
    .unwrap();
    let a = agenda(&r.core, now, now + 14 * 24 * HOUR).await;
    let runs: Vec<_> = a
        .lines
        .iter()
        .filter(|l| l.kind == WorkKind::Routine)
        .collect();
    assert!(
        (9..=11).contains(&runs.len()),
        "two weeks of weekdays: {}",
        runs.len()
    );
    let zone = r.core.kernel.config().zone.clone();
    for l in &runs {
        let z = theseus_kernel::cron::local(&zone, l.at_ms, "%a %H:%M");
        assert!(!z.starts_with("Sat") && !z.starts_with("Sun"), "{z}");
        assert!(z.ends_with("08:00"), "{z}");
    }
    let n: Vec<u32> = runs.iter().filter_map(|l| l.occurrence).collect();
    assert!(
        n.windows(2).all(|p| p[1] == p[0] + 1),
        "numbered in order: {n:?}"
    );
    assert!(
        a.lines.windows(2).all(|p| p[0].at_ms <= p[1].at_ms),
        "soonest first"
    );
    assert!(a.lines.iter().any(|l| l.title == "Once"));
    assert!(!a.more);
    // The window starts later: the numbers carry on, not from 1.
    let later = agenda(&r.core, now + 7 * 24 * HOUR, now + 14 * 24 * HOUR).await;
    let first = later
        .lines
        .iter()
        .find(|l| l.kind == WorkKind::Routine)
        .unwrap();
    assert!(first.occurrence.unwrap() > runs[0].occurrence.unwrap());
    // Paused: no occurrences at all.
    rpc(&r.core, "routine.pause", json!({"id": id}))
        .await
        .unwrap();
    // The board applies a committed frame after the call returns (theseus-t0y6v).
    board_caught_up(&r.core, &id).await;
    let paused = agenda(&r.core, now, now + 14 * 24 * HOUR).await;
    assert!(
        !paused.lines.iter().any(|l| l.kind == WorkKind::Routine),
        "a paused series has no occurrences"
    );
    assert!(paused.lines.iter().any(|l| l.title == "Once"));
    rpc(&r.core, "routine.resume", json!({"id": id}))
        .await
        .unwrap();
    // A minute series fills the cap.
    rpc(&r.core, "routine.delete", json!({"id": id}))
        .await
        .unwrap();
    let minute = made_routine(&r.core, "every 1m").await;
    board_caught_up(&r.core, minute["id"].as_str().unwrap()).await;
    let busy = agenda(&r.core, now, now + 2 * 24 * HOUR).await;
    assert_eq!(busy.lines.len(), AGENDA_MAX);
    assert!(busy.more);
    let err = rpc(
        &r.core,
        "work.agenda",
        json!({"from_ms": now, "to_ms": now}),
    )
    .await;
    assert!(err.is_err());
}

/// The shapes of the frames a routine's run and a met watch make: which view
/// changed to what, in order, with the ids and numbers made alike.
#[tokio::test(flavor = "multi_thread")]
async fn the_frames_a_routines_run_and_a_met_watch_make_read_as_the_golden_says() {
    let r = rig();
    let mut w = Watcher::start(&r.core).await;
    let info = made_routine(&r.core, "every 1m").await;
    let file = r.dir.path().join("pr");
    let made = rpc(
        &r.core,
        "watch.create",
        json!({"title": "PR 123 merged?", "check": {"kind": "file", "path": file},
               "until": "exists", "every": "1m"}),
    )
    .await
    .unwrap();
    let wid = made["watch"]["wake_id"].as_str().unwrap().to_string();
    r.core
        .watches
        .with(&wid, |e| e.spec.every_ms = 700)
        .unwrap();
    due_soon(&r.core).await;
    until("the run's report", 30, || {
        of_kind(&w.held(), WorkKind::Routine)
            .first()
            .is_some_and(|v| v.routine.as_ref().is_some_and(|x| x.runs_30d.ok == 1))
    })
    .await;
    std::fs::write(&file, "here\n").unwrap();
    until("met", 10, || {
        of_kind(&w.held(), WorkKind::Watch)
            .first()
            .is_some_and(|v| v.state == WorkState::Done)
    })
    .await;
    w.drain();
    let _ = info;
    let mut seen: Vec<String> = Vec::new();
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    for c in &w.changed {
        for v in c
            .views
            .iter()
            .filter(|v| matches!(v.kind, WorkKind::Routine | WorkKind::Watch))
        {
            let line = format!(
                "{:?} {:?} parent={} · {} · {} · next={} paused={} series={}",
                v.kind,
                v.state,
                v.parent.as_deref().map(|p| &p[..4]).unwrap_or("-"),
                v.attention.label,
                v.purpose.as_deref().unwrap_or("-"),
                v.next_at_ms.is_some(),
                v.paused,
                v.series.is_some(),
            );
            // Each kind's changes alone: the two interleave as the frames fall.
            if last.insert(v.kind.as_str().to_string(), line.clone()) != Some(line.clone()) {
                seen.push(line);
            }
        }
    }
    let got = seen.join("\n") + "\n";
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/horizon_board.txt");
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, &got).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(got, want, "THESEUS_GOLDEN=write rewrites it");
}
