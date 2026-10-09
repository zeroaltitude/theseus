//! Queries ahead of the backfill (theseus-w9qv). A recall's query is embedded
//! on its connection's thread while the embedding thread backfills, and both
//! run the one model; two things kept the query behind the backfill:
//!
//! - **rayon's pool.** candle runs its softmax and layer norms as rayon's
//!   `par_chunks`, which a thread outside a pool hands to the global pool and
//!   waits for. The tender gives that pool one thread (`[index] threads`), and
//!   it was born of the embedding thread, at nice 19: each of a query's
//!   layers waited behind the backfill's own jobs, at the backfill's priority
//!   (171 to 747 ms an embedding under load, against 2 to 5 ms for the word
//!   search). A query's embedding now runs in a pool of its own
//!   ([`install`]), born of the first query's thread at the tender's own
//!   priority, so it never queues behind a backfill job.
//! - **the CPU.** While a query embeds ([`Ahead::hold`]), the embedding thread
//!   starts no batch: a batch already begun runs to its end (a forward pass
//!   is not cut), the next waits for the query, and the query's end wakes it.
//!   So a query has the cores the backfill would take, on a small machine
//!   too.
//!
//! Chosen over a queue that the embedding thread would serve queries from: a
//! query served there waits for the batch in progress (up to a whole batch of
//! eight 128-token texts), and the thread runs at nice 19, the priority the
//! query must not have.
//!
//! **Sized for one query** (theseus-zo1y). A recall's query is one short
//! text, and its embedding is a chain of matmuls: candle splits each across
//! `RAYON_NUM_THREADS` jobs, read at every matmul, run in the pool the caller
//! is in. So the queries' pool has [`query_threads`] threads (up to
//! [`MAX_QUERY_THREADS`], half the machine's cores at most, so a query never
//! takes the daemon's), `RAYON_NUM_THREADS` is the larger of that and
//! `[index] threads`, and the global pool, the backfill's, is built with
//! `[index] threads` alone ([`build_backfill_pool`]), from the embedding
//! thread at nice 19: a backfill's matmuls are split as a query's, and run
//! one at a time on its one thread, as before.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

/// The most threads one query's embedding takes: the scale plan's spike
/// measured 214 to 224 ms at 128 tokens on 4, against about 350 on one.
pub const MAX_QUERY_THREADS: usize = 4;

/// The pool queries embed in, of [`threads`] threads.
static QUERY_POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
/// The queries' threads, when the binary set them (`--query-threads`).
static QUERY_THREADS: OnceLock<usize> = OnceLock::new();
/// The backfill's threads (`[index] threads`), when the binary set them.
static BACKFILL_THREADS: OnceLock<usize> = OnceLock::new();

/// A query's threads on a machine of `cores`: half of them, at least one and
/// at most [`MAX_QUERY_THREADS`].
pub fn query_threads(cores: usize) -> usize {
    (cores / 2).clamp(1, MAX_QUERY_THREADS)
}

/// The machine's cores, as the process may use them.
pub fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// Set once, before any pool is built (the binary's `serve`): `queries`
/// threads for the queries' pool (0: [`query_threads`] of this machine),
/// `backfill` for the global one. Returns the queries' threads, the number
/// `RAYON_NUM_THREADS` must be at least.
pub fn set_threads(queries: usize, backfill: usize) -> usize {
    let queries = if queries == 0 {
        query_threads(cores())
    } else {
        queries
    };
    let _ = QUERY_THREADS.set(queries);
    let _ = BACKFILL_THREADS.set(backfill.max(1));
    queries
}

/// The queries' pool's threads: the binary's, else this machine's.
pub fn threads() -> usize {
    QUERY_THREADS
        .get()
        .copied()
        .unwrap_or_else(|| query_threads(cores()))
}

/// Build rayon's global pool, the backfill's, with the binary's `[index]
/// threads`, from the calling thread (the embedding thread, at nice 19, so
/// its threads are too). Nothing when the binary set none (tests), or when
/// something built it first, which is said.
pub fn build_backfill_pool() {
    let Some(&n) = BACKFILL_THREADS.get() else {
        return;
    };
    if let Err(e) = rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .thread_name(|i| format!("index-backfill-{i}"))
        .build_global()
    {
        tracing::warn!(error = %e, threads = n, "index: the backfill's pool was built before the embedding thread");
    }
}

/// Run `f` (a query's embedding) in the queries' own rayon pool, so candle's
/// parallel parts never wait behind the backfill's in the global pool. Where
/// the pool cannot be built (no thread to spare), on the global one, as before.
pub fn install<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    let pool = QUERY_POOL.get_or_init(|| {
        match rayon::ThreadPoolBuilder::new()
            .num_threads(threads())
            .thread_name(|i| format!("index-query-{i}"))
            .build()
        {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(error = %e, "index: no pool of its own for queries; they share the backfill's");
                None
            }
        }
    });
    match pool {
        Some(p) => p.install(f),
        None => f(),
    }
}

/// The queries embedding now, and the batches the backfill held back for them.
#[derive(Debug, Default)]
pub struct Ahead {
    queries: AtomicUsize,
    deferred: AtomicU64,
}

/// A query's embedding in progress: the backfill starts no batch until it
/// ends. Its drop calls `wake` once no query is left.
pub struct Hold<'a, W: Fn()> {
    ahead: &'a Ahead,
    wake: W,
}

impl Ahead {
    /// A query begins to embed.
    pub fn hold<W: Fn()>(&self, wake: W) -> Hold<'_, W> {
        self.queries.fetch_add(1, Ordering::SeqCst);
        Hold { ahead: self, wake }
    }

    /// Whether the backfill may start a batch now; when not, it is counted.
    pub fn may_backfill(&self) -> bool {
        let free = self.queries.load(Ordering::SeqCst) == 0;
        if !free {
            self.deferred.fetch_add(1, Ordering::Relaxed);
        }
        free
    }

    /// Batches held back for a query so far.
    pub fn deferred(&self) -> u64 {
        self.deferred.load(Ordering::Relaxed)
    }
}

impl<W: Fn()> Drop for Hold<'_, W> {
    fn drop(&mut self) {
        if self.ahead.queries.fetch_sub(1, Ordering::SeqCst) == 1 {
            (self.wake)();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_query_takes_half_the_cores_up_to_four() {
        let sizes: Vec<usize> = [1, 2, 3, 4, 8, 16, 64].map(query_threads).to_vec();
        assert_eq!(sizes, [1, 1, 1, 2, 4, 4, 4]);
        assert_eq!(
            install(rayon::current_num_threads),
            query_threads(cores()),
            "the queries' pool has a query's threads"
        );
    }

    #[test]
    fn a_hold_defers_the_backfill_and_its_end_wakes_it() {
        let ahead = Ahead::default();
        let woken = Cell::new(0);
        assert!(ahead.may_backfill());
        {
            let _a = ahead.hold(|| woken.set(woken.get() + 1));
            let b = ahead.hold(|| woken.set(woken.get() + 1));
            assert!(!ahead.may_backfill());
            drop(b);
            assert_eq!(woken.get(), 0, "a query is still embedding");
            assert!(!ahead.may_backfill());
        }
        assert_eq!(woken.get(), 1, "the last query's end wakes the backfill");
        assert!(ahead.may_backfill());
        assert_eq!(ahead.deferred(), 2);
    }

    /// The global pool's every thread busy (as the backfill's jobs keep its
    /// one thread in the tender): work installed in the queries' pool still
    /// runs its parallel parts at once.
    #[test]
    fn the_queries_pool_runs_while_the_global_one_is_busy() {
        use rayon::prelude::*;
        let n = rayon::current_num_threads();
        let (release, held) = mpsc::channel::<()>();
        let held = std::sync::Arc::new(std::sync::Mutex::new(held));
        let (started, busy) = mpsc::channel();
        for _ in 0..n {
            let (held, started) = (held.clone(), started.clone());
            rayon::spawn(move || {
                started.send(()).unwrap();
                let _ = held.lock().unwrap().recv();
            });
        }
        for _ in 0..n {
            busy.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let sum: u64 = install(|| (0..10_000u64).into_par_iter().sum());
            tx.send(sum).unwrap();
        });
        let got = rx.recv_timeout(Duration::from_secs(10));
        for _ in 0..n {
            release.send(()).unwrap();
        }
        assert_eq!(got, Ok(49_995_000), "the query waited for the global pool");
    }
}
