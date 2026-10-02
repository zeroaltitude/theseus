//! The kernel's reads by state (theseus-lv2, theseus-2qt): what each reader
//! reads, counted by a store that keeps a tally, and that a store with no
//! terms answers the same.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use theseus_store::{kinds, NewRecord, Record, RecordKind, Store, StoreStats, WalConfig, WalStore};

use crate::clock::VirtualClock;
use crate::kernel::*;
use crate::spool::Spool;
use crate::tests::{auth, dispatched};
use crate::types::*;

/// A store that counts the records its whole-kind and by-term reads return,
/// by kind: what a reader costs.
struct Counting {
    inner: Arc<dyn Store>,
    read: Mutex<BTreeMap<RecordKind, u64>>,
}

impl Counting {
    fn tally(&self, kind: RecordKind, n: usize) {
        *self.read.lock().unwrap().entry(kind).or_default() += n as u64;
    }
    /// The records read of `kind` since the last call, and start again.
    fn take(&self, kind: RecordKind) -> u64 {
        self.read.lock().unwrap().remove(&kind).unwrap_or(0)
    }
    fn reset(&self) {
        self.read.lock().unwrap().clear();
    }
}

impl Store for Counting {
    fn append(&self, batch: &[NewRecord]) -> anyhow::Result<Vec<u64>> {
        self.inner.append(batch)
    }
    fn get(&self, position: u64) -> anyhow::Result<Option<Record>> {
        self.inner.get(position)
    }
    fn scan(&self, from: u64, to: Option<u64>, limit: usize) -> anyhow::Result<Vec<Record>> {
        self.inner.scan(from, to, limit)
    }
    fn latest_by_key(&self, kind: RecordKind, key: &str) -> anyhow::Result<Option<Record>> {
        self.inner.latest_by_key(kind, key)
    }
    fn latest_of_kind(&self, kind: RecordKind) -> anyhow::Result<Vec<Record>> {
        let v = self.inner.latest_of_kind(kind)?;
        self.tally(kind, v.len());
        Ok(v)
    }
    fn tail_of_kind(&self, kind: RecordKind, n: usize) -> anyhow::Result<Vec<Record>> {
        self.inner.tail_of_kind(kind, n)
    }
    fn count_of_kind(&self, kind: RecordKind) -> anyhow::Result<u64> {
        self.inner.count_of_kind(kind)
    }
    fn scan_scope(&self, scope: &str, after: u64, limit: usize) -> anyhow::Result<Vec<Record>> {
        self.inner.scan_scope(scope, after, limit)
    }
    fn count_in_scope(&self, scope: &str) -> anyhow::Result<u64> {
        self.inner.count_in_scope(scope)
    }
    fn last_position(&self) -> u64 {
        self.inner.last_position()
    }
    fn checkpoint(&self) -> anyhow::Result<u64> {
        self.inner.checkpoint()
    }
    fn stats(&self) -> anyhow::Result<StoreStats> {
        self.inner.stats()
    }
    fn latest_by_terms(
        &self,
        kind: RecordKind,
        lo: &str,
        hi: &str,
    ) -> anyhow::Result<Option<Vec<Record>>> {
        let v = self.inner.latest_by_terms(kind, lo, hi)?;
        if let Some(v) = &v {
            self.tally(kind, v.len());
        }
        Ok(v)
    }
    fn count_by_terms(&self, kind: RecordKind, lo: &str, hi: &str) -> anyhow::Result<Option<u64>> {
        self.inner.count_by_terms(kind, lo, hi)
    }
    fn latest_with_prefix(&self, kind: RecordKind, prefix: &str) -> anyhow::Result<Vec<Record>> {
        let v = self.inner.latest_with_prefix(kind, prefix)?;
        self.tally(kind, v.len());
        Ok(v)
    }
}

const PARKED: usize = 40;

fn projected(dir: &std::path::Path) -> Arc<dyn Store> {
    Arc::new(
        WalStore::open_projected(
            &dir.join("store"),
            WalConfig::default(),
            &crate::terms::PROJECTION,
        )
        .unwrap()
        .with_checkpoint_every(0),
    )
}

fn counting(dir: &std::path::Path) -> Arc<Counting> {
    Arc::new(Counting {
        inner: projected(dir),
        read: Mutex::default(),
    })
}

/// `PARKED` conversations waiting on input, one queued, and one mid-turn
/// with a job dispatched; then the process dies. Returns its directory and
/// clock, and the queued, the running, and the job's ids.
fn parked_and_busy() -> (tempfile::TempDir, Arc<VirtualClock>, String, String, String) {
    let w = crate::tests::world();
    let open = || {
        w.kernel
            .open_execution(
                &new_id("ses"),
                SessionKind::Conversation,
                auth(),
                None,
                None,
            )
            .unwrap()
    };
    for _ in 0..PARKED {
        open();
    }
    let queued = w.kernel.wake_input(&open().id).unwrap();
    let (_, running, g) = crate::tests::running(&w);
    let job = dispatched(&w, &g, "proc.run", 100);
    std::mem::forget(g); // the process dies holding its turn
    let crate::tests::World {
        dir, clock, kernel, ..
    } = w;
    drop(kernel);
    (dir, clock, queued.id, running.id, job.correlation_id)
}

/// The start, the driver's read, health, the reconcile, and a stop's read
/// of its execution's actions read only what they act on: no parked
/// conversation, and no action but the one sent (theseus-lv2, theseus-2qt).
#[test]
fn readers_by_state_read_no_parked_execution() {
    let (dir, clock, queued, running, job) = parked_and_busy();
    let store = counting(dir.path());
    let k = Kernel::new(store.clone(), clock, KernelConfig::default());
    let spool = Spool::open(&dir.path().join("spool")).unwrap();
    let rep = k.startup(Some(&spool), &NoEvidence).unwrap();
    assert_eq!(rep.requeued_interrupted, std::slice::from_ref(&running));
    assert_eq!(rep.reconcile.open_executions, PARKED as u64 + 2);
    assert_eq!(rep.reconcile.open_actions, 1);
    assert_eq!(
        store.take(kinds::EXECUTION),
        1,
        "the start read the running one"
    );
    assert_eq!(store.take(kinds::ACTION), 1, "and the job it sent");

    let mut runnable: Vec<String> = k
        .maybe_runnable()
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    runnable.sort();
    let mut want = vec![queued.clone(), running.clone()];
    want.sort();
    assert_eq!(runnable, want, "the queued, and the requeued");
    assert_eq!(store.take(kinds::EXECUTION), 2);

    let st = k.stats().unwrap();
    assert_eq!(
        st.executions_by_state,
        BTreeMap::from([("queued".into(), 2), ("waiting".into(), PARKED as u64)])
    );
    assert_eq!(
        st.actions_by_state,
        BTreeMap::from([("dispatched".into(), 1)])
    );
    assert_eq!(
        store.take(kinds::EXECUTION) + store.take(kinds::ACTION),
        0,
        "health counts"
    );

    k.reconcile(&NoEvidence).unwrap();
    assert_eq!(
        store.take(kinds::EXECUTION),
        0,
        "no due time: no execution read"
    );
    assert_eq!(store.take(kinds::ACTION), 1);

    let unsettled: Vec<String> = k
        .unsettled_actions(&running)
        .unwrap()
        .into_iter()
        .map(|a| a.correlation_id)
        .collect();
    assert_eq!(unsettled, [job]);
    assert_eq!(store.take(kinds::ACTION), 1, "its own, not every action");
    assert!(k.unsettled_actions(&queued).unwrap().is_empty());
    store.reset();
}

/// A store that keeps no terms (an older build's index, a test's store)
/// answers every read by state as the terms do, from every record.
#[test]
fn a_store_with_no_terms_answers_the_same() {
    let (dir, clock, _, running, _) = parked_and_busy();
    let spool = Spool::open(&dir.path().join("spool")).unwrap();
    let k = Kernel::new(
        projected(dir.path()),
        clock.clone(),
        KernelConfig::default(),
    );
    k.startup(Some(&spool), &NoEvidence).unwrap();
    let ids = |v: Vec<Execution>| -> Vec<String> { v.into_iter().map(|e| e.id).collect() };
    let by_terms = (
        ids(k.open_executions().unwrap()),
        ids(k.maybe_runnable().unwrap()),
        k.stats().unwrap().executions_by_state,
        k.unsettled_actions(&running).unwrap(),
        k.open_actions().unwrap(),
    );
    drop(k);
    let plain: Arc<dyn Store> =
        Arc::new(WalStore::open(&dir.path().join("store"), WalConfig::default()).unwrap());
    assert!(plain
        .latest_by_terms(kinds::EXECUTION, "s:", "s;")
        .unwrap()
        .is_none());
    let k = Kernel::new(plain, clock, KernelConfig::default());
    let every = (
        ids(k.open_executions().unwrap()),
        ids(k.maybe_runnable().unwrap()),
        k.stats().unwrap().executions_by_state,
        k.unsettled_actions(&running).unwrap(),
        k.open_actions().unwrap(),
    );
    assert_eq!(by_terms, every);
    assert_eq!(every.0.len(), PARKED + 2);
}

/// Every state is in health's lists, and the open ones are those that have
/// not ended: a new state fails to compile here until it is listed.
#[test]
fn the_state_lists_hold_every_state() {
    for s in EXEC_STATES {
        match s {
            ExecState::Queued
            | ExecState::Running
            | ExecState::Waiting
            | ExecState::Blocked
            | ExecState::Cancelled
            | ExecState::Failed
            | ExecState::BudgetExhausted
            | ExecState::Complete => {}
        }
        assert_eq!(OPEN_EXECUTIONS.contains(&s), !s.is_terminal(), "{s:?}");
    }
    for s in ACTION_STATES {
        match s {
            ActionState::Planned
            | ActionState::Authorized
            | ActionState::Dispatched
            | ActionState::Succeeded
            | ActionState::Failed
            | ActionState::OutcomeUnknown
            | ActionState::Cancelled => {}
        }
        assert_eq!(OPEN_ACTIONS.contains(&s), !s.is_settled(), "{s:?}");
    }
    let distinct = |v: Vec<&str>| v.iter().collect::<std::collections::BTreeSet<_>>().len();
    assert_eq!(
        distinct(EXEC_STATES.iter().map(|s| s.as_str()).collect()),
        8
    );
    assert_eq!(
        distinct(ACTION_STATES.iter().map(|s| s.as_str()).collect()),
        7
    );
}
