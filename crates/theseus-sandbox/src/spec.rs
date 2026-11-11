//! What a job in L1 is given: its command, environment, view, and limits.
//! The wrapper hands it to the init over a pipe, never as arguments, so a
//! granted secret in the environment never shows on a command line.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One job in L1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    /// The command. `argv[0]` without a slash is found on the job's own
    /// `PATH` inside the view, as `execvp` would find it.
    pub argv: Vec<String>,
    /// The command's whole environment: nothing else reaches it.
    pub env: Vec<(String, String)>,
    /// The command's working directory, inside the view.
    pub cwd: PathBuf,
    /// The workspace roots, at the same absolute paths: the tree read-only,
    /// under an overlay whose writes go to scratch.
    pub workspace: Vec<PathBuf>,
    /// More read-only binds at the same absolute paths (`[sandbox] ro_paths`),
    /// files or directories.
    pub ro_paths: Vec<PathBuf>,
    /// The operator's HOME, an empty tmpfs at the same path.
    pub home: Option<PathBuf>,
    /// Paths covered in the view, whatever bound them when it was built (17b:
    /// Theseus's floor, its store, spool, bindings, token, and socket): a
    /// directory by an empty read-only tmpfs, a file or socket by `/dev/null`.
    /// An entry is covered at what it resolves to in the view, so a symlink
    /// hides the path it names; one that resolves to nothing there is skipped,
    /// and neither a link nor a dangling link ever stops the job
    /// (theseus-txh9w).
    ///
    /// **What the list promises is what exists when the job starts, not what
    /// the job can read.** A cover is a mount, made once, at a path that
    /// resolved to something; an entry that resolved to nothing was skipped,
    /// and a bind of the host's tree is a live window, not a copy, so content
    /// written on the host under that bind at a skipped name afterwards is
    /// readable by the job, unmasked. `view::mask`'s doc is the whole of it:
    /// which parts carry that window, why no mount can close it for a
    /// read-only bind, and the residue (two binds, or a hard link, reaching
    /// one inode by a path no resolution derives). An operator who needs more
    /// must not bind a tree that can gain a hidden name (theseus-3i6d1).
    #[serde(default)]
    pub hidden: Vec<PathBuf>,
    pub hostname: String,
    pub limits: Limits,
    /// 18b: the port of the egress listener the init opens on 127.0.0.1
    /// inside the job's network namespace and hands to the wrapper.
    pub egress_port: Option<u16>,
    /// Read-write binds at the same absolute paths (theseus-pw1q.21.1): the
    /// one place a job's writes persist. Only a self build's launcher sets
    /// it (`L1.rw`'s doc says why); a directory only.
    #[serde(default)]
    pub rw: Vec<PathBuf>,
    /// The host facilities this view grants beyond the contract's floor.
    /// Nothing by default; **only a self build's launcher turns one on**
    /// ([`Facilities`]).
    #[serde(default, skip_serializing_if = "Facilities::is_none")]
    pub facilities: Facilities,
}

/// Host facilities a view may be granted beyond what every L1 job gets
/// (theseus-1eemp). Each is off by default, and **the only caller that turns
/// one on is `theseus_core::sandbox::self_build_view`**: the model's
/// `proc.run` builds its view in `for_job`, which cannot reach this field,
/// so a view that is not a self build's is as it was.
///
/// **Why they exist.** A self build runs the suite on the code it wrote, and
/// 108 of that suite's tests have the sandbox, the terminal and the device
/// nodes as their *subject*. Without these grants those tests fail on the
/// environment rather than on the branch, so the loop cannot tell a change
/// that broke the sandbox from one that did not — and a loop that cannot
/// test the sandbox, the pty and the daemon can never improve them.
///
/// **What they are not.** None of them reaches the floor, a credential, a
/// sealed set, the store or the spool: those are `hidden`, `rw` and the
/// environment, and nothing here touches any of the three. [`Facilities::nesting`]
/// documents why granting the mount calls does not lift a `hidden` cover.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facilities {
    /// A `devpts` instance of the job's own at `/dev/pts`, and the
    /// `/dev/ptmx` that allocates in it. Without it `posix_openpt` and
    /// `openpty` answer `ENOENT`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pty: bool,
    /// The device nodes in [`crate::MORE_DEVICES`] as well as the four
    /// every job gets.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub devices: bool,
    /// The calls a job needs to build a sandbox of its own: `clone` with the
    /// `CLONE_NEW*` flags, `unshare`, `mount`, `umount2`, `pivot_root` and
    /// `mount_setattr`. `setns` and the new mount API (`open_tree`,
    /// `move_mount`, `fsopen`, `fsconfig`, `fsmount`, `fspick`) stay denied:
    /// the init uses none of them, so granting them would buy nothing.
    ///
    /// **This does not lift a `hidden` cover, and the reason is the kernel's,
    /// not ours.** A cover is a mount laid over a live bind, so a job that
    /// could `umount2` it would read the operator's real path. It cannot:
    ///
    /// - the job's own mount namespace is owned by user namespace 1, which
    ///   the wrapper made; the command runs in user namespace 2 (`init`'s
    ///   `Command::setup` unshares into it), and `may_mount` asks for
    ///   `CAP_SYS_ADMIN` **in the namespace that owns the mount namespace**.
    ///   A capability held in a child user namespace is not held in its
    ///   parent, so `umount2` of a cover answers `EPERM` whatever seccomp
    ///   says — and that is true today, before this grant;
    /// - the only way to hold mount privileges is a **new** user namespace
    ///   with a **new** mount namespace, and `copy_mnt_ns` marks every mount
    ///   inherited across a user-namespace boundary `MNT_LOCKED`.
    ///   `do_umount` refuses a locked mount, and `do_loopback` refuses a
    ///   non-recursive bind of a directory with locked children — which is
    ///   the other way one would try to walk around a cover.
    ///
    /// `tests_facilities` plants both attempts inside a granted view and
    /// asserts the sealed bytes stay unreadable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nesting: bool,
    /// A `/proc` and a `/sys` **without Docker's masks** and without the
    /// read-only remounts under `/proc`.
    ///
    /// **This is the kernel's precondition for [`nesting`](Self::nesting) to
    /// be of any use, and it was found by measurement, not by reading.** With
    /// `nesting` alone the nested `clone` succeeds and the nested init then
    /// dies at `mounting /proc: Operation not permitted`:
    /// `mount_too_revealing` refuses a fresh procfs in a less-privileged
    /// mount namespace unless the namespace already holds a *fully visible*
    /// mount of that filesystem, and `mnt_already_visible` does not count a
    /// mount that has locked child mounts covering anything but a
    /// permanently-empty directory. Our masks are exactly such children, so
    /// a masked `/proc` makes a nested procfs impossible — and there is no
    /// third option: a second, hidden, fully-visible procfs would itself be
    /// skipped by the same rule, and binding the outer `/proc` would take
    /// the nested job out of its own pid namespace, which is the thing under
    /// test.
    ///
    /// **Its price, measured and not minimised.** It reaches no credential,
    /// no store, no spool, no sealed set and no floor path — but an
    /// unmasked `/proc` and `/sys` disclose host facts: `/proc/sched_debug`
    /// and `/proc/timer_list` name tasks beyond the job's pid namespace,
    /// `/sys/fs/cgroup` names the operator's units, `/sys/firmware` carries
    /// DMI and ACPI identifiers, and `/sys/devices/virtual/powercap` is an
    /// energy side-channel. The writes those paths would need are refused
    /// by capability (`sysrq-trigger` and a non-namespaced sysctl each want
    /// `CAP_SYS_ADMIN` in the initial user namespace, which no nested one
    /// confers) and `/proc/kcore` wants `CAP_SYS_RAWIO` there, so the cost
    /// is **disclosure, not control**.
    ///
    /// **It is off, and it is an owner question** (asked 2026-11-11 by the
    /// shift-5 self-view builder). The owner's 11:34 grant covers the
    /// facilities the suite's subjects need; it was given without this fact,
    /// because nobody had it yet. Turning it on also means a self build's
    /// own view no longer satisfies `clause_08_proc_and_sys_masked` — the
    /// clause it is running in order to test. Until he answers, `nesting` is
    /// granted, this is not, and the gate names the 48 tests that stay
    /// withheld (`self_build::WITHHELD`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub visible_proc: bool,
}

impl Facilities {
    /// Nothing granted: every L1 job but a self build's.
    pub const NONE: Self = Self {
        pty: false,
        devices: false,
        nesting: false,
        visible_proc: false,
    };

    /// Whether nothing is granted (`serde`'s skip, and the contract's
    /// default).
    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    /// The names granted, for a report and for the gate's honesty line.
    pub fn named(&self) -> Vec<&'static str> {
        [
            (self.pty, "pty"),
            (self.devices, "devices"),
            (self.nesting, "nesting"),
            (self.visible_proc, "visible_proc"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }
}

/// What one job may use. Its memory has no limit of its own, as an L0
/// job's has none (theseus-gyin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Scratch: the overlays' writes, every workspace root together.
    pub scratch_mb: u64,
    /// Each of `/tmp`, `/dev/shm`, and HOME.
    pub tmp_mb: u64,
    /// The largest file the command may write, its output included
    /// (`RLIMIT_FSIZE`). Past it, the write fails and the command gets
    /// `SIGXFSZ`.
    pub output_mb: u64,
    /// The job's processes (`RLIMIT_NPROC`, which the kernel counts in the
    /// job's own user namespace, so it is a per-job limit). Linux exempts
    /// root from it, so `spawn` refuses a root operator's job (`refusal`).
    pub pids: u64,
}

impl Default for Limits {
    /// The design's defaults (§2.12): scratch 1024 MB, output 64 MB, 512
    /// processes.
    fn default() -> Self {
        Self {
            scratch_mb: 1024,
            tmp_mb: 1024,
            output_mb: 64,
            pids: 512,
        }
    }
}

/// The hostname a job sees.
pub const HOSTNAME: &str = "theseus-l1";

impl Spec {
    /// A job with nothing granted: no workspace, no HOME, no environment.
    pub fn new(argv: Vec<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            argv,
            env: Vec::new(),
            cwd: cwd.into(),
            workspace: Vec::new(),
            ro_paths: Vec::new(),
            home: None,
            hidden: Vec::new(),
            hostname: HOSTNAME.into(),
            limits: Limits::default(),
            egress_port: None,
            rw: Vec::new(),
            facilities: Facilities::NONE,
        }
    }

    /// Why this spec cannot run, or None. Checked by `spawn` before the
    /// clone, so a bad spec fails with its reason and starts nothing.
    pub fn invalid(&self) -> Option<String> {
        if self.argv.is_empty() {
            return Some("the command is empty".into());
        }
        if !self.cwd.is_absolute() {
            return Some(format!("the cwd {} is not absolute", self.cwd.display()));
        }
        let user = self
            .workspace
            .iter()
            .map(|p| ("a workspace root", p))
            .chain(self.ro_paths.iter().map(|p| ("a read-only path", p)))
            .chain(self.rw.iter().map(|p| ("a read-write path", p)))
            .chain(self.home.iter().map(|p| ("HOME", p)));
        for (what, p) in user {
            if let Some(why) = bad_view_path(p) {
                return Some(format!("{what}, {}, {why}", p.display()));
            }
        }
        if let Some(p) = self.hidden.iter().find(|p| !p.is_absolute()) {
            return Some(format!("a hidden path, {}, is not absolute", p.display()));
        }
        if self.hostname.is_empty() || self.hostname.len() > 64 {
            return Some("the hostname must be 1 to 64 bytes".into());
        }
        None
    }
}

/// Why `p` cannot be placed in the view, or None.
fn bad_view_path(p: &Path) -> Option<&'static str> {
    if !p.is_absolute() {
        return Some("is not absolute");
    }
    if p.components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Some("holds . or ..");
    }
    if p == Path::new("/") {
        return Some("is the root");
    }
    for owned in ["/proc", "/sys", "/dev", "/.oldroot", "/.scratch"] {
        if p.starts_with(owned) {
            return Some("is inside a part of the view the sandbox owns");
        }
    }
    None
}

/// The program that becomes the job's init: this binary in its
/// `job-sandbox` role, by default.
#[derive(Debug, Clone)]
pub struct Init {
    pub exe: PathBuf,
    pub args: Vec<String>,
}

/// The role word that makes a binary the sandbox's init.
pub const INIT_ROLE: &str = "job-sandbox";

impl Default for Init {
    /// `/proc/self/exe job-sandbox`: the running binary, even after a newer
    /// one is installed over its path.
    fn default() -> Self {
        Self {
            exe: "/proc/self/exe".into(),
            args: vec![INIT_ROLE.into()],
        }
    }
}
