//! `theseus tasks` (theseus-mnd9g): the daemon's work board as a list of tasks. One `work.watch` read draws the
//! tree (`render::task_tree`): a conversation, its tasks one line each with
//! what each is doing, the command that answers a task that asks, and a
//! footer of counts. `--watch` keeps the connection and prints a line for
//! each task event (started, asks, reported, failed) and each notice the
//! policy makes that is not one of those, driven by the push, and tells the
//! daemon a person is looking (`interactive`). `--graph` is the task graph
//! and `--json` the task list, as before. `theseus work` is an alias of
//! `tasks --watch` (theseus-6kgp8); `board.rs` adds what the work board showed to the tree: threads, the
//! job ids `peek` and `attach` take, and the daemon's own work.

use anyhow::Result;
use clap::Args;
use serde_json::Value;
use theseus_client::render::task_tree;
use theseus_client::Conn;
use theseus_protocol::work::WorkView;
use theseus_protocol::{method, notify, Message, WorkWatchParams, WorkWatchResult};

use crate::board::{self, Follow};
use crate::cmd;

/// How many of the conversations that need no one are listed besides those
/// that need you or work.
const LIMIT: u32 = 20;

#[derive(Args, Debug, Default)]
pub struct TasksArgs {
    /// Only this session's tasks (the tasks it started).
    #[arg(long, short)]
    pub session: Option<String>,
    /// The task graph (every record as a tree) and each task's pieces, as `tasks` printed
    /// before the tree.
    #[arg(long)]
    pub graph: bool,
    /// Keep the connection: print a line for each task that starts, asks, reports or fails, and
    /// each other notice the notification policy makes, until Ctrl-C.
    #[arg(long)]
    pub watch: bool,
    /// How many of the conversations that need no one are listed besides those that need you or
    /// work (the most recent first). Default 20.
    #[arg(long)]
    pub limit: Option<u32>,
    /// The board as one list of conversations, with no group for a thread.
    #[arg(long)]
    pub flat: bool,
}

/// The params of the one read: only the session's own tree when one is named.
async fn params(conn: &mut Conn, a: &TasksArgs, interactive: bool) -> Result<Value> {
    let roots = match a.session.as_deref() {
        Some(s) => Some(vec![theseus_client::resolve::session(conn, s).await?]),
        None => None,
    };
    Ok(serde_json::to_value(WorkWatchParams {
        limit: Some(a.limit.unwrap_or(LIMIT)),
        roots,
        // A person is looking at it, but only while it watches.
        interactive: Some(interactive),
    })?)
}

/// Run `theseus tasks`.
pub async fn run(conn: &mut Conn, json: bool, a: TasksArgs) -> Result<()> {
    if a.graph || (json && !a.watch) {
        return cmd::tasks(conn, json, a.session).await;
    }
    let mut early: Vec<(String, Value)> = Vec::new();
    let p = params(conn, &a, a.watch).await?;
    let v = conn
        .call(method::WORK_WATCH, p, |m, p| {
            early.push((m.to_string(), p.clone()))
        })
        .await?;
    let snap: WorkWatchResult = serde_json::from_value(v.clone())?;
    if json {
        println!("{}", serde_json::json!({"result": v}));
    } else {
        print_board(&snap, a.flat);
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
    let mut follow = Follow::new(&snap);
    for (m, p) in early {
        follow.show(&m, &p, json)?;
    }
    while let Some(msg) = conn.next().await? {
        let Message::Notification(n) = msg else {
            continue;
        };
        if n.method == notify::EVENTS_LOST {
            // Fell behind: the snapshot again, then on.
            let p = params(conn, &a, true).await?;
            let v = conn.call(method::WORK_WATCH, p, |_, _| {}).await?;
            let snap: WorkWatchResult = serde_json::from_value(v)?;
            if !json {
                println!(
                    "-- events were lost; the board again at position {} --",
                    snap.position
                );
                print_board(&snap, a.flat);
            }
            follow.reset(&snap);
            continue;
        }
        follow.show(&n.method, &n.params, json)?;
    }
    Ok(())
}

/// The snapshot, drawn at the terminal's width.
fn print_board(snap: &WorkWatchResult, flat: bool) {
    let lines = board::lines(
        snap,
        flat,
        theseus_protocol::now_unix_ms(),
        theseus_client::render::style::terminal_width(),
    );
    for l in lines {
        println!("{l}");
    }
}

/// The tree as text; `total` is how many roots the board holds.
pub fn tree_lines_at(views: &[WorkView], now_ms: u64, width: usize, total: u64) -> Vec<String> {
    let roots = views.iter().filter(|v| v.parent.is_none()).count() as u64;
    task_tree::tree(views, now_ms, width, total.saturating_sub(roots).min(total))
        .into_iter()
        .map(|l| l.text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use theseus_protocol::notices::Notice;

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
                "question": {"id": "act_ask", "kind": "approval", "prompt": "fs.write notes.txt",
                    "asked_by": {"id": "exe_1", "kind": "conversation", "title": "Beta summary"},
                    "options": [
                        {"id": "approve", "label": "Approve", "style": "primary", "takes_note": false, "effect": "runs"},
                        {"id": "decline", "label": "Decline", "style": "plain", "takes_note": true, "effect": "does not"}],
                    "floor": false, "may_answer": true},
                "attention": {"level": "needs_you", "label": "confirm fs.write: notes.txt", "since_ms": 4}}),
            ),
        ];
        assert_eq!(
            tree_lines_at(&views, 61_000, 100, 0),
            vec![
                "● exe_1  Beta summary",
                "          theseus answer ct_ask approve · theseus answer ct_ask decline --note …",
                "  ◐ e_task  Read the buoys  1:00  job 1:00",
                "1 needs you · 1 working",
            ]
        );
    }

    /// A task that ended says so in words; a notice says how loud it is.
    #[test]
    fn a_change_and_a_notice_are_one_line_each() {
        let v = view(
            json!({"id": "exe_abcdef123456", "kind": "task", "title": "Buoys",
            "previous": "running", "state": "done",
            "attention": {"level": "idle", "label": "complete", "since_ms": 9}}),
        );
        assert_eq!(
            theseus_client::render::task_tree::event_line(
                &theseus_client::render::task_tree::events(None, &v)[0]
            )
            .text,
            "✓ Buoys finished"
        );
        let n: Notice = serde_json::from_value(json!({
            "urgency": "interrupt", "why": "asked",
            "work": {"id": "act_1", "kind": "question", "title": ""},
            "root": {"id": "exe_1", "kind": "conversation", "title": ""},
            "line": "● Beta summary asks: fs.write notes.txt?", "position": 1, "at_ms": 1
        }))
        .unwrap();
        assert_eq!(
            crate::work::notice_line(&n),
            "!! ● Beta summary asks: fs.write notes.txt?"
        );
    }
}
