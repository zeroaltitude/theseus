//! The task graph in a place (M7 step 39b, theseus-ext.14): the board, one
//! message per place edited in place as its tasks change, and `/tasks`, the
//! records' tree before the task sessions of today. Pure: records in, text
//! out; the claim's time of day is the daemon's clock (`theseus_core::push::hm`).

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use theseus_core::task_graph::TaskRecord;
use theseus_protocol::ConfirmRequest;

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

/// The tasks whose home is `session`: `theseus_core::task_graph::homed`'s,
/// in its order, in time linear in `records` (theseus-83qm). Each step of
/// `home()`'s walk is a lookup in maps built once, where the core's scans
/// every record at each step of each record's walk.
fn homed<'a>(records: &'a [TaskRecord], session: &str) -> Vec<&'a TaskRecord> {
    // The first record of each id and of each task session, as the core's
    // finds take them.
    let mut by_id: HashMap<&str, &TaskRecord> = HashMap::with_capacity(records.len());
    let mut by_session: HashMap<&str, &TaskRecord> = HashMap::new();
    for r in records {
        by_id.entry(r.id.as_str()).or_insert(r);
        if let Some(s) = r.session.as_deref() {
            by_session.entry(s).or_insert(r);
        }
    }
    let root_of = |t: &'a TaskRecord| {
        let mut at = t;
        for _ in 0..32 {
            match at.parent.as_deref().and_then(|p| by_id.get(p).copied()) {
                Some(p) => at = p,
                None => break,
            }
        }
        at
    };
    let home = |t: &'a TaskRecord| {
        let mut at = t;
        for _ in 0..32 {
            let root = root_of(at);
            match by_session.get(root.origin.session.as_str()) {
                Some(&own) if own.id != root.id => at = own,
                _ => return root.origin.session.as_str(),
            }
        }
        at.origin.session.as_str()
    };
    records.iter().filter(|&t| home(t) == session).collect()
}

/// The tree of `tasks` (a place's, oldest first), each child under its
/// parent in the plan's order: the open roots first, newest first, then the
/// closed ones, newest first (theseus-83qm). Past `MAX_LINES` the closed
/// lines go first: every open task stays, with any closed task above it, and
/// the closed lines fill the room left in their order, so each line kept has
/// its parent's above it. The lines, and the line that says what was left
/// out.
fn tree(tasks: &[&TaskRecord], now_ms: u64, version: bool) -> (Vec<String>, Option<String>) {
    let mut here: HashMap<&str, &TaskRecord> = HashMap::with_capacity(tasks.len());
    for &t in tasks {
        here.entry(t.id.as_str()).or_insert(t);
    }
    let mut kids: HashMap<&str, Vec<&TaskRecord>> = HashMap::new();
    let mut roots: Vec<(usize, &TaskRecord)> = Vec::new();
    for (i, &t) in tasks.iter().enumerate() {
        match t.parent.as_deref().filter(|p| here.contains_key(p)) {
            Some(p) => kids.entry(p).or_default().push(t),
            None => roots.push((i, t)),
        }
    }
    // A later record breaks a tie in time.
    roots.sort_by_key(|&(i, t)| (t.state.is_closed(), Reverse((t.created_at_ms, i))));
    // Depth first: each task with its depth.
    let mut order: Vec<(&TaskRecord, usize)> = Vec::with_capacity(tasks.len());
    let mut seen: HashSet<&str> = HashSet::with_capacity(tasks.len());
    let mut stack: Vec<(&TaskRecord, usize)> = roots.iter().rev().map(|&(_, t)| (t, 0)).collect();
    while let Some((t, d)) = stack.pop() {
        if !seen.insert(t.id.as_str()) {
            continue;
        }
        order.push((t, d));
        if let Some(cs) = kids.get(t.id.as_str()) {
            stack.extend(cs.iter().rev().map(|&c| (c, d + 1)));
        }
    }
    let mut keep = vec![true; order.len()];
    if order.len() > MAX_LINES {
        let mut needed: HashSet<&str> = HashSet::new();
        for &(t, _) in order.iter().filter(|(t, _)| !t.state.is_closed()) {
            let mut at = Some(t);
            while let Some(x) = at.filter(|x| needed.insert(x.id.as_str())) {
                at = x.parent.as_deref().and_then(|p| here.get(p).copied());
            }
        }
        let mut first = MAX_LINES;
        let mut room = MAX_LINES.saturating_sub(needed.len());
        for (k, (t, _)) in keep.iter_mut().zip(&order) {
            let slot = if needed.contains(t.id.as_str()) {
                &mut first
            } else {
                &mut room
            };
            *k = *slot > 0;
            *slot = slot.saturating_sub(1);
        }
    }
    let (mut lines, mut open, mut closed) = (Vec::new(), 0usize, 0usize);
    for (&(t, d), k) in order.iter().zip(keep) {
        if k {
            lines.push(format!(
                "{}• {}",
                "  ".repeat(d.min(16)),
                task_line(t, now_ms, version)
            ));
        } else if t.state.is_closed() {
            closed += 1;
        } else {
            open += 1;
        }
    }
    let which = match (open, closed) {
        (0, 0) => return (lines, None),
        (_, 0) => "all open".to_string(),
        (0, _) => "all closed".to_string(),
        (o, _) => format!("{o} of them open"),
    };
    let left = format!(
        "-# and {} more, {which}; `theseus tasks` lists them all",
        open + closed
    );
    (lines, Some(left))
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
    out.extend(left);
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
        out.extend(left);
    }
    if mine.is_empty() || !of_today.is_empty() {
        out.push(super::tasks(&of_today, now_ms));
    } else {
        out.push("-# No task sessions here today.".into());
    }
    out.join("\n")
}

/// The layer-1 card (39b): "Change the acceptance of tsk_… (title)? Before:
/// … After: …", answered with Accept and Decline on Approve's and Decline's
/// ids (`Buttons::Accept`).
pub fn change_card(
    req: &ConfirmRequest,
    c: &theseus_protocol::tasks::TaskChange,
    task: &str,
    route: &super::Route,
    elsewhere: &str,
) -> super::CardText {
    let asked_for = match route {
        super::Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let also = match elsewhere {
        "" => String::new(),
        e => format!(" · you can also answer {e}"),
    };
    let content = format!(
        "📝 {task}{}\n-# {asked_for}Accept applies it; Decline leaves the task as it is · expires <t:{}:R>{also}",
        clip(&c.question(), 1500),
        req.expires_at_ms / 1000
    );
    super::CardText {
        content,
        line: format!("{task}the {} of task `…{}`", c.field, short_id(&c.task)),
        budget: false,
    }
}

fn short_id(id: &str) -> &str {
    id.get(id.len().saturating_sub(6)..).unwrap_or(id)
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
        assert!(t.ends_with("-# No task sessions here today."), "{t}");
    }

    /// A lagoon task made at `at_ms`.
    fn made(id: &str, parent: Option<&str>, state: TaskState, at_ms: u64) -> TaskRecord {
        TaskRecord {
            created_at_ms: at_ms,
            ..task(id, parent, state)
        }
    }

    /// theseus-83qm: a place that has had 30 tasks, the oldest 29 closed,
    /// shows its open one first on the board and in `/tasks`, then the
    /// closed ones newest first; the cut takes closed lines and says so.
    #[test]
    fn an_open_task_behind_29_closed_shows_first() {
        let mut records: Vec<TaskRecord> = (0..29)
            .map(|i| made(&format!("tsk_{i:010}"), None, TaskState::Done, i))
            .collect();
        records.push(made("tsk_000000reef", None, TaskState::Accepted, 29));
        let b = board(&records, "ses_0000lagoon", 1_000).unwrap();
        let lines: Vec<&str> = b.lines().collect();
        assert_eq!(lines[0], "📋 **Task board** · 1 open, 29 closed");
        assert_eq!(
            lines[1],
            "• `…00reef` the tsk_000000reef part — accepted · agent"
        );
        assert_eq!(
            lines[2],
            "• `…000028` the tsk_0000000028 part — done · agent"
        );
        assert_eq!(lines.len(), 1 + 25 + 2, "{b}");
        assert_eq!(
            lines[26],
            "-# and 5 more, all closed; `theseus tasks` lists them all"
        );
        let t = tasks_here(&records, "ses_0000lagoon", &[], 1_000);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(
            lines[1],
            "• `…00reef` the tsk_000000reef part — accepted · agent · v1"
        );
        assert_eq!(
            lines[26],
            "-# and 5 more, all closed; `theseus tasks` lists them all"
        );
    }

    /// Past 25 lines the closed lines go first: an open step after 27 closed
    /// ones in its plan stays under its parent, and so does a closed task
    /// above an open one.
    #[test]
    fn the_cut_takes_closed_lines_first() {
        let mut records = vec![made("tsk_000000reef", None, TaskState::Accepted, 0)];
        records.extend((1..=27).map(|i| {
            made(
                &format!("tsk_{i:010}"),
                Some("tsk_000000reef"),
                TaskState::Done,
                i,
            )
        }));
        records.push(made(
            "tsk_00000north",
            Some("tsk_000000reef"),
            TaskState::Accepted,
            28,
        ));
        records.push(made("tsk_00000atoll", None, TaskState::Done, 29));
        records.push(made(
            "tsk_000000buoy",
            Some("tsk_00000atoll"),
            TaskState::Accepted,
            30,
        ));
        let b = board(&records, "ses_0000lagoon", 1_000).unwrap();
        let lines: Vec<&str> = b.lines().collect();
        assert_eq!(lines[0], "📋 **Task board** · 3 open, 28 closed");
        assert_eq!(
            lines[1],
            "• `…00reef` the tsk_000000reef part — accepted · agent"
        );
        assert_eq!(
            lines[2],
            "  • `…000001` the tsk_0000000001 part — done · agent"
        );
        assert_eq!(
            lines[22],
            "  • `…000021` the tsk_0000000021 part — done · agent"
        );
        assert_eq!(
            &lines[23..26],
            [
                "  • `…0north` the tsk_00000north part — accepted · agent",
                "• `…0atoll` the tsk_00000atoll part — done · agent",
                "  • `…00buoy` the tsk_000000buoy part — accepted · agent",
            ]
        );
        assert_eq!(
            lines[26],
            "-# and 6 more, all closed; `theseus tasks` lists them all"
        );
        // More open tasks than lines: the first 25, and the count of each.
        let mut many: Vec<TaskRecord> = (0..30)
            .map(|i| made(&format!("tsk_{i:010}"), None, TaskState::Accepted, i))
            .collect();
        many.push(made("tsk_00000atoll", None, TaskState::Done, 30));
        let b = board(&many, "ses_0000lagoon", 1_000).unwrap();
        assert!(
            b.contains("\n-# and 6 more, 5 of them open; `theseus tasks` lists them all\n"),
            "{b}"
        );
    }

    /// A place's tasks are the core's `homed`, in its order, for every
    /// session: task sessions inside task sessions, a missing parent, and two
    /// tasks each the other's parent (theseus-83qm made the walk linear here).
    #[test]
    fn the_places_tasks_are_the_cores_homed() {
        let mut delegated = task("tsk_000delegate", None, TaskState::Accepted);
        delegated.session = Some("ses_000delegate".into());
        let reef = task("tsk_000000reef", None, TaskState::Accepted);
        let north = task("tsk_00000north", Some("tsk_000000reef"), TaskState::Done);
        let mut inside = task("tsk_0000inside", None, TaskState::Accepted);
        inside.origin.session = "ses_000delegate".into();
        let mut under = task("tsk_00000under", Some("tsk_0000inside"), TaskState::Done);
        under.origin.session = "ses_000delegate".into();
        let mut nest = task("tsk_000000nest", None, TaskState::Accepted);
        nest.session = Some("ses_000000nest".into());
        nest.origin.session = "ses_000delegate".into();
        let mut deep = task("tsk_000000deep", None, TaskState::Accepted);
        deep.origin.session = "ses_000000nest".into();
        let mut other = task("tsk_00000other", None, TaskState::Accepted);
        other.origin.session = "ses_00000atoll".into();
        let orphan = task("tsk_0000orphan", Some("tsk_gone"), TaskState::Done);
        let loop_a = task("tsk_000loop_a", Some("tsk_000loop_b"), TaskState::Accepted);
        let mut loop_b = task("tsk_000loop_b", Some("tsk_000loop_a"), TaskState::Accepted);
        loop_b.origin.session = "ses_00000atoll".into();
        let records = vec![
            delegated, reef, north, inside, under, nest, deep, other, orphan, loop_a, loop_b,
        ];
        let ids = |v: Vec<&TaskRecord>| v.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        for s in [
            "ses_0000lagoon",
            "ses_000delegate",
            "ses_000000nest",
            "ses_00000atoll",
            "ses_00000nobody",
        ] {
            assert_eq!(
                ids(homed(&records, s)),
                ids(theseus_core::task_graph::homed(&records, s)),
                "{s}"
            );
        }
        assert_eq!(homed(&records, "ses_0000lagoon").len(), 9);
        assert_eq!(homed(&records, "ses_00000atoll").len(), 2);
    }

    /// The layer-1 card's words (39b): the change, before and after, with
    /// what Accept and Decline do.
    #[test]
    fn the_layer_one_card_says_the_change_before_and_after() {
        let req: ConfirmRequest = serde_json::from_value(serde_json::json!({
            "correlation_id": "act_0000reef", "session_id": "ses_0000lagoon",
            "execution_id": "exe_0000lagoon", "tool": "task.update", "input": {},
            "reason": "layer 1", "by": "operator", "requested_at_ms": 1_000, "expires_at_ms": 901_000,
            "change": {"task": "tsk_000000reef", "title": "Chart the reef", "field": "acceptance",
                       "before": "every marker has a depth", "after": "the chart is signed"}
        }))
        .unwrap();
        let c = crate::render::card(&req, &crate::render::Route::Here, "");
        assert_eq!(
            c.content,
            "📝 Change the acceptance of tsk_000000reef (Chart the reef)? Before: every marker \
             has a depth After: the chart is signed\n-# Accept applies it; Decline leaves the task \
             as it is · expires <t:901:R>"
        );
        assert_eq!(c.line, "the acceptance of task `…00reef`");
        assert!(!c.budget);
    }
}
