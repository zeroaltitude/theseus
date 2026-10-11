//! Who runs at which priority (theseus-y9a3). The tender's own work (the WAL's
//! ingest, tantivy's merges, the backfill) is background: nice 10 (the
//! design's; the embedding thread lowers itself to 19) in the idle I/O class.
//! A query is not: it is a turn's recall, whose words must answer inside the
//! 250 ms deadline. Run at nice 10 in the idle I/O class beside a busy
//! machine (the builds at nice 0), a 3 ms word search waited ~250 ms for a
//! core and a cold one ~450 ms for its pages; on the owner's daemon 6 of
//! 2026-10-10's 13 live recalls ended at the deadline with nothing, and no
//! recall since the words' race (theseus-w9qv) had vectors.
//!
//! So the socket's threads keep the priority the tender started at (the
//! daemon's): [`crate::serve`] starts the socket's thread first, and only
//! then lowers the calling thread, from which every other thread is born. A
//! thread cannot take back a priority it gave up (`RLIMIT_NICE` is 0 under
//! the daemon's unit), so the order is the whole of it. A query's embedding
//! pool is born of the first query's thread (`ahead`), so it runs at the
//! queries' priority too.

pub use theseus_protocol::index::IndexPriority as Priority;

const IOPRIO_WHO_PROCESS: libc::c_int = 1;
const IOPRIO_CLASS_SHIFT: libc::c_int = 13;
const IOPRIO_CLASS_IDLE: libc::c_int = 3;

/// The calling thread's priority. On Linux both calls name the calling
/// thread, not the whole process.
pub fn current() -> Priority {
    // SAFETY: no pointers; for the calling thread it cannot fail (-1 is the
    // nice value -1).
    let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    // SAFETY: plain integers.
    let io = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, 0) };
    let io = match (io >> IOPRIO_CLASS_SHIFT) as libc::c_int {
        _ if io < 0 => "unknown",
        1 => "realtime",
        IOPRIO_CLASS_IDLE => "idle",
        _ => "best-effort",
    };
    Priority {
        nice,
        io: io.into(),
    }
}

/// Lower the calling thread to `nice` in the idle I/O class: every thread it
/// starts after inherits both. The idle class (roadmap row 51): the tender's
/// commits, about 140 ms of fsyncs each on this disk, wait for the disk to be
/// otherwise idle, so the core's WAL syncs never queue behind them, where the
/// I/O scheduler honours classes. A failure is logged, never fatal.
pub fn lower(nice: i32) {
    // SAFETY: no pointers; it sets the calling thread's own priority.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) } != 0 {
        tracing::warn!(nice, "index: could not lower its priority");
    }
    // SAFETY: plain integers; it sets the calling thread's own I/O priority.
    let set = unsafe {
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            0,
            IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT,
        )
    };
    if set != 0 {
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "index: could not take the idle I/O class"
        );
    }
}
