//! `horizon_of` over every kind and state, with times around midnight and the
//! week's end; the seam's bytes; the answers a new kind offers.

use super::*;
use crate::work::{Doing, DoingKind, WorkView};
use crate::{Attention, Level};
use serde_json::{json, Value};

const HOUR: u64 = 3_600_000;
const DAY: u64 = 24 * HOUR;
/// A local midnight (the client's), then the day built on it.
const MIDNIGHT: u64 = 1_790_000_000_000 - 1_790_000_000_000 % DAY;

fn day() -> LocalDay {
    LocalDay {
        tomorrow_ms: MIDNIGHT + DAY,
        week_end_ms: MIDNIGHT + 8 * DAY,
    }
}

fn view(kind: WorkKind, state: WorkState) -> WorkView {
    WorkView {
        moved_to: None,
        id: "wak_00a1b2c3".into(),
        kind,
        position: 1,
        at_ms: MIDNIGHT,
        parent: None,
        root: "wak_00a1b2c3".into(),
        session_id: None,
        title: "Tide notes".into(),
        place: None,
        state,
        previous: None,
        attention: Attention {
            level: match state {
                WorkState::Failed | WorkState::NeedsYou => Level::NeedsYou,
                WorkState::Ready => Level::Ready,
                s if s.ended() => Level::Idle,
                _ => Level::Working,
            },
            label: "x".into(),
            since_ms: MIDNIGHT,
        },
        started_at_ms: 0,
        state_since_ms: MIDNIGHT,
        ended_at_ms: state.ended().then_some(MIDNIGHT),
        doing: None,
        progress: None,
        question: None,
        cost: None,
        eta: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        expect: None,
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
        data: None,
    }
}

fn dated(kind: WorkKind, at: u64) -> WorkView {
    let mut v = view(kind, WorkState::Waiting);
    v.next_at_ms = Some(at);
    v
}

fn series() -> Series {
    Series {
        every: Some("1d".into()),
        occurrence: 3,
        ..Default::default()
    }
}

#[test]
fn a_dated_view_moves_from_week_to_today_at_the_clients_midnight() {
    let d = day();
    let at = MIDNIGHT + DAY + 9 * HOUR;
    let v = dated(WorkKind::Wake, at);
    // The evening before: tomorrow 09:00 is This week.
    assert_eq!(
        horizon_of(&v, MIDNIGHT + 23 * HOUR + 59 * 60_000, &d),
        Horizon::Week
    );
    assert_eq!(horizon_of(&v, MIDNIGHT + 23 * HOUR, &d), Horizon::Week);
    // After midnight the client passes the next day, and the same view is
    // Today, with no frame in between.
    let next = LocalDay {
        tomorrow_ms: d.tomorrow_ms + DAY,
        week_end_ms: d.week_end_ms + DAY,
    };
    assert_eq!(horizon_of(&v, MIDNIGHT + DAY, &next), Horizon::Today);
}

#[test]
fn the_edges_of_today_and_the_week_are_exact() {
    let d = day();
    let now = MIDNIGHT + 10 * HOUR;
    let at = |ms: u64| horizon_of(&dated(WorkKind::Wake, ms), now, &d);
    assert_eq!(at(now), Horizon::Now, "due this instant is overdue");
    assert_eq!(at(now - 1), Horizon::Now);
    assert_eq!(at(now + 1), Horizon::Today);
    assert_eq!(at(d.tomorrow_ms - 1), Horizon::Today);
    assert_eq!(at(d.tomorrow_ms), Horizon::Week);
    assert_eq!(at(d.week_end_ms - 1), Horizon::Week);
    assert_eq!(at(d.week_end_ms), Horizon::Later);
}

#[test]
fn every_kind_and_state_has_a_horizon() {
    let d = day();
    let now = MIDNIGHT + 10 * HOUR;
    let kinds = [
        WorkKind::Conversation,
        WorkKind::Task,
        WorkKind::Step,
        WorkKind::Job,
        WorkKind::Question,
        WorkKind::Wake,
        WorkKind::System,
        WorkKind::Routine,
        WorkKind::Watch,
        WorkKind::Project,
    ];
    let states = [
        WorkState::Queued,
        WorkState::Running,
        WorkState::Waiting,
        WorkState::NeedsYou,
        WorkState::Ready,
        WorkState::Done,
        WorkState::Failed,
        WorkState::Cancelled,
        WorkState::Expired,
        WorkState::Unknown,
    ];
    for kind in kinds {
        for state in states {
            for next in [
                None,
                Some(now + 5 * HOUR),
                Some(now + 3 * DAY),
                Some(now + 30 * DAY),
            ] {
                let mut v = view(kind, state);
                v.next_at_ms = next;
                let h = horizon_of(&v, now, &d);
                if kind == WorkKind::Project {
                    assert_eq!(h, Horizon::Projects, "{kind:?} {state:?}");
                } else if state.ended() && state != WorkState::Failed {
                    assert_eq!(
                        h,
                        Horizon::Later,
                        "{kind:?} {state:?}: an ended thing is not ahead"
                    );
                } else if matches!(state, WorkState::Failed | WorkState::NeedsYou) {
                    assert_eq!(h, Horizon::Now, "{kind:?} {state:?}: it needs you");
                } else if kind == WorkKind::Watch {
                    assert_eq!(h, Horizon::World, "{kind:?} {state:?}");
                } else if matches!(state, WorkState::Running | WorkState::Queued) {
                    assert_eq!(h, Horizon::Now, "{kind:?} {state:?}");
                } else if kind == WorkKind::Routine {
                    assert_eq!(h, Horizon::Recurring, "{kind:?} {state:?}");
                }
            }
        }
    }
}

#[test]
fn a_series_is_recurring_and_a_running_routine_is_now() {
    let d = day();
    let now = MIDNIGHT + 10 * HOUR;
    let mut v = dated(WorkKind::Wake, now + HOUR);
    v.series = Some(series());
    v.purpose = Some("remind".into());
    assert_eq!(horizon_of(&v, now, &d), Horizon::Recurring);
    let mut r = view(WorkKind::Routine, WorkState::Ready);
    r.next_at_ms = Some(now + HOUR);
    assert_eq!(horizon_of(&r, now, &d), Horizon::Recurring);
    r.state = WorkState::Running;
    r.attention.level = Level::Working;
    assert_eq!(horizon_of(&r, now, &d), Horizon::Now);
    r.state = WorkState::NeedsYou;
    r.attention.level = Level::NeedsYou;
    assert_eq!(
        horizon_of(&r, now, &d),
        Horizon::Now,
        "a failed run needs you"
    );
    // Paused: waiting, still recurring.
    let mut p = view(WorkKind::Routine, WorkState::Waiting);
    p.paused = true;
    assert_eq!(horizon_of(&p, now, &d), Horizon::Recurring);
}

#[test]
fn a_watch_by_purpose_waits_on_the_world_and_a_met_one_is_over() {
    let d = day();
    let now = MIDNIGHT + 10 * HOUR;
    let mut w = dated(WorkKind::Wake, now + HOUR);
    w.purpose = Some("watch".into());
    assert_eq!(horizon_of(&w, now, &d), Horizon::World);
    let met = view(WorkKind::Watch, WorkState::Done);
    assert_eq!(horizon_of(&met, now, &d), Horizon::Later);
    let expired = view(WorkKind::Watch, WorkState::Expired);
    assert_eq!(horizon_of(&expired, now, &d), Horizon::Later);
}

#[test]
fn a_wake_with_no_time_is_later_and_other_work_with_none_is_now() {
    let d = day();
    let now = MIDNIGHT + 10 * HOUR;
    assert_eq!(
        horizon_of(&view(WorkKind::Wake, WorkState::Waiting), now, &d),
        Horizon::Later
    );
    let mut t = view(WorkKind::Task, WorkState::Waiting);
    t.doing = Some(Doing {
        what: DoingKind::Job,
        label: "cargo test".into(),
        since_ms: now,
        turn: 1,
        loop_index: 0,
        last_line: None,
    });
    assert_eq!(horizon_of(&t, now, &d), Horizon::Now);
    // A deferred task asleep until Friday is dated.
    t.next_at_ms = Some(now + 3 * DAY);
    t.doing = None;
    assert_eq!(horizon_of(&t, now, &d), Horizon::Week);
}

#[test]
fn the_new_fields_are_absent_from_the_bytes_when_unset_and_old_bytes_decode() {
    let v = view(WorkKind::Wake, WorkState::Waiting);
    let json = serde_json::to_value(&v).unwrap();
    for k in [
        "purpose",
        "next_at_ms",
        "series",
        "paused",
        "routine",
        "watch",
        "project",
    ] {
        assert!(json.get(k).is_none(), "{k} is left out at its default");
    }
    let back: WorkView = serde_json::from_value(json).unwrap();
    assert_eq!(back, v);
    let mut full = view(WorkKind::Routine, WorkState::Ready);
    full.purpose = Some("routine".into());
    full.next_at_ms = Some(MIDNIGHT + DAY);
    full.series = Some(Series {
        every: Some("1d".into()),
        days: vec!["mon".into(), "fri".into()],
        until_ms: Some(MIDNIGHT + 30 * DAY),
        occurrence: 4,
        missed_last: 2,
        cron: None,
    });
    full.paused = true;
    full.routine = Some(RoutineView {
        schedule: "weekdays 08:00".into(),
        last: Some(RoutineLast {
            outcome: "ok".into(),
            at_ms: MIDNIGHT,
            duration_ms: 41_000,
            cost_usd: 0.12,
            line: Some("two tides".into()),
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
    let again: WorkView = serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
    assert_eq!(again, full);
}

#[test]
fn the_new_kinds_answers_name_methods_and_a_reminders_stay_four() {
    let ids = |a: Vec<crate::wakes::WakeAnswer>| a.into_iter().map(|a| a.id).collect::<Vec<_>>();
    assert_eq!(
        ids(routine_failed_answers()),
        ["run_now", "pause", "details"]
    );
    assert_eq!(ids(watch_met_answers()), ["done", "keep"]);
    assert_eq!(ids(milestone_answers()), ["open", "move"]);
    assert_eq!(crate::wakes::reminder_answers().len(), 4);
    assert_eq!(
        routine_failed_answers()[0].method,
        crate::method::ROUTINE_RUN_NOW
    );
    assert_eq!(watch_met_answers()[0].method, crate::method::WAKE_DONE);
    // "Open" reads the project; the overdue milestone has no task.get any more.
    assert_eq!(milestone_answers()[0].method, crate::method::PROJECT_SHOW);
}

#[test]
fn the_horizons_are_seven_in_order() {
    let names: Vec<&str> = Horizon::ALL.iter().map(|h| h.as_str()).collect();
    assert_eq!(
        names,
        [
            "now",
            "today",
            "week",
            "later",
            "recurring",
            "world",
            "projects"
        ]
    );
    assert!(Horizon::ALL.windows(2).all(|w| w[0] < w[1]));
}

fn hm(ms: u64) -> String {
    crate::utc_hm(ms)
}

/// A routine whose run failed interrupts once, in the failure's words, and
/// its notice offers run now, pause and details; a met watch's notice offers
/// done and keep watching; a reminder's stay four; a quiet transition has
/// none.
#[test]
fn the_routines_failure_and_the_watchs_end_ring_with_their_answers() {
    use crate::notices::{answers_for, policy, reminder_notice, Rules, Urgency, Why};
    let mut ready = view(WorkKind::Routine, WorkState::Ready);
    ready.title = "Morning digest".into();
    let mut failed = ready.clone();
    failed.state = WorkState::NeedsYou;
    failed.attention = Attention {
        level: Level::NeedsYou,
        label: "failed: the feed was down".into(),
        since_ms: MIDNIGHT + 1,
    };
    failed.report = Some(crate::work::WorkReport {
        outcome: "failed".into(),
        first_line: None,
        reason: Some("the feed was down".into()),
    });
    failed.position = 2;
    let n = policy(Some(&ready), &failed, &Rules::default(), &hm).expect("a notice");
    assert_eq!((n.urgency, n.why), (Urgency::Interrupt, Why::Blocked));
    assert!(
        n.line.contains("Morning digest") && n.line.contains("the feed was down"),
        "{}",
        n.line
    );
    let ids = |v: Vec<crate::wakes::WakeAnswer>| v.into_iter().map(|a| a.id).collect::<Vec<_>>();
    assert_eq!(
        ids(answers_for(&failed, n.why)),
        ["run_now", "pause", "details"]
    );
    // A routine that goes from running to ready says nothing, and has none.
    let mut running = ready.clone();
    running.state = WorkState::Running;
    running.attention.level = Level::Working;
    assert!(policy(Some(&running), &ready, &Rules::default(), &hm).is_none());
    assert!(answers_for(&ready, Why::Finished).is_empty());
    // A met watch's wake.noticed.
    let noticed = |purpose: &str| crate::WakeNoticed {
        wake_id: "wak_00c3d4e5".into(),
        short: "c3d4e5".into(),
        session_id: "ses_home".into(),
        execution_id: "exe_home".into(),
        purpose: purpose.into(),
        note: "👁 PR 123 merged? → MERGED".into(),
        due_at_ms: MIDNIGHT,
        late_ms: 0,
        while_down: false,
        every: None,
        occurrence: None,
        posted_to: None,
        home: true,
        answers: vec![],
    };
    let met = reminder_notice(&noticed("watch.met"), &hm);
    assert_eq!(met.urgency, Urgency::Interrupt);
    assert_eq!(ids(met.answers), ["done", "keep"]);
    assert!(reminder_notice(&noticed("watch.expired"), &hm)
        .answers
        .is_empty());
    assert_eq!(reminder_notice(&noticed("remind"), &hm).answers.len(), 4);
}

/// The digest's lines from rows already written: a reminder's firing, late
/// and after downtime, a routine's firing that missed some, a skip and why, a
/// watch met, given up and failing; other rows say nothing.
#[test]
fn the_digest_tells_the_futures_rows() {
    let line = |k: &str, d: Value| digest_line(k, &d, "Morning digest", &hm);
    assert_eq!(
        line(
            "wake.fired",
            json!({"due_at_ms": 1_790_000_000_000u64, "late_ms": 0})
        )
        .unwrap(),
        "⏰ Morning digest fired at 14:13"
    );
    assert_eq!(
        line(
            "wake.fired",
            json!({"due_at_ms": 1_790_000_000_000u64, "late_ms": 7_500_000, "while_down": true})
        )
        .unwrap(),
        "⏰ Morning digest fired at 14:13 (2 h 5 min late, the daemon was down)"
    );
    assert_eq!(
        line(
            "wake.fired",
            json!({"due_at_ms": 1_790_000_000_000u64, "purpose": "routine", "missed": 2})
        )
        .unwrap(),
        "🔁 Morning digest ran at 14:13 · 2 missed while the daemon was down"
    );
    assert_eq!(
        line(
            "wake.skipped",
            json!({"due_at_ms": 1_790_000_000_000u64, "why": "budget"})
        )
        .unwrap(),
        "🔁 Morning digest skipped at 14:13: the month's budget was spent"
    );
    assert_eq!(
        line(
            "wake.skipped",
            json!({"due_at_ms": 1_790_000_000_000u64, "why": "owner"})
        )
        .unwrap(),
        "🔁 Morning digest skipped at 14:13: the owner declined it"
    );
    assert_eq!(
        line("watch.met", json!({"value": "MERGED"})).unwrap(),
        "👁 Morning digest met: MERGED"
    );
    assert_eq!(
        line("watch.expired", json!({"checks": 611})).unwrap(),
        "👁 Morning digest gave up after 611 checks"
    );
    assert_eq!(
        line("watch.failing", json!({"error": "no route to host"})).unwrap(),
        "👁 Morning digest is failing: no route to host"
    );
    assert!(line("turn.ended", json!({})).is_none());
}

/// A mail watch's board line is worded once: how long it has waited, and the
/// follow-up's day when it has one.
#[test]
fn a_mail_watchs_wait_is_worded_once() {
    assert_eq!(
        reply_wait("Dana replies?", 2 * DAY + 3 * HOUR, Some("Fri")),
        "waiting: reply on 'Dana replies?' (2 d, follow-up Fri)"
    );
    assert_eq!(
        reply_wait(" Dana replies? ", 3 * HOUR, None),
        "waiting: reply on 'Dana replies?' (3 h)"
    );
    assert_eq!(
        reply_wait("Dana replies?", 90_000, None),
        "waiting: reply on 'Dana replies?' (1 min)"
    );
}
