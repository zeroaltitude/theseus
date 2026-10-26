//! A build whose gate never started files nothing (theseus-otkwu): the
//! cause of an item whose newest attempt was infrastructure is not filed
//! again, and a genuinely red one is filed exactly as before.

use theseus_protocol::tasks::TaskState;

use super::*;
use crate::fact::rsi_pr::SelfBranchBuilt;

/// Night one files the timeout cause; the item then fails after one attempt
/// that wrote `numbers`; the cause is seen again; the second night's report.
fn second_night(gate: &str, numbers: Value) -> (usize, usize, Vec<String>) {
    let c = acting(0, 20);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let n1 = tick();
    c.backlog_night(n1).unwrap();
    let item = listed(&c)
        .into_iter()
        .find(|i| i.cause == "turn.failed:timeout")
        .unwrap();
    c.rec(None).record(&SelfBranchBuilt {
        item: &item.id,
        issue: "theseus-sail1",
        branch: "self/20261026-trim",
        head: None,
        gate,
        report: None,
        cost_usd: 1.0,
        stopped_at: None,
        why: "Trim the sails.",
        numbers,
    });
    let mut t = crate::task_graph::get(&c.store, &item.id).unwrap().unwrap();
    t.state = TaskState::Failed;
    t.version += 1;
    c.store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    failed_turn(&c, "timeout: again", n1 + 3 * HOUR);
    failed_turn(&c, "timeout: and again", n1 + 4 * HOUR);
    failed_turn(&c, "timeout: and again", n1 + 5 * HOUR);
    let night = c.backlog_night(n1 + DAY).unwrap();
    let open = listed(&c)
        .into_iter()
        .filter(|i| i.cause == "turn.failed:timeout" && i.state != "failed")
        .map(|i| i.id)
        .collect();
    (night.filed.len(), night.kept_out, open)
}

#[test]
fn an_item_whose_gate_never_started_is_not_filed_again() {
    let (filed, kept_out, open) =
        second_night("red", json!({"gate_secs": 0, "exit": 127, "pushed": false}));
    assert_eq!((filed, kept_out), (0, 1));
    assert!(open.is_empty(), "{open:?}");
}

#[test]
fn an_item_whose_gate_ran_red_is_filed_again_as_it_always_was() {
    let (filed, kept_out, open) =
        second_night("red", json!({"gate_secs": 312, "exit": 1, "pushed": false}));
    assert_eq!((filed, kept_out), (1, 0));
    assert_eq!(open.len(), 1, "{open:?}");
}
