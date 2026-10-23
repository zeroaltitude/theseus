//! The draft trees' root among the workspace roots (theseus-iilre).

use std::path::{Path, PathBuf};

/// `guarded_paths`, with the draft trees' root among the workspace roots
/// while a class drafts: the review's runs in a tree take the tool's posture,
/// and the state dir's approve-list hold does not reach them.
pub(crate) fn guarded_with_drafts(
    cfg: &crate::Config,
    state: &Path,
) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    let (mut roots, floor, approve) = super::guarded_paths(cfg, state);
    roots.extend(crate::config::routing_cn::drafts_root(cfg));
    (roots, floor, approve)
}
