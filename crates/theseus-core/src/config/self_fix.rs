//! `[self_fix]`: the self loop's repair in place (issue 36; `crate::rsi::join::fix`).
//!
//! A self build whose gate is red in its own work, or whose review asks for a small repair, is
//! given once to a fixer on the reviewer's profile, whose one commit the builder's profile then
//! approves. **It is on by default**, and with `mode = "off"` the loop is exactly what it was.
//! The two bounds are the most a fix may change: lines added and removed, and files.
//!
//! **Why this is a section of its own and not `[self]`.** The keys belong with `[self]` by
//! subject, and the issue asked for them there. `[self]`'s Rust type lives in
//! `config/self_improve.rs`, which `scripts/keel-guard.py` lists in `KEEL_FILES`: a field on
//! `SelfImproveConfig` is a `keel-file` finding that only the owner's signature clears. So the
//! section stands beside it, as `[self_issues]` does. If the owner would rather have
//! `[self] fix`, `fix_lines` and `fix_files`, the move is his signature and these three keys.

use serde::{Deserialize, Serialize};

/// Whether a fix is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FixMode {
    /// A red gate in the change's own work, and a review's `fix`, are given to a fixer.
    #[default]
    On,
    /// Nothing is repaired in place: the loop as it was before issue 36.
    Off,
}

/// `[self_fix]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelfFixConfig {
    /// `on` (the default) or `off`.
    #[serde(default)]
    pub mode: FixMode,
    /// The most lines one fix may add and remove, together.
    #[serde(default = "default_lines")]
    pub lines: u64,
    /// The most files one fix may change.
    #[serde(default = "default_files")]
    pub files: u64,
    /// The wall-clock limit, in minutes, of one turn of a fixer, of the approver of a fix, and
    /// of an independent reviewer: past it the turn is stopped and counts as no answer.
    #[serde(default = "default_turn_minutes")]
    pub turn_minutes: u64,
}

/// A fix of 60 changed lines is a small repair, not a second change (the issue's default).
fn default_lines() -> u64 {
    60
}

fn default_files() -> u64 {
    6
}

/// An hour: a review that checks out the branch and builds its tests takes tens of minutes on a
/// cold build folder, and a fixer runs the tests it touches.
fn default_turn_minutes() -> u64 {
    60
}

impl Default for SelfFixConfig {
    fn default() -> Self {
        Self {
            mode: FixMode::On,
            lines: default_lines(),
            files: default_files(),
            turn_minutes: default_turn_minutes(),
        }
    }
}

impl SelfFixConfig {
    /// A config that says nothing serializes to nothing.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Whether a fix may be tried at all.
    pub fn on(&self) -> bool {
        self.mode == FixMode::On
    }
}

#[cfg(test)]
mod tests {
    use super::{FixMode, SelfFixConfig};

    #[test]
    fn it_is_on_with_its_bounds_by_default_and_off_is_read() {
        let c = SelfFixConfig::default();
        assert!(c.on());
        assert_eq!((c.lines, c.files, c.turn_minutes), (60, 6, 60));
        let off: SelfFixConfig = toml::from_str("mode = \"off\"\nlines = 10").unwrap();
        assert_eq!(off.mode, FixMode::Off);
        assert_eq!((off.lines, off.files), (10, 6));
        assert!(toml::from_str::<SelfFixConfig>("fix = \"on\"").is_err());
    }
}
