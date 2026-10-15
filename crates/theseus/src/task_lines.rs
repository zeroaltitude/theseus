//! The task lines of a line-mode conversation (theseus-mnd9g): `theseus chat
//! --plain`, a pipe, or `TERM=dumb` runs `watch --interactive`, which prints
//! one line to stderr for each event that matters of the conversation's
//! tasks — started (with its title), asks (the question and the commands that
//! answer it), reported (the first line), failed (why, in words) — the same
//! words the chat's footer screen prints (`render::task_tree::event_line`).
//! Pure: a snapshot or a frame's changes in, lines out.

use std::collections::{HashMap, HashSet};

use theseus_client::render::task_tree::{self, Event};
use theseus_protocol::work::WorkView;
use theseus_protocol::{WorkChanged, WorkWatchResult};

/// The commands under a question fit this many columns, as a terminal wide
/// enough for the long form.
const COMMAND_ROOM: usize = 200;

#[derive(Default)]
pub struct TaskLines {
    views: HashMap<String, WorkView>,
    /// Questions told already: a snapshot after a reconnect tells none twice.
    told: HashSet<String>,
}

impl TaskLines {
    /// The board as it stands: nothing is said of what runs, and a question
    /// that waits is told once, with its commands.
    pub fn snapshot(&mut self, r: &WorkWatchResult) -> Vec<String> {
        self.views = r.views.iter().map(|v| (v.id.clone(), v.clone())).collect();
        let mut out = Vec::new();
        let mut open: Vec<&WorkView> = self
            .views
            .values()
            .filter(|v| v.kind == theseus_protocol::work::WorkKind::Task && !v.state.ended())
            .collect();
        open.sort_by_key(|v| (v.started_at_ms, v.id.clone()));
        for v in open {
            if let Some(q) = &v.question {
                if self.told.insert(q.id.clone()) {
                    out.extend(asked(&Event::Asked(v, q)));
                }
            }
        }
        out
    }

    /// A frame's changes, by the position rule.
    pub fn changed(&mut self, c: &WorkChanged) -> Vec<String> {
        let mut out = Vec::new();
        for v in &c.views {
            let prev = self.views.get(&v.id);
            if prev.is_some_and(|p| p.position >= v.position) {
                continue;
            }
            for e in task_tree::events(prev, v) {
                if let Event::Asked(w, q) = &e {
                    if w.kind != theseus_protocol::work::WorkKind::Task
                        || !self.told.insert(q.id.clone())
                    {
                        continue;
                    }
                    out.extend(asked(&e));
                } else if matches!(&e, Event::Started(w) | Event::Reported(w)
                    | Event::Failed(w) | Event::Cancelled(w) if w.kind
                        == theseus_protocol::work::WorkKind::Task)
                {
                    out.push(task_tree::event_line(&e).text);
                    out.extend(
                        task_tree::event_commands(&e, COMMAND_ROOM).map(|c| format!("  {c}")),
                    );
                }
            }
            self.views.insert(v.id.clone(), v.clone());
        }
        for id in &c.gone {
            self.views.remove(id);
        }
        out
    }
}

/// A question's line and the commands that answer it.
fn asked(e: &Event) -> Vec<String> {
    let Event::Asked(_, q) = e else {
        return Vec::new();
    };
    vec![
        task_tree::event_line(e).text,
        format!("  {}", task_tree::answer_commands(q, COMMAND_ROOM)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn view(v: Value) -> WorkView {
        let mut base = json!({
            "id": "exe_1", "kind": "task", "position": 9, "at_ms": 9, "root": "ses_c",
            "parent": "ses_c", "title": "", "state": "running",
            "attention": {"level": "working", "label": "turn 1", "since_ms": 5},
            "started_at_ms": 1, "state_since_ms": 5
        });
        for (k, x) in v.as_object().unwrap() {
            base[k] = x.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    fn asking(position: u64) -> WorkView {
        view(json!({
            "id": "exe_paint", "session_id": "ses_paint2a329e", "title": "Paint log",
            "state": "needs_you", "position": position,
            "attention": {"level": "needs_you", "label": "confirm fs.write", "since_ms": 5},
            "question": {"id": "act_x2a329e", "kind": "approval", "prompt": "write regatta/paint-log.md (37 B)",
                "asked_by": {"id": "exe_paint", "kind": "task", "title": "Paint log"},
                "options": [
                    {"id": "approve", "label": "Approve", "style": "primary", "takes_note": false, "effect": "x"},
                    {"id": "decline", "label": "Decline", "style": "plain", "takes_note": true, "effect": "x"}],
                "floor": false, "may_answer": true}
        }))
    }

    fn frame(position: u64, views: Vec<WorkView>) -> WorkChanged {
        WorkChanged {
            position,
            at_ms: 0,
            views,
            gone: vec![],
            threads: vec![],
        }
    }

    /// Five tasks print five titled `started` lines; a question is told once,
    /// with the commands; a report and a failure are told in words; the same
    /// frame again is told nothing.
    #[test]
    fn each_task_event_is_one_line_and_a_question_has_its_commands() {
        let mut t = TaskLines::default();
        let snap = WorkWatchResult {
            position: 1,
            views: vec![],
            total: 0,
            threads: vec![],
        };
        assert!(t.snapshot(&snap).is_empty());
        let names = [
            "Paint log",
            "Buoy survey",
            "Tide tables",
            "Rigging check",
            "Weather brief",
        ];
        let mut lines = Vec::new();
        for (i, n) in names.iter().enumerate() {
            let v = view(
                json!({"id": format!("exe_{i}"), "session_id": format!("ses_abc{i}def"),
                "title": n, "position": 10 + i as u64}),
            );
            lines.extend(t.changed(&frame(10 + i as u64, vec![v])));
        }
        assert_eq!(
            lines,
            [
                "◐ started: Paint log (bc0def)",
                "◐ started: Buoy survey (bc1def)",
                "◐ started: Tide tables (bc2def)",
                "◐ started: Rigging check (bc3def)",
                "◐ started: Weather brief (bc4def)",
            ]
        );
        let ask = t.changed(&frame(20, vec![asking(20)]));
        assert_eq!(
            ask,
            [
                "◐ started: Paint log (2a329e)",
                "● Paint log asks: write regatta/paint-log.md (37 B)?",
                "  theseus answer 2a329e approve · theseus answer 2a329e decline --note …",
            ]
        );
        assert!(t.changed(&frame(20, vec![asking(20)])).is_empty(), "twice");
        let done = view(json!({"id": "exe_paint", "session_id": "ses_paint2a329e",
            "title": "Paint log", "state": "done", "position": 21,
            "attention": {"level": "idle", "label": "complete", "since_ms": 6},
            "report": {"outcome": "done", "first_line": "The hull is red."}}));
        assert_eq!(
            t.changed(&frame(21, vec![done])),
            ["✓ Paint log reported: The hull is red."]
        );
    }

    /// A reconnect's snapshot tells a waiting question once, never twice.
    #[test]
    fn a_snapshot_tells_a_waiting_question_once() {
        let mut t = TaskLines::default();
        let snap = WorkWatchResult {
            position: 1,
            views: vec![asking(3)],
            total: 1,
            threads: vec![],
        };
        assert_eq!(t.snapshot(&snap).len(), 2);
        assert!(t.snapshot(&snap).is_empty());
    }
}
