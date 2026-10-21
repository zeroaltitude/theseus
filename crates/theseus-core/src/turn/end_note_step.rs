//! The end note (theseus-53ujg.1): a turn that would end leaving new files its
//! commands made beside the files its file tools wrote, or with files edited
//! after its last command ran, is told so once, in the harness's own voice,
//! and goes on for one more loop. Facts only: what exists and what changed,
//! never what to do. The stop nudge's shape (`nudge_step.rs`): one harness
//! node and its `turn.end_noted` row in one frame, `stop_reason` cleared.
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
        let facts = facts_of(t.tc.execution_id);
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
