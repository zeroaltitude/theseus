//! The cloud cross-check reads the ledger's tail a page at a time (issue 21,
//! step two). Its first cut decoded the last 100,000 rows at once, an hour
//! after every start, and the daemon held 1 to 2 GiB while it did.
//!
//! Two things are held here. The ANSWER is the same: the ids found over a
//! ledger of mixed rows, heavy ones among them, are exactly those the whole
//! tail's reading found (`Ledgered::of` over `ledger_tail`), for a tail
//! shorter than the ledger, as long, and longer, and for pages that do and do
//! not divide it. The BOUND holds: the heap the read holds at its peak is a
//! page and the ids, not the tail. The measure is a counting allocator, in a
//! binary of its own (as `theseus-store`'s `index_memory`), so nothing else
//! allocates while the read runs; nextest runs each test in a process of its
//! own as well.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};

use serde_json::{json, Value};
use theseus_core::aws::crosscheck::{ledgered_of_tail, Ledgered};
use theseus_core::ledger::LedgerRow;
use theseus_core::store::Store;

/// What is allocated and not yet freed, in bytes, and the most it has been
/// since `PEAK` was last set to it.
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

/// The system allocator, counting.
struct Counting;

fn grew(by: isize) {
    let now = LIVE.fetch_add(by, Relaxed) + by;
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: each call is the system allocator's, with the caller's own
// arguments; the count is all this adds.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller holds `alloc`'s contract, which this passes on.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size() as isize);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator with `layout`.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size() as isize, Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr` came from this allocator with `layout`, and the
        // caller holds `realloc`'s contract for `new_size`.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            grew(new_size as isize - layout.size() as isize);
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// A ledger of `n` rows of mixed kinds, in the shapes the check reads: an
/// `aws.called` row with one request id, one with several, a job's row with a
/// correlation id, a row with neither, and a heavy row of another kind (a
/// 16 KiB text in its data) every seventh row. Ids are invented.
fn ledger(n: usize) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    for i in 0..n {
        let (kind, data): (&str, Value) = match i % 7 {
            0 => (
                "aws.called",
                json!({"request_id": format!("req-{i}"), "correlation_id": format!("act_{i}")}),
            ),
            1 => (
                "aws.called",
                json!({"request_ids": [format!("req-{i}a"), format!("req-{i}b")], "correlation_id": format!("act_{i}")}),
            ),
            2 => (
                "turn.ended",
                json!({"correlation_id": format!("exe_{}", i % 50)}),
            ),
            3 => (
                "budget.asked",
                json!({"note": "no ids here", "request_id": format!("not-a-call-{i}")}),
            ),
            4 => (
                "recall.ran",
                json!({"items": "x".repeat(16 << 10), "correlation_id": format!("rec_{i}")}),
            ),
            5 => (
                "aws.called",
                json!({"request_id": 7, "request_ids": "not a list", "correlation_id": null}),
            ),
            _ => ("startup.step", Value::Null),
        };
        store
            .append_ledger(&LedgerRow::new(
                theseus_protocol::LedgerKind::parse(kind).expect("a known kind"),
                None,
                None,
                data,
            ))
            .unwrap();
    }
    (dir, store)
}

fn same(a: &Ledgered, b: &Ledgered, what: &str) {
    assert_eq!(a.request_ids, b.request_ids, "{what}: request ids");
    assert_eq!(
        a.correlation_ids, b.correlation_ids,
        "{what}: correlation ids"
    );
}

/// The whole tail's reading, as the check did it.
fn whole(store: &Store, tail: usize) -> Ledgered {
    let rows = store.ledger_tail::<LedgerRow>(tail).unwrap();
    Ledgered::of(rows.iter().map(|(_, r)| (r.kind.as_str(), &r.data)))
}

/// The same ids as the whole tail's, whether the tail is shorter than the
/// ledger, as long, or longer, and whether the page divides it or not.
#[test]
fn the_paged_read_finds_what_the_whole_tail_finds() {
    let (_dir, store) = ledger(700);
    let total = store.ledger_len().unwrap() as usize;
    assert!(total >= 700);
    for tail in [1, 100, 255, 256, 257, total - 1, total, total + 500] {
        let old = whole(&store, tail);
        for page in [1, 7, 256, 100_000] {
            let new = ledgered_of_tail(&store, tail, page).unwrap();
            same(&old, &new, &format!("tail {tail}, page {page}"));
        }
    }
    // It found something to compare.
    let all = whole(&store, total);
    assert!(all.request_ids.len() > 200 && all.correlation_ids.len() > 100);
    // A row's stray `request_id` of another kind is no call's.
    assert!(!all.request_ids.iter().any(|r| r.starts_with("not-a-call")));
}

/// An empty ledger, and a tail of nothing, read nothing.
#[test]
fn an_empty_tail_finds_nothing() {
    let (_dir, store) = ledger(0);
    let l = ledgered_of_tail(&store, 100_000, 256).unwrap();
    assert!(l.request_ids.is_empty());
    let (_dir, store) = ledger(20);
    let l = ledgered_of_tail(&store, 0, 256).unwrap();
    assert!(l.request_ids.is_empty() && l.correlation_ids.is_empty());
}

/// Over 4,000 rows, a quarter of them heavy (16 KiB each: 16 MiB of data in
/// all), the whole tail's reading holds all of it at once, and the paged
/// one a page of it: the peak the read adds is under a tenth of the data's
/// size, and under what two pages of the heavy rows would take.
#[test]
fn the_paged_read_holds_a_page_not_the_tail() {
    let (_dir, store) = ledger(4_000);
    let total = store.ledger_len().unwrap() as usize;
    let data: isize = 4_000 / 7 * (16 << 10);

    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let old = whole(&store, total);
    let whole_peak = PEAK.load(Relaxed) - base;
    drop(old);

    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let new = ledgered_of_tail(&store, total, 64).unwrap();
    let paged_peak = PEAK.load(Relaxed) - base;
    drop(new);

    assert!(
        whole_peak > data / 2,
        "the whole tail's reading holds the data ({whole_peak} of {data} bytes): the measure sees it"
    );
    assert!(
        paged_peak < data / 10,
        "the paged read peaked at {paged_peak} bytes over {data} bytes of data: it holds more than a page"
    );
    // A page of 64 rows holds at most 64 / 7 heavy ones, and the ids are small.
    assert!(
        (paged_peak as usize) < 2 * (64 / 7 + 1) * (16 << 10) * 4,
        "{paged_peak}"
    );
}
