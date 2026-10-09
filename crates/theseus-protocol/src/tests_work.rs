//! The work view's pure functions: `attention_of` over every kind, state, and
//! question; `answer_options` for each kind of question; and
//! `WorkView::from_execution` for each state and wait.

use super::*;
use crate::{utc_hm, PendingConfirm};

const AT: u64 = 1_790_000_000_000;

fn view(kind: WorkKind, state: WorkState, question: Option<Question>) -> WorkView {
    WorkView {
        id: "exe_0000a1b2c3".into(),
        kind,
        position: 7,
        at_ms: AT,
        parent: None,
        root: "exe_0000a1b2c3".into(),
        session_id: Some("ses_0000a1b2c3".into()),
        title: "Tide notes".into(),
        place: None,
        state,
        previous: None,
        attention: Attention {
            level: Level::Idle,
            label: String::new(),
            since_ms: 0,
        },
        started_at_ms: AT - 60_000,
        state_since_ms: AT - 1_000,
        ended_at_ms: None,
        doing: None,
        progress: None,
        question,
        cost: Some(Cost {
            spent_usd: 10.02,
            limit_usd: 10.0,
            turn_usd: None,
        }),
        eta: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        flagged: false,
    }
}

fn question(kind: QuestionKind) -> Question {
    let mut q = Question {
        id: "cor_q1".into(),
        kind,
        asked_by: WorkRef {
            id: "exe_0000a1b2c3".into(),
            kind: WorkKind::Task,
            session_id: None,
            title: "Beta summary".into(),
        },
        prompt: "proc.run run cargo test".into(),
        detail: None,
        input: None,
        options: Vec::new(),
        on_expiry: Some("not run".into()),
        expires_at_ms: Some(AT + 600_000),
        floor: false,
        may_answer: true,
        tool: Some("proc.run".into()),
        asked_at_ms: AT,
        external_text: false,
        choices: Vec::new(),
    };
    q.options = answer_options(&q);
    q
}

const KINDS: [WorkKind; 7] = [
    WorkKind::Conversation,
    WorkKind::Task,
    WorkKind::Step,
    WorkKind::Job,
    WorkKind::Question,
    WorkKind::Wake,
    WorkKind::System,
];

const STATES: [WorkState; 10] = [
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

/// What `attention_of`'s table says, written out a second way: by question
/// first, then by state.
fn expected(state: WorkState, question: bool) -> Level {
    if question && !state.ended() {
        return Level::NeedsYou;
    }
    match state {
        WorkState::Failed | WorkState::NeedsYou => Level::NeedsYou,
        WorkState::Ready => Level::Ready,
        WorkState::Done | WorkState::Cancelled | WorkState::Expired => Level::Idle,
        _ => Level::Working,
    }
}

/// Every kind × every state × {no question, an approval, a budget question}.
#[test]
fn every_kind_and_state_with_and_without_a_question_has_its_level() {
    let questions = [
        None,
        Some(question(QuestionKind::Approval)),
        Some(question(QuestionKind::Budget)),
    ];
    let mut seen = 0;
    for kind in KINDS {
        for state in STATES {
            for q in &questions {
                let v = view(kind, state, q.clone());
                let a = attention_of(&v);
                assert_eq!(
                    a.level,
                    expected(state, q.is_some()),
                    "{kind:?} {state:?} with {q:?}: {a:?}"
                );
                assert!(!a.label.is_empty(), "{kind:?} {state:?}: a label");
                assert_eq!(a.since_ms, v.state_since_ms);
                seen += 1;
            }
        }
    }
    assert_eq!(seen, 7 * 10 * 3);
}

/// Each label, as `attention()`'s are written.
#[test]
fn each_rule_has_its_label() {
    let label = |v: WorkView| attention_of(&v).label;
    let conv = |s| view(WorkKind::Conversation, s, None);
    assert_eq!(
        label(view(
            WorkKind::Task,
            WorkState::NeedsYou,
            Some(question(QuestionKind::Budget))
        )),
        "budget: $10.02 of $10"
    );
    assert_eq!(
        label(view(
            WorkKind::Task,
            WorkState::NeedsYou,
            Some(question(QuestionKind::Approval))
        )),
        "confirm proc.run: run cargo test"
    );
    let mut floor = question(QuestionKind::Approval);
    floor.floor = true;
    assert_eq!(
        label(view(WorkKind::Question, WorkState::NeedsYou, Some(floor))),
        "confirm proc.run: run cargo test · floor"
    );
    let mut v = conv(WorkState::Failed);
    v.report = Some(WorkReport {
        outcome: "failed".into(),
        first_line: None,
        reason: Some("the provider refused the key\nmore".into()),
    });
    assert_eq!(label(v), "failed: the provider refused the key");
    assert_eq!(label(conv(WorkState::Failed)), "failed");
    assert_eq!(label(conv(WorkState::NeedsYou)), "waiting on you");
    assert_eq!(label(conv(WorkState::Running)), "turn 1");
    assert_eq!(
        label(view(WorkKind::Job, WorkState::Running, None)),
        "running"
    );
    let mut v = conv(WorkState::Queued);
    v.why = Some("wake".into());
    assert_eq!(label(v), "queued · wake");
    let mut v = conv(WorkState::Waiting);
    v.doing = Some(Doing {
        what: DoingKind::Job,
        label: "waiting on 2 calls".into(),
        since_ms: AT,
        turn: 2,
        loop_index: 0,
    });
    assert_eq!(label(v), "waiting on 2 calls");
    assert_eq!(label(conv(WorkState::Ready)), "ready");
    assert_eq!(label(conv(WorkState::Done)), "complete");
    assert_eq!(label(view(WorkKind::Job, WorkState::Done, None)), "done");
    assert_eq!(label(conv(WorkState::Expired)), "expired");
}

/// Each kind of question's answers: the floor marks approve dangerous, and
/// external text adds approve and trust.
#[test]
fn each_kind_of_question_has_its_answers() {
    let ids =
        |q: &Question| -> Vec<String> { answer_options(q).into_iter().map(|o| o.id).collect() };
    let mut q = question(QuestionKind::Approval);
    assert_eq!(ids(&q), ["approve", "decline"]);
    let opts = answer_options(&q);
    assert_eq!(opts[0].style, Style::Primary);
    assert!(opts[1].takes_note, "a decline takes a note");
    q.external_text = true;
    assert_eq!(ids(&q), ["approve", "approve_trust", "decline"]);
    q.floor = true;
    let opts = answer_options(&q);
    assert_eq!(
        opts[0].style,
        Style::Danger,
        "the floor's approve is dangerous"
    );
    assert_eq!(opts[1].style, Style::Danger);
    assert_eq!(ids(&question(QuestionKind::Budget)), ["reset", "decline"]);
    assert_eq!(ids(&question(QuestionKind::Change)), ["accept", "decline"]);
    assert_eq!(ids(&question(QuestionKind::Extension)), ["ack", "decline"]);
    let mut ask = question(QuestionKind::Ask);
    ask.choices = vec!["the north channel".into(), "the south channel".into()];
    let opts = answer_options(&ask);
    assert_eq!(
        opts.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["choice:0", "choice:1", "choice:other"]
    );
    assert_eq!(opts[1].label, "the south channel");
    assert!(opts[2].takes_note, "something else, in words");
}

fn execution(state: &str, wake: Option<WaitingOn>) -> ExecutionView {
    let mut x = ExecutionView {
        position: 9,
        at_ms: AT,
        execution_id: "exe_0000a1b2c3".into(),
        session_id: "ses_0000a1b2c3".into(),
        kind: SessionKind::Task,
        parent_session_id: Some("ses_0000d4e5f6".into()),
        state: state.into(),
        previous: Some("running".into()),
        waiting_on: wake,
        pending: vec![],
        turns: 3,
        outstanding: 0,
        spent_usd: 0.42,
        limit_usd: 10.0,
        ended_reason: None,
        why: None,
        wake_at_ms: None,
        attention: Attention {
            level: Level::Working,
            label: String::new(),
            since_ms: AT - 5_000,
        },
    };
    x.attention = crate::attention(&x, &utc_hm);
    x.attention.since_ms = AT - 5_000;
    x
}

/// Each state and wait of today's push, as the design maps it.
#[test]
fn an_execution_maps_to_its_work_state_for_each_state_and_wait() {
    let cases = [
        ("queued", None, 0, WorkState::Queued),
        ("running", None, 0, WorkState::Running),
        ("waiting", Some(WaitingOn::Input), 0, WorkState::Ready),
        ("waiting", Some(WaitingOn::Input), 1, WorkState::Waiting),
        (
            "waiting",
            Some(WaitingOn::Actions {
                correlation_ids: vec!["c".into()],
            }),
            1,
            WorkState::Waiting,
        ),
        (
            "waiting",
            Some(WaitingOn::Execution {
                execution_id: "exe_c".into(),
            }),
            0,
            WorkState::Waiting,
        ),
        (
            "waiting",
            Some(WaitingOn::DueAt { at_ms: AT + 1 }),
            0,
            WorkState::Waiting,
        ),
        (
            "waiting",
            Some(WaitingOn::Confirm {
                confirm_id: "q".into(),
            }),
            0,
            WorkState::NeedsYou,
        ),
        (
            "waiting",
            Some(WaitingOn::Budget {
                correlation_id: "q".into(),
            }),
            0,
            WorkState::NeedsYou,
        ),
        ("waiting", Some(WaitingOn::Unknown), 0, WorkState::Waiting),
        ("blocked", None, 0, WorkState::NeedsYou),
        ("budget_exhausted", None, 0, WorkState::NeedsYou),
        ("failed", None, 0, WorkState::Failed),
        ("complete", None, 0, WorkState::Done),
        ("cancelled", None, 0, WorkState::Cancelled),
        ("paused_for_jev", None, 0, WorkState::Unknown),
    ];
    for (state, wake, outstanding, want) in cases {
        let mut x = execution(state, wake.clone());
        x.outstanding = outstanding;
        let w = WorkView::from_execution(&x);
        assert_eq!(w.state, want, "{state} on {wake:?}");
        assert_eq!(w.ended_at_ms.is_some(), want.ended(), "{state}");
    }
    // The tree, the cost, and the attention, carried.
    let w = WorkView::from_execution(&execution("running", None));
    assert_eq!(w.kind, WorkKind::Task);
    assert_eq!(w.parent.as_deref(), Some("ses_0000d4e5f6"));
    assert_eq!(w.root, "ses_0000d4e5f6");
    assert_eq!(w.previous, Some(WorkState::Running));
    assert_eq!(w.cost.map(|c| c.spent_usd), Some(0.42));
    assert_eq!(w.attention.label, "turn 3");
    assert_eq!(w.doing.map(|d| d.what), Some(DoingKind::Thinking));
    assert!(w.title.is_empty(), "no title until the view carries one");
}

/// A pending question comes across in brief; a failure's reason as its
/// report.
#[test]
fn an_executions_question_and_failure_come_across() {
    let mut x = execution(
        "waiting",
        Some(WaitingOn::Confirm {
            confirm_id: "cor_1".into(),
        }),
    );
    x.pending = vec![PendingConfirm {
        correlation_id: "cor_1".into(),
        tool: "fs.write".into(),
        reason: "create projects/beta-summary.txt (40 B)".into(),
        floor: true,
        budget: false,
        expires_at_ms: AT + 600_000,
    }];
    let w = WorkView::from_execution(&x);
    let q = w.question.expect("its question");
    assert_eq!(q.id, "cor_1");
    assert_eq!(q.kind, QuestionKind::Approval);
    assert_eq!(q.prompt, "fs.write create projects/beta-summary.txt (40 B)");
    assert_eq!(q.asked_at_ms, AT - 5_000, "when it came to need you");
    assert_eq!(q.options[0].style, Style::Danger);
    assert_eq!(w.state, WorkState::NeedsYou);
    assert_eq!(w.doing.map(|d| d.what), Some(DoingKind::Asking));
    let mut x = execution("failed", None);
    x.ended_reason = Some("the provider refused the request (400)".into());
    let w = WorkView::from_execution(&x);
    assert_eq!(
        w.report,
        Some(WorkReport {
            outcome: "failed".into(),
            first_line: None,
            reason: Some("the provider refused the request (400)".into()),
        })
    );
    assert_eq!(
        attention_of(&w).label,
        "failed: the provider refused the request (400)"
    );
}

/// A view round-trips, its optional fields absent when unset, and a state
/// from a newer daemon reads as unknown.
#[test]
fn a_view_round_trips_and_a_newer_state_reads_as_unknown() {
    let v = view(WorkKind::Task, WorkState::Running, None);
    let wire = serde_json::to_value(&v).unwrap();
    for absent in ["parent", "report", "woke", "flagged", "question", "doing"] {
        assert!(
            wire.get(absent).is_none(),
            "{absent}: absent, not null: {wire}"
        );
    }
    let back: WorkView = serde_json::from_value(wire).unwrap();
    assert_eq!(back, v);
    let s: WorkState = serde_json::from_str(r#""paused_for_jev""#).unwrap();
    assert_eq!(s, WorkState::Unknown);
}
