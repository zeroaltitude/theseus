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
    /// Paths covered in the view whatever binds them (17b: Theseus's floor,
    /// its store, spool, bindings, token, and socket): a directory by an
    /// empty read-only tmpfs, a file or socket by `/dev/null`. One the view
    /// does not hold is skipped.
    #[serde(default)]
    pub hidden: Vec<PathBuf>,
    pub hostname: String,
    pub limits: Limits,
    /// A cgroup directory made for this job, with its limits already
    /// written; the init is moved into it before it is released. None when
    /// the daemon's cgroup is not delegated.
    pub cgroup: Option<PathBuf>,
    /// 18b: the port of the egress listener the init opens on 127.0.0.1
    /// inside the job's network namespace and hands to the wrapper.
    pub egress_port: Option<u16>,
}

/// What one job may use. Memory is the cgroup's alone (`Spec::cgroup`).
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
    /// job's own user namespace, so it is a per-job limit with or without a
    /// cgroup).
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
            cgroup: None,
            egress_port: None,
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
