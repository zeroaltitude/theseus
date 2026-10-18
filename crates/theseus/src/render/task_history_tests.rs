use serde_json::json;
use theseus_protocol::tasks::{TaskOrigin, TaskRecord, TaskState};
use theseus_protocol::{TaskDetail, TaskInfo, TaskListResult};

use super::*;

/// 2026-10-17 12:00 at UTC−7 (the tests' zone).
const NOW: u64 = 1_792_263_600_000;
const HOUR: u64 = 3_600_000;

fn info(short: &str, title: &str, state: &str, ended_ago: u64, spent: f64) -> TaskInfo {
    serde_json::from_value(json!({
        "task_id": format!("ses_00000000000000{short}"), "short": short,
        "execution_id": format!("exe_{short}"), "parent_session_id": "ses_home",
        "parent_execution_id": "exe_home", "title": title, "state": state,
        "spent_usd": spent, "limit_usd": 2.0, "cost_usd": spent, "turns": 3,
        "created_at_ms": NOW - ended_ago - 30 * 60_000, "updated_at_ms": NOW - ended_ago,
        "target": "discord:dm:42",
    }))
    .unwrap()
}

fn line(t: &TaskInfo, report: &str) -> TaskDetail {
    TaskDetail {
        task_id: t.task_id.clone(),
        report_line: Some(report.into()),
        ..Default::default()
    }
}

fn three() -> TaskListResult {
    let a = info("a1b2c3", "Paint the quay", "complete", HOUR / 2, 0.4);
    let b = info("b2c3d4", "Sound the channel", "failed", 3 * HOUR, 1.25);
    let c = info(
        "c3d4e5",
        "Chart the buoys",
        "cancelled",
        3 * 86_400_000,
        0.0,
    );
    let details = vec![
        line(&a, "Painted the quay railings green."),
        line(&b, "the model was unavailable"),
        line(&c, "stopped by you"),
    ];
    TaskListResult {
        tasks: vec![a, b, c],
        records: vec![],
        details,
    }
}

#[test]
fn the_list_is_a_line_a_task_with_its_end_its_money_and_its_report() {
    let l = lines(&three(), NOW, 100, "ended tasks");
    assert_eq!(
        l,
        [
            "✓ a1b2c3  Paint the quay     done 30m ago        $0.40  Painted the quay railings green.",
            "✗ b2c3d4  Sound the channel  failed 3h00m ago    $1.25  the model was unavailable",
            "○ c3d4e5  Chart the buoys    stopped 2026-10-14     $0  stopped by you",
        ]
    );
}

#[test]
fn the_report_gives_way_to_the_width() {
    let l = lines(&three(), NOW, 80, "ended tasks");
    assert!(l[0].ends_with('…'), "{}", l[0]);
    assert!(l[0].chars().count() <= 80, "{}", l[0]);
    // Too narrow for a useful report: the line stops at the money.
    let l = lines(&three(), NOW, 60, "ended tasks");
    assert!(l[0].ends_with("$0.40"), "{}", l[0]);
}

#[test]
fn an_empty_answer_says_so_in_words() {
    let none = TaskListResult {
        tasks: vec![],
        records: vec![],
        details: vec![],
    };
    assert_eq!(lines(&none, NOW, 80, "ended tasks"), ["no ended tasks"]);
    assert_eq!(lines(&none, NOW, 80, "tasks"), ["no tasks"]);
}

#[test]
fn nothing_open_says_what_ended_today() {
    assert_eq!(nothing_open(0), "no tasks");
    assert_eq!(
        nothing_open(3),
        "no open tasks · 3 ended today: theseus tasks --ended"
    );
}

#[test]
fn since_reads_an_age_or_a_day() {
    assert_eq!(since_ms("24h", NOW).unwrap(), NOW - 24 * HOUR);
    assert_eq!(since_ms("7d", NOW).unwrap(), NOW - 7 * 86_400_000);
    assert_eq!(since_ms("2w", NOW).unwrap(), NOW - 14 * 86_400_000);
    assert_eq!(since_ms("30m", NOW).unwrap(), NOW - 30 * 60_000);
    // 2026-10-15 at UTC−7 midnight is 07:00 UTC.
    assert_eq!(since_ms("2026-10-15", NOW).unwrap(), 1_792_047_600_000);
    assert_eq!(since_ms("2026-10-17", NOW).unwrap(), NOW - 12 * HOUR);
    assert!(since_ms("soon", NOW).is_err());
    assert!(since_ms("2027-01-01", NOW)
        .unwrap_err()
        .to_string()
        .contains("future"));
}

fn record(id: &str, title: &str, state: TaskState, session: Option<&str>) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        title: title.into(),
        state,
        session: session.map(Into::into),
        origin: TaskOrigin::default(),
        ..Default::default()
    }
}

fn one(state: &str, pending: u32, paused: bool, report: Option<&str>) -> TaskListResult {
    let mut t = info("a1b2c3", "Paint the quay", state, HOUR, 0.4);
    t.pending_confirms = pending;
    let mut rec = record(
        "tsk_a1b2c3",
        "Paint the quay",
        TaskState::InProgress,
        Some(&t.task_id),
    );
    rec.objective = "Paint the railings along the quay.\nTwo coats.".into();
    let d = TaskDetail {
        task_id: t.task_id.clone(),
        report_line: report.map(|r| r.lines().next().unwrap().into()),
        report: report.map(Into::into),
        paused,
    };
    TaskListResult {
        tasks: vec![t],
        records: vec![
            rec,
            record("tsk_step001", "Scrape", TaskState::Done, None),
            record(
                "tsk_d4e5f6",
                "Buy paint",
                TaskState::InProgress,
                Some("ses_kid"),
            ),
        ],
        details: vec![d],
    }
}

fn text(r: &TaskListResult) -> String {
    show(r).unwrap().join("\n")
}

#[test]
fn show_of_a_running_task_has_its_commands_and_no_report() {
    let s = text(&one("running", 0, false, None));
    for want in [
        "◐ Paint the quay  ·  running",
        "reports   discord:dm:42",
        "spent     $0.40 of $2",
        "started   2026-10-17",
        "Brief\n  Paint the railings along the quay.\n  Two coats.",
        "Plan\n  ✓ Scrape",
        "Children\n  ◐ ",
        "theseus chat --task a1b2c3",
        "theseus task pause a1b2c3",
        "theseus cancel a1b2c3",
    ] {
        assert!(s.contains(want), "missing {want:?} in:\n{s}");
    }
    assert!(!s.contains("Report") && !s.contains("ended "), "{s}");
}

#[test]
fn show_of_an_asking_task_says_how_to_answer() {
    let s = text(&one("running", 1, false, None));
    assert!(s.contains("theseus answer ID …"), "{s}");
}

#[test]
fn show_of_a_paused_task_offers_resume() {
    let s = text(&one("running", 0, true, None));
    assert!(
        s.contains("·  paused") && s.contains("theseus task resume a1b2c3"),
        "{s}"
    );
    assert!(!s.contains("task pause"), "{s}");
}

#[test]
fn show_of_an_ended_task_has_the_whole_report_and_no_stop_command() {
    let s = text(&one(
        "complete",
        0,
        false,
        Some("Painted it.\nTwo coats, green."),
    ));
    for want in [
        "✓ Paint the quay  ·  done",
        "ended     2026-10-17",
        "Report\n  Painted it.\n  Two coats, green.",
        "theseus chat --task a1b2c3",
    ] {
        assert!(s.contains(want), "missing {want:?} in:\n{s}");
    }
    assert!(
        !s.contains("theseus cancel") && !s.contains("task pause"),
        "{s}"
    );
}

#[test]
fn show_of_a_plan_item_is_refused_in_words() {
    let r = TaskListResult {
        tasks: vec![],
        records: vec![record("tsk_step001", "Scrape", TaskState::Done, None)],
        details: vec![],
    };
    assert!(show(&r).unwrap_err().to_string().contains("plan item"));
}
