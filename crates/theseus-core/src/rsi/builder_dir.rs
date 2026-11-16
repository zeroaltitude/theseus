//! The builder's build folder: the one directory a self builder's `proc.run`
//! can write and keep (issue 20, "The builder cannot run the tests of what it
//! writes"; the owner's yes of 2026-11-15, message 1571650461218373648).
//!
//! The builder is a model session whose commands run in L1, where whatever a
//! command writes lands in scratch and is thrown away when it ends. A test
//! build of the core is about ten gigabytes, so the builder could not run one
//! (scratch is 1 GB) and, had it fit, every command would have started from
//! nothing. So while an item is being built its builder's commands get one
//! read-write directory, `<state>/self/builder/<slug>`, and cargo is pointed
//! at it (`CARGO_TARGET_DIR`, and `TMPDIR` for temporary files). Design
//! note: `docs/design/self-builder-build-folder.md`.
//!
//! - **Its own, and apart from the gate's.** The gate builds in
//!   `<state>/self/target/<slug>` ([`super::tree::Tree::target`]); this folder
//!   is a different directory, bound into no view the gate runs in, so what a
//!   model compiled never reaches a verdict.
//! - **Not a slot.** It is beside `<state>/self/trees`, never under it: a
//!   directory there counts as a tree in use (`capacity::trees_open`).
//! - **On the disk the capacity check reads.** `capacity::check` is given a
//!   path under `<state>/self`, so the free space it compares with
//!   `[self] min_free_gb` is the space this folder's bytes were taken from.
//! - **Removed with the tree.** `tree::close` removes it, and so does the
//!   stale-item cleanup (`pipeline::clear`).
//! - **Bounded** by [`MAX_BYTES`]: no command starts in a folder that is over
//!   it, and the daemon stops one that is running when the folder reaches it,
//!   each saying so in the result the builder reads.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The most disk a builder's build folder may hold: 48 GiB. Measured on a fleet box, a debug build of the
/// core's tests leaves 8.1 GiB, a build of the whole workspace's tests 32.6 GiB, and a package's
/// tests built on top of that one (another feature set) 37.9 GiB (`docs/design/self-builder-build-folder.md`).
/// So this holds the largest build the builder is asked to make with a quarter to spare. The owner's
/// yes was for about 10 GB an item, which is what an ordinary item uses; he has since asked for
/// generous allocations for the self builder (2026-11-15 17:39:14, chat message 1571686049867112519:
/// "larger disk allocations, etc. ... extend those for flexibility over safety"), and this stands.
///
/// **How it is enforced, and how well.** Nothing meters a command's writes as they happen (L1 has no
/// quota). The folder is measured when a command starts, which then refuses it, and by the daemon on
/// the heartbeat's timer (60 s by default, `Core::stop_jobs_over_builder_bound`), which stops a running
/// command. So a build can write past this by whatever it writes between two looks; the disk's floor
/// is the backstop there. A limit of this folder alone, and no existing cap's default.
pub const MAX_BYTES: u64 = 48 * 1024 * 1024 * 1024;

/// [`MAX_BYTES`]; a test sets a bound it can reach (`set_max_for_test`).
fn max() -> u64 {
    #[cfg(test)]
    {
        let m = TEST_MAX.load(std::sync::atomic::Ordering::Relaxed);
        if m != 0 {
            return m;
        }
    }
    MAX_BYTES
}

#[cfg(test)]
static TEST_MAX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A bound a test can fill, for this process.
#[cfg(test)]
pub(crate) fn set_max_for_test(bytes: u64) {
    TEST_MAX.store(bytes, std::sync::atomic::Ordering::Relaxed);
}

/// The variable cargo reads for where a build goes.
pub const TARGET_VAR: &str = "CARGO_TARGET_DIR";

/// The variable the compiler, the linker, cargo and `mktemp` read for temporary files.
pub const TMP_VAR: &str = "TMPDIR";

/// The directory that holds every item's build folder, under the state
/// directory `state`.
pub fn root_in(state: &Path) -> PathBuf {
    state.join("self").join("builder")
}

/// The folder for the item named `slug` under `state`.
pub fn dir_in(state: &Path, slug: &str) -> PathBuf {
    root_in(state).join(slug)
}

/// Where cargo builds inside the folder `dir`: `CARGO_TARGET_DIR`.
pub fn target_in(dir: &Path) -> PathBuf {
    dir.join("target")
}

/// Where a command's temporary files go inside the folder `dir`: `TMPDIR`, which the compiler, the
/// linker, cargo and `mktemp` follow. It is on disk, under [`MAX_BYTES`], so what is large lands
/// there and not in the view's memory-backed `/tmp` (`sandbox::builder_view`).
pub fn tmp_in(dir: &Path) -> PathBuf {
    dir.join("tmp")
}

/// The folder for `slug`, made if it was not there, with its `target` and `tmp` directories. Made
/// again at each command's start: a `cargo clean` empties `target`, and a command may remove either.
pub fn ensure(state: &Path, slug: &str) -> std::io::Result<PathBuf> {
    let dir = dir_in(state, slug);
    std::fs::create_dir_all(target_in(&dir))?;
    std::fs::create_dir_all(tmp_in(&dir))?;
    Ok(dir)
}

/// Remove the folder for `slug`, and say what it held: the bytes freed, or
/// none when there was no folder. Never an error for a folder that is gone.
pub fn remove(state: &Path, slug: &str) -> std::io::Result<Option<u64>> {
    let dir = dir_in(state, slug);
    if !dir.exists() {
        return Ok(None);
    }
    let held = size(&dir);
    super::tree::remove_all(&dir)?;
    Ok(Some(held))
}

/// The disk `dir` holds, in bytes, as `du` counts it (blocks, not lengths: a
/// debug build's sparse files fill what they are allotted). Links are
/// counted and never followed; what cannot be read counts for nothing.
pub fn size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(m) = e.path().symlink_metadata() else {
                continue;
            };
            total = total.saturating_add(m.blocks().saturating_mul(512));
            if m.is_dir() {
                stack.push(e.path());
            }
        }
    }
    total
}

/// A folder that has reached its bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Full {
    pub dir: PathBuf,
    pub bytes: u64,
}

impl Full {
    /// Whether `dir` is at or over [`MAX_BYTES`].
    pub fn of(dir: &Path) -> Option<Self> {
        let bytes = size(dir);
        (bytes >= max()).then(|| Self {
            dir: dir.to_path_buf(),
            bytes,
        })
    }

    /// What the builder reads when a command is refused for it.
    pub fn refusal(&self) -> String {
        format!(
            "the build folder for this item holds {} GiB and has reached its limit of {} GiB, so no \
             command starts until it is smaller. Build output is in {} (cargo's CARGO_TARGET_DIR: \
             `cargo clean` empties it) and temporary files in {} (TMPDIR): delete what is in them, \
             then run the command again. Nothing else was wrong with the command.",
            gb(self.bytes),
            gb(max()),
            target_in(&self.dir).display(),
            tmp_in(&self.dir).display()
        )
    }

    /// Who a running command was stopped by, as its result says it.
    pub fn stopped_by(&self) -> String {
        format!(
            "the build folder's limit (the item's build folder holds {} GiB, at its limit of {} GiB; \
             `cargo clean` empties its build output, and its temporary files are in its tmp \
             directory)",
            gb(self.bytes),
            gb(max())
        )
    }
}

fn gb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1u64 << 30) as f64)
}
