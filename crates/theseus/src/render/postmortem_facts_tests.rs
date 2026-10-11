//! The postmortem's facts, section by section, on small hand-made rows
//! (theseus-m2tiu). `tests/postmortem.rs` renders a recorded incident.

use serde_json::{json, Value};
use theseus_protocol::LedgerEntry;

use super::{analyze, Window};
use crate::render::postmortem::{markdown, Report};
use crate::render::time::pin_for_tests;

const S1: &str = "ses_aaaa1111";
const S2: &str = "ses_bbbb2222";

/// One row to make: its time, kind, session, turn and data.
type Spec = (
    u64,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Value,
);

/// Rows, numbered in the order given, at the times given.
fn rows(spec: Vec<Spec>) -> Vec<LedgerEntry> {
    spec.into_iter()
        .enumerate()
        .map(|(i, (at, kind, session, turn, data))| LedgerEntry {
            position: 100 + i as u64,
            at_unix_ms: at,
            kind: kind.into(),
            session_id: session.map(str::to_string),
            turn_id: turn.map(str::to_string),
            data,
        })
        .collect()
}

fn report(rows: &[LedgerEntry], scope: &[String]) -> Report {
    pin_for_tests();
    analyze(&Window {
        rows,
        since_ms: 0,
        until_ms: 10_000_000,
        scope,
    })
}

fn quiet_turn(out: &mut Vec<Spec>, at: u64, session: &'static str, turn: &'static str, cost: f64) {
    out.push((
        at,
        "turn.started",
        Some(session),
        Some(turn),
        json!({"author": "owner"}),
    ));
    out.push((
        at + 2_000,
        "turn.ended",
        Some(session),
        Some(turn),
        json!({"loops": 1, "tool_calls": 0, "cost_usd": cost, "stop_reason": "end_turn"}),
    ));
}

/// A kill in the middle of a turn: the restart's rows say how long it was
/// down, which turn was requeued as interrupted, what the start answered
/// "unknown", and a wake that ran late; the turn that resumed names why it ran.
#[rustfmt::skip]
#[test]
fn a_restart_names_what_it_resumed() {
    let r = rows(vec![
        (10_000, "turn.started", Some(S1), Some("turn_1"), json!({"author": "owner"})),
        (40_000, "startup.step", None, None, json!({"step": 1, "name": "store"})),
        (40_001, "execution.interrupted", Some(S1), None, json!({"execution_id": "exe_1", "interrupted": 1, "turn": 1})),
        (40_002, "startup.step", None, None, json!({"step": 2, "name": "load", "requeued": ["exe_1"]})),
        (40_003, "startup.step", None, None, json!({"step": 3, "name": "spool", "drained": 2})),
        (40_004, "startup.step", None, None, json!({"step": 4, "name": "reconcile", "report": {"marked_unknown": ["cor_unsure"]}})),
        (40_100, "server.started", None, None, json!({})),
        (40_150, "server.serving", None, None, json!({"serving_us": 9000})),
        (41_000, "wake.fired", Some(S1), None, json!({"wake_id": "wak_1", "late_ms": 21_000, "while_down": true})),
        (41_500, "turn.started", Some(S1), Some("turn_2"), json!({"author": "owner"})),
        (43_000, "turn.ended", Some(S1), Some("turn_2"), json!({"loops": 2, "tool_calls": 1, "cost_usd": 0.02})),
    ]);
    let rep = report(&r, &[]);
    assert_eq!(rep.restarts.len(), 1);
    let x = &rep.restarts[0];
    assert!(x.stopped_at_ms.is_none(), "no stop row: it was killed");
    assert_eq!(
        x.down_ms,
        Some(30_100),
        "down from the last row before the start"
    );
    assert_eq!(x.requeued.len(), 1);
    assert_eq!(
        (x.requeued[0].session.as_str(), x.requeued[0].turn.as_deref()),
        (S1, Some("turn_1"))
    );
    assert_eq!(x.spool_drained, 2);
    assert_eq!(x.unknown, ["cor_unsure"]);
    assert_eq!(x.wakes_late.len(), 1);
    assert_eq!(x.wakes_late[0].late_ms, 21_000);
    assert_eq!(rep.summary.restarts, 1);
    let md = markdown(&rep);
    for fact in [
        "was killed or lost power",
        "down 30.1 s",
        "Turn requeued as interrupted: ses_aaaa1111 (turn_1)",
        "2 completions taken from the spool",
        "Call cor_unsure answered \"unknown\"",
        "Wake wak_1 of ses_aaaa1111 ran 21.0 s late",
        "interrupted by a restart",
        "turn_2 started (resumed after a restart)",
    ] {
        assert!(md.contains(fact), "`{fact}` is in:\n{md}");
    }
}

/// A clean stop says when it stopped, and how long the daemon was down.
#[test]
fn a_clean_stop_says_when_it_stopped() {
    let clean = rows(vec![
        (5_000, "server.stopping", None, None, json!({})),
        (9_000, "startup.step", None, None, json!({"step": 1})),
        (9_100, "server.started", None, None, json!({})),
        (9_200, "server.serving", None, None, json!({})),
    ]);
    let rep = report(&clean, &[]);
    assert_eq!(rep.restarts[0].stopped_at_ms, Some(5_000));
    assert_eq!(rep.restarts[0].down_ms, Some(4_100));
    assert!(markdown(&report(&clean, &[])).contains("stopped cleanly at"));
}

/// A question that no one answers is an expired one; an answered one has its
/// answerer, its surface and its latency; a question still waiting is open.
#[test]
fn questions_are_answered_expired_or_still_open() {
    let ask = |id: &str, tool: &str| json!({"correlation_id": id, "tool": tool, "reason": "it touches the daemon's own state", "requested_at_ms": 1_000});
    let r = rows(vec![
        (
            1_000,
            "tool.confirm_requested",
            Some(S1),
            None,
            ask("cor_a", "proc.run"),
        ),
        (
            1_500,
            "tool.confirm_requested",
            Some(S2),
            None,
            ask("cor_b", "fs.write"),
        ),
        (
            2_000,
            "tool.confirm_requested",
            Some(S2),
            None,
            ask("cor_c", "proc.run"),
        ),
        (
            5_000,
            "action.confirm_answered",
            Some(S1),
            None,
            json!({"correlation_id": "cor_a", "approved": true, "by": "owner", "via": "the CLI"}),
        ),
        (
            26_500,
            "action.expired",
            Some(S2),
            None,
            json!({"correlation_id": "cor_b", "tool": "fs.write", "waited_ms": 25_000}),
        ),
    ]);
    let rep = report(&r, &[]);
    let by = |id: &str| rep.asked.iter().find(|a| a.correlation_id == id).unwrap();
    assert_eq!(by("cor_a").outcome, "approved");
    assert_eq!(by("cor_a").latency_ms, Some(4_000));
    assert_eq!(
        (by("cor_a").by.as_deref(), by("cor_a").via.as_deref()),
        (Some("owner"), Some("the CLI"))
    );
    assert_eq!(by("cor_b").outcome, "expired");
    assert_eq!(by("cor_b").latency_ms, Some(25_000));
    assert_eq!(by("cor_c").outcome, "open");
    assert_eq!(
        (rep.summary.asked, rep.summary.answered, rep.summary.expired),
        (3, 1, 1)
    );
    assert_eq!(rep.open.questions.len(), 1);
    assert_eq!(rep.open.questions[0].correlation_id, "cor_c");
    let md = markdown(&rep);
    for fact in [
        "approved by owner from the CLI after 4.0 s",
        "expired unanswered after 25.0 s",
        "still waiting",
        "Question waiting:",
    ] {
        assert!(md.contains(fact), "`{fact}` is in:\n{md}");
    }
}

/// A run of quiet turns folds into one line, with its cost; a short run, and
/// a turn that was not quiet, stay as they are.
#[test]
fn quiet_turns_fold_into_one_line() {
    let mut v = Vec::new();
    for (i, t) in ["t1", "t2", "t3", "t4", "t5"].into_iter().enumerate() {
        quiet_turn(&mut v, 1_000 + i as u64 * 10_000, S1, t, 0.01);
    }
    // A turn that failed breaks the run.
    v.push((
        60_000,
        "turn.started",
        Some(S1),
        Some("t6"),
        json!({"author": "owner"}),
    ));
    v.push((
        62_000,
        "turn.failed",
        Some(S1),
        Some("t6"),
        json!({"loops": 1, "reason": "provider: overloaded", "cost_usd": 0.0}),
    ));
    quiet_turn(&mut v, 70_000, S1, "t7", 0.01);
    quiet_turn(&mut v, 80_000, S1, "t8", 0.01);
    let rep = report(&rows(v), &[]);
    let folds: Vec<_> = rep.timeline.iter().filter(|l| l.folded > 1).collect();
    assert_eq!(folds.len(), 1, "{:#?}", rep.timeline);
    assert_eq!(folds[0].folded, 5);
    assert_eq!(folds[0].text, "… 5 turns, $0.05, nothing notable …");
    // The two after the failure are too few to fold, and keep their lines.
    assert!(rep.timeline.iter().any(|l| l.text.contains("turn t7")));
    assert!(rep.timeline.iter().any(|l| l.text.contains("turn t8")));
    assert!(rep.timeline.iter().any(|l| l.text.contains("t6 FAILED")));
    assert_eq!((rep.summary.turns, rep.summary.turns_failed), (8, 1));
    let first = rep.summary.first_failure.as_ref().unwrap();
    assert!(first.text.contains("overloaded"), "{}", first.text);
}

/// Cost by model and by session sums to the model calls' settle rows, and the
/// money a call reserved and never settled is held at the window's end.
#[rustfmt::skip]
#[test]
fn spend_sums_to_the_settle_rows_and_reservations_are_held() {
    let call = |model: &str, cost: f64| json!({"model": model, "cost_usd": cost, "loop": 1});
    let planned = |id: &str, tool: &str, reserved: f64| json!({"correlation_id": id, "tool": tool, "reserved_usd": reserved});
    let settled = |id: &str, cost: f64| json!({"correlation_id": id, "outcome": "succeeded", "producer": "provider:anthropic", "duration_ms": 900, "cost_usd": cost});
    let r = rows(vec![
        (1_000, "action.planned", Some(S1), None, planned("m1", "model.call", 0.50)),
        (1_900, "provider.call", Some(S1), Some("t1"), call("opus", 0.125)),
        (1_901, "action.succeeded", Some(S1), None, settled("m1", 0.125)),
        (2_000, "action.planned", Some(S2), None, planned("m2", "model.call", 0.50)),
        (2_900, "provider.call", Some(S2), Some("t2"), call("sonnet", 0.0625)),
        (2_901, "action.succeeded", Some(S2), None, settled("m2", 0.0625)),
        (3_000, "action.planned", Some(S1), None, planned("m3", "model.call", 0.50)),
        (3_900, "provider.call", Some(S1), Some("t3"), call("opus", 0.25)),
        (3_901, "action.succeeded", Some(S1), None, settled("m3", 0.25)),
        // Planned, never settled: held.
        (4_000, "action.planned", Some(S2), None, planned("m4", "model.call", 0.75)),
    ]);
    let rep = report(&r, &[]);
    let settle_sum = 0.125 + 0.0625 + 0.25;
    assert_eq!(rep.cost.total_usd, settle_sum);
    let model = |k: &str| rep.cost.by_model.iter().find(|c| c.key == k).unwrap();
    assert_eq!((model("opus").usd, model("opus").calls), (0.375, 2));
    assert_eq!((model("sonnet").usd, model("sonnet").calls), (0.0625, 1));
    assert_eq!(
        rep.cost.by_model.iter().map(|c| c.usd).sum::<f64>(),
        settle_sum
    );
    assert_eq!(
        rep.cost.by_session.iter().map(|c| c.usd).sum::<f64>(),
        settle_sum
    );
    assert_eq!(rep.cost.by_session[0].key, S1, "the top spender first");
    assert_eq!(rep.summary.top_spenders.len(), 2);
    assert_eq!(rep.cost.held.len(), 1);
    assert_eq!(
        (rep.cost.held[0].correlation_id.as_str(), rep.cost.held_usd),
        ("m4", 0.75)
    );
    assert_eq!(rep.open.reservations.len(), 1);
    let md = markdown(&rep);
    assert!(md.contains("Total $0.44"), "{md}");
    assert!(
        md.contains("Reservations held at the window's end: $0.75"),
        "{md}"
    );
}

/// A tool call over 30 s, one that fails, and a model call cut each get a
/// line of their own; a short one gets none.
#[test]
fn long_failed_and_cut_calls_are_lines_and_short_ones_are_not() {
    let planned = |id: &str| json!({"correlation_id": id, "tool": "proc.run", "reserved_usd": 0.0});
    let done = |id: &str, ms: u64, producer: &str| json!({"correlation_id": id, "outcome": "x", "producer": producer, "duration_ms": ms});
    let r = rows(vec![
        (
            1_000,
            "turn.started",
            Some(S1),
            Some("t1"),
            json!({"author": "owner"}),
        ),
        (1_001, "action.planned", Some(S1), None, planned("c_long")),
        (
            1_002,
            "tool.job_started",
            Some(S1),
            None,
            json!({"correlation_id": "c_long", "argv": ["sh", "-c", "sleep 33"]}),
        ),
        (1_003, "action.planned", Some(S1), None, planned("c_short")),
        (
            2_000,
            "action.succeeded",
            Some(S1),
            None,
            done("c_short", 29_999, "job"),
        ),
        (
            34_100,
            "action.succeeded",
            Some(S1),
            None,
            done("c_long", 33_100, "job"),
        ),
        (34_200, "action.planned", Some(S1), None, planned("c_bad")),
        (
            34_300,
            "action.failed",
            Some(S1),
            None,
            done("c_bad", 40, "job"),
        ),
        (
            34_400,
            "provider.cut",
            Some(S1),
            Some("t1"),
            json!({"by": "stop", "elapsed_ms": 2_500, "cost_usd": 0.01, "model": "opus"}),
        ),
        (
            35_000,
            "turn.ended",
            Some(S1),
            Some("t1"),
            json!({"loops": 3, "tool_calls": 3, "cost_usd": 0.01}),
        ),
    ]);
    let rep = report(&r, &[]);
    let texts: Vec<&str> = rep.timeline.iter().map(|l| l.text.as_str()).collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("`proc.run` (`sh -c sleep 33`) ran 33.1 s")),
        "{texts:#?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("`proc.run` failed (job) after 40 ms")),
        "{texts:#?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("model call cut (stop) after 2.5 s")),
        "{texts:#?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("29.9")),
        "a 30 s call is no line: {texts:#?}"
    );
    // The turn that held them has a start line and an end line.
    assert!(texts.iter().any(|t| t.contains("turn t1 started (owner)")));
    assert!(texts.iter().any(|t| t.contains("turn t1 ended in 34.0 s")));
}

/// A task is opened and ends with its spend; its wake is pending until it
/// fires; what a window ends in the middle of is open.
#[test]
fn what_is_open_at_the_end_is_named() {
    let r = rows(vec![
        (
            1_000,
            "turn.started",
            Some(S1),
            Some("t1"),
            json!({"author": "owner"}),
        ),
        (
            1_100,
            "wake.set",
            Some(S1),
            None,
            json!({"wake_id": "wak_open", "due_at_ms": 9_000_000}),
        ),
        (
            1_200,
            "wake.set",
            Some(S1),
            None,
            json!({"wake_id": "wak_done", "due_at_ms": 2_000}),
        ),
        (
            2_100,
            "wake.fired",
            Some(S1),
            None,
            json!({"wake_id": "wak_done", "late_ms": 100}),
        ),
        (
            1_300,
            "execution.opened",
            Some(S2),
            None,
            json!({"execution_id": "exe_t", "kind": "task", "limit_usd": 2.0, "parent": "exe_p"}),
        ),
        (
            3_000,
            "task.ended",
            Some(S1),
            None,
            json!({"task": "exe_t", "state": "complete", "reason": "report", "spent_usd": 0.5, "limit_usd": 2.0}),
        ),
    ]);
    let rep = report(&r, &[]);
    assert_eq!(rep.open.turns.len(), 1);
    assert_eq!(rep.open.turns[0].turn, "t1");
    assert_eq!(rep.open.wakes.len(), 1);
    assert_eq!(rep.open.wakes[0].wake, "wak_open");
    assert_eq!(rep.summary.tasks, [S2]);
    let md = markdown(&rep);
    assert!(md.contains("Turn running: ses_aaaa1111 t1"), "{md}");
    assert!(md.contains("Wake pending: ses_aaaa1111 wak_open"), "{md}");
    assert!(
        md.contains("task exe_t ended complete (report), spent $0.50 of $2.00"),
        "{md}"
    );
    assert!(md.contains("task opened, limit $2.00"), "{md}");
}

/// A narrowed report groups its timeline by session, and a window of nothing
/// says so in every section.
#[test]
fn a_narrowed_report_groups_by_session_and_an_empty_one_says_so() {
    let mut v = Vec::new();
    quiet_turn(&mut v, 1_000, S1, "t1", 0.01);
    quiet_turn(&mut v, 5_000, S2, "t2", 0.02);
    v.push((
        9_000,
        "turn.started",
        Some(S1),
        Some("t3"),
        json!({"author": "owner"}),
    ));
    v.push((
        9_500,
        "turn.failed",
        Some(S1),
        Some("t3"),
        json!({"loops": 1, "reason": "boom"}),
    ));
    let scope = vec![S1.to_string(), S2.to_string()];
    let md = markdown(&report(&rows(v), &scope));
    assert!(
        md.contains(&format!("### {S1}")) && md.contains(&format!("### {S2}")),
        "{md}"
    );
    assert!(
        md.contains("narrowed to ses_aaaa1111, ses_bbbb2222"),
        "{md}"
    );
    let empty = markdown(&report(&[], &[]));
    for fact in [
        "The daemon did not restart in this window.",
        "Nothing happened in this window.",
        "Nothing was asked of the owner.",
        "Nothing is open.",
        "No failure in the window.",
    ] {
        assert!(empty.contains(fact), "`{fact}` is in:\n{empty}");
    }
}
