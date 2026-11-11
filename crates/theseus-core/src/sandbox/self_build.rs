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
//! - **read-only**: `~/.rustup`, the pinned node (`~/.nvm/versions/node/v`
//!   [`NODE_PIN`], that one directory: the cockpit phase of the gate runs its
//!   lint, tests and build on it, theseus-selom), main's copy of the gate, and
//!   the operator's `[sandbox] ro_paths`. The cockpit's `node_modules` are not
//!   bound: the gate's `npm ci --offline` makes them in the tree from `~/.npm`'s
//!   overlay, and a package that cache lacks is refused ([`supply_chain`]).
//! - **no network**: an empty egress list, so no route at all. Cargo is told
//!   `--offline` (`CARGO_NET_OFFLINE`), so a branch that adds a crate the
//!   cache lacks fails its gate at once, saying so ([`supply_chain`]):
//!   refused, never fetched.
//! - 8,192 processes (the job default of 512 stays), and file, scratch and
//!   `/tmp` caps sized for a compiler: the model's `proc.run` limits would
//!   stop a link.
//! - **the facilities the suite's own subjects need** ([`FACILITIES`]): a
//!   pty, `/dev/full` and `/dev/tty`, and the six calls that build a sandbox
//!   of its own, with the `/proc` masks off, which the kernel requires before
//!   those calls buy anything. [`WITHHELD`] names the four tests that no
//!   grant reaches. This is the only view in the build that gets any of them,
//!   and the reason is the one the owner gave on 2026-11-11: a self loop
//!   that cannot test the sandbox, the pty, the daemon and the devices can
//!   never improve them. What it does **not** get is unchanged: no floor
//!   path, no credential, no sealed set, no store, no spool, no network.

use std::path::{Path, PathBuf};

use theseus_kernel::job::{SandboxLimits, L1};

use super::Sandbox;

/// A self build's processes (`RLIMIT_NPROC` in its own user namespace): a
/// parallel cargo build and a test run fork far past the job default.
pub const BUILD_PIDS: u64 = 8192;

/// What a self build's view grants that no other L1 job's does, and the
/// whole of it (theseus-1eemp).
///
/// **Measured, not guessed.** The 2026-11-11 08:43 self build was the first
/// ever to reach the suite, and 123 of 9,819 tests failed, none of them the
/// item's change. Classified by the error each printed:
///
/// | n | what it printed | granted by |
/// |---:|---|---|
/// | 58 | `posix_openpt`/`openpty` → `ENOENT`: no `/dev/ptmx` | `pty` |
/// | 49 | `cloning the init into its namespaces: EPERM`, our own filter | `nesting` |
/// | 1 | `/dev/full` → `NotFound`, on a host that has it | `devices` |
/// | 15 | a timeout or a value, naming no facility; downstream of the above | — |
///
/// And the ladder, measured by running those same 123 inside this view one
/// grant at a time: **0 -> 60 (`pty`) -> 63 (`devices`) -> 67 (`nesting`)
/// -> 119 (`visible_proc`)**. `nesting` reads as worth one test on its own
/// and is worth 52 with the grant that unblocks it, which is why the two are
/// granted together and why neither is granted quietly.
///
/// A run inside this view is not a weaker run than a lane's: it is the only
/// run in which the sandbox contract, the terminal and the device nodes are
/// *tested* rather than skipped. `theseus_sandbox::Facilities` carries why
/// each grant cannot reach the floor; the one that needs an argument rather
/// than an assertion is `nesting`, and its doc gives it.
pub const FACILITIES: theseus_sandbox::Facilities = theseus_sandbox::Facilities {
    pty: true,
    devices: true,
    nesting: true,
    // ON, and the grant with the largest price and the largest return, so it
    // is the one to read twice. `nesting` without it buys ONE test of 123:
    // every nested L1 job dies at `mounting /proc: Operation not permitted`
    // while `/proc`'s masks are on. With it the count goes 67 -> 119. It
    // discloses host facts through `/proc` (`sched_debug` and `timer_list`
    // name tasks beyond the job's pid namespace) and gives control of none,
    // and it is narrowed to `/proc` alone: `/sys`'s masks stay on, because
    // taking them off as well changed nothing. A confirm line is with the
    // owner (2026-11-11); his 11:34 grant covers the facilities the suite's
    // subjects need and this is one, so it is on rather than held, and one
    // word takes it back off.
    visible_proc: true,
};
/// The largest file a self build may write: a linked binary with debug info.
const BUILD_OUTPUT_MB: u64 = 16 * 1024;
/// Each of `/tmp`, `/dev/shm` and HOME, and scratch, for a build.
const BUILD_TMP_MB: u64 = 4 * 1024;

/// The node release the cockpit phase of a self build's gate runs on, one exact
/// version as `rust-toolchain.toml` pins Rust's (theseus-selom). It is found at
/// `~/.nvm/versions/node/v<NODE_PIN>` (`nvm install <NODE_PIN>`); the view binds
/// that one directory read-only, so the daemon's own `PATH` and the operator's
/// other node versions never decide what a self build's cockpit tests run on.
pub const NODE_PIN: &str = "24.18.0";

/// Where the pinned node lives under `home`.
pub fn node_home(home: &Path) -> PathBuf {
    home.join(".nvm/versions/node").join(format!("v{NODE_PIN}"))
}

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
        let own_ro: Vec<PathBuf> = [home(".rustup"), node_home(b.home)]
            .into_iter()
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
            facilities: FACILITIES,
        }
    }
}

/// The environment of a self build's command, the whole of it: the
/// operator's `HOME` (an empty tmpfs in the view), a `PATH` that finds the
/// pinned node and cargo's shims, the build tree, cargo told it has no
/// network, and the two names that carry the gate's honesty line about the
/// view this run was judged in ([`WITHHELD`]). Nothing of the daemon's own
/// state and no credential, which `tests_jail` holds by an allowlist.
pub fn env(home: &Path, target: &Path) -> Vec<(String, String)> {
    let var = |k: &str, v: String| (k.to_string(), v);
    vec![
        var(
            "PATH",
            format!(
                "{}/bin:{}/.cargo/bin:/usr/local/bin:/usr/bin:/bin",
                node_home(home).display(),
                home.display()
            ),
        ),
        var("HOME", home.display().to_string()),
        var("LANG", "C.UTF-8".into()),
        var("CARGO_TARGET_DIR", target.display().to_string()),
        var("CARGO_NET_OFFLINE", "true".into()),
        var("RUSTC_WRAPPER", String::new()),
        var("CARGO_BUILD_RUSTC_WRAPPER", String::new()),
        var("GIT_TERMINAL_PROMPT", "0".into()),
        // The gate's honesty line, which only the launcher can fill: the view
        // a self run was judged in is as much a part of reading its result as
        // the phases it timed, and a run inside a relaxed view that said so
        // nowhere would be the quietest possible version of this bucket's own
        // disease. `gate.sh`'s summary prints the first and registers the
        // second as any other stand-down; both are unset for a lane gate.
        var("THESEUS_VIEW_GRANTS", view_grants()),
        var("THESEUS_VIEW_WITHHELD", WITHHELD.to_string()),
    ]
}

/// What the gate prints as *the view this run had*.
fn view_grants() -> String {
    format!("a self build's, granting {}", FACILITIES.named().join(", "))
}

/// What a self run still does NOT do that an ordinary lane gate does, in the
/// words the gate's `what this run did NOT do:` line carries — because a run
/// that silently skips nothing and silently runs nothing is the failure this
/// whole bucket exists to kill.
///
/// **Measured on 2026-11-11, on the exact 123 tests the 08:43 self build
/// failed, run inside this view one grant at a time:** nothing granted 0,
/// `pty` 60, `+devices` 63, `+nesting` 67, `+visible_proc` **119**.
///
/// The four that remain are named here, and **not one of them is a facility
/// this view withholds** — each fails identically at every rung, so no
/// further grant would move it:
///
/// - `theseusd::sandbox a_probe_script_in_l1_shows_the_contract_and_l0_the_contrast`
///   asserts that an L1 job and an L0 job see *different* hostnames. Inside a
///   self build both are `theseus-l1`, because a self build has no L0: its own
///   view is L1. **No grant can fix a test whose subject is the contrast
///   between inside and outside, run from inside.** It wants a named skip for
///   a self run, which is a line, not a grant.
/// - `theseus-core term::tests_keep::a_close_leaves_no_child_forked_into_a_new_session_during_it`
///   and `::a_keeping_close_names_a_child_forked_into_a_new_session_during_it`
///   fail on `fork` (`Resource temporarily unavailable`, and its sibling's
///   `the forker forked`) at every rung, alone as well as beside the other
///   nineteen binaries. A limit or an ordering inside the view, not a
///   facility; it wants its own line.
/// - `theseus-core rpc::tests_self_mint::a_validation_runs_its_tiers_writes_one_row_and_loads_nothing`
///   ends both arms `handback:step-failed` at every rung. Unexplained. Ruled
///   out: a facility (grant-independent), `/tmp` being `noexec` (it is not)
///   and load (it fails alone). It wants its own line too.
///
/// Empty would mean a self run does everything a lane gate does. It does not,
/// and the gate must not pretend otherwise.
const WITHHELD: &str = "4 tests no grant reaches: the L1-vs-L0 contrast probe, two term close-and-fork tests, and the self-mint fixture tier";

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
