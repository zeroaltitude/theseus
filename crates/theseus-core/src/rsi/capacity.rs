//! The self pipeline's capacity guard (theseus-pw1q.16): a build starts only
//! when the host has room for it. Four things are read, every time, and the
//! first that holds says why:
//!
//! 1. **Slots**: fewer builds in flight than `[self] pipeline_builds`, and
//!    fewer self trees open (`<state>/self/trees/<slug>`) than that too, so a
//!    tree a red build left behind holds its slot until it is closed.
//! 2. **Disk**: the build trees' filesystem keeps `[self] min_free_gb` free
//!    (`statvfs`, as [`crate::disk`] reads the state dir's).
//! 3. **Memory**: memory pressure (`some avg10` of `/proc/pressure/memory`)
//!    under [`MEMORY_BUSY`].
//! 4. **CPU**: the five-minute load average a core (`/proc/loadavg`) under
//!    [`CPU_LOAD_BUSY`]: the host has a spare core on average, so a build can
//!    get cores. Not CPU pressure: see [`CPU_LOAD_BUSY`]. Memory pressure is
//!    read the way the daemon's background passes read pressure
//!    (`theseus_store::pressure`); without PSI it does not hold, and a load
//!    that cannot be read does not hold either.
//!
//! The slots are read first and the host after, but every check runs
//! whether or not the slots are free: a free slot never skips the host's.
//! The guard reads; it writes nothing. A held start says why in `theseus
//! self pipeline` and, when the reason changes, in a `self.pipeline.held`
//! row.

use std::path::Path;

use serde::Serialize;

/// Memory pressure (`some avg10`, in %) at or over which no build starts:
/// tasks stalled on memory a tenth of the time already.
pub const MEMORY_BUSY: f64 = 10.0;
/// Runnable tasks a core (the five-minute load average over the cores this
/// process may use) at or over which no build starts: on average no core was
/// spare, so a build would not get cores, and the gate's timing tests flake.
///
/// It was CPU pressure, `some avg10` at 60 %, and that asks the wrong
/// question (theseus-1eemp, 2026-11-11). `some` is the share of time at least
/// one task waited, averaged over the CPUs that were busy at all, so eight
/// loops pinned to four cores read 94 % on a 16-core host with twelve cores
/// idle (the guard shut for 11 h 48 m), while 32 loops across all sixteen read
/// 56 % at a load of 37 (the guard open). It is a statement about the busy
/// CPUs, and a build is placed on an idle one. `full`, the obvious repair, is
/// a constant zero: the kernel reports no system-wide `full` for the CPU
/// (`total=0` at a load of 37 and 99.8 % busy, kernel 6.18).
///
/// One a core: the 5-minute average is the kernel's own smoothing, so a
/// cargo build's first ten seconds do not shut the guard, and a load that has
/// held at the cores for about three minutes does. The host's own load while
/// the factory worked on 2026-11-11 ran from 0.3 a core (08:30) to 1.1 (09:00,
/// three cargo units compiling): the owner's allocation for this host is three
/// units, so a start waits for a fourth's room and takes it when the compiles
/// ease, which they do within minutes. Linux counts tasks in uninterruptible
/// sleep too, so a host stalled on disk reads busy: the guard errs towards
/// waiting. It errs the other way on pinning: loops pinned to four cores read
/// the load of their number, not four cores' share, and a pinned overload
/// (24 loops on 4 cores) holds a start with twelve cores idle. The line is
/// argued from what a spare core is, not calibrated against the self gate's
/// flake rate: that measurement is owed (theseus-1eemp).
pub const CPU_LOAD_BUSY: f64 = 1.0;
const GB: u64 = 1024 * 1024 * 1024;

/// Where the host is read: the real one, or a test's stand-in.
pub trait Probe: Send + Sync {
    /// Free bytes an unprivileged process may write on `path`'s filesystem.
    fn free_bytes(&self, path: &Path) -> std::io::Result<u64>;
    /// `some avg10` of `/proc/pressure/<kind>`, in %; none without PSI.
    fn pressure(&self, kind: &str) -> Option<f64>;
    /// The five-minute load average a core; none when it cannot be read.
    fn cpu_load(&self) -> Option<f64>;
}

/// The host this daemon runs on.
pub struct Host;

impl Probe for Host {
    fn free_bytes(&self, path: &Path) -> std::io::Result<u64> {
        use crate::disk::Probe as _;
        crate::disk::Statvfs.space(path).map(|s| s.free)
    }

    fn pressure(&self, kind: &str) -> Option<f64> {
        theseus_store::pressure::some_avg10_of(kind)
    }

    fn cpu_load(&self) -> Option<f64> {
        let cores = std::thread::available_parallelism().ok()?.get();
        load_per_core(&std::fs::read_to_string("/proc/loadavg").ok()?, cores)
    }
}

/// The five-minute average of `/proc/loadavg`'s text, over `cores`.
fn load_per_core(text: &str, cores: usize) -> Option<f64> {
    let five: f64 = text.split_whitespace().nth(1)?.parse().ok()?;
    (cores > 0 && five.is_finite() && five >= 0.0).then(|| five / cores as f64)
}

/// What the guard read.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Room {
    pub building: usize,
    pub trees: usize,
    pub slots: usize,
    /// Free GB on the trees' filesystem; none when it could not be read.
    pub free_gb: Option<f64>,
    pub min_free_gb: u64,
    pub memory: Option<f64>,
    /// CPU pressure, `some avg10` in %: shown, never held on.
    pub cpu: Option<f64>,
    /// The five-minute load average a core: what the CPU hold reads.
    pub cpu_load: Option<f64>,
}

/// Why a build may not start now.
#[derive(Debug, Clone, PartialEq)]
pub enum Hold {
    Slots,
    Disk,
    Memory,
    Cpu,
}

impl Hold {
    pub fn name(&self) -> &'static str {
        match self {
            Hold::Slots => "slots",
            Hold::Disk => "disk",
            Hold::Memory => "memory",
            Hold::Cpu => "cpu",
        }
    }

    /// The sentence `theseus self pipeline` and the row say.
    pub fn why(&self, r: &Room) -> String {
        match self {
            Hold::Slots => format!(
                "every slot is taken: {} building and {} self trees open, of [self] \
                 pipeline_builds = {}",
                r.building, r.trees, r.slots
            ),
            Hold::Disk => match r.free_gb {
                Some(gb) => format!(
                    "the build trees' disk has {gb:.1} GB free, under [self] min_free_gb = {}",
                    r.min_free_gb
                ),
                None => "the build trees' disk could not be read, so no build starts".into(),
            },
            Hold::Memory => format!(
                "memory pressure is {:.1}% (some avg10), at or over {MEMORY_BUSY}%",
                r.memory.unwrap_or_default()
            ),
            Hold::Cpu => format!(
                "the host has no spare core: {:.2} runnable tasks a core over five minutes \
                 (load average), at or over {CPU_LOAD_BUSY}",
                r.cpu_load.unwrap_or_default()
            ),
        }
    }
}

/// The self trees open under `trees`: one directory each (a `.keel` mark
/// beside one is no tree).
pub fn trees_open(trees: &Path) -> usize {
    std::fs::read_dir(trees)
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .count()
        })
        .unwrap_or(0)
}

/// Whether a build may start: what was read, and the first hold, if any.
/// `at` is a path on the build trees' filesystem (the state dir's `self`,
/// or the state dir before it exists).
pub fn check(
    probe: &dyn Probe,
    at: &Path,
    building: usize,
    trees: usize,
    slots: u32,
    min_free_gb: u64,
) -> (Room, Option<Hold>) {
    let free = probe.free_bytes(at).ok();
    let room = Room {
        building,
        trees,
        slots: slots as usize,
        free_gb: free.map(|b| (b as f64 / GB as f64 * 10.0).floor() / 10.0),
        min_free_gb,
        memory: probe.pressure("memory"),
        cpu: probe.pressure("cpu"),
        cpu_load: probe.cpu_load(),
    };
    let mut holds = Vec::new();
    if building >= room.slots || trees >= room.slots {
        holds.push(Hold::Slots);
    }
    if free.is_none_or(|b| b < min_free_gb.saturating_mul(GB)) {
        holds.push(Hold::Disk);
    }
    if room.memory.is_some_and(|m| m >= MEMORY_BUSY) {
        holds.push(Hold::Memory);
    }
    if room.cpu_load.is_some_and(|l| l >= CPU_LOAD_BUSY) {
        holds.push(Hold::Cpu);
    }
    let first = holds.into_iter().next();
    (room, first)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A host whose disk and pressure a test sets.
    #[derive(Debug)]
    pub(crate) struct Stand {
        pub(crate) free_gb: Mutex<Option<u64>>,
        pub(crate) memory: Mutex<Option<f64>>,
        pub(crate) cpu: Mutex<Option<f64>>,
        /// The five-minute load average a core.
        pub(crate) load: Mutex<Option<f64>>,
    }

    impl Default for Stand {
        fn default() -> Self {
            Self {
                free_gb: Mutex::new(Some(500)),
                memory: Mutex::new(Some(0.0)),
                cpu: Mutex::new(Some(0.0)),
                load: Mutex::new(Some(0.0)),
            }
        }
    }

    impl Probe for Stand {
        fn free_bytes(&self, _path: &Path) -> std::io::Result<u64> {
            self.free_gb
                .lock()
                .unwrap()
                .map(|g| g * GB)
                .ok_or_else(|| std::io::Error::other("no disk"))
        }
        fn pressure(&self, kind: &str) -> Option<f64> {
            match kind {
                "memory" => *self.memory.lock().unwrap(),
                _ => *self.cpu.lock().unwrap(),
            }
        }
        fn cpu_load(&self) -> Option<f64> {
            *self.load.lock().unwrap()
        }
    }

    fn held(s: &Stand, building: usize, trees: usize) -> Option<&'static str> {
        check(s, Path::new("/"), building, trees, 2, 40)
            .1
            .map(|h| h.name())
    }

    /// Each of the four holds on its own, and none on a host with room. A
    /// free slot never skips the host's checks.
    #[test]
    fn each_hold_says_why_and_a_free_slot_still_reads_the_host() {
        let s = Stand::default();
        assert_eq!(held(&s, 0, 0), None);
        assert_eq!(held(&s, 1, 1), None);
        assert_eq!(held(&s, 2, 0), Some("slots"), "two building of two");
        assert_eq!(held(&s, 1, 2), Some("slots"), "two trees of two");
        *s.free_gb.lock().unwrap() = Some(39);
        assert_eq!(held(&s, 0, 0), Some("disk"), "slots free, disk low");
        *s.free_gb.lock().unwrap() = None;
        assert_eq!(held(&s, 0, 0), Some("disk"), "an unread disk holds");
        *s.free_gb.lock().unwrap() = Some(40);
        assert_eq!(held(&s, 0, 0), None, "exactly the line is room");
        *s.memory.lock().unwrap() = Some(12.5);
        assert_eq!(held(&s, 0, 0), Some("memory"));
        *s.memory.lock().unwrap() = None;
        *s.load.lock().unwrap() = Some(1.0);
        assert_eq!(held(&s, 0, 0), Some("cpu"));
        *s.load.lock().unwrap() = None;
        assert_eq!(held(&s, 0, 0), None, "no PSI, no load: nothing holds");
        let (room, hold) = check(&s, Path::new("/"), 2, 3, 2, 40);
        assert_eq!(
            hold.unwrap().why(&room),
            "every slot is taken: 2 building and 3 self trees open, of [self] pipeline_builds = 2"
        );
        *s.free_gb.lock().unwrap() = Some(3);
        let (room, hold) = check(&s, Path::new("/"), 0, 0, 2, 40);
        assert_eq!(
            hold.unwrap().why(&room),
            "the build trees' disk has 3.0 GB free, under [self] min_free_gb = 40"
        );
    }

    /// The two load shapes of 2026-11-11 on the 16-core WSL host (theseus-1eemp;
    /// `b5-psi/` beside the report has the rigs and the logs). What the old
    /// bar, `some avg10` at 60 %, said of each is the second column; the guard
    /// now reads the load, the third.
    ///
    /// - **Eight busy loops pinned to CPUs 0-3, left running 11 h 48 m** by a
    ///   test rig that never killed them: `some avg10` 94.03 and load1 8.48,
    ///   read by the conductor at 05:0x. The old bar shut the guard with
    ///   twelve cores idle. Eight runnable loops pull the load average to 8
    ///   (the five-minute figure below is that, not a reading): 0.5 a core.
    ///   **It starts.**
    /// - **Forty busy loops across all sixteen cores:** `some avg10` 54 to 63
    ///   over six minutes, on both sides of the old bar at a load1 of 33 to
    ///   62, and 55.34 at the minute whose `/proc/loadavg` the line below is
    ///   (the host was already loaded: load5 25.82, 1.61 a core, and 46.51 by
    ///   the sixth minute). No core is spare; **it holds.**
    ///   `full avg10` was 0.00 in every sample of both.
    #[test]
    fn a_pinned_burst_starts_and_a_saturated_host_holds_whatever_some_says() {
        let s = Stand::default();
        *s.cpu.lock().unwrap() = Some(94.03);
        *s.load.lock().unwrap() = load_per_core("8.48 8.00 8.00 9/1446 707318\n", 16);
        assert_eq!(held(&s, 0, 0), None, "12 of 16 cores idle: start");
        *s.cpu.lock().unwrap() = Some(55.34);
        *s.load.lock().unwrap() = load_per_core("41.96 25.82 13.79 43/1480 2723045\n", 16);
        assert_eq!(held(&s, 0, 0), Some("cpu"), "load5 1.61 a core: hold");
        let (room, hold) = check(&s, Path::new("/"), 0, 0, 2, 40);
        assert_eq!(
            hold.unwrap().why(&room),
            "the host has no spare core: 1.61 runnable tasks a core over five minutes \
             (load average), at or over 1"
        );
        // Pressure alone never holds: it is shown, not read.
        *s.cpu.lock().unwrap() = Some(100.0);
        *s.load.lock().unwrap() = Some(0.5);
        assert_eq!(held(&s, 0, 0), None);
    }

    /// The line itself: a load that averages a task a core holds, one under
    /// does not. Four cores, so the figures are whole.
    #[test]
    fn the_line_is_one_runnable_task_a_core() {
        let s = Stand::default();
        for (five, want) in [(3.99, None), (4.0, Some("cpu")), (4.01, Some("cpu"))] {
            *s.load.lock().unwrap() = load_per_core(&format!("0.1 {five} 0.1 1/2 3\n"), 4);
            assert_eq!(held(&s, 0, 0), want, "load5 {five} on 4 cores");
        }
    }

    /// `/proc/loadavg`'s own text, and what is not it.
    #[test]
    fn the_load_is_the_second_field_over_the_cores() {
        assert_eq!(
            load_per_core("5.25 7.63 4.71 3/1446 2715813\n", 16),
            Some(7.63 / 16.0)
        );
        assert_eq!(load_per_core("5.25\n", 16), None, "one field");
        assert_eq!(load_per_core("5.25 nan 4.71 3/1446 1\n", 16), None);
        assert_eq!(load_per_core("5.25 -1 4.71 3/1446 1\n", 16), None);
        assert_eq!(
            load_per_core("5.25 7.63 4.71 3/1446 1\n", 0),
            None,
            "no cores"
        );
        assert_eq!(load_per_core("", 16), None);
    }

    /// The trees are the directories under `trees`, never a keel mark.
    #[test]
    fn the_trees_are_the_directories_and_not_the_marks() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(trees_open(&d.path().join("none")), 0);
        std::fs::create_dir(d.path().join("tsk-a")).unwrap();
        std::fs::create_dir(d.path().join("tsk-b")).unwrap();
        std::fs::write(d.path().join("tsk-c.keel"), "keel").unwrap();
        assert_eq!(trees_open(d.path()), 2);
    }

    /// The real host reads: some disk under `/`, and PSI or none.
    #[test]
    fn the_real_host_reads_its_disk() {
        assert!(Host.free_bytes(Path::new("/")).unwrap() > 0);
        if let Some(c) = Host.pressure("cpu") {
            assert!((0.0..=100.0).contains(&c));
        }
        if let Some(l) = Host.cpu_load() {
            assert!(l >= 0.0, "{l}");
        }
    }
}
