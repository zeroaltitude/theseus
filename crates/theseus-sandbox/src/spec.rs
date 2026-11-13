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
    /// A `/proc` without either of its two mask groups: the covers
    /// (`PROC_MASKED`) and the read-only remounts (`proc_readonly`, both in
    /// `view.rs`). **`/sys` is untouched** — `SYS_MASKED` stays on,
    /// firmware, cgroups and powercap with it.
    ///
    /// **It is ON for a self build's view, by the owner's decision of
    /// 2026-11-11 16:32** (`logistics/OWNER-ANSWERS-20261110-1406.md`, sixth
    /// set, S6-1: a yes to this grant *only*). It is on deliberately and is
    /// not an oversight; one word takes it back off.
    ///
    /// **What it buys is EVIDENCE, NOT PROOF, and the distinction is the
    /// point.** The ladder was measured once, inside the view, on a rig that
    /// no longer exists: of 123 tests that fail in an ungranted view, 0 pass
    /// at no grants, 67 with `pty`, `devices` and `nesting`, and 119 with
    /// this grant as well. **The counts are one unreproducible run** and are
    /// not to be restated as a cause.
    ///
    /// **But the cause of the last step is now established, and it is this
    /// grant.** Measured on 2026-11-13 on two kernels (`7.0.0-1014-aws` and
    /// `6.18.54.1-microsoft-standard-WSL2`), one tree and one session per
    /// host, the two rungs differing in this one bool: a real nested L1 job
    /// spawned from inside the view is **refused with `mounting /proc:
    /// Operation not permitted`** at `pty`, `devices` and `nesting`, and
    /// **starts and exits 0** with this grant as well. A musl and a glibc
    /// build of the probe agreed exactly, so this is no libc artifact. **So
    /// the grant is a hard precondition for a nested L1 job**, `nesting`
    /// alone does not suffice on either kernel, and the 1.0.6 line that
    /// would have taken it back off if it did is closed: nothing comes off.
    ///
    /// **The `nested_proc` probe that once put this in doubt measures the
    /// wrong thing.** It mounts a bare procfs at `/tmp`, where nothing is
    /// covered; a real init mounts at `/proc`, where the inherited locked
    /// covers are. At the very rung the old doubt rested on — `nesting`
    /// alone on 6.18 — the two disagree: `nested_proc: ok` while a real
    /// nested L1 job is refused. That probe is also kernel-dependent, which
    /// this tree did not record: under `nesting` alone it is `ok` on 6.18
    /// and `EPERM` on 7.0.0. Conclusions drawn from it about what this grant
    /// buys are unsound, and are not to be put back.
    ///
    /// **Its price is disclosure, and it is smaller than it was once written
    /// to be.** It reaches no credential, no store, no spool, no sealed set
    /// and no floor path. What it actually discloses, measured path by path
    /// at every rung on two kernels, is **`/proc/interrupts`** (the host's
    /// CPU count and its IRQ and device names — a hardware fingerprint and a
    /// timing side-channel) and **`/proc/keys`** (the operator's keyring
    /// names, uid and permission mask; **never a payload**). **No host
    /// process is visible at any rung**: the job sees two pids, its init and
    /// itself, granted or not. Neither `/proc/sched_debug` (absent on both
    /// kernels) nor `/proc/timer_list` (root-only in the initial namespace)
    /// discloses anything — this paragraph as it stood before `d9204ea`
    /// (2026-11-11) claimed they name tasks beyond the job's pid namespace,
    /// and that was wrong.
    ///
    /// **Writes are contained by namespacing, not refused by capability.**
    /// As the job every sysctl write is denied, but as the correctly-mapped
    /// nested root that `nesting` grants, `net/ipv4/ip_forward` and
    /// `user/max_user_namespaces` both succeed — on the job's *own* net and
    /// user namespace. Every host-wide write (`sysrq-trigger`,
    /// `core_pattern`, `drop_caches`, `modprobe`) stays denied, and
    /// `/proc/kcore` wants `CAP_SYS_RAWIO` in the initial user namespace,
    /// which no nested one confers. So the cost is **disclosure, not
    /// control**.
    ///
    /// **And `clause_08_proc_and_sys_masked` is no longer a property of
    /// every L1 job**: a self build's view is the one exception, and
    /// [`Facilities`]'s own contract — that only `self_build_view` grants —
    /// is what confines it. The clause's *test* still passes inside the
    /// view, because it builds its own job spec with default facilities, so
    /// a green `contract` is not evidence the exception is contained;
    /// `tests_jail` pins that separately. The gate names the **4** tests no
    /// grant reaches (`self_build::WITHHELD`): the L1-vs-L0 contrast probe,
    /// two term close-and-fork tests, and the self-mint fixture tier.
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
