//! The TUI's rows fed by the work board (theseus-k2vca): each row says what
//! its task is doing and for how long, a finished task's first report line
//! shows under it until seen, the header counts every level and the window
//! title only what needs you; and a daemon without `work.watch` leaves the
//! rows as they were.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::{utc_hm, ExecutionView, SessionKind, WaitingOn};

use crate::tests::{conv, info, input, pending, question, view, Captured, Rig, World, NOW, T0};

const REG: &str = "ses_reg0001";

/// A work view: a base for a task of the regatta, with `extra` laid over it.
fn wview(v: &ExecutionView, title: &str, extra: Value) -> Value {
    let mut base = json!({
        "id": v.execution_id,
        "kind": if v.kind == SessionKind::Task { "task" } else { "conversation" },
        "position": v.position, "at_ms": v.at_ms,
        "parent": v.parent_session_id.as_ref().map(|p| format!("exe_{p}")),
        "root": format!("exe_{REG}"), "session_id": v.session_id,
        "title": title, "state": "running",
        "attention": v.attention, "started_at_ms": T0 - 600_000,
        "state_since_ms": v.attention.since_ms,
    });
    for (k, x) in extra.as_object().unwrap() {
        base[k] = x.clone();
    }
    base
}

fn waiting_on_job() -> Option<WaitingOn> {
    Some(WaitingOn::Actions {
        correlation_ids: vec!["act_job01".into()],
    })
}

/// The five-task regatta: a conversation and five tasks, as the executions
/// say it; and as the work board does.
fn regatta() -> (Vec<ExecutionView>, Vec<Value>) {
    let reg = conv(100, REG, "waiting", input(), 0.40);
    let mk = |p, sid: &str, state: &str, w, pend| {
        view(p, sid, SessionKind::Task, Some(REG), state, w, pend, 2, 0.1)
    };
    let sound = mk(101, "ses_sound01", "waiting", waiting_on_job(), vec![]);
    let fetch = mk(102, "ses_fetch01", "running", None, vec![]);
    let paint = mk(
        103,
        "ses_paint01",
        "waiting",
        Some(WaitingOn::Confirm {
            confirm_id: "cor_paint01".into(),
        }),
        vec![pending("cor_paint01", "write regatta/paint-log.md (37 B)")],
    );
    let mut gauge = mk(104, "ses_gauge01", "failed", None, vec![]);
    gauge.ended_reason = Some("the provider refused the request (400)".into());
    gauge.attention = theseus_protocol::attention(&gauge, &utc_hm);
    let boats = mk(105, "ses_boats01", "running", None, vec![]);
    let work = vec![
        wview(
            &reg,
            "Regatta",
            json!({"state": "ready", "kind": "conversation", "parent": null}),
        ),
        wview(
            &sound,
            "Sound the north channel",
            json!({"state": "waiting",
                "doing": {"what": "job", "label": "waiting on 1 call", "since_ms": T0 - 134_000,
                    "turn": 2, "loop_index": 0},
                "progress": {"steps_done": 3, "steps_total": 5, "said": "leg 5: 4.5 fathoms",
                    "at_ms": T0},
            }),
        ),
        json!({
            "id": "act_job01", "kind": "job", "position": 101, "at_ms": T0, "parent": sound.execution_id,
            "root": format!("exe_{REG}"), "title": "sounder --leg 5", "state": "running",
            "attention": {"level": "working", "label": "running", "since_ms": T0 - 134_000},
            "started_at_ms": T0 - 134_000, "state_since_ms": T0 - 134_000,
            "eta": {"usual_ms": 160_000, "runs": 4},
        }),
        wview(
            &fetch,
            "Fetch Saturday's forecast",
            json!({"doing": {"what": "thinking", "label": "turn 3", "since_ms": T0 - 31_000,
                "turn": 3, "loop_index": 0}}),
        ),
        wview(
            &paint,
            "Paint the south buoy",
            json!({"state": "needs_you", "question": {
                "id": "cor_paint01", "kind": "approval",
                "asked_by": {"id": paint.execution_id, "kind": "task", "title": "Paint the south buoy"},
                "prompt": "fs.write create regatta/paint-log.md (37 B)", "options": [],
                "floor": false, "may_answer": true, "tool": "fs.write", "asked_at_ms": T0 - 5_000}}),
        ),
        wview(
            &gauge,
            "Read the tide gauge",
            json!({"state": "failed", "ended_at_ms": T0 + 3_600_000,
                "report": {"outcome": "failed", "reason": "the provider refused the request (400)"}}),
        ),
        wview(
            &boats,
            "Count the boats",
            json!({"doing": {"what": "tool", "label": "fs.read", "since_ms": T0 - 12_000,
                "turn": 4, "loop_index": 0}}),
        ),
    ];
    (vec![reg, sound, fetch, paint, gauge, boats], work)
}

fn regatta_world(work: Option<Vec<Value>>) -> Arc<Mutex<World>> {
    let (views, mut works) = regatta();
    let mut w = World {
        board: json!({"position": 200, "executions": views, "total": 6,
            "confirms": [question("ses_paint01", "cor_paint01", "write regatta/paint-log.md (37 B)", 103)]}),
        ..World::default()
    };
    for (sid, kind, label, title) in [
        (REG, SessionKind::Conversation, Some("Regatta"), None),
        (
            "ses_sound01",
            SessionKind::Task,
            None,
            Some("Sound the north channel"),
        ),
        (
            "ses_fetch01",
            SessionKind::Task,
            None,
            Some("Fetch Saturday's forecast"),
        ),
        (
            "ses_paint01",
            SessionKind::Task,
            None,
            Some("Paint the south buoy"),
        ),
        (
            "ses_gauge01",
            SessionKind::Task,
            None,
            Some("Read the tide gauge"),
        ),
        (
            "ses_boats01",
            SessionKind::Task,
            None,
            Some("Count the boats"),
        ),
    ] {
        w.titles.insert(
            sid.to_string(),
            serde_json::to_value(info(sid, kind, label, title)).unwrap(),
        );
    }
    if let Some(extra) = work {
        works = extra;
    }
    w.answers.insert(
        "work.watch".into(),
        Ok(json!({"position": 200, "views": works, "total": 6})),
    );
    Arc::new(Mutex::new(w))
}

async fn rig(width: u16, world: Arc<Mutex<World>>) -> (Rig, Captured) {
    NOW.with(|n| n.set(T0));
    let mut rig = Rig::new(width, 30, crate::tests::script(world));
    let out = Captured::default();
    rig.runner.out = Box::new(out.clone());
    rig.shows("Paint").await;
    rig.asked("work.watch", 1).await;
    rig.settle().await;
    (rig, out)
}

fn row<'a>(screen: &'a [String], name: &str) -> &'a str {
    screen
        .iter()
        .find(|l| l.contains(name))
        .unwrap_or_else(|| panic!("no row for {name}: {screen:#?}"))
}

/// Each row form, at 80 and 120 columns: a job against its usual time and the
/// plan's steps, a model, a tool, a question, a failure.
#[tokio::test]
async fn each_row_says_what_its_task_is_doing_and_for_how_long() {
    for width in [80u16, 120] {
        let (rig, _) = rig(width, regatta_world(None)).await;
        let s = rig.screen();
        eprintln!("{width}:\n{}", s.join("\n"));
        assert!(
            row(&s, "Sound").contains("3/5 job 2:14 of usual 2:40"),
            "{s:#?}"
        );
        assert!(row(&s, "Fetch").contains("model 0:31"), "{s:#?}");
        assert!(row(&s, "Count").contains("fs.read 0:12"), "{s:#?}");
        assert!(row(&s, "Paint").contains("asks: fs.write create"), "{s:#?}");
        assert!(
            row(&s, "Read th").contains("failed 15:13: the provider"),
            "{s:#?}"
        );
    }
}

fn push_work(rig: &Rig, position: u64, views: Vec<Value>) {
    rig.daemon().notify(
        "work.changed",
        json!({"position": position, "at_ms": T0, "views": views}),
    );
}

/// The header counts each level; the window title counts only what needs you,
/// and nothing when nothing does.
#[tokio::test]
async fn the_header_counts_each_level_and_the_title_only_what_needs_you() {
    let (mut rig, out) = rig(120, regatta_world(None)).await;
    let head = rig.screen()[0].clone();
    assert!(
        head.contains("● 1 needs you · ◐ 3 working · ✗ 1 failed"),
        "{head}"
    );
    assert!(
        out.text().contains("\x1b]0;theseus ●1\x07"),
        "{:?}",
        out.text()
    );
    assert!(
        !out.text().contains("theseus ●4"),
        "working tasks are not counted"
    );
    // The paint task is answered: nothing needs you, and the title says so.
    let (views, _) = regatta();
    let mut paint = views[3].clone();
    paint.position = 300;
    paint.state = "running".into();
    paint.waiting_on = None;
    paint.pending.clear();
    paint.attention = theseus_protocol::attention(&paint, &utc_hm);
    rig.daemon().notify(
        "confirm.resolved",
        json!({"correlation_id": "cor_paint01", "session_id": "ses_paint01", "approved": true}),
    );
    rig.daemon()
        .notify("execution.changed", serde_json::to_value(&paint).unwrap());
    rig.until("the title to clear", |_| {
        out.text().ends_with("\x1b]0;theseus\x07")
    })
    .await;
    let head = rig.screen()[0].clone();
    assert!(!head.contains("needs you"), "{head}");
    assert!(head.contains("◐ 4 working"), "{head}");
}

/// A finished task's first report line shows under its row, marked new until
/// it is seen; looking at the task clears the mark.
#[tokio::test]
async fn a_report_appears_under_its_row_and_is_new_until_seen() {
    let (mut rig, _) = rig(120, regatta_world(None)).await;
    assert!(!rig.screen().iter().any(|l| l.contains("41 boats")));
    let (views, works) = regatta();
    let mut boats = views[5].clone();
    boats.position = 400;
    boats.previous = Some("running".into());
    boats.state = "complete".into();
    boats.attention = theseus_protocol::attention(&boats, &utc_hm);
    let mut w = works
        .iter()
        .find(|w| w["id"] == boats.execution_id.as_str())
        .unwrap()
        .clone();
    w["position"] = json!(400);
    w["state"] = json!("done");
    w["doing"] = Value::Null;
    w["attention"] = serde_json::to_value(&boats.attention).unwrap();
    w["ended_at_ms"] = json!(T0 + 30_000);
    w["report"] =
        json!({"outcome": "done", "first_line": "Counted 41 boats: 29 keelboats, 12 dinghies"});
    rig.daemon()
        .notify("execution.changed", serde_json::to_value(&boats).unwrap());
    push_work(&rig, 400, vec![w]);
    rig.shows("Counted 41 boats").await;
    let s = rig.screen();
    let at = s
        .iter()
        .position(|l| l.contains("Count the boats"))
        .unwrap();
    assert!(s[at].contains("done 14:13"), "{s:#?}");
    assert!(
        s[at + 1].contains("◆ Counted 41 boats: 29 keelboats, 12"),
        "{s:#?}"
    );
    // Open it (only the done: the first row): seen.
    rig.press(&[
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyCode::Enter,
    ])
    .await;
    rig.press(&[crossterm::event::KeyCode::Esc]).await;
    rig.settle().await;
    let s = rig.screen();
    assert!(!s.iter().any(|l| l.contains('◆')), "{s:#?}");
}

/// An older daemon does not know `work.watch`: the rows say their labels as
/// before, and the TUI said it was a screen a person looks at.
#[tokio::test]
async fn an_older_daemon_leaves_the_rows_as_they_were() {
    let world = regatta_world(None);
    world.lock().unwrap().answers.remove("work.watch");
    let (rig, out) = rig_without_work(120, world).await;
    let s = rig.screen();
    assert!(row(&s, "waiting on 1").contains("◐"), "{s:#?}");
    assert!(
        !s.iter()
            .any(|l| l.contains("model 0:31") || l.contains("of usual")),
        "{s:#?}"
    );
    assert!(row(&s, "Pain").contains("confirm proc.run"), "{s:#?}");
    let asked = rig.daemon().asked("work.watch");
    assert_eq!(asked, [json!({"interactive": true})]);
    assert!(
        rig.screen()[0].contains("● 1 needs you"),
        "the header still counts"
    );
    assert!(out.text().contains("theseus ●1"));
}

async fn rig_without_work(width: u16, world: Arc<Mutex<World>>) -> (Rig, Captured) {
    NOW.with(|n| n.set(T0));
    let mut rig = Rig::new(width, 30, crate::tests::script(world));
    let out = Captured::default();
    rig.runner.out = Box::new(out.clone());
    rig.shows("turn 2").await;
    rig.asked("work.watch", 1).await;
    rig.settle().await;
    (rig, out)
}

/// An elapsed time repaints on the minute, never each second; a question's
/// countdown, on screen, each second. On tokio's paused clock.
#[tokio::test(start_paused = true)]
async fn elapsed_repaints_on_the_minute_and_a_countdown_each_second() {
    let (mut rig, _) = rig(120, regatta_world(None)).await;
    let next = (T0 / 60_000 + 1) * 60_000;
    assert!(row(&rig.screen(), "Fetch").contains("model 0:31"));
    assert_eq!(
        rig.runner.app.deadline(),
        Some(next),
        "one timer, on the minute: no card, no countdown"
    );
    NOW.with(|n| n.set(next));
    rig.shows("model 1:11").await;
    // The card of a question with a time on it, on screen, counts down each
    // second.
    let mut paint = regatta().0[3].clone();
    paint.position = 500;
    paint.pending[0].expires_at_ms = next + 90_000;
    rig.daemon()
        .notify("execution.changed", serde_json::to_value(&paint).unwrap());
    rig.settle().await;
    rig.press(&[
        crossterm::event::KeyCode::Char('b'),
        crossterm::event::KeyCode::Enter,
    ])
    .await;
    rig.asked("session.history", 1).await;
    rig.settle().await;
    let t = rig.runner.app.now_ms;
    let d = rig.runner.app.deadline().expect("a deadline");
    assert!(
        d <= t + 1_000,
        "the countdown ticks each second: {d} vs {t}"
    );
}

/// After `events.lost` on the work stream the board is asked for again.
#[tokio::test]
async fn lost_work_events_ask_the_board_again() {
    let (mut rig, _) = rig(120, regatta_world(None)).await;
    rig.daemon()
        .notify("events.lost", json!({"dropped": 3, "streams": ["work"]}));
    rig.asked("work.watch", 2).await;
}
