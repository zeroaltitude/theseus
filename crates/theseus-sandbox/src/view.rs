//! The view a job sees (design §2.2's table), built by the init from an
//! empty root:
//!
//! - `/` is a fresh tmpfs, entered by `pivot_root`, and read-only once built;
//! - the system (`/usr`, `/bin`, `/sbin`, `/lib*`, `/etc`) and `ro_paths` are
//!   read-only binds, with symlinks where the host has them;
//! - the workspace roots are at their own paths, read-only under overlays
//!   whose upper directories are one capped tmpfs: scratch;
//! - HOME, `/tmp`, and `/dev/shm` are empty capped tmpfs mounts;
//! - `/dev` holds `null`, `zero`, `random`, and `urandom`, the fd links, and
//!   `shm`;
//! - `/proc` is a new proc for the job's pid namespace, and `/sys` a fresh
//!   sysfs for its network namespace, each masked as Docker masks them.
//!
//! The host's root stays at `/.oldroot` while the view is built from it, and
//! goes before the command starts. Scratch is hidden too: the init keeps a
//! descriptor to it, to report what was written.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::spec::Spec;
use crate::sys::{self, cpath, Failure, Stage};
use crate::sys::{MOUNT_ATTR_NODEV, MOUNT_ATTR_NOEXEC, MOUNT_ATTR_NOSUID, MOUNT_ATTR_RDONLY};

/// The system's directories, bound read-only when the host has them.
const SYSTEM: [&str; 8] = [
    "/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/etc",
];

/// The device nodes a job gets, bound from the host's.
pub const DEVICES: [&str; 4] = ["null", "zero", "random", "urandom"];

/// Docker's masks of `/proc`: covered (a file by `/dev/null`, a directory by
/// an empty read-only tmpfs), and read-only.
const PROC_MASKED: [&str; 10] = [
    "asound",
    "acpi",
    "interrupts",
    "kcore",
    "keys",
    "latency_stats",
    "timer_list",
    "timer_stats",
    "sched_debug",
    "scsi",
];
const PROC_READONLY: [&str; 5] = ["bus", "fs", "irq", "sys", "sysrq-trigger"];
/// `/sys`'s masks: the design's three, and Docker's powercap.
const SYS_MASKED: [&str; 4] = [
    "firmware",
    "kernel/security",
    "fs/cgroup",
    "devices/virtual/powercap",
];

const OLD: &str = "/.oldroot";
const SCRATCH: &str = "/.scratch";

/// What building the view left the init.
pub(crate) struct View {
    /// Scratch's root (one `<i>/upper` per workspace root), hidden from the
    /// job, kept for the summary. None without a workspace.
    pub scratch: Option<OwnedFd>,
    /// A fresh sysfs is at `/sys`.
    pub sys: bool,
}

fn c(p: impl AsRef<Path>) -> io::Result<CString> {
    cpath(p.as_ref())
}

/// The host's path `p`, under the old root.
fn host(p: &Path) -> PathBuf {
    Path::new(OLD).join(p.strip_prefix("/").unwrap_or(p))
}

pub(crate) fn build(spec: &Spec) -> Result<View, Failure> {
    // Nothing done here propagates back to the host.
    sys::mount(None, c"/", None, libc::MS_REC | libc::MS_PRIVATE, None)
        .stage("making the mounts private")?;
    // A new proc or sysfs must keep the host's atime setting, or the kernel
    // refuses it as too revealing.
    let proc_atime = atime_flags("/proc");
    let sys_atime = atime_flags("/sys");

    // The new root: a fresh tmpfs on /tmp, then pivot_root into it. It
    // holds only mount points, and is read-only once the view is built.
    tmpfs("/tmp", 0o755, Some(1), 0).stage("mounting the new root")?;
    mkdir("/tmp/.oldroot", 0o700).stage("making the old root's mount point")?;
    sys::pivot_root(c"/tmp", c"/tmp/.oldroot").stage("pivot_root")?;
    std::env::set_current_dir("/").stage("entering the new root")?;

    for p in SYSTEM {
        system(p).stage(format!("binding {p}"))?;
    }
    dev(spec).stage("building /dev")?;
    // Before the old root goes: the kernel mounts a new proc or sysfs only
    // while the host's is still in this mount namespace.
    proc(proc_atime)?;
    let sys = sysfs(sys_atime)?;
    mkdir("/tmp", 0o755).stage("making /tmp")?;
    tmpfs("/tmp", 0o1777, Some(spec.limits.tmp_mb), 0).stage("mounting /tmp")?;

    let scratch = if spec.workspace.is_empty() {
        None
    } else {
        mkdir(SCRATCH, 0o700).stage("making scratch")?;
        tmpfs(SCRATCH, 0o755, Some(spec.limits.scratch_mb), 0).stage("mounting scratch")?;
        Some(())
    };
    for (path, what) in plan(spec) {
        match what {
            Part::Home => mkdir_all(&path)
                .and_then(|()| tmpfs(&path, 0o700, Some(spec.limits.tmp_mb), 0))
                .stage(format!("mounting HOME at {}", path.display()))?,
            Part::ReadOnly => bind_ro(&host(&path), &path)
                .stage(format!("binding {} read-only", path.display()))?,
            Part::Workspace(i) => overlay(&host(&path), &path, i)
                .stage(format!("mounting the workspace {}", path.display()))?,
        }
    }

    // The host's root goes, and scratch is hidden: the init keeps it open.
    sys::umount2(c"/.oldroot", libc::MNT_DETACH).stage("detaching the old root")?;
    fs::remove_dir(OLD).stage("removing the old root's mount point")?;
    let scratch = match scratch {
        Some(()) => {
            let fd = open_path(SCRATCH).stage("opening scratch")?;
            sys::umount2(c"/.scratch", libc::MNT_DETACH).stage("hiding scratch")?;
            fs::remove_dir(SCRATCH).stage("removing scratch's mount point")?;
            Some(fd)
        }
        None => None,
    };
    // The root itself read-only: a write outside scratch, /tmp, and HOME is
    // EROFS.
    sys::mount_setattr(
        c"/",
        false,
        MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV,
    )
    .stage("making the root read-only")?;
    std::env::set_current_dir("/").stage("entering the view")?;
    Ok(View { scratch, sys })
}

enum Part {
    Home,
    ReadOnly,
    Workspace(usize),
}

/// The operator's part of the view, parents before children, so that a
/// read-only path or a workspace under HOME lands inside HOME's tmpfs.
fn plan(spec: &Spec) -> Vec<(PathBuf, Part)> {
    let mut v: Vec<(PathBuf, Part)> = Vec::new();
    v.extend(spec.home.iter().map(|h| (h.clone(), Part::Home)));
    v.extend(spec.ro_paths.iter().map(|p| (p.clone(), Part::ReadOnly)));
    v.extend(
        spec.workspace
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), Part::Workspace(i))),
    );
    // Stable: at one depth, HOME first, then the binds, then the overlays.
    v.sort_by_key(|(p, _)| p.components().count());
    v
}

fn mkdir(p: impl AsRef<Path>, mode: u32) -> io::Result<()> {
    match fs::DirBuilder::new().mode(mode).create(p.as_ref()) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        r => r,
    }
}

/// Every missing directory of `p`, made in the view (0755).
fn mkdir_all(p: &Path) -> io::Result<()> {
    if p.is_dir() {
        return Ok(());
    }
    fs::DirBuilder::new().recursive(true).mode(0o755).create(p)
}

/// An empty file to mount a file over.
fn touch(p: &Path) -> io::Result<()> {
    if let Some(parent) = p.parent() {
        mkdir_all(parent)?;
    }
    if p.exists() {
        return Ok(());
    }
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(p)
        .map(drop)
}

fn tmpfs(
    target: impl AsRef<Path>,
    mode: u32,
    size_mb: Option<u64>,
    flags: libc::c_ulong,
) -> io::Result<()> {
    let mut data = format!("mode={mode:o}");
    if let Some(mb) = size_mb {
        data.push_str(&format!(",size={}k", mb.max(1) * 1024));
    }
    sys::mount(
        Some(c"tmpfs"),
        &c(target)?,
        Some(c"tmpfs"),
        libc::MS_NOSUID | libc::MS_NODEV | flags,
        Some(&CString::new(data)?),
    )
}

/// A recursive bind of `source` at `target`, then read-only, nosuid, and
/// nodev all the way down. Recursive, since the kernel refuses to bind one
/// mount of the host's alone when others are locked above it.
fn bind_ro(source: &Path, target: &Path) -> io::Result<()> {
    if fs::metadata(source)?.is_dir() {
        mkdir_all(target)?;
    } else {
        touch(target)?;
    }
    let t = c(target)?;
    sys::mount(
        Some(&c(source)?),
        &t,
        None,
        libc::MS_BIND | libc::MS_REC,
        None,
    )?;
    sys::mount_setattr(
        &t,
        true,
        MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV,
    )
}

/// One of the system's directories: a read-only bind, or the same symlink
/// the host has (`/bin -> usr/bin` on a merged-usr system), or nothing.
fn system(p: &str) -> io::Result<()> {
    let source = host(Path::new(p));
    match fs::symlink_metadata(&source) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.file_type().is_symlink() => {
            std::os::unix::fs::symlink(fs::read_link(&source)?, p)
        }
        Ok(_) => bind_ro(&source, Path::new(p)),
    }
}

fn dev(spec: &Spec) -> io::Result<()> {
    mkdir("/dev", 0o755)?;
    tmpfs("/dev", 0o755, Some(1), 0)?;
    for name in DEVICES {
        let target = PathBuf::from("/dev").join(name);
        touch(&target)?;
        let t = c(&target)?;
        sys::mount(Some(&c(host(&target))?), &t, None, libc::MS_BIND, None)?;
        // Never nodev: these are the devices.
        sys::mount_setattr(&t, false, MOUNT_ATTR_NOSUID | MOUNT_ATTR_NOEXEC)?;
    }
    for (link, to) in [
        ("fd", "/proc/self/fd"),
        ("stdin", "/proc/self/fd/0"),
        ("stdout", "/proc/self/fd/1"),
        ("stderr", "/proc/self/fd/2"),
    ] {
        std::os::unix::fs::symlink(to, Path::new("/dev").join(link))?;
    }
    mkdir("/dev/shm", 0o755)?;
    tmpfs("/dev/shm", 0o1777, Some(spec.limits.tmp_mb), 0)?;
    // Nothing more is made in /dev.
    sys::mount_setattr(&c("/dev")?, false, MOUNT_ATTR_RDONLY)
}

fn proc(atime: libc::c_ulong) -> Result<(), Failure> {
    mkdir("/proc", 0o555).stage("making /proc")?;
    sys::mount(
        Some(c"proc"),
        c"/proc",
        Some(c"proc"),
        libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC | atime,
        None,
    )
    .stage("mounting /proc")?;
    for name in PROC_MASKED {
        mask(&Path::new("/proc").join(name)).stage(format!("masking /proc/{name}"))?;
    }
    for name in PROC_READONLY {
        let p = Path::new("/proc").join(name);
        if fs::symlink_metadata(&p).is_err() {
            continue;
        }
        let t = c(&p).stage("a /proc path")?;
        sys::mount(Some(&t), &t, None, libc::MS_BIND | libc::MS_REC, None)
            .and_then(|()| {
                sys::mount_setattr(
                    &t,
                    true,
                    MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV | MOUNT_ATTR_NOEXEC,
                )
            })
            .stage(format!("making /proc/{name} read-only"))?;
    }
    Ok(())
}

/// A fresh sysfs, read-only, for the job's own network namespace. False if
/// the kernel refuses it: the view then has no `/sys`.
fn sysfs(atime: libc::c_ulong) -> Result<bool, Failure> {
    mkdir("/sys", 0o555).stage("making /sys")?;
    let mounted = sys::mount(
        Some(c"sysfs"),
        c"/sys",
        Some(c"sysfs"),
        libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC | atime,
        None,
    );
    match mounted {
        Err(e) if e.raw_os_error() == Some(libc::EPERM) => return Ok(false),
        r => r.stage("mounting /sys")?,
    }
    for name in SYS_MASKED {
        mask(&Path::new("/sys").join(name)).stage(format!("masking /sys/{name}"))?;
    }
    Ok(true)
}

/// Covers `p`: a file with `/dev/null`, a directory with an empty read-only
/// tmpfs. A path this kernel lacks is skipped.
fn mask(p: &Path) -> io::Result<()> {
    let m = match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        r => r?,
    };
    if m.is_dir() {
        tmpfs(p, 0o555, Some(1), libc::MS_RDONLY | libc::MS_NOEXEC)
    } else {
        sys::mount(Some(c"/dev/null"), &c(p)?, None, libc::MS_BIND, None)
    }
}

/// A workspace root: the host's tree (`lower`) read-only under an overlay
/// at `target`, its writes going to scratch's `<i>/upper`.
fn overlay(lower: &Path, target: &Path, i: usize) -> io::Result<()> {
    if !fs::metadata(lower)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a workspace root must be a directory",
        ));
    }
    let base = Path::new(SCRATCH).join(i.to_string());
    let (upper, work) = (base.join("upper"), base.join("work"));
    for d in [&base, &upper, &work] {
        mkdir(d, 0o755)?;
    }
    mkdir_all(target)?;
    let data = format!(
        "lowerdir={},upperdir={},workdir={},userxattr",
        escape(lower),
        escape(&upper),
        escape(&work)
    );
    sys::mount(
        Some(c"overlay"),
        &c(target)?,
        Some(c"overlay"),
        libc::MS_NOSUID | libc::MS_NODEV,
        Some(&CString::new(data)?),
    )
}

/// A path in overlayfs's options: `,` separates options and `:` layers, so
/// both are escaped, and so is the escape.
fn escape(p: &Path) -> String {
    let mut s = String::new();
    for ch in p.to_string_lossy().chars() {
        if matches!(ch, '\\' | ',' | ':') {
            s.push('\\');
        }
        s.push(ch);
    }
    s
}

fn open_path(p: &str) -> io::Result<OwnedFd> {
    let fd = sys::cvt(unsafe {
        libc::open(
            c(p)?.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    })?;
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// The mount flags that carry `path`'s mount's atime setting.
fn atime_flags(path: &str) -> libc::c_ulong {
    let Ok(p) = c(path) else { return 0 };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(p.as_ptr(), &mut st) } != 0 {
        return 0;
    }
    let f = st.f_flag;
    let mut flags = 0;
    if f & libc::ST_NOATIME != 0 {
        flags |= libc::MS_NOATIME;
    }
    if f & libc::ST_NODIRATIME != 0 {
        flags |= libc::MS_NODIRATIME;
    }
    if f & libc::ST_RELATIME != 0 {
        flags |= libc::MS_RELATIME;
    }
    if f & (libc::ST_NOATIME | libc::ST_RELATIME) == 0 {
        flags |= libc::MS_STRICTATIME;
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_options_escape_their_separators() {
        assert_eq!(escape(Path::new("/a/b")), "/a/b");
        assert_eq!(escape(Path::new("/a,b:c\\d")), "/a\\,b\\:c\\\\d");
    }

    #[test]
    fn the_operators_part_is_mounted_parents_first() {
        let mut spec = Spec::new(vec!["true".into()], "/");
        spec.home = Some("/home/u".into());
        spec.ro_paths = vec!["/home/u/.cargo".into(), "/opt".into()];
        spec.workspace = vec!["/home/u/w".into(), "/tmp/x".into()];
        let order: Vec<String> = plan(&spec)
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        assert_eq!(
            order,
            ["/opt", "/home/u", "/tmp/x", "/home/u/.cargo", "/home/u/w"]
        );
    }

    #[test]
    fn a_host_path_is_found_under_the_old_root() {
        assert_eq!(host(Path::new("/usr/bin")), Path::new("/.oldroot/usr/bin"));
    }
}
