//! The work board as a list of tasks (theseus-mnd9g): `theseus tasks`, and the
//! chat's `/tasks`. One line for each task under its conversation, with a
//! mark, its short id, its title, how long it has run and what it is doing
//! now; under a task that needs you, the command that answers it; and a
//! footer of counts. Pure: the caller passes the views, the time and the
//! width, so the same rows are drawn by the CLI, by the chat, and by a test.
//!
//! Marks: `●` needs you, `◐` works, `✗` failed, `✓` done, `○` anything else.
//! A column that does not fit gives way in this order: the titles are cut
//! (down to a floor), then the activity.
//!
//! The second half is the words of a task's events (`events`): a task
//! started, asks, reported or failed. `theseus tasks --watch` prints them as
//! lines and the chat prints them above its dock, one wording for both.

use std::collections::HashMap;

use theseus_protocol::notices::name_of;
use theseus_protocol::term::words::clip;
use theseus_protocol::work::progress_line::{cost_part, progress_rest, step_words};
use theseus_protocol::work::{DoingKind, Question, QuestionKind, WorkKind, WorkState, WorkView};
use theseus_protocol::Level;

use super::time::fmt_starts;
use super::{Line, Tag};

/// A title is cut no shorter than this before the activity gives way.
const TITLE_FLOOR: usize = 12;
/// An activity is cut no shorter than this.
const ACTIVITY_FLOOR: usize = 10;
/// A long activity (a job's last line, the model's own words) takes no more
/// than this of the width, so the titles stay readable beside it.
const ACTIVITY_MAX: usize = 60;

/// The last six characters of an id: what every command that names a task
/// takes.
pub fn short_id(id: &str) -> String {
    let n = id.chars().count();
    id.chars().skip(n.saturating_sub(6)).collect()
}

/// `2:14`, or `1:02:03` past an hour.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

/// What a task is called: its title, else its kind and the end of its id.
pub fn name(v: &WorkView) -> String {
    let t = first_line(&v.title);
    if t.is_empty() {
        name_of(&v.work_ref())
    } else {
        t.to_string()
    }
}

/// Whether the list has a row for this kind: a task, and what waits on a
/// time or a condition (a routine, a watch, a project), which are no task
/// and count as none.
fn listed(k: WorkKind) -> bool {
    matches!(
        k,
        WorkKind::Task | WorkKind::Routine | WorkKind::Watch | WorkKind::Project
    )
}

/// The counts of the footer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub needs_you: u32,
    pub working: u32,
    pub failed: u32,
    pub done: u32,
}

impl Counts {
    /// `1 needs you · 3 working · 1 failed · 2 done`; what is zero is left
    /// out, and nothing at all says `no tasks`.
    pub fn words(&self) -> String {
        let mut parts = Vec::new();
        for (n, w) in [
            (self.needs_you, "needs you"),
            (self.working, "working"),
            (self.failed, "failed"),
            (self.done, "done"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {w}"));
            }
        }
        if parts.is_empty() {
            "no tasks".to_string()
        } else {
            parts.join(" · ")
        }
    }

    fn add(&mut self, v: &WorkView, asks: bool) {
        match v.state {
            _ if asks => self.needs_you += 1,
            WorkState::Failed => self.failed += 1,
            WorkState::NeedsYou => self.needs_you += 1,
            WorkState::Done => self.done += 1,
            s if s.working() => self.working += 1,
            _ => {}
        }
    }
}

/// The mark of a view.
pub fn mark(v: &WorkView, asks: bool) -> char {
    if asks {
        return '●';
    }
    match v.state {
        WorkState::Failed => '✗',
        WorkState::NeedsYou => '●',
        WorkState::Done => '✓',
        s if s.working() => '◐',
        _ => '○',
    }
}

fn tag_of(v: &WorkView, asks: bool) -> Tag {
    if asks {
        return Tag::Level(Level::NeedsYou);
    }
    match v.state {
        WorkState::Failed => Tag::Bad,
        _ => Tag::Level(v.attention.level),
    }
}

/// The views under one parent, found by an id or a session id.
struct Index<'a> {
    kids: HashMap<&'a str, Vec<&'a WorkView>>,
    /// A session id or an execution id, to the view's own id.
    ids: HashMap<&'a str, &'a str>,
}

impl<'a> Index<'a> {
    fn new(views: &'a [WorkView]) -> Self {
        let mut ids = HashMap::new();
        for v in views {
            ids.insert(v.id.as_str(), v.id.as_str());
            if let Some(s) = v.session_id.as_deref() {
                ids.insert(s, v.id.as_str());
            }
        }
        let mut kids: HashMap<&str, Vec<&WorkView>> = HashMap::new();
        for v in views {
            if let Some(p) = v.parent.as_deref().and_then(|p| ids.get(p)) {
                if *p != v.id {
                    kids.entry(p).or_default().push(v);
                }
            }
        }
        for list in kids.values_mut() {
            list.sort_by_key(|k| (k.started_at_ms, k.id.clone()));
        }
        Self { kids, ids }
    }

    fn under(&self, v: &WorkView) -> &[&'a WorkView] {
        self.kids.get(v.id.as_str()).map_or(&[], Vec::as_slice)
    }

    fn is_root(&self, v: &WorkView) -> bool {
        v.parent
            .as_deref()
            .is_none_or(|p| !self.ids.contains_key(p))
    }
}

/// The open question of a view: its own, else a question under it.
pub fn open_question<'a>(v: &'a WorkView, kids: &[&'a WorkView]) -> Option<&'a Question> {
    if v.state.ended() {
        return None;
    }
    v.question.as_ref().or_else(|| {
        kids.iter()
            .filter(|k| k.kind == WorkKind::Question && !k.state.ended())
            .find_map(|k| k.question.as_ref())
    })
}

/// What a task is doing, in a phrase: its question, a job or a call running
/// with how long, the model with how long, or how it ended.
pub fn activity(v: &WorkView, kids: &[&WorkView], now_ms: u64, clock: fn(u64) -> String) -> String {
    if let Some(q) = open_question(v, kids) {
        if q.kind == QuestionKind::Budget {
            return v.attention.label.clone();
        }
        return format!("asks: {}", first_line(&q.prompt));
    }
    if let Some(w) = ahead(v, now_ms) {
        return w;
    }
    match v.state {
        WorkState::Done => {
            return match v.report.as_ref().and_then(|r| r.first_line.as_deref()) {
                Some(l) if !l.trim().is_empty() => first_line(l).to_string(),
                _ => v.attention.label.clone(),
            };
        }
        WorkState::Failed | WorkState::NeedsYou | WorkState::Cancelled | WorkState::Expired => {
            return v.attention.label.clone();
        }
        _ => {}
    }
    let since = |at: u64| clock(now_ms.saturating_sub(at));
    let running_job = kids
        .iter()
        .find(|k| k.kind == WorkKind::Job && !k.state.ended());
    let s = match (running_job, v.doing.as_ref()) {
        (Some(j), _) => {
            // The job's own last line, sampled by the daemon (job-follow).
            let last = j
                .doing
                .as_ref()
                .and_then(|d| d.last_line.as_deref())
                .map(first_line)
                .filter(|l| !l.is_empty());
            match last {
                Some(l) => format!("job {} › {l}", since(began(j))),
                None => format!("job {}", since(began(j))),
            }
        }
        (None, Some(d)) => match d.what {
            DoingKind::Thinking => format!("model {}", since(d.since_ms)),
            DoingKind::Tool => format!("{} {}", d.label, since(d.since_ms)),
            DoingKind::Job => format!("job {}", since(d.since_ms)),
            _ => d.label.clone(),
        },
        (None, None) => v.attention.label.clone(),
    };
    // `paused` first, so a clipped row never loses it; then the one progress
    // line every surface shares (step, dollars, time left), then the model's
    // own words, or the plan's current step when it has none yet.
    let said = v.progress.as_ref().and_then(|p| {
        p.said
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(p.current.as_deref())
            .map(first_line)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
    });
    let mut parts = Vec::new();
    if v.paused {
        parts.push("paused".to_string());
    }
    parts.push(s);
    parts.extend(progress_rest(v, now_ms));
    parts.extend(said);
    parts.join(" · ")
}

/// What a routine, a watch or a project waits for: `routine · next Fri
/// 09:00`, `watch · paused`; none for any other kind.
fn ahead(v: &WorkView, now_ms: u64) -> Option<String> {
    let what = match v.kind {
        WorkKind::Routine => "routine",
        WorkKind::Watch => "watch",
        WorkKind::Project => "project",
        _ => return None,
    };
    let when = match (v.paused, v.next_at_ms) {
        (true, _) => "paused".to_string(),
        (false, Some(t)) => format!("next {}", fmt_starts(t, now_ms)),
        _ => v.attention.label.clone(),
    };
    Some(format!("{what} · {when}"))
}

/// When a view began, else when it came to its state.
fn began(v: &WorkView) -> u64 {
    if v.started_at_ms > 0 {
        v.started_at_ms
    } else {
        v.state_since_ms
    }
}

/// How long a task has run, or ran: blank while its start is unknown.
fn elapsed(v: &WorkView, now_ms: u64, clock: fn(u64) -> String) -> String {
    if v.started_at_ms == 0 {
        return String::new();
    }
    let end = if v.state.ended() {
        v.ended_at_ms.unwrap_or(v.state_since_ms)
    } else {
        now_ms
    };
    clock(end.saturating_sub(v.started_at_ms))
}

/// `<1m`, `5m`, `1h05m`: a time that changes once a minute, for a screen that
/// is drawn only when something changes (the chat's footer).
pub fn coarse(ms: u64) -> String {
    let m = ms / 60_000;
    match m {
        0 => "<1m".to_string(),
        1..=59 => format!("{m}m"),
        _ => format!("{}h{:02}m", m / 60, m % 60),
    }
}

/// The commands that answer a question, exactly: the first answer and the
/// decline (with a note), or an `ask`'s choices. When the two commands in
/// full do not fit `room`, the second drops its repeated start (`… decline`).
pub fn answer_commands(q: &Question, room: usize) -> String {
    let id = short_id(&q.id);
    let base = format!("theseus answer {id}");
    let ids: Vec<&str> = q
        .options
        .iter()
        .filter(|o| o.id != "choice:other")
        .map(|o| o.id.as_str())
        .collect();
    if q.kind == QuestionKind::Ask {
        // A question with choices never says approve or decline, even when its options did not
        // come with it: its answer is a number.
        if ids.is_empty() {
            return format!("{base} <number>");
        }
        let all: Vec<String> = ids.iter().map(|o| format!("{base} {o}")).collect();
        let full = all.join(" · ");
        if full.chars().count() <= room {
            return full;
        }
        let rest: Vec<String> = ids.iter().skip(1).map(|o| format!("… {o}")).collect();
        let first = ids.first().map(|o| format!("{base} {o}"));
        return first
            .into_iter()
            .chain(rest)
            .collect::<Vec<_>>()
            .join(" · ");
    }
    let yes = ids.first().copied().unwrap_or("approve");
    let no = ids
        .last()
        .copied()
        .filter(|o| *o != yes)
        .unwrap_or("decline");
    let full = format!("{base} {yes} · {base} {no} --note …");
    if full.chars().count() <= room {
        full
    } else {
        format!("{base} {yes} · … {no} --note …")
    }
}

/// The commands that deal with a failed task (theseus-7zgsm): a retry opens
/// a linked task from the same brief, a dismissal stops it needing you. None
/// for anything else, and for a failure already dismissed.
pub fn failed_commands(v: &WorkView) -> Option<String> {
    if v.kind != WorkKind::Task || v.state != WorkState::Failed || v.dismissed {
        return None;
    }
    let id = short_id(v.session_id.as_deref().unwrap_or(&v.id));
    Some(format!("theseus retry {id} · theseus dismiss {id}"))
}

struct Row<'a> {
    v: &'a WorkView,
    kids: &'a [&'a WorkView],
    indent: usize,
    asks: Option<&'a Question>,
}

/// The tree of tasks as lines, `width` columns at most, with the footer.
/// `more` is how many conversations the board holds that the views left out.
pub fn tree(views: &[WorkView], now_ms: u64, width: usize, more: u64) -> Vec<Line> {
    let idx = Index::new(views);
    let mut roots: Vec<&WorkView> = views
        .iter()
        .filter(|v| idx.is_root(v) && (v.kind == WorkKind::Conversation || listed(v.kind)))
        .collect();
    roots.sort_by_key(|v| (v.attention.level.rank(), v.state_since_ms, v.id.clone()));

    let mut counts = Counts::default();
    // Each group: an optional header row, then its task rows.
    let mut groups: Vec<(Option<Row>, Vec<Row>)> = Vec::new();
    for r in &roots {
        let mut rows = Vec::new();
        if listed(r.kind) {
            push_task(&idx, r, 0, &mut rows);
            groups.push((None, rows));
            continue;
        }
        let mut tasks: Vec<&WorkView> = idx
            .under(r)
            .iter()
            .copied()
            .filter(|k| listed(k.kind))
            .collect();
        // What needs you first, the longest waiting first; a task's own
        // tasks stay under it.
        tasks.sort_by_key(|k| (k.attention.level.rank(), k.state_since_ms, k.id.clone()));
        for k in tasks {
            push_task(&idx, k, 1, &mut rows);
        }
        let asks = open_question(r, idx.under(r));
        let header_wanted = !rows.is_empty() || asks.is_some() || r.state.working();
        if !header_wanted {
            continue;
        }
        let header = Row {
            v: r,
            kids: idx.under(r),
            indent: 0,
            asks,
        };
        groups.push((Some(header), rows));
    }
    for (h, rows) in &groups {
        if let Some(h) = h.as_ref().filter(|h| h.asks.is_some()) {
            counts.add(h.v, true);
        }
        for row in rows.iter().filter(|r| r.v.kind == WorkKind::Task) {
            counts.add(row.v, row.asks.is_some());
        }
    }

    let all: Vec<(&Row, bool)> = groups
        .iter()
        .flat_map(|(h, rows)| {
            h.iter()
                .map(|h| (h, true))
                .chain(rows.iter().map(|r| (r, false)))
        })
        .collect();
    let mut out = Vec::new();
    if all.is_empty() {
        out.push(Line::new(Tag::Plain, "no tasks"));
        return out;
    }
    let cells: Vec<Cells> = all
        .iter()
        .map(|(r, _)| Cells::of(r, now_ms, self::clock))
        .collect();
    let layout = Layout::of(&all, &cells, width);
    for ((row, header), cell) in all.iter().zip(&cells) {
        out.push(layout.line(row, cell, *header));
        let pad = " ".repeat(Layout::prefix(row));
        let room = width.saturating_sub(pad.len());
        if let Some(q) = row.asks {
            out.push(Line::new(
                Tag::Dim,
                format!("{pad}{}", clip(&answer_commands(q, room), room)),
            ));
        } else if let Some(cmds) = failed_commands(row.v) {
            out.push(Line::new(Tag::Dim, format!("{pad}{}", clip(&cmds, room))));
        }
    }
    let mut foot = counts.words();
    if more > 0 {
        foot.push_str(&format!(" · +{more} more conversations (--limit)"));
    }
    out.push(Line::new(Tag::Dim, clip(&foot, width)));
    out
}

fn push_task<'a>(idx: &'a Index<'a>, v: &'a WorkView, depth: usize, out: &mut Vec<Row<'a>>) {
    let kids = idx.under(v);
    out.push(Row {
        v,
        kids,
        indent: depth * 2,
        asks: open_question(v, kids),
    });
    for k in kids {
        if k.kind == WorkKind::Task {
            push_task(idx, k, depth + 1, out);
        }
    }
}

/// A row's text before it is laid out.
struct Cells {
    id: String,
    title: String,
    elapsed: String,
    activity: String,
}

impl Cells {
    fn of(r: &Row, now_ms: u64, clock: fn(u64) -> String) -> Self {
        let sid = r.v.session_id.as_deref().unwrap_or(&r.v.id);
        Cells {
            id: short_id(sid),
            title: name(r.v),
            elapsed: elapsed(r.v, now_ms, clock),
            activity: activity(r.v, r.kids, now_ms, clock),
        }
    }
}

/// The columns, fitted once for all rows.
struct Layout {
    title_w: usize,
    elapsed_w: usize,
    activity_w: usize,
}

impl Layout {
    fn of(rows: &[(&Row, bool)], cells: &[Cells], width: usize) -> Self {
        let widest = |f: &dyn Fn(&Cells) -> usize| cells.iter().map(f).max().unwrap_or(0);
        let elapsed_w = widest(&|c| c.elapsed.chars().count());
        let mut title_w = widest(&|c| c.title.chars().count());
        let mut activity_w = widest(&|c| c.activity.chars().count()).min(ACTIVITY_MAX);
        let prefix = rows
            .iter()
            .map(|(r, _)| r.indent + 2 + 6 + 2)
            .max()
            .unwrap_or(10);
        // title, two spaces, elapsed, two spaces, activity.
        let room = width.saturating_sub(prefix + 2 + elapsed_w + 2);
        if title_w + activity_w > room {
            // The titles give way first, down to a floor.
            title_w = title_w.min(room.saturating_sub(activity_w).max(TITLE_FLOOR));
        }
        if title_w + activity_w > room {
            activity_w = room
                .saturating_sub(title_w)
                .max(ACTIVITY_FLOOR.min(activity_w));
        }
        Layout {
            title_w,
            elapsed_w,
            activity_w,
        }
    }

    /// The columns before the title: the indent, the mark, the id.
    fn prefix(r: &Row) -> usize {
        r.indent + 2 + 6 + 2
    }

    fn line(&self, r: &Row, c: &Cells, header: bool) -> Line {
        let lead = " ".repeat(r.indent);
        let mark = mark(r.v, r.asks.is_some());
        let title = clip(&c.title, self.title_w);
        let text = if header {
            // A conversation: its title, then how many of its tasks need
            // you or work, where a task has its time and its doing.
            format!("{lead}{mark} {}  {title}", c.id)
        } else {
            format!(
                "{lead}{mark} {}  {title:<tw$}  {:>ew$}  {}",
                c.id,
                c.elapsed,
                clip(&c.activity, self.activity_w),
                tw = self.title_w,
                ew = self.elapsed_w,
            )
        };
        Line::new(tag_of(r.v, r.asks.is_some()), text.trim_end().to_string())
    }
}

/// The chat's footer: the counts of every task, and a row for each that works
/// or asks (`max` at most, the ones that ask first), drawn with minute
/// clocks so nothing on it changes between minutes. `more` is how many rows
/// `max` left out; `next_ms` is the soonest moment a clock on a row changes.
#[derive(Debug, Default)]
pub struct Footer {
    pub counts: Counts,
    pub rows: Vec<Line>,
    pub more: usize,
    pub next_ms: Option<u64>,
}

pub fn footer(views: &[WorkView], now_ms: u64, width: usize, max: usize) -> Footer {
    let idx = Index::new(views);
    let mut all: Vec<Row> = views
        .iter()
        .filter(|v| v.kind == WorkKind::Task)
        .map(|v| {
            let kids = idx.under(v);
            Row {
                v,
                kids,
                indent: 0,
                asks: open_question(v, kids),
            }
        })
        .collect();
    let mut counts = Counts::default();
    for r in &all {
        counts.add(r.v, r.asks.is_some());
    }
    all.retain(|r| r.asks.is_some() || r.v.state.working());
    all.sort_by_key(|r| {
        (
            r.asks.is_none(),
            r.v.attention.level.rank(),
            r.v.state_since_ms,
            r.v.id.clone(),
        )
    });
    let more = all.len().saturating_sub(max);
    all.truncate(max);
    let shown: Vec<(&Row, bool)> = all.iter().map(|r| (r, false)).collect();
    let cells: Vec<Cells> = all.iter().map(|r| Cells::of(r, now_ms, coarse)).collect();
    let layout = Layout::of(&shown, &cells, width);
    let rows = shown
        .iter()
        .zip(&cells)
        .map(|((r, h), c)| layout.line(r, c, *h))
        .collect();
    // A clock on a row turns over a minute after the moment it counts from.
    let next_ms = all
        .iter()
        .flat_map(|r| {
            let job = r
                .kids
                .iter()
                .find(|k| k.kind == WorkKind::Job && !k.state.ended())
                .map(|j| began(j));
            [
                Some(r.v.started_at_ms),
                r.v.doing.as_ref().map(|d| d.since_ms),
                job,
            ]
        })
        .flatten()
        .filter(|at| *at > 0)
        .map(|at| now_ms + 60_000 - now_ms.saturating_sub(at) % 60_000)
        .min();
    Footer {
        counts,
        rows,
        more,
        next_ms,
    }
}

// ------------------------------------------------------------------ events

/// What happened to a task, as a terminal says it.
#[derive(Debug, Clone, PartialEq)]
pub enum Event<'a> {
    /// A task the board had not shown.
    Started(&'a WorkView),
    /// A question for you, new.
    Asked(&'a WorkView, &'a Question),
    /// A task ended and said what it did.
    Reported(&'a WorkView),
    /// A task failed or is blocked, with why.
    Failed(&'a WorkView),
    Cancelled(&'a WorkView),
    /// The plan moved to another step.
    Step(&'a WorkView),
    /// The forecast says the task has run past its usual time (once).
    PastUsual(&'a WorkView),
    Paused(&'a WorkView),
    Resumed(&'a WorkView),
}

/// The plan's step a view shows: `(step, of)`.
fn step_of(v: &WorkView) -> Option<(u32, u32)> {
    let p = v.progress.as_ref().filter(|p| p.steps_total > 0)?;
    Some(((p.steps_done + 1).min(p.steps_total), p.steps_total))
}

/// The events one view's change makes, `prev` being the last view of the
/// same id (None when the board had not shown it). A task starts, reports and
/// fails; a task or a conversation asks. A conversation's turns are the
/// chat's own.
pub fn events<'a>(prev: Option<&WorkView>, next: &'a WorkView) -> Vec<Event<'a>> {
    if !matches!(next.kind, WorkKind::Task | WorkKind::Conversation) {
        return Vec::new();
    }
    let task = next.kind == WorkKind::Task;
    let mut out = Vec::new();
    if task && prev.is_none() && !next.state.ended() {
        out.push(Event::Started(next));
    }
    let was_q = prev
        .filter(|p| !p.state.ended())
        .and_then(|p| p.question.as_ref())
        .map(|q| q.id.as_str());
    if let Some(q) = next.question.as_ref().filter(|_| !next.state.ended()) {
        if was_q != Some(q.id.as_str()) {
            out.push(Event::Asked(next, q));
        }
    }
    if let Some(p) = prev.filter(|_| task && !next.state.ended()) {
        if step_of(p) != step_of(next) && step_of(next).is_some() {
            out.push(Event::Step(next));
        }
        let past = |v: &WorkView| v.forecast.as_ref().is_some_and(|f| f.past);
        if past(next) && !past(p) {
            out.push(Event::PastUsual(next));
        }
        if next.paused != p.paused {
            out.push(if next.paused {
                Event::Paused(next)
            } else {
                Event::Resumed(next)
            });
        }
    }
    let was = prev.map(|p| p.state);
    if task && (was != Some(next.state) || prev.is_none()) {
        match next.state {
            WorkState::Done => out.push(Event::Reported(next)),
            WorkState::Failed => out.push(Event::Failed(next)),
            WorkState::Cancelled => out.push(Event::Cancelled(next)),
            WorkState::NeedsYou
                if next.question.is_none()
                    && next.report.as_ref().is_some_and(|r| !r.outcome.is_empty()) =>
            {
                out.push(Event::Failed(next));
            }
            _ => {}
        }
    }
    out
}

/// An event in words, one line: `◐ started: Paint log (2a329e)`,
/// `● Paint log asks: write regatta/paint-log.md (37 B)?`, `✓ Paint log
/// reported: the hull is red`, `✗ Paint log failed: the provider refused`.
pub fn event_line(e: &Event) -> Line {
    match e {
        Event::Started(v) => Line::new(
            Tag::Level(Level::Working),
            format!(
                "◐ started: {} ({})",
                name(v),
                short_id(v.session_id.as_deref().unwrap_or(&v.id))
            ),
        ),
        Event::Asked(v, q) => Line::new(
            Tag::Ask,
            format!("● {} asks: {}?", name(v), first_line(&q.prompt)),
        ),
        Event::Reported(v) => {
            let said = v
                .report
                .as_ref()
                .and_then(|r| r.first_line.as_deref())
                .map(first_line)
                .filter(|l| !l.is_empty());
            Line::new(
                Tag::Ok,
                match said {
                    Some(l) => format!("✓ {} reported: {l}", name(v)),
                    None => format!("✓ {} finished", name(v)),
                },
            )
        }
        Event::Failed(v) => {
            let why = v
                .report
                .as_ref()
                .and_then(|r| r.reason.as_deref())
                .map(first_line)
                .filter(|l| !l.is_empty());
            Line::new(
                Tag::Bad,
                match why {
                    Some(w) => format!("✗ {} failed: {w}", name(v)),
                    None => format!("✗ {} {}", name(v), v.attention.label),
                },
            )
        }
        Event::Cancelled(v) => Line::new(Tag::Warn, format!("✗ {} cancelled", name(v))),
        Event::Step(v) => {
            let mut parts = v
                .progress
                .as_ref()
                .and_then(|p| step_words(p.steps_done, p.steps_total))
                .into_iter()
                .collect::<Vec<_>>();
            parts.extend(cost_part(v));
            Line::new(
                Tag::Level(Level::Working),
                format!("◐ {}: {}", name(v), parts.join(" · ")),
            )
        }
        Event::PastUsual(v) => {
            Line::new(Tag::Warn, format!("◐ {} is past its usual time", name(v)))
        }
        Event::Paused(v) => Line::new(Tag::Warn, format!("⏸ {} paused", name(v))),
        Event::Resumed(v) => {
            Line::new(Tag::Level(Level::Working), format!("▶ {} resumed", name(v)))
        }
    }
}

/// The commands under an event for a terminal that cannot answer by key
/// (`tasks --watch`, line mode): a question's answers, a failure's retry and
/// dismissal.
pub fn event_commands(e: &Event, room: usize) -> Option<String> {
    match e {
        Event::Asked(_, q) => Some(answer_commands(q, room)),
        Event::Failed(v) => failed_commands(v),
        _ => None,
    }
}

#[cfg(test)]
#[path = "task_tree_tests.rs"]
mod tests;
