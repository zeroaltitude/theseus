//! The work view's and the notification policy's shapes on the wire
//! (theseus-753z): a whole `WorkView` of each kind and a `Notice`, each
//! compared with its fixture in `tests/wire/work/`, byte for byte, both ways;
//! and an older peer's view, without the optional fields, still decodes.
//! `THESEUS_GOLDEN=write` rewrites the fixtures.

use std::path::PathBuf;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use theseus_protocol::notices::{policy, Notice, Rules};
use theseus_protocol::threads::ThreadState;
use theseus_protocol::work::*;
use theseus_protocol::{utc_hm, Attention, Level, ThreadGroup};

const AT: u64 = 1_790_015_160_000;

fn fixture(name: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "wire",
        "work",
        &format!("{name}.json"),
    ]
    .iter()
    .collect()
}

/// Compare with the fixture both ways, or write it under
/// `THESEUS_GOLDEN=write`.
fn wire<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(name: &str, value: &T) {
    let got = serde_json::to_string(value).unwrap();
    let path = fixture(name);
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{got}\n")).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (write it with THESEUS_GOLDEN=write)",
            path.display()
        )
    });
    let want = want.trim_end_matches('\n');
    assert_eq!(got, want, "{name}: the bytes on the wire changed");
    let back: T = serde_json::from_str(want).unwrap();
    assert_eq!(&back, value, "{name}: decodes as it was");
}

fn r(id: &str, kind: WorkKind, title: &str) -> WorkRef {
    WorkRef {
        id: id.into(),
        kind,
        session_id: Some("ses_q7f3k2".into()),
        title: title.into(),
    }
}

/// A whole view: every optional field set.
fn whole(id: &str, kind: WorkKind, state: WorkState, title: &str) -> WorkView {
    let mut question = Question {
        id: "cor_m4p8z1".into(),
        kind: QuestionKind::Approval,
        asked_by: r("exe_b2c3d4", WorkKind::Task, "Beta summary"),
        prompt: "fs.write create projects/beta-summary.txt (40 B)".into(),
        detail: Some("write under projects/".into()),
        input: Some(json!({"path": "projects/beta-summary.txt", "bytes": 40})),
        options: Vec::new(),
        on_expiry: Some("not run".into()),
        expires_at_ms: Some(AT + 600_000),
        floor: false,
        may_answer: true,
        tool: Some("fs.write".into()),
        asked_at_ms: AT,
        choices: Vec::new(),
    };
    question.options = answer_options(&question);
    let mut v = WorkView {
        moved_to: None,
        id: id.into(),
        kind,
        position: 4_096,
        at_ms: AT,
        parent: Some("exe_a1b2c3".into()),
        root: "exe_a1b2c3".into(),
        session_id: Some("ses_q7f3k2".into()),
        title: title.into(),
        place: Some("cli".into()),
        state,
        previous: Some(WorkState::Running),
        attention: Attention {
            level: Level::Idle,
            label: String::new(),
            since_ms: 0,
        },
        started_at_ms: AT - 840_000,
        state_since_ms: AT - 5_000,
        ended_at_ms: Some(AT),
        doing: Some(Doing {
            what: DoingKind::Tool,
            label: "fs.write".into(),
            since_ms: AT - 5_000,
            turn: 4,
            loop_index: 2,
            last_line: None,
        }),
        progress: Some(Progress {
            steps_done: 2,
            steps_total: 5,
            current: Some("compare the soundings".into()),
            said: Some("Two of five charts agree.".into()),
            at_ms: AT - 1_000,
        }),
        question: Some(question),
        cost: Some(Cost {
            spent_usd: 1.25,
            limit_usd: 10.0,
            turn_usd: Some(0.5),
        }),
        eta: Some(Eta {
            usual_ms: 95_000,
            runs: 12,
        }),
        forecast: None,
        below: Some(Rollup {
            needs_you: 1,
            working: 2,
            done: 3,
            failed: 0,
            total: 6,
        }),
        why: Some("wake".into()),
        report: Some(WorkReport {
            outcome: "failed".into(),
            first_line: Some("Two of five charts agree.".into()),
            reason: Some("the provider refused the request (400): invalid_request_error".into()),
        }),
        woke: Some(Woke {
            note: "the tide turns".into(),
            at_ms: AT - 60_000,
            purpose: None,
        }),
        expect: None,
        flagged: true,
        dismissed: false,
        retry_of: None,
        thread: None,
        purpose: None,
        next_at_ms: None,
        series: None,
        paused: false,
        routine: None,
        watch: None,
        project: None,
        data: None,
    };
    v.attention = attention_of(&v);
    v
}

/// A running job's view carries its program's usual time and its last output
/// line (theseus-e1hu4); an older peer's `doing` without it still decodes.
#[test]
fn a_running_job_says_its_last_line() {
    let mut v = whole(
        "act_d4e5f6",
        WorkKind::Job,
        WorkState::Running,
        "sh -c leg.sh",
    );
    v.doing = Some(Doing {
        what: DoingKind::Job,
        label: "sh -c leg.sh".into(),
        since_ms: AT - 5_000,
        turn: 4,
        loop_index: 2,
        last_line: Some("leg 7 of 30".into()),
    });
    v.attention = attention_of(&v);
    wire("view_job_running", &v);
    let old: Doing = serde_json::from_value(json!({
        "what": "job", "label": "x", "since_ms": 1, "turn": 1, "loop_index": 0
    }))
    .unwrap();
    assert_eq!(old.last_line, None);
}

#[test]
fn a_whole_view_of_each_kind() {
    for (kind, id, state, title) in [
        (
            WorkKind::Conversation,
            "exe_a1b2c3",
            WorkState::Ready,
            "harbour survey",
        ),
        (
            WorkKind::Task,
            "exe_b2c3d4",
            WorkState::Failed,
            "Alpha check",
        ),
        (
            WorkKind::Step,
            "tsk_c3d4e5",
            WorkState::Running,
            "compare the soundings",
        ),
        (
            WorkKind::Job,
            "act_d4e5f6",
            WorkState::Done,
            "cargo test -p core",
        ),
        (
            WorkKind::Question,
            "act_m4p8z1",
            WorkState::NeedsYou,
            "fs.write create projects/beta-summary.txt (40 B)",
        ),
        (
            WorkKind::Wake,
            "wak_e5f6a7",
            WorkState::Waiting,
            "the tide turns",
        ),
        (
            WorkKind::System,
            "exe_f6a7b8",
            WorkState::Queued,
            "index rebuild",
        ),
    ] {
        wire(
            &format!("view_{}", kind.as_str()),
            &whole(id, kind, state, title),
        );
    }
}

#[test]
fn a_notice() {
    let mut prev = whole(
        "exe_b2c3d4",
        WorkKind::Task,
        WorkState::Running,
        "Beta summary",
    );
    prev.question = None;
    let next = whole(
        "exe_b2c3d4",
        WorkKind::Task,
        WorkState::NeedsYou,
        "Beta summary",
    );
    let n = policy(Some(&prev), &next, &Rules::default(), &utc_hm).expect("it asks");
    wire::<Notice>("notice_asked", &n);
}

/// An older peer's view, with only the identity fields, decodes with every
/// optional field absent, and writes the same bytes again.
#[test]
fn an_older_peers_view_decodes() {
    let line = r#"{"id":"exe_a1b2c3","kind":"conversation","position":7,"at_ms":1790015160000,"root":"exe_a1b2c3","title":"","state":"ready","attention":{"level":"ready","label":"ready","since_ms":0},"started_at_ms":0,"state_since_ms":0}"#;
    let v: WorkView = serde_json::from_str(line).unwrap();
    assert_eq!(
        (
            v.parent.as_ref(),
            v.report.as_ref(),
            v.woke.as_ref(),
            v.flagged
        ),
        (None, None, None, false)
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), line);
    let state: WorkState = serde_json::from_str(r#""paused_for_jev""#).unwrap();
    assert_eq!(state, WorkState::Unknown, "a newer daemon's state");
    let q: Question = serde_json::from_value(json!({"id": "cor_1", "kind": "budget",
        "asked_by": {"id": "exe_1", "kind": "task"}, "prompt": "budget.reset", "options": [],
        "floor": false, "may_answer": false}))
    .unwrap();
    assert_eq!((q.tool, q.asked_at_ms), (None, 0));
}

// ------------------------------------------------------------- work.watch

/// `work.watch`'s params, its snapshot, and `work.changed` (theseus-hkx30),
/// each compared with its fixture; the notification names are the table's.
#[test]
fn the_work_watch_shapes() {
    use theseus_protocol::{notify, Event, WorkChanged, WorkWatchParams, WorkWatchResult};
    wire(
        "watch_params",
        &WorkWatchParams {
            limit: Some(20),
            roots: Some(vec!["ses_q7f3k2".into()]),
            interactive: Some(true),
        },
    );
    let conversation = whole(
        "exe_a1b2c3",
        WorkKind::Conversation,
        WorkState::Running,
        "harbour survey",
    );
    let task = whole(
        "exe_b2c3d4",
        WorkKind::Task,
        WorkState::NeedsYou,
        "Alpha check",
    );
    wire(
        "watch_result",
        &WorkWatchResult {
            position: 4_096,
            views: vec![conversation.clone(), task.clone()],
            total: 12,
            threads: vec![],
        },
    );
    // A thread's work carries its thread, and the board its groups.
    let mut threaded = conversation;
    threaded.thread = Some("pr-round".into());
    let mut threaded_task = task;
    threaded_task.thread = Some("pr-round".into());
    wire(
        "watch_result_threads",
        &WorkWatchResult {
            position: 4_096,
            views: vec![threaded, threaded_task],
            total: 12,
            threads: vec![ThreadGroup {
                slug: "pr-round".into(),
                title: "PR round".into(),
                state: ThreadState::Active,
                persona: Some("maren".into()),
                sealed: true,
                seal_pending: Some("the harbour notes are private".into()),
                session_id: Some("ses_q7f3k2".into()),
                last_active_ms: AT,
                context_tokens: Some(41_000),
                today_usd: Some(1.2),
            }],
        },
    );
    let job = whole(
        "act_d4e5f6",
        WorkKind::Job,
        WorkState::Done,
        "cargo test -p core",
    );
    let changed = WorkChanged {
        position: 4_097,
        at_ms: AT,
        views: vec![job],
        gone: vec!["act_a0b1c2".into()],
        threads: vec![],
    };
    wire("changed", &changed);
    assert_eq!(
        Event::WorkChanged(changed).notification().method,
        notify::WORK_CHANGED
    );
    let mut prev = whole(
        "exe_b2c3d4",
        WorkKind::Task,
        WorkState::Running,
        "Beta summary",
    );
    prev.question = None;
    let next = whole(
        "exe_b2c3d4",
        WorkKind::Task,
        WorkState::NeedsYou,
        "Beta summary",
    );
    let n = policy(Some(&prev), &next, &Rules::default(), &utc_hm).expect("it asks");
    assert_eq!(
        Event::WorkNotice(n).notification().method,
        notify::WORK_NOTICE
    );
}

/// A watch with no params watches everything, and a snapshot from a peer
/// that sends no `gone` decodes.
#[test]
fn an_empty_watch_and_a_changed_without_gone_decode() {
    use theseus_protocol::{WorkChanged, WorkWatchParams};
    let p: WorkWatchParams = serde_json::from_str("{}").unwrap();
    assert_eq!(p, WorkWatchParams::default());
    assert_eq!(serde_json::to_string(&p).unwrap(), "{}");
    let c: WorkChanged = serde_json::from_str(r#"{"position":5,"at_ms":9,"views":[]}"#).unwrap();
    assert!(c.gone.is_empty());
    assert_eq!(
        serde_json::to_string(&c).unwrap(),
        r#"{"position":5,"at_ms":9,"views":[]}"#
    );
}

// ------------------------------------------------------------ the future

/// A routine, a watch and a project as the board sends them, and `work.agenda`'s
/// params and answer (theseus-nuhsz), each compared with its fixture.
#[test]
fn the_future_shapes() {
    use theseus_protocol::work::horizon::*;
    let mut routine = whole(
        "wak_a41d04",
        WorkKind::Routine,
        WorkState::Ready,
        "Morning digest",
    );
    routine.purpose = Some("routine".into());
    routine.next_at_ms = Some(AT + 3_600_000);
    routine.series = Some(Series {
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
    routine.routine = Some(RoutineView {
        schedule: "weekdays 08:00".into(),
        last: Some(RoutineLast {
            outcome: "ok".into(),
            at_ms: AT - 600_000,
            duration_ms: 41_000,
            cost_usd: 0.12,
            line: Some("Two tides today.".into()),
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
    wire("view_routine", &routine);
    let mut watch = whole(
        "wak_c3d4e5",
        WorkKind::Watch,
        WorkState::Waiting,
        "PR 123 merged?",
    );
    watch.purpose = Some("watch".into());
    watch.watch = Some(WatchView {
        check: "gh pr view 123".into(),
        until: "equals MERGED".into(),
        last: Some(WatchLast {
            value: "OPEN".into(),
            at_ms: AT - 60_000,
        }),
        checks: 611,
        expires_at_ms: Some(AT + 72 * 3_600_000),
        failing: None,
    });
    wire("view_watch", &watch);
    let mut project = whole(
        "prj_000a1b",
        WorkKind::Project,
        WorkState::Running,
        "Harbour map",
    );
    project.project = Some(ProjectView {
        goal: "Ship the harbour map".into(),
        steps: Tally { done: 3, total: 9 },
        milestones: Tally { done: 1, total: 3 },
        next: Some(NextDue {
            title: "Survey the north quay".into(),
            due_at_ms: Some(AT + 86_400_000),
        }),
        overdue: 0,
        review: None,
    });
    wire("view_project", &project);
}

/// `work.agenda`'s params and answer, the horizon's name, and `status.short`'s
/// two new facts.
#[test]
fn the_agenda_shapes() {
    use theseus_protocol::work::horizon::*;
    wire(
        "agenda_params",
        &AgendaParams {
            from_ms: AT,
            to_ms: AT + 7 * 86_400_000,
            roots: None,
        },
    );
    wire(
        "agenda_result",
        &AgendaResult {
            from_ms: AT,
            to_ms: AT + 7 * 86_400_000,
            lines: vec![AgendaLine {
                at_ms: AT + 3_600_000,
                id: "wak_a41d04".into(),
                kind: WorkKind::Routine,
                what: AgendaWhat::Run,
                title: "Morning digest".into(),
                session_id: Some("ses_q7f3k2".into()),
                purpose: Some("routine".into()),
                occurrence: Some(22),
            }],
            more: false,
        },
    );
    // The horizon names and the status line's two new facts.
    wire("horizon", &Horizon::Week);
    wire(
        "status_short",
        &theseus_protocol::StatusShortResult {
            position: 9,
            needs_you: 1,
            working: 2,
            failed: vec![],
            first: None,
            next_due_ms: Some(AT + 60_000),
            routines_failed: 1,
        },
    );
}
