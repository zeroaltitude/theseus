//! The index holds at most its cache's bound of memory, however much passes
//! through it (theseus-vjn7). redb's default cache is 1 GiB, and an import, a
//! rebuild, or a long day of reads filled it with the whole index: memory a
//! daemon idle beside a busy machine had in swap, which the store's close
//! then read back, page by page, only to free it. Install #9's stop waited
//! 11.87 s on that close.
//!
//! The measure is the live heap, from a counting allocator: a binary of its
//! own, so nothing else in it allocates while the index works (nextest runs
//! each test in a process of its own as well).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};

use theseus_store::index::{IndexEntry, RedbIndex, CACHE_BYTES};
use theseus_store::Location;

/// What is allocated and not yet freed, in bytes.
static LIVE: AtomicIsize = AtomicIsize::new(0);

/// The system allocator, counting.
struct Counting;

// SAFETY: each call is the system allocator's, with the caller's own
// arguments; the count is all this adds.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller holds `alloc`'s contract, which this passes on.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            LIVE.fetch_add(layout.size() as isize, Relaxed);
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
            LIVE.fetch_add(new_size as isize - layout.size() as isize, Relaxed);
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// A keyed record's entry, its key long enough that the keys alone are
/// several times the cache's bound.
fn entry(position: u64) -> IndexEntry {
    IndexEntry {
        position,
        at_unix_ms: 0,
        kind: 1,
        key: Some(format!("{position:010}-{}", "k".repeat(500))),
        scope: Some("ses_one".into()),
        loc: Location {
            segment: 1,
            offset: position * 600,
            len: 600,
        },
        terms: None,
        sums: None,
        tags: Vec::new(),
    }
}

/// Four times the bound in keys, written as frames write them (a batch a
/// commit, none durable), then each read back as turns and lists read: what
/// stays live is the bound and the database's own small state, not the data
/// that passed through.
#[test]
fn the_index_holds_at_most_its_caches_bound_however_much_passes_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let idx = RedbIndex::open(&dir.path().join("index.redb")).unwrap();
    let before = LIVE.load(Relaxed);
    let n = (4 * CACHE_BYTES / 512) as u64;
    let mut at = 0;
    while at < n {
        let batch: Vec<IndexEntry> = (at..n.min(at + 2048)).map(entry).collect();
        idx.apply(&batch, false).unwrap();
        at += 2048;
    }
    for p in (0..n).step_by(7) {
        let key = entry(p).key.unwrap();
        assert_eq!(idx.latest_position(1, &key).unwrap(), Some(p));
    }
    let held = LIVE.load(Relaxed) - before;
    // The cache, and its allocator's state and the open tables beside it.
    let bound = CACHE_BYTES as isize + CACHE_BYTES as isize / 4;
    assert!(
        held <= bound,
        "the index holds {} MiB after {} MiB of keys passed through it: more than its cache's \
         {} MiB bound, so a stop's close frees (and reads back from swap) all of it",
        held >> 20,
        (n as usize * 512) >> 20,
        CACHE_BYTES >> 20
    );
}
