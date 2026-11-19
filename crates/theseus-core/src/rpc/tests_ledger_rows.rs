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
