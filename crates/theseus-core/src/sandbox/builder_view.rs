//! The view a self builder's `proc.run` runs in: an ordinary L1 job's view
//! (`Sandbox::job_view`) with a read-write directory added, the item's build
//! folder (`rsi::builder_dir`), so that what cargo builds there is kept
//! between commands, and one more beside it, the item's own tree, whose `.git`
//! file stays read-only (`with_own_tree`). Nothing else changes: the same workspace overlays, the
//! same read-only paths and hidden list (less a mask over a directory that
//! holds the folder, as in the gate's view), no network beyond the call's own
//! egress list, the same limits but four ([`BUILDER_OUTPUT_MB`] and the constants beside it), and none of
//! the facilities a self build's gate is given (`self_build::FACILITIES`). The gate's view
//! (`Sandbox::self_build_view`) never binds this directory, so what a builder
//! compiled is not there for the gate to read.

use std::path::{Path, PathBuf};

use theseus_kernel::job::L1;

use super::Sandbox;

// The allocations of a self builder's view. The owner, 2026-11-15 17:39:14 (chat message
// 1571686049867112519): "since our current mode is one-at-a-time self improvements, it means we can
// have relatively generous disk allocations and file allocations. so please extend those for
// flexibility over safety." One item builds at a time, so each is sized for one builder with the
// machine's build resources to itself. They are this view's own: the configured `[sandbox]` caps
// are not changed and no other job's view has them. `max` keeps a configured value that is larger.
//
// **Generous on disk, bounded in memory.** What is on DISK is generous: one file (`RLIMIT_FSIZE`,
// a file lands in the build folder, which is on disk), the 48 GiB build folder itself
// (`rsi::builder_dir::MAX_BYTES`) and processes. What is MEMORY-BACKED is not: `/tmp`, `/dev/shm`,
// HOME and scratch are all tmpfs mounts (`theseus_sandbox::view`), so what a command writes there
// is RAM, with no memory limit behind it, on a machine that is also the owner's. Their ceilings are
// 4 GiB each, four times the default, and the large temporaries are taken out of them by pointing
// `TMPDIR` at the build folder's own `tmp` directory (`env`), which is disk, under its bound.

/// The largest single file a builder's command may write (`RLIMIT_FSIZE`): 16 GiB, as the gate's view
/// allows (`self_build::BUILD_OUTPUT_MB`): the link of the core's tests is far past a job's default
/// 64 MiB (the 2026-11-15 run died on it, `SIGXFSZ`), and a workspace's test binaries are large. A
/// file this size can only be made on disk (the build folder; the tmpfs mounts are 4 GiB).
pub const BUILDER_OUTPUT_MB: u64 = 16 * 1024;
/// Scratch, the overlays' writes (tmpfs, MEMORY): 4 GiB, from the job default of 1 GiB. Cargo's
/// registry-index and package-cache writes land here; the build output, which filled the 1 GiB on the
/// first real core test build, no longer does (it is in the build folder).
pub const BUILDER_SCRATCH_MB: u64 = 4 * 1024;
/// Each of `/tmp`, `/dev/shm` and HOME (tmpfs, MEMORY): 4 GiB, from the job default of 1 GiB.
/// Compiler and linker temporaries follow `TMPDIR`, which is on disk, so these hold small files.
pub const BUILDER_TMP_MB: u64 = 4 * 1024;
/// Processes: 32,768, for parallel compilers and a test run's own children on a many-core box; the job
/// default is 512, and the gate's view is 8,192. Only `RLIMIT_NPROC` binds an L1 job: `[tools]
/// job_pids_max` is a cgroup cap the wrapper applies on the L0 spawn path alone (`job.rs`, `spawn_l0`).
pub const BUILDER_PIDS: u64 = 32 * 1024;

impl Sandbox {
    /// The view for a self builder's command, with `dir` its one writable and
    /// kept directory. `dir` is made by `rsi::own_tree::builder_folder` from the
    /// session's own task, never from a call's input.
    pub fn builder_view(&self, dir: &Path) -> L1 {
        Self::with_builder_folder(self.job_view(), dir)
    }

    /// `view`, an L1 job's, with `dir` added as its one read-write directory
    /// and the builder's four allocations. Everything else of `view` (the call's own
    /// egress list included) is as it was.
    pub fn with_builder_folder(mut view: L1, dir: &Path) -> L1 {
        // A mask over a directory that holds the bind would cover the bind
        // (`self_build_view` says the same of the gate's binds): what the
        // directory holds that must stay hidden is listed on its own (the
        // store, the spool, the sealed sets).
        view.hidden.retain(|h| !(dir != h && dir.starts_with(h)));
        view.rw = vec![dir.to_path_buf()];
        let l = &mut view.limits;
        l.output_mb = l.output_mb.max(BUILDER_OUTPUT_MB);
        l.scratch_mb = l.scratch_mb.max(BUILDER_SCRATCH_MB);
        l.tmp_mb = l.tmp_mb.max(BUILDER_TMP_MB);
        l.pids = l.pids.max(BUILDER_PIDS);
        view
    }
}

/// What a self session's L1 command is given beyond an L1 job's view, each named by the daemon from
/// its own record of the session's task (`rsi::own_tree::self_places`), never from a call's input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelfPlaces {
    /// The item's build folder, read-write and kept (`with_builder_folder`).
    pub folder: Option<PathBuf>,
    /// The item's own tree, read-write beside the folder, its `.git` file read-only (`with_own_tree`).
    pub tree: Option<PathBuf>,
    /// The daemon's checkout of the head a reviewer reads, read-only.
    pub review: Option<PathBuf>,
}

impl Sandbox {
    /// The view a self session's L1 command runs in: `view` with `places` added, as `toolrun::job`
    /// gives it. The tree is bound only beside a build folder (a builder's).
    pub fn self_view(view: L1, places: &SelfPlaces) -> L1 {
        let mut view = match &places.folder {
            Some(dir) => Self::with_builder_folder(view, dir),
            None => view,
        };
        if let (Some(tree), Some(_)) = (&places.tree, &places.folder) {
            view = Self::with_own_tree(view, tree);
        }
        if let Some(t) = &places.review {
            view.hidden.retain(|h| !(t != h && t.starts_with(h)));
            view.ro_paths.push(t.clone());
        }
        view
    }

    /// `view` with `tree` bound read-write and its `.git` file bound read-only over it. The view
    /// mounts a deeper path after a shallower one (`theseus_sandbox::view`), so the `.git` bind lies
    /// on the tree's: the file cannot be written, removed, renamed or replaced (a mount point), and
    /// the tree itself cannot be renamed. The tree's git directory is in the repository, which stays
    /// an overlay.
    pub fn with_own_tree(mut view: L1, tree: &Path) -> L1 {
        view.hidden.retain(|h| !(tree != h && tree.starts_with(h)));
        view.rw.push(tree.to_path_buf());
        view.ro_paths.push(tree.join(".git"));
        view
    }
}

/// The environment a builder's command is given for its build folder `dir`, which replaces any a call
/// set: cargo's target directory (`dir/target`) and `TMPDIR` (`dir/tmp`), both on disk in the folder.
pub fn env(dir: &Path) -> Vec<(String, String)> {
    vec![
        (
            crate::rsi::builder_dir::TARGET_VAR.to_string(),
            crate::rsi::builder_dir::target_in(dir)
                .display()
                .to_string(),
        ),
        (
            crate::rsi::builder_dir::TMP_VAR.to_string(),
            crate::rsi::builder_dir::tmp_in(dir).display().to_string(),
        ),
    ]
}
