//! The view a job sees (design §2.2's table), built by the init from an
//! empty root:
//!
//! - `/` is a fresh tmpfs, entered by `pivot_root`, and read-only once built;
//! - the system (`/usr`, `/bin`, `/sbin`, `/lib*`, `/etc`) and `ro_paths` are
//!   read-only binds, with symlinks where the host has them;
//! - the workspace roots are at their own paths, read-only under overlays
//!   whose upper directories are one capped tmpfs: scratch;
//! - `rw` paths are writable binds at their own paths, the one place a job's
//!   writes persist (a self build's tree and build tree, theseus-pw1q.21.1);
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
            Part::ReadWrite => bind_rw(&host(&path), &path)
                .stage(format!("binding {} read-write", path.display()))?,
            Part::Workspace(i) => overlay(&host(&path), &path, i)
                .stage(format!("mounting the workspace {}", path.display()))?,
        }
    }
    // What the operator's part must never show, whatever binds it (17b).
    for p in &spec.hidden {
        mask(p).stage(format!("hiding {}", p.display()))?;
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
    ReadWrite,
    Workspace(usize),
}

/// The operator's part of the view, parents before children, so that a
/// read-only path or a workspace under HOME lands inside HOME's tmpfs.
fn plan(spec: &Spec) -> Vec<(PathBuf, Part)> {
    let mut v: Vec<(PathBuf, Part)> = Vec::new();
    v.extend(spec.home.iter().map(|h| (h.clone(), Part::Home)));
    v.extend(spec.ro_paths.iter().map(|p| (p.clone(), Part::ReadOnly)));
    v.extend(spec.rw.iter().map(|p| (p.clone(), Part::ReadWrite)));
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

/// A recursive bind of the directory `source` at `target`, writable: what the
/// job writes there is the host's (theseus-pw1q.21.1). nosuid and nodev, as
/// every bind is; exec stays, since a build runs what it makes.
fn bind_rw(source: &Path, target: &Path) -> io::Result<()> {
    if !fs::metadata(source)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a read-write path must be a directory",
        ));
    }
    mkdir_all(target)?;
    let t = c(target)?;
    sys::mount(
        Some(&c(source)?),
        &t,
        None,
        libc::MS_BIND | libc::MS_REC,
        None,
    )?;
    sys::mount_setattr(&t, true, MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV)
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
        // The same resolution the bind below will do, and the same three
        // errors skipped (theseus-txh9w): these binds are the only thing
        // making /proc/sys and /proc/sysrq-trigger read-only, since /proc is
        // not mounted MS_RDONLY and the root's attributes are not recursive,
        // so an error swallowed here would quietly leave them writable.
        let p = Path::new("/proc").join(name);
        if resolved(&p)
            .stage(format!("reading /proc/{name}"))?
            .is_none()
        {
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

/// What `p` names in the view, or None when nothing is behind it.
///
/// **Every decision about a path here is made the way the mount will make
/// it** (theseus-txh9w). `mount(2)` follows symlinks on its target, so a
/// non-following `symlink_metadata` answered a different question from the
/// one the next line asked: it called a symlink to a directory "not a
/// directory", and called a dangling link a file rather than nothing. Both
/// callers below acted on that answer and the mount then failed, ending the
/// job before it had started.
///
/// ENOENT, ENOTDIR and ELOOP are the three ways a path resolves to nothing —
/// no target, a non-directory in the middle, or a loop — and are the only
/// errors answered None. **Any other error is the init's to report**: a
/// caller that skipped past every error could leave the view short of a
/// cover or a read-only bind and say nothing about it.
fn resolved(p: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::metadata(p) {
        Err(e)
            if matches!(
                e.raw_os_error(),
                Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP)
            ) =>
        {
            Ok(None)
        }
        r => r.map(Some),
    }
}

/// Covers `p`: a directory with an empty read-only tmpfs, anything else with
/// `/dev/null`. A path that resolves to nothing in the view is skipped.
///
/// **The skip is not a way past the mask.** The resolution happens here, in
/// the view, after every bind and overlay is in place and while the old root
/// is still attached, so "resolves to nothing" means exactly "no content is
/// readable through this path inside the view": a link whose target no bind
/// shows, a link with no target at all, or a loop. What a link *does* name in
/// the view is covered at the place it names, which hides that content both
/// through the link and at its own path — so following widens the mask rather
/// than narrowing it, and no path the old code covered is skipped by the new.
///
/// **What the skip cannot cover, said plainly.** A path that resolves to
/// nothing *now* is not hidden *later*. A bind of the host's tree (`ro_paths`,
/// a read-write path) is a live window onto the host, not a copy: content
/// written under it on the host after the view was built, at a path this
/// function skipped, appears in the view unmasked. (A reviewer hid a dangling
/// link and an absent path, started a job, wrote both 600 ms later, and the
/// job read them in full.) Nothing here can close that for a read-only
/// bind: a mount needs a target, and that bind forbids making one. The other
/// two cases differ, and `mask` does not take them: a read-write path
/// (`Part::ReadWrite`, `bind_rw`) could be given a target, but making it
/// would write into the operator's real host tree; a workspace overlay
/// (`Part::Workspace`) could be given one in its own upper layer, which never
/// reaches the host, so a mask there is makeable; the answer below says why it
/// is still left undone, which was an open question until theseus-3i6d1 and is
/// now a decision. What does
/// hold is what the paragraph above says: the job's command does not exist
/// while the view is built, the root is read-only by the time it does, and a
/// mount point that was covered cannot be unlinked from inside. So `hidden`
/// is a promise about what exists when the job starts, and an operator who
/// needs more must not bind a tree that can gain the hidden name. Two binds,
/// or a hard link, reaching one inode by a path no resolution derives are the
/// same residue (the old theseus-quft); the tests below pin the skip itself
/// and the window.
///
/// **The window's answer** (theseus-3i6d1): accept it, bound it, and say it.
/// Three closures were weighed first, and each is rejected on evidence in this
/// tree rather than on taste.
///
/// *Cover the nearest existing ancestor instead, and let the kernel resolve at
/// open time.* It does not widen what the job reaches; it destroys the bind.
/// `plan` mounts parents first so that a read-only path under HOME lands
/// inside HOME's tmpfs, so under the config template's own `[sandbox]
/// ro_paths = ["~/.cargo", "~/.rustup"]` the nearest existing ancestor of
/// `~/.cargo/credentials` is `~/.cargo`, the bind itself: an empty read-only
/// tmpfs there hides the registry and the toolchain the operator bound on
/// purpose. The variant that does work is to decompose the bind — bind each
/// entry of the parent instead of the parent recursively, leaving the skipped
/// name with no backing at all — and it closes the window without refusing
/// anything and without writing to the host. Its costs are a mount per entry
/// per level, `bind_ro`'s recursive-bind reason to re-derive for each one, a
/// view that no longer shows a file the host gains legitimately, and a changed
/// mount table, which several §7 clauses read. Not small, and not provable on
/// a host that cannot build a view.
///
/// *Refuse a hidden path that resolves to nothing, rather than skipping it.*
/// It would refuse nearly every job. theseus-core's `sandbox::CREDENTIALS` is
/// appended to every view's `hidden` unconditionally, HOME here is an empty
/// tmpfs, and the floor's own paths are a daemon's state: an entry resolves to
/// nothing whenever no bind shows its directory or the host does not hold the
/// file, which is the common case, not the odd one. It also reverses
/// theseus-txh9w's landed rule that a hidden entry never stops a job. The
/// narrower form — refuse only where the skipped path falls inside a live
/// window — fails on the same default: two of the three `CREDENTIALS` entries
/// sit under the template's first `ro_paths` entry, so a machine where `cargo
/// login` has never run would refuse every L1 job. (The cost is not "jobs that
/// name a path they are about to create": no job names this list. It is every
/// job.) What plain `origin/main` did to the dangling half before txh9w was
/// an accident of a non-following read, not a policy, so it is no precedent.
///
/// *Re-check at the job's open.* There is no hook. `seccomp.rs` is classic BPF
/// that allows by default and denies a list with `RET_ERRNO`; it has no
/// `RET_USER_NOTIF` and there is no supervisor to hold one, and standing a
/// userspace decision in front of every `openat` of every job is the wrong
/// shape for §2.2's 25 ms start.
///
/// *So: accept it, and bound it.* What bounds it, each read off the code
/// above. Only a live window carries it: `Part::ReadOnly`, `Part::ReadWrite`
/// (a self build's launcher alone sets `Spec::rw`) and a workspace overlay's
/// lower layer. The view's own root is read-only from the `mount_setattr` in
/// `build`, and HOME, `/tmp`, `/dev/shm` and scratch hold the job's own bytes,
/// so a name the job makes there is its own content and no leak. The job
/// cannot open the window itself: a read-only bind refuses its writes and a
/// workspace's go to scratch, so the writer must be a process on the host,
/// outside the sandbox. Every live window in a view is a path the operator
/// named. And `[sandbox] egress` is empty by default, so what a job reads has
/// no route out that the operator did not give it.
///
/// *And say it — which is the one thing not built.* `mask` tells no one it
/// skipped: it answers `Ok(())` and no reader ever learns a hidden entry was
/// left open. The shape is NOTIFY OVER BLOCK (§2), and it is why none of the
/// three closures is taken: `mask` should answer whether it covered or
/// skipped; `build` should keep the skipped entries that lie inside a live
/// window (a pure path test against `Spec::ro_paths`, `rw` and `workspace`);
/// they should ride `report::Started` beside `sys`, which is already that
/// channel for what the view could not do; theseus-kernel's `job_l1` should
/// put them in `detail["sandbox"]`, as it does `granted` and `skipped`; and
/// theseus-core should make that a row and a line of the result's head, as it
/// does `sandbox.egress_refused`. Specified, not built: it crosses three
/// crates and a new fact, and no behavioural test of it can run on a host that
/// refuses every L1 job.
///
/// The workspace mask stays undone for a reason of its own: its target would
/// be made in scratch's upper layer, which `View.scratch` keeps open precisely
/// to report what the job wrote, so masking there would put the harness's own
/// files in the job's write report (`report::Scratch`). It is makeable, and it
/// should follow a fix to that report, not precede it.
///
/// None of this touches the residue above. A skipped path and a second path to
/// a covered inode are the same defect — a cover is placed at a path, and a
/// path is not an inode — but the notify answer reports a *skipped* entry, and
/// in the residue's case the entry resolved and *was* covered; what leaks is a
/// name that was never on the list. Closing that means masking by inode, which
/// is a walk of every live bind at every job start, against the same 25 ms. So
/// the residue stays named and unowned here, and this answer does not claim
/// it.
fn mask(p: &Path) -> io::Result<()> {
    let Some(m) = resolved(p)? else {
        return Ok(());
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
    // statvfs's flag bits (Linux's; musl's libc crate lacks ST_RELATIME).
    const ST_NOATIME: libc::c_ulong = 0x400;
    const ST_NODIRATIME: libc::c_ulong = 0x800;
    const ST_RELATIME: libc::c_ulong = 0x1000;
    let f = st.f_flag as libc::c_ulong;
    let mut flags = 0;
    if f & ST_NOATIME != 0 {
        flags |= libc::MS_NOATIME;
    }
    if f & ST_NODIRATIME != 0 {
        flags |= libc::MS_NODIRATIME;
    }
    if f & ST_RELATIME != 0 {
        flags |= libc::MS_RELATIME;
    }
    if f & (ST_NOATIME | ST_RELATIME) == 0 {
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

    /// theseus-txh9w. `resolved` is the one answer both the mask and
    /// `/proc`'s read-only binds act on, and it must be the answer `mount(2)`
    /// would give: a link is what it names, and only a path with nothing
    /// behind it is None. Flip it back to a non-following read and the two
    /// link cases below go the other way, which is the defect this row
    /// closes in both of its call sites at once.
    #[test]
    fn a_path_is_resolved_the_way_the_mount_will_resolve_it() {
        let d = tempfile::tempdir().expect("a temp dir");
        let p = |n: &str| d.path().join(n);
        let at = |n: &str| resolved(&p(n)).expect("a resolution");
        fs::create_dir(p("dir")).expect("the directory");
        fs::write(p("file"), "x").expect("the file");
        for (to, at) in [
            ("dir", "to_dir"),
            ("file", "to_file"),
            ("gone", "dangling"),
            ("loop_b", "loop_a"),
            ("loop_a", "loop_b"),
        ] {
            std::os::unix::fs::symlink(p(to), p(at)).expect("a link");
        }
        // A link is what it names: a link to a directory is a directory, so
        // it is covered by the tmpfs and not by the bind of /dev/null that
        // "not a directory" would have chosen and that mount(2) would then
        // have refused with ENOTDIR.
        for n in ["dir", "to_dir"] {
            assert!(at(n).is_some_and(|m| m.is_dir()), "{n}");
        }
        for n in ["file", "to_file"] {
            assert!(at(n).is_some_and(|m| m.is_file()), "{n}");
        }
        // Nothing is behind these: no target, a loop, or no path at all.
        // There is nothing to cover, and nothing to fail the job over.
        for n in ["gone", "dangling", "loop_a", "loop_b"] {
            assert!(at(n).is_none(), "{n}");
        }
        // A non-directory in the middle of a path is nothing too (ENOTDIR).
        assert!(at("file/under").is_none());
        // Every other failure is an error, and the init's to report: a name
        // past NAME_MAX is ENAMETOOLONG on any filesystem, and a `resolved`
        // that took it for nothing would skip a cover or a bind in silence.
        let e = resolved(&p(&"x".repeat(300))).expect_err("a name past NAME_MAX");
        assert_eq!(e.raw_os_error(), Some(libc::ENAMETOOLONG));
    }

    /// The skip is a behaviour, held here so it is a decision and not an
    /// accident: a hidden path that resolves to nothing is left alone (no
    /// mount, nothing made at it), and `mask` says nothing about it. What it
    /// cannot do about content that arrives later is `mask`'s doc.
    #[test]
    fn a_hidden_path_that_resolves_to_nothing_is_skipped_and_left_absent() {
        let tmp = tempfile::tempdir().expect("a temp dir");
        let dir = tmp.path().to_path_buf();
        let absent = dir.join("absent");
        let dangling = dir.join("dangling");
        std::os::unix::fs::symlink(dir.join("nowhere"), &dangling).unwrap();
        for p in [&absent, &dangling] {
            assert!(resolved(p).unwrap().is_none(), "{} resolves", p.display());
            mask(p).expect("a path that resolves to nothing is skipped");
        }
        assert!(!absent.exists(), "mask made the absent path");
        assert!(dangling.symlink_metadata().is_ok(), "mask removed the link");
    }

    /// The window `mask`'s doc names, reduced to what runs without
    /// privileges (theseus-3i6d1): the mask installs nothing at a path that
    /// resolves to nothing, so the name is still a usable name, and content
    /// that arrives at it afterwards reads in full. The temp dir stands in
    /// for a `ro_paths` bind, whose directory is the host's own and goes on
    /// being the host's for as long as the job runs; what this cannot drive
    /// is the privileged half (a real job, a real bind, the content written
    /// on the host while the job runs), which needs a host that can build
    /// an L1 view at all — `crates/theseusd/tests/sandbox.rs`, never a root
    /// box. If a later step closes the window, this test is what fails, and
    /// `mask`'s doc and `Spec::hidden`'s are what must change with it.
    #[test]
    fn content_that_arrives_after_the_mask_is_readable_at_the_name_it_skipped() {
        let tmp = tempfile::tempdir().expect("a temp dir");
        let absent = tmp.path().join("credentials");
        let dangling = tmp.path().join("linked");
        std::os::unix::fs::symlink(tmp.path().join("gone"), &dangling).unwrap();
        // At the moment the view is built, neither resolves, so both are
        // skipped and no cover is made (`mask` makes no mount at all here:
        // every other arm of it would, which is why this test uses only the
        // two shapes that resolve to nothing).
        for p in [&absent, &dangling] {
            assert!(resolved(p).unwrap().is_none(), "{} resolves", p.display());
            mask(p).expect("a path that resolves to nothing is skipped");
        }
        // The content arrives afterwards, as the host's writer makes it.
        let secret = "[registry]\ntoken = \"not-a-real-token\"\n";
        fs::write(&absent, secret).expect("the absent path's content");
        fs::write(tmp.path().join("gone"), secret).expect("the link's target");
        // Both names now resolve, and both read in full: the bytes, not a
        // cover's empty file, and not a short read.
        for p in [&absent, &dangling] {
            let m = resolved(p)
                .expect("a resolution")
                .unwrap_or_else(|| panic!("{} still resolves to nothing", p.display()));
            assert!(m.is_file(), "{}", p.display());
            assert_eq!(m.len(), secret.len() as u64, "{}", p.display());
            assert_eq!(
                fs::read_to_string(p).expect("the content").as_str(),
                secret,
                "{}",
                p.display()
            );
        }
    }

    #[test]
    fn a_host_path_is_found_under_the_old_root() {
        assert_eq!(host(Path::new("/usr/bin")), Path::new("/.oldroot/usr/bin"));
    }
}
