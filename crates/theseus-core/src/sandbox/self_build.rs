//! The view a self branch's code runs in (theseus-pw1q.21.1; RSI loop 1,
//! §4.2): the branch's gate, the planted revert's test runs, the merge gate
//! with its benches, auto-revert's gate, and the install's build all go
//! through [`Sandbox::self_build_view`], so a `build.rs`, a test or a script
//! a branch wrote runs below the same floor every L1 job has: the store, the
//! spool, the bindings, the 1Password token, the credentials, the socket and
//! the sealed holdout sets are hidden, whatever binds them.
//!
//! What the view holds, and nothing else of the operator's:
//!
//! - **read-write**: the tree and its build tree. The only writes that
//!   persist, and the only place `L1.rw` is ever set (the model's `proc.run`
//!   builds its view in `for_job`, which cannot reach it).
//! - **overlays** (writes land in scratch and vanish): the repo, for git's
//!   objects and the worktree's admin files; `~/.cargo`, so cargo's package
//!   cache lock and index writes succeed; `~/.npm`, for npm's cache; and
//!   `~/.cache/theseus`, where the merge gate reads the bench history.
//! - **read-only**: `~/.rustup`, main's copy of the gate, and the operator's
//!   `[sandbox] ro_paths`.
//! - **no network**: an empty egress list, so no route at all. Cargo is told
//!   `--offline` (`CARGO_NET_OFFLINE`), so a branch that adds a crate the
//!   cache lacks fails its gate at once, saying so ([`supply_chain`]):
//!   refused, never fetched.
//! - 8,192 processes (the job default of 512 stays), and file, scratch and
//!   `/tmp` caps sized for a compiler: the model's `proc.run` limits would
//!   stop a link.

use std::path::{Path, PathBuf};

use theseus_kernel::job::{SandboxLimits, L1};

use super::Sandbox;

/// A self build's processes (`RLIMIT_NPROC` in its own user namespace): a
/// parallel cargo build and a test run fork far past the job default.
pub const BUILD_PIDS: u64 = 8192;
/// The largest file a self build may write: a linked binary with debug info.
const BUILD_OUTPUT_MB: u64 = 16 * 1024;
/// Each of `/tmp`, `/dev/shm` and HOME, and scratch, for a build.
const BUILD_TMP_MB: u64 = 4 * 1024;

/// Where a self build runs.
#[derive(Debug, Clone)]
pub struct SelfBuild<'a> {
    /// The branch's worktree, read-write.
    pub tree: &'a Path,
    /// Its build tree (`CARGO_TARGET_DIR`), read-write.
    pub target: &'a Path,
    /// The repo the worktree was cut from, an overlay.
    pub repo: &'a Path,
    /// More read-only files: main's copy of the gate.
    pub ro: &'a [PathBuf],
    /// The operator's HOME: where `.cargo`, `.npm`, `.rustup` and
    /// `.cache/theseus` are.
    pub home: &'a Path,
}

impl Sandbox {
    /// The view for a self build of `b`, over this daemon's floor.
    pub fn self_build_view(&self, b: &SelfBuild<'_>) -> L1 {
        let base = self.job_view();
        let home = |p: &str| b.home.join(p);
        let workspace = vec![
            b.repo.to_path_buf(),
            home(".cargo"),
            home(".npm"),
            home(".cache/theseus"),
        ];
        // The view's own read-only binds, apart from the operator's
        // `[sandbox] ro_paths`, which the mask filter below must not see.
        let own_ro: Vec<PathBuf> = std::iter::once(home(".rustup"))
            .chain(b.ro.iter().cloned())
            .collect();
        let ro_paths: Vec<PathBuf> = own_ro
            .iter()
            .cloned()
            .chain(base.ro_paths)
            .filter(|p| !workspace.contains(p))
            .collect();
        let rw = vec![b.tree.to_path_buf(), b.target.to_path_buf()];
        // The floor and the credentials, the sealed sets among them, less any
        // directory that holds what this view binds: the view is built from
        // nothing but its binds, so a mask over such a directory (the
        // approve list's `~/.theseus`, which holds the gate, the tree and the
        // build tree) would cover the binds themselves, and the gate would
        // find no script. What the directory holds that must stay hidden is
        // listed on its own (the store, the spool, the sealed sets). Only the
        // view's own binds count: a configured `ro_paths` entry inside a
        // masked directory must not un-mask it (theseus-i3h47).
        let hidden: Vec<PathBuf> = base
            .hidden
            .into_iter()
            .filter(|h| {
                !(rw.iter().chain(&own_ro).chain(&workspace))
                    .any(|bound| bound != h && bound.starts_with(h))
            })
            .collect();
        L1 {
            workspace,
            ro_paths,
            hidden,
            limits: SandboxLimits {
                scratch_mb: base.limits.scratch_mb.max(BUILD_TMP_MB),
                tmp_mb: base.limits.tmp_mb.max(BUILD_TMP_MB),
                output_mb: base.limits.output_mb.max(BUILD_OUTPUT_MB),
                pids: base.limits.pids.max(BUILD_PIDS),
            },
            egress: Vec::new(),
            egress_dns: None,
            rw,
        }
    }
}

/// The environment of a self build's command, the whole of it: the
/// operator's `HOME` (an empty tmpfs in the view), a `PATH` that finds
/// cargo's shims, the build tree, and cargo told it has no network.
pub fn env(home: &Path, target: &Path) -> Vec<(String, String)> {
    let var = |k: &str, v: String| (k.to_string(), v);
    vec![
        var(
            "PATH",
            format!("{}/.cargo/bin:/usr/local/bin:/usr/bin:/bin", home.display()),
        ),
        var("HOME", home.display().to_string()),
        var("LANG", "C.UTF-8".into()),
        var("CARGO_TARGET_DIR", target.display().to_string()),
        var("CARGO_NET_OFFLINE", "true".into()),
        var("RUSTC_WRAPPER", String::new()),
        var("CARGO_BUILD_RUSTC_WRAPPER", String::new()),
        var("GIT_TERMINAL_PROMPT", "0".into()),
    ]
}

/// The lines cargo and npm print when a build needed the network, which a
/// self build never has.
const OFFLINE_SIGNS: [&str; 6] = [
    "offline mode (via `--offline`)",
    "attempting to make an HTTP request, but --offline was specified",
    "you're using offline mode",
    "failed to download from",
    "unable to update registry",
    "ENOTCACHED",
];

/// Why a red gate's `log` says the branch needed a crate or package the
/// local cache lacks, or `None`. A self branch that adds a dependency fails
/// here (the supply chain: refused, not fetched); the owner adds it, or the
/// branch drops it.
pub fn supply_chain(log: &str) -> Option<&'static str> {
    OFFLINE_SIGNS.iter().any(|s| log.contains(s)).then_some(
        "supply chain: the branch needs a crate or package that the local cache does not hold, \
         and a self build has no network and fetches nothing (cargo runs --offline). Add the \
         dependency yourself, or take it out of the branch.",
    )
}
