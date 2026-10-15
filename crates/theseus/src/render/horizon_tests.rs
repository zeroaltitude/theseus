//! The horizon's words (theseus-nuhsz): every section from seeded views and
//! occurrences, at a fixed clock and zone, against a golden; the sections'
//! rules one at a time; and the day's edges. `THESEUS_GOLDEN=write` rewrites
//! the golden.

use theseus_protocol::work::horizon::{
    AgendaLine, AgendaWhat, Horizon, Month, NextDue, ProjectView, RoutineLast, RoutineView,
    RunCounts, Series, Tally, WatchLast, WatchView,
};
use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::{Attention, Level};

use super::*;

/// 2026-09-21 14:13:20 UTC, which is 07:13 at the tests' UTC−7.
const NOW: u64 = 1_790_000_000_000;
const HOUR: u64 = 3_600_000;

fn view(id: &str, kind: WorkKind, title: &str, state: WorkState, level: Level) -> WorkView {
    WorkView {
        id: id.into(),
        kind,
        position: 10,
        at_ms: NOW,
        parent: Some("exe_home".into()),
        root: "exe_home".into(),
        session_id: Some(format!("ses_{id}")),
        title: title.into(),
        place: None,
        state,
        previous: None,
        attention: Attention {
            level,
            label: match state {
                WorkState::NeedsYou => "failed: the feed was down".into(),
                _ => "ready".into(),
            },
            since_ms: NOW,
        },
        started_at_ms: NOW,
        state_since_ms: NOW,
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

fn routine(id: &str, title: &str, at: u64, state: WorkState) -> WorkView {
    let level = if state == WorkState::NeedsYou {
        Level::NeedsYou
    } else {
        Level::Ready
    };
    let mut v = view(id, WorkKind::Routine, title, state, level);
    v.purpose = Some("routine".into());
    v.next_at_ms = Some(at);
    v.series = Some(Series {
        every: Some("1d".into()),
        days: vec![
            "mon".into(),
            "tue".into(),
            "wed".into(),
            "thu".into(),
            "fri".into(),
        ],
        occurrence: 22,
        missed_last: 2,
        ..Default::default()
    });
    v.routine = Some(RoutineView {
        schedule: "weekdays 08:00".into(),
        last: Some(RoutineLast {
            outcome: "ok".into(),
            at_ms: NOW - 20 * HOUR,
            duration_ms: 41_000,
            cost_usd: 0.12,
            line: Some("two tides today".into()),
        }),
        runs_30d: RunCounts {
            ok: 20,
            failed: 1,
            skipped: 2,
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
        last: Some(WatchLast {
            value: "OPEN".into(),
            at_ms: NOW - HOUR,
        }),
        checks: 611,
        expires_at_ms: Some(NOW + 72 * HOUR),
        failing: None,
    });
    v
}

fn project(id: &str, title: &str) -> WorkView {
    let mut v = view(
        id,
        WorkKind::Project,
        title,
        WorkState::Running,
        Level::Working,
    );
    v.project = Some(ProjectView {
        goal: "Ship the harbour map".into(),
        steps: Tally { done: 3, total: 9 },
        milestones: Tally { done: 1, total: 3 },
        next: Some(NextDue {
            title: "Survey the north quay".into(),
            due_at_ms: Some(NOW + 4 * 86_400_000),
        }),
        overdue: 0,
        review: None,
    });
    v
}

fn line(
    id: &str,
    kind: WorkKind,
    what: AgendaWhat,
    title: &str,
    at: u64,
    n: Option<u32>,
) -> AgendaLine {
    AgendaLine {
        at_ms: at,
        id: id.into(),
        kind,
        what,
        title: title.into(),
        session_id: None,
        purpose: Some(
            match kind {
                WorkKind::Routine => "routine",
                _ => "remind",
            }
            .into(),
        ),
        occurrence: n,
    }
}

fn seeded() -> (Vec<WorkView>, Vec<AgendaLine>) {
    let views = vec![
        reminder("wak_a8cebb", "Call the harbour master", NOW + 3 * HOUR),
        reminder("wak_b9dfcc", "Send the tide table", NOW + 30 * HOUR),
        reminder("wak_e1f2a3", "Renew the mooring", NOW + 12 * 86_400_000),
        routine(
            "wak_a41d04",
            "Morning digest",
            NOW + 20 * HOUR,
            WorkState::Ready,
        ),
        routine(
            "wak_f00d12",
            "Evening haiku",
            NOW + 11 * HOUR,
            WorkState::NeedsYou,
        ),
        watch("wak_c3d4e5", "PR 123 merged?"),
        project("prj_000a1b", "Harbour map"),
    ];
    let lines = vec![
        line(
            "wak_a8cebb",
            WorkKind::Wake,
            AgendaWhat::Due,
            "Call the harbour master",
            NOW + 3 * HOUR,
            None,
        ),
        line(
            "wak_f00d12",
            WorkKind::Routine,
            AgendaWhat::Run,
            "Evening haiku",
            NOW + 11 * HOUR,
            Some(30),
        ),
        line(
            "wak_b9dfcc",
            WorkKind::Wake,
            AgendaWhat::Due,
            "Send the tide table",
            NOW + 30 * HOUR,
            None,
        ),
        line(
            "wak_a41d04",
            WorkKind::Routine,
            AgendaWhat::Run,
            "Morning digest",
            NOW + 20 * HOUR,
            Some(22),
        ),
        line(
            "wak_e1f2a3",
            WorkKind::Wake,
            AgendaWhat::Due,
            "Renew the mooring",
            NOW + 12 * 86_400_000,
            None,
        ),
    ];
    (views, lines)
}

fn text(a: &Agenda) -> String {
    lines(a)
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The whole future, at a fixed clock: the golden.
#[test]
fn the_agenda_of_a_seeded_board_reads_as_the_golden_says() {
    theseus_client_time();
    let (views, lines) = seeded();
    let a = build(&views, &lines, NOW, &local_day(NOW));
    let got = text(&a);
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/horizon_golden.txt");
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, &got).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(got, want, "THESEUS_GOLDEN=write rewrites it");
}

fn theseus_client_time() {
    crate::render::time::pin_for_tests();
}

#[test]
fn each_section_holds_what_its_horizon_says() {
    theseus_client_time();
    let (views, lines) = seeded();
    let a = build(&views, &lines, NOW, &local_day(NOW));
    let in_section = |h: Horizon| -> Vec<String> {
        a.sections
            .iter()
            .find(|s| s.horizon == h)
            .map(|s| s.rows.iter().map(|r| r.id.clone()).collect())
            .unwrap_or_default()
    };
    // The failed routine needs you: Now, never also Recurring.
    assert_eq!(in_section(Horizon::Now), ["wak_f00d12"]);
    assert_eq!(in_section(Horizon::Recurring), ["wak_a41d04"]);
    assert_eq!(in_section(Horizon::World), ["wak_c3d4e5"]);
    assert_eq!(in_section(Horizon::Projects), ["prj_000a1b"]);
    // 10:13 today at UTC−7 is Today; tomorrow 13:13 is This week; 12 days is Later.
    assert!(in_section(Horizon::Today).contains(&"wak_a8cebb".to_string()));
    assert_eq!(
        in_section(Horizon::Week)
            .iter()
            .filter(|i| *i == "wak_b9dfcc")
            .count(),
        1
    );
    assert_eq!(in_section(Horizon::Later), ["wak_e1f2a3"]);
    assert!(a.header.contains("🔁✗ 1 failed"), "{}", a.header);
}

#[test]
fn nothing_ahead_says_so_and_a_paused_series_is_not_a_day_row() {
    theseus_client_time();
    let a = build(&[], &[], NOW, &local_day(NOW));
    assert!(a.sections.is_empty());
    assert!(text(&a).contains("nothing is ahead"));
    // A paused routine shows under Recurring with "paused", and
    // `lines_from_views` gives it no occurrence.
    let mut paused = routine(
        "wak_a41d04",
        "Morning digest",
        NOW + 20 * HOUR,
        WorkState::Waiting,
    );
    paused.paused = true;
    assert!(lines_from_views(std::slice::from_ref(&paused)).is_empty());
    let a = build(&[paused], &[], NOW, &local_day(NOW));
    let row = &a.sections[0].rows[0].text;
    assert!(
        row.contains("paused") && row.contains("weekdays 08:00"),
        "{row}"
    );
}

#[test]
fn views_that_are_not_the_future_are_left_out() {
    theseus_client_time();
    let task = view(
        "exe_x",
        WorkKind::Task,
        "a running task",
        WorkState::Running,
        Level::Working,
    );
    let conv = view(
        "exe_y",
        WorkKind::Conversation,
        "a chat",
        WorkState::Ready,
        Level::Ready,
    );
    let mut met = watch("wak_done", "met long ago");
    met.state = WorkState::Done;
    met.attention.level = Level::Idle;
    let a = build(&[task, conv, met], &[], NOW, &local_day(NOW));
    assert!(a.sections.is_empty(), "{:?}", a.sections);
}

#[test]
fn the_day_is_the_local_one() {
    theseus_client_time();
    // 07:13:20 local at UTC−7: the day ends in 16 h 46 min 40 s.
    let d = local_day(NOW);
    assert_eq!(d.tomorrow_ms, NOW + 16 * HOUR + 46 * 60_000 + 40_000);
    assert_eq!(d.week_end_ms, d.tomorrow_ms + 7 * 86_400_000);
    assert_eq!(short("wak_a8cebb"), "a8cebb");
}
