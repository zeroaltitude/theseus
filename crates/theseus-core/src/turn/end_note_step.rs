//! The end note (theseus-53ujg.1): a turn that would end leaving new files its
//! commands made beside the files its file tools wrote, or with files edited
//! after its last command ran, is told so once, in the harness's own voice,
//! and goes on for one more loop. Facts only: what exists and what changed,
//! never what to do. The stop nudge's shape (`nudge_step.rs`): one harness
//! node and its `turn.end_noted` row in one frame, `stop_reason` cleared.
//!
//! What a build leaves is not what was left (theseus-pp5hi): a closed list of
//! residue names (`RESIDUE_DIRS`, `RESIDUE_FILES`, `RESIDUE_EXTS`) is not
//! counted toward the note, unless the task's own words name that path. A turn
//! whose every leftover is residue ends; the result's `files` still names them.
//! Edits no command ran since are never residue.
//!
//! Never in a shared place, with `[model] end_note` off, in a turn the stop
//! nudge already continued, while a job of the turn runs, at a strict loop
//! cap, in a turn that wrote nothing, or twice in one turn. The turn's result
//! carries the same facts as `files`, note or not (outside shared places).

use std::path::{Path, PathBuf};

use theseus_protocol::files_left::{EndNote, FilesLeft, PathList};

use super::*;
use crate::rewind;
use crate::toolrun::job::beside;

/// The note's author on its node.
pub const END_NOTE_AUTHOR: &str = "harness:end_note";

/// What the turn holds of its end note, and the drop of what the turn's files
/// were noted in (`rewind`'s writes, `beside`'s listings): every exit of the
/// turn drops them.
pub(super) struct Held {
    execution_id: String,
    /// The note sent, and the turn's tool calls before it.
    sent: Option<(EndNote, u32)>,
}

impl Held {
    pub(super) fn of(execution_id: &str) -> Self {
        beside::begin_turn(execution_id);
        rewind::begin_written(execution_id);
        Self {
            execution_id: execution_id.into(),
            sent: None,
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        rewind::forget_written(&self.execution_id);
        beside::forget_turn(&self.execution_id);
    }
}

/// What the note says is left and unrun, from the turn's own record.
struct Facts {
    left: Vec<PathBuf>,
    unrun: Vec<PathBuf>,
}

/// Directories a build or a test run leaves, by name.
const RESIDUE_DIRS: &[&str] = &["__pycache__", ".pytest_cache", ".venv", "build"];
/// Files a build leaves, by name.
const RESIDUE_FILES: &[&str] = &["a.out"];
/// Files a build leaves, by extension.
const RESIDUE_EXTS: &[&str] = &["so", "pyc"];

/// Whether `path` is on the closed residue list. Its own name only: a
/// listing's entries are the leftovers, and a residue-named directory above
/// the project says nothing about them.
fn is_residue(path: &Path) -> bool {
    let Some(file) = path.file_name().map(|f| f.to_string_lossy().into_owned()) else {
        return false;
    };
    RESIDUE_DIRS.contains(&file.as_str())
        || RESIDUE_FILES.contains(&file.as_str())
        || path
            .extension()
            .is_some_and(|e| RESIDUE_EXTS.contains(&e.to_string_lossy().as_ref()))
}

/// Whether `word` stands in `text` as its own token: not inside a longer name.
/// A bare word like `build` must touch a `/`, or any sentence would name it.
fn mentions(text: &str, word: &str) -> bool {
    let bare = word.chars().all(|c| c.is_ascii_alphabetic());
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(ident)
            && !after.is_some_and(ident)
            && (!bare || before == Some('/') || after == Some('/'))
    })
}

/// Whether the task's own words name `path`: its name, or (for an extension)
/// the extension itself.
fn named_by_task(prompt: &str, path: &Path) -> bool {
    path.file_name()
        .is_some_and(|f| mentions(prompt, &f.to_string_lossy()))
        || path.extension().is_some_and(|e| {
            let e = e.to_string_lossy();
            RESIDUE_EXTS.contains(&e.as_ref()) && mentions(prompt, &format!(".{e}"))
        })
}

/// The words the person and the parent said to this session, which name
/// what the task asked for.
fn task_words(t: &Turn<'_>) -> String {
    let store = t.tc.store;
    let nodes = store.session_nodes(t.tc.session_id).unwrap_or_default();
    nodes
        .into_iter()
        .filter(|(_, n)| n.origin != crate::node::Origin::Harness)
        .filter_map(|(_, n)| match n.body {
            Body::UserMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the turn left that a task would want kept: everything but the
/// closed list of build residue, and residue the task names.
fn kept(t: &Turn<'_>, left: Vec<PathBuf>) -> Vec<PathBuf> {
    if !left.iter().any(|p| is_residue(p)) {
        return left;
    }
    let words = task_words(t);
    left.into_iter()
        .filter(|p| !is_residue(p) || named_by_task(&words, p))
        .collect()
}

/// New files the turn's commands left that still exist in a directory its
/// file tools wrote to, and the files written after its last command ended.
fn facts_of(execution_id: &str) -> Facts {
    let written = rewind::written(execution_id);
    if written.is_empty() {
        return Facts {
            left: vec![],
            unrun: vec![],
        };
    }
    let ran = beside::facts(execution_id);
    let beside_edits = |p: &&PathBuf| {
        p.parent()
            .is_some_and(|d| written.iter().any(|w| w.path.parent() == Some(d)))
    };
    let left = ran
        .left
        .iter()
        .filter(beside_edits)
        .filter(|p| std::fs::symlink_metadata(p).is_ok())
        .cloned()
        .collect();
    let unrun = match ran.last_end_ms {
        Some(end) => written
            .iter()
            .filter(|w| w.at_ms > end)
            .map(|w| w.path.clone())
            .collect(),
        None => vec![],
    };
    Facts { left, unrun }
}

impl TurnRunner {
    /// The paths as the model reads them: scrubbed, at most `MAX_PATHS`.
    fn named(&self, paths: &[PathBuf]) -> String {
        let list = PathList::of(
            paths
                .iter()
                .map(|p| self.tools.scrubber.scrub(&p.display().to_string()).0)
                .collect(),
        );
        match list.more {
            0 => list.paths.join(", "),
            n => format!("{} and {n} more", list.paths.join(", ")),
        }
    }

    /// After the stop nudge declined: true when the turn goes on once more,
    /// told what it would have left. `i` is the loop's index.
    pub(super) fn end_note(&self, t: &mut Turn<'_>, i: u32) -> Result<bool> {
        if t.stop_reason != "no_tool_calls"
            || !self.cfg.model.end_note
            || t.nudge.sent()
            || t.end_note.sent.is_some()
        {
            return Ok(false);
        }
        let mut facts = facts_of(t.tc.execution_id);
        facts.left = kept(t, facts.left);
        if (facts.left.is_empty() && facts.unrun.is_empty()) || !self.may_continue(t)? {
            return Ok(false);
        }
        let mut said = vec!["The harness: this turn would end here.".to_string()];
        if !facts.left.is_empty() {
            said.push(format!(
                "Left beside its edits: {}.",
                self.named(&facts.left)
            ));
        }
        if !facts.unrun.is_empty() {
            said.push(format!(
                "Changed after the last command ran: {}.",
                self.named(&facts.unrun)
            ));
        }
        let note = EndNote {
            left: facts.left.len() as u32,
            unrun: facts.unrun.len() as u32,
            loop_index: i,
            acted: false,
        };
        let (sid, turn_id) = (t.tc.session_id, t.tc.turn_id);
        // The row and the node, in one frame: the row waits for the node's.
        t.record(&fact::turn::TurnEndNoted { note: &note });
        let node = Node::relayed(
            sid,
            Some(turn_id),
            crate::node::Origin::Harness,
            END_NOTE_AUTHOR,
            &format!("[{}]", said.join(" ")),
        );
        t.tc.store.append(&[node.record()?])?;
        t.tc.node_written(&node);
        t.end_note.sent = Some((note, t.tool_calls));
        t.stop_reason.clear();
        Ok(true)
    }

    /// The turn's result `files`: none in a shared place, and none when the
    /// turn wrote nothing and its commands left nothing.
    pub(super) fn files_left(&self, t: &Turn<'_>) -> Option<FilesLeft> {
        if t.tc.class == crate::places::PlaceClass::Shared {
            return None;
        }
        let text = |p: &Path| self.tools.scrubber.scrub(&p.display().to_string()).0;
        let changed: Vec<String> = rewind::written(t.tc.execution_id)
            .iter()
            .map(|w| text(&w.path))
            .collect();
        let left: Vec<String> = beside::facts(t.tc.execution_id)
            .left
            .iter()
            .filter(|p| std::fs::symlink_metadata(p).is_ok())
            .map(|p| text(p))
            .collect();
        let end_note = t.end_note.sent.as_ref().map(|(n, before)| EndNote {
            acted: t.tool_calls > *before,
            ..n.clone()
        });
        let files = FilesLeft {
            changed: PathList::of(changed),
            left: PathList::of(left),
            end_note,
            dir: t.tc.dir.map(|d| self.tools.scrubber.scrub(d).0),
        };
        (!files.is_empty() || files.end_note.is_some()).then_some(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The closed list, by a leftover's own name; a project directory called
    /// `build` above it changes nothing.
    #[test]
    fn residue_is_a_closed_list_of_names() {
        for p in [
            "/w/src/__pycache__",
            "/w/src/.pytest_cache",
            "/w/src/.venv",
            "/w/src/build",
            "/w/src/a.out",
            "/w/src/libx.so",
            "/w/src/mod.pyc",
        ] {
            assert!(is_residue(Path::new(p)), "{p}");
        }
        for p in [
            "/w/src/main",
            "/w/src/results.csv",
            "/w/src/model.bin",
            "/w/src/solver.o",
            "/w/build/proj/main",
            "/w/src/builder",
            "/w/src/libx.so.1",
        ] {
            assert!(!is_residue(Path::new(p)), "{p}");
        }
    }

    /// A task names a path as a token, never inside a longer name, and the
    /// bare word `build` only beside a slash.
    #[test]
    fn the_task_names_a_path_as_a_token() {
        assert!(named_by_task("Produce ./a.out.", Path::new("/w/a.out")));
        assert!(named_by_task(
            "Build src/libfoo.so, then test.",
            Path::new("/w/libfoo.so")
        ));
        assert!(named_by_task(
            "Ship any .so you need.",
            Path::new("/w/libbar.so")
        ));
        assert!(named_by_task(
            "Output goes in build/.",
            Path::new("/w/build")
        ));
        assert!(named_by_task(
            "Put it under src/build",
            Path::new("/w/build")
        ));
        assert!(!named_by_task(
            "Build the project and run it.",
            Path::new("/w/build")
        ));
        assert!(!named_by_task("Produce libfoo.so.", Path::new("/w/foo.so")));
        assert!(!named_by_task(
            "Use the .sock path.",
            Path::new("/w/libbar.so")
        ));
        assert!(!named_by_task(
            "Write main.py.",
            Path::new("/w/__pycache__")
        ));
    }
}
