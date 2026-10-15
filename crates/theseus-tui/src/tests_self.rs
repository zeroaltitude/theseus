//! The self view (theseus-pw1q.19): the pane over a scripted daemon's
//! `self.log`, drawn at two widths against a fixed log, followed live through
//! the events the TUI already holds (never a timer), folded on a narrow
//! screen, and plain about the mode being off.

use std::time::Duration;

use crossterm::event::KeyCode;
use serde_json::{json, Value};

use crate::tests::{changed, harbour_world, opened, script, Rig, T0};

/// The tests' epoch (14:13:20 UTC, 07:13 at the tests' zone), plus `n` minutes.
fn at(n: u64) -> u64 {
    T0 + n * 60_000
}

fn row(
    position: u64,
    minutes: u64,
    kind: &str,
    what: &str,
    numbers: Value,
    undo: Option<&str>,
) -> Value {
    let mut r =
        json!({"position": position, "at_unix_ms": at(minutes), "kind": kind, "what": what});
    if !numbers.is_null() {
        r["numbers"] = numbers;
    }
    if let Some(u) = undo {
        r["undo"] = json!(u);
    }
    r
}

/// Eight rows, newest first.
fn rows() -> Vec<Value> {
    vec![
        row(
            80,
            70,
            "self.joined",
            "self/fern-lamp joined main",
            json!({"cost_usd": 0.42}),
            Some("theseus self veto abc123"),
        ),
        row(
            70,
            60,
            "self.review.verdict",
            "review of self/fern-lamp: pass",
            Value::Null,
            None,
        ),
        row(
            60,
            50,
            "self.branch.built",
            "self/fern-lamp built, gate green",
            json!({"cost_usd": 1.5}),
            None,
        ),
        row(
            50,
            40,
            "self.backlog.started",
            "item tsk_fern started",
            Value::Null,
            None,
        ),
        row(
            40,
            30,
            "self.backlog.ranked",
            "item tsk_fern ranked 1",
            Value::Null,
            None,
        ),
        row(
            30,
            20,
            "self.backlog.filed",
            "item tsk_fern filed",
            Value::Null,
            None,
        ),
        row(
            20,
            10,
            "self.resumed",
            "self-improvement resumed by the owner",
            Value::Null,
            Some("theseus self halt [why]"),
        ),
        row(
            10,
            5,
            "self.halted",
            "self-improvement halted by the CLI",
            Value::Null,
            Some("theseus self resume"),
        ),
    ]
}

fn state(mode: &str, halted: bool) -> Value {
    json!({"mode": mode, "halted": halted, "gate": if mode == "off" { "off" } else { "allowed" },
           "at_ms": at(20), "by": "the owner", "via": "cli"})
}

fn log(mode: &str, rows: Vec<Value>, budget: bool) -> Value {
    let mut l = json!({"rows": rows, "state": state(mode, false), "more": false});
    if budget {
        l["budget"] = json!({"day": "2026-09-21", "day_spent_usd": 1.92, "day_line_usd": 30.0,
            "branch_line_usd": 10.0,
            "branches": [{"branch": "self/fern-lamp", "spent_usd": 1.92, "open_usd": 0.0}]});
    }
    l
}

fn rig(width: u16, height: u16, self_log: Value) -> Rig {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("self.log".into(), Ok(self_log));
    Rig::new(width, height, script(world))
}

async fn open(rig: &mut Rig) {
    rig.asked("health", 1).await;
    rig.settle().await;
    rig.press(&[KeyCode::Char('l')]).await;
    rig.asked("self.log", 1).await;
}

fn pane(rig: &Rig) -> Vec<String> {
    rig.screen()[1..rig.screen().len() - 1].to_vec()
}

#[tokio::test]
async fn the_pane_at_a_wide_width_has_the_state_the_budget_and_the_rows() {
    let mut rig = rig(120, 24, log("act", rows(), true));
    open(&mut rig).await;
    rig.shows("fern-lamp joined main").await;
    let screen = pane(&rig);
    insta_lines(&screen[..14], &[
        " Self-improvement",
        " mode act; the kill switch is released by the owner at 2026-09-21 07:33; no sealed dir set",
        " Budget 2026-09-21: $1.92 of $30.00 spent or reserved today; a branch's line is $10.00.",
        "   self/fern-lamp: $1.92 of $10.00",
        "",
        "▸09-21 08:23 self.joined          self/fern-lamp joined main · $0.42 · undo: theseus self veto abc123",
        " 09-21 08:13 self.review.verdict  review of self/fern-lamp: pass",
        " 09-21 08:03 self.branch.built    self/fern-lamp built, gate green · $1.50",
        " 09-21 07:53 self.backlog.started item tsk_fern started",
        " 09-21 07:43 self.backlog.ranked  item tsk_fern ranked 1",
        " 09-21 07:33 self.backlog.filed   item tsk_fern filed",
        " 09-21 07:23 self.resumed         self-improvement resumed by the owner · undo: theseus self halt [why]",
        " 09-21 07:18 self.halted          self-improvement halted by the CLI · undo: theseus self resume",
    ]);
    let last = rig.screen().last().cloned().unwrap();
    assert!(
        last.contains("[enter] whole row") && last.contains("[esc] close"),
        "{last:?}"
    );
}

/// A fixed text against the screen's lines, with the first mismatch named.
fn insta_lines(got: &[String], want: &[&str]) {
    for (i, w) in want.iter().enumerate() {
        assert_eq!(
            got.get(i).map(String::as_str),
            Some(*w),
            "line {i}:\n{}",
            got.join("\n")
        );
    }
}

#[tokio::test]
async fn a_narrow_screen_folds_to_the_state_and_the_newest_five() {
    let mut rig = rig(70, 24, log("act", rows(), true));
    open(&mut rig).await;
    rig.shows("fern-lamp joined").await;
    let screen = pane(&rig);
    insta_lines(
        &screen[..9],
        &[
            " Self-improvement",
            " mode act; the kill switch is released by the owner at 2026-09-21",
            " 07:33; no sealed dir set",
            "",
            "▸09-21 08:23 self.joined          self/fern-lamp joined main · $0.42…",
            " 09-21 08:13 self.review.verdict  review of self/fern-lamp: pass",
            " 09-21 08:03 self.branch.built    self/fern-lamp built, gate green ·…",
            " 09-21 07:53 self.backlog.started item tsk_fern started",
            " 09-21 07:43 self.backlog.ranked  item tsk_fern ranked 1",
        ],
    );
    let all = screen.join("\n");
    assert!(
        !all.contains("Budget") && !all.contains("tsk_fern filed"),
        "{all}"
    );
    assert!(all.contains("older: theseus self log --since 30d"), "{all}");
}

#[tokio::test]
async fn off_says_so_plainly_and_still_lists_the_rows() {
    let mut rig = rig(120, 24, log("off", rows()[..2].to_vec(), false));
    open(&mut rig).await;
    rig.shows("Self-improvement is off. Nothing has run.").await;
    let all = pane(&rig).join("\n");
    assert!(
        all.contains("mode off: nothing self-directed runs"),
        "{all}"
    );
    assert!(all.contains("self.review.verdict"), "{all}");
    assert!(!all.contains("Budget"), "no budget line while off: {all}");
}

#[tokio::test]
async fn enter_shows_the_picked_row_whole_and_esc_comes_back_then_closes() {
    let mut rig = rig(120, 24, log("act", rows(), true));
    open(&mut rig).await;
    rig.shows("fern-lamp joined main").await;
    rig.press(&[KeyCode::Down, KeyCode::Down, KeyCode::Enter])
        .await;
    let all = pane(&rig).join("\n");
    assert!(
        all.contains("self.branch.built") && all.contains("numbers  {\"cost_usd\":1.5}"),
        "{all}"
    );
    assert!(all.contains("position 60"), "{all}");
    assert!(!all.contains("self.joined"), "only the picked row: {all}");
    rig.press(&[KeyCode::Esc]).await;
    assert!(pane(&rig).join("\n").contains("self.joined"));
    rig.press(&[KeyCode::Esc]).await;
    assert!(
        !rig.screen().join("\n").contains("Self-improvement"),
        "closed"
    );
    assert!(rig.screen().last().unwrap().contains("[l] self"));
}

#[tokio::test]
async fn the_pane_follows_the_log_through_events_it_already_hears() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "self.log".into(),
        Ok(log("act", rows()[1..].to_vec(), true)),
    );
    let mut rig = Rig::new(120, 24, script(world.clone()));
    open(&mut rig).await;
    rig.shows("review of self/fern-lamp: pass").await;
    assert!(!rig.screen().join("\n").contains("fern-lamp joined main"));
    // A row lands, and an execution changes state beside it.
    world
        .lock()
        .unwrap()
        .answers
        .insert("self.log".into(), Ok(log("act", rows(), true)));
    rig.daemon()
        .notify("execution.changed", changed(&opened(200, "ses_selfx1")));
    rig.asked("self.log", 2).await;
    rig.shows("fern-lamp joined main").await;
}

#[tokio::test(start_paused = true)]
async fn nothing_reads_the_log_on_a_timer_and_a_burst_is_two_reads() {
    let mut rig = rig(120, 24, log("act", rows(), true));
    open(&mut rig).await;
    rig.shows("fern-lamp joined main").await;
    rig.settle().await;
    // Ten minutes of nothing on the paused clock, the loop waiting through
    // every deadline the app asks for: no read.
    for _ in 0..10 {
        let _ = tokio::time::timeout(Duration::from_secs(60), rig.runner.step()).await;
    }
    assert_eq!(
        rig.daemon().asked("self.log").len(),
        1,
        "no timer reads the log"
    );
    // A burst while a read is in flight: one more read, not one each.
    let d = rig.daemon();
    for i in 0..5 {
        d.notify(
            "execution.changed",
            changed(&opened(300 + i, &format!("ses_burst{i}"))),
        );
    }
    rig.settle().await;
    let n = d.asked("self.log").len();
    assert!(
        (2..=3).contains(&n),
        "a burst is a read or two, not five: {n}"
    );
    // Closed, events read nothing.
    rig.press(&[KeyCode::Esc]).await;
    d.notify("execution.changed", changed(&opened(400, "ses_after1")));
    rig.settle().await;
    assert_eq!(d.asked("self.log").len(), n, "a closed pane reads nothing");
}

#[tokio::test]
async fn an_older_daemon_without_a_self_log_is_told_in_the_pane() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("self.log".into(), Err((-32601, "unknown method".into())));
    let mut rig = Rig::new(120, 24, script(world));
    open(&mut rig).await;
    rig.shows("self.log: unknown method").await;
}

/// The kill switch moved from another surface: no execution changes beside
/// it, only `self.changed`, and the pane reads the log again.
#[tokio::test]
async fn a_move_of_the_switch_from_another_surface_shows_without_a_key() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "self.log".into(),
        Ok(log("act", rows()[1..].to_vec(), true)),
    );
    let mut rig = Rig::new(120, 24, script(world.clone()));
    open(&mut rig).await;
    rig.shows("review of self/fern-lamp: pass").await;
    world
        .lock()
        .unwrap()
        .answers
        .insert("self.log".into(), Ok(log("act", rows(), true)));
    rig.daemon().notify(
        "self.changed",
        json!({"kind": "self.halted", "at_ms": at(80)}),
    );
    rig.asked("self.log", 2).await;
    rig.shows("fern-lamp joined main").await;
}
