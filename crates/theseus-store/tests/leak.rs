//! The stop waits for a handle on the store that a thread still holds (theseus-q0re4): a thread that
//! upgrades a `Weak` to the core per unit of work can be mid-unit when the runtime is dropped, and the
//! store closes when that unit ends. One test, because the watch is the process's.

use std::sync::mpsc;
use std::time::Duration;

use theseus_store::leak;
use theseus_store::store::WalStore;

#[test]
fn the_wait_ends_when_the_last_handle_goes_and_not_before() {
    let dir = tempfile::tempdir().unwrap();
    // Nothing watched: nothing to wait for.
    assert!(leak::wait_closed(Duration::ZERO));

    let store = std::sync::Arc::new(WalStore::open(dir.path(), Default::default()).unwrap());
    leak::watch(&store);
    let held = store.clone();
    drop(store);
    assert_eq!(leak::holders(), 1);
    // A holder is alive, so the bound is all the wait has.
    assert!(!leak::wait_closed(Duration::from_millis(50)));
    assert_eq!(leak::holders(), 1);

    // A thread holds it a while longer: the wait ends with the drop, and the store is gone by then.
    let (go, wait) = mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        wait.recv().unwrap();
        drop(held);
    });
    let waiter =
        std::thread::spawn(|| (leak::wait_closed(Duration::from_secs(30)), leak::holders()));
    go.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(waiter.join().unwrap(), (true, 0));
    // Closed stays closed.
    assert!(leak::wait_closed(Duration::ZERO));
}
