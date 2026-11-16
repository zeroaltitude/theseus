//! The view a self builder's `proc.run` runs in: an ordinary L1 job's view
//! (`Sandbox::job_view`) with one read-write directory added, the item's build
//! folder (`rsi::builder_dir`), so that what cargo builds there is kept
//! between commands. Nothing else changes: the same workspace overlays, the
//! same read-only paths and hidden list (less a mask over a directory that
//! holds the folder, as in the gate's view), no network beyond the call's own
//! egress list, the same limits but four ([`BUILDER_OUTPUT_MB`] and the constants beside it), and none of
//! the facilities a self build's gate is given (`self_build::FACILITIES`). The gate's view
//! (`Sandbox::self_build_view`) never binds this directory, so what a builder
//! compiled is not there for the gate to read.

use std::path::Path;

use theseus_kernel::job::L1;

use super::Sandbox;

// The allocations of a self builder's view. The owner, 2026-11-15 17:39:14 (chat message
// 1571686049867112519): "since our current mode is one-at-a-time self improvements, it means we can
// have relatively generous disk allocations and file allocations. so please extend those for
// flexibility over safety." One item builds at a time, so each is sized for one builder with the
// machine's build resources to itself. Each is a ceiling and costs nothing until it is written;
// `/tmp`, `/dev/shm`, HOME and scratch are memory-backed (tmpfs), so what is written there is
// memory. They are this view's own: the configured `[sandbox]` caps are not changed and no other
// job's view has them. `max` keeps a configured value that is already larger.

/// The largest single file a builder's command may write (`RLIMIT_FSIZE`): 16 GiB, as the gate's view
/// allows (`self_build::BUILD_OUTPUT_MB`): the link of the core's tests is far past a job's default
/// 64 MiB (the 2026-11-15 run died on it, `SIGXFSZ`), and a workspace's test binaries are large.
pub const BUILDER_OUTPUT_MB: u64 = 16 * 1024;
/// Scratch, the overlays' writes (cargo's registry index and package cache writes land here): 16 GiB,
/// where the job default is 1 GiB and the first real core test build filled it.
pub const BUILDER_SCRATCH_MB: u64 = 16 * 1024;
/// Each of `/tmp`, `/dev/shm` and HOME: 16 GiB, for the compiler's and linker's temporaries and a
/// test run's fixtures, from the job default of 1 GiB.
pub const BUILDER_TMP_MB: u64 = 16 * 1024;
/// Processes: 32,768, for parallel compilers and a test run's own children on a many-core box; the job
/// default is 512, and the gate's view is 8,192.
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

/// The environment a builder's command is given for its build folder: cargo's
/// target directory, which replaces any a call set.
pub fn env(dir: &Path) -> (String, String) {
    (
        crate::rsi::builder_dir::TARGET_VAR.to_string(),
        dir.display().to_string(),
    )
}
