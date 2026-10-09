//! A queued view keeps its why (theseus-q5af): the board takes `why` from
//! the frame's `execution.queued` row, and a later frame that touched a
//! still-queued execution without a row of its own (a second task's report
//! joining its parent's `reports`) dropped it, so a parent queued by its
//! task's report read `queued` alone. It is kept while the execution is
//! queued and no new row says otherwise, and goes when its turn starts.

use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use theseus_kernel::{ExecState, Execution};
use theseus_protocol::LedgerKind;
use theseus_store::kinds;

use super::super::{Board, Frame};
use super::execution;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

fn core(dir: &tempfile::TempDir) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::scripted(vec![])),
        store,
    ))
    .unwrap()
}

/// A frame at `position` holding `e`'s record, and an `execution.queued`
/// row with `why` when one is given.
fn frame(position: u64, e: &Execution, why: Option<&str>) -> Frame {
    let mut records = vec![(kinds::EXECUTION, position, serde_json::to_vec(e).unwrap())];
    if let Some(w) = why {
        let row = LedgerRow::new(
            LedgerKind::ExecutionQueued,
            Some(&e.session_id),
            None,
            json!({"execution_id": e.id, "why": w}),
        );
        records.push((
            kinds::LEDGER,
            position + 1,
            serde_json::to_vec(&row).unwrap(),
        ));
    }
    Frame {
        at_ms: position,
        committed: Instant::now(),
        position: position + 1,
        records,
        nodes: vec![],
    }
}

#[test]
fn queued_report_survives_a_second_reports_frame_and_goes_when_the_turn_starts() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let mut b = Board::default();
    let mut e = execution(ExecState::Waiting, None);
    e.parent = None;
    b.apply(&core, &frame(10, &e, None));
    // The first task's report queues it.
    e.state = ExecState::Queued;
    e.reports = vec!["exe_task1".into()];
    let sent = b.apply(&core, &frame(20, &e, Some("report")));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].why.as_deref(), Some("report"));
    assert_eq!(sent[0].attention.label, "queued · report");
    // The second task's report joins `reports`: a frame with no queue row,
    // since the parent is queued already. Its view (sent or not) keeps why.
    e.reports.push("exe_task2".into());
    b.apply(&core, &frame(30, &e, None));
    let held = b.entries[&e.id].view.clone().unwrap();
    assert_eq!(held.why.as_deref(), Some("report"), "{held:?}");
    assert_eq!(held.attention.label, "queued · report");
    // A frame that queues it again for another reason says the new one.
    let sent = b.apply(&core, &frame(40, &e, Some("wake")));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].why.as_deref(), Some("wake"));
    // Its turn starts: the why goes.
    e.state = ExecState::Running;
    e.turns += 1;
    let sent = b.apply(&core, &frame(50, &e, None));
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].state.as_str(), sent[0].why.as_deref()),
        ("running", None)
    );
    // And a later queue without a row of its own has none to keep.
    e.state = ExecState::Queued;
    b.apply(&core, &frame(60, &e, None));
    assert_eq!(b.entries[&e.id].view.as_ref().unwrap().why, None);
}
