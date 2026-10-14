//! The short line's facts (theseus-8kn12): the groups the board keeps as views
//! change equal a recount of the views it holds, after any run of changes.

use std::collections::HashMap;

use proptest::prelude::*;
use theseus_kernel::{ExecState, Wake};
use theseus_protocol::ExecutionView;

use super::super::short::Short;
use super::super::{attention, hm, view};
use super::execution;

/// The view of execution `id` in `state`, waiting on `wake`, with `calls`
/// tool calls in flight, at WAL position `pos`.
fn a_view(id: u8, state: ExecState, wake: u8, calls: u8, pos: u64) -> ExecutionView {
    let wake = match wake % 3 {
        0 => Some(Wake::Input),
        1 => Some(Wake::DueAt { at_ms: 9_000 }),
        _ => Some(Wake::Confirm {
            confirm_id: "cor_x".into(),
        }),
    };
    let mut e = execution(state, wake);
    e.id = format!("exe_{id:04}");
    e.session_id = format!("ses_{id:04}");
    e.outstanding = (0..calls).map(|i| format!("cor_{i}")).collect();
    let mut v = view(&e, vec![], None, pos, 1_000 + pos * 7 % 50);
    v.outstanding = u32::from(calls);
    v.attention = attention(&v, &hm);
    v
}

const STATES: [ExecState; 8] = [
    ExecState::Queued,
    ExecState::Running,
    ExecState::Waiting,
    ExecState::Blocked,
    ExecState::Cancelled,
    ExecState::Failed,
    ExecState::BudgetExhausted,
    ExecState::Complete,
];

proptest! {
    /// Twelve executions change state over and over, as `Board::apply` moves
    /// them: after every change the kept groups are the recount's, and the
    /// answer's counts are the full read's (`Board::summary`'s on the CLI).
    #[test]
    fn the_groups_equal_a_full_recount_after_any_run_of_changes(
        steps in proptest::collection::vec((0u8..12, 0usize..8, 0u8..3, 0u8..3), 1..200),
    ) {
        let mut kept = Short::default();
        let mut now: HashMap<u8, ExecutionView> = HashMap::new();
        for (pos, (id, state, wake, calls)) in steps.into_iter().enumerate() {
            let new = a_view(id, STATES[state], wake, calls, pos as u64 + 1);
            kept.change(now.get(&id), Some(&new));
            now.insert(id, new);
            prop_assert_eq!(&kept, &Short::of(now.values()));
        }
        let r = kept.result(1);
        let level = |l: theseus_protocol::Level| {
            now.values().filter(|v| v.attention.level == l).count() as u32
        };
        let failed = now
            .values()
            .filter(|v| v.attention.level == theseus_protocol::Level::NeedsYou && v.state == "failed")
            .count() as u32;
        prop_assert_eq!(r.failed.len() as u32, failed);
        prop_assert_eq!(r.needs_you + failed, level(theseus_protocol::Level::NeedsYou));
        prop_assert_eq!(r.working, level(theseus_protocol::Level::Working));
    }
}

/// The first execution that needs you is the one that has waited longest, and
/// a failure only when nothing else needs you.
#[test]
fn the_first_is_the_longest_waiting_and_a_failure_comes_last() {
    let mut s = Short::default();
    let mut waiting = a_view(1, ExecState::Waiting, 2, 0, 5);
    waiting.attention.since_ms = 500;
    let mut older = a_view(2, ExecState::Waiting, 2, 0, 6);
    older.attention.since_ms = 100;
    let mut failed = a_view(3, ExecState::Failed, 0, 0, 7);
    failed.attention.since_ms = 1;
    s.change(None, Some(&failed));
    assert_eq!(s.result(9).first.as_deref(), Some("exe_0003"));
    s.change(None, Some(&waiting));
    s.change(None, Some(&older));
    let r = s.result(9);
    assert_eq!(r.first.as_deref(), Some("exe_0002"));
    assert_eq!((r.needs_you, r.working, r.failed.len()), (2, 0, 1));
    assert_eq!(r.failed[0].position, 7);
}
