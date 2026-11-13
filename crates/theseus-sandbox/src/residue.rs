//! The residue the mask leaves, and the bounded walk that narrows it
//! (quft-hidden-bind, the old theseus-quft).
//!
//! `view::mask` places a cover at a *path*, and a path is not an inode: a
//! hard link, a second bind, or any other name that reaches a covered inode
//! by a route no resolution of the hidden list derives is readable through
//! that other name. `view.rs`'s `mask` doc states the defect; this is the
//! answer the owner took on 2026-11-12, in his words: **a bounded walk,
//! inside §2.2's existing 25 ms start budget, which states that it is
//! bounded and not exhaustive.** The budget does not move.
//!
//! So the walk is bounded twice, by [`LOOKS`] names and by [`SLICE`] of
//! wall clock, whichever comes first, and what it did is a value
//! ([`Residue`]) that rides `report::Started` to the wrapper, lands in an
//! L1 completion's `detail.sandbox.residue`, and is a line at the head of
//! the job's result. **The statement is part of the behaviour**: a reader
//! is told what was covered, what was not walked, and that a clean walk is
//! no proof that no other name reaches a covered inode.
//!
//! Three things bound what the walk is asked to do, each for a reason off
//! `view.rs`:
//!
//! - **Only the live windows are walked** — `Part::ReadOnly` and
//!   `Part::ReadWrite`, the binds of the host's own trees. The view's own
//!   root, HOME, `/tmp`, `/dev/shm` and scratch hold the job's own bytes,
//!   so a name the job makes there is its own content and no leak.
//! - **A workspace overlay is not walked.** Its cover's target would be
//!   made in scratch's upper layer, which `View.scratch` keeps open
//!   precisely to report what the job wrote, so masking there would put the
//!   harness's own files in the job's write report (`report::Scratch`).
//!   That is `mask`'s own standing reason for leaving the workspace mask
//!   undone, and it is unchanged by this row.
//! - **The walk follows no symlink into a new tree and descends no mount it
//!   did not come from**: identity is `(st_dev, st_ino)`, and a name whose
//!   device is not the window's cannot be a second path to anything the
//!   window holds.
//!
//! A negative is not a proof of absence even when every window was walked
//! to its end, which is why [`Residue::say`] says so in every arm: a
//! `ro_paths` bind is a live window onto the host, so a name that reaches a
//! covered inode can be made on the host after the view was built. Closing
//! *that* needs a mechanism no mount provides (`view.rs`'s `mask` doc,
//! "the window's answer").

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How many names the walk may resolve, over all of a view's windows. A
/// name costs one `stat`, and a cold `stat` on this tree's own `ro_paths`
/// is the slowest thing the walk does; the count is the bound that makes
/// the walk's cost independent of how large the operator's trees are.
pub const LOOKS: u32 = 4096;

/// How much of the start the walk may spend. §2.2's budget for the whole
/// start is 25 ms and does not move, so the walk takes a slice of it and
/// stops: a window on a cold disk can exhaust this long before [`LOOKS`].
pub const SLICE: Duration = Duration::from_millis(8);

/// Why the walk stopped before it had seen every name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stopped {
    /// [`LOOKS`] names had been resolved.
    Looks,
    /// [`SLICE`] had been spent.
    Time,
}

impl Stopped {
    /// The bound, as the statement names it.
    pub fn as_str(self) -> &'static str {
        match self {
            Stopped::Looks => "its name limit",
            Stopped::Time => "its time slice",
        }
    }
}

/// What the bounded walk covered, and what it did not. The empty value is
/// a view with no live window to walk, which says so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Residue {
    /// The second paths to a covered inode that the walk covered.
    pub covered: u32,
    /// How many names it resolved.
    pub looked: u32,
    /// The windows it walked to the end.
    pub walked: u32,
    /// The windows it stopped short in, or never reached, by path.
    pub left: Vec<String>,
    /// The bound it stopped at, when it stopped at one.
    pub stopped: Option<Stopped>,
}

impl Residue {
    /// The statement, which is part of the behaviour (the owner,
    /// 2026-11-12): what the walk covered, what it did not, and that a
    /// clean walk proves no absence. One sentence, in the shape of the
    /// other `[L1: …]` lines at the head of a job's result.
    ///
    /// **Every arm ends with the same clause**, deliberately: a window is
    /// the host's own tree for as long as the job runs, so even a walk that
    /// saw every name saw it at one instant.
    pub fn say(&self) -> String {
        let mut s = String::from("[L1: the hidden list was masked by inode as well as by path: ");
        s += &match self.covered {
            0 => "no second path to a covered inode was found".to_string(),
            1 => "1 second path to a covered inode was covered too".to_string(),
            n => format!("{n} second paths to a covered inode were covered too"),
        };
        s += &format!(
            " ({} name{} resolved, {} window{} walked to the end",
            self.looked,
            plural(self.looked),
            self.walked,
            plural(self.walked)
        );
        if let Some(stop) = self.stopped {
            s += &format!(", and the walk stopped at {}", stop.as_str());
        }
        s += ")";
        if !self.left.is_empty() {
            s += &format!(". Not walked to the end: {}", self.left.join(", "));
        }
        s += ". The walk is bounded and not exhaustive, and a window is the \
             host's own tree while the job runs, so this is no proof that no \
             other name reaches a covered inode]";
        s
    }
}

fn plural(n: u32) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The walk's two bounds, spent one name at a time.
///
/// It is a value with its own clock so that the walk has one place to ask
/// "may I look at another name", and so that both bounds can be driven in a
/// unit test without a filesystem: [`Budget::of`] takes the room and the
/// deadline, and [`Budget::new`] is what the view uses.
pub(crate) struct Budget {
    left: u32,
    until: Instant,
}

impl Budget {
    /// The walk's own budget: [`LOOKS`] names, and [`SLICE`] from now.
    pub(crate) fn new() -> Budget {
        Budget::of(LOOKS, Instant::now() + SLICE)
    }

    pub(crate) fn of(looks: u32, until: Instant) -> Budget {
        Budget { left: looks, until }
    }

    /// Room for one more name, or the bound that is spent. Taking the room
    /// spends it, so a caller that is told yes has already paid.
    pub(crate) fn take(&mut self) -> Result<(), Stopped> {
        if self.left == 0 {
            return Err(Stopped::Looks);
        }
        if Instant::now() >= self.until {
            return Err(Stopped::Time);
        }
        self.left -= 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name bound is spent one name at a time and then refuses, and the
    /// refusal names which bound it was: the statement's words come off
    /// this, so a walk that stopped must be able to say why.
    #[test]
    fn the_name_bound_is_spent_one_name_at_a_time() {
        let mut b = Budget::of(3, Instant::now() + Duration::from_secs(60));
        for i in 0..3 {
            assert_eq!(b.take(), Ok(()), "name {i}");
        }
        assert_eq!(b.take(), Err(Stopped::Looks));
        // Spent stays spent: a second ask is the same answer, not a reset.
        assert_eq!(b.take(), Err(Stopped::Looks));
    }

    /// The time bound, driven by a deadline already past: with plenty of
    /// names left the walk still stops, and says it was the slice.
    #[test]
    fn the_time_bound_stops_a_walk_with_names_to_spare() {
        let mut b = Budget::of(LOOKS, Instant::now() - Duration::from_millis(1));
        assert_eq!(b.take(), Err(Stopped::Time));
        // And the name bound is still whole, so the two are independent.
        let mut b = Budget::of(LOOKS, Instant::now() + Duration::from_secs(60));
        assert_eq!(b.take(), Ok(()));
    }

    /// The statement says all three things in every arm (the owner's
    /// decision, 2026-11-12). Counted, not merely non-empty: a `say` that
    /// dropped the "no proof" clause from one arm would otherwise read as a
    /// pass.
    #[test]
    fn every_statement_says_what_it_covered_what_it_left_and_that_it_proves_no_absence() {
        let clean = Residue {
            covered: 0,
            looked: 311,
            walked: 2,
            left: vec![],
            stopped: None,
        };
        let cut = Residue {
            covered: 2,
            looked: LOOKS,
            walked: 1,
            left: vec!["/opt/shared".into()],
            stopped: Some(Stopped::Looks),
        };
        let said = [Residue::default().say(), clean.say(), cut.say()];
        let proves_no_absence = said
            .iter()
            .filter(|s| s.contains("bounded and not exhaustive") && s.contains("no proof"))
            .count();
        assert_eq!(proves_no_absence, said.len(), "{said:?}");
        let counts = said.iter().filter(|s| s.contains("name")).count();
        assert_eq!(counts, said.len(), "{said:?}");
        // The covered arm names its number, the clean arm says none, and
        // only the cut arm names a bound and what it left.
        assert!(clean.say().contains("no second path"), "{}", clean.say());
        assert!(cut.say().contains("2 second paths"), "{}", cut.say());
        assert!(
            cut.say().contains("its name limit") && cut.say().contains("/opt/shared"),
            "{}",
            cut.say()
        );
        assert!(!clean.say().contains("Not walked"), "{}", clean.say());
    }
}
