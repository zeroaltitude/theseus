//! `/tasks` and `/task`'s words (theseus-a5uds). Pure.
//!
//! - **`/tasks [show:]`**: the board's parts for one place as an ephemeral V2
//!   message, with `↻` (`tb:refresh:<show>`) that draws it again where it is,
//!   and the board's select to open a task.
//! - **`/task name:`'s autocomplete**: a place's tasks as at most 25 choices,
//!   the ones that need you first, then the working, then the ended, each by
//!   its title and short id; what was typed narrows them (title, id or its
//!   last six). `new` is the first choice when nothing narrows it away.

use serde_json::Value;
use theseus_protocol::work::{WorkKind, WorkState, WorkView};

use super::board_v2::{
    ago, ask_line, done_line, headline, short, title_of, working_line, OPEN, PICK,
};
use super::clip;
use super::v2::{button, container, row, section, select, separator, text, validate, Look};
use crate::runtime::emoji::state;

/// `tb:refresh:<show>`.
pub const REFRESH: &str = "tb:refresh:";
/// The choice that opens the new-task modal.
pub const NEW: &str = "new";
/// Discord's most autocomplete choices.
pub const MAX_CHOICES: usize = 25;

/// Which parts `/tasks` shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Show {
    #[default]
    All,
    NeedsYou,
    Working,
    Done,
}

impl Show {
    pub fn parse(s: &str) -> Option<Show> {
        Some(match s {
            "all" => Show::All,
            "needs_you" => Show::NeedsYou,
            "working" => Show::Working,
            "done" => Show::Done,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Show::All => "all",
            Show::NeedsYou => "needs_you",
            Show::Working => "working",
            Show::Done => "done",
        }
    }
}

/// The three parts of a place, as the model projects them.
pub struct Parts<'a> {
    pub needs: &'a [WorkView],
    pub working: &'a [WorkView],
    pub done: &'a [WorkView],
}

fn build(show: Show, p: &Parts<'_>, caps: [usize; 3], now_ms: u64) -> Vec<Value> {
    let want = |s: Show| show == Show::All || show == s;
    let (needs, working, done) = (
        if want(Show::NeedsYou) { p.needs } else { &[] },
        if want(Show::Working) { p.working } else { &[] },
        if want(Show::Done) { p.done } else { &[] },
    );
    let mut inner = vec![text(format!(
        "📋 **Tasks here** · {}",
        headline(needs.len(), working.len(), done.len())
    ))];
    let [cn, cw, cd] = caps;
    if !needs.is_empty() {
        inner.push(separator());
        inner.push(text(format!(
            "**{} Needs you**",
            state(WorkState::NeedsYou)
        )));
        for v in needs.iter().take(cn) {
            inner.push(section(
                vec![text(format!(
                    "**{}** `…{}`\n{}",
                    title_of(v),
                    short(&v.id),
                    ask_line(v)
                ))],
                button(format!("{OPEN}{}", v.id), "Answer ↗", Look::Primary),
            ));
        }
    }
    if !working.is_empty() {
        inner.push(separator());
        let mut lines = vec![format!("**{} Now**", state(WorkState::Running))];
        lines.extend(working.iter().rev().take(cw).map(working_line));
        inner.push(text(lines.join("\n")));
    }
    if !done.is_empty() {
        inner.push(separator());
        let mut lines = vec![format!("**{} Done**", state(WorkState::Done))];
        lines.extend(done.iter().take(cd).map(done_line));
        inner.push(text(lines.join("\n")));
    }
    inner.push(separator());
    inner.push(row(vec![button(
        format!("{REFRESH}{}", show.as_str()),
        "↻ Refresh",
        Look::Secondary,
    )]));
    let options: Vec<(String, String)> = needs
        .iter()
        .take(cn)
        .map(|v| ("🔴", v))
        .chain(working.iter().rev().take(cw).map(|v| ("⚙️", v)))
        .chain(done.iter().take(cd).map(|v| ("✅", v)))
        .map(|(m, v)| (format!("{m} {}", title_of(v)), v.id.clone()))
        .take(MAX_CHOICES)
        .collect();
    if !options.is_empty() {
        inner.push(select(PICK, "Open a task…", &options));
    }
    inner.push(text(format!("-# as of {}", ago(now_ms))));
    vec![container(Some(0x3E_63_DD), inner)]
}

/// `/tasks`' message for `show`, within Discord's limits (the parts' tails
/// fold as they do on the board).
pub fn render(show: Show, p: &Parts<'_>, now_ms: u64) -> Vec<Value> {
    let mut caps = [7, 12, 8];
    loop {
        let c = build(show, p, caps, now_ms);
        if validate(&c).is_ok() {
            return c;
        }
        let shown = [
            p.needs.len().min(caps[0]),
            p.working.len().min(caps[1]),
            p.done.len().min(caps[2]),
        ];
        match (0..3).max_by_key(|&i| shown[i]).filter(|&i| shown[i] > 0) {
            Some(i) => caps[i] -= 1,
            None => return c,
        }
    }
}

/// How a state ranks in the choices: needs you, working, ended.
fn rank(s: WorkState) -> u8 {
    match s {
        WorkState::NeedsYou => 0,
        s if s.working() => 1,
        _ => 2,
    }
}

/// `/task name:`'s choices as `(name, value)`: the tasks among `views`
/// matching `typed`, ranked, at most [`MAX_CHOICES`] with `new` first.
pub fn task_choices(views: &[WorkView], typed: &str) -> Vec<(String, String)> {
    let typed = typed.trim().trim_start_matches('…').to_lowercase();
    let mut tasks: Vec<&WorkView> = views
        .iter()
        .filter(|v| v.kind == WorkKind::Task)
        .filter(|v| {
            typed.is_empty()
                || v.title.to_lowercase().contains(&typed)
                || v.id.to_lowercase().contains(&typed)
                || short(&v.id).to_lowercase().contains(&typed)
        })
        .collect();
    tasks.sort_by_key(|v| {
        (
            rank(v.state),
            std::cmp::Reverse(v.ended_at_ms.unwrap_or(v.state_since_ms)),
            v.id.clone(),
        )
    });
    let mut out = Vec::new();
    if typed.is_empty() || NEW.starts_with(&typed) {
        out.push(("➕ New task".to_string(), NEW.to_string()));
    }
    out.extend(tasks.into_iter().map(|v| {
        let mark = match v.state {
            WorkState::NeedsYou => "🔴",
            s if s.working() => "⚙️",
            WorkState::Done => "✅",
            WorkState::Failed => "❌",
            _ => "⛔",
        };
        (
            clip(&format!("{mark} {} …{}", title_of(v), short(&v.id)), 100),
            v.id.clone(),
        )
    }));
    out.truncate(MAX_CHOICES);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::v2::{custom_ids, texts};
    use theseus_protocol::{Attention, Level};

    fn view(n: usize, state: WorkState, title: &str) -> WorkView {
        let at = 1_790_000_000_000 + n as u64;
        WorkView {
            id: format!("exe_01J0000000000000000000{n:04}"),
            kind: WorkKind::Task,
            position: n as u64,
            at_ms: 0,
            parent: None,
            root: "exe_chat".into(),
            session_id: Some(format!("ses_{n}")),
            title: title.into(),
            place: None,
            state,
            previous: None,
            attention: Attention {
                level: Level::Working,
                label: String::new(),
                since_ms: 0,
            },
            started_at_ms: at,
            state_since_ms: at,
            ended_at_ms: state.ended().then_some(at),
            doing: None,
            progress: None,
            cost: None,
            eta: None,
            question: None,
            report: None,
            woke: None,
            below: None,
            why: None,
            flagged: false,
            dismissed: false,
            retry_of: None,
            forecast: None,
            thread: None,
            purpose: None,
            next_at_ms: None,
            series: None,
            paused: false,
            routine: None,
            watch: None,
            project: None,
        }
    }

    #[test]
    fn the_choices_rank_needs_you_first_then_working_then_ended_and_stop_at_25() {
        let mut v = vec![
            view(1, WorkState::Done, "Old chart"),
            view(2, WorkState::Running, "Paint the buoy"),
            view(3, WorkState::NeedsYou, "Count the tides"),
        ];
        let c = task_choices(&v, "");
        assert_eq!(c[0].1, NEW);
        let order: Vec<&str> = c[1..].iter().map(|x| x.1.as_str()).collect();
        assert_eq!(
            order,
            [
                "exe_01J00000000000000000000003",
                "exe_01J00000000000000000000002",
                "exe_01J00000000000000000000001"
            ]
        );
        for n in 10..60 {
            v.push(view(n, WorkState::Running, "Another task"));
        }
        assert_eq!(task_choices(&v, "").len(), MAX_CHOICES);
        assert!(task_choices(&v, "")
            .iter()
            .all(|c| c.0.chars().count() <= 100));
    }

    #[test]
    fn what_was_typed_narrows_by_title_and_by_short_id_and_new_leaves_with_it() {
        let v = vec![
            view(1, WorkState::Running, "Paint the buoy"),
            view(2, WorkState::Running, "Chart the reef"),
        ];
        let by_title = task_choices(&v, "REEF");
        assert_eq!(by_title.len(), 1);
        assert!(by_title[0].0.contains("Chart the reef"));
        let by_id = task_choices(&v, "0001");
        assert_eq!(by_id.len(), 1);
        assert!(by_id[0].0.contains("Paint the buoy"));
        assert_eq!(task_choices(&v, "ne")[0].1, NEW);
        assert!(task_choices(&v, "reef").iter().all(|c| c.1 != NEW));
    }

    #[test]
    fn show_keeps_only_its_part_and_the_message_stays_valid() {
        let needs = [view(1, WorkState::NeedsYou, "Count the tides")];
        let working = [view(2, WorkState::Running, "Paint the buoy")];
        let p = Parts {
            needs: &needs,
            working: &working,
            done: &[],
        };
        let now = 1_790_000_100_000;
        let only = render(Show::Working, &p, now);
        assert_eq!(validate(&only), Ok(()));
        let t = texts(&only).join("\n");
        assert!(
            t.contains("Paint the buoy") && !t.contains("Count the tides"),
            "{t}"
        );
        assert!(custom_ids(&only).contains(&"tb:refresh:working".to_string()));
        let all = render(Show::All, &p, now);
        let t = texts(&all).join("\n");
        assert!(
            t.contains("Paint the buoy") && t.contains("Count the tides"),
            "{t}"
        );
        assert_eq!(Show::parse("needs_you"), Some(Show::NeedsYou));
        assert_eq!(Show::parse("nope"), None);
    }
}
