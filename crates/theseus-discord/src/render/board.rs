//! The task graph in a place (M7 step 39b, theseus-ext.14): the board, one
//! message per place edited in place as its tasks change, and `/tasks`, the
//! records' tree before the task sessions of today. Pure: records in, text
//! out; the claim's time of day is the daemon's clock (`theseus_core::push::hm`).

use theseus_core::task_graph::{depth, homed, TaskRecord};

use super::clip;

/// The board's message key on its place's lane: one per place.
pub const BOARD_KEY: &str = "task-board";
/// How the board begins: after a restart, the lane finds it among the pins
/// by it.
pub const BOARD_HEAD: &str = "📋 **Task board**";
/// The most task lines a board or `/tasks` shows.
const MAX_LINES: usize = 25;

/// One task: `` `…a1b2c3` Title — accepted · agent · 🔒 session d4e5f6 until
/// 14:05 · v2 ``, its claim only while it holds at `now_ms`.
fn task_line(t: &TaskRecord, now_ms: u64, version: bool) -> String {
    let mut s = format!(
        "`…{}` {} — {}",
        t.short(),
        clip(&t.title, 80),
        t.state.as_str()
    );
    s.push_str(&format!(" · {}", t.owner));
    if let Some(c) = t.claim_at(now_ms) {
        s.push_str(&format!(
            " · 🔒 session {} until {}",
            c.session_short(),
            theseus_core::push::hm(c.until_ms)
        ));
    }
    if t.proposal.is_some() {
        s.push_str(" · a change waits for you");
    }
    if version {
        s.push_str(&format!(" · v{}", t.version));
    }
    s
}

/// The tree of `tasks` (a place's, oldest first), each child under its
/// parent: open first in its order, a closed task too, to `MAX_LINES`.
fn tree(tasks: &[&TaskRecord], now_ms: u64, version: bool) -> (Vec<String>, usize) {
    let roots: Vec<&TaskRecord> = tasks
        .iter()
        .copied()
        .filter(|t| {
            t.parent
                .as_deref()
                .is_none_or(|p| !tasks.iter().any(|x| x.id == p))
        })
        .collect();
    let mut order: Vec<&TaskRecord> = Vec::new();
    let mut stack: Vec<&TaskRecord> = roots.into_iter().rev().collect();
    while let Some(t) = stack.pop() {
        if order.iter().any(|o| o.id == t.id) {
            continue;
        }
        order.push(t);
        for c in tasks
            .iter()
            .filter(|c| c.parent.as_deref() == Some(t.id.as_str()))
            .rev()
        {
            stack.push(c);
        }
    }
    let lines: Vec<String> = order
        .iter()
        .take(MAX_LINES)
        .map(|t| {
            format!(
                "{}• {}",
                "  ".repeat(depth(&order, t)),
                task_line(t, now_ms, version)
            )
        })
        .collect();
    let left = order.len().saturating_sub(MAX_LINES);
    (lines, left)
}

/// The board: the place's tasks as a tree (id, title, state, owner, claim).
/// None while the place has no task.
pub fn board(records: &[TaskRecord], session: &str, now_ms: u64) -> Option<String> {
    let mine = homed(records, session);
    if mine.is_empty() {
        return None;
    }
    let open = mine.iter().filter(|t| !t.state.is_closed()).count();
    let mut out = vec![format!(
        "{BOARD_HEAD} · {open} open, {} closed",
        mine.len() - open
    )];
    let (lines, left) = tree(&mine, now_ms, false);
    out.extend(lines);
    if left > 0 {
        out.push(format!("-# and {left} more; `/tasks` lists them"));
    }
    out.push("-# Edited in place as the tasks change.".into());
    Some(out.join("\n"))
}

/// `/tasks`: the place's task records as a tree (states, owners, claims,
/// versions), then its task sessions of today (`super::tasks`).
pub fn tasks_here(
    records: &[TaskRecord],
    session: &str,
    sessions: &[theseus_protocol::TaskInfo],
    now_ms: u64,
) -> String {
    let mine = homed(records, session);
    let today = theseus_core::wake::local(now_ms);
    let day = |ms: u64| {
        let l = theseus_core::wake::local(ms);
        (l.year, l.month, l.day)
    };
    let of_today: Vec<theseus_protocol::TaskInfo> = sessions
        .iter()
        .filter(|t| day(t.created_at_ms) == (today.year, today.month, today.day))
        .cloned()
        .collect();
    let mut out = Vec::new();
    if !mine.is_empty() {
        let open = mine.iter().filter(|t| !t.state.is_closed()).count();
        out.push(format!(
            "**Task graph here** · {open} open, {} closed",
            mine.len() - open
        ));
        let (lines, left) = tree(&mine, now_ms, true);
        out.extend(lines);
        if left > 0 {
            out.push(format!(
                "-# and {left} more; `theseus tasks` lists them all"
            ));
        }
    }
    if mine.is_empty() || !of_today.is_empty() {
        out.push(super::tasks(&of_today, now_ms));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_core::task_graph::{TaskClaim, TaskOrigin, TaskState};

    fn task(id: &str, parent: Option<&str>, state: TaskState) -> TaskRecord {
        TaskRecord {
            id: id.into(),
            version: 1,
            title: format!("the {id} part"),
            state,
            parent: parent.map(String::from),
            owner: "agent".into(),
            origin: TaskOrigin {
                session: "ses_0000lagoon".into(),
                principal: "operator".into(),
                by_model: true,
            },
            ..TaskRecord::default()
        }
    }

    /// The board renders the place's tree with each claim while it holds,
    /// and nothing of another place's tasks; `/tasks` adds versions.
    #[test]
    fn the_board_renders_the_places_tree_and_its_claims() {
        let mut reef = task("tsk_000000reef", None, TaskState::Accepted);
        let until = 1_790_000_000_000 + 1_800_000;
        reef.claim = Some(TaskClaim {
            by: "exe_0000lagoon".into(),
            session: "ses_0000lagoon".into(),
            until_ms: until,
        });
        let north = task("tsk_00000north", Some("tsk_000000reef"), TaskState::Done);
        let mut other = task("tsk_00000other", None, TaskState::Accepted);
        other.origin.session = "ses_00000atoll".into();
        let records = vec![reef, north, other];
        let now = 1_790_000_000_000;
        let b = board(&records, "ses_0000lagoon", now).unwrap();
        let hm = theseus_core::push::hm(until);
        assert_eq!(
            b,
            format!(
                "📋 **Task board** · 1 open, 1 closed\n\
                 • `…00reef` the tsk_000000reef part — accepted · agent · 🔒 session lagoon until {hm}\n  \
                 • `…0north` the tsk_00000north part — done · agent\n\
                 -# Edited in place as the tasks change."
            )
        );
        assert!(board(&records, "ses_00000nobody", now).is_none());
        // Past its lease the claim reads free.
        assert!(!board(&records, "ses_0000lagoon", until)
            .unwrap()
            .contains('🔒'));
        let t = tasks_here(&records, "ses_0000lagoon", &[], now);
        assert!(
            t.starts_with("**Task graph here** · 1 open, 1 closed"),
            "{t}"
        );
        assert!(
            t.contains(&format!("🔒 session lagoon until {hm} · v1")),
            "{t}"
        );
        assert!(!t.contains("other part"), "{t}");
    }
}
