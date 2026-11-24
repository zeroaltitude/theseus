use crate::clock::VirtualClock;
use crate::kernel::{Kernel, KernelConfig, NoEvidence};
use crate::types::SessionKind;
use std::sync::Arc;
use theseus_store::{Store, WalConfig, WalStore};

#[test]
fn a_retained_running_frame_cannot_be_admitted_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let store = Arc::new(WalStore::open(&path, WalConfig::default()).unwrap());
    let k = Kernel::new(
        store.clone(),
        VirtualClock::new(1000),
        KernelConfig::default(),
    );
    k.startup(None, &NoEvidence).unwrap();
    let e = k
        .open_execution(
            "ses_retained",
            SessionKind::Conversation,
            crate::tests::auth(),
            None,
            None,
        )
        .unwrap();
    k.wake_input(&e.id).unwrap();
    store.fail_next_index_writes(3);
    assert!(k
        .admit_queued(&e.id)
        .err()
        .unwrap()
        .is::<theseus_store::store::RetainedFrame>());
    assert!(
        matches!(k.admit_input_queued(&e.id, true).unwrap_err().downcast_ref::<crate::KernelError>(),
        Some(crate::KernelError::ExecutionRetained { id }) if id == &e.id)
    );
    let frames = store.stats().unwrap().frames_appended;
    assert!(
        matches!(k.admit_queued(&e.id).unwrap_err().downcast_ref::<crate::KernelError>(), Some(crate::KernelError::ExecutionRetained { id }) if id == &e.id),
        "stale queued state must not be admitted again"
    );
    assert!(
        matches!(k.hold_turn_queued(&e.id).unwrap_err().downcast_ref::<crate::KernelError>(), Some(crate::KernelError::ExecutionRetained { id }) if id == &e.id),
        "new input must not read the stale state either"
    );
    assert_eq!(store.stats().unwrap().frames_appended, frames);
    drop(k);
    drop(store);
    let reopened = WalStore::open(&path, WalConfig::default()).unwrap();
    let rows = reopened
        .tail_of_kind(theseus_store::kinds::LEDGER, usize::MAX)
        .unwrap();
    let running = rows
        .iter()
        .filter(|r| {
            serde_json::from_slice::<serde_json::Value>(&r.payload).unwrap()["kind"]
                == "execution.running"
        })
        .count();
    assert_eq!(running, 1, "replay must see one running transition");
}

fn during_gap(new_conversation: bool) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(WalStore::open(&dir.path().join("store"), WalConfig::default()).unwrap());
    let k = Kernel::new(
        store.clone(),
        VirtualClock::new(1000),
        KernelConfig::default(),
    );
    k.startup(None, &NoEvidence).unwrap();
    let open = |sid| {
        k.open_execution(
            sid,
            SessionKind::Conversation,
            crate::tests::auth(),
            None,
            None,
        )
        .unwrap()
    };
    let first = open("ses_gap");
    let second = (!new_conversation).then(|| open("ses_other"));
    k.wake_input(&first.id).unwrap();
    store.fail_next_index_writes(3);
    assert!(k
        .admit_queued(&first.id)
        .unwrap_err()
        .is::<theseus_store::store::RetainedFrame>());
    let second = second.unwrap_or_else(|| open("ses_new"));
    let guard = k
        .admit_input_queued(&second.id, true)
        .expect("unaffected input must run during the gap");
    assert!(store.stats().unwrap().index_gap.is_some());
    assert_eq!(
        k.execution(&second.id).unwrap().unwrap().state,
        crate::types::ExecState::Running
    );
    let rows = store
        .tail_of_kind(theseus_store::kinds::LEDGER, usize::MAX)
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| {
                let row: serde_json::Value = serde_json::from_slice(&r.payload).unwrap();
                row["kind"] == "execution.running" && row["data"]["execution_id"] == second.id
            })
            .count(),
        1
    );
    drop(guard);
}

#[test]
fn a_second_execution_can_run_during_an_index_gap() {
    during_gap(false);
}

#[test]
fn a_new_conversations_message_can_run_during_an_index_gap() {
    during_gap(true);
}

#[test]
fn only_an_indexed_execution_record_clears_the_retained_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(WalStore::open(&dir.path().join("store"), WalConfig::default()).unwrap());
    let k = Kernel::new(
        store.clone(),
        VirtualClock::new(1000),
        KernelConfig::default(),
    );
    k.startup(None, &NoEvidence).unwrap();
    let e = k
        .open_execution(
            "ses_saved",
            SessionKind::Conversation,
            crate::tests::auth(),
            None,
            None,
        )
        .unwrap();
    k.wake_input(&e.id).unwrap();
    let queued = k.execution(&e.id).unwrap().unwrap();
    store.fail_next_index_writes(3);
    assert!(k
        .admit_queued(&e.id)
        .unwrap_err()
        .is::<theseus_store::store::RetainedFrame>());
    let view = k.view(store.clone());
    let record = crate::kernel::exec_record(&queued).unwrap();
    // Even a later staged record cannot clear the id before its outer write.
    let aborted: anyhow::Result<()> = view.frame(&[&e.id], |tx| {
        tx.commit(std::slice::from_ref(&record))?;
        assert!(tx.hold_turn_queued(&e.id).is_err());
        anyhow::bail!("abort the staged update")
    });
    assert!(aborted.is_err());
    assert!(view.hold_turn_queued(&e.id).is_err());
    view.frame(&[&e.id], |tx| tx.commit(&[record])).unwrap();
    let guard = k
        .admit_queued(&e.id)
        .expect("the later indexed record is current");
    assert!(store.stats().unwrap().index_gap.is_some());
    drop(guard);
}

#[test]
fn a_held_turn_can_admit_when_another_executions_frame_is_retained() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(WalStore::open(&dir.path().join("store"), WalConfig::default()).unwrap());
    let k = Kernel::new(
        store.clone(),
        VirtualClock::new(1000),
        KernelConfig::default(),
    );
    k.startup(None, &NoEvidence).unwrap();
    let open = |sid| {
        k.open_execution(
            sid,
            SessionKind::Conversation,
            crate::tests::auth(),
            None,
            None,
        )
        .unwrap()
    };
    let first = open("ses_gap");
    let second = open("ses_held");
    let guard = k.hold_turn_queued(&second.id).unwrap();
    k.wake_input(&first.id).unwrap();
    store.fail_next_index_writes(3);
    assert!(k
        .admit_queued(&first.id)
        .unwrap_err()
        .is::<theseus_store::store::RetainedFrame>());
    assert!(k.admit_held(&guard).unwrap());
    assert!(k.admitted(&guard).unwrap());
    assert!(store.stats().unwrap().index_gap.is_some());
}
