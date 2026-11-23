//! The binding-row flusher gives up on a store that refuses every frame (issue 6): it does
//! not retry once a second for the life of the process, and is armed again only after a
//! frame has been written.

use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;

use super::*;

/// A store whose WAL has room for one small frame: any row past it is refused for good.
fn refusing() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let wal = WalStore::open(
        dir.path(),
        WalConfig {
            max_total_bytes: Some(600),
            ..WalConfig::default()
        },
    )
    .unwrap();
    let store = Store {
        inner: Arc::new(wal),
        dir: dir.path().to_path_buf(),
        blobs: Arc::new(crate::blobs::Blobs::new(dir.path())),
        turn: None,
        pending: Arc::default(),
        cache: Arc::default(),
        kept: Arc::default(),
        sessions: Arc::default(),
        watcher: Arc::default(),
        tasks: Arc::default(),
        held_writes: Arc::default(),
        reads: Default::default(),
        faults: Default::default(),
        plain_faults: Default::default(),
        page_fault: Default::default(),
        page_window: Default::default(),
        scope_fault: Default::default(),
    };
    store.defer_within(Duration::from_millis(100));
    (store, dir)
}

fn big_row() -> NewRecord {
    NewRecord::bytes(kinds::LEDGER, None, vec![b'x'; 2000])
}

/// KEY: after `FLUSH_TRIES` failed flushes the flusher stops, the row still waits, and a long
/// stretch of time makes no further attempt. Without the cap it tries every 100 ms for ever.
#[tokio::test(start_paused = true)]
async fn the_flusher_stops_after_its_tries_against_a_store_that_refuses() {
    let (store, _dir) = refusing();
    store.defer_waiting(big_row()).unwrap();
    // Each try is a sleep and a write on the blocking pool: let them all happen.
    for _ in 0..(FLUSH_TRIES * 4) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        tokio::task::yield_now().await;
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(store.pending.gave_up.load(SeqCst), "the flusher gave up");
    assert!(
        !store.pending.flushing.load(SeqCst),
        "and is no longer awake"
    );
    assert!(
        store.pending.any(),
        "the row is not lost: it waits for the next frame"
    );
    // A row parked now does not arm another flusher while the store refuses.
    store.defer_waiting(big_row()).unwrap();
    assert!(!store.pending.flushing.load(SeqCst), "not armed again");
    // A frame the store does take, through the store's own append (not the flusher), re-arms
    // it: the first review's F8. The stuck row is taken away first, as a turn's frame would
    // carry it off.
    store.pending.take();
    store
        .append(&[NewRecord::bytes(kinds::LEDGER, None, b"small".to_vec())])
        .unwrap();
    assert!(
        !store.pending.gave_up.load(SeqCst),
        "a written frame clears the give-up"
    );
    store
        .defer_waiting(NewRecord::bytes(kinds::LEDGER, None, b"later".to_vec()))
        .unwrap();
    assert!(
        store.pending.flushing.load(SeqCst),
        "and the next parked row arms a flusher again"
    );
}
