//! The commitments block (theseus-lherl; the horizon row HZ1): what this
//! conversation promised, in the model's own request.
//!
//! The model had one scheduling tool, `wake.at`, and learned its own wakes
//! only from its transcript. After a compaction the "Set wake" results may be
//! gone, and asked "make that 4 pm" it could only set a second wake. So the
//! turn's pending wakes ride in the request's tail beside the task graph's
//! view (`task_graph::view`): the same last message, after the cached prefix,
//! with no breakpoint of its own (the view's `attach_to` moves the
//! conversation's one). Recompiled every loop, so it survives every
//! compaction, and nothing at all while none is pending.
//!
//! - **One line each**, at most `MAX_LINES` and `MAX_TOKENS` in all (past
//!   either, `and N more`): `⏰ a8cebb Fri 09:00 check PR 123`, and a series
//!   `🔁 a41d04 every 1d 21:00 (next Fri) evening haiku`. The line's first
//!   word is its icon, so a later source of commitments (a routine, a watch,
//!   a deferred task) joins as another line with its own icon.
//! - **No read of its own.** The list is the one `take_wakes_listing` read at
//!   the turn's start, kept in the turn's `Pending`, and replaced by what each
//!   `wake.at`, `wake.cancel` and `wake.move` frame of this turn returns. A
//!   cancel from outside, mid-turn, shows at the next turn.
//! - **The tools** are `wake_tools.rs`'s: `wake.cancel` and `wake.move`.

use std::sync::Mutex;

use theseus_kernel::{PendingWake, Purpose};

use crate::wake::{local, Local};

/// How the block begins: a reader of a request (a test's stand-in model) can
/// tell it from the conversation's, as it can the task graph's view
/// (`task_graph::view::is_head`).
pub const HEAD: &str = "[Your pending wakes in this conversation";
/// The most lines it shows, and its bound in tokens (estimated as the view's
/// are, `task_graph::view::BYTES_PER_TOKEN` to a token).
pub const MAX_LINES: usize = 10;
pub const MAX_TOKENS: u64 = 300;
/// A line's note, in characters, at the longest.
const TITLE_CHARS: usize = 50;

/// The turn's pending wakes, as the last read or the last wake tool's frame
/// left them. Held by the turn's frames and lent to its tools through
/// `TurnCtx`.
#[derive(Default)]
pub struct Pending(
    Mutex<Vec<PendingWake>>,
    /// What this turn scheduled: wakes, watches and tasks it set, which keep
    /// a promise to follow up (`initiative::promise`).
    std::sync::atomic::AtomicU32,
);

impl Pending {
    /// The turn scheduled something that will wake it later.
    pub fn scheduled_one(&self) {
        self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// How many things the turn scheduled so far.
    pub fn scheduled(&self) -> u32 {
        self.1.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set(&self, wakes: Vec<PendingWake>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = wakes;
    }

    pub fn get(&self) -> Vec<PendingWake> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// What the block showed, for the turn's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    /// The lines shown, and the wakes left out past the bounds.
    pub lines: u32,
    pub left_out: u32,
    pub tokens: u64,
}

fn tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `Oct 23`.
fn month_day(l: &Local) -> String {
    format!("{} {}", MONTHS[(l.month.clamp(1, 12) - 1) as usize], l.day)
}

/// `Fri 09:00`, and with its date `Fri Oct 23 09:00` a week or more from
/// now: a time of day alone would not say which day it means.
fn when(due: &Local, due_ms: u64, now_ms: u64) -> String {
    if due_ms.saturating_sub(now_ms) >= 6 * 86_400_000 {
        format!("{} {} {}", due.weekday(), month_day(due), due.hm())
    } else {
        format!("{} {}", due.weekday(), due.hm())
    }
}

/// One wake's line. `shown` is what a watch's line says of its value.
fn line(
    w: &PendingWake,
    now_ms: u64,
    at: &dyn Fn(u64) -> Local,
    shown: &dyn Fn(&PendingWake) -> Option<String>,
) -> String {
    let due = at(w.due_at_ms);
    let short = crate::task::short(&w.id);
    // A watch says what it last saw: `👁 c3d4e5 PR 123 merged? (OPEN at 14:05)`.
    if w.probe().is_some() {
        let title: String = crate::session::title_from(&w.note)
            .chars()
            .take(TITLE_CHARS)
            .collect();
        let seen = shown(w).map_or(String::new(), |v| format!(" ({v})"));
        return format!("👁 {short} {title}{seen}");
    }
    let title: String = crate::session::title_from(&w.note)
        .chars()
        .take(TITLE_CHARS)
        .collect();
    // A reminder, a routine or a watch says what it is; a held one says so.
    let title = match w.purpose {
        Purpose::Check => title,
        p => format!("{}: {title}", p.as_str()),
    };
    let paused = w.paused_at_ms.is_some();
    match &w.repeat {
        None if paused => format!(
            "⏸ {short} {} (paused) {title}",
            when(&due, w.due_at_ms, now_ms)
        ),
        None => format!("⏰ {short} {} {title}", when(&due, w.due_at_ms, now_ms)),
        Some(r) => {
            let mut rule = format!("every {}", r.every);
            if !r.days.is_empty() {
                let days: Vec<&str> = r.days.iter().map(|d| d.as_str()).collect();
                rule.push_str(&format!(" on {}", days.join(",")));
            }
            let until = r.until_ms.map_or(String::new(), |u| {
                let u = at(u);
                format!(" until {} {}", month_day(&u), u.hm())
            });
            let next = if paused {
                "paused".to_string()
            } else {
                format!("next {}", due.weekday())
            };
            let icon = if paused { "⏸" } else { "🔁" };
            format!("{icon} {short} {rule} {}{until} ({next}) {title}", due.hm())
        }
    }
}

/// The block's text for `wakes` (soonest first) at `now_ms`, in the zone
/// `at` reads; none when there are none. Pure: the clock and the zone are
/// passed in.
pub fn render(wakes: &[PendingWake], now_ms: u64, at: &dyn Fn(u64) -> Local) -> Option<Shown> {
    render_with(wakes, &[], now_ms, at, &|_| None)
}

/// `render`, with the routines this conversation made after the wakes
/// (theseus-pvm5c; `routine::lines`: `🔁 a41d04 weekdays 08:00 Morning digest
/// (next Fri)`), and what a watch's line says of its value (`shown`,
/// theseus-f2r2b), under the same bounds.
pub fn render_with(
    wakes: &[PendingWake],
    routines: &[String],
    now_ms: u64,
    at: &dyn Fn(u64) -> Local,
    shown: &dyn Fn(&PendingWake) -> Option<String>,
) -> Option<Shown> {
    let all: Vec<String> = wakes
        .iter()
        .map(|w| line(w, now_ms, at, shown))
        .chain(routines.iter().cloned())
        .collect();
    if all.is_empty() {
        return None;
    }
    let head = format!(
        "{HEAD}, {}: to change one, move it with wake_move or end it with wake_cancel, \
         never set a second.]",
        all.len()
    );
    // A closing line costs a few tokens: keep room for it.
    let more = |n: usize| format!("(and {n} more: wake.list, or `theseus wakes`, shows them all.)");
    let mut lines: Vec<String> = Vec::new();
    let mut used = tokens(&head);
    for (i, l) in all.iter().enumerate() {
        let rest = all.len() - i - 1;
        let reserve = if rest > 0 { tokens(&more(rest)) + 1 } else { 0 };
        if lines.len() >= MAX_LINES || used + tokens(l) + 1 + reserve > MAX_TOKENS {
            break;
        }
        used += tokens(l) + 1;
        lines.push(l.clone());
    }
    let left_out = all.len() - lines.len();
    let mut text = head;
    for l in &lines {
        text.push('\n');
        text.push_str(l);
    }
    if left_out > 0 {
        text.push('\n');
        text.push_str(&more(left_out));
    }
    Some(Shown {
        tokens: tokens(&text),
        lines: lines.len() as u32,
        left_out: left_out as u32,
        text,
    })
}

/// The turn's side (`compile_step`): the pending wakes' block, attached to
/// the compiled request after the task view, its tokens counted in the
/// estimate. Returns what it showed; None, and the request untouched, when
/// none is pending.
pub fn attach(
    pending: &Pending,
    routines: &[String],
    now_ms: u64,
    compiled: &mut crate::compiler::Compiled,
    watch_value: &dyn Fn(&PendingWake) -> Option<String>,
) -> Option<Shown> {
    let shown = render_with(&pending.get(), routines, now_ms, &local, watch_value)?;
    crate::task_graph::view::attach_text(&mut compiled.request, &shown.text);
    let t = shown.tokens;
    compiled.est_tokens += t;
    compiled.estimate.tokens += t;
    compiled.estimate.estimated += t;
    compiled.estimate.upper += t + t * crate::compiler::MARGIN_PERCENT / 100;
    Some(shown)
}
