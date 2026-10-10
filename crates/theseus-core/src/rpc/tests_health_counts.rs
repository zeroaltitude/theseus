//! Health from counts (theseus-id8d): what health says of the executions and
//! the parked tasks comes from the store's terms (the counts by state, the
//! `ot` term of a task not ended), never a decode per open execution, and
//! after a run of mixed changes (sessions and tasks opened, retired,
//! stopped, cancelled, ended, and the daemon killed and started again) it
//! equals a recount of every execution.

use std::collections::BTreeMap;
use std::sync::Arc;

use theseus_kernel::{ExecState, Execution, SessionKind};
use theseus_protocol::SessionOpenParams;
use theseus_store::{kinds, records_read_here, NewRecord, Store as _};

use super::Core;
use crate::provider::FakeProvider;
use crate::Config;

fn core(dir: &std::path::Path) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let store = crate::store::Store::open(&dir.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    Core::build(super::Parts::for_tests(cfg, fake, store)).unwrap()
}

fn open(core: &Core, kind: SessionKind) -> String {
    let rec = core
        .open_session(SessionOpenParams {
            kind: Some(kind),
            ..Default::default()
        })
        .unwrap();
    rec.execution_id.unwrap()
}

/// An execution moved to `state` by its own record, as a transition that
/// ends it writes it.
fn end(core: &Core, id: &str, state: ExecState) {
    let mut e = core.kernel.execution(id).unwrap().unwrap();
    e.state = state;
    e.ended_reason = Some("ended by the test".into());
    let rec = NewRecord::json(kinds::EXECUTION, Some(id), &e).unwrap();
    core.store.inner().append(&[rec]).unwrap();
}

/// Every execution, read whole: the recount the counts must equal.
fn recount(core: &Core) -> (BTreeMap<String, u64>, Vec<String>, Vec<Execution>) {
    let all = core.kernel.executions().unwrap();
    let mut by_state = BTreeMap::new();
    for e in &all {
        *by_state.entry(e.state.as_str().to_string()).or_default() += 1;
    }
    let open_tasks = all
        .iter()
        .filter(|e| e.kind == SessionKind::Task && !e.state.is_terminal())
        .map(|e| e.id.clone())
        .collect();
    let open = all.into_iter().filter(|e| !e.state.is_terminal()).collect();
    (by_state, open_tasks, open)
}

/// Health's counts, its open tasks and its parked tasks against a recount.
fn holds(core: &Core, when: &str) {
    let (by_state, open_tasks, open) = recount(core);
    let h = core.health();
    assert_eq!(h.kernel.executions_by_state, by_state, "{when}");
    let ids: Vec<String> = core
        .kernel
        .open_tasks()
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, open_tasks, "{when}: the open tasks by their term");
    let now = theseus_protocol::now_unix_ms();
    let full = crate::parked::parked(&open, |_| None, |_| None, now);
    let parked: Vec<_> = h.tasks.unwrap().parked;
    let key = |p: &theseus_protocol::ParkedTask| (p.execution_id.clone(), p.blocker.clone());
    assert_eq!(
        parked.iter().map(key).collect::<Vec<_>>(),
        full.iter().map(key).collect::<Vec<_>>(),
        "{when}: the parked tasks, as every open execution gives them"
    );
}

#[test]
fn health_counts_equal_a_recount_after_mixed_changes_and_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(dir.path());
    let conv: Vec<String> = (0..12)
        .map(|_| open(&core, SessionKind::Conversation))
        .collect();
    let tasks: Vec<String> = (0..9).map(|_| open(&core, SessionKind::Task)).collect();
    holds(&core, "opened");
    assert_eq!(core.kernel.open_tasks().unwrap().len(), 9);

    // Retired by hand: the session's state, never its execution's.
    let sessions = core
        .store
        .list_sessions::<crate::session::SessionRecord>()
        .unwrap();
    for s in sessions.iter().take(3) {
        core.retire_session(&s.session_id, &"cli".into()).unwrap();
    }
    holds(&core, "retired");
    core.kernel.stop_execution(&conv[1], "cli").unwrap();
    core.kernel.cancel_execution(&tasks[1], "cli").unwrap();
    core.kernel.cancel_execution(&conv[0], "cli").unwrap();
    end(&core, &tasks[2], ExecState::Complete);
    end(&core, &tasks[3], ExecState::Failed);
    end(&core, &tasks[4], ExecState::Blocked);
    holds(&core, "stopped, cancelled and ended");
    // Blocked is not an end: four tasks ended, the blocked one waits.
    assert_eq!(core.kernel.open_tasks().unwrap().len(), 6);

    // Killed: the core dropped with no clean stop, and a new one on the
    // same store.
    drop(core);
    let core = self::core(dir.path());
    holds(&core, "after the restart");
    let more = open(&core, SessionKind::Task);
    core.kernel.cancel_execution(&tasks[5], "cli").unwrap();
    holds(&core, "after the restart's changes");
    assert!(core
        .kernel
        .open_tasks()
        .unwrap()
        .iter()
        .any(|e| e.id == more));
}

/// Health reads no execution of a parked conversation: the reads of a health
/// answer stay the same with two hundred more of them on the store.
#[test]
fn health_reads_no_record_of_an_open_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(dir.path());
    for _ in 0..3 {
        open(&core, SessionKind::Conversation);
    }
    open(&core, SessionKind::Task);
    let reads = || {
        let before = records_read_here();
        let _ = core.health();
        records_read_here() - before
    };
    let few = reads();
    for _ in 0..200 {
        open(&core, SessionKind::Conversation);
    }
    let many = reads();
    assert_eq!(
        many, few,
        "a health answer's records, with 200 more sessions"
    );
}
