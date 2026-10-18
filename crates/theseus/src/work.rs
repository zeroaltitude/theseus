//! The work board's lines (theseus-hkx30): a conversation at the top, its tasks, plan steps, jobs, questions and
//! wakes below it, each with what it needs from you or is doing. `theseus work` is an alias of `theseus tasks
//! --watch` (theseus-6kgp8): `WorkArgs` are its arguments, and `board.rs` draws the board with these lines for what
//! the task tree does not draw (threads, jobs, the daemon's own work, routines, watches, projects, wakes).

use std::collections::BTreeMap;

use clap::Args;
use theseus_protocol::notices::Notice;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{Level, ThreadGroup};

use crate::tasks_tree::TasksArgs;

#[derive(Args, Debug)]
pub struct WorkArgs {
    /// Accepted and ignored: `work` always keeps the connection, as `tasks --watch` does.
    #[arg(long)]
    watch: bool,
    /// How many of the conversations that need no one are listed besides those that need you or
    /// work (the most recent first). Default 20.
    #[arg(long)]
    limit: Option<u32>,
    /// The board as one list of conversations, as before the threads: no group a thread.
    #[arg(long)]
    flat: bool,
}

impl WorkArgs {
    /// `theseus work` is `theseus tasks --watch` (theseus-6kgp8).
    pub fn into_tasks(self) -> TasksArgs {
        TasksArgs {
            watch: true,
            limit: self.limit,
            flat: self.flat,
            ..TasksArgs::default()
        }
    }
}

/// What a client has applied of a view: its position, then its time. A job's
/// last line is sampled between frames, so a newer sample has the same
/// position and a later time (theseus-e1hu4); a snapshot older than a sample
/// then never takes its place.
pub(crate) type Seen = BTreeMap<String, (u64, u64)>;

pub(crate) fn stamp(v: &WorkView) -> (u64, u64) {
    (v.position, v.at_ms)
}

/// The glyph of a level.
fn glyph(l: Level) -> &'static str {
    match l {
        Level::NeedsYou => "●",
        Level::Working => "◐",
        Level::Ready => "○",
        Level::Idle => "✓",
    }
}

/// What kind of work a view is, as a word.
fn kind_word(k: WorkKind) -> &'static str {
    match k {
        WorkKind::Conversation => "conversation",
        WorkKind::Task => "task",
        WorkKind::Step => "step",
        WorkKind::Job => "job",
        WorkKind::Question => "question",
        WorkKind::Wake => "wake",
        WorkKind::System => "system",
        WorkKind::Routine => "routine",
        WorkKind::Watch => "watch",
        WorkKind::Project => "project",
    }
}

/// The daemon's own work (theseus-jn17m): the root as `⚙ Theseus  (1
/// working, 2 done, 0 failed)`, a job as `◐ Importing Claude Code history ·
/// batch 41 of 120 · 3:12`; a failed one is marked `✗`.
fn system_line(v: &WorkView, now: u64) -> String {
    if theseus_protocol::work_system::is_root(&v.id) {
        let b = v.below.unwrap_or_default();
        return format!(
            "⚙ {}  ({} working, {} done, {} failed)",
            v.title, b.working, b.done, b.failed
        );
    }
    let glyph = match v.state {
        theseus_protocol::work::WorkState::Failed => "✗",
        _ => glyph(v.attention.level),
    };
    format!("{glyph} {}", theseus_protocol::work_system::line(v, now))
}

/// A view as one line of the tree: its glyph, its title, and what it says of
/// itself (its attention's label), then its progress and rollup.
pub(crate) fn view_line(v: &WorkView, now: u64) -> String {
    if v.kind == WorkKind::System {
        return system_line(v, now);
    }
    let title = if v.title.is_empty() {
        format!(
            "{} {}",
            kind_word(v.kind),
            &v.id[v.id.len().saturating_sub(6)..]
        )
    } else {
        v.title.clone()
    };
    let mut s = format!(
        "{} {title}  {}",
        glyph(v.attention.level),
        v.attention.label
    );
    if let Some(line) = v.progress.as_ref().and_then(|p| p.line()) {
        s.push_str(&format!("  [{line}]"));
    }
    // A running job's time, against its usual (theseus-e1hu4).
    if v.kind == WorkKind::Job && v.state == theseus_protocol::work::WorkState::Running {
        s.push_str(&format!(
            "  [{}]",
            theseus_protocol::job_follow::job_words(
                now.saturating_sub(v.started_at_ms),
                v.eta.map(|e| e.usual_ms)
            )
        ));
    }
    if let Some(f) = &v.forecast {
        let spent = v.cost.map_or(0.0, |c| c.spent_usd);
        s.push_str(&format!("  [{}]", f.spent_of(spent)));
        if f.past {
            s.push_str(" past its p90");
        }
    }
    if let Some(b) = &v.below {
        if b.total > 0 {
            s.push_str(&format!(
                "  ({} need you, {} working, {} done, {} failed)",
                b.needs_you, b.working, b.done, b.failed
            ));
        }
    }
    if let Some(l) = v.doing.as_ref().and_then(|d| d.last_line.as_deref()) {
        s.push_str(&format!("  › {l}"));
    }
    s
}

/// A change, as `--watch` prints it.
pub(crate) fn change_line(v: &WorkView, now: u64) -> String {
    let before = v
        .previous
        .filter(|p| *p != v.state)
        .map(|p| format!("{} → ", p.as_str()));
    format!(
        "@{} {} {} {}{}  {}",
        v.position,
        kind_word(v.kind),
        &v.id[v.id.len().saturating_sub(6)..],
        before.unwrap_or_default(),
        v.state.as_str(),
        view_line(v, now)
    )
}

/// A notice, as `--watch` prints it.
pub(crate) fn notice_line(n: &Notice) -> String {
    let who = match n.urgency {
        theseus_protocol::notices::Urgency::Interrupt => "!!",
        theseus_protocol::notices::Urgency::Inform => "! ",
        theseus_protocol::notices::Urgency::Quiet => "· ",
    };
    format!("{who} {}", n.line)
}

/// A thread's change, as `--watch` prints it.
pub(crate) fn thread_change_line(g: &ThreadGroup) -> String {
    let lock = if g.sealed { "🔒 " } else { "" };
    let seal = match &g.seal_pending {
        Some(why) => format!("  ● seal recommended: {why}"),
        None => String::new(),
    };
    format!(
        "thread {} {}  {lock}{}{seal}",
        g.slug,
        g.state.as_str(),
        g.title
    )
}

/// The threads of a grouped board: `Threads: 2 live, 1 need you`, then a header for each, a
/// seal recommendation, and its items.
pub(crate) fn thread_section(
    g: &theseus_protocol::Grouped,
    now_ms: u64,
    width: usize,
) -> Vec<String> {
    let mut out = vec![format!(
        "Threads: {} live, {} need you",
        g.threads.len(),
        g.needs_you()
    )];
    for t in &g.threads {
        let mut head = t.header(now_ms, width.saturating_sub(16));
        match (t.needs_you, t.working) {
            (0, 0) => {}
            (0, w) => head.push_str(&format!("  ◐ {w} working")),
            (n, _) => head.push_str(&format!("  ● {n} need you")),
        }
        out.push(head);
        if let Some(why) = &t.group.seal_pending {
            out.push(format!("  ● seal recommended: {why}"));
        }
        for v in &t.items {
            out.push(format!("  {}", view_line(v, now_ms)));
        }
    }
    out
}

/// The snapshot as a tree: each root, needs-you first, with its views
/// beneath it in order of when they began, indented by depth.
pub fn tree_lines(views: &[WorkView], now: u64) -> Vec<String> {
    let mut kids: BTreeMap<&str, Vec<&WorkView>> = BTreeMap::new();
    let mut roots: Vec<&WorkView> = Vec::new();
    let ids: std::collections::HashSet<&str> = views.iter().map(|v| v.id.as_str()).collect();
    for v in views {
        match v.parent.as_deref() {
            Some(p) if ids.contains(p) => kids.entry(p).or_default().push(v),
            _ => roots.push(v),
        }
    }
    roots.sort_by_key(|v| (v.attention.level.rank(), v.state_since_ms, v.id.clone()));
    let mut out = Vec::new();
    for r in roots {
        push_tree(r, 0, &kids, now, &mut out);
    }
    out
}

fn push_tree(
    v: &WorkView,
    depth: usize,
    kids: &BTreeMap<&str, Vec<&WorkView>>,
    now: u64,
    out: &mut Vec<String>,
) {
    out.push(format!("{}{}", "  ".repeat(depth), view_line(v, now)));
    let mut below = kids.get(v.id.as_str()).cloned().unwrap_or_default();
    below.sort_by_key(|k| (k.started_at_ms, k.id.clone()));
    for k in below {
        push_tree(k, depth + 1, kids, now, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serde_json::Value;

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

    /// The tree: roots by what they need, children under their parents in the
    /// order they began, one indent a level.
    #[test]
    fn the_tree_indents_children_under_their_parents() {
        let views = vec![
            view(
                json!({"id": "exe_idle", "title": "Quiet one", "state": "ready",
                "attention": {"level": "ready", "label": "ready", "since_ms": 1}}),
            ),
            view(json!({"id": "exe_1", "title": "Beta summary",
                "progress": {"steps_done": 1, "steps_total": 3, "at_ms": 1},
                "below": {"needs_you": 1, "working": 1, "done": 0, "failed": 0, "total": 3}})),
            view(
                json!({"id": "act_job", "kind": "job", "parent": "exe_task", "root": "exe_1",
                "title": "cargo test -p core", "started_at_ms": 7,
                "eta": {"usual_ms": 94000, "runs": 5},
                "doing": {"what": "job", "label": "cargo test -p core", "since_ms": 7,
                    "turn": 1, "loop_index": 0, "last_line": "test result: 41 passed"},
                "attention": {"level": "working", "label": "running", "since_ms": 7}}),
            ),
            view(
                json!({"id": "exe_task", "kind": "task", "parent": "exe_1", "root": "exe_1",
                "title": "Read the buoys", "started_at_ms": 3,
                "attention": {"level": "working", "label": "turn 1", "since_ms": 3}}),
            ),
            view(
                json!({"id": "act_ask", "kind": "question", "parent": "exe_1", "root": "exe_1",
                "title": "fs.write notes.txt", "state": "needs_you", "started_at_ms": 4,
                "attention": {"level": "needs_you", "label": "confirm fs.write: notes.txt", "since_ms": 4}}),
            ),
        ];
        assert_eq!(
            tree_lines(&views, 7 + 5_400),
            vec![
                "◐ Beta summary  turn 3  [1/3]  (1 need you, 1 working, 0 done, 0 failed)",
                "  ◐ Read the buoys  turn 1",
                "    ◐ cargo test -p core  running  [job 0:05 of usual 1:34]  › test result: 41 passed",
                "  ● fs.write notes.txt  confirm fs.write: notes.txt",
                "○ Quiet one  ready",
            ]
        );
    }

    /// The daemon's own work (theseus-jn17m): the root closed over its count,
    /// a running job with its count and its time, an ended one with how it
    /// ended and how long it took, a failed one marked.
    #[test]
    fn the_theseus_root_lists_its_jobs_with_progress_and_time() {
        let sys = |v: Value| {
            let mut base = json!({
                "kind": "system", "position": 9, "at_ms": 9, "root": "system", "parent": "system",
                "state": "running",
                "attention": {"level": "working", "label": "running", "since_ms": 5},
                "started_at_ms": 1_000, "state_since_ms": 1_000
            });
            for (k, x) in v.as_object().unwrap() {
                base[k] = x.clone();
            }
            serde_json::from_value::<WorkView>(base).unwrap()
        };
        let views = vec![
            sys(
                json!({"id": "system", "parent": null, "title": "Theseus", "state": "ready",
                "attention": {"level": "ready", "label": "ready", "since_ms": 5},
                "below": {"needs_you": 0, "working": 1, "done": 1, "failed": 1, "total": 3}}),
            ),
            sys(
                json!({"id": "sys_a", "title": "Importing Claude Code history",
                "attention": {"level": "working", "label": "batch 41 of 120", "since_ms": 5}}),
            ),
            sys(
                json!({"id": "sys_b", "title": "Rebuilding the index", "started_at_ms": 2_000,
                "state": "done", "ended_at_ms": 62_000,
                "attention": {"level": "idle", "label": "done", "since_ms": 5}}),
            ),
            sys(
                json!({"id": "sys_c", "title": "Syncing OpenClaw history", "started_at_ms": 3_000,
                "state": "failed", "ended_at_ms": 4_000,
                "attention": {"level": "idle", "label": "failed: the exporter's cursor is ahead of the file", "since_ms": 5}}),
            ),
        ];
        assert_eq!(
            tree_lines(&views, 193_000),
            vec![
                "⚙ Theseus  (1 working, 1 done, 1 failed)",
                "  ◐ Importing Claude Code history · batch 41 of 120 · 3:12",
                "  ✓ Rebuilding the index · done · 1:00",
                "  ✗ Syncing OpenClaw history · failed: the exporter's cursor is ahead of the file · 0:01",
            ]
        );
    }

    /// A change says its position, kind, short id and the state it moved from;
    /// a notice says how loud it is.
    #[test]
    fn a_change_and_a_notice_are_one_line_each() {
        let v = view(
            json!({"id": "exe_abcdef123456", "kind": "task", "title": "Buoys",
            "previous": "running", "state": "done",
            "attention": {"level": "idle", "label": "complete", "since_ms": 9}}),
        );
        assert_eq!(
            change_line(&v, 9),
            "@9 task 123456 running → done  ✓ Buoys  complete"
        );
        let n: Notice = serde_json::from_value(json!({
            "urgency": "interrupt", "why": "asked",
            "work": {"id": "act_1", "kind": "question", "title": ""},
            "root": {"id": "exe_1", "kind": "conversation", "title": ""},
            "line": "● Beta summary asks: fs.write notes.txt?", "position": 1, "at_ms": 1
        }))
        .unwrap();
        assert_eq!(
            notice_line(&n),
            "!! ● Beta summary asks: fs.write notes.txt?"
        );
    }

    /// Health's push line names the task watchers only while there are some.
    #[test]
    fn the_push_line_counts_task_watchers() {
        let mut p: theseus_protocol::PushStatus =
            serde_json::from_value(json!({"seeded": true, "seed_us": 1000, "watchers": 1}))
                .unwrap();
        assert!(!theseus_client::render::push_line(&p).contains("task watcher"));
        p.work_watchers = 2;
        p.interactive = 1;
        assert!(theseus_client::render::push_line(&p)
            .contains("1 watcher · 2 task watchers (1 interactive) · "));
    }
}
