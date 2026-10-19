//! What a turn changed and what its commands left (theseus-53ujg.1): the paths
//! its file tools wrote, the new files its commands left beside them, and the
//! one note the harness gave the model when the turn would have ended with
//! either. The result carries it as `files`; every surface shows its `line`
//! with the turn's other harness lines. A shared place's turn has none.

use serde::{Deserialize, Serialize};

use crate::TurnSubmitResult;

/// The most paths a list names; the rest are counted in `more`.
pub const MAX_PATHS: usize = 8;

/// Paths, capped: the first `MAX_PATHS`, and how many more there were.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PathList {
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl PathList {
    pub fn of(mut all: Vec<String>) -> Self {
        let more = all.len().saturating_sub(MAX_PATHS) as u32;
        all.truncate(MAX_PATHS);
        Self { paths: all, more }
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// `main, cmain and 3 more`, names relative to `base` when under it.
    fn words(&self, base: Option<&str>) -> String {
        let mut s = self
            .paths
            .iter()
            .map(|p| relative(p, base))
            .collect::<Vec<_>>()
            .join(", ");
        if self.more > 0 {
            s.push_str(&format!(" and {} more", self.more));
        }
        s
    }
}

fn relative(path: &str, base: Option<&str>) -> String {
    base.and_then(|b| path.strip_prefix(b.trim_end_matches('/')))
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// The end note a turn was given: what it named, the loop it stopped in, and
/// whether the model called a tool after it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EndNote {
    /// New files its commands left beside its edits.
    pub left: u32,
    /// Files edited after its last command ran.
    pub unrun: u32,
    /// The loop whose answer stopped.
    #[serde(rename = "loop")]
    pub loop_index: u32,
    #[serde(default)]
    pub acted: bool,
}

/// What a turn's file tools changed and its commands left.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FilesLeft {
    /// Files its file tools wrote, created ones included.
    pub changed: PathList,
    /// The new files its commands left that still exist.
    pub left: PathList,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub end_note: Option<EndNote>,
    /// The turn's working directory: the names a line shows are relative to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dir: Option<String>,
}

impl FilesLeft {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.left.is_empty()
    }

    /// The one line every surface shows: `Files: changed main.rs; left by
    /// commands: main, cmain`.
    pub fn line(&self) -> String {
        let base = self.dir.as_deref();
        let mut parts = Vec::new();
        if !self.changed.is_empty() {
            parts.push(format!("changed {}", self.changed.words(base)));
        }
        if !self.left.is_empty() {
            parts.push(format!("left by commands: {}", self.left.words(base)));
        }
        format!("Files: {}", parts.join("; "))
    }
}

impl TurnSubmitResult {
    /// The turn's files line, when its file tools changed any or its commands
    /// left any.
    pub fn files_line(&self) -> Option<String> {
        self.files
            .as_ref()
            .filter(|f| !f.is_empty())
            .map(FilesLeft::line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str]) -> PathList {
        PathList::of(names.iter().map(|n| n.to_string()).collect())
    }

    #[test]
    fn the_line_names_changes_and_leftovers_relative_to_the_directory() {
        let f = FilesLeft {
            changed: list(&["/app/poly/main.rs"]),
            left: list(&["/app/poly/main", "/app/poly/cmain"]),
            end_note: None,
            dir: Some("/app/poly".into()),
        };
        assert_eq!(
            f.line(),
            "Files: changed main.rs; left by commands: main, cmain"
        );
        let f = FilesLeft {
            left: PathList::of((0..11).map(|i| format!("/elsewhere/f{i}")).collect()),
            dir: Some("/app/poly".into()),
            ..FilesLeft::default()
        };
        assert_eq!(
            f.line(),
            "Files: left by commands: /elsewhere/f0, /elsewhere/f1, /elsewhere/f2, /elsewhere/f3, \
             /elsewhere/f4, /elsewhere/f5, /elsewhere/f6, /elsewhere/f7 and 3 more"
        );
    }

    #[test]
    fn a_turn_that_changed_and_left_nothing_has_no_line() {
        let mut r = TurnSubmitResult::default();
        assert!(r.files_line().is_none());
        r.files = Some(FilesLeft::default());
        assert!(r.files_line().is_none());
    }
}
