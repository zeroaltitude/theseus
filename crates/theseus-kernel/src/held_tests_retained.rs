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
    let frames = store.stats().unwrap().frames_appended;
    assert!(
        k.admit_queued(&e.id).is_err(),
        "stale queued state must not be admitted again"
    );
    assert!(
        k.hold_turn_queued(&e.id).is_err(),
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
