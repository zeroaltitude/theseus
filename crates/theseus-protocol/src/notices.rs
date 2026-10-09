//! The notification policy (theseus-753z): when a piece of work's change is
//! worth telling the owner, how urgently, and in what words. One pure
//! function, `policy()`, for every surface, so the TUI, herdr, Discord, and
//! the cockpit ping for the same reasons, and an answer clears the ping
//! everywhere (`retracts`). `deliver()` says how one notice reaches one
//! viewer, and `Burst` folds a root's burst into one ping. No clock: the
//! caller passes the times, and a time of day is written by its `hm`, as
//! `attention()`'s is.
//!
//! (The method-name table owns the module name `notify`, so this one is
//! `notices`.)

use serde::{Deserialize, Serialize};

use crate::usd;
use crate::work::{DoingKind, Question, QuestionKind, WorkKind, WorkRef, WorkState, WorkView};

mod burst;
pub use burst::Burst;

/// How urgently a notice asks for the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    /// Nothing to ping: the record, and a retraction.
    Quiet,
    /// A badge, the digest.
    Inform,
    /// A ping.
    Interrupt,
}

/// Why a notice was given: the policy's rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Why {
    Asked,
    Reminder,
    Answered,
    Expired,
    Failed,
    Blocked,
    Finished,
    Reported,
    SpendCrossed,
    Woke,
}

/// One notice: what changed, for whom, and its one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "WorkNotice"))]
pub struct Notice {
    pub urgency: Urgency,
    pub why: Why,
    pub work: WorkRef,
    pub root: WorkRef,
    /// `● Beta summary asks: fs.write create notes.txt (40 B)? · expires 18:36`.
    pub line: String,
    /// The question it is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    /// The question whose ping this clears, on every surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retracts: Option<String>,
    pub position: u64,
    pub at_ms: u64,
}

/// Which roots' finishing interrupts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Finished {
    /// Only a root the owner flagged.
    Flagged,
    /// A flagged root, or one that worked `long_run_ms` or more.
    #[default]
    LongRoots,
    /// Every root.
    Everything,
}

/// The policy's numbers, as the owner's thread decided them: data, so a
/// config section can tune them later without code.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct Rules {
    /// A root that worked this long interrupts when it is done: 5 minutes.
    pub long_run_ms: u64,
    /// A question is reminded of once, at this share of its life: half.
    pub remind_at: f32,
    pub finished: Finished,
    /// At most one Interrupt per root in this window: 10 s.
    pub burst_ms: u64,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            long_run_ms: 300_000,
            remind_at: 0.5,
            finished: Finished::LongRoots,
            burst_ms: 10_000,
        }
    }
}

/// Who a notice would reach, as the surface knows them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Viewer {
    /// The work or session on the viewer's screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub focused: Option<String>,
    /// The position of the work's view the viewer last saw; 0 for none.
    pub seen: u64,
    /// How long the viewer has been away (the terminal lost focus); 0 while
    /// present. Focus counts only while present.
    pub away_ms: u64,
    /// The viewer asked for no pings (`--notify off`).
    pub quiet: bool,
    /// A ping may make a sound.
    pub sound: bool,
}

/// How a notice reaches a viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "as", rename_all = "snake_case")]
pub enum Delivery {
    Ping { sound: bool },
    Badge,
    Nothing,
}

/// How `n` reaches `v`: nothing for the work in focus while the viewer is
/// present, for what they have seen, or for a quiet notice; a badge for an
/// Inform; a ping for an Interrupt, with sound unless the viewer is quiet or
/// it is a finish (rule 5 interrupts without sound).
pub fn deliver(n: &Notice, v: &Viewer) -> Delivery {
    if n.urgency == Urgency::Quiet {
        return Delivery::Nothing;
    }
    let in_focus = v
        .focused
        .as_deref()
        .is_some_and(|f| f == n.work.id || n.work.session_id.as_deref() == Some(f));
    if v.away_ms == 0 && in_focus {
        return Delivery::Nothing;
    }
    if v.seen > 0 && n.position <= v.seen {
        return Delivery::Nothing;
    }
    match n.urgency {
        Urgency::Interrupt => Delivery::Ping {
            sound: v.sound && !v.quiet && n.why != Why::Finished,
        },
        _ => Delivery::Badge,
    }
}

/// How people name a piece of work: its title, or its kind and the end of
/// its id (`task a1b2c3`).
pub fn name_of(r: &WorkRef) -> String {
    if !r.title.is_empty() {
        return r.title.clone();
    }
    let n = r.id.chars().count();
    let tail: String = r.id.chars().skip(n.saturating_sub(6)).collect();
    format!("{} {tail}", r.kind.as_str())
}

/// A span as people say it: `40 s`, `14 min`, `2 h 5 min`.
fn span(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ if s % 3600 / 60 == 0 => format!("{} h", s / 3600),
        _ => format!("{} h {} min", s / 3600, s % 3600 / 60),
    }
}

/// The open question a view holds: none once it ended.
fn open_question(v: &WorkView) -> Option<&Question> {
    v.question.as_ref().filter(|_| !v.state.ended())
}

/// Who asks a question: its asker, by name.
fn asker(v: &WorkView, q: &Question) -> String {
    if v.kind == WorkKind::Question {
        name_of(&q.asked_by)
    } else {
        name_of(&v.work_ref())
    }
}

/// When a question's reminder is due: `remind_at` of its life after it was
/// asked. None for a question that never expires.
pub fn remind_due(q: &Question, since_ms: u64, rules: &Rules) -> Option<u64> {
    let expires = q.expires_at_ms.filter(|e| *e > 0)?;
    let asked = if q.asked_at_ms > 0 {
        q.asked_at_ms
    } else {
        since_ms
    };
    let life = expires.saturating_sub(asked);
    Some(asked + (life as f64 * f64::from(rules.remind_at)) as u64)
}

/// The view's reminder time, if its open question has one still ahead.
pub fn next_reminder(v: &WorkView, rules: &Rules) -> Option<u64> {
    let q = open_question(v)?;
    let at = remind_due(q, v.state_since_ms, rules)?;
    (at > v.at_ms).then_some(at)
}

/// The notification policy (the owner's DM thread's table). First match
/// wins:
///
/// | # | Transition | Urgency |
/// |---|---|---|
/// | 1 | a question appears, of any kind, at any depth | Interrupt |
/// | 2 | a question reaches `remind_at` of its life unanswered (once) | Interrupt |
/// | 3 | it fails or blocks (needs you, no question) | Interrupt |
/// | 4 | a wake fires: on a root conversation (the owner's reminders) | Interrupt; a task's own: Inform |
/// | 5 | a root the owner flagged, or one that worked `long_run_ms`, goes to ready or idle | Interrupt, no sound |
/// | 6 | a question is answered, anywhere | Quiet, and `retracts` its ping |
/// | 7 | a question expires unanswered | Inform, and `retracts` |
/// | 8 | a task reports; a job ends; a short turn ends | Inform |
/// | 9 | spend crosses its limit or a multiple of it | Inform |
/// | 10 | working ↔ working, queued, spend only, cancelled | nothing |
///
/// A reminder is a view against itself later: pass the last view as `prev`
/// and the same view at the time it is due as `next` (`next_reminder` says
/// when). Each rule fires on its transition, once, never again for the same
/// state. `hm` writes a time of day.
pub fn policy(
    prev: Option<&WorkView>,
    next: &WorkView,
    rules: &Rules,
    hm: &dyn Fn(u64) -> String,
) -> Option<Notice> {
    let c = Change::new(prev, next);
    let said = asked(&c, hm)
        .or_else(|| reminder(&c, rules))
        .or_else(|| failed(&c))
        .or_else(|| woke(&c))
        .or_else(|| finished(&c, rules))
        .or_else(|| answered(&c))
        .or_else(|| ended(&c))
        .or_else(|| spend(&c))?;
    Some(Notice {
        urgency: said.urgency,
        why: said.why,
        work: next.work_ref(),
        root: root_ref(next),
        line: said.line,
        question: said.question,
        retracts: c.gone.map(|q| q.id.clone()),
        position: next.position,
        at_ms: next.at_ms,
    })
}

/// A transition, as the rules read it.
struct Change<'a> {
    prev: Option<&'a WorkView>,
    next: &'a WorkView,
    /// The question open before, and now.
    prev_q: Option<&'a Question>,
    next_q: Option<&'a Question>,
    /// The question open before and not now: answered, or expired.
    gone: Option<&'a Question>,
    name: String,
}

impl<'a> Change<'a> {
    fn new(prev: Option<&'a WorkView>, next: &'a WorkView) -> Self {
        let prev_q = prev.and_then(open_question);
        let next_q = open_question(next);
        Self {
            prev,
            next,
            prev_q,
            next_q,
            gone: prev_q.filter(|p| next_q.is_none_or(|q| q.id != p.id)),
            name: name_of(&next.work_ref()),
        }
    }
}

/// What a rule says: how urgently, why, its line, and its question.
struct Said {
    urgency: Urgency,
    why: Why,
    line: String,
    question: Option<String>,
}

fn said(urgency: Urgency, why: Why, line: String, question: Option<&Question>) -> Option<Said> {
    Some(Said {
        urgency,
        why,
        line,
        question: question.map(|q| q.id.clone()),
    })
}

/// 1: a new question.
fn asked(c: &Change, hm: &dyn Fn(u64) -> String) -> Option<Said> {
    let q = c.next_q.filter(|q| c.prev_q.is_none_or(|p| p.id != q.id))?;
    said(
        Urgency::Interrupt,
        Why::Asked,
        asked_line(c.next, q, hm),
        Some(q),
    )
}

/// 2: the same question at its reminder.
fn reminder(c: &Change, rules: &Rules) -> Option<Said> {
    let (p, q) = (c.prev?, c.next_q?);
    let due = remind_due(q, c.next.state_since_ms, rules)?;
    let expires = q.expires_at_ms.unwrap_or(u64::MAX);
    if !(p.at_ms < due && due <= c.next.at_ms && c.next.at_ms < expires) {
        return None;
    }
    let left = (expires - c.next.at_ms).div_ceil(60_000);
    let line = format!(
        "● still waiting, {left} min left: {}",
        q.subject(&asker(c.next, q))
    );
    said(Urgency::Interrupt, Why::Reminder, line, Some(q))
}

/// 3: a failure or a block, on the transition into it. A job's failure is
/// its caller's to handle (rule 8); a question's wait is rule 1's.
fn failed(c: &Change) -> Option<Said> {
    let (next, was) = (c.next, c.prev.map(|p| p.state));
    let asking = next
        .doing
        .as_ref()
        .is_some_and(|d| d.what == DoingKind::Asking);
    let why = match next.state {
        WorkState::Failed if was != Some(WorkState::Failed) && next.kind != WorkKind::Job => {
            Why::Failed
        }
        WorkState::NeedsYou
            if c.next_q.is_none() && !asking && was != Some(WorkState::NeedsYou) =>
        {
            Why::Blocked
        }
        _ => return None,
    };
    let word = if why == Why::Failed {
        "failed"
    } else {
        "blocked"
    };
    let line = format!("✗ {} {}", c.name, label_or(next, word));
    said(Urgency::Interrupt, why, line, None)
}

/// 4: a wake fired: on a root conversation (where the owner's reminders
/// live) it interrupts; a task's own wake informs.
fn woke(c: &Change) -> Option<Said> {
    let next = c.next;
    let line = woke_line(c.prev, next, &c.name)?;
    let on_root = match next.kind {
        WorkKind::Wake => next.parent.as_deref() == Some(next.root.as_str()),
        _ => next.parent.is_none() && next.kind == WorkKind::Conversation,
    };
    let urgency = if on_root {
        Urgency::Interrupt
    } else {
        Urgency::Inform
    };
    said(urgency, Why::Woke, line, None)
}

/// 5, and 8's short turn: a root at rest after work. Long or flagged, it
/// interrupts (without sound, `deliver` says); short, it informs, unless
/// an answer is what moved it (rule 6).
fn finished(c: &Change, rules: &Rules) -> Option<Said> {
    let (p, next) = (c.prev?, c.next);
    let rested = matches!(next.state, WorkState::Ready | WorkState::Done);
    let root =
        next.parent.is_none() && matches!(next.kind, WorkKind::Conversation | WorkKind::Task);
    if p.attention.level != crate::Level::Working || !rested || !root {
        return None;
    }
    let ran = next.at_ms.saturating_sub(p.attention.since_ms);
    let flagged = next.flagged || p.flagged;
    let loud = match rules.finished {
        Finished::Flagged => flagged,
        Finished::LongRoots => flagged || ran >= rules.long_run_ms,
        Finished::Everything => true,
    };
    let word = if next.state == WorkState::Ready {
        "ready"
    } else {
        "done"
    };
    if loud {
        let mut line = format!("✓ {} is {word} after {}", c.name, span(ran));
        if let Some(n) = next.below.map(|b| b.done).filter(|n| *n > 0) {
            let s = if n == 1 { "" } else { "s" };
            line.push_str(&format!(" · {n} task{s} reported"));
        }
        return said(Urgency::Interrupt, Why::Finished, line, None);
    }
    if c.gone.is_some() {
        return None;
    }
    let line = format!("✓ {} is {word}", c.name);
    said(Urgency::Inform, Why::Finished, line, None)
}

/// 6 and 7: a question answered, or expired.
fn answered(c: &Change) -> Option<Said> {
    let (q, next) = (c.gone?, c.next);
    let expired = next.state == WorkState::Expired
        || q.expires_at_ms.is_some_and(|e| e > 0 && next.at_ms >= e);
    let who = asker(c.prev.unwrap_or(next), q);
    if expired {
        let after = q.on_expiry.as_deref().unwrap_or("not run");
        let line = format!("⌛ {} expired, {after}", q.subject(&who));
        return said(Urgency::Inform, Why::Expired, line, Some(q));
    }
    let report = next
        .report
        .as_ref()
        .filter(|_| next.kind == WorkKind::Question);
    let line = match report {
        Some(r) => match r.reason.as_deref() {
            Some(by) => format!("✓ {} {by}: {who}", r.outcome),
            None => format!("✓ {}: {who}", r.outcome),
        },
        None => format!("✓ answered: {}", q.subject(&who)),
    };
    said(Urgency::Quiet, Why::Answered, line, Some(q))
}

/// 9: spend past its limit, or a multiple of it.
fn spend(c: &Change) -> Option<Said> {
    let (p, now) = (c.prev?.cost?, c.next.cost?);
    if now.limit_usd <= 0.0 {
        return None;
    }
    let times = |spent: f64| (spent / now.limit_usd).floor() as u64;
    let k = times(now.spent_usd);
    if k == 0 || k <= times(p.spent_usd) {
        return None;
    }
    let line = if k == 1 {
        format!("$ {} passed its {} limit", c.name, usd(now.limit_usd))
    } else {
        format!(
            "$ {} passed {}, {k}× its {} limit",
            c.name,
            usd(now.limit_usd * k as f64),
            usd(now.limit_usd)
        )
    };
    said(Urgency::Inform, Why::SpendCrossed, line, None)
}

/// The root a view is under, as a reference.
fn root_ref(v: &WorkView) -> WorkRef {
    if v.root == v.id {
        return v.work_ref();
    }
    WorkRef {
        id: v.root.clone(),
        kind: if v.root == "system" {
            WorkKind::System
        } else {
            WorkKind::Conversation
        },
        session_id: None,
        title: String::new(),
    }
}

/// Its attention's label, or `word` when it has none.
fn label_or(v: &WorkView, word: &str) -> String {
    if v.attention.label.is_empty() {
        word.to_string()
    } else {
        v.attention.label.clone()
    }
}

/// `● Beta summary asks: fs.write create notes.txt (40 B)? · expires 18:36`.
fn asked_line(v: &WorkView, q: &Question, hm: &dyn Fn(u64) -> String) -> String {
    let who = asker(v, q);
    let mut line = match q.kind {
        QuestionKind::Budget => match v.cost {
            Some(c) => format!(
                "● {who} reached its {} limit: reset its spend? ({} spent)",
                usd(c.limit_usd),
                usd(c.spent_usd)
            ),
            None => format!("● {who} reached its spend limit: reset it?"),
        },
        _ => format!("● {who} asks: {}?", q.prompt.trim_end_matches('?')),
    };
    if let Some(e) = q.expires_at_ms.filter(|e| *e > 0) {
        line.push_str(&format!(" · expires {}", hm(e)));
    }
    line
}

/// Rule 4's line, when a wake fired between the two views: the view says
/// the wake whose turn ended (`woke`), or, from today's push, it was queued
/// or runs for a wake it was not before (`why`), or a wake itself fired.
fn woke_line(prev: Option<&WorkView>, next: &WorkView, name: &str) -> Option<String> {
    if let Some(w) = next.woke.as_ref() {
        if prev.and_then(|p| p.woke.as_ref()) != Some(w) {
            return Some(format!("⏰ {name} · {}", w.note));
        }
    }
    if next.kind == WorkKind::Wake {
        let fired =
            next.state == WorkState::Done && prev.is_none_or(|p| p.state != WorkState::Done);
        return fired.then(|| format!("⏰ {name}"));
    }
    let for_wake = |v: &WorkView| v.why.as_deref() == Some("wake") && v.state.working();
    (for_wake(next) && !prev.is_some_and(for_wake)).then(|| format!("⏰ {name} · its wake fired"))
}

/// 8: a task's report, or a job's end.
fn ended(c: &Change) -> Option<Said> {
    let (p, next, name) = (c.prev?, c.next, &c.name);
    if next.kind == WorkKind::Task && next.state.ended() {
        if let Some(r) = next
            .report
            .as_ref()
            .filter(|r| p.report.as_ref() != Some(*r))
        {
            let words = r
                .first_line
                .as_deref()
                .or(r.reason.as_deref())
                .unwrap_or(&r.outcome);
            let line = format!("📋 {name} reported: {words}");
            return said(Urgency::Inform, Why::Reported, line, None);
        }
        if !p.state.ended() && next.state == WorkState::Done {
            let line = format!("📋 {name} reported");
            return said(Urgency::Inform, Why::Reported, line, None);
        }
    }
    if next.kind == WorkKind::Job && next.state.ended() && !p.state.ended() {
        let line = match next.state {
            WorkState::Failed => format!("✗ {name} failed"),
            WorkState::Cancelled => return None,
            _ => format!("✓ {name} ended"),
        };
        return said(Urgency::Inform, Why::Finished, line, None);
    }
    None
}

#[cfg(test)]
#[path = "tests_notices.rs"]
mod tests;
