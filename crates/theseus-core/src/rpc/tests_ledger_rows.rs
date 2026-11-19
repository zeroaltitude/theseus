//! The tender's and the language servers' rows are written in the order they
//! were said (`ledger_rows`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_protocol::LedgerKind;

use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

/// Five hundred rows said one after the other from a loop with nothing
/// between them are in the ledger in that order. Each was once its own task
/// on the blocking pool, and the pool wrote them in any order: a server's
/// `lsp.ready` could land before its `lsp.started` (issue 20, the third pass).
/// The rows carry their number; the test waits for all of them, however long
/// the machine takes, and then reads the order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rows_said_in_a_row_are_written_in_that_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::scripted(vec![])),
        store,
    ))
    .unwrap();
    let said = super::ledger_rows::ordered(&core);
    for n in 0..500 {
        said(LedgerRow::new(
            LedgerKind::LspStarted,
            None,
            None,
            json!({"n": n}),
        ));
    }
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let written: Vec<u64> = core
            .store
            .ledger_tail::<LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == LedgerKind::LspStarted.as_str())
            .filter_map(|(_, r)| r.data["n"].as_u64())
            .collect();
        if written.len() == 500 {
            assert_eq!(written, (0..500).collect::<Vec<u64>>());
            return;
        }
        assert!(
            Instant::now() < until,
            "{} of 500 rows in 60 s",
            written.len()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A write that panics ends the writer that made it and nothing else: the
/// rows behind it stay queued, and the next row said starts a writer that
/// writes them (a flag left set would have held every later row for good).
#[test]
fn a_write_that_panics_does_not_stop_the_rows_after_it() {
    use super::ledger_rows::{drain_with, Queue};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Mutex;
    let row = |n: u64| LedgerRow::new(LedgerKind::LspStarted, None, None, json!({"n": n}));
    let queue = Mutex::new(Queue {
        rows: (0..3).map(row).collect(),
        draining: true,
    });
    let first = catch_unwind(AssertUnwindSafe(|| {
        drain_with(&queue, |_| panic!("the disk"));
    }));
    assert!(first.is_err());
    {
        let q = queue.lock().unwrap();
        assert!(!q.draining, "the writer that panicked is not still counted");
        assert_eq!(q.rows.len(), 2, "the rows behind the failed one are queued");
    }
    let mut written = vec![];
    drain_with(&queue, |r| written.push(r.data["n"].as_u64().unwrap()));
    assert_eq!(written, [1, 2]);
}
