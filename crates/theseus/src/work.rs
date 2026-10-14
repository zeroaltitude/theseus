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
use theseus_protocol::{method, notify, Event, Level, Message, WorkWatchParams, WorkWatchResult};

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
        for l in tree_lines(&snap.views) {
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
    let mut last: BTreeMap<String, u64> = snap
        .views
        .iter()
        .map(|v| (v.id.clone(), v.position))
        .collect();
    for (m, p) in early {
        show(&m, &p, json, &mut last)?;
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
                for l in tree_lines(&snap.views) {
                    println!("{l}");
                }
            }
            last = snap
                .views
                .iter()
                .map(|v| (v.id.clone(), v.position))
                .collect();
            continue;
        }
        show(&n.method, &n.params, json, &mut last)?;
    }
    Ok(())
}

/// One notification, printed: each view newer than the last printed for its
/// id, each id that left, each notice.
fn show(m: &str, p: &Value, json: bool, last: &mut BTreeMap<String, u64>) -> Result<()> {
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
                if v.position > *seen {
                    *seen = v.position;
                    println!("{}", change_line(v));
                }
            }
            for id in &c.gone {
                last.remove(id);
                println!("@{} {id} left the board", c.position);
            }
        }
        Some(Event::WorkNotice(n)) => println!("{}", notice_line(&n)),
        _ => {}
    }
    Ok(())
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

/// A view as one line of the tree: its glyph, its title, and what it says of
/// itself (its attention's label), then its progress and rollup.
fn view_line(v: &WorkView) -> String {
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
    if let Some(p) = &v.progress {
        s.push_str(&format!("  [{}/{} steps]", p.steps_done, p.steps_total));
    }
    if let Some(e) = &v.eta {
        s.push_str(&format!("  [usually {} s]", e.usual_ms / 1000));
    }
    if let Some(b) = &v.below {
        if b.total > 0 {
            s.push_str(&format!(
                "  ({} need you, {} working, {} done, {} failed)",
                b.needs_you, b.working, b.done, b.failed
            ));
        }
    }
    s
}

/// A change, as `--watch` prints it.
fn change_line(v: &WorkView) -> String {
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
        view_line(v)
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

/// The snapshot as a tree: each root, needs-you first, with its views
/// beneath it in order of when they began, indented by depth.
pub fn tree_lines(views: &[WorkView]) -> Vec<String> {
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
        push_tree(r, 0, &kids, &mut out);
    }
    out
}

fn push_tree(
    v: &WorkView,
    depth: usize,
    kids: &BTreeMap<&str, Vec<&WorkView>>,
    out: &mut Vec<String>,
) {
    out.push(format!("{}{}", "  ".repeat(depth), view_line(v)));
    let mut below = kids.get(v.id.as_str()).cloned().unwrap_or_default();
    below.sort_by_key(|k| (k.started_at_ms, k.id.clone()));
    for k in below {
        push_tree(k, depth + 1, kids, out);
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
            tree_lines(&views),
            vec![
                "◐ Beta summary  turn 3  [1/3 steps]  (1 need you, 1 working, 0 done, 0 failed)",
                "  ◐ Read the buoys  turn 1",
                "    ◐ cargo test -p core  running  [usually 94 s]",
                "  ● fs.write notes.txt  confirm fs.write: notes.txt",
                "○ Quiet one  ready",
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
            change_line(&v),
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
