//! The simulator's fakes that other crates' tests use: a stand-in for
//! Discord's REST API (theseus-q4v) and its gateway (theseus-6g62). The
//! `theseus-sim` binary serves them too (`theseus-sim fake-discord`), for
//! scratch daemons.

pub mod discord_proof;
pub mod fake_discord;
pub mod fake_gateway;

/// This process's CPU time (`CLOCK_PROCESS_CPUTIME_ID`): a clock a loaded machine
/// does not stretch, for a test whose bound is on the work done and not on the wait
/// for a turn on the CPU (theseus-1b2zm).
pub fn process_cpu() -> std::time::Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes the timespec it is given, nothing else.
    unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) };
    std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}
