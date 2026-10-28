//! The notification policy (theseus-753z): when a piece of work's change is
//! worth telling the owner, how urgently, and in what words. One pure
//! function, `policy()`, shared by the surfaces that read it, the TUI and
//! herdr today, so they ping for the same reasons, and an answer clears the
//! ping in each (`retracts`). `deliver()` says how one notice reaches one
//! viewer, and `Burst` folds a root's burst into one ping. No clock: the
//! caller passes the times, and a time of day is written by its `hm`, as
//! `attention()`'s is.
//!
//! Discord's chat does not read it: whether a chat message buzzes is the
//! binding's own table (theseus-discord's `policy.rs`, theseus-l1y1: the
//! owner's lively chat, every message pinging unless its place silences its
//! category), and a card or a failure posts there as the core's outbox
//! writes it, not as this policy's urgency says.
//!
//! (The method-name table owns the module name `notify`, so this one is
//! `notices`.)

use serde::{Deserialize, Serialize};

use crate::initiative::{Initiative, Proactive};
use crate::usd;
use crate::wakes::{reminder_answers, WakeAnswer, WakeNoticed};
use crate::work::{DoingKind, Question, QuestionKind, WorkKind, WorkRef, WorkState, WorkView};
use crate::work_digest::agent_of;

mod burst;
pub use burst::Burst;

/// The ways a ping may reach the owner from a terminal client, as `--notify`
/// and the config's `[tui] notify` name them (theseus-f0bi9). The core checks
/// the config against it and the clients' `Delivery::parse` accepts each.
pub const NOTIFY_KINDS: [&str; 6] = ["auto", "toast", "bell", "osc9", "osc777", "off"];

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
    /// A task passed the 90th percentile of its forecast (rule 11).
    PastForecast,
    /// A project's milestone is past its date and still open
    /// (theseus-n5ui2): an Interrupt once, without sound.
    Overdue,
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
    /// What the owner may answer it with: a reminder's Done and its three
    /// snoozes, each a direct call (`wake.done`, `wake.snooze`) until
    /// `work.answer` routes every answer. None for the rest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<WakeAnswer>,
    /// For a question an agent answered (theseus-5rvss): the agent, as the
    /// digest names it (`claude`, `no terminal`). None is the owner's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub by: Option<String>,
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
    /// The life a held question is reminded by, as it has no deadline of its
    /// own (theseus-lrc3m): `[kernel] confirm_ttl_secs`'s default, 15 minutes.
    pub held_life_ms: u64,
    pub finished: Finished,
    /// At most one Interrupt per root in this window: 10 s.
    pub burst_ms: u64,
    /// How readily the persona in play speaks first (theseus-slvl.9): at
    /// `quiet` a finished root and a follow-up the model promised inform
    /// instead of interrupting; `normal` is the table as written.
    pub initiative: Initiative,
}

impl Rules {
    /// `urgency` as the persona's initiative level shapes it for a message
    /// of kind `p` (theseus-slvl.9; the one place the level reads): an
    /// Interrupt the level does not let becomes an Inform, a badge and the
    /// digest, and a chatty persona's promised follow-up, which only
    /// informed, interrupts.
    pub fn shaped(&self, urgency: Urgency, p: Proactive) -> Urgency {
        match urgency {
            Urgency::Interrupt if !self.initiative.lets(p) => Urgency::Inform,
            Urgency::Inform
                if p == Proactive::Followup && self.initiative == Initiative::Chatty =>
            {
                Urgency::Interrupt
            }
            u => u,
        }
    }
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            long_run_ms: 300_000,
            remind_at: 0.5,
            held_life_ms: 15 * 60_000,
            finished: Finished::LongRoots,
            burst_ms: 10_000,
            initiative: Initiative::Normal,
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
/// present, for what they have seen (but a reminder), or for a quiet
/// notice; a badge for an Inform; a ping for an Interrupt, with sound unless
/// the viewer is quiet or it is a finish (rule 5 interrupts without sound).
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
    // A reminder is for a question seen and not answered, and carries the
    // position of the view that asked.
    if v.seen > 0 && n.position <= v.seen && n.why != Why::Reminder {
        return Delivery::Nothing;
    }
    match n.urgency {
        Urgency::Interrupt => Delivery::Ping {
            sound: v.sound && !v.quiet && !matches!(n.why, Why::Finished | Why::Overdue),
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
/// asked. A question held past its life has no deadline (theseus-v2dnc), so
/// its life is the one it would have had (`Rules::held_life_ms`); a budget
/// question holds without one and is not reminded of.
pub fn remind_due(q: &Question, since_ms: u64, rules: &Rules) -> Option<u64> {
    let asked = if q.asked_at_ms > 0 {
        q.asked_at_ms
    } else {
        since_ms
    };
    let life = match q.expires_at_ms.filter(|e| *e > 0) {
        Some(expires) => expires.saturating_sub(asked),
        None if q.kind != QuestionKind::Budget => rules.held_life_ms,
        None => return None,
    };
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
/// | 2 | a question reaches `remind_at` of its life unanswered (once); a held one, which has no deadline, of the life it would have had (`Rules::held_life_ms`) | Interrupt |
/// | 3 | it fails or blocks (needs you, no question) | Interrupt |
/// | 4 | a wake fires, by its purpose: a reminder | Interrupt, with its four answers (`reminder_notice`); a check-in, a routine: Inform; a wake that says no purpose: Interrupt on a root conversation, a task's own Inform. A watch's end (`wake.noticed` with purpose `watch.met`, `watch.expired` or `watch.failing`): met Interrupt, with sound; gave up and failing Inform |
/// | 5 | a root the owner flagged, or one that worked `long_run_ms`, goes to ready or idle | Interrupt, no sound |
/// | 6 | a question is answered, anywhere | Quiet, and `retracts` its ping |
/// | 7 | a question expires unanswered | Inform, and `retracts` |
/// | 8 | a task reports; a job ends; a short turn ends; a system job fails, or ends after `long_run_ms` | Inform |
/// | 8 | a task reports (Interrupt if the owner flagged it: "tell me when done"); a job ends; a short turn ends | Inform |
/// | 9 | spend crosses its limit or a multiple of it | Inform |
/// | 10 | working ↔ working, queued, spend only, cancelled | nothing |
/// | 11 | a task passes the p90 of its forecast, of dollars or of minutes, whichever comes first (once: the view's `forecast.past` turns true) | Inform, with Keep going and Stop |
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
        .or_else(|| woke(&c, rules))
        .or_else(|| finished(&c, rules))
        .or_else(|| answered(&c))
        .or_else(|| ended(&c, rules))
        .or_else(|| spend(&c))
        .or_else(|| past_forecast(&c))?;
    let answers = if said.why == Why::PastForecast {
        forecast_answers()
    } else {
        vec![]
    };
    Some(Notice {
        urgency: said.urgency,
        why: said.why,
        work: next.work_ref(),
        root: root_ref(next),
        line: said.line,
        question: said.question,
        retracts: c.gone.map(|q| q.id.clone()),
        answers,
        by: said.by,
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
    by: Option<String>,
}

fn said(urgency: Urgency, why: Why, line: String, question: Option<&Question>) -> Option<Said> {
    Some(Said {
        urgency,
        why,
        line,
        question: question.map(|q| q.id.clone()),
        by: None,
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
    let expires = q.expires_at_ms.filter(|e| *e > 0).unwrap_or(u64::MAX);
    if !(p.at_ms < due && due <= c.next.at_ms && c.next.at_ms < expires) {
        return None;
    }
    let subject = q.subject(&asker(c.next, q));
    // A held question has no deadline to count down to: it says how long it
    // has waited.
    let line = if expires == u64::MAX {
        let asked = if q.asked_at_ms > 0 {
            q.asked_at_ms
        } else {
            c.next.state_since_ms
        };
        format!(
            "● still waiting, asked {} ago: {subject}",
            span(c.next.at_ms.saturating_sub(asked))
        )
    } else {
        format!(
            "● still waiting, {} min left: {subject}",
            (expires - c.next.at_ms).div_ceil(60_000)
        )
    };
    said(Urgency::Interrupt, Why::Reminder, line, Some(q))
}

/// 3: a failure or a block, on the transition into it. A job's failure is
/// its caller's to handle (rule 8); a question's wait is rule 1's.
fn failed(c: &Change) -> Option<Said> {
    let (next, was) = (c.next, c.prev.map(|p| p.state));
    // Blocked: needs you with no question and none being asked. A question
    // answered into a block (a declined reset left budget-exhausted) is a
    // transition into it too, though the state stays needs you.
    let blocked = |v: &WorkView, q: Option<&Question>| {
        v.state == WorkState::NeedsYou
            && q.is_none()
            && !v
                .doing
                .as_ref()
                .is_some_and(|d| d.what == DoingKind::Asking)
    };
    let why = match next.state {
        // A job's failure is its caller's (rule 8), a system job's is an Inform
        // of its own (rule 8's system arm): neither interrupts.
        WorkState::Failed
            if was != Some(WorkState::Failed)
                && !matches!(next.kind, WorkKind::Job | WorkKind::System) =>
        {
            Why::Failed
        }
        WorkState::NeedsYou
            if blocked(next, c.next_q) && !c.prev.is_some_and(|p| blocked(p, c.prev_q)) =>
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

/// 4: a wake fired. By its purpose when the view says it (theseus-8ux6x): a
/// reminder interrupts wherever it lives; a check-in, a routine and a watch
/// inform. A view that says none is read as before: on a root conversation it
/// interrupts, and a task's own wake informs.
fn woke(c: &Change, rules: &Rules) -> Option<Said> {
    let next = c.next;
    let line = woke_line(c.prev, next, &c.name)?;
    let purpose = next.woke.as_ref().and_then(|w| w.purpose.as_deref());
    let urgency = match purpose {
        Some(p) => wake_urgency(p),
        None => {
            let on_root = match next.kind {
                WorkKind::Wake => next.parent.as_deref() == Some(next.root.as_str()),
                _ => next.parent.is_none() && next.kind == WorkKind::Conversation,
            };
            if on_root {
                Urgency::Interrupt
            } else {
                Urgency::Inform
            }
        }
    };
    let kind = match purpose {
        Some("remind" | "watch.met") => Proactive::Asked,
        _ => Proactive::Followup,
    };
    said(rules.shaped(urgency, kind), Why::Woke, line, None)
}

/// What the owner may answer a notice with, by the kind of work it is about
/// (theseus-nuhsz): a routine whose run failed or blocked (run now, pause,
/// details). A reminder's and a met watch's ride `wake.noticed`
/// (`reminder_notice`); everything else has none.
pub fn answers_for(v: &WorkView, why: Why) -> Vec<WakeAnswer> {
    match (v.kind, why) {
        (WorkKind::Routine, Why::Failed | Why::Blocked) => {
            crate::work::horizon::routine_failed_answers()
        }
        _ => vec![],
    }
}

/// Rule 4's ping by purpose: a reminder is for the owner, so it interrupts;
/// so does a watch that is met (`watch.met`, theseus-f2r2b: what the owner
/// waited for). The model's own check-ins, a routine's run, and a watch that
/// gave up (`watch.expired`) or keeps failing (`watch.failing`) only inform.
pub fn wake_urgency(purpose: &str) -> Urgency {
    match purpose {
        "remind" | "watch.met" => Urgency::Interrupt,
        p => routine_urgency(p).unwrap_or(Urgency::Inform),
    }
}

/// The ping rule for a routine's runs (theseus-52tut; the horizon report's
/// H-4 to H-7), one entry for each thing a run can come to. A notice about
/// one is a `wake.noticed` whose purpose is the entry's:
///
/// | Purpose | A run... | Urgency |
/// |---|---|---|
/// | `routine_reported` | reports (its report still speaks in the chat when `speak: always`) | Inform |
/// | `routine_failed` | fails | Interrupt, once per routine per local day, with the streak |
/// | `routine_blocked` | is blocked: a call outside its grant went unanswered | Interrupt, once per routine per local day |
/// | `routine_skipped` | is skipped (overlap, a spent month, no catch-up) | Inform |
/// | `routine_missed` | was missed while the daemon was down | Inform, once at the start |
pub const ROUTINE_PURPOSES: [&str; 5] = [
    "routine_reported",
    "routine_failed",
    "routine_blocked",
    "routine_skipped",
    "routine_missed",
];

/// How urgent a routine entry is; none for a purpose that is not one.
pub fn routine_urgency(purpose: &str) -> Option<Urgency> {
    match purpose {
        "routine_failed" | "routine_blocked" => Some(Urgency::Interrupt),
        p if ROUTINE_PURPOSES.contains(&p) => Some(Urgency::Inform),
        _ => None,
    }
}

/// Whether a failed or blocked run pings now: once per routine per local
/// day. `last_day` is the day of the last such Interrupt, `today` this one
/// (`2026-10-14`, as the caller's zone writes it); the first one ever pings.
pub fn routine_pings(last_day: Option<&str>, today: &str) -> bool {
    last_day.is_none_or(|d| d != today)
}

/// A failed or blocked routine's answers: Run now, Pause, Details, and for a
/// blocked one Add to grant first. Each names the call it makes with the
/// routine's id (`work.answer` routes them by the notice's id, the
/// routine's wake).
pub fn routine_answers(blocked: bool) -> Vec<WakeAnswer> {
    let a = |id: &str, label: &str, method: &str| WakeAnswer {
        id: id.into(),
        label: label.into(),
        method: method.into(),
        when: None,
    };
    let mut out = Vec::new();
    if blocked {
        out.push(a(
            "add_to_grant",
            "Add to grant",
            crate::method::ROUTINE_UPDATE,
        ));
    }
    out.push(a("run_now", "Run now", crate::method::ROUTINE_RUN_NOW));
    out.push(a("pause", "Pause", crate::method::ROUTINE_PAUSE));
    out.push(a("details", "Details", crate::method::ROUTINE_RUNS));
    out
}

/// The line of a routine's failure or block: `✗ Morning digest failed (2 in
/// a row): Gmail needs a login`, `✗ Morning digest blocked: gog.gmail.search
/// needs approval (not in its grant)`. The streak shows from the second.
pub fn routine_line(title: &str, blocked: bool, streak: u32, why: &str) -> String {
    let word = if blocked { "blocked" } else { "failed" };
    let row = if streak > 1 {
        format!(" ({streak} in a row)")
    } else {
        String::new()
    };
    let why = why.trim();
    if why.is_empty() {
        format!("✗ {title} {word}{row}")
    } else {
        format!("✗ {title} {word}{row}: {why}")
    }
}

/// The icon a wake notice's line begins with: a watch's eye, else the clock.
fn icon_of(purpose: &str) -> &'static str {
    if purpose == "watch" || purpose.starts_with("watch.") {
        "👁"
    } else {
        "⏰"
    }
}

/// How a project's milestone reminder begins (theseus-n5ui2): `⚑ due today:
/// Harbour survey, milestone 2`. The notice reads the flag as a milestone's,
/// and draws it without the clock icon a plain reminder takes.
pub const MILESTONE_MARK: &str = "⚑";
/// What follows the flag when the date has passed: `⚑ overdue: …`. That
/// notice is an Interrupt once, with no sound (`Why::Overdue`).
pub const OVERDUE_MARK: &str = "⚑ overdue";

/// The notice for a notice wake that fired (`wake.noticed`): the reminder's
/// Interrupt with its four answers, from the one event every surface reads,
/// so each says the same words and offers the same answers. A reminder that
/// came late says so. No frame position rides the event, so its time stands
/// for it (past any frame's), and a surface that saw the wake's conversation
/// is still told.
pub fn reminder_notice(n: &WakeNoticed, hm: &dyn Fn(u64) -> String) -> Notice {
    let title = n.note.lines().next().unwrap_or("").trim().to_string();
    let late = if n.late_ms > 5_000 {
        let down = if n.while_down {
            ", the daemon was down"
        } else {
            ""
        };
        format!(" · due {}, {} late{down}", hm(n.due_at_ms), span(n.late_ms))
    } else {
        String::new()
    };
    let remind = n.purpose == "remind";
    let milestone = remind && title.starts_with(MILESTONE_MARK);
    let why = if milestone && title.starts_with(OVERDUE_MARK) {
        Why::Overdue
    } else {
        Why::Woke
    };
    let routine = routine_urgency(&n.purpose).is_some();
    // A routine's entry says its own line (`routine_line`), not a clock; a milestone's has no clock either.
    let line = if routine {
        n.note.clone()
    } else if milestone {
        format!("{title}{late}")
    } else {
        format!("{} {title}{late}", icon_of(&n.purpose))
    };
    Notice {
        urgency: wake_urgency(&n.purpose),
        why,
        work: WorkRef {
            id: n.wake_id.clone(),
            kind: WorkKind::Wake,
            session_id: Some(n.session_id.clone()),
            title,
        },
        root: WorkRef {
            id: n.execution_id.clone(),
            kind: WorkKind::Conversation,
            session_id: Some(n.session_id.clone()),
            title: String::new(),
        },
        line,
        question: None,
        retracts: None,
        by: None,
        answers: if remind {
            reminder_answers()
        } else if routine {
            n.answers.clone()
        } else if n.purpose == "watch.met" {
            crate::work::horizon::watch_met_answers()
        } else {
            vec![]
        },
        position: n.due_at_ms.saturating_add(n.late_ms),
        at_ms: n.due_at_ms.saturating_add(n.late_ms),
    }
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
        let urgency = rules.shaped(Urgency::Interrupt, Proactive::Finished);
        return said(urgency, Why::Finished, line, None);
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
    // An agent's answer says so (theseus-5rvss): the settle's `by` is `by agent:<name>`.
    let agent = report
        .and_then(|r| r.reason.as_deref())
        .and_then(|by| agent_of(by.strip_prefix("by ")?));
    let line = match (report, agent) {
        (Some(r), Some(a)) => format!("✓ {} by an agent ({a}): {who}", r.outcome),
        (Some(r), None) => match r.reason.as_deref() {
            Some(by) => format!("✓ {} {by}: {who}", r.outcome),
            None => format!("✓ {}: {who}", r.outcome),
        },
        (None, _) => format!("✓ answered: {}", q.subject(&who)),
    };
    let mut said = said(Urgency::Quiet, Why::Answered, line, Some(q))?;
    said.by = agent.map(str::to_string);
    Some(said)
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

/// 11: a running task passed the p90 of its forecast. The view's
/// `forecast.past` is the board's sticky bit, so the rule fires on the change
/// to true and never again, in a run or across a restart (a seed reads it
/// already true). The forecast never stops a task: the notice offers the
/// owner Keep going (dismiss it) and Stop (`task.cancel`).
fn past_forecast(c: &Change) -> Option<Said> {
    let next = c.next;
    let f = next.forecast.as_ref().filter(|f| f.past)?;
    if next.kind != WorkKind::Task || next.state.ended() {
        return None;
    }
    if c.prev
        .and_then(|p| p.forecast.as_ref())
        .is_some_and(|p| p.past)
    {
        return None;
    }
    let spent = next.cost.map_or(0.0, |c| c.spent_usd);
    let mut line = format!(
        "⏱ {} is past its forecast: {} (p90 {}), {} (p90 {})",
        c.name,
        usd(spent),
        usd(f.usd_p90),
        span(next.at_ms.saturating_sub(next.started_at_ms)),
        span((f.min_p90 * 60_000.0) as u64)
    );
    if let Some(read) = f.read_words() {
        line.push_str(&format!(" · {read}"));
    }
    said(Urgency::Inform, Why::PastForecast, line, None)
}

/// The two answers a past-forecast notice carries: Keep going dismisses it
/// (no call), Stop is `task.cancel` on the notice's work.
pub fn forecast_answers() -> Vec<WakeAnswer> {
    vec![
        WakeAnswer {
            id: "keep_going".into(),
            label: "Keep going".into(),
            method: String::new(),
            when: None,
        },
        WakeAnswer {
            id: "stop".into(),
            label: "Stop".into(),
            method: crate::method::TASK_CANCEL.into(),
            when: None,
        },
    ]
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
fn ended(c: &Change, rules: &Rules) -> Option<Said> {
    if c.next.kind == WorkKind::System && c.next.parent.is_some() {
        return system_ended(c, rules);
    }
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
            return said(told(next, p), Why::Reported, line, None);
        }
        if !p.state.ended() && next.state == WorkState::Done {
            let line = format!("📋 {name} reported");
            return said(told(next, p), Why::Reported, line, None);
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

/// 8, the daemon's own work (theseus-jn17m): a system job that fails is an
/// Inform with its reason, one that runs `long_run_ms` or more and ends is an
/// Inform (a badge: no sound), a short one ends in silence, and progress
/// never speaks (a view that stays working is rule 10). A job the daemon's
/// stop or a restart cut (cancelled) says nothing either.
fn system_ended(c: &Change, rules: &Rules) -> Option<Said> {
    let (next, name) = (c.next, &c.name);
    // A job that appears already ended (a sync that failed at once) is the
    // transition too; one that was ended before is not.
    if c.prev.is_some_and(|p| p.state.ended()) || !next.state.ended() {
        return None;
    }
    match next.state {
        WorkState::Failed => {
            let why = next
                .report
                .as_ref()
                .and_then(|r| r.reason.as_deref())
                .filter(|r| !r.is_empty());
            let line = match why {
                Some(w) => format!("✗ {name} failed: {w}"),
                None => format!("✗ {name} failed"),
            };
            said(Urgency::Inform, Why::Failed, line, None)
        }
        WorkState::Done => {
            let ran = next
                .ended_at_ms
                .unwrap_or(next.at_ms)
                .saturating_sub(next.started_at_ms);
            if next.started_at_ms == 0 || ran < rules.long_run_ms {
                return None;
            }
            let line = match next
                .report
                .as_ref()
                .and_then(|r| r.first_line.as_deref())
                .filter(|l| !l.is_empty())
            {
                Some(l) => format!("✓ {name} is done after {} · {l}", span(ran)),
                None => format!("✓ {name} is done after {}", span(ran)),
            };
            said(Urgency::Inform, Why::Finished, line, None)
        }
        _ => None,
    }
}

/// A task's report informs; one the owner asked to hear when it is done
/// (`flagged`, `task.create { notify_done }`, theseus-j7cje) interrupts.
fn told(next: &WorkView, prev: &WorkView) -> Urgency {
    match next.flagged || prev.flagged {
        true => Urgency::Interrupt,
        false => Urgency::Inform,
    }
}

#[cfg(test)]
#[path = "tests_notices.rs"]
mod tests;
