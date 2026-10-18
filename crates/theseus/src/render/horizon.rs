//! The horizon as a terminal shows it (theseus-nuhsz; horizon HZ8): the
//! future in seven sections, built from the work board's future views and
//! `work.agenda`'s occurrences, for `theseus agenda` and the TUI's Horizon
//! section. Pure: the caller passes the clock and its day.
//!
//! - **Now**: what needs you or runs, or is overdue (`horizon_of`).
//! - **Today**, **This week**, **Later**: occurrences by their time on this
//!   machine's clock (`bucket_at`). Later holds one-shot things alone; a
//!   series is shown whole under Recurring.
//! - **Recurring**: each routine and repeating reminder with its next time and
//!   its last 30 days.
//! - **Waiting on the world**: each watch with what it last saw.
//! - **Projects**.

use theseus_protocol::work::horizon::{
    bucket_at, horizon_of, AgendaLine, AgendaWhat, Horizon, LocalDay,
};
use theseus_protocol::work::{WorkKind, WorkState, WorkView};

use super::time::{fmt_hm, fmt_starts, local_day_at};
use super::{Line, Tag};

/// One row of a section: the words, and what a key acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The work's id (a wake's, a task's, a project's).
    pub id: String,
    pub kind: WorkKind,
    pub purpose: Option<String>,
    pub session_id: Option<String>,
    pub paused: bool,
    pub tag: Tag,
    pub text: String,
    /// The same row in a narrow place (a sidebar): the time or the glyph, and
    /// the title, with what matters most after.
    pub compact: String,
}

/// A horizon's heading and rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub horizon: Horizon,
    pub rows: Vec<Row>,
}

/// The whole future, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Agenda {
    pub header: String,
    pub sections: Vec<Section>,
}

/// This machine's day around `now_ms`: where today ends and the week with it.
pub fn local_day(now_ms: u64) -> LocalDay {
    LocalDay {
        tomorrow_ms: local_day_at(now_ms, -1, 0, 0).unwrap_or(now_ms + 86_400_000),
        week_end_ms: local_day_at(now_ms, -8, 0, 0).unwrap_or(now_ms + 8 * 86_400_000),
    }
}

/// The window `work.agenda` is asked for: from now to a month ahead.
pub fn window(now_ms: u64) -> (u64, u64) {
    (now_ms, now_ms + 30 * 86_400_000)
}

/// The last six characters of an id: how people name it.
pub fn short(id: &str) -> String {
    let n = id.chars().count();
    id.chars().skip(n.saturating_sub(6)).collect()
}

/// Whether a view belongs to the future: dated, or a routine, a watch or a
/// project.
pub fn is_future(v: &WorkView) -> bool {
    matches!(
        v.kind,
        WorkKind::Routine | WorkKind::Watch | WorkKind::Project
    ) || v.next_at_ms.is_some()
}

/// What a verb names: a routine by its session (`routine pause 9d0939`), the
/// rest by their own id.
fn name_of(kind: WorkKind, id: &str, session: Option<&str>) -> String {
    match (kind, session) {
        (WorkKind::Routine, Some(s)) => short(s),
        _ => short(id),
    }
}

fn icon(kind: WorkKind, purpose: Option<&str>) -> &'static str {
    match (kind, purpose) {
        (WorkKind::Routine, _) => "🔁",
        (WorkKind::Watch, _) => "👁",
        (WorkKind::Project, _) => "◆",
        (WorkKind::Task, _) => "▷",
        (_, Some("remind")) => "⏰",
        _ => "⏳",
    }
}

fn clip(s: &str, n: usize) -> String {
    let one = s.lines().next().unwrap_or("");
    let t: String = one.chars().take(n).collect();
    if t.chars().count() < one.chars().count() {
        format!("{t}…")
    } else {
        t
    }
}

/// A series in words: `every 1d`, `mon,fri every 1d`, or the cron text.
fn series_words(v: &WorkView) -> String {
    match (&v.routine, &v.series) {
        (Some(r), _) if !r.schedule.is_empty() => r.schedule.clone(),
        (_, Some(s)) => match (&s.cron, &s.every) {
            (Some(c), _) => c.clone(),
            (None, Some(e)) if s.days.is_empty() => format!("every {e}"),
            (None, Some(e)) => format!("{} every {e}", s.days.join(",")),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn row(v: &WorkView, tag: Tag, text: String, compact: String) -> Row {
    Row {
        compact,
        id: v.id.clone(),
        kind: v.kind,
        purpose: v.purpose.clone(),
        session_id: v.session_id.clone(),
        paused: v.paused,
        tag,
        text,
    }
}

fn line_row(l: &AgendaLine, text: String, compact: String) -> Row {
    Row {
        compact,
        id: l.id.clone(),
        kind: l.kind,
        purpose: l.purpose.clone(),
        session_id: l.session_id.clone(),
        paused: false,
        tag: Tag::Plain,
        text,
    }
}

/// The sections, in the order a person reads the future. `views` are the
/// board's future views (`work.watch`'s snapshot holds them beside the tree),
/// `lines` `work.agenda`'s occurrences from `window(now_ms)`.
pub fn build(views: &[WorkView], lines: &[AgendaLine], now_ms: u64, day: &LocalDay) -> Agenda {
    let mut by: Vec<Vec<Row>> = Horizon::ALL.iter().map(|_| Vec::new()).collect();
    let mut dated: Vec<(Horizon, u64, Row)> = Vec::new();
    let slot = |h: Horizon| Horizon::ALL.iter().position(|x| *x == h).unwrap_or(0);
    let open: Vec<&WorkView> = views
        .iter()
        .filter(|v| is_future(v))
        .filter(|v| !v.state.ended() || v.state == WorkState::Failed)
        .collect();
    for v in &open {
        match horizon_of(v, now_ms, day) {
            Horizon::Now => by[slot(Horizon::Now)].push(now_row(v, now_ms)),
            Horizon::Recurring => by[slot(Horizon::Recurring)].push(recurring_row(v, now_ms)),
            Horizon::World => by[slot(Horizon::World)].push(world_row(v, now_ms)),
            Horizon::Projects => by[slot(Horizon::Projects)].push(project_row(v, now_ms)),
            _ => {}
        }
    }
    for l in lines {
        if matches!(l.what, AgendaWhat::Expires) {
            continue;
        }
        let h = bucket_at(l.at_ms, now_ms, day);
        let (h, when) = match h {
            Horizon::Today => (h, fmt_hm(l.at_ms)),
            Horizon::Week => (h, fmt_starts(l.at_ms, now_ms)),
            Horizon::Later if l.occurrence.is_none() && l.what != AgendaWhat::Run => {
                (h, fmt_starts(l.at_ms, now_ms))
            }
            _ => continue,
        };
        let what = match l.what {
            AgendaWhat::Starts => " (starts)",
            AgendaWhat::Run => " (run)",
            _ => "",
        };
        dated.push((
            h,
            l.at_ms,
            line_row(
                l,
                format!(
                    "{when:<12} {} {}{what}  {}",
                    icon(l.kind, l.purpose.as_deref()),
                    clip(&l.title, 60),
                    name_of(l.kind, &l.id, l.session_id.as_deref())
                ),
                format!(
                    "{when} {} {}",
                    icon(l.kind, l.purpose.as_deref()),
                    clip(&l.title, 40)
                ),
            ),
        ));
    }
    dated.sort_by(|a, b| (a.1, &a.2.id).cmp(&(b.1, &b.2.id)));
    for (h, _, row) in dated {
        by[slot(h)].push(row);
    }
    let sections: Vec<Section> = Horizon::ALL
        .iter()
        .zip(by)
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(h, rows)| Section { horizon: *h, rows })
        .collect();
    Agenda {
        header: header(&sections, &open, now_ms),
        sections,
    }
}

fn header(sections: &[Section], open: &[&WorkView], now_ms: u64) -> String {
    let count = |h: Horizon| {
        sections
            .iter()
            .find(|s| s.horizon == h)
            .map_or(0, |s| s.rows.len())
    };
    let failed = open
        .iter()
        .filter(|v| v.kind == WorkKind::Routine && v.state == WorkState::NeedsYou)
        .count();
    let mut parts = vec![format!("theseus agenda {}", fmt_hm(now_ms))];
    for (glyph, word, n) in [
        ("●", "now", count(Horizon::Now)),
        ("⏰", "today", count(Horizon::Today)),
        ("📅", "this week", count(Horizon::Week)),
        ("🔁", "recurring", count(Horizon::Recurring)),
        ("👁", "watching", count(Horizon::World)),
        ("◆", "projects", count(Horizon::Projects)),
    ] {
        if n > 0 {
            parts.push(format!("{glyph} {n} {word}"));
        }
    }
    if failed > 0 {
        parts.push(format!("🔁✗ {failed} failed"));
    }
    if parts.len() == 1 {
        parts.push("nothing ahead".into());
    }
    parts.join(" · ")
}

fn now_row(v: &WorkView, now_ms: u64) -> Row {
    let overdue = match v.next_at_ms {
        Some(t) if t <= now_ms && v.state == WorkState::Waiting => {
            format!(" · due {}", fmt_hm(t))
        }
        _ => String::new(),
    };
    let tag = if v.attention.level == theseus_protocol::Level::NeedsYou {
        Tag::Bad
    } else {
        Tag::Plain
    };
    row(
        v,
        tag,
        format!(
            "{} {}  {} · {}{overdue}",
            icon(v.kind, v.purpose.as_deref()),
            clip(&v.title, 50),
            name_of(v.kind, &v.id, v.session_id.as_deref()),
            v.attention.label
        ),
        format!(
            "{} {} · {}",
            icon(v.kind, v.purpose.as_deref()),
            clip(&v.title, 24),
            clip(&v.attention.label, 24)
        ),
    )
}

fn recurring_row(v: &WorkView, now_ms: u64) -> Row {
    let next = match (v.paused, v.next_at_ms) {
        (true, _) => "paused".to_string(),
        (false, Some(t)) => format!("next {}", fmt_starts(t, now_ms)),
        _ => String::new(),
    };
    let stats = v
        .routine
        .as_ref()
        .map(|r| {
            format!(
                " · {} ok {} failed {} skipped (30 d) · ${:.2} of ${:.2} this month",
                r.runs_30d.ok,
                r.runs_30d.failed,
                r.runs_30d.skipped,
                r.month.spent_usd,
                r.month.limit_usd
            )
        })
        .unwrap_or_default();
    let missed = v
        .series
        .as_ref()
        .filter(|s| s.missed_last > 0)
        .map(|s| format!(" · {} missed while the daemon was down", s.missed_last))
        .unwrap_or_default();
    row(
        v,
        Tag::Plain,
        format!(
            "{} {}  {}  {} · {next}{stats}{missed}",
            icon(v.kind, v.purpose.as_deref()),
            name_of(v.kind, &v.id, v.session_id.as_deref()),
            series_words(v),
            clip(&v.title, 40),
        ),
        format!(
            "{} {} · {}",
            icon(v.kind, v.purpose.as_deref()),
            clip(&v.title, 28),
            if v.paused {
                "paused".to_string()
            } else {
                v.next_at_ms
                    .map_or_else(String::new, |t| fmt_starts(t, now_ms))
            }
        ),
    )
}

fn world_row(v: &WorkView, now_ms: u64) -> Row {
    let w = v.watch.as_ref();
    let check = w.map(|w| format!("{} → {}", clip(&w.check, 40), w.until));
    let last = w
        .and_then(|w| w.last.as_ref())
        .map(|l| format!(" · last {} at {}", clip(&l.value, 30), fmt_hm(l.at_ms)))
        .unwrap_or_default();
    let checks = w
        .map(|w| format!(" · {} checks", w.checks))
        .unwrap_or_default();
    let ends = w
        .and_then(|w| w.expires_at_ms)
        .map(|t| format!(" · gives up {}", fmt_starts(t, now_ms)))
        .unwrap_or_default();
    let failing = w
        .and_then(|w| w.failing.as_deref())
        .map(|f| format!(" · FAILING: {}", clip(f, 40)))
        .unwrap_or_default();
    row(
        v,
        if failing.is_empty() {
            Tag::Plain
        } else {
            Tag::Warn
        },
        format!(
            "👁 {}  {}  {}{last}{checks}{ends}{failing}",
            short(&v.id),
            clip(&v.title, 40),
            check.unwrap_or_default()
        ),
        format!(
            "👁 {}{}",
            clip(&v.title, 28),
            w.and_then(|w| w.last.as_ref())
                .map_or_else(String::new, |l| format!(" · {}", clip(&l.value, 12)))
        ),
    )
}

fn project_row(v: &WorkView, now_ms: u64) -> Row {
    let p = v.project.as_ref();
    let next = p
        .and_then(|p| p.next.as_ref())
        .map(|n| match n.due_at_ms {
            Some(t) => format!(" · next {} ({})", clip(&n.title, 30), fmt_starts(t, now_ms)),
            None => format!(" · next {}", clip(&n.title, 30)),
        })
        .unwrap_or_default();
    let tallies = p
        .map(|p| {
            format!(
                " · steps {}/{} · milestones {}/{}{}",
                p.steps.done,
                p.steps.total,
                p.milestones.done,
                p.milestones.total,
                if p.overdue > 0 {
                    format!(" · {} overdue", p.overdue)
                } else {
                    String::new()
                }
            )
        })
        .unwrap_or_default();
    row(
        v,
        if p.is_some_and(|p| p.overdue > 0) {
            Tag::Warn
        } else {
            Tag::Plain
        },
        format!("◆ {}  {}{tallies}{next}", short(&v.id), clip(&v.title, 40)),
        format!(
            "◆ {}{}",
            clip(&v.title, 28),
            p.map_or_else(String::new, |p| format!(
                " · {}/{}",
                p.steps.done, p.steps.total
            ))
        ),
    )
}

/// The occurrences the views themselves hold: each dated, unpaused view's next
/// time, a routine's apart (it is under Recurring). A client with no
/// `work.agenda` call (the TUI) builds the day sections from these; a series
/// shows its next occurrence alone.
pub fn lines_from_views(views: &[WorkView]) -> Vec<AgendaLine> {
    views
        .iter()
        .filter(|v| is_future(v) && !v.paused && !v.state.ended())
        .filter_map(|v| {
            let at_ms = v.next_at_ms?;
            let what = match v.kind {
                // A watch gives up once, and a routine is under Recurring.
                WorkKind::Watch | WorkKind::Routine => return None,
                WorkKind::Task => AgendaWhat::Starts,
                WorkKind::Project => AgendaWhat::Milestone,
                _ => AgendaWhat::Due,
            };
            Some(AgendaLine {
                at_ms,
                id: v.id.clone(),
                kind: v.kind,
                what,
                title: v.title.clone(),
                session_id: v.session_id.clone(),
                purpose: v.purpose.clone(),
                occurrence: v.series.as_ref().map(|s| s.occurrence),
            })
        })
        .collect()
}

/// The verbs line: the commands that act on what is listed.
pub const VERBS: &str = "theseus remind WHEN NOTE · wake snooze ID [WHEN] · answer ID done · wake pause ID · wake resume ID · cancel ID · routine run|pause|resume ID · watch-for rm ID";

/// The agenda as lines, a heading to a section, an indent to a row.
pub fn lines(a: &Agenda) -> Vec<Line> {
    let mut out = vec![Line::new(Tag::Dim, a.header.clone())];
    for s in &a.sections {
        out.push(Line::new(Tag::Plain, String::new()));
        out.push(Line::new(Tag::Plain, heading(s.horizon)));
        for r in &s.rows {
            out.push(Line::new(r.tag, format!("  {}", r.text)));
        }
    }
    if a.sections.is_empty() {
        out.push(Line::new(Tag::Plain, "nothing is ahead".to_string()));
    }
    out.push(Line::new(Tag::Plain, String::new()));
    out.push(Line::new(Tag::Dim, VERBS.to_string()));
    out
}

/// A section's heading.
pub fn heading(h: Horizon) -> String {
    match h {
        Horizon::Recurring => "Recurring · 30 days".to_string(),
        other => other.title().to_string(),
    }
}

#[cfg(test)]
#[path = "horizon_tests.rs"]
mod tests;
