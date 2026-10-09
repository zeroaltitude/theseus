//! The work view's and the notification policy's shapes on the wire
//! (theseus-753z): a whole `WorkView` of each kind and a `Notice`, each
//! compared with its fixture in `tests/wire/work/`, byte for byte, both ways;
//! and an older peer's view, without the optional fields, still decodes.
//! `THESEUS_GOLDEN=write` rewrites the fixtures.

use std::path::PathBuf;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use theseus_protocol::notices::{policy, Notice, Rules};
use theseus_protocol::work::*;
use theseus_protocol::{utc_hm, Attention, Level};

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
        external_text: true,
        choices: Vec::new(),
    };
    question.options = answer_options(&question);
    let mut v = WorkView {
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
        }),
        flagged: true,
    };
    v.attention = attention_of(&v);
    v
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
    assert_eq!((q.tool, q.asked_at_ms, q.external_text), (None, 0, false));
}
