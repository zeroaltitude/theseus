//! The daemon's resident memory (theseus-9lxe): what it holds, and giving
//! back what it no longer needs.
//!
//! - **Freed is not given back.** glibc's allocator keeps an arena per
//!   thread that allocated at once, and a blocking-pool thread that built
//!   something large (`import.sessions`' catalog, a full `ontology.list`, the
//!   books' terms rebuilt, an import's batch) leaves its arena's freed pages
//!   held: a scratch daemon over 21,779 imported sessions held 202 MiB from
//!   the system with 60 MiB in use. So once the daemon has been quiet for
//!   `QUIET` after work (`Resident::mark`, at each method's answer and each
//!   build's end), the tender trims (`malloc_trim`, every arena), when the
//!   free bytes are worth it (`TRIM_FLOOR`).
//! - **The import's catalog is dropped after an idle stretch.** It is built
//!   by the first `import.sessions` read and kept while the import is
//!   unchanged; after `CATALOG_IDLE` with no read it is dropped, and the next
//!   read builds it again. The Context page reads it every 30 s while open,
//!   so it stays while the page does.
//! - **Health's `resident` block** says the resident set, the heap in use
//!   and held as the tender last read it, the trims, and the largest caches
//!   with their bounds. Health never reads the heap itself: `mallinfo2`
//!   walks every free chunk under each arena's lock, 10 ms on a heap that
//!   holds 100 MiB free, and the cockpit asks for health every 2 s.
//! - **Off the start path**: the tender starts after serving and waits on a
//!   `Notify` and tokio's timer, never a poll; each trim and drop runs on the
//!   blocking pool. A musl build reads no heap and trims nothing: its
//!   allocator gives freed pages back itself.

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use theseus_protocol::resident::{CacheHealth, HeapHealth, ResidentHealth, TrimHealth};
use tokio::sync::Notify;

use crate::rpc::Core;

/// How long the daemon is quiet after work before the tender trims.
pub const QUIET: Duration = Duration::from_secs(10);
/// The longest a trim waits for quiet after the first mark since the last
/// one: the cockpit reads `health` every 2 s while a page is in sight, so a
/// daemon it watches is never quiet for `QUIET`.
pub const MAX_DEFER: Duration = Duration::from_secs(60);
/// How long the import's catalog is kept with no read.
pub const CATALOG_IDLE: Duration = Duration::from_secs(10 * 60);
/// The free heap below which a trim is not worth its walk of the arenas.
pub const TRIM_FLOOR: u64 = 8 << 20;

/// The process's resident set, in bytes: `/proc/self/statm`'s second field
/// in pages. 0 where it cannot be read.
pub fn rss_bytes() -> u64 {
    let pages = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0);
    // SAFETY: sysconf reads a constant of the running system.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    pages * u64::try_from(page).unwrap_or(4096)
}

/// The allocator's heap, every arena: glibc's `mallinfo2`. `None` on
/// another libc.
pub fn heap() -> Option<HeapHealth> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: mallinfo2 takes each arena's lock and only reads.
        let m = unsafe { libc::mallinfo2() };
        Some(HeapHealth {
            in_use_bytes: (m.uordblks + m.hblkhd) as u64,
            held_bytes: (m.arena + m.hblkhd) as u64,
        })
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        None
    }
}

/// Give the allocator's free pages back, every arena (`malloc_trim(0)`),
/// and say what it took and what the resident set was either side. `None`
/// on another libc.
pub fn trim() -> Option<TrimHealth> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let before = rss_bytes();
        let t0 = Instant::now();
        // SAFETY: malloc_trim takes each arena's lock; it frees nothing in use.
        unsafe { libc::malloc_trim(0) };
        Some(TrimHealth {
            at_ms: theseus_protocol::now_unix_ms(),
            ms: (t0.elapsed().as_secs_f64() * 1e5).round() / 100.0,
            rss_before_bytes: before,
            rss_after_bytes: rss_bytes(),
        })
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        None
    }
}

/// Whether a heap's free bytes are worth a trim.
pub fn worth_trimming(h: &HeapHealth) -> bool {
    h.held_bytes.saturating_sub(h.in_use_bytes) >= TRIM_FLOOR
}

/// The marks of work done and the trims that followed.
#[derive(Default)]
pub struct Resident {
    wake: Arc<Notify>,
    state: Mutex<State>,
}

struct State {
    /// The newest work's end since the last trim.
    marked: Option<Instant>,
    /// The first work's end since the last trim.
    first: Option<Instant>,
    trims: u64,
    last: Option<TrimHealth>,
    /// The heap as the tender last read it, at a trim's turn.
    heap: Option<HeapHealth>,
    /// `QUIET`, `MAX_DEFER` and `CATALOG_IDLE`, shorter in tests.
    quiet: Duration,
    max_defer: Duration,
    catalog_idle: Duration,
}

impl State {
    /// When a trim is due: `quiet` after the newest mark, and never later
    /// than `max_defer` after the first.
    fn trim_at(&self) -> Option<Instant> {
        let m = self.marked?;
        Some((m + self.quiet).min(self.first.unwrap_or(m) + self.max_defer))
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            marked: None,
            first: None,
            trims: 0,
            last: None,
            heap: None,
            quiet: QUIET,
            max_defer: MAX_DEFER,
            catalog_idle: CATALOG_IDLE,
        }
    }
}

/// What the tender does next.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Nothing is due: wait for a mark.
    Wait,
    /// Sleep until then (or a mark).
    Until(Instant),
    /// Act now: drop the catalog, trim, or both.
    Act { drop_catalog: bool, trim: bool },
}

/// The tender's choice at `now`, pure: a trim is due at `trim_at`
/// (`State::trim_at`), the catalog's drop `idle` after its newest read.
pub(crate) fn next(
    now: Instant,
    trim_at: Option<Instant>,
    catalog_read: Option<Instant>,
    idle: Duration,
) -> Next {
    let drop_at = catalog_read.map(|r| r + idle);
    let trim = trim_at.is_some_and(|t| t <= now);
    let drop_catalog = drop_at.is_some_and(|t| t <= now);
    if trim || drop_catalog {
        return Next::Act { drop_catalog, trim };
    }
    match (trim_at, drop_at) {
        (None, None) => Next::Wait,
        (a, b) => Next::Until(a.into_iter().chain(b).min().expect("one is set")),
    }
}

impl Resident {
    /// Work has ended (a method answered, a build finished): a trim is due
    /// once the daemon has been quiet for `quiet` from now.
    pub fn mark(&self) {
        let now = Instant::now();
        let mut s = self.state.lock().unwrap();
        s.marked = Some(now);
        s.first.get_or_insert(now);
        drop(s);
        self.wake.notify_one();
    }

    /// Shorter bounds than `QUIET` and `CATALOG_IDLE`, for a test.
    #[cfg(test)]
    pub(crate) fn set_bounds(&self, quiet: Duration, catalog_idle: Duration) {
        self.set_max_defer(quiet, MAX_DEFER, catalog_idle);
    }

    /// `set_bounds`, with a shorter `MAX_DEFER` too.
    #[cfg(test)]
    pub(crate) fn set_max_defer(
        &self,
        quiet: Duration,
        max_defer: Duration,
        catalog_idle: Duration,
    ) {
        let mut s = self.state.lock().unwrap();
        s.quiet = quiet;
        s.max_defer = max_defer;
        s.catalog_idle = catalog_idle;
        drop(s);
        self.wake.notify_one();
    }

    /// The newest mark since the last trim, for a test.
    #[cfg(test)]
    pub(crate) fn marked(&self) -> Option<Instant> {
        self.state.lock().unwrap().marked
    }

    /// When a trim is due, and the catalog's idle bound.
    fn plan(&self) -> (Option<Instant>, Duration) {
        let s = self.state.lock().unwrap();
        (s.trim_at(), s.catalog_idle)
    }

    fn catalog_idle(&self) -> Duration {
        self.state.lock().unwrap().catalog_idle
    }

    fn take_mark(&self, now: Instant) -> bool {
        let mut s = self.state.lock().unwrap();
        match s.trim_at() {
            Some(t) if t <= now => {
                s.marked = None;
                s.first = None;
                true
            }
            _ => false,
        }
    }

    fn read_heap(&self, h: Option<HeapHealth>) {
        self.state.lock().unwrap().heap = h;
    }

    fn trimmed(&self, t: TrimHealth) {
        let mut s = self.state.lock().unwrap();
        s.trims += 1;
        s.last = Some(t);
    }

    /// Health's block, with the caches the caller names.
    pub fn health(&self, caches: Vec<CacheHealth>) -> ResidentHealth {
        let s = self.state.lock().unwrap();
        ResidentHealth {
            rss_bytes: rss_bytes(),
            heap: s.heap.clone(),
            trims: s.trims,
            last_trim: s.last.clone(),
            caches,
        }
    }
}

impl Core {
    /// Health's `resident` block: the resident set, the heap, the trims, and
    /// the node cache, the import's catalog and the index's cache by size.
    pub fn resident_health(&self) -> ResidentHealth {
        let mut caches = Vec::new();
        let n = self.store.node_cache().health();
        caches.push(CacheHealth {
            name: "node cache".into(),
            bytes: n.bytes,
            cap_bytes: n.cap_bytes,
            entries: n.entries,
            estimated: false,
            note: "decoded nodes by heat, `[memory] node_cache_mb`".into(),
        });
        caches.push(self.episodes.health(self.resident.catalog_idle()));
        caches.push(CacheHealth {
            name: "index cache".into(),
            bytes: 0,
            cap_bytes: theseus_store::index::CACHE_BYTES as u64,
            entries: 0,
            estimated: false,
            note: "redb's own read cache and write buffer: its bound alone is known".into(),
        });
        self.resident.health(caches)
    }

    /// Start the tender after serving (theseus-9lxe): it waits for a mark,
    /// then for `quiet` with no other, and trims; and it drops the import's
    /// catalog after `catalog_idle` with no read. It holds the core by
    /// `Weak`, and the trim and the drop run on the blocking pool.
    pub fn tend_memory_after_serving(self: &Arc<Self>) {
        let wake = self.resident.wake.clone();
        let core = Arc::downgrade(self);
        // The start's own garbage is the first to give back.
        self.resident.mark();
        tokio::spawn(async move {
            loop {
                let Some(c) = core.upgrade() else { return };
                let (trim_at, idle) = c.resident.plan();
                let n = next(Instant::now(), trim_at, c.episodes.read_at(), idle);
                drop(c);
                match n {
                    Next::Wait => wake.notified().await,
                    Next::Until(at) => {
                        tokio::select! {
                            () = tokio::time::sleep_until(at.into()) => {}
                            () = wake.notified() => {}
                        }
                    }
                    Next::Act { drop_catalog, trim } => {
                        let core = core.clone();
                        let done = tokio::task::spawn_blocking(move || {
                            act(&core, drop_catalog, trim);
                        })
                        .await;
                        if done.is_err() {
                            return;
                        }
                    }
                }
            }
        });
    }
}

/// Drop an idle catalog, then trim when a mark is due and the free heap is
/// worth it. A drop is work too: it marks, so a trim follows.
fn act(core: &Weak<Core>, drop_catalog: bool, trim_due: bool) {
    let Some(c) = core.upgrade() else { return };
    if drop_catalog {
        if let Some(rows) = c.episodes.drop_if_idle(c.resident.catalog_idle()) {
            tracing::info!(
                rows,
                "resident: the import's catalog was dropped after an idle stretch"
            );
            c.resident.mark();
        }
    }
    if !(trim_due && c.resident.take_mark(Instant::now())) {
        return;
    }
    let h = heap();
    c.resident.read_heap(h.clone());
    if h.is_some_and(|h| worth_trimming(&h)) {
        if let Some(t) = trim() {
            tracing::info!(
                ms = t.ms,
                rss_before_mib = t.rss_before_bytes >> 20,
                rss_after_mib = t.rss_after_bytes >> 20,
                "resident: the allocator's free pages were given back"
            );
            c.resident.trimmed(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: Duration = Duration::from_secs(10);
    const I: Duration = Duration::from_secs(600);

    #[test]
    fn nothing_marked_and_no_catalog_waits_for_a_mark() {
        assert_eq!(next(Instant::now(), None, None, I), Next::Wait);
    }

    #[test]
    fn a_mark_trims_only_after_the_quiet_stretch() {
        let t = Instant::now();
        assert_eq!(next(t, Some(t + Q), None, I), Next::Until(t + Q));
        assert_eq!(
            next(t + Q - Duration::from_millis(1), Some(t + Q), None, I),
            Next::Until(t + Q)
        );
        assert_eq!(
            next(t + Q, Some(t + Q), None, I),
            Next::Act {
                drop_catalog: false,
                trim: true
            }
        );
    }

    #[test]
    fn a_catalog_is_dropped_after_its_idle_stretch_and_a_read_moves_it() {
        let t = Instant::now();
        assert_eq!(next(t, None, Some(t), I), Next::Until(t + I));
        let read_again = t + Duration::from_secs(300);
        assert_eq!(
            next(t + I, None, Some(read_again), I),
            Next::Until(read_again + I)
        );
        assert_eq!(
            next(read_again + I, None, Some(read_again), I),
            Next::Act {
                drop_catalog: true,
                trim: false
            }
        );
    }

    #[test]
    fn the_sooner_of_the_two_is_slept_until() {
        let t = Instant::now();
        assert_eq!(next(t, Some(t + Q), Some(t), I), Next::Until(t + Q));
    }

    /// Marks every 2 s (the cockpit's `health` poll) never leave `QUIET`,
    /// so the trim is due `MAX_DEFER` after the first of them.
    #[test]
    fn marks_that_never_stop_put_the_trim_off_at_most_max_defer() {
        let t = Instant::now();
        let mut s = State {
            marked: Some(t),
            first: Some(t),
            ..State::default()
        };
        assert_eq!(s.trim_at(), Some(t + QUIET));
        let last = t + Duration::from_secs(58);
        s.marked = Some(last);
        assert_eq!(s.trim_at(), Some(t + MAX_DEFER));
        assert_eq!(
            next(t + MAX_DEFER, s.trim_at(), None, I),
            Next::Act {
                drop_catalog: false,
                trim: true
            }
        );
        s.marked = None;
        assert_eq!(s.trim_at(), None, "no mark, no trim");
    }

    #[test]
    fn a_trim_is_worth_it_only_past_the_floor() {
        let h = |in_use: u64, held: u64| HeapHealth {
            in_use_bytes: in_use,
            held_bytes: held,
        };
        assert!(!worth_trimming(&h(10 << 20, (10 << 20) + TRIM_FLOOR - 1)));
        assert!(worth_trimming(&h(10 << 20, (10 << 20) + TRIM_FLOOR)));
        assert!(!worth_trimming(&h(20 << 20, 10 << 20)));
    }

    #[test]
    fn the_resident_set_and_the_heap_are_read() {
        assert!(rss_bytes() > 0);
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            let h = heap().expect("glibc's heap");
            assert!(h.held_bytes >= h.in_use_bytes);
            assert!(h.in_use_bytes > 0);
        }
    }
}
