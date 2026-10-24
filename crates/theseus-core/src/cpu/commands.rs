//! The CPUs a command really has (theseus-yct32). A job runs as the daemon's
//! child (`theseusd job-wrapper`, or L1's namespaces, which have no cgroup
//! of their own), so the cgroup that limits a command is the daemon's own.
//! Inside a container `nproc` counts the host's CPUs while the cgroup's
//! `cpu.max` may allow two, and a `make -j` sized from `nproc` only adds
//! throttling. So the session block names the count once, a stable fact
//! (`compiler/session_block.rs`): the tightest of the cgroup v2 quota (every
//! ancestor's, rounded up), the cpuset, and `available_parallelism`.
//!
//! Read once per process, through a path parameter so the tests use a
//! fixture tree. Nothing here fails: an unreadable or malformed file is no
//! limit, and the count falls back to `available_parallelism`.

use std::path::Path;
use std::sync::OnceLock;

/// What limits the count below the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// The cgroup's `cpu.max`.
    Quota,
    /// The cgroup's `cpuset.cpus.effective`.
    Cpuset,
    /// The process's own mask (`available_parallelism`).
    Affinity,
}

/// The CPUs a command may use, and what the host shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cpus {
    pub usable: usize,
    pub host: usize,
    /// None when nothing limits `usable` below `host`.
    pub limit: Option<Limit>,
}

impl Cpus {
    /// The count under `sys` (`/sys` in production) for the cgroup at `own`
    /// (the v2 path `/proc/self/cgroup` names, `/` in a container), given
    /// `available_parallelism`.
    pub fn read(sys: &Path, own: &str, parallelism: usize) -> Self {
        let parallelism = parallelism.max(1);
        let root = sys.join("fs/cgroup");
        let leaf = root.join(own.trim_start_matches('/'));
        // The tightest quota of the cgroup and its ancestors, and the
        // nearest effective cpuset.
        let (mut quota, mut cpuset) = (None::<usize>, None::<usize>);
        let mut dir = Some(leaf.as_path());
        while let Some(d) = dir.filter(|d| d.starts_with(&root)) {
            if let Some(q) = read(d, "cpu.max").as_deref().and_then(quota_of) {
                quota = Some(quota.map_or(q, |p| p.min(q)));
            }
            if cpuset.is_none() {
                cpuset = read(d, "cpuset.cpus.effective")
                    .as_deref()
                    .and_then(count_of);
            }
            dir = d.parent();
        }
        let host = read(&sys.join("devices/system/cpu"), "online")
            .as_deref()
            .and_then(count_of)
            .unwrap_or(parallelism)
            .max(parallelism);
        let usable = [quota, cpuset, Some(parallelism)]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(parallelism)
            .max(1);
        let limit = (usable < host).then(|| {
            if quota == Some(usable) {
                Limit::Quota
            } else if cpuset == Some(usable) {
                Limit::Cpuset
            } else {
                Limit::Affinity
            }
        });
        Self {
            usable,
            host,
            limit,
        }
    }

    /// This process's, read once.
    pub fn of_commands() -> Self {
        static CPUS: OnceLock<Cpus> = OnceLock::new();
        *CPUS.get_or_init(|| {
            let parallelism = std::thread::available_parallelism().map_or(1, |n| n.get());
            let own = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
            Self::read(Path::new("/sys"), v2_path(&own).unwrap_or("/"), parallelism)
        })
    }

    /// The fact as the model reads it.
    pub fn fact(&self) -> String {
        let why = match self.limit {
            None => return format!("CPUs available to commands: {}.", self.usable),
            Some(Limit::Quota) => "cgroup quota",
            Some(Limit::Cpuset) => "cgroup cpuset",
            Some(Limit::Affinity) => "CPU affinity",
        };
        format!(
            "CPUs available to commands: {} ({why}; the host shows {}).",
            self.usable, self.host
        )
    }
}

fn read(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok()
}

/// The cgroup v2 line's path (`0::/a/b`), when there is one.
fn v2_path(proc_cgroup: &str) -> Option<&str> {
    proc_cgroup.lines().find_map(|l| l.strip_prefix("0::"))
}

/// `"<quota> <period>"` as whole CPUs, rounded up; `"max <period>"` is none.
fn quota_of(text: &str) -> Option<usize> {
    let mut it = text.split_whitespace();
    let quota: u64 = it.next()?.parse().ok()?;
    let period: u64 = it.next().map_or(Some(100_000), |p| p.parse().ok())?;
    (period > 0).then(|| usize::try_from(quota.div_ceil(period).max(1)).unwrap_or(usize::MAX))
}

/// A CPU list (`0-3,8,10-11`) as its count; none when empty or malformed.
fn count_of(text: &str) -> Option<usize> {
    let mut n = 0usize;
    for part in text.trim().split(',').filter(|p| !p.is_empty()) {
        n += match part.split_once('-') {
            Some((a, b)) => {
                let (a, b): (usize, usize) = (a.parse().ok()?, b.parse().ok()?);
                b.checked_sub(a)? + 1
            }
            None => part.parse::<usize>().map(|_| 1).ok()?,
        };
    }
    (n > 0).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture `/sys`: the cgroup tree's files at their paths, and the
    /// host's online CPUs.
    fn sys(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in files {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    #[test]
    fn a_quota_of_two_periods_is_two_cpus_of_the_hosts_thirty_two() {
        let d = sys(&[
            ("fs/cgroup/cpu.max", "200000 100000\n"),
            ("fs/cgroup/cpuset.cpus.effective", "0-31\n"),
            ("devices/system/cpu/online", "0-31\n"),
        ]);
        let c = Cpus::read(d.path(), "/", 32);
        assert_eq!(
            c,
            Cpus {
                usable: 2,
                host: 32,
                limit: Some(Limit::Quota)
            }
        );
        assert_eq!(
            c.fact(),
            "CPUs available to commands: 2 (cgroup quota; the host shows 32)."
        );
        // A part of a CPU rounds up: 1.5 CPUs is 2.
        let d = sys(&[("fs/cgroup/cpu.max", "150000 100000")]);
        assert_eq!(Cpus::read(d.path(), "/", 8).usable, 2);
    }

    #[test]
    fn no_quota_with_a_cpuset_of_four_is_four() {
        let d = sys(&[
            ("fs/cgroup/cpu.max", "max 100000\n"),
            ("fs/cgroup/cpuset.cpus.effective", "0-3\n"),
            ("devices/system/cpu/online", "0-15\n"),
        ]);
        let c = Cpus::read(d.path(), "/", 16);
        assert_eq!(
            c,
            Cpus {
                usable: 4,
                host: 16,
                limit: Some(Limit::Cpuset)
            }
        );
        assert_eq!(
            c.fact(),
            "CPUs available to commands: 4 (cgroup cpuset; the host shows 16)."
        );
    }

    /// The daemon's own cgroup, nested: an ancestor's quota binds a child
    /// that sets none, and the tightest of them wins.
    #[test]
    fn an_ancestors_quota_binds_the_cgroup_below_it() {
        let d = sys(&[
            ("fs/cgroup/cpu.max", "max 100000"),
            ("fs/cgroup/box/cpu.max", "300000 100000"),
            ("fs/cgroup/box/theseus/cpu.max", "max 100000"),
            ("fs/cgroup/box/theseus/cpuset.cpus.effective", "0-7"),
            ("devices/system/cpu/online", "0-7"),
        ]);
        let c = Cpus::read(d.path(), "/box/theseus", 8);
        assert_eq!((c.usable, c.limit), (3, Some(Limit::Quota)));
    }

    #[test]
    fn unreadable_files_give_available_parallelism() {
        let d = tempfile::tempdir().unwrap();
        let c = Cpus::read(&d.path().join("absent"), "/", 6);
        assert_eq!(
            c,
            Cpus {
                usable: 6,
                host: 6,
                limit: None
            }
        );
        assert_eq!(c.fact(), "CPUs available to commands: 6.");
        // Malformed files are no limit either.
        let d = sys(&[
            ("fs/cgroup/cpu.max", "lots\n"),
            ("fs/cgroup/cpuset.cpus.effective", "3-1\n"),
            ("devices/system/cpu/online", "\n"),
        ]);
        assert_eq!(Cpus::read(d.path(), "/", 6).usable, 6);
        // A mask below the host's is said as the process's own.
        let d = sys(&[("devices/system/cpu/online", "0-11")]);
        let c = Cpus::read(d.path(), "/", 4);
        assert_eq!((c.usable, c.host, c.limit), (4, 12, Some(Limit::Affinity)));
    }

    #[test]
    fn the_v2_line_names_the_cgroup() {
        assert_eq!(
            v2_path("12:cpu:/x\n0::/user.slice/theseus\n"),
            Some("/user.slice/theseus")
        );
        assert_eq!(v2_path("4:memory:/x\n"), None);
        assert_eq!(count_of("0-3,8,10-11"), Some(7));
        assert_eq!(quota_of("max 100000"), None);
        assert_eq!(quota_of("50000 100000"), Some(1));
    }
}
