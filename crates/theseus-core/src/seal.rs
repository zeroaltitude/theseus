//! The seal probe (theseus-ouqru, design §3): whether this machine is a sealed
//! benchmark container, one whose whole filesystem is a task's public image
//! and to which nothing of a host reaches. A program that runs on a sealed
//! machine can read nothing private, so the tool screen takes the seal as the
//! provenance of its read reach (`data::provenance`).
//!
//! It is read from the kernel's own mount table, `/proc/self/mountinfo`, and
//! from nothing else. There is no config key, flag or environment variable
//! that turns it on, so neither a typo nor a model's input can claim it. The
//! owner's own daemon fails at the first rule: its root is a disk filesystem,
//! not an overlay.
//!
//! `judge` returns [`Seal::Sealed`] only when
//!
//! 1. the mount at `/` is an `overlay`: a container image, not a host root;
//! 2. every other mount is of a type in [`ALLOWED`], or is one of the
//!    [`BOUND`] mount points a benchmark container is given by its harness
//!    (the trial's own log directories and the three files a container
//!    runtime writes), bound from a path below its filesystem's root; and
//! 3. the table was readable and every line of it parsed.
//!
//! Anything else is [`Seal::Unsealed`], naming the mount point and the type:
//! an unknown filesystem type, an unreadable table, a line that does not
//! parse. The allowed set is closed, so a type nobody thought of refuses.
//!
//! **What a seal proves, and what it does not.** It proves an **overlay root
//! and no mount of a disk or network filesystem outside the [`BOUND`] mount
//! points**. That is less than "no host filesystem is reachable", and the
//! difference is three things, each of which belongs to whoever launches the
//! container — never to a model, a config key, or the owner's daemon:
//!
//! - A bind carries its **source superblock's** type, so a host directory
//!   that itself lives on a `tmpfs`, an `overlay` or a `ramfs` — a host's
//!   `/run`, or a directory of a host overlay under a nested runtime —
//!   appears as an allowed type at an arbitrary mount point and is admitted.
//!   A table cannot tell a container's own fresh `tmpfs` from a bind of the
//!   host's. Tightening that without refusing real container tables is
//!   `theseus-otpm1` and not a line here: `/proc/bus`, and a `/proc/kcore`
//!   masked from `/dev`, are allowed mounts bound from a subtree in every
//!   Docker table.
//! - [`BOUND`] admits **any** source path at its six mount points, at any
//!   type, constrained only to a bind of a subtree rather than a whole
//!   filesystem. This list is the one place that trust is written down.
//! - It does not prove that whoever built the image put nothing private in
//!   it; for a benchmark that premise is the owner's (a public task's image
//!   plus an install).
//!
//! So a verdict is worth only the record of what it admitted, and
//! [`Seal::Sealed`] carries the [`BOUND`] mounts it trusted, with the source
//! root of each, through [`Seal::reason`] into the ledger.

use std::sync::OnceLock;

/// Mount types a sealed container's table may hold besides its overlay root:
/// memory- and kernel-backed filesystems, none of which is a host's disk.
pub const ALLOWED: [&str; 21] = [
    "overlay",
    "tmpfs",
    "ramfs",
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "mqueue",
    "shm",
    "cgroup",
    "cgroup2",
    "securityfs",
    "bpf",
    "tracefs",
    "debugfs",
    "pstore",
    "configfs",
    "fusectl",
    "binfmt_misc",
    "nsfs",
    "hugetlbfs",
];

/// Mount points a benchmark container is given by its harness, of any type
/// (a bind of a host path shows the host's type): the trial's own output
/// directories (Harbor's `/logs/*`) and the files a container runtime writes
/// into every container. Each must be bound from below its filesystem's root.
pub const BOUND: [&str; 6] = [
    "/logs/agent",
    "/logs/artifacts",
    "/logs/verifier",
    "/etc/resolv.conf",
    "/etc/hostname",
    "/etc/hosts",
];

/// A machine's seal verdict, each with the words a ledger row says it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seal {
    /// Sealed, and **what it admitted**: every [`BOUND`] mount point the
    /// table held, with the source root of each. A verdict is worth only
    /// that record, since those six points are admitted at any type and from
    /// any source (the module's own limits).
    Sealed(String),
    /// Not sealed, and why: the first mount that broke a rule, with its type,
    /// or what could not be read.
    Unsealed(String),
}

impl Seal {
    pub fn is_sealed(&self) -> bool {
        matches!(self, Seal::Sealed(_))
    }

    /// The verdict's reason as a row says it.
    pub fn reason(&self) -> &str {
        match self {
            Seal::Sealed(why) | Seal::Unsealed(why) => why,
        }
    }

    /// A sealed verdict for a test, trusting no harness mount point.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::sealed(&[])
    }

    /// `Sealed`, with the harness mount points it trusted.
    fn sealed(trusted: &[String]) -> Self {
        let head = "overlay root, no disk or network filesystem mounted";
        Seal::Sealed(match trusted.is_empty() {
            true => format!("{head}, and no harness mount point"),
            false => format!("{head} outside {}", trusted.join(", ")),
        })
    }
}

/// One mount table line: its root within its filesystem, its mount point
/// (unescaped) and its type.
struct Mount {
    root: String,
    point: String,
    fstype: String,
}

/// `\040` and its kind: the kernel escapes a space, tab, newline and
/// backslash in a path as three octal digits.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let digits = b
            .get(i + 1..i + 4)
            .filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)));
        match (b[i], digits) {
            (b'\\', Some(d)) => {
                let n = d.iter().fold(0u32, |n, c| n * 8 + u32::from(c - b'0'));
                out.push(n as u8);
                i += 4;
            }
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `36 35 98:0 /root /point opts [optional...] - fstype source superopts`.
fn parse(line: &str) -> Option<Mount> {
    let f: Vec<&str> = line.split_whitespace().collect();
    // Six fixed fields, then the optional ones up to the `-`.
    let dash = f.iter().skip(6).position(|w| *w == "-")? + 6;
    let fstype = f.get(dash + 1)?;
    f.get(2)?.split_once(':')?;
    Some(Mount {
        root: unescape(f[3]),
        point: unescape(f[4]),
        fstype: (*fstype).to_string(),
    })
}

/// The verdict for a `/proc/self/mountinfo`'s text. Read as a string, so a
/// test supplies a table without touching `/proc`.
pub fn judge(mountinfo: &str) -> Seal {
    let mut mounts = Vec::new();
    for (i, line) in mountinfo.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match parse(line) {
            Some(m) => mounts.push(m),
            None => return Seal::Unsealed(format!("mount table line {} did not parse", i + 1)),
        }
    }
    // Mounts stack: the last one at `/` is the root a process sees.
    let Some(root) = mounts.iter().rfind(|m| m.point == "/") else {
        return Seal::Unsealed("the mount table has no root".into());
    };
    if root.fstype != "overlay" {
        return Seal::Unsealed(format!("/ is {}, not an overlay", root.fstype));
    }
    // Every harness mount point the table holds is written down, whichever
    // rule admitted it: those are the points admitted at any type and from
    // any source, so the verdict is worth only this record of them.
    let mut trusted = Vec::new();
    for m in mounts.iter().filter(|m| m.point != "/") {
        let allowed = ALLOWED.contains(&m.fstype.as_str());
        let listed = BOUND.contains(&m.point.as_str());
        if !(allowed || (listed && m.root != "/")) {
            return Seal::Unsealed(format!("{} is {}, a host's", m.point, m.fstype));
        }
        if listed {
            trusted.push(format!("{} from {}", m.point, m.root));
        }
    }
    Seal::sealed(&trusted)
}

/// A debug build's plant: `THESEUS_TEST_MOUNTINFO`, a file holding a mount
/// table or the table itself, stands in for the kernel's, so a live check can
/// make a scratch daemon sealed or not (written as
/// `THESEUS_TEST_L1_WAIT_DELAY_MS` is). A release build has no such variable.
#[cfg(debug_assertions)]
fn planted() -> Option<String> {
    std::env::var("THESEUS_TEST_MOUNTINFO").ok()
}

#[cfg(not(debug_assertions))]
fn planted() -> Option<String> {
    None
}

/// The verdict for the plant's value `plant` if there is one, else this
/// machine's own table, or unsealed when that cannot be read.
fn probe_from(plant: Option<String>) -> Seal {
    if let Some(v) = plant {
        let table = match v.starts_with('/') && !v.contains('\n') {
            true => std::fs::read_to_string(&v).unwrap_or_default(),
            false => v,
        };
        return judge(&table);
    }
    match std::fs::read_to_string("/proc/self/mountinfo") {
        Ok(t) => judge(&t),
        Err(e) => Seal::Unsealed(format!("/proc/self/mountinfo was not readable: {e}")),
    }
}

/// The daemon's verdict, read once.
pub fn of() -> &'static Seal {
    static SEAL: OnceLock<Seal> = OnceLock::new();
    SEAL.get_or_init(|| probe_from(planted()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A container table as Docker writes one: an overlay root, the usual
    /// kernel mounts, and the trial's log directories and the runtime's
    /// three files bound from the host's disk.
    const DOCKER: &str = "\
1383 1258 0:155 / / rw,relatime - overlay overlay rw,lowerdir=/l/a:/l/b,upperdir=/u/diff,workdir=/u/work
1385 1383 0:191 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
1386 1383 0:192 / /dev rw,nosuid - tmpfs tmpfs rw,size=65536k,mode=755
1387 1386 0:193 / /dev/pts rw,nosuid,noexec,relatime - devpts devpts rw,gid=5,mode=620
1388 1383 0:194 / /sys ro,nosuid,nodev,noexec,relatime - sysfs sysfs ro
1389 1388 0:25 / /sys/fs/cgroup ro,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw,nsdelegate
1390 1386 0:189 / /dev/mqueue rw,nosuid,nodev,noexec,relatime - mqueue mqueue rw
1391 1386 0:195 / /dev/shm rw,nosuid,nodev,noexec,relatime - tmpfs shm rw,size=65536k
1392 1383 8:80 /srv/runs/job/trial/agent /logs/agent rw,relatime - ext4 /dev/sdf rw,discard
1393 1383 8:80 /srv/runs/job/trial/artifacts /logs/artifacts rw,relatime - ext4 /dev/sdf rw,discard
1394 1383 8:80 /srv/runs/job/trial/verifier /logs/verifier rw,relatime - ext4 /dev/sdf rw,discard
1395 1383 8:80 /var/lib/docker/containers/c1/resolv.conf /etc/resolv.conf rw,relatime - ext4 /dev/sdf rw
1396 1383 8:80 /var/lib/docker/containers/c1/hostname /etc/hostname rw,relatime - ext4 /dev/sdf rw
1397 1383 8:80 /var/lib/docker/containers/c1/hosts /etc/hosts rw,relatime - ext4 /dev/sdf rw
1271 1385 0:191 /bus /proc/bus ro,nosuid,nodev,noexec,relatime - proc proc rw
1277 1385 0:192 /null /proc/kcore rw,nosuid - tmpfs tmpfs rw,size=65536k,mode=755
";

    fn plus(line: &str) -> String {
        format!("{DOCKER}{line}\n")
    }

    /// Sealed, and the verdict names every harness mount point it admitted
    /// with the source root of each: those six are trusted at any type and
    /// from any source, so a reader of the ledger must see which they were.
    #[test]
    fn a_docker_shaped_table_is_sealed_and_names_what_it_trusted() {
        let v = judge(DOCKER);
        assert!(v.is_sealed(), "{v:?}");
        let why = v.reason();
        assert!(
            why.starts_with("overlay root, no disk or network filesystem"),
            "{why}"
        );
        for p in BOUND {
            assert!(why.contains(p), "{p} is in the table and not in {why}");
        }
        assert!(
            why.contains("/logs/agent from /srv/runs/job/trial/agent"),
            "{why}"
        );
        assert!(
            why.contains("/etc/hosts from /var/lib/docker/containers/c1/hosts"),
            "{why}"
        );
        // A table with no harness mount point says so rather than nothing.
        let bare = "1 0 0:9 / / rw - overlay overlay rw\n2 1 0:5 / /proc rw - proc proc rw\n";
        let v = judge(bare);
        assert!(
            v.is_sealed() && v.reason().ends_with("no harness mount point"),
            "{v:?}"
        );
    }

    /// `docker run -v /home:/home`: a host disk at a mount point nobody listed.
    #[test]
    fn a_host_directory_bound_in_is_unsealed_and_named() {
        let t = plus("1400 1383 8:80 /home/u /home/u rw,relatime - ext4 /dev/sdf rw");
        let Seal::Unsealed(why) = judge(&t) else {
            panic!("sealed")
        };
        assert!(why.contains("/home/u") && why.contains("ext4"), "{why}");
    }

    /// The list is exact: a bind at a path beside a listed one, or below it,
    /// is a host directory the harness did not give.
    #[test]
    fn only_the_listed_mount_points_may_be_bound() {
        for p in [
            "/logs",
            "/logs/agent/more",
            "/logs/other",
            "/etc",
            "/etc/passwd",
        ] {
            let t = plus(&format!(
                "1400 1383 8:80 /srv/x {p} rw,relatime - ext4 /dev/sdf rw"
            ));
            assert!(!judge(&t).is_sealed(), "{p}");
        }
    }

    /// A listed mount point bound from the filesystem's own root is the whole
    /// disk, not a directory of it.
    #[test]
    fn a_listed_mount_point_bound_from_a_whole_disk_is_unsealed() {
        let t = plus("1400 1383 8:80 / /logs/agent rw,relatime - ext4 /dev/sdf rw");
        assert!(!judge(&t).is_sealed());
    }

    #[test]
    fn an_ext4_root_is_unsealed() {
        let t = "\
1 0 8:1 / / rw,relatime - ext4 /dev/sda1 rw
2 1 0:5 / /proc rw - proc proc rw
";
        let Seal::Unsealed(why) = judge(t) else {
            panic!("sealed")
        };
        assert!(why.contains("ext4") && why.contains('/'), "{why}");
    }

    /// Every disk or network type a host may mount, and one nobody listed.
    #[test]
    fn a_type_the_set_does_not_name_is_unsealed() {
        for ty in [
            "xfs",
            "btrfs",
            "zfs",
            "f2fs",
            "squashfs",
            "9p",
            "virtiofs",
            "nfs4",
            "cifs",
            "fuse.sshfs",
            "vboxsf",
            "futurefs",
        ] {
            let t = plus(&format!("1400 1383 0:99 / /data rw - {ty} src rw"));
            assert!(!judge(&t).is_sealed(), "{ty}");
        }
    }

    #[test]
    fn a_table_that_does_not_parse_is_unsealed() {
        for bad in [
            "garbage",
            "1 0 8:1 / / rw,relatime",
            "1 0 nocolon / / rw - overlay overlay rw",
            "1 0 0:1 / /x rw - ",
        ] {
            let t = format!("{DOCKER}{bad}\n");
            let Seal::Unsealed(why) = judge(&t) else {
                panic!("sealed: {bad}")
            };
            assert!(why.contains("did not parse"), "{bad}: {why}");
        }
        assert!(!judge("").is_sealed(), "an empty table has no root");
    }

    /// Mounts stack: the root a process sees is the last at `/`.
    #[test]
    fn the_last_mount_at_the_root_is_the_root() {
        let t = "\
1 0 8:1 / / rw - ext4 /dev/sda1 rw
2 1 0:9 / / rw - overlay overlay rw
";
        assert!(judge(t).is_sealed());
        let t = "\
2 1 0:9 / / rw - overlay overlay rw
1 0 8:1 / / rw - ext4 /dev/sda1 rw
";
        assert!(!judge(t).is_sealed());
    }

    #[test]
    fn a_mount_point_is_unescaped_before_it_is_compared() {
        assert_eq!(unescape(r"/a\040b"), "/a b");
        let t = plus(r"1400 1383 8:80 /srv/x /logs/agent\040x rw - ext4 /dev/sdf rw");
        assert!(!judge(&t).is_sealed());
    }

    /// The plant: a table given as text, or as a file, replaces the kernel's;
    /// a file that cannot be read is an empty table, which is unsealed.
    #[test]
    fn a_planted_table_stands_in_for_the_kernels() {
        assert!(probe_from(Some(DOCKER.to_string())).is_sealed());
        assert!(!probe_from(Some("1 0 8:1 / / rw - ext4 /dev/sda1 rw".into())).is_sealed());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mountinfo");
        std::fs::write(&file, DOCKER).unwrap();
        assert!(probe_from(Some(file.display().to_string())).is_sealed());
        let missing = dir.path().join("none").display().to_string();
        assert!(!probe_from(Some(missing)).is_sealed());
        // With no plant it reads this machine's own table and never panics.
        let _ = probe_from(None);
    }

    /// The reason is a readable sentence for the ledger.
    #[test]
    fn the_verdict_says_why() {
        assert!(Seal::for_test().reason().contains("overlay"));
        assert!(Seal::Unsealed("x".into()).reason() == "x");
    }
}
