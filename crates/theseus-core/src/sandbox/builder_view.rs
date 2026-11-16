//! The view a self builder's `proc.run` runs in: an ordinary L1 job's view
//! (`Sandbox::job_view`) with one read-write directory added, the item's build
//! folder (`rsi::builder_dir`), so that what cargo builds there is kept
//! between commands. Nothing else changes: the same workspace overlays, the
//! same read-only paths and hidden list (less a mask over a directory that
//! holds the folder, as in the gate's view), no network beyond the call's own
//! egress list, the same limits but one ([`BUILDER_OUTPUT_MB`]), and none of
//! the facilities a self build's gate is given (`self_build::FACILITIES`). The gate's view
//! (`Sandbox::self_build_view`) never binds this directory, so what a builder
//! compiled is not there for the gate to read.

use std::path::Path;

use theseus_kernel::job::L1;

use super::Sandbox;

/// The largest single file a builder's command may write: 4 GiB, for the linker. A job's default
/// is `[sandbox] output_mb`, 64 MiB, and a linked debug build of the core's tests is far past it
/// (the 2026-11-15 run died on the first link, `SIGXFSZ`). One file this size can only land where
/// the view lets a command write: the build folder (bounded by `rsi::builder_dir::MAX_BYTES`,
/// which the daemon enforces while a command runs), scratch and `/tmp` (each its own size cap).
/// This is the view's own limit, set here; the configured `[sandbox] output_mb` is not changed,
/// and no other job's view has this one.
pub const BUILDER_OUTPUT_MB: u64 = 4 * 1024;

impl Sandbox {
    /// The view for a self builder's command, with `dir` its one writable and
    /// kept directory. `dir` is made by `rsi::own_tree::builder_folder` from the
    /// session's own task, never from a call's input.
    pub fn builder_view(&self, dir: &Path) -> L1 {
        Self::with_builder_folder(self.job_view(), dir)
    }

    /// `view`, an L1 job's, with `dir` added as its one read-write directory
    /// and the builder's file limit. Everything else of `view` (the call's own
    /// egress list included) is as it was.
    pub fn with_builder_folder(mut view: L1, dir: &Path) -> L1 {
        // A mask over a directory that holds the bind would cover the bind
        // (`self_build_view` says the same of the gate's binds): what the
        // directory holds that must stay hidden is listed on its own (the
        // store, the spool, the sealed sets).
        view.hidden.retain(|h| !(dir != h && dir.starts_with(h)));
        view.rw = vec![dir.to_path_buf()];
        view.limits.output_mb = view.limits.output_mb.max(BUILDER_OUTPUT_MB);
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
