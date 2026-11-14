//! No self build starts while main is red (issue 6, the loop's failure handling,
//! condition 3).
//!
//! A build the loop starts on a red main fails on what main broke, spends a gate to learn
//! it, and counts as a failed try of an item that did nothing wrong. So before each start the
//! pipeline asks what main's own gate last said.
//!
//! **What "red" means here** (the first review's F2 changed it from "the newest `main` row"):
//! the newest recorded whole gate **of the commit main is at now** failed. The record is the
//! gate's phase-times log (`gate-times.csv`, the one the auto-revert watch reads too): a row
//! for the phase `total`, labelled `<branch> <git describe>`. A row counts when its commit is
//! main's head (`refs/remotes/origin/main` of `[self] repo`, a prefix match) and it was not
//! run on a dirty tree. The branch in the label does not matter: a gate run on a detached
//! checkout of main's head is labelled `HEAD …` and counts. **What is NOT read**: a self-join's own
//! gate, which runs in the sandbox, whose `HOME` is an empty tmpfs, so its `gate-times.csv` row is
//! lost with it; only gates run on the owner's machine are in the log. And a tagged commit's
//! `git describe` is the bare tag, naming no commit here, so a row for it is not read. Three
//! consequences, each from a way the first draft was wrong on a real log:
//!
//! - a red row of an **old** commit says nothing once main has moved on from it;
//! - a log with no row for main's head, or none that can be read, is nothing known, and builds
//!   start, so a machine that has never gated its main is not held for ever;
//! - the status words are the log's own: `ok` and `failed` (and `0` or a nonzero number, as the
//!   auto-revert's tests write them); any other word is not read.
//!
//! There is deliberately no word that overrides the hold: a later green gate of main's head
//! lifts it, and so does main moving on.

use std::path::PathBuf;

use crate::rsi::revert::signals::{fields, Files};
use crate::rsi::tree::Place;

/// Main's head gate, when it ended red.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Red {
    /// The abbreviated commit of main the gate judged.
    pub commit: String,
    /// When that gate ended (unix ms).
    pub at_ms: u64,
}

impl Red {
    /// The sentence a hold says (`theseus self pipeline`).
    pub fn why(&self) -> String {
        let at = crate::wake::local(self.at_ms);
        format!(
            "main's last gate was red at {} ({:04}-{:02}-{:02} {:02}:{:02}), so no self build \
             starts until a later gate of main is green or main moves on",
            self.commit, at.year, at.month, at.day, at.hour, at.minute
        )
    }
}

/// What the pipeline asks before a start.
pub trait Main: Send + Sync {
    /// Main's head gate when it was red; none when it was green or nothing is known.
    fn red(&self, place: &Place) -> Option<Red>;
}

/// The machine's own record: the gate's phase-times log, and git for main's head.
pub struct Recorded {
    pub gate_times: PathBuf,
}

impl Default for Recorded {
    fn default() -> Self {
        Self {
            gate_times: Files::default().gate_times,
        }
    }
}

impl Main for Recorded {
    fn red(&self, place: &Place) -> Option<Red> {
        // One git call, off the runtime's worker (the tick calls this synchronously).
        let head = theseus_store::blocking(|| {
            place
                .git_ok(
                    &place.repo,
                    &["rev-parse", "refs/remotes/origin/main^{commit}"],
                )
                .ok()
        })?;
        red_in(
            &std::fs::read_to_string(&self.gate_times).unwrap_or_default(),
            head.trim(),
        )
    }
}

/// No record is read: what a test's default is, so no test reads the machine's own log.
pub struct Unrecorded;

impl Main for Unrecorded {
    fn red(&self, _: &Place) -> Option<Red> {
        None
    }
}

/// The commit a label names: the last hex run of its `git describe` (`v1-4-g1a2b3c`, or a bare
/// abbreviation), `None` when the tree was dirty or the label names none.
fn commit_of(label: &str) -> Option<String> {
    let (_branch, describe) = label.trim().split_once(' ')?;
    let describe = describe.trim();
    if describe.ends_with("-dirty") {
        return None;
    }
    let tail = describe.rsplit('-').next().unwrap_or(describe);
    let hex = tail.strip_prefix('g').unwrap_or(tail);
    (hex.len() >= 7 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_string())
}

/// Whether a status word is a pass (`Some(false)`), a failure (`Some(true)`), or neither.
fn failed(status: &str) -> Option<bool> {
    match status {
        "ok" | "0" => Some(false),
        "failed" => Some(true),
        n => n.parse::<i64>().ok().map(|n| n != 0),
    }
}

/// The newest `total` row of a gate of `head` in the log (`time,label,phase,seconds,status`),
/// as a [`Red`] when it failed.
pub fn red_in(csv: &str, head: &str) -> Option<Red> {
    let head = head.to_ascii_lowercase();
    let (at_ms, commit, red) = csv
        .lines()
        .filter_map(|l| {
            let f = fields(l);
            let [time, label, phase, _secs, status] = f.as_slice() else {
                return None;
            };
            if phase != "total" {
                return None;
            }
            let commit = commit_of(label)?;
            head.starts_with(&commit.to_ascii_lowercase())
                .then_some(())?;
            let red = failed(status)?;
            let at_ms = crate::wake::parse_at(time).ok()?;
            Some((at_ms, commit, red))
        })
        .max_by_key(|(at_ms, _, _)| *at_ms)?;
    red.then_some(Red { commit, at_ms })
}
