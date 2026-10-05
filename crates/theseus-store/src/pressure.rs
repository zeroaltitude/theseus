//! Background passes yield to the machine (theseus-tood; the Linux survey's
//! card 4).
//!
//! Before each chunk of a pass that answers no one (a stretch of the WAL's
//! history check, of the index's terms or shape, a learning pack, an
//! embedding batch), its loop waits while the machine is busy: the kernel's
//! pressure stall information, `some avg10` of `/proc/pressure/cpu` and
//! `/proc/pressure/io`, at or over the gate's own settle thresholds (CPU
//! 20 %, IO 10 %: `scripts/gate.sh`'s `settle`). It looks again every second,
//! and goes after `bound` anyway, so a machine that stays busy still gets
//! its passes done, only later. Without PSI (a kernel built without it, or
//! booted with `psi=0`) nothing waits. On the runtime the wait is on tokio's
//! timer ([`quiet`]); a thread of its own sleeps ([`quiet_blocking`]).
//!
//! A thread that never answers anyone, the learning run's, also takes
//! `SCHED_IDLE` ([`idle_this_thread`]). That is unprivileged and one-way.

use std::path::Path;
use std::time::Duration;

/// CPU pressure (`some avg10`, in %) at or over which a chunk waits.
pub const CPU_BUSY: f64 = 20.0;
/// IO pressure (`some avg10`, in %) at or over which a chunk waits.
pub const IO_BUSY: f64 = 10.0;
/// How often a waiting chunk looks again. PSI's averages move every 2 s.
pub const LOOK_EVERY: Duration = Duration::from_secs(1);
/// The longest one chunk waits: one window of `avg10`, so a burst passes,
/// and a machine busy for good costs a pass at most this much a chunk.
pub const BOUND: Duration = Duration::from_secs(10);

/// What keeps background work waiting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Busy {
    /// CPU `some avg10`, in %.
    Cpu(f64),
    /// IO `some avg10`, in %.
    Io(f64),
}

/// What is busy now: `None` when CPU and IO pressure are both under their
/// thresholds, and on a kernel without PSI.
pub fn busy() -> Option<Busy> {
    busy_in(Path::new("/proc/pressure"))
}

fn busy_in(dir: &Path) -> Option<Busy> {
    let avg10 = |file: &str| {
        std::fs::read_to_string(dir.join(file))
            .ok()
            .as_deref()
            .and_then(some_avg10)
    };
    if let Some(cpu) = avg10("cpu").filter(|v| *v >= CPU_BUSY) {
        return Some(Busy::Cpu(cpu));
    }
    avg10("io").filter(|v| *v >= IO_BUSY).map(Busy::Io)
}

/// The `some` line's `avg10` of one pressure file.
fn some_avg10(text: &str) -> Option<f64> {
    text.lines()
        .find_map(|l| l.strip_prefix("some "))?
        .split_whitespace()
        .find_map(|f| f.strip_prefix("avg10="))?
        .parse()
        .ok()
}

/// Before a chunk of background work on the runtime: while [`busy`], wait on
/// tokio's timer, [`LOOK_EVERY`] at a time, up to `bound`. How long it
/// waited: zero when the first look found the machine quiet.
pub async fn quiet(bound: Duration) -> Duration {
    quiet_with(busy, bound).await
}

async fn quiet_with(mut busy: impl FnMut() -> Option<Busy>, bound: Duration) -> Duration {
    let start = tokio::time::Instant::now();
    let mut waited = Duration::ZERO;
    while waited < bound && busy().is_some() {
        tokio::time::sleep(LOOK_EVERY.min(bound - waited)).await;
        waited = start.elapsed();
    }
    waited
}

/// [`quiet`] on a thread of its own, which may sleep.
pub fn quiet_blocking(bound: Duration) -> Duration {
    quiet_blocking_unless(bound, || false)
}

/// [`quiet_blocking`], ending too at the first look that finds `stopped`.
pub fn quiet_blocking_unless(bound: Duration, stopped: impl Fn() -> bool) -> Duration {
    let start = std::time::Instant::now();
    let mut waited = Duration::ZERO;
    while waited < bound && !stopped() && busy().is_some() {
        std::thread::sleep(LOOK_EVERY.min(bound - waited));
        waited = start.elapsed();
    }
    waited
}

/// The block device under `path`, and its active I/O scheduler, as
/// `/sys/dev/block` says.
pub fn io_scheduler(path: &Path) -> Option<(String, String)> {
    use std::os::unix::fs::MetadataExt;
    let dev = std::fs::metadata(path).ok()?.dev();
    let sys = format!("/sys/dev/block/{}:{}", libc::major(dev), libc::minor(dev));
    let sys = std::fs::canonicalize(sys).ok()?;
    // A partition's queue is its disk's.
    let queue = [
        sys.join("queue/scheduler"),
        sys.parent()?.join("queue/scheduler"),
    ]
    .into_iter()
    .find(|q| q.exists())?;
    let text = std::fs::read_to_string(&queue).ok()?;
    let active = text
        .split_whitespace()
        .find_map(|w| w.strip_prefix('[')?.strip_suffix(']'))?;
    let disk = queue.parent()?.parent()?.file_name()?.to_string_lossy();
    Some((disk.into_owned(), active.to_string()))
}

/// What `theseusd check` says of background work on this machine: whether
/// the passes can see pressure, and whether the disk under `state` (or under
/// its nearest directory that exists: a check makes none) honours I/O
/// priorities, which the index tender's idle class needs.
pub fn words(state: &Path) -> String {
    let state = state
        .ancestors()
        .find(|p| p.exists())
        .unwrap_or(Path::new("/"));
    let psi = if Path::new("/proc/pressure/cpu").exists() {
        format!(
            "background passes wait while CPU pressure is {CPU_BUSY} % or more, or IO {IO_BUSY} % (PSI)"
        )
    } else {
        "no pressure information (/proc/pressure): background passes keep their fixed pacing"
            .to_string()
    };
    let disk = match io_scheduler(state) {
        Some((disk, s)) if s == "none" || s == "kyber" => format!(
            "the state's disk, {disk}, schedules with {s}, which ignores I/O priorities: the index \
             tender's idle I/O class changes nothing there"
        ),
        Some((disk, s)) => format!(
            "the state's disk, {disk}, schedules with {s}, which honours the index tender's idle I/O \
             class"
        ),
        None => "the state's disk and its I/O scheduler are unknown".to_string(),
    };
    format!("background: {psi}; {disk}")
}

/// Put the calling thread in `SCHED_IDLE`, where it runs only on a CPU no
/// other work wants. Unprivileged, and one-way: back out needs
/// `RLIMIT_NICE`, which is 0. So only for a thread that answers no one,
/// and that starts no thread anything else uses: a thread inherits its
/// creator's policy.
pub fn idle_this_thread() -> std::io::Result<()> {
    let param = libc::sched_param { sched_priority: 0 };
    // SAFETY: `param` outlives the call; pid 0 is the calling thread.
    if unsafe { libc::sched_setscheduler(0, libc::SCHED_IDLE, &param) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Whether the calling thread is in `SCHED_IDLE`.
pub fn this_thread_is_idle() -> bool {
    // SAFETY: no pointers; pid 0 is the calling thread.
    unsafe { libc::sched_getscheduler(0) == libc::SCHED_IDLE }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// The kernel's own format, as `/proc/pressure/cpu` writes it.
    #[test]
    fn the_some_lines_avg10_is_read() {
        let cpu = "some avg10=25.61 avg60=38.88 avg300=12.00 total=123\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        assert_eq!(some_avg10(cpu), Some(25.61));
        assert_eq!(
            some_avg10("full avg10=9.00 avg60=0 avg300=0 total=0\n"),
            None
        );
        assert_eq!(some_avg10(""), None);
    }

    fn fake(dir: &Path, cpu: f64, io: f64) {
        let line = |v: f64| format!("some avg10={v:.2} avg60=0.00 avg300=0.00 total=1\n");
        std::fs::write(dir.join("cpu"), line(cpu)).unwrap();
        std::fs::write(dir.join("io"), line(io)).unwrap();
    }

    /// Busy at the gate's own thresholds, CPU first; quiet under them; and
    /// nothing at all without PSI.
    #[test]
    fn busy_at_the_gates_thresholds_and_never_without_psi() {
        let d = tempfile::tempdir().unwrap();
        fake(d.path(), 19.99, 9.99);
        assert_eq!(busy_in(d.path()), None);
        fake(d.path(), 20.0, 0.0);
        assert_eq!(busy_in(d.path()), Some(Busy::Cpu(20.0)));
        fake(d.path(), 3.0, 10.0);
        assert_eq!(busy_in(d.path()), Some(Busy::Io(10.0)));
        fake(d.path(), 50.0, 50.0);
        assert_eq!(busy_in(d.path()), Some(Busy::Cpu(50.0)));
        assert_eq!(busy_in(&d.path().join("absent")), None);
    }

    /// While busy, a chunk waits a second at a time and goes as soon as it is
    /// quiet; busy for good, it goes at the bound; quiet, it waits for
    /// nothing (tokio's paused clock).
    #[tokio::test(start_paused = true)]
    async fn a_chunk_waits_while_busy_and_goes_when_quiet_within_the_bound() {
        let looks = Cell::new(0u32);
        let busy_for = |n: u32| {
            let looks = &looks;
            move || {
                looks.set(looks.get() + 1);
                (looks.get() <= n).then_some(Busy::Cpu(42.0))
            }
        };
        assert_eq!(quiet_with(busy_for(0), BOUND).await, Duration::ZERO);
        looks.set(0);
        assert_eq!(quiet_with(busy_for(3), BOUND).await, LOOK_EVERY * 3);
        looks.set(0);
        assert_eq!(quiet_with(busy_for(u32::MAX), BOUND).await, BOUND);
        let bound = Duration::from_millis(2500);
        looks.set(0);
        assert_eq!(quiet_with(busy_for(u32::MAX), bound).await, bound);
    }

    /// The state's disk and its scheduler, where there is one (a tmpfs has
    /// none), and the check's line either way.
    #[test]
    fn the_disk_under_a_path_names_its_scheduler() {
        let d = tempfile::tempdir().unwrap();
        if let Some((disk, s)) = io_scheduler(d.path()) {
            assert!(!disk.is_empty());
            assert!(
                ["none", "mq-deadline", "bfq", "kyber"].contains(&s.as_str()),
                "{s}"
            );
        }
        assert!(words(d.path()).starts_with("background: "));
    }

    /// The switch, on a thread of the test's own, and only that thread.
    #[test]
    fn a_thread_takes_sched_idle_and_only_it() {
        std::thread::spawn(|| {
            assert!(!this_thread_is_idle());
            idle_this_thread().unwrap();
            assert!(this_thread_is_idle());
        })
        .join()
        .unwrap();
        assert!(!this_thread_is_idle());
    }
}
