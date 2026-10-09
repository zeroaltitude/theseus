//! The review's join fixes (theseus-753z): `attention_of` against
//! `attention()` over today's push (for every execution view the push can
//! carry, the work view built from it has the same attention when
//! `attention_of` computes it again, as the daemon's work board will, but
//! for what a work view does not carry); a question answered into a block;
//! and a reminder of a question the viewer has seen.

use super::*;
use crate::{attention, utc_hm, PendingConfirm};

const AT: u64 = 1_790_000_000_000;

fn ask(tool: &str, reason: &str) -> PendingConfirm {
    PendingConfirm {
        correlation_id: format!("cor_{tool}"),
        tool: tool.into(),
        reason: reason.into(),
        floor: false,
        budget: false,
        expires_at_ms: AT + 300_000,
    }
}

fn budget_ask() -> PendingConfirm {
    PendingConfirm {
        budget: true,
        expires_at_ms: 0,
        ..ask("budget.reset", "This session reached its spend limit")
    }
}

fn floor_ask() -> PendingConfirm {
    PendingConfirm {
        floor: true,
        ..ask("fs.write", "write under ~/.theseus")
    }
}

const STATES: [&str; 9] = [
    "queued",
    "running",
    "waiting",
    "blocked",
    "cancelled",
    "failed",
    "budget_exhausted",
    "complete",
    "paused_for_jev",
];

fn wakes() -> Vec<Option<WaitingOn>> {
    vec![
        None,
        Some(WaitingOn::DueAt {
            at_ms: AT + 3_600_000,
        }),
        Some(WaitingOn::Actions {
            correlation_ids: vec!["cor_1".into(), "cor_2".into()],
        }),
        Some(WaitingOn::Execution {
            execution_id: "exe_0000c4d5e6".into(),
        }),
        Some(WaitingOn::Confirm {
            confirm_id: "cor_proc.run".into(),
        }),
        Some(WaitingOn::Input),
        Some(WaitingOn::Budget {
            correlation_id: "cor_budget.reset".into(),
        }),
        Some(WaitingOn::Unknown),
    ]
}

/// One execution view, with the push's own attention on it.
#[allow(clippy::too_many_arguments)]
fn exec(
    kind: SessionKind,
    state: &str,
    wake: Option<WaitingOn>,
    pending: Vec<PendingConfirm>,
    outstanding: u32,
    reason: Option<&str>,
    why: Option<&str>,
    wake_at: Option<u64>,
) -> ExecutionView {
    let mut x = ExecutionView {
        position: 7,
        at_ms: AT,
        execution_id: "exe_0000a1b2c3".into(),
        session_id: "ses_0000a1b2c3".into(),
        kind,
        parent_session_id: (kind == SessionKind::Task).then(|| "ses_0000d4e5f6".into()),
        state: state.into(),
        previous: None,
        waiting_on: wake,
        pending,
        turns: 4,
        outstanding,
        spent_usd: 10.02,
        limit_usd: 10.0,
        ended_reason: reason.map(String::from),
        why: why.map(String::from),
        wake_at_ms: wake_at,
        attention: Attention {
            level: Level::Idle,
            label: String::new(),
            since_ms: 0,
        },
    };
    x.attention = attention(&x, &utc_hm);
    x
}

/// The push's attention with what a work view does not carry taken out:
/// how many more questions wait (`· +1 more`), a ready session's wake time
/// (`ready · wake 14:00`: `attention_of` has no `hm`), and the name of a
/// state this build does not know (`unknown`, read as `working`). And one
/// label the work view words its own way: a failed execution with a
/// question still pending reads `failed` (both need you), since a work
/// view's question closes when its work ends.
fn lossy(x: &ExecutionView) -> Attention {
    let mut a = x.attention.clone();
    if x.state == "failed" && !x.pending.is_empty() {
        a.label = match x.ended_reason.as_deref() {
            Some(r) => format!("failed: {}", crate::push::clip(r)),
            None => "failed".into(),
        };
    }
    if let Some((head, _)) = a.label.split_once(" · +") {
        a.label = head.to_string();
    }
    if a.label.starts_with("ready · wake ") {
        a.label = "ready".into();
    }
    if a.level == Level::Working && !STATES[..8].contains(&x.state.as_str()) {
        a.label = "working".into();
    }
    a
}

/// An extra of a view: outstanding calls, an ended reason, a why, a wake
/// time.
type Extra = (u32, Option<&'static str>, Option<&'static str>, Option<u64>);

/// Every kind × state × wake × questions × {outstanding, reason, why, wake
/// time}: the push's attention, and `attention_of` on its work view.
#[test]
fn attention_of_a_work_view_from_the_push_equals_the_pushs_attention() {
    let questions = [
        vec![],
        vec![ask("proc.run", "run cargo test")],
        vec![budget_ask()],
        vec![floor_ask(), ask("proc.run", "")],
        vec![ask("proc.run", "x"), budget_ask()],
        vec![ask("proc.run", &"y".repeat(80))],
    ];
    let extras: [Extra; 5] = [
        (0, None, None, None),
        (1, None, None, None),
        (0, Some("the provider refused the key"), None, None),
        (0, None, Some("wake"), None),
        (0, None, None, Some(AT + 7_200_000)),
    ];
    let (mut seen, mut differ) = (0, Vec::new());
    for kind in [SessionKind::Conversation, SessionKind::Task] {
        for state in STATES {
            for wake in wakes() {
                for q in &questions {
                    for (out, reason, why, wake_at) in extras {
                        let x = exec(
                            kind,
                            state,
                            wake.clone(),
                            q.clone(),
                            out,
                            reason,
                            why,
                            wake_at,
                        );
                        let w = WorkView::from_execution(&x);
                        let mut a = attention_of(&w);
                        a.since_ms = x.attention.since_ms;
                        seen += 1;
                        if a != lossy(&x) {
                            differ.push(format!(
                                "{state} on {wake:?}, {} pending, out {out}, reason {reason:?}, \
                                 why {why:?}, wake {wake_at:?}: push {:?} {:?}, work {:?} {:?}",
                                q.len(),
                                x.attention.level,
                                x.attention.label,
                                a.level,
                                a.label
                            ));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(seen, 2 * 9 * 8 * 6 * 5);
    assert!(
        differ.is_empty(),
        "{} of {seen} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
}

/// A budget question declined into `budget_exhausted` stays needs you, but it
/// is a block now, not a question: rule 3 interrupts once, and the notice
/// retracts the question's ping. Its next view says nothing again.
#[test]
fn a_question_answered_into_a_block_interrupts_once() {
    use crate::notices::{policy, Rules, Urgency, Why};
    let kind = SessionKind::Conversation;
    let mut asked = exec(
        kind,
        "waiting",
        Some(WaitingOn::Budget {
            correlation_id: "cor_budget.reset".into(),
        }),
        vec![budget_ask()],
        0,
        None,
        None,
        None,
    );
    asked.position = 7;
    let mut stuck = exec(kind, "budget_exhausted", None, vec![], 0, None, None, None);
    stuck.position = 8;
    stuck.at_ms = AT + 1_000;
    let (prev, next) = (
        WorkView::from_execution(&asked),
        WorkView::from_execution(&stuck),
    );
    let n = policy(Some(&prev), &next, &Rules::default(), &utc_hm).expect("a notice");
    assert_eq!(
        (n.urgency, n.why),
        (Urgency::Interrupt, Why::Blocked),
        "{n:?}"
    );
    assert_eq!(n.line, "✗ conversation a1b2c3 budget exhausted");
    assert_eq!(n.retracts.as_deref(), Some("cor_budget.reset"));
    let mut later = stuck;
    later.position = 9;
    later.at_ms = AT + 2_000;
    let again = WorkView::from_execution(&later);
    assert_eq!(
        policy(Some(&next), &again, &Rules::default(), &utc_hm),
        None
    );
}

/// A reminder is for a question seen and not answered: the viewer's seen
/// position, which a reminder shares with the view that asked, does not
/// silence it. The question itself, once seen, stays silent.
#[test]
fn a_reminder_pings_though_its_question_was_seen() {
    use crate::notices::{deliver, policy, Delivery, Rules, Viewer, Why};
    let kind = SessionKind::Conversation;
    let mut ready = exec(
        kind,
        "waiting",
        Some(WaitingOn::Input),
        vec![],
        0,
        None,
        None,
        None,
    );
    ready.position = 6;
    let mut asked = exec(
        kind,
        "waiting",
        Some(WaitingOn::Confirm {
            confirm_id: "cor_proc.run".into(),
        }),
        vec![ask("proc.run", "run cargo test")],
        0,
        None,
        None,
        None,
    );
    asked.position = 7;
    asked.attention.since_ms = AT;
    let (before, w) = (
        WorkView::from_execution(&ready),
        WorkView::from_execution(&asked),
    );
    let rules = Rules::default();
    let first = policy(Some(&before), &w, &rules, &utc_hm).expect("it asks");
    let mut later = w.clone();
    later.at_ms = crate::notices::next_reminder(&w, &rules).expect("a reminder");
    let n = policy(Some(&w), &later, &rules, &utc_hm).expect("its reminder");
    assert_eq!(n.why, Why::Reminder);
    let viewer = Viewer {
        seen: 7,
        away_ms: 60_000,
        sound: true,
        ..Viewer::default()
    };
    assert_eq!(
        deliver(&first, &viewer),
        Delivery::Nothing,
        "seen: the question"
    );
    assert_eq!(
        deliver(&n, &viewer),
        Delivery::Ping { sound: true },
        "the reminder"
    );
}
