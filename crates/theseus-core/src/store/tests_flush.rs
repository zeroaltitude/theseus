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

/// The same deferred rows ride a flusher, a plain write, or a turn. An index
/// failure after the WAL sync must never enqueue those rows a second time.
#[test]
fn retained_unindexed_rows_are_not_deferred_again() {
    for path in ["flusher", "plain", "turn"] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let row = |key: &str| NewRecord::bytes(kinds::META, Some(key), key.as_bytes().to_vec());
        store.pending.park(row("parked"));
        store.inner.fail_next_index_writes(3);
        let turn = store.for_turn("session");
        match path {
            "flusher" => store.flush_waiting(),
            "plain" => assert!(store.append(&[row("direct")]).is_err()),
            "turn" => {
                turn.defer(row("waiting")).unwrap();
                assert!(turn.append(&[row("direct")]).is_err());
            }
            _ => unreachable!(),
        }
        assert!(
            !store.pending.any(),
            "{path}: retained rows were queued again"
        );
        assert!(turn
            .turn
            .as_ref()
            .unwrap()
            .waiting
            .lock()
            .unwrap()
            .is_empty());
        turn.flush().unwrap();
        store.append(&[row("next")]).unwrap();
        drop(turn);
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        for key in if path == "turn" {
            vec!["parked", "waiting"]
        } else {
            vec!["parked"]
        } {
            let rows = store.inner.scan(0, None, 100).unwrap();
            assert_eq!(
                rows.iter()
                    .filter(|r| r.key.as_deref() == Some(key))
                    .count(),
                1,
                "{path}: {key}"
            );
        }
    }
}

#[test]
fn a_turns_commit_callers_do_not_retry_their_own_retained_records() {
    for path in ["append", "meta", "session", "ledger", "kernel"] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let turn = store.for_turn("session");
        store.inner.fail_next_index_writes(3);
        let row = NewRecord::json(
            kinds::LEDGER,
            None,
            &serde_json::json!({"kind": "test.owned", "data": {"caller": path}}),
        )
        .unwrap();
        let result = match path {
            "append" => turn.append(&[row]).map(|_| ()),
            "meta" => turn.put_meta("own", &7),
            "session" => turn.put_session("own", &7),
            "ledger" => turn
                .append_ledger(&serde_json::json!({"kind": "test.owned"}))
                .map(|_| ()),
            "kernel" => turn.shared().append(&[row]).map(|_| ()),
            _ => unreachable!(),
        };
        assert!(result
            .unwrap_err()
            .is::<theseus_store::store::RetainedFrame>());
        turn.flush().unwrap();
        drop(turn);
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        assert_eq!(
            reopened
                .inner
                .scan(0, None, 100)
                .unwrap()
                .iter()
                .filter(|r| {
                    r.kind != kinds::LEDGER
                        || r.decode::<serde_json::Value>().unwrap()["kind"]
                            != "ledger.chain_started"
                })
                .count(),
            1,
            "{path} retried its own row"
        );
        assert!(
            theseus_store::chain_walk::verify(reopened.inner.as_ref(), None, None)
                .unwrap()
                .broken
                .is_none()
        );
    }
}
