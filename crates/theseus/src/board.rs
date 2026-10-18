//! The board `theseus tasks` draws (theseus-6kgp8). `theseus work` is an alias of `theseus tasks --watch`, so
//! `tasks` holds what the work board showed: the task tree is the body, and beside it come the board's threads
//! (a header for each live thread, with its tasks, questions and wakes under it), a `Jobs` list with each job's
//! id (what `peek` and `attach` take), and the work that is not a task: the daemon's own jobs, routines, watches,
//! projects and wakes. `--watch` prints a line for each of those that changes, as `work --watch` did: a job, a
//! wake, a routine, a watch, a project, the daemon's own work, and each thread. A step's change and a question's
//! are the task's own lines (its plan count, its ask).

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use serde_json::Value;
use theseus_client::render::task_tree;
use theseus_protocol::notices::{Notice, Why};
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{group_threads, notify, Event, ThreadGroup, WorkWatchResult};

use crate::work::{self, Seen};

/// Whether a view is a line of the board's own list (a job, a wake, a routine, a watch, a project, the
/// daemon's work), rather than a task's part.
fn listed(v: &WorkView) -> bool {
    !matches!(
        v.kind,
        WorkKind::Conversation | WorkKind::Task | WorkKind::Step | WorkKind::Question
    )
}

/// The snapshot as `tasks` prints it. `flat` is the board without threads.
pub fn lines(snap: &WorkWatchResult, flat: bool, now_ms: u64, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let grouped = (!flat && !snap.threads.is_empty())
        .then(|| group_threads(&snap.views, &snap.threads))
        .filter(|g| !g.threads.is_empty());
    let loose: Vec<WorkView> = match &grouped {
        Some(g) => {
            out.extend(work::thread_section(g, now_ms, width));
            g.loose.iter().map(|v| (*v).clone()).collect()
        }
        None => snap.views.clone(),
    };
    // `tree_lines_at` takes the roots the board holds, and counts the ones it was given: so the ones
    // the snapshot does not carry (`--limit`) are added once, whether the threads took some or not.
    let parentless = |vs: &[WorkView]| vs.iter().filter(|v| v.parent.is_none()).count() as u64;
    let unlisted = snap.total.saturating_sub(parentless(&snap.views));
    let tree =
        crate::tasks_tree::tree_lines_at(&loose, now_ms, width, parentless(&loose) + unlisted);
    if tree.as_slice() != ["no tasks"] {
        if grouped.is_some() {
            out.push("Other:".to_string());
        }
        out.extend(tree);
    }
    // A conversation's own question: the tree draws its answer commands but not what it asks, so the
    // board says it (the work board listed every question).
    let asking: Vec<&WorkView> = loose
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
    let own = daemon_and_schedules(&loose);
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
    groups: BTreeMap<String, ThreadGroup>,
}

impl Follow {
    pub fn new(snap: &WorkWatchResult) -> Self {
        let mut f = Follow {
            last: HashMap::new(),
            seen: Seen::new(),
            groups: BTreeMap::new(),
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
        self.groups = snap
            .threads
            .iter()
            .map(|g| (g.slug.clone(), g.clone()))
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
    /// told for its id, a change line for each other listed view, a thread's change, each id that
    /// left, and each notice that is not a task's own event.
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
                for g in &c.threads {
                    let before = self.groups.get(&g.slug).cloned();
                    let mut next = before.clone().unwrap_or_else(|| g.clone());
                    if before.is_some() {
                        next.merge(g.clone());
                    }
                    if before.as_ref() != Some(&next) {
                        out.push(format!(
                            "@{} {}",
                            c.position,
                            work::thread_change_line(&next)
                        ));
                    }
                    self.groups.insert(g.slug.clone(), next);
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
    use crate::tasks_tree::tree_lines_at;

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

    fn group(slug: &str, title: &str) -> ThreadGroup {
        serde_json::from_value(json!({
            "slug": slug, "title": title, "state": "active", "session_id": format!("ses_{slug}"),
            "last_active_ms": 1_000_000 - 120_000, "context_tokens": 41000, "today_usd": 1.2
        }))
        .unwrap()
    }

    /// Two threads, each with a task and a question, and one conversation in none.
    fn threaded() -> WorkWatchResult {
        let t = |id: &str, kind: &str, parent: &str, thread: &str, level: &str, label: &str| {
            view(
                json!({"id": id, "kind": kind, "parent": parent, "root": parent, "title": id,
                "thread": thread, "started_at_ms": 3,
                "state": if level == "needs_you" { "needs_you" } else { "running" },
                "attention": {"level": level, "label": label, "since_ms": 3}}),
            )
        };
        let head = |id: &str, thread: &str| {
            view(
                json!({"id": id, "title": thread, "thread": thread, "state": "ready", "at_ms": 1_000_000 - 60_000,
                "attention": {"level": "ready", "label": "ready", "since_ms": 1}}),
            )
        };
        WorkWatchResult {
            position: 9,
            views: vec![
                head("exe_pr", "pr-round"),
                t(
                    "exe_pr_t", "task", "exe_pr", "pr-round", "working", "turn 2",
                ),
                t(
                    "act_pr_q",
                    "question",
                    "exe_pr",
                    "pr-round",
                    "needs_you",
                    "confirm fs.write",
                ),
                head("exe_tides", "tides"),
                t(
                    "exe_tides_t",
                    "task",
                    "exe_tides",
                    "tides",
                    "working",
                    "turn 1",
                ),
                t(
                    "act_tides_q",
                    "question",
                    "exe_tides",
                    "tides",
                    "needs_you",
                    "confirm proc.run",
                ),
                view(json!({"id": "exe_dm", "title": "The DM", "state": "ready",
                    "attention": {"level": "ready", "label": "ready", "since_ms": 1}})),
                view(
                    json!({"id": "exe_dm_t", "kind": "task", "parent": "exe_dm", "root": "exe_dm",
                    "title": "Tide tables", "started_at_ms": 3,
                    "attention": {"level": "working", "label": "turn 4", "since_ms": 3}}),
                ),
            ],
            total: 3,
            threads: vec![group("pr-round", "PR round"), group("tides", "Tides")],
        }
    }

    /// `theseus work` puts each thread's open work under its header, then what is in no
    /// thread as it always was; `--flat` and a board with no thread print today's list.
    #[test]
    fn the_board_groups_by_thread_and_flat_is_todays_list() {
        let snap = threaded();
        let lines = board_lines(&snap, false, 1_000_000, 100);
        assert_eq!(
            lines,
            vec![
                "Threads: 2 live, 2 need you",
                "PR round · active · 1m ago · warm · 41k ctx · $1.20 today  ● 1 need you",
                "  ● act_pr_q  confirm fs.write",
                "  ◐ exe_pr_t  turn 2",
                "Tides · active · 1m ago · warm · 41k ctx · $1.20 today  ● 1 need you",
                "  ● act_tides_q  confirm proc.run",
                "  ◐ exe_tides_t  turn 1",
                "Other:",
                "○ exe_dm  The DM",
                "  ◐ e_dm_t  Tide tables  16:39  turn 4",
                "1 working",
            ]
        );
        let flat = board_lines(&snap, true, 1_000_000, 100);
        // The tree, then the questions the tree gives only answer commands for.
        let mut tree = tree_lines_at(&snap.views, 1_000_000, 100, 0);
        tree.extend(
            [
                "Asking (theseus answer ID):",
                "  t_pr_q  ● act_pr_q  confirm fs.write",
                "  ides_q  ● act_tides_q  confirm proc.run",
            ]
            .map(String::from),
        );
        assert_eq!(flat, tree);
        let mut none = snap;
        none.threads.clear();
        assert_eq!(board_lines(&none, false, 1_000_000, 100), tree);
    }

    /// At 40 columns the header gives up its details, never the title or the count.
    #[test]
    fn a_narrow_board_keeps_the_title_and_the_count() {
        let lines = board_lines(&threaded(), false, 1_000_000, 40);
        assert_eq!(lines[1], "PR round · active  ● 1 need you");
        assert_eq!(lines[4], "Tides · active · 1m ago  ● 1 need you");
    }

    /// A sealed thread's header begins with its lock; a pending seal is a line that needs you.
    #[test]
    fn a_sealed_thread_has_its_lock_and_a_pending_seal_its_line() {
        let mut snap = threaded();
        snap.threads[0].sealed = true;
        snap.threads[1].seal_pending = Some("the notes are private".into());
        let lines = board_lines(&snap, false, 1_000_000, 100);
        assert!(lines[1].starts_with("🔒 PR round"), "{}", lines[1]);
        assert_eq!(lines[0], "Threads: 2 live, 3 need you");
        assert!(lines.contains(&"  ● seal recommended: the notes are private".to_string()));
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
        let text = board_lines(&snap, false, 7 + 5_400, 100).join("\n");
        assert!(
            text.contains(
                "Jobs (theseus peek JOB · theseus attach JOB):\n  abcdef  ◐ cargo test -p core"
            ),
            "{text}"
        );
        assert!(text.contains("› test result: 41 passed"), "{text}");
        assert!(text.contains("○ Check the tide  wakes 18:00"), "{text}");
        assert!(text.contains("○ Morning brief  daily 08:00"), "{text}");
        assert!(
            text.contains("⚙ Theseus  (1 working, 0 done, 0 failed)"),
            "{text}"
        );
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

    /// `--watch` tells a job's change once, not a stale sample, a thread's change, a departure, and a
    /// task's own events as `tasks --watch` always did.
    #[test]
    fn the_watch_tells_each_change_of_a_job_a_thread_and_a_task_once() {
        let mut base = threaded();
        base.threads[0].today_usd = Some(1.2);
        let mut f = Follow::new(&base);
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
        // A thread that changed.
        let mut g = base.threads[0].clone();
        g.title = "PR round two".into();
        let t = changed(vec![], 11, json!({"threads": [g]}));
        let said = f.words(m, &t).unwrap();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("@11 ") && said[0].contains("PR round two"),
            "{said:?}"
        );
        assert!(
            f.words(m, &t).unwrap().is_empty(),
            "an unchanged thread says nothing"
        );
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
        let text = board_lines(&snap, false, 9_000, 100).join("\n");
        assert!(
            text.contains("Asking (theseus answer ID):\n  k_conv  ● fs.write notes.txt  confirm fs.write: notes.txt"),
            "{text}"
        );
        assert_eq!(text.matches("fs.write notes.txt").count(), 1, "{text}");
    }
}
