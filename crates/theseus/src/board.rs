//! The board `theseus tasks` draws (theseus-6kgp8). `theseus work` is an alias of `theseus tasks --watch`, so
//! `tasks` holds what the work board showed: the task tree is the body, and beside it come a `Jobs` list with
//! each job's id (what `peek` and `attach` take), and the work that is not a task: the daemon's own jobs,
//! routines, watches, projects and wakes. `--watch` prints a line for each of those that changes, as `work
//! --watch` did: a job, a wake, a routine, a watch, a project, the daemon's own work. A step's change and a
//! question's are the task's own lines (its plan count, its ask).

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde_json::Value;
use theseus_client::render::task_tree;
use theseus_protocol::notices::{Notice, Why};
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{notify, Event, WorkWatchResult};

use crate::work::{self, Seen};

/// Whether a view is a line of the board's own list (a job, a wake, a routine, a watch, a project, the
/// daemon's work), rather than a task's part.
fn listed(v: &WorkView) -> bool {
    !matches!(
        v.kind,
        WorkKind::Conversation | WorkKind::Task | WorkKind::Step | WorkKind::Question
    )
}

/// The snapshot as `tasks` prints it.
pub fn lines(snap: &WorkWatchResult, now_ms: u64, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    // `tree_lines_at` takes the roots the board holds: the ones the snapshot does not carry
    // (`--limit`) are counted too.
    let parentless = snap.views.iter().filter(|v| v.parent.is_none()).count() as u64;
    let total = parentless.max(snap.total);
    let tree = crate::tasks_tree::tree_lines_at(&snap.views, now_ms, width, total);
    if tree.as_slice() != ["no tasks"] {
        out.extend(tree);
    }
    // A conversation's own question: the tree draws its answer commands but not what it asks, so the
    // board says it (the work board listed every question).
    let asking: Vec<&WorkView> = snap
        .views
        .iter()
        .filter(|v| v.kind == WorkKind::Question && !v.state.ended())
        .filter(|v| {
            v.parent.as_deref().is_some_and(|p| {
                snap.views
                    .iter()
                    .any(|c| c.id == p && c.kind == WorkKind::Conversation)
            })
        })
        .collect();
    if !asking.is_empty() {
        out.push("Asking (theseus answer ID):".to_string());
        for q in asking {
            out.push(format!("  {}  {}", short_id(q), work::view_line(q, now_ms)));
        }
    }
    let jobs: Vec<&WorkView> = snap
        .views
        .iter()
        .filter(|v| v.kind == WorkKind::Job && !under_system(v, &snap.views))
        .collect();
    if !jobs.is_empty() {
        out.push("Jobs (theseus peek JOB · theseus attach JOB):".to_string());
        for j in jobs {
            out.push(format!("  {}  {}", short_id(j), work::view_line(j, now_ms)));
        }
    }
    let own = daemon_and_schedules(&snap.views);
    if !own.is_empty() {
        out.extend(work::tree_lines(&own, now_ms));
    }
    if out.is_empty() {
        out.push("no tasks".to_string());
    }
    out
}

/// A job's id as `peek` and `attach` take it: the last six characters of the whole.
pub fn short_id(v: &WorkView) -> &str {
    &v.id[v.id.len().saturating_sub(6)..]
}

/// Whether a view sits under the daemon's own root (its jobs are told in that tree).
fn under_system(v: &WorkView, views: &[WorkView]) -> bool {
    let by_id: HashMap<&str, &WorkView> = views.iter().map(|v| (v.id.as_str(), v)).collect();
    let mut at = v;
    let mut seen = HashSet::new();
    while let Some(p) = at.parent.as_deref().and_then(|p| by_id.get(p)) {
        if p.kind == WorkKind::System || !seen.insert(p.id.as_str()) {
            return p.kind == WorkKind::System;
        }
        at = p;
    }
    false
}

/// The views that are the daemon's own work, a routine, a watch, a project or a wake, and what is
/// under the daemon's root: lines the task tree does not draw.
fn daemon_and_schedules(views: &[WorkView]) -> Vec<WorkView> {
    views
        .iter()
        .filter(|v| {
            matches!(
                v.kind,
                WorkKind::Wake | WorkKind::Routine | WorkKind::Watch | WorkKind::Project
            ) || v.kind == WorkKind::System
                || (v.kind == WorkKind::Job && under_system(v, views))
        })
        .cloned()
        .collect()
}

/// What `tasks --watch` has printed so far, so a notification prints only what is new.
pub struct Follow {
    /// The last view of each id, for the task events.
    last: HashMap<String, WorkView>,
    /// What was applied of each other view: its position, then its time (a job's last line is sampled
    /// between frames, theseus-e1hu4).
    seen: Seen,
}

impl Follow {
    pub fn new(snap: &WorkWatchResult) -> Self {
        let mut f = Follow {
            last: HashMap::new(),
            seen: Seen::new(),
        };
        f.reset(snap);
        f
    }

    /// Start again from a snapshot (after `events.lost`).
    pub fn reset(&mut self, snap: &WorkWatchResult) {
        self.last = snap
            .views
            .iter()
            .map(|v| (v.id.clone(), v.clone()))
            .collect();
        self.seen = snap
            .views
            .iter()
            .map(|v| (v.id.clone(), work::stamp(v)))
            .collect();
    }

    /// One notification, printed (`--json` prints the notification itself).
    pub fn show(&mut self, m: &str, p: &Value, json: bool) -> Result<()> {
        if json {
            if m == notify::WORK_CHANGED || m == notify::WORK_NOTICE {
                println!("{}", serde_json::json!({"method": m, "params": p}));
            }
            return Ok(());
        }
        for l in self.words(m, p)? {
            println!("{l}");
        }
        Ok(())
    }

    /// One notification, as lines: a line for each task event of each view newer than the last
    /// told for its id, a change line for each other listed view, each id that left, and each
    /// notice that is not a task's own event.
    pub fn words(&mut self, m: &str, p: &Value) -> Result<Vec<String>> {
        let mut out = Vec::new();
        match Event::from_notification(m, p)? {
            Some(Event::WorkChanged(c)) => {
                let now = theseus_protocol::now_unix_ms();
                for v in &c.views {
                    if listed(v) {
                        let seen = self.seen.entry(v.id.clone()).or_default();
                        if work::stamp(v) > *seen {
                            *seen = work::stamp(v);
                            out.push(work::change_line(v, now));
                        }
                        self.last.insert(v.id.clone(), v.clone());
                        continue;
                    }
                    let prev = self.last.get(&v.id);
                    if prev.is_some_and(|p| p.position >= v.position) {
                        continue;
                    }
                    for e in task_tree::events(prev, v) {
                        out.push(task_tree::event_line(&e).text);
                        if let Some(c) = task_tree::event_commands(
                            &e,
                            theseus_client::render::style::terminal_width(),
                        ) {
                            out.push(format!("  {c}"));
                        }
                    }
                    self.last.insert(v.id.clone(), v.clone());
                }
                for id in &c.gone {
                    if self.last.remove(id).is_some_and(|v| listed(&v)) {
                        out.push(format!("@{} {id} left the board", c.position));
                    }
                    self.seen.remove(id);
                }
            }
            Some(Event::WorkNotice(n)) if !told_by_events(&n) => out.push(work::notice_line(&n)),
            _ => {}
        }
        Ok(out)
    }
}

/// Whether the view's change already printed this notice's words: a
/// question, a failure, a block and a task's report are task events.
fn told_by_events(n: &Notice) -> bool {
    matches!(
        n.why,
        Why::Asked | Why::Failed | Why::Blocked | Why::Reported | Why::Answered
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use super::lines as board_lines;

    fn view(v: Value) -> WorkView {
        let mut base = json!({
            "id": "exe_1", "kind": "conversation", "position": 9, "at_ms": 9, "root": "exe_1",
            "title": "", "state": "running",
            "attention": {"level": "working", "label": "turn 3", "since_ms": 5},
            "started_at_ms": 1, "state_since_ms": 5
        });
        for (k, x) in v.as_object().unwrap() {
            base[k] = x.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    fn snap(views: Vec<WorkView>) -> WorkWatchResult {
        WorkWatchResult {
            position: 9,
            total: views.iter().filter(|v| v.parent.is_none()).count() as u64,
            views,
            threads: vec![],
        }
    }

    fn changed(views: Vec<Value>, position: u64, extra: Value) -> Value {
        let mut p = json!({"position": position, "at_ms": 1_000, "views": views});
        for (k, x) in extra.as_object().unwrap() {
            p[k] = x.clone();
        }
        p
    }

    fn job_json(position: u64, at_ms: u64, last: &str) -> Value {
        json!({"id": "act_job_abcdef", "kind": "job", "parent": "exe_task", "root": "exe_1",
            "title": "cargo test -p core", "started_at_ms": 7, "position": position, "at_ms": at_ms,
            "state": "running", "state_since_ms": 7,
            "doing": {"what": "job", "label": "cargo test -p core", "since_ms": 7,
                "turn": 1, "loop_index": 0, "last_line": last},
            "attention": {"level": "working", "label": "running", "since_ms": 7}})
    }

    /// A job is a line of the Jobs list with the id `peek` and `attach` take; the daemon's own work, a
    /// wake and a routine stay lines of the board (nothing the work board showed is gone).
    #[test]
    fn the_jobs_wakes_and_the_daemons_own_work_are_listed() {
        let sys = |v: Value| {
            let mut base = json!({"kind": "system", "root": "system", "parent": "system"});
            for (k, x) in v.as_object().unwrap() {
                base[k] = x.clone();
            }
            view(base)
        };
        let snap = snap(vec![
            view(json!({"id": "exe_1", "title": "Beta summary"})),
            view(
                json!({"id": "exe_task", "kind": "task", "parent": "exe_1", "title": "Read the buoys",
                "attention": {"level": "working", "label": "turn 1", "since_ms": 3}}),
            ),
            serde_json::from_value(job_json(9, 9, "test result: 41 passed")).unwrap(),
            view(
                json!({"id": "wak_1", "kind": "wake", "parent": "exe_1", "title": "Check the tide",
                "state": "ready", "attention": {"level": "ready", "label": "wakes 18:00", "since_ms": 1}}),
            ),
            view(
                json!({"id": "rtn_1", "kind": "routine", "parent": null, "title": "Morning brief",
                "state": "ready", "attention": {"level": "ready", "label": "daily 08:00", "since_ms": 1}}),
            ),
            sys(
                json!({"id": "system", "parent": null, "title": "Theseus", "state": "ready",
                "attention": {"level": "ready", "label": "ready", "since_ms": 5},
                "below": {"needs_you": 0, "working": 1, "done": 0, "failed": 0, "total": 1}}),
            ),
            sys(
                json!({"id": "sys_a", "title": "Importing Claude Code history",
                "attention": {"level": "working", "label": "batch 41 of 120", "since_ms": 5}}),
            ),
        ]);
        let text = board_lines(&snap, 7 + 5_400, 100).join("\n");
        assert!(
            text.contains(
                "Jobs (theseus peek JOB · theseus attach JOB):\n  abcdef  ◐ cargo test -p core"
            ),
            "{text}"
        );
        assert!(text.contains("› test result: 41 passed"), "{text}");
        assert!(text.contains("○ Check the tide  wakes 18:00"), "{text}");
        assert!(text.contains("○ Morning brief  daily 08:00"), "{text}");
        assert!(text.contains("⚙ Theseus  (1 working)"), "{text}");
        assert!(
            text.contains("◐ Importing Claude Code history · batch 41 of 120"),
            "{text}"
        );
        // The daemon's job is told under its root, not twice in the Jobs list.
        assert_eq!(
            text.matches("Importing Claude Code history").count(),
            1,
            "{text}"
        );
    }

    /// `--watch` tells a job's change once, not a stale sample, a departure, and a task's own
    /// events as `tasks --watch` always did.
    #[test]
    fn the_watch_tells_each_change_of_a_job_and_a_task_once() {
        let mut f = Follow::new(&snap(vec![]));
        let m = notify::WORK_CHANGED;

        let said = f
            .words(
                m,
                &changed(vec![job_json(10, 20, "compiling")], 10, json!({})),
            )
            .unwrap();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("@10 job abcdef running"), "{said:?}");
        assert!(said[0].contains("› compiling"), "{said:?}");
        // The same frame again, and an older sample of it, say nothing.
        assert!(f
            .words(
                m,
                &changed(vec![job_json(10, 20, "compiling")], 10, json!({}))
            )
            .unwrap()
            .is_empty());
        assert!(f
            .words(m, &changed(vec![job_json(10, 19, "older")], 10, json!({})))
            .unwrap()
            .is_empty());
        // A later sample of the same frame is a line.
        let later = f
            .words(
                m,
                &changed(vec![job_json(10, 30, "linking")], 10, json!({})),
            )
            .unwrap();
        assert!(later[0].contains("› linking"), "{later:?}");
        // A task that starts: its own event line.
        let task = json!({"id": "exe_buoy", "kind": "task", "parent": "exe_1", "root": "exe_1",
            "session_id": "ses_buoy5a877c", "title": "Buoy survey", "position": 12, "at_ms": 40,
            "state": "running", "attention": {"level": "working", "label": "turn 1", "since_ms": 3},
            "started_at_ms": 3, "state_since_ms": 3});
        let said = f.words(m, &changed(vec![task], 12, json!({}))).unwrap();
        assert!(
            said.iter().any(|l| l.contains("started: Buoy survey")),
            "{said:?}"
        );
        // The job leaves; a task's leaving is not a line (as before).
        let said = f
            .words(
                m,
                &changed(vec![], 13, json!({"gone": ["act_job_abcdef", "exe_buoy"]})),
            )
            .unwrap();
        assert_eq!(said, vec!["@13 act_job_abcdef left the board".to_string()]);
    }

    /// A notice a task's own event already told is not said twice; any other notice is.
    #[test]
    fn a_notice_is_said_once() {
        let mut f = Follow::new(&snap(vec![]));
        let n = |why: &str| {
            json!({"urgency": "inform", "why": why,
                "work": {"id": "act_1", "kind": "question", "title": ""},
                "root": {"id": "exe_1", "kind": "conversation", "title": ""},
                "line": "the tide turned", "position": 1, "at_ms": 1})
        };
        assert!(f
            .words(notify::WORK_NOTICE, &n("asked"))
            .unwrap()
            .is_empty());
        assert_eq!(
            f.words(notify::WORK_NOTICE, &n("woke")).unwrap(),
            vec!["!  the tide turned".to_string()]
        );
    }

    /// A conversation's own question is said (the tree draws only its answer commands); a task's is
    /// the task's `asks:` line, so it is not said twice.
    #[test]
    fn a_conversations_question_is_said_and_a_tasks_is_not_said_twice() {
        let ask = |id: &str, parent: &str| {
            view(
                json!({"id": id, "kind": "question", "parent": parent, "state": "needs_you",
                "title": "fs.write notes.txt",
                "attention": {"level": "needs_you", "label": "confirm fs.write: notes.txt", "since_ms": 4}}),
            )
        };
        let snap = snap(vec![
            view(json!({"id": "exe_1", "title": "Beta summary"})),
            view(
                json!({"id": "exe_task", "kind": "task", "parent": "exe_1", "title": "Read the buoys",
                "attention": {"level": "working", "label": "turn 1", "since_ms": 3}}),
            ),
            ask("act_ask_conv", "exe_1"),
            ask("act_ask_task", "exe_task"),
        ]);
        let text = board_lines(&snap, 9_000, 100).join("\n");
        assert!(
            text.contains("Asking (theseus answer ID):\n  k_conv  ● fs.write notes.txt  confirm fs.write: notes.txt"),
            "{text}"
        );
        assert_eq!(text.matches("fs.write notes.txt").count(), 1, "{text}");
    }
}
