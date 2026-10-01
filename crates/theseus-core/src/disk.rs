//! Free space under the state dir (theseus-102, review 2's R3).
//!
//! On a full disk every append to the store fails, the rows that would say so
//! among them. So health reports the free space of the filesystem that holds
//! the state dir, and says when it is low (`[server] disk_warn_mb`), and a job
//! is refused below a floor (`[server] disk_floor_mb`), with its reason, so the
//! model and the operator see why. The issue had jobs wait below the floor; a
//! wait needs a wake when space comes back, which nothing provides yet, so a
//! job is refused instead.
//!
//! It sees only the filesystem Linux reports (`statvfs`):
//! `theseus_protocol::DiskStatus` says what that misses under WSL.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use theseus_protocol::DiskStatus;

const MB: u64 = 1024 * 1024;

/// A filesystem's space, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    /// What an unprivileged process may still write (`f_bavail`).
    pub free: u64,
    pub total: u64,
}

/// Where the space is read: `statvfs`, or a test's stand-in, so that no test
/// fills a real disk.
pub trait Probe: Send + Sync {
    fn space(&self, path: &Path) -> std::io::Result<Space>;
}

/// The real filesystem's space.
pub struct Statvfs;

impl Probe for Statvfs {
    fn space(&self, path: &Path) -> std::io::Result<Space> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::other("the path holds a NUL byte"))?;
        // SAFETY: an all-zero `statvfs` is a valid value of the plain C
        // struct, and the call fills it from a NUL-terminated path.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let frsize = st.f_frsize as u64;
        Ok(Space {
            free: (st.f_bavail as u64).saturating_mul(frsize),
            total: (st.f_blocks as u64).saturating_mul(frsize),
        })
    }
}

/// A job refused because the disk is below the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub free_mb: u64,
    pub floor_mb: u64,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the disk under the state dir has {} MB free, below the floor of {} MB, so the job \
             was not started",
            crate::narrative::thousands(self.free_mb),
            crate::narrative::thousands(self.floor_mb)
        )
    }
}

/// The state dir's filesystem, with its warning and its floor.
pub struct Disk {
    path: PathBuf,
    warn_mb: u64,
    floor_mb: u64,
    probe: RwLock<Arc<dyn Probe>>,
}

impl Disk {
    /// The filesystem that holds `path`, read through `statvfs`.
    pub fn new(path: PathBuf, warn_mb: u64, floor_mb: u64) -> Self {
        Self {
            path,
            warn_mb,
            floor_mb,
            probe: RwLock::new(Arc::new(Statvfs)),
        }
    }

    /// Read the space from `probe` from now on: a test's stand-in.
    pub fn set_probe(&self, probe: Arc<dyn Probe>) {
        *self.probe.write().unwrap() = probe;
    }

    /// Where the space stands now, for health.
    pub fn status(&self) -> DiskStatus {
        let probe = self.probe.read().unwrap().clone();
        let mut status = DiskStatus {
            path: self.path.display().to_string(),
            warn_mb: self.warn_mb,
            floor_mb: self.floor_mb,
            ..Default::default()
        };
        match probe.space(&self.path) {
            Ok(space) => {
                let free_mb = space.free / MB;
                status.state = if self.floor_mb > 0 && free_mb < self.floor_mb {
                    "below_floor"
                } else if self.warn_mb > 0 && free_mb < self.warn_mb {
                    "low"
                } else {
                    "ok"
                }
                .into();
                status.free_mb = free_mb;
                status.total_mb = space.total / MB;
            }
            Err(e) => {
                status.state = "unknown".into();
                status.error = Some(e.to_string());
            }
        }
        status
    }

    /// Why a job may not start now: the space under the state dir is below
    /// the floor. `None` above it, with no floor, or when the space cannot be
    /// read: that is logged, and no job is refused for it.
    pub fn refusal(&self) -> Option<Refusal> {
        if self.floor_mb == 0 {
            return None;
        }
        let status = self.status();
        match status.state.as_str() {
            "below_floor" => Some(Refusal {
                free_mb: status.free_mb,
                floor_mb: self.floor_mb,
            }),
            "unknown" => {
                tracing::warn!(
                    path = %status.path,
                    error = status.error.as_deref().unwrap_or(""),
                    "the free space under the state dir could not be read; the job starts anyway"
                );
                None
            }
            _ => None,
        }
    }
}

/// A stand-in for `statvfs` that reports what a test sets: no test fills a
/// real disk to see the warning or the floor.
pub struct FixedSpace(std::sync::Mutex<std::io::Result<Space>>);

impl FixedSpace {
    pub fn new(free_mb: u64, total_mb: u64) -> Arc<Self> {
        Arc::new(Self(std::sync::Mutex::new(Ok(Space {
            free: free_mb * MB,
            total: total_mb * MB,
        }))))
    }

    /// Report `free_mb` from now on.
    pub fn set_free_mb(&self, free_mb: u64) {
        let mut s = self.0.lock().unwrap();
        let total = s.as_ref().map_or(free_mb * MB, |s| s.total);
        *s = Ok(Space {
            free: free_mb * MB,
            total,
        });
    }

    /// Fail every read with `why`, as an unreadable filesystem would.
    pub fn fail(&self, why: &str) {
        *self.0.lock().unwrap() = Err(std::io::Error::other(why.to_string()));
    }
}

impl Probe for FixedSpace {
    fn space(&self, _: &Path) -> std::io::Result<Space> {
        match &*self.0.lock().unwrap() {
            Ok(s) => Ok(*s),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Health's free space, its warning, and its floor (theseus-102), read
    /// through a stand-in for `statvfs`: ok above the warning, low under it,
    /// below the floor under that, and unknown when the filesystem cannot be
    /// read. Only below the floor is a job refused, and its reason gives both
    /// numbers.
    #[test]
    fn health_says_ok_low_or_below_the_floor_and_only_the_floor_refuses() {
        let disk = Disk::new(PathBuf::from("/invented/state"), 5120, 1024);
        let fake = FixedSpace::new(80_000, 100_000);
        disk.set_probe(fake.clone());
        let s = disk.status();
        assert_eq!(
            s,
            DiskStatus {
                path: "/invented/state".into(),
                state: "ok".into(),
                free_mb: 80_000,
                total_mb: 100_000,
                warn_mb: 5120,
                floor_mb: 1024,
                error: None,
            }
        );
        assert_eq!(disk.refusal(), None);
        fake.set_free_mb(5119);
        assert_eq!(disk.status().state, "low");
        assert_eq!(disk.refusal(), None, "a warning refuses nothing");
        fake.set_free_mb(1024);
        assert_eq!(disk.status().state, "low", "at the floor is not below it");
        fake.set_free_mb(812);
        assert_eq!(disk.status().state, "below_floor");
        let r = disk.refusal().unwrap();
        assert_eq!(
            r.to_string(),
            "the disk under the state dir has 812 MB free, below the floor of 1,024 MB, so the \
             job was not started"
        );
        fake.fail("invented failure");
        let s = disk.status();
        assert_eq!(s.state, "unknown");
        assert_eq!(s.error.as_deref(), Some("invented failure"));
        assert_eq!(disk.refusal(), None, "an unreadable disk refuses nothing");
    }

    /// 0 turns either off.
    #[test]
    fn zero_never_warns_and_never_refuses() {
        let disk = Disk::new(PathBuf::from("/invented/state"), 0, 0);
        disk.set_probe(FixedSpace::new(1, 100_000));
        assert_eq!(disk.status().state, "ok");
        assert_eq!(disk.refusal(), None);
    }

    /// The real `statvfs` reads this filesystem: some space, of a total at
    /// least as large.
    #[test]
    fn statvfs_reads_a_real_filesystem() {
        let d = tempfile::tempdir().unwrap();
        let s = Statvfs.space(d.path()).unwrap();
        assert!(s.total > 0 && s.free <= s.total, "{s:?}");
        assert!(Statvfs.space(Path::new("/no/such/invented/dir")).is_err());
    }
}
