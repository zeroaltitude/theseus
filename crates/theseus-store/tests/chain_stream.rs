//! The chain's walk streams (theseus-bmkxk): it holds a page of rows at a
//! time, never the ledger. The measure is the live heap, from a counting
//! allocator in a binary of its own: a ledger several times the walk's bound
//! is verified, and the heap's peak while it walks stays near a page.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};

use theseus_store::chain_walk::{verify, PAGE};
use theseus_store::{kinds, NewRecord, Store, WalStore};

/// What is allocated and not yet freed, in bytes, and its high-water mark.
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// SAFETY: each call is the system allocator's, with the caller's own
// arguments; the count is all this adds.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller holds `alloc`'s contract, which this passes on.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = LIVE.fetch_add(layout.size() as isize, Relaxed) + layout.size() as isize;
            PEAK.fetch_max(now, Relaxed);
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
            let now = LIVE.fetch_add(new_size as isize - layout.size() as isize, Relaxed)
                + new_size as isize
                - layout.size() as isize;
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

const ROWS: usize = 30_000;
const PAD: usize = 1_000;

fn row(n: usize) -> NewRecord {
    let pad = "x".repeat(PAD);
    NewRecord::bytes(
        kinds::LEDGER,
        None,
        format!(
            "{{\"at_unix_ms\":{n},\"kind\":\"turn.started\",\"data\":{{\"n\":{n},\"pad\":\"{pad}\"}}}}"
        )
        .into_bytes(),
    )
}

#[test]
fn a_walk_over_thirty_megabytes_of_rows_holds_a_page() {
    let dir = tempfile::tempdir().unwrap();
    let store = WalStore::open(dir.path(), Default::default()).unwrap();
    for chunk in (0..ROWS).collect::<Vec<_>>().chunks(1_000) {
        let batch: Vec<NewRecord> = chunk.iter().map(|&n| row(n)).collect();
        store.append(&batch).unwrap();
    }
    // A first walk warms the index's own cache, which is bounded and not the walk's.
    let warm = verify(&store, None, None).unwrap();
    assert!(warm.broken.is_none(), "{warm:?}");
    assert_eq!(warm.rows, ROWS as u64 + 1);

    let before = LIVE.load(Relaxed);
    PEAK.store(before, Relaxed);
    let t = std::time::Instant::now();
    let r = verify(&store, None, None).unwrap();
    let took = t.elapsed();
    let grew = PEAK.load(Relaxed) - before;
    assert!(r.broken.is_none() && r.rows == ROWS as u64 + 1);

    let data = (ROWS * PAD) as isize;
    // A page of records, once in the page and once as the read built it.
    let page = (PAGE * (PAD + 200)) as isize;
    eprintln!("walked {data} bytes of rows in {took:?}; the heap's peak grew by {grew}");
    assert!(
        grew < 3 * page,
        "the walk's heap grew by {grew} bytes over {data} bytes of rows; a page is {page}"
    );
    assert!(grew < data / 2, "the walk held a large part of the ledger");
}
