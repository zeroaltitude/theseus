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
//! 4. **CPU**: CPU pressure (`some avg10` of `/proc/pressure/cpu`) under
//!    [`CPU_BUSY`]. Both are read the way the daemon's background passes read
//!    pressure (`theseus_store::pressure`); without PSI neither holds.
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
/// CPU pressure (`some avg10`, in %) at or over which no build starts. Higher
/// than a background pass's bar (20 %): a build is the work itself, and one
/// compiling already raises it.
pub const CPU_BUSY: f64 = 60.0;
const GB: u64 = 1024 * 1024 * 1024;

/// Where the host is read: the real one, or a test's stand-in.
pub trait Probe: Send + Sync {
    /// Free bytes an unprivileged process may write on `path`'s filesystem.
    fn free_bytes(&self, path: &Path) -> std::io::Result<u64>;
    /// `some avg10` of `/proc/pressure/<kind>`, in %; none without PSI.
    fn pressure(&self, kind: &str) -> Option<f64>;
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
    pub cpu: Option<f64>,
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
                "CPU pressure is {:.1}% (some avg10), at or over {CPU_BUSY}%",
                r.cpu.unwrap_or_default()
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
    if room.cpu.is_some_and(|c| c >= CPU_BUSY) {
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
    }

    impl Default for Stand {
        fn default() -> Self {
            Self {
                free_gb: Mutex::new(Some(500)),
                memory: Mutex::new(Some(0.0)),
                cpu: Mutex::new(Some(0.0)),
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
        *s.cpu.lock().unwrap() = Some(75.0);
        assert_eq!(held(&s, 0, 0), Some("cpu"));
        *s.cpu.lock().unwrap() = None;
        assert_eq!(held(&s, 0, 0), None, "no PSI: neither holds");
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
    }
}
