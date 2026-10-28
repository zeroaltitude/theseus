//! The future in Discord (theseus-j90w2; horizon HZ9): the words of the
//! board's **Coming up**, **Routines** and **Projects** sections, of `/agenda`,
//! and of `/routine list`. Pure: the daemon's answers in, text and V2 parts
//! out; Discord's own `<t:…>` stamps carry every time, so a quiet board is
//! never edited for the clock and the viewer's zone is the viewer's.

use serde_json::Value;
use theseus_protocol::routines::RoutineInfo;
use theseus_protocol::usd;
use theseus_protocol::watches::WatchInfo;
use theseus_protocol::work::horizon::{AgendaLine, AgendaWhat};
use theseus_protocol::work::progress_line::progress_line;
use theseus_protocol::work::{WorkKind, WorkState, WorkView};

use super::clip;
use super::v2::{button, row, text, Look};

/// A routine's board buttons: `hr:<verb>:<routine id>`.
pub const ROUTINE_PREFIX: &str = "hr:";
/// How many lines of Coming up the board tries first.
pub const COMING: usize = 5;
/// How many routines the board tries first (each is a text and a row of two
/// buttons: four components).
pub const ROUTINES: usize = 3;
/// How many projects the board tries first.
pub const PROJECTS: usize = 3;
/// How many tasks going `/agenda` names before it folds the rest.
pub const GOING: usize = 6;

/// What the board's future sections are drawn from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FutureParts {
    pub coming: Vec<AgendaLine>,
    pub routines: Vec<RoutineInfo>,
    pub projects: Vec<WorkView>,
    /// Each routine's last runs counted: `(ok, failed)`, by routine id.
    pub records: std::collections::HashMap<String, (u32, u32)>,
}

impl FutureParts {
    pub fn is_empty(&self) -> bool {
        self.coming.is_empty() && self.routines.is_empty() && self.projects.is_empty()
    }
}

/// The emoji of an occurrence: its kind, as `theseus agenda` marks it.
pub fn emoji(l: &AgendaLine) -> &'static str {
    match (l.what, l.purpose.as_deref()) {
        (AgendaWhat::Run, _) => "🔁",
        (AgendaWhat::Starts, _) => "▷",
        (AgendaWhat::Expires, _) => "👁",
        (AgendaWhat::Milestone, _) => "◎",
        (AgendaWhat::Due, Some("watch")) => "👁",
        (AgendaWhat::Due, Some("remind")) => "⏰",
        (AgendaWhat::Due, _) => "💬",
    }
}

/// An occurrence's time: Discord's clock stamp inside a day and a half, its
/// date and time beyond.
pub fn when(at_ms: u64, now_ms: u64) -> String {
    let style = if at_ms.saturating_sub(now_ms) < 36 * 3_600_000 {
        "t"
    } else {
        "f"
    };
    format!("<t:{}:{style}>", at_ms / 1000)
}

/// One line of Coming up: `<time> ⏰ title`.
pub fn coming_line(l: &AgendaLine, now_ms: u64) -> String {
    format!(
        "{} {} {}",
        when(l.at_ms, now_ms),
        emoji(l),
        clip(&l.title.replace('\n', " "), 70)
    )
}

/// The next `n` lines, soonest first, those an owner reads on a board: a
/// watch's expiry is not coming up.
pub fn next_lines(lines: &[AgendaLine], n: usize) -> Vec<AgendaLine> {
    let mut v: Vec<AgendaLine> = lines
        .iter()
        .filter(|l| l.what != AgendaWhat::Expires)
        .cloned()
        .collect();
    v.sort_by_key(|l| (l.at_ms, l.id.clone()));
    v.truncate(n);
    v
}

/// The tasks going now (theseus-1t4q8): a task that has started and not ended, with no start time still ahead of it (a
/// deferred task is an occurrence, `AgendaWhat::Starts`, and a routine's run is a task of its own), longest-running
/// first. A task waiting on the owner is going: it holds a place on the timeline until it ends.
pub fn going(views: &[WorkView]) -> Vec<WorkView> {
    let mut v: Vec<WorkView> = views
        .iter()
        .filter(|v| {
            v.kind == WorkKind::Task
                && !v.state.ended()
                && v.state != WorkState::Queued
                && v.started_at_ms > 0
                && v.next_at_ms.is_none()
        })
        .cloned()
        .collect();
    v.sort_by_key(|v| (v.started_at_ms, v.id.clone()));
    v
}

/// One line of a task going: `▶ title · since <t:…:R> · $0.40 of $2`, the daemon's own progress line after it.
pub fn going_line(v: &WorkView, now_ms: u64) -> String {
    let mut s = format!(
        "▶ {} · since <t:{}:R>",
        clip(&v.title.replace('\n', " "), 50),
        v.started_at_ms / 1000
    );
    if let Some(p) = progress_line(v, now_ms) {
        s.push_str(" · ");
        s.push_str(&clip(&p, 60));
    }
    s
}

/// `$0.86 of $10`.
pub fn month_words(r: &RoutineInfo) -> String {
    format!("{} of {}", usd(r.month_spent_usd), usd(r.month_usd))
}

/// The last run's mark: ✓, ✗, or ⏳ while one runs.
pub fn last_mark(r: &RoutineInfo) -> String {
    if r.running {
        return "⏳ running".into();
    }
    match &r.last {
        None => "no run yet".into(),
        Some(run) => {
            let mark = match run.state.as_str() {
                "complete" => "✓",
                "failed" => "✗",
                "cancelled" => "⏹",
                _ => "✗",
            };
            let blocked = if run.blocked.is_some() {
                " blocked"
            } else {
                ""
            };
            format!("{mark}{blocked} <t:{}:R>", run.started_ms / 1000)
        }
    }
}

/// A routine's 30-day record: `28 ✓ 1 ✗`, from its latest runs.
pub fn record(ok: u32, failed: u32) -> String {
    format!("{ok} ✓ {failed} ✗")
}

/// One routine's line on the board: its next run, its last run, its 30 days
/// and its month.
pub fn routine_line(r: &RoutineInfo, rec: Option<(u32, u32)>) -> String {
    let next = if r.paused {
        "paused".to_string()
    } else {
        match r.next_ms {
            Some(ms) => format!("next <t:{}:R>", ms / 1000),
            None => "no next run".into(),
        }
    };
    let mut bits = vec![
        format!("🔁 **{}** `…{}`", clip(&r.title, 50), r.short),
        next,
        last_mark(r),
    ];
    if let Some((ok, failed)) = rec {
        bits.push(record(ok, failed));
    }
    bits.push(month_words(r));
    bits.join(" · ")
}

/// The buttons under a routine: Run now, and Pause or Resume.
pub fn routine_row(r: &RoutineInfo) -> Value {
    let (verb, label) = if r.paused {
        ("resume", "▶ Resume")
    } else {
        ("pause", "⏸ Pause")
    };
    row(vec![
        button(
            format!("{ROUTINE_PREFIX}run:{}", r.id),
            "▶ Run now",
            Look::Secondary,
        ),
        button(
            format!("{ROUTINE_PREFIX}{verb}:{}", r.id),
            label,
            Look::Secondary,
        ),
    ])
}

/// A project's one line: `◎ Harbour survey ▰▰▰▱▱▱▱ 3/7 · next: milestone 2, Oct 21`.
pub fn project_line(v: &WorkView) -> String {
    let title = clip(v.title.trim(), 40);
    let Some(p) = &v.project else {
        return format!("◎ {title}");
    };
    let total = p.steps.total.max(p.milestones.total);
    let done = if p.steps.total >= p.milestones.total {
        p.steps.done
    } else {
        p.milestones.done
    };
    let width = total.min(7);
    let filled = if total == 0 {
        0
    } else {
        ((done as f64 / total as f64) * width as f64).round() as u32
    };
    let bar: String = (0..width)
        .map(|i| if i < filled { '▰' } else { '▱' })
        .collect();
    let mut s = format!("◎ {title}");
    if total > 0 {
        s.push_str(&format!(" {bar} {done}/{total}"));
    }
    if let Some(n) = &p.next {
        s.push_str(&format!(" · next: {}", clip(&n.title, 40)));
        if let Some(d) = n.due_at_ms {
            s.push_str(&format!(", <t:{}:D>", d / 1000));
        }
    }
    if p.overdue > 0 {
        s.push_str(&format!(" · {} overdue", p.overdue));
    }
    s
}

/// Coming up as one text display: its heading and lines (at most `cap`, the
/// rest folded into `+N more · /agenda`).
pub fn coming_text(lines: &[AgendaLine], now_ms: u64, cap: usize) -> Value {
    let mut out = vec!["**⏰ Coming up**".to_string()];
    out.extend(lines.iter().take(cap).map(|l| coming_line(l, now_ms)));
    if lines.len() > cap {
        out.push(format!("-# +{} more · /agenda", lines.len() - cap));
    }
    text(out.join("\n"))
}

/// Projects as one text display.
pub fn projects_text(projects: &[WorkView], cap: usize) -> Value {
    let mut out = vec!["**◎ Projects**".to_string()];
    out.extend(projects.iter().take(cap).map(project_line));
    if projects.len() > cap {
        out.push(format!("-# +{} more · /agenda", projects.len() - cap));
    }
    text(out.join("\n"))
}

// ---- /agenda ------------------------------------------------------------

/// What `/agenda` shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Today,
    Week,
    All,
}

impl Span {
    pub fn parse(s: &str) -> Span {
        match s {
            "today" => Span::Today,
            "week" => Span::Week,
            _ => Span::All,
        }
    }
}

/// The ephemeral `/agenda` text: Coming up within the span, Routines (each
/// with its next run and last result), Waiting on the world (each watch with
/// what it last saw), Projects. Within Discord's 2,000 characters; the rest
/// is named, not dropped silently.
pub fn agenda_text(
    span: Span,
    now_ms: u64,
    lines: &[AgendaLine],
    routines: &[RoutineInfo],
    watches: &[WatchInfo],
    projects: &[WorkView],
    going_now: &[WorkView],
) -> String {
    let horizon = match span {
        Span::Today => now_ms + 24 * 3_600_000,
        Span::Week => now_ms + 7 * 86_400_000,
        Span::All => u64::MAX,
    };
    let mut coming: Vec<&AgendaLine> = lines
        .iter()
        .filter(|l| l.what != AgendaWhat::Expires && l.at_ms <= horizon)
        .collect();
    coming.sort_by_key(|l| (l.at_ms, l.id.clone()));
    let mut out: Vec<String> = Vec::new();
    let head = match span {
        Span::Today => "today",
        Span::Week => "this week",
        Span::All => "all of it",
    };
    out.push(format!("**📅 Agenda** · {head}"));
    out.push("**⏰ Coming up**".into());
    if coming.is_empty() && going_now.is_empty() {
        out.push("nothing dated".into());
    }
    // New lines, ahead of what is coming (theseus-1t4q8): the tasks going now.
    out.extend(going_now.iter().take(GOING).map(|v| going_line(v, now_ms)));
    if going_now.len() > GOING {
        out.push(format!(
            "-# +{} more going · /task",
            going_now.len() - GOING
        ));
    }
    out.extend(coming.iter().take(15).map(|l| coming_line(l, now_ms)));
    if coming.len() > 15 {
        out.push(format!("-# +{} more", coming.len() - 15));
    }
    if !routines.is_empty() {
        out.push("**🔁 Routines**".into());
        out.extend(routines.iter().take(8).map(|r| routine_line(r, None)));
        if routines.len() > 8 {
            out.push(format!("-# +{} more · /routine list", routines.len() - 8));
        }
    }
    if !watches.is_empty() && span != Span::Today {
        out.push("**👁 Waiting on the world**".into());
        for w in watches.iter().take(6) {
            let seen = w
                .value
                .as_deref()
                .map(|v| format!(" (last: {})", clip(v, 30)))
                .unwrap_or_default();
            out.push(format!(
                "👁 {} · until {}{}",
                clip(&w.title, 50),
                clip(&w.until, 30),
                seen
            ));
        }
        if watches.len() > 6 {
            out.push(format!("-# +{} more · /watch list", watches.len() - 6));
        }
    }
    if !projects.is_empty() && span == Span::All {
        out.push("**◎ Projects**".into());
        out.extend(projects.iter().take(5).map(project_line));
    }
    clip_to(&out.join("\n"), 1_900)
}

/// `text` within `n` characters, ending on a line, saying it was cut.
fn clip_to(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        return text.to_string();
    }
    let mut out = String::new();
    for line in text.lines() {
        if out.chars().count() + line.chars().count() + 30 > n {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("-# …cut; `theseus agenda` has the rest");
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use theseus_protocol::routines::RunRow;
    use theseus_protocol::work::WorkKind;

    const NOW: u64 = 1_790_000_000_000;

    fn line(at: u64, title: &str, what: AgendaWhat, purpose: Option<&str>) -> AgendaLine {
        AgendaLine {
            at_ms: at,
            id: format!("wak_{at}"),
            kind: WorkKind::Wake,
            what,
            title: title.into(),
            session_id: None,
            purpose: purpose.map(Into::into),
            occurrence: None,
        }
    }

    pub(crate) fn routine(title: &str, paused: bool, state: &str) -> RoutineInfo {
        RoutineInfo {
            id: "ses_01J0000000000000000000a41d04".into(),
            short: "a41d04".into(),
            title: title.into(),
            schedule: "weekdays 08:00".into(),
            series: "weekdays 08:00".into(),
            brief: "do it".into(),
            paused,
            next_ms: Some(NOW + 3_600_000),
            next: Some("Fri 08:00".into()),
            run_usd: 0.5,
            month_usd: 10.0,
            month_spent_usd: 0.86,
            deliver_to: "dm".into(),
            catch_up: "auto".into(),
            speak: "always".into(),
            runs: 22,
            grant: vec![],
            streak: 0,
            running: false,
            last: Some(RunRow {
                task: "ses_t".into(),
                short: "t".into(),
                run: 22,
                state: state.into(),
                started_ms: NOW - 7_200_000,
                started_local: "Thu 08:00".into(),
                duration_ms: 52_000,
                cost_usd: 0.04,
                report: None,
                blocked: None,
            }),
        }
    }

    #[test]
    fn coming_up_lines_carry_the_time_the_kind_and_the_title() {
        let l = line(NOW + 3_600_000, "stretch", AgendaWhat::Due, Some("remind"));
        assert_eq!(
            coming_line(&l, NOW),
            format!("<t:{}:t> ⏰ stretch", (NOW + 3_600_000) / 1000)
        );
        let far = line(NOW + 3 * 86_400_000, "report", AgendaWhat::Run, None);
        assert!(coming_line(&far, NOW).contains(":f> 🔁 report"));
    }

    #[test]
    fn the_next_five_leave_out_a_watchs_giving_up() {
        let mut lines: Vec<AgendaLine> = (0..8)
            .map(|i| {
                line(
                    NOW + (8 - i) * 60_000,
                    &format!("t{i}"),
                    AgendaWhat::Due,
                    Some("remind"),
                )
            })
            .collect();
        lines.push(line(NOW + 1, "gives up", AgendaWhat::Expires, None));
        let next = next_lines(&lines, COMING);
        assert_eq!(next.len(), 5);
        assert!(next.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
        assert!(next.iter().all(|l| l.title != "gives up"));
    }

    #[test]
    fn a_routines_line_says_next_last_record_and_month() {
        let r = routine("Morning digest", false, "complete");
        let s = routine_line(&r, Some((28, 1)));
        assert!(s.contains("**Morning digest**"), "{s}");
        assert!(s.contains("next <t:"), "{s}");
        assert!(s.contains("✓ <t:"), "{s}");
        assert!(s.contains("28 ✓ 1 ✗"), "{s}");
        assert!(s.ends_with("$0.86 of $10"), "{s}");
        let bad = routine("Morning digest", false, "failed");
        assert!(routine_line(&bad, None).contains("✗ <t:"));
        let held = routine("Morning digest", true, "complete");
        assert!(routine_line(&held, None).contains("paused"));
    }

    #[test]
    fn a_routines_row_offers_run_now_and_pause_or_resume() {
        let r = routine("Morning digest", false, "complete");
        let ids = crate::render::v2::custom_ids(&[routine_row(&r)]);
        assert_eq!(
            ids,
            [
                "hr:run:ses_01J0000000000000000000a41d04",
                "hr:pause:ses_01J0000000000000000000a41d04"
            ]
        );
        let held = routine("Morning digest", true, "complete");
        assert!(crate::render::v2::custom_ids(&[routine_row(&held)])[1].starts_with("hr:resume:"));
    }

    #[test]
    fn the_agenda_text_has_its_sections_and_stays_in_two_thousand() {
        let lines: Vec<AgendaLine> = (0..40)
            .map(|i| {
                line(
                    NOW + 60_000 * (i + 1),
                    &format!("a reminder number {i} with a long title to fill the room"),
                    AgendaWhat::Due,
                    Some("remind"),
                )
            })
            .collect();
        let routines: Vec<RoutineInfo> = (0..12)
            .map(|i| routine(&format!("Routine {i}"), false, "complete"))
            .collect();
        let t = agenda_text(Span::All, NOW, &lines, &routines, &[], &[], &[]);
        assert!(
            t.contains("**⏰ Coming up**") && t.contains("**🔁 Routines**"),
            "{t}"
        );
        assert!(t.chars().count() <= 2000, "{}", t.chars().count());
        let today = agenda_text(Span::Today, NOW, &lines, &[], &[], &[], &[]);
        assert!(today.contains("today"));
        assert!(!today.contains("Routines"));
    }

    fn task(id: &str, state: theseus_protocol::work::WorkState, started: u64) -> WorkView {
        use theseus_protocol::work::Rollup;
        use theseus_protocol::{Attention, Level};
        WorkView {
            moved_to: None,
            id: id.into(),
            kind: WorkKind::Task,
            position: 1,
            at_ms: NOW,
            parent: None,
            root: "exe_chat".into(),
            session_id: Some(format!("ses_{id}")),
            title: format!("the task {id}"),
            place: None,
            state,
            previous: None,
            attention: Attention {
                level: Level::Working,
                label: String::new(),
                since_ms: NOW,
            },
            started_at_ms: started,
            state_since_ms: started,
            ended_at_ms: state.ended().then_some(NOW - 1_000),
            doing: None,
            progress: None,
            question: None,
            cost: None,
            eta: None,
            below: Some(Rollup::default()),
            why: None,
            report: None,
            woke: None,
            expect: None,
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
            data: None,
        }
    }

    #[test]
    fn a_task_going_is_one_new_line_in_coming_up_and_nothing_else_moves() {
        use theseus_protocol::work::WorkState;
        let lines = vec![line(
            NOW + 3_600_000,
            "stretch",
            AgendaWhat::Due,
            Some("remind"),
        )];
        let before = agenda_text(Span::All, NOW, &lines, &[], &[], &[], &[]);
        let mut live = task("tsk_live", WorkState::Running, NOW - 600_000);
        live.cost = Some(theseus_protocol::work::Cost {
            spent_usd: 0.4,
            limit_usd: 2.0,
            turn_usd: None,
        });
        let after = agenda_text(Span::All, NOW, &lines, &[], &[], &[], &[live]);
        let mut extra: Vec<&str> = after.lines().collect();
        let said: Vec<&str> = before.lines().collect();
        let line_going = extra
            .iter()
            .position(|l| l.starts_with("▶ "))
            .unwrap_or_else(|| panic!("no going line: {after}"));
        let going = extra.remove(line_going);
        assert_eq!(extra, said, "every existing line is as it was: {after}");
        assert!(going.contains("the task tsk_live"), "{going}");
        assert!(
            going.contains(&format!("<t:{}:R>", (NOW - 600_000) / 1000)),
            "{going}"
        );
        assert!(going.contains("$0.40 of $2"), "{going}");
        assert!(
            line_going > after.lines().position(|l| l.contains("Coming up")).unwrap(),
            "it is in the Coming up section: {after}"
        );
        assert!(
            line_going
                < after
                    .lines()
                    .position(|l| l.contains("stretch"))
                    .unwrap_or(usize::MAX),
            "tasks going first, then what is coming: {after}"
        );
    }

    #[test]
    fn only_a_started_unfinished_task_is_going() {
        use theseus_protocol::work::WorkState;
        let mut deferred = task("tsk_later", WorkState::Waiting, NOW - 60_000);
        deferred.next_at_ms = Some(NOW + 3_600_000);
        let mut routine = task("wak_r", WorkState::Running, NOW - 60_000);
        routine.kind = WorkKind::Routine;
        let views = vec![
            task("tsk_run", WorkState::Running, NOW - 60_000),
            task("tsk_needs", WorkState::NeedsYou, NOW - 120_000),
            task("tsk_done", WorkState::Done, NOW - 180_000),
            task("tsk_failed", WorkState::Failed, NOW - 180_000),
            task("tsk_queued", WorkState::Queued, 0),
            deferred,
            routine,
        ];
        let ids: Vec<String> = going(&views).into_iter().map(|v| v.id).collect();
        assert_eq!(ids, ["tsk_needs", "tsk_run"], "oldest start first");
        let t = agenda_text(Span::All, NOW, &[], &[], &[], &[], &going(&views));
        assert!(
            !t.contains("nothing dated"),
            "a task going is something: {t}"
        );
        assert_eq!(t.lines().filter(|l| l.starts_with("▶ ")).count(), 2, "{t}");
        let none = agenda_text(Span::All, NOW, &[], &[], &[], &[], &[]);
        assert!(none.contains("nothing dated"), "{none}");
    }

    #[test]
    fn the_going_lines_are_capped_and_the_text_stays_in_two_thousand() {
        use theseus_protocol::work::WorkState;
        let many: Vec<WorkView> = (0..40)
            .map(|i| {
                task(
                    &format!("tsk_{i:02}"),
                    WorkState::Running,
                    NOW - 60_000 * (i + 1),
                )
            })
            .collect();
        let t = agenda_text(Span::All, NOW, &[], &[], &[], &[], &going(&many));
        assert!(t.chars().count() <= 2000, "{}", t.chars().count());
        assert!(
            t.lines().filter(|l| l.starts_with("▶ ")).count() <= 8,
            "{t}"
        );
        assert!(t.contains("-# +"), "the rest is named, not dropped: {t}");
    }
}
