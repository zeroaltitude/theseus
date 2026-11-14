//! A join leaves no tree open (issue 6, the install's reap and its guard).
//!
//! A tree a join phase opens for itself is closed before the phase ends: the keel check's,
//! the planted revert's, the install's, a probe's and a revert's. One left behind holds the
//! pipeline's slot for good (a 102 MB install tree stood open for fifteen hours, and every
//! start in that time was held on "slots" with nothing to clear it). The guard asks, before
//! each phase and at the end of the pass, whether any is still open. In a test it fails; on
//! the machine it says which tree, so the hold on the slot is explained and not a mystery.
//! The merge's own tree stays open across the phases that use it, and is checked at the end of
//! the pass only.

use std::path::Path;

use crate::rsi::tree::Place;

/// The trees one phase opens and closes.
const PHASE_TREES: &[&str] = &["keel-", "planted-", "install-", "probe-", "revert-"];
/// The merge's tree, open from the merge to the push.
const MERGE_TREE: &str = "join-";

/// The names of the join's trees open under the trees directory; with `merge`, the merge's too.
pub fn left_open(trees: &Path, merge: bool) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(trees) else {
        return Vec::new();
    };
    let mut names: Vec<String> = dir
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            PHASE_TREES.iter().any(|p| n.starts_with(p)) || (merge && n.starts_with(MERGE_TREE))
        })
        .collect();
    names.sort();
    names
}

/// Ask whether a join tree is open `when`. A test fails on one; the machine says which.
pub fn assert_closed(p: &Place, when: &str, merge: bool) {
    let open = left_open(&p.trees(), merge);
    if open.is_empty() {
        return;
    }
    let said = format!(
        "a join left a tree open {when}: {}, which holds the pipeline's slot until it is closed",
        open.join(", ")
    );
    if cfg!(test) {
        panic!("{said}");
    }
    tracing::warn!(trees = ?open, "self-join: {said}");
}
