//! A tree stays while a gate runs in it (issue 6, condition 7).
//!
//! Two self builds were aborted in one night because their tree vanished under a running
//! gate. A tree has several closers: the pipeline's tick closes the trees of items that are
//! failed, done or dropped, the joins close their merge trees, and a gate runs for half an
//! hour inside one. Nothing told a closer that a gate was still running there, so an item
//! whose record said "failed" (its execution ended, its owner dropped it) lost its tree and
//! its build tree under its own gate. The record cannot say when a gate is running, because a
//! gate is a child of the daemon, not a row.
//!
//! So the gate takes a [`Lease`] on the tree and its build tree for as long as it runs, and
//! [`super::tree::close`] refuses while one is held (the pipeline's tick leaves such a tree
//! alone and closes it on a later tick, once the gate is over). A lease is memory of this
//! process, which is the right scope: a gate dies with the daemon that started it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LEASED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A hold on some paths, released when it drops.
#[derive(Debug)]
pub struct Lease(Vec<PathBuf>);

impl Lease {
    /// Hold `paths` (a tree and its build tree) until the lease drops.
    pub fn take(paths: &[&Path]) -> Lease {
        let mut held = LEASED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mine: Vec<PathBuf> = paths.iter().map(|p| p.to_path_buf()).collect();
        held.extend(mine.iter().cloned());
        Lease(mine)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut held = LEASED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for p in &self.0 {
            if let Some(i) = held.iter().position(|h| h == p) {
                held.swap_remove(i);
            }
        }
    }
}

/// The first of `paths` a gate holds, if any.
pub fn held(paths: &[&Path]) -> Option<PathBuf> {
    let held = LEASED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    paths
        .iter()
        .find(|p| held.iter().any(|h| h == *p))
        .map(|p| p.to_path_buf())
}
