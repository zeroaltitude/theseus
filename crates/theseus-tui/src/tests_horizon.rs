//! The Horizon section (theseus-nuhsz): the future under the task tree, from
//! `work.watch`'s views and `work.changed`; `H` gives it the keys, and `z`
//! `p` `r` `x` and enter act on the row under the cursor.

use crossterm::event::KeyCode;
use serde_json::{json, Value};
use theseus_protocol::work::horizon::{Month, RoutineView, RunCounts, Series, WatchView};
use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::{Attention, Level};

use crate::tests::{clock, harbour_world, script, Rig, NOW, T0};

const HOUR: u64 = 3_600_000;

fn view(id: &str, kind: WorkKind, title: &str, state: WorkState, level: Level) -> WorkView {
    WorkView {
        id: id.into(),
        kind,
        position: 10,
        at_ms: T0,
        parent: Some("exe_home".into()),
        root: "exe_home".into(),
        session_id: Some(format!("ses_{id}")),
        title: title.into(),
        place: None,
        state,
        previous: None,
        attention: Attention {
            level,
            label: "x".into(),
            since_ms: T0,
        },
        started_at_ms: T0,
        state_since_ms: T0,
        ended_at_ms: None,
        doing: None,
        progress: None,
        question: None,
        cost: None,
        eta: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        flagged: false,
        purpose: None,
        next_at_ms: None,
        series: None,
        paused: false,
        routine: None,
        watch: None,
        project: None,
        dismissed: false,
        retry_of: None,
        forecast: None,
        thread: None,
    }
}

fn reminder(id: &str, title: &str, at: u64) -> WorkView {
    let mut v = view(
        id,
        WorkKind::Wake,
        title,
        WorkState::Waiting,
        Level::Working,
    );
    v.purpose = Some("remind".into());
    v.next_at_ms = Some(at);
    v
}

fn routine(id: &str, title: &str, at: u64) -> WorkView {
    let mut v = view(id, WorkKind::Routine, title, WorkState::Ready, Level::Ready);
    v.purpose = Some("routine".into());
    v.next_at_ms = Some(at);
    v.series = Some(Series {
        every: Some("1d".into()),
        days: vec!["mon".into(), "tue".into()],
        occurrence: 3,
        ..Default::default()
    });
    v.routine = Some(RoutineView {
        schedule: "weekdays 08:00".into(),
        last: None,
        runs_30d: RunCounts {
            ok: 20,
            failed: 1,
            skipped: 0,
        },
        month: Month {
            spent_usd: 2.5,
            limit_usd: 10.0,
        },
        deliver_to: "dm".into(),
    });
    v
}

fn watch(id: &str, title: &str) -> WorkView {
    let mut v = view(
        id,
        WorkKind::Watch,
        title,
        WorkState::Waiting,
        Level::Working,
    );
    v.purpose = Some("watch".into());
    v.watch = Some(WatchView {
        check: "gh pr view 123".into(),
        until: "equals MERGED".into(),
        last: None,
        checks: 12,
        expires_at_ms: Some(T0 + 72 * HOUR),
        failing: None,
    });
    v
}

fn world_with(views: Vec<WorkView>) -> Rig {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "work.watch".into(),
        Ok(json!({"position": 10, "views": views, "total": 0})),
    );
    Rig::new(120, 30, script(world))
}

fn the_future() -> Vec<WorkView> {
    vec![
        // 10:13 local at the tests' zone: today.
        reminder("wak_a8cebb", "Call the harbour master", T0 + 3 * HOUR),
        // Tomorrow 13:13 local: this week, until the day turns.
        reminder("wak_b9dfcc", "Send the tide table", T0 + 30 * HOUR),
        routine("wak_a41d04", "Morning digest", T0 + 20 * HOUR),
        watch("wak_c3d4e5", "PR 123 merged?"),
    ]
}

/// The sections show under the task tree, in order, with each row's words.
#[tokio::test]
async fn the_horizon_shows_under_the_tree_with_a_heading_to_each_section() {
    let mut rig = world_with(the_future());
    rig.shows("Today").await;
    rig.shows("Call the harbour master").await;
    rig.shows("This week").await;
    rig.shows("Send the tide table").await;
    rig.shows("Recurring").await;
    rig.shows("Morning digest").await;
    rig.shows("Waiting on the world").await;
    rig.shows("PR 123 merged?").await;
    let screen = rig.screen().join("\n");
    let at = |s: &str| screen.find(s).unwrap_or_else(|| panic!("{s} on {screen}"));
    assert!(at("Today") < at("This week") && at("This week") < at("Recurring"));
    assert!(at("Recurring") < at("Waiting on the world"));
}

/// `H` focuses it; `z` snoozes a reminder, and the call is the owner's verb.
#[tokio::test]
async fn h_then_z_snoozes_the_reminder_under_the_cursor() {
    let mut rig = world_with(the_future());
    rig.shows("Call the harbour master").await;
    rig.press(&[KeyCode::Char('H'), KeyCode::Char('z')]).await;
    rig.asked("wake.snooze", 1).await;
    assert_eq!(
        rig.daemon().asked("wake.snooze"),
        [json!({"wake": "wak_a8cebb", "author": "the TUI"})]
    );
}

/// `j` moves the cursor down the items; `p` and `r` act on a routine, `x` on a
/// watch, and a key that does not fit the row says so and sends nothing.
#[tokio::test]
async fn the_keys_act_by_the_kind_of_the_row_and_refuse_the_rest() {
    let mut rig = world_with(the_future());
    rig.shows("Morning digest").await;
    // Items in order: Today's reminder, This week's, the routine, the watch.
    rig.press(&[KeyCode::Char('H'), KeyCode::Char('j'), KeyCode::Char('j')])
        .await;
    rig.press(&[KeyCode::Char('z')]).await;
    rig.shows("only a reminder snoozes").await;
    rig.press(&[KeyCode::Char('p')]).await;
    rig.asked("routine.pause", 1).await;
    assert_eq!(
        rig.daemon().asked("routine.pause"),
        [json!({"id": "ses_wak_a41d04", "author": "the TUI"})]
    );
    rig.press(&[KeyCode::Char('r')]).await;
    rig.asked("routine.run_now", 1).await;
    rig.press(&[KeyCode::Char('x')]).await;
    rig.shows("p pauses a routine").await;
    rig.press(&[KeyCode::Char('j'), KeyCode::Char('x')]).await;
    rig.asked("wake.cancel", 1).await;
    assert_eq!(
        rig.daemon().asked("wake.cancel"),
        [json!({"wake": "wak_c3d4e5", "author": "the TUI"})]
    );
    rig.press(&[KeyCode::Char('r')]).await;
    rig.shows("only a routine runs now").await;
    assert!(rig.daemon().asked("wake.snooze").is_empty());
}

/// A paused routine resumes with `p`.
#[tokio::test]
async fn p_on_a_paused_routine_resumes_it() {
    let mut paused = routine("wak_a41d04", "Morning digest", T0 + 20 * HOUR);
    paused.paused = true;
    paused.state = WorkState::Waiting;
    let mut rig = world_with(vec![paused]);
    rig.shows("paused").await;
    rig.press(&[KeyCode::Char('H'), KeyCode::Char('p')]).await;
    rig.asked("routine.resume", 1).await;
}

/// The day is this client's: a reminder tomorrow afternoon is This week, and
/// after the local midnight the same view is Today with no frame between.
#[tokio::test]
async fn a_reminder_moves_from_this_week_to_today_at_the_local_midnight() {
    let mut rig = world_with(vec![reminder(
        "wak_b9dfcc",
        "Send the tide table",
        T0 + 30 * HOUR,
    )]);
    rig.shows("This week").await;
    assert!(!rig.screen().join("\n").contains("Today"));
    // The tests' zone is UTC-7: 07:00 UTC the next day is its midnight.
    NOW.with(|n| n.set(T0 + 17 * HOUR));
    assert!(clock() > T0);
    rig.press(&[KeyCode::Char('?'), KeyCode::Esc]).await;
    rig.shows("Today").await;
    assert!(!rig.screen().join("\n").contains("This week"));
}

/// A frame from the daemon changes the section: a new reminder joins, a view
/// that is not the future's leaves, and an old position changes nothing.
#[tokio::test]
async fn work_changed_updates_the_section_by_the_position_rule() {
    let mut rig = world_with(vec![reminder(
        "wak_a8cebb",
        "Call the harbour master",
        T0 + 3 * HOUR,
    )]);
    rig.shows("Call the harbour master").await;
    let mut later = reminder("wak_d0e1f2", "Water the ferns", T0 + 5 * HOUR);
    later.position = 12;
    rig.daemon().notify(
        "work.changed",
        json!({"position": 12, "at_ms": T0, "views": [later], "gone": []}),
    );
    rig.shows("Water the ferns").await;
    let mut stale = reminder("wak_d0e1f2", "Old words", T0 + 5 * HOUR);
    stale.position = 11;
    rig.daemon().notify(
        "work.changed",
        json!({"position": 13, "at_ms": T0, "views": [stale], "gone": ["wak_a8cebb"]}),
    );
    rig.settle().await;
    let screen = rig.screen().join("\n");
    assert!(screen.contains("Water the ferns") && !screen.contains("Old words"));
    assert!(!screen.contains("Call the harbour master"), "gone leaves");
    let _: Value = json!(null);
}

/// An older daemon that refuses `work.watch` leaves the section empty and the
/// rest of the screen as it was.
#[tokio::test]
async fn a_daemon_without_the_board_leaves_no_section() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "work.watch".into(),
        Err((-32601, "no method work.watch".into())),
    );
    let mut rig = Rig::new(120, 30, script(world));
    rig.shows("DM").await;
    rig.press(&[KeyCode::Char('H')]).await;
    rig.shows("nothing is ahead").await;
}
