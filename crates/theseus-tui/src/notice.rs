//! Notices (design `stage2` §2.9; theseus-753z): on transitions only, by the
//! one notification policy every surface calls (`theseus_protocol::notices`).
//! Each view the board takes live becomes a work view
//! (`WorkView::from_execution`), and `policy()` says whether its change is
//! worth telling, how urgently, and in what words; a question's reminder is
//! the view against itself at half its life. A notice waits out a second
//! and is checked again against the latest view before it fires; a burst of
//! a root's notices pings once (`Burst`); and `deliver()`, with the TUI as
//! its viewer (the session on screen while the terminal has focus, the seen
//! file, `--notify off` as quiet), says whether it rings, shows in the
//! footer only, or says nothing. How a ping reaches the operator is
//! `--notify`'s: the bell (the default), OSC 9, OSC 777, or off.

use std::collections::HashMap;

use theseus_protocol::notices::{self, Burst, Notice, Rules, Urgency, Viewer, Why};
use theseus_protocol::work::WorkView;
use theseus_protocol::{ExecutionView, Level};

/// How long a transition must hold before its notice fires.
pub const DEBOUNCE_MS: u64 = 1_000;

/// How a ping reaches the operator's terminal. (The policy's own
/// `notices::Delivery` says whether a notice pings at all.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delivery {
    /// The terminal's bell.
    #[default]
    Bell,
    /// OSC 9, a desktop notification (iTerm2, Windows Terminal, kitty).
    Osc9,
    /// OSC 777, a desktop notification with a title (rxvt, foot, Ghostty).
    Osc777,
    Off,
}

impl Delivery {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bell" => Delivery::Bell,
            "osc9" => Delivery::Osc9,
            "osc777" => Delivery::Osc777,
            "off" => Delivery::Off,
            _ => return None,
        })
    }

    /// The bytes that deliver `text`: none when off, and no bell for a ping
    /// without sound. No control character of the text reaches the
    /// terminal's sequence.
    pub fn bytes(self, text: &str, sound: bool) -> Vec<u8> {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        match self {
            Delivery::Bell if sound => b"\x07".to_vec(),
            Delivery::Bell => Vec::new(),
            Delivery::Osc9 => format!("\x1b]9;theseus: {clean}\x07").into_bytes(),
            Delivery::Osc777 => format!("\x1b]777;notify;theseus;{clean}\x07").into_bytes(),
            Delivery::Off => Vec::new(),
        }
    }
}

/// A notice that fired: its session, its line, and how it reaches the
/// operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fired {
    pub sid: String,
    pub line: String,
    pub why: Why,
    pub delivery: notices::Delivery,
}

/// The notices waiting out their second, by session, and what the policy
/// needs to remember: each session's last work view, and the burst.
#[derive(Debug)]
pub struct Notices {
    rules: Rules,
    burst: Burst,
    last: HashMap<String, WorkView>,
    due: HashMap<String, (Notice, u64)>,
}

impl Default for Notices {
    fn default() -> Self {
        let rules = Rules::default();
        Self {
            burst: Burst::new(&rules),
            rules,
            last: HashMap::new(),
            due: HashMap::new(),
        }
    }
}

impl Notices {
    /// A view the board took from a snapshot: remembered, with no notice (a
    /// notice is for a transition seen as it happens).
    pub fn saw(&mut self, v: &ExecutionView, title: &str) {
        self.last.insert(v.session_id.clone(), work(v, title));
    }

    /// A session's view moved live: the policy's notice, if any, waits out
    /// its second. A notice waiting for the same session is replaced unless
    /// it is the more urgent; an answer drops the ping it clears.
    pub fn moved(
        &mut self,
        v: &ExecutionView,
        title: &str,
        now_ms: u64,
        hm: &dyn Fn(u64) -> String,
    ) {
        let next = work(v, title);
        let prev = self.last.insert(v.session_id.clone(), next.clone());
        if let Some(n) = notices::policy(prev.as_ref(), &next, &self.rules, hm) {
            self.queue(&v.session_id, n, now_ms + DEBOUNCE_MS);
        }
    }

    fn queue(&mut self, sid: &str, n: Notice, at: u64) {
        if let Some(q) = n.retracts.as_deref() {
            self.due
                .retain(|_, (d, _)| d.question.as_deref() != Some(q));
        }
        if n.urgency == Urgency::Quiet {
            return;
        }
        match self.due.get(sid) {
            Some((old, _)) if old.urgency > n.urgency => {}
            _ => {
                self.due.insert(sid.to_string(), (n, at));
            }
        }
    }

    /// The soonest time a notice or a question's reminder is due.
    pub fn next_due(&self) -> Option<u64> {
        let reminders = self
            .last
            .values()
            .filter_map(|w| notices::next_reminder(w, &self.rules));
        self.due.values().map(|(_, at)| *at).chain(reminders).min()
    }

    /// The notices due by `now_ms` that still hold, each checked against the
    /// session's latest view (`view`), folded by the burst rule, and given
    /// its delivery to `viewer`. Each fires once.
    pub fn take_due(
        &mut self,
        now_ms: u64,
        view: &dyn Fn(&str) -> Option<ExecutionView>,
        viewer: &dyn Fn(&str) -> Viewer,
        hm: &dyn Fn(u64) -> String,
    ) -> Vec<Fired> {
        self.remind(now_ms, hm);
        let mut due: Vec<(String, Notice)> = Vec::new();
        let sids: Vec<String> = self
            .due
            .iter()
            .filter(|(_, (_, at))| *at <= now_ms)
            .map(|(sid, _)| sid.clone())
            .collect();
        for sid in sids {
            let Some((n, _)) = self.due.remove(&sid) else {
                continue;
            };
            if view(&sid).is_some_and(|v| holds(&n, &v)) {
                due.push((sid, n));
            }
        }
        due.sort_by_key(|(_, n)| n.at_ms);
        let sid_of = |n: &Notice| n.work.session_id.clone().unwrap_or_default();
        self.burst
            .fold_all(due.into_iter().map(|(_, n)| n).collect())
            .into_iter()
            .map(|n| Fired {
                delivery: notices::deliver(&n, &viewer(&sid_of(&n))),
                sid: sid_of(&n),
                line: n.line,
                why: n.why,
            })
            .filter(|f| f.delivery != notices::Delivery::Nothing)
            .collect()
    }

    /// The questions whose reminder is due: each view against itself now.
    fn remind(&mut self, now_ms: u64, hm: &dyn Fn(u64) -> String) {
        let due: Vec<String> = self
            .last
            .iter()
            .filter(|(_, w)| notices::next_reminder(w, &self.rules).is_some_and(|at| at <= now_ms))
            .map(|(sid, _)| sid.clone())
            .collect();
        for sid in due {
            let Some(last) = self.last.get_mut(&sid) else {
                continue;
            };
            let mut now = last.clone();
            now.at_ms = now_ms;
            let n = notices::policy(Some(last), &now, &self.rules, hm);
            *last = now;
            if let Some(n) = n {
                self.queue(&sid, n, now_ms);
            }
        }
    }
}

/// A session's work view, named as the board names it.
fn work(v: &ExecutionView, title: &str) -> WorkView {
    let mut w = WorkView::from_execution(v);
    w.title = title.to_string();
    w
}

/// Whether the latest view still says what the notice says: its question
/// still waits, it still needs you, it is still at rest. What happened
/// (a report, a wake, an expiry, spend) holds.
fn holds(n: &Notice, v: &ExecutionView) -> bool {
    match n.why {
        Why::Asked | Why::Reminder => n
            .question
            .as_deref()
            .is_some_and(|q| v.pending.iter().any(|p| p.correlation_id == q)),
        Why::Failed | Why::Blocked => v.attention.level == Level::NeedsYou,
        Why::Finished => {
            matches!(v.attention.level, Level::Ready | Level::Idle) && v.state != "cancelled"
        }
        _ => true,
    }
}
