//! `theseus work` (theseus-hkx30): the daemon's work board as a tree. A
//! conversation at the top, its tasks, plan steps, jobs, questions and wakes
//! below it, each with what it needs from you or is doing. `--watch` keeps the
//! connection and prints a line for each `work.changed` view and each
//! `work.notice`, driven by the push.
//!
//! This is the minimal reader the board shipped with (`cli-tasks` builds the
//! real tree on it): it uses `work.watch` and nothing else, so it is the
//! second client of the wire as well as a way to see the board.

use std::collections::BTreeMap;

use anyhow::Result;
use clap::Args;
use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::notices::Notice;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{
    group_threads, method, notify, Event, Level, Message, ThreadGroup, WorkWatchParams,
    WorkWatchResult,
};

#[derive(Args, Debug)]
pub struct WorkArgs {
    /// Keep the connection: print a line for each change of the board and each notice the
    /// notification policy makes, until Ctrl-C.
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

/// Run `theseus work`.
pub async fn run(conn: &mut Conn, json: bool, a: WorkArgs) -> Result<()> {
    let mut early: Vec<(String, Value)> = Vec::new();
    let params = WorkWatchParams {
        limit: Some(a.limit.unwrap_or(20)),
        roots: None,
        // A person is looking at it, but only while it watches.
        interactive: Some(a.watch),
    };
    let v = conn
        .call(method::WORK_WATCH, serde_json::to_value(params)?, |m, p| {
            early.push((m.to_string(), p.clone()))
        })
        .await?;
    let snap: WorkWatchResult = serde_json::from_value(v.clone())?;
    if json {
        println!("{}", serde_json::json!({"result": v}));
    } else {
        for l in board_lines(&snap, a.flat, now_ms(), width()) {
            println!("{l}");
        }
        if snap.views.is_empty() {
            println!("nothing is going on");
        }
    }
    if !a.watch {
        let _ = conn
            .call(method::WORK_UNWATCH, serde_json::json!({}), |_, _| {})
            .await;
        return Ok(());
    }
    eprintln!(
        "watching the work board: {} of {} roots at position {} (Ctrl-C to stop)",
        snap.views.iter().filter(|v| v.parent.is_none()).count(),
        snap.total,
        snap.position
    );
    let mut last: Seen = snap
        .views
        .iter()
        .map(|v| (v.id.clone(), stamp(v)))
        .collect();
    let mut groups: BTreeMap<String, ThreadGroup> = snap
        .threads
        .iter()
        .map(|g| (g.slug.clone(), g.clone()))
        .collect();
    for (m, p) in early {
        show(&m, &p, json, &mut last, &mut groups)?;
    }
    while let Some(msg) = conn.next().await? {
        let Message::Notification(n) = msg else {
            continue;
        };
        if n.method == notify::EVENTS_LOST {
            // Fell behind: the snapshot again, then on.
            let v = conn
                .call(
                    method::WORK_WATCH,
                    serde_json::to_value(WorkWatchParams {
                        limit: Some(a.limit.unwrap_or(20)),
                        roots: None,
                        interactive: Some(true),
                    })?,
                    |_, _| {},
                )
                .await?;
            let snap: WorkWatchResult = serde_json::from_value(v)?;
            if !json {
                println!(
                    "-- events were lost; the board again at position {} --",
                    snap.position
                );
                for l in board_lines(&snap, a.flat, now_ms(), width()) {
                    println!("{l}");
                }
            }
            groups = snap
                .threads
                .iter()
                .map(|g| (g.slug.clone(), g.clone()))
                .collect();
            last = snap
                .views
                .iter()
                .map(|v| (v.id.clone(), stamp(v)))
                .collect();
            continue;
        }
        show(&n.method, &n.params, json, &mut last, &mut groups)?;
    }
    Ok(())
}

/// One notification, printed: each view newer than the last printed for its
/// id, each id that left, each notice.
fn show(
    m: &str,
    p: &Value,
    json: bool,
    last: &mut Seen,
    groups: &mut BTreeMap<String, ThreadGroup>,
) -> Result<()> {
    if json {
        if m == notify::WORK_CHANGED || m == notify::WORK_NOTICE {
            println!("{}", serde_json::json!({"method": m, "params": p}));
        }
        return Ok(());
    }
    match Event::from_notification(m, p)? {
        Some(Event::WorkChanged(c)) => {
            for v in &c.views {
                let seen = last.entry(v.id.clone()).or_default();
                if stamp(v) > *seen {
                    *seen = stamp(v);
                    println!("{}", change_line(v, now_ms()));
                }
            }
            for id in &c.gone {
                last.remove(id);
                println!("@{} {id} left the board", c.position);
            }
            for g in &c.threads {
                let before = groups.get(&g.slug).cloned();
                let mut next = before.clone().unwrap_or_else(|| g.clone());
                if before.is_some() {
                    next.merge(g.clone());
                }
                if before.as_ref() != Some(&next) {
                    println!("@{} {}", c.position, thread_change_line(&next));
                }
                groups.insert(g.slug.clone(), next);
            }
        }
        Some(Event::WorkNotice(n)) => println!("{}", notice_line(&n)),
        _ => {}
    }
    Ok(())
}

/// What a client has applied of a view: its position, then its time. A job's
/// last line is sampled between frames, so a newer sample has the same
/// position and a later time (theseus-e1hu4); a snapshot older than a sample
/// then never takes its place.
type Seen = BTreeMap<String, (u64, u64)>;

fn stamp(v: &WorkView) -> (u64, u64) {
    (v.position, v.at_ms)
}

fn now_ms() -> u64 {
    theseus_protocol::now_unix_ms()
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
fn notice_line(n: &Notice) -> String {
    let who = match n.urgency {
        theseus_protocol::notices::Urgency::Interrupt => "!!",
        theseus_protocol::notices::Urgency::Inform => "! ",
        theseus_protocol::notices::Urgency::Quiet => "· ",
    };
    format!("{who} {}", n.line)
}

/// A thread's change, as `--watch` prints it.
fn thread_change_line(g: &ThreadGroup) -> String {
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

/// The terminal's width: `COLUMNS` when the shell exports it, else 100.
fn width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .filter(|w| *w >= 20)
        .unwrap_or(100)
}

/// The snapshot as `theseus work` prints it: a group for each live thread,
/// its open tasks, questions and wakes under it (needs you first), then
/// what is in no thread as the tree it always was; `flat`, or a board with no
/// thread on it, prints that tree alone.
pub fn board_lines(snap: &WorkWatchResult, flat: bool, now_ms: u64, width: usize) -> Vec<String> {
    if flat || snap.threads.is_empty() {
        return tree_lines(&snap.views, now_ms);
    }
    let g = group_threads(&snap.views, &snap.threads);
    if g.threads.is_empty() {
        return tree_lines(&snap.views, now_ms);
    }
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
    let loose: Vec<WorkView> = g.loose.iter().map(|v| (*v).clone()).collect();
    if !loose.is_empty() {
        out.push("Other:".to_string());
        out.extend(
            tree_lines(&loose, now_ms)
                .into_iter()
                .map(|l| format!("  {l}")),
        );
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
                "  ○ The DM  ready",
            ]
        );
        let flat = board_lines(&snap, true, 1_000_000, 100);
        assert_eq!(flat, tree_lines(&snap.views, 1_000_000));
        let mut none = snap;
        none.threads.clear();
        assert_eq!(
            board_lines(&none, false, 1_000_000, 100),
            tree_lines(&none.views, 1_000_000)
        );
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
