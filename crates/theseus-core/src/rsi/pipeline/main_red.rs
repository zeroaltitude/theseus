//! No self build starts while main is red (issue 6, the loop's failure handling,
//! condition 3).
//!
//! A build the loop starts on a red main fails on what main broke, spends a gate to learn
//! it, and counts as a failed try of an item that did nothing wrong. So before each start the
//! pipeline asks what main's own gate last said.
//!
//! **What "red" means here**: the newest recorded run of main's whole gate ended with a
//! failure, and no later run of main's gate has passed. The record is the gate's phase-times
//! log (`gate-times.csv`, the one the auto-revert watch reads too): a row for the phase
//! `total` whose label starts `main `. With no such row, or none that can be read, nothing is
//! known and builds start, so a machine that has never logged a gate is not held for ever.
//! There is deliberately no word that overrides the hold: a later green gate of main lifts it.

use std::path::PathBuf;

use crate::rsi::revert::signals::{fields, main_commit, Files};

/// Main's last whole gate, when it ended red.
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
             starts until a later gate of main is green",
            self.commit, at.year, at.month, at.day, at.hour, at.minute
        )
    }
}

/// What the pipeline asks before a start.
pub trait Main: Send + Sync {
    /// Main's last whole gate, when it was red; none when it was green or nothing is known.
    fn red(&self) -> Option<Red>;
}

/// The machine's own record: the gate's phase-times log.
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
    fn red(&self) -> Option<Red> {
        red_in(&std::fs::read_to_string(&self.gate_times).unwrap_or_default())
    }
}

/// No record is read: what a test's default is, so no test reads the machine's own log.
pub struct Unrecorded;

impl Main for Unrecorded {
    fn red(&self) -> Option<Red> {
        None
    }
}

/// The newest `total` row of a gate of main in the log (`time,label,phase,seconds,status`),
/// as a [`Red`] when its status is not 0.
pub fn red_in(csv: &str) -> Option<Red> {
    let newest = csv
        .lines()
        .filter_map(|l| {
            let f = fields(l);
            let [time, label, phase, _secs, status] = f.as_slice() else {
                return None;
            };
            let at_ms = crate::wake::parse_at(time).ok()?;
            let commit = main_commit(label)?;
            (phase == "total").then(|| (at_ms, commit, status != "0"))
        })
        .max_by_key(|(at_ms, _, _)| *at_ms)?;
    let (at_ms, commit, red) = newest;
    red.then_some(Red { commit, at_ms })
}
