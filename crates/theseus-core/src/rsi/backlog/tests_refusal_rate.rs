//! The rule of the refusal alarm, on its numbers alone: the night's own
//! reading of the ledger is `rpc::tests_self_backlog::refusals`.

use std::collections::BTreeMap;

use theseus_protocol::tasks::{TaskOrigin, TaskRecord, TaskState};

use super::refusals::{
    held, is_spike, said, Tally, HISTORY_MIN_CALLS, MIN_REFUSALS, REFILE_NIGHTS,
};
use super::state::{Meta, State};
use crate::task_graph;

fn t(refused: u64, calls: u64) -> Tally {
    Tally { refused, calls }
}

/// A usual share of 12% over a hundred calls, the same night's 40 of 60.
#[test]
fn a_share_at_twice_the_usual_with_enough_refusals_is_a_spike() {
    assert!(is_spike(t(40, 60), t(12, 100)));
    // Exactly twice counts; a hair under does not.
    assert!(is_spike(t(24, 100), t(12, 100)));
    assert!(!is_spike(t(23, 100), t(12, 100)));
}

#[test]
fn an_ordinary_nights_refusals_are_no_spike_however_many() {
    // 30 of 250 is 12%: the usual share, and more than twenty refusals.
    assert!(!is_spike(t(30, 250), t(12, 100)));
    // Twice the share, but a handful of refusals on a quiet night.
    assert!(!is_spike(t(MIN_REFUSALS - 1, 30), t(1, 100)));
}

#[test]
fn a_producer_with_no_history_is_held_to_sixty_percent() {
    assert!(is_spike(t(36, 60), t(0, 0)));
    assert!(!is_spike(t(35, 60), t(0, 0)));
    // History under the floor is no history, whatever its share said.
    assert!(!is_spike(t(30, 100), t(0, HISTORY_MIN_CALLS - 1)));
    assert!(is_spike(t(30, 100), t(0, HISTORY_MIN_CALLS)));
}

#[test]
fn the_title_says_the_numbers_and_the_usual_share() {
    assert_eq!(
        said("inproc:fs.edit", t(41, 60), t(12, 100)),
        "fs.edit refused 41 of 60 calls last night; its usual share is 12%"
    );
    let none = said("inproc:fs.patch", t(41, 60), t(0, 10));
    assert!(none.starts_with("fs.patch refused 41 of 60 calls last night; it has no usual"));
}

fn item(state: TaskState, created_ms: u64) -> (State, BTreeMap<String, TaskRecord>) {
    let t = task_graph::NewTask {
        id: "x_1".into(),
        title: "an item",
        objective: String::new(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: None,
        origin: TaskOrigin {
            session: "self:backlog".into(),
            principal: "self".into(),
            by_model: true,
            self_started: true,
            because: None,
        },
        state,
    }
    .build(created_ms);
    let mut st = State::default();
    st.items.insert(
        t.id.clone(),
        Meta {
            cause: "refusal.rate:inproc:fs.edit".into(),
            source: "action.failed".into(),
            count: 1,
            usd: 0.0,
            person: false,
            task_failed: true,
            work: 2,
            last_ms: created_ms,
            keel: None,
            score: 0,
            rank: 0,
            pin: None,
            pattern: None,
        },
    );
    (st, BTreeMap::from([(t.id.clone(), t)]))
}

/// An open item holds its cause; a closed one holds it for seven nights.
#[test]
fn a_cause_is_held_while_open_and_for_seven_nights_after_its_filing() {
    let key = "refusal.rate:inproc:fs.edit";
    let day = 86_400_000;
    let (st, tasks) = item(TaskState::Proposed, 0);
    assert!(held(key, &st, &tasks, 30 * day), "open for a month");
    let (st, tasks) = item(TaskState::Failed, 0);
    assert!(held(key, &st, &tasks, (REFILE_NIGHTS - 1) * day));
    assert!(!held(key, &st, &tasks, REFILE_NIGHTS * day));
    assert!(!held("refusal.rate:inproc:fs.patch", &st, &tasks, day));
}
