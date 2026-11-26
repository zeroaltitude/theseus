//! The clearance of one exact graph before it leaves for the remote (`super`): the daemon's scrubber over what it
//! sends, then the outgoing check against what the item's sessions read. A branch's push makes its own
//! (`rsi::tree::push`, which runs main's scrub script too); the join's and the revert's use [`clear`]. What was
//! read is gathered once per item per pass of the join ([`Reads`]), in memory only.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use super::leave::{base_runs, leaving};
use super::{Deferral, Heard, Refusal, Unread, Why};
use crate::rsi::tree::Place;

/// Why a graph does not leave now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// It must not leave (words, a file not inspected, something that cannot be read): held for the owner.
    Refused(Refusal),
    /// The daemon's scrubber found this many secrets in it: held for the owner.
    Scrub(u32),
    /// The check is put off to a later pass: no hold, no card.
    Deferred(Deferral),
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Refused(r) => write!(f, "{r}"),
            Stop::Scrub(hits) => write!(
                f,
                "the daemon's scrubber found {hits} thing(s) that must not leave in what it would send"
            ),
            Stop::Deferred(d) => write!(f, "the check is put off to a later pass: {}", d.words()),
        }
    }
}

/// One exact graph about to leave: `base..tip`, read in `dir`, for `branch`.
pub struct Graph<'a> {
    pub place: &'a Place,
    pub dir: &'a Path,
    pub base: &'a str,
    pub tip: &'a str,
    pub branch: &'a str,
}

/// Whether `g` may leave: the scrub of what it sends, then the outgoing check against `heard`.
pub fn clear(
    g: &Graph<'_>,
    scrub: &dyn Fn(&str) -> u32,
    heard: Result<&Heard, Unread>,
) -> Result<(), Stop> {
    let out = leaving(g.place, g.dir, g.base, g.tip, g.branch).map_err(Stop::Refused)?;
    let hits = scrub(&out.scrub_text());
    if hits > 0 {
        return Err(Stop::Scrub(hits));
    }
    let heard = match heard {
        Ok(h) => h,
        Err(Unread::Defer(d)) => return Err(Stop::Deferred(d)),
        Err(Unread::Unresolved(w)) => return Err(Stop::Refused(Refusal::Unresolved(w))),
    };
    heard
        .check(&out, |wanted| base_runs(g.place, g.dir, g.base, wanted))
        .map_err(Stop::Refused)
}

/// What each item's sessions read, gathered at most once in one pass and kept nowhere else. Two passes are never
/// equal or unequal by it.
#[derive(Default, Clone)]
pub struct Reads {
    by_item: HashMap<String, Heard>,
}

impl Reads {
    /// What `item`'s sessions read, if this pass gathered it.
    pub fn get(&self, item: &str) -> Option<Heard> {
        self.by_item.get(item).cloned()
    }

    /// Keep what `item`'s sessions read for the rest of the pass (a failure is never kept).
    pub fn put(&mut self, item: &str, heard: Heard) {
        self.by_item.insert(item.to_string(), heard);
    }
}

impl PartialEq for Reads {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Reads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Reads({} items)", self.by_item.len())
    }
}

/// When a read started now must end ([`super::DEADLINE`]; a test may shorten it).
pub fn deadline() -> Instant {
    #[cfg(test)]
    if let Some(d) = tests_deadline::get() {
        return Instant::now() + d;
    }
    Instant::now() + super::DEADLINE
}

/// Whether `tip`'s tree is the tree of a commit already on main at `base`: a revert that sends nothing new.
pub fn tree_on_main(p: &Place, dir: &Path, base: &str, tip: &str) -> Result<bool, Why> {
    let tree = p
        .git_ok(dir, &["rev-parse", &format!("{tip}^{{tree}}")])
        .map_err(|_| Why::Commits)?;
    let trees = p
        .git_ok(dir, &["log", "--format=%T", base])
        .map_err(|_| Why::Base)?;
    Ok(trees.lines().any(|t| t.trim() == tree.trim()))
}

#[cfg(test)]
pub(crate) mod tests_deadline {
    use std::sync::Mutex;
    use std::time::Duration;

    static DEADLINE: Mutex<Option<Duration>> = Mutex::new(None);

    pub(crate) fn get() -> Option<Duration> {
        *DEADLINE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Every read's deadline, in this process, until the guard drops.
    pub(crate) fn set(d: Duration) -> impl Drop {
        *DEADLINE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(d);
        Reset
    }

    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            *DEADLINE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }
}
