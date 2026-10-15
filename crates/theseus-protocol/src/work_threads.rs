//! Threads on the work board (theseus-5i8i): the pure grouping every surface
//! shares. The board holds views, each stamped with the thread its tree
//! belongs to (`WorkView.thread`), and the thread groups beside them
//! (`ThreadGroup`); a surface calls `group` to get each live thread with its
//! open tasks, questions and wakes under it, and the work that is in no
//! thread where the board shows it today. Types and pure functions only, like
//! `work.rs`: no clock (the caller passes `now_ms`), no I/O.
//!
//! The counting rules, written once:
//! - A thread's **needs you** is its open items at the needs-you level, its
//!   conversation's own wait when no question view of it is open, and a seal
//!   recommended and not yet answered (one).
//! - A **paused** thread shows its items and its count, and leaves the
//!   board's needs-you count (`Grouped::needs_you`): the owner put it down.
//! - A thread that is **done** is shown only while something under it is open.
//! - **Warm** is a session whose last activity is within `WARM_MS`, the
//!   provider cache's lifetime: a message to a cold thread rewrites its
//!   context, and costs by its size.

use crate::push::usd;
use crate::threads::ThreadState;
use crate::work::{WorkKind, WorkView};
use crate::{Level, ThreadGroup};

/// How long after its last activity a thread's cache is taken to be warm
/// (the provider's five-minute cache).
pub const WARM_MS: u64 = 5 * 60 * 1000;

/// One thread on the board.
#[derive(Debug, Clone)]
pub struct GroupedThread<'a> {
    pub group: &'a ThreadGroup,
    /// The thread's own conversation, when the board holds it.
    pub head: Option<&'a WorkView>,
    /// Its open tasks, questions and wakes: needs you first, the longest
    /// waiting first within a level.
    pub items: Vec<&'a WorkView>,
    /// What waits on the owner here, a seal recommendation included.
    pub needs_you: u32,
    /// Its items that are under way.
    pub working: u32,
    /// The later of the record's last activity and the conversation's.
    pub last_active_ms: u64,
}

/// The board by thread.
#[derive(Debug, Clone)]
pub struct Grouped<'a> {
    /// Needs you first, then the most recently active.
    pub threads: Vec<GroupedThread<'a>>,
    /// Work that is in no thread, in the board's order.
    pub loose: Vec<&'a WorkView>,
}

/// Whether a view is one a thread's group lists: an open task, question or
/// wake, or one that failed and has not been looked at.
fn listed(v: &WorkView) -> bool {
    matches!(v.kind, WorkKind::Task | WorkKind::Question | WorkKind::Wake)
        && (!v.state.ended() || v.attention.level == Level::NeedsYou)
}

/// How many of `views` wait on the owner: each view at the needs-you level,
/// but a conversation or a task that waits on a question counts through the
/// question's own view when the list holds it.
pub fn needs_count(views: &[&WorkView]) -> u32 {
    views
        .iter()
        .filter(|v| v.attention.level == Level::NeedsYou)
        .filter(|v| {
            let asking =
                v.question.is_some() && matches!(v.kind, WorkKind::Conversation | WorkKind::Task);
            !(asking
                && views
                    .iter()
                    .any(|q| q.kind == WorkKind::Question && q.parent.as_deref() == Some(&v.id)))
        })
        .count() as u32
}

/// The board by thread. `views` are the board's views in its order (a flat
/// list, roots then their subtrees), `groups` its thread groups.
pub fn group<'a>(views: &'a [WorkView], groups: &'a [ThreadGroup]) -> Grouped<'a> {
    let mut threads: Vec<GroupedThread<'a>> = Vec::new();
    for g in groups {
        let mine: Vec<&WorkView> = views
            .iter()
            .filter(|v| v.thread.as_deref() == Some(g.slug.as_str()))
            .collect();
        let head = mine
            .iter()
            .copied()
            .find(|v| v.kind == WorkKind::Conversation && v.parent.is_none());
        let mut items: Vec<&WorkView> = mine.iter().copied().filter(|v| listed(v)).collect();
        items.sort_by_key(|v| (v.attention.level.rank(), v.state_since_ms, v.id.as_str()));
        if g.state == ThreadState::Done && items.is_empty() {
            continue;
        }
        let mut counted = items.clone();
        counted.extend(head);
        let needs_you = needs_count(&counted) + u32::from(g.seal_pending.is_some());
        let working = items
            .iter()
            .filter(|v| v.attention.level == Level::Working)
            .count() as u32;
        threads.push(GroupedThread {
            group: g,
            head,
            items,
            needs_you,
            working,
            last_active_ms: g.last_active_ms.max(head.map_or(0, |h| h.at_ms)),
        });
    }
    threads.sort_by(|a, b| {
        (
            a.needs_you == 0,
            std::cmp::Reverse(a.last_active_ms),
            &a.group.slug,
        )
            .cmp(&(
                b.needs_you == 0,
                std::cmp::Reverse(b.last_active_ms),
                &b.group.slug,
            ))
    });
    let known: std::collections::HashSet<&str> = groups.iter().map(|g| g.slug.as_str()).collect();
    let loose = views
        .iter()
        .filter(|v| v.thread.as_deref().is_none_or(|t| !known.contains(t)))
        .collect();
    Grouped { threads, loose }
}

impl Grouped<'_> {
    /// What waits on the owner on the whole board: the work in no thread, and
    /// each thread that is not paused.
    pub fn needs_you(&self) -> u32 {
        needs_count(&self.loose)
            + self
                .threads
                .iter()
                .map(GroupedThread::board_needs)
                .sum::<u32>()
    }
}

impl GroupedThread<'_> {
    /// What this thread adds to the board's needs-you count: nothing while
    /// it is paused.
    pub fn board_needs(&self) -> u32 {
        match self.group.state {
            ThreadState::Paused => 0,
            _ => self.needs_you,
        }
    }

    /// Whether the thread's cache is warm at `now_ms`.
    pub fn warm(&self, now_ms: u64) -> bool {
        self.last_active_ms > 0 && now_ms.saturating_sub(self.last_active_ms) <= WARM_MS
    }

    /// The thread's title, with a lock when it is sealed.
    pub fn title(&self) -> String {
        match self.group.sealed {
            true => format!("🔒 {}", self.group.title),
            false => self.group.title.clone(),
        }
    }

    /// The header line at `width` characters: the title, then as much of
    /// state, last activity, warm or cold, context size, today's dollars and
    /// persona as fits, in that order. The title is cut with `…` rather than
    /// dropped.
    pub fn header(&self, now_ms: u64, width: usize) -> String {
        let g = self.group;
        let mut parts: Vec<String> = vec![g.state.as_str().to_string()];
        parts.push(match self.last_active_ms {
            0 => "no messages yet".to_string(),
            at => format!("{} ago", span(now_ms.saturating_sub(at))),
        });
        if self.last_active_ms > 0 {
            parts.push(if self.warm(now_ms) { "warm" } else { "cold" }.to_string());
        }
        if let Some(t) = g.context_tokens {
            parts.push(format!("{} ctx", tokens(t)));
        }
        if let Some(u) = g.today_usd.filter(|u| *u > 0.0) {
            parts.push(format!("{} today", usd(u)));
        }
        if let Some(p) = &g.persona {
            parts.push(p.clone());
        }
        let title = self.title();
        let mut out = clip(&title, width);
        for p in parts {
            let next = format!("{out} · {p}");
            if next.chars().count() > width {
                break;
            }
            out = next;
        }
        out
    }
}

/// `text` cut to `width` characters, ending `…` when cut.
fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let cut: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// A span in its largest unit: `40s`, `3m`, `2h`, `5d`.
fn span(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// Tokens as people say them: `900`, `41k`, `1.2M`.
fn tokens(t: u64) -> String {
    match t {
        0..=999 => t.to_string(),
        1_000..=999_999 => format!("{}k", (t + 500) / 1000),
        _ => format!("{:.1}M", t as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
#[path = "tests_work_threads.rs"]
mod tests;
