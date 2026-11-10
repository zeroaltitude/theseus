//! `Core::store_status()` carries the store's standing index gap into health
//! (theseus-lu5n). The render test builds a `StoreStatus` by hand and the
//! store test stops at `stats()`; this drives the one hop between them.

use std::sync::Arc;

use theseus_store::{kinds, NewRecord, Store as _};

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

#[test]
fn health_names_the_stores_standing_index_gap() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(dir.path());

    let healthy = core.store_status();
    assert_eq!(healthy.index_gap, None, "no failure forced: no gap");
    assert_eq!(healthy.refused_records, 0);

    // Three failed index writes is the retry bound: the append fails, its
    // frame is durable, and the gap stands until a reopen.
    core.store.inner().fail_next_index_writes(3);
    let rec = NewRecord::json(kinds::META, Some("gap.probe"), &1).unwrap();
    assert!(core.store.inner().append(&[rec]).is_err());

    let at = core
        .store
        .inner()
        .index_gap()
        .expect("the store reports the gap it opened");
    let told = core.store_status();
    assert_eq!(told.index_gap, Some(at), "health carries the store's position");
    assert_eq!(told.refused_records, 0, "a gap is no refused record");
}
