//! A session's event stream as Discord messages. Pure: notifications and
//! posts in, messages out, so the whole rendering is tested without Discord.
//!
//! Two kinds of message (theseus-q4v):
//! - **Live progress**, the `Renderer`, best-effort and never replayed: while a
//!   turn runs, each loop's streamed text as messages edited in place (split
//!   under Discord's 2000-character limit, with code fences closed and
//!   reopened across a split), and one message per loop listing its tool
//!   calls, updated as each runs. Rendering is a diff: every tick re-renders
//!   the live turns and emits only the messages whose text changed, so a burst
//!   of deltas costs one edit.
//! - **Posts**, the outbox's, delivered once: a turn's reply (its loops' text
//!   in their final form, and a footer with the turn's model, loops, tools,
//!   dollars, and time), a confirm card with Approve and Decline buttons and
//!   how it closed, and notices. Their text is `reply_parts`, `card`,
//!   `settled`, and the notice functions below. A reply's parts have the keys
//!   the stream used, so its last state edits the streamed messages.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde_json::Value;
use theseus_core::outbox::Closed;
use theseus_protocol::{ConfirmRequest, Event, TurnSubmitResult};

/// Discord's limit is 2000 characters; parts stay under it with room for a fence repair.
pub const PART_LIMIT: usize = 1900;
const DISCORD_LIMIT: usize = 2000;
/// Turns kept for late updates (a confirm answered after the turn parked).
const RECENT_TURNS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Buttons {
    /// Leave the message's components as they are.
    Keep,
    /// Approve and Decline for this correlation id.
    Confirm(String),
    /// Approve, "Approve + trust session", and Decline (theseus-9bp): the
    /// call waits because its session read external text.
    ConfirmTrust(String),
    /// One "Should have asked…" select menu (theseus-sgh): an option per
    /// distinct notified tool on the message, at most `MAX_ASKED`.
    ShouldHaveAsked(Vec<Asked>),
    /// Remove every component.
    Clear,
}

/// One "should have asked" choice (theseus-sgh): a notified tool, and the
/// call whose notice it was, which the press names. On a tool message it is
/// the tool's newest notified call there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub tool: String,
    pub correlation_id: Option<String>,
}

/// Discord allows 25 options in a select menu.
pub const MAX_ASKED: usize = 25;

/// A structured notice (a Discord embed): a call ran under a `notify` posture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeCard {
    pub title: String,
    /// 0xRRGGBB.
    pub color: u32,
    pub description: String,
    pub fields: Vec<(String, String)>,
    /// A "Should have asked" button for this call; None once its tool asks
    /// first (theseus-sgh).
    pub ask: Option<Asked>,
}

pub const AMBER: u32 = 0xE3A008;

/// How a confirm message opens; the runtime strips it when the card resolves.
pub const ASK: &str = "**Approve?** ";
/// How a floor confirm opens.
pub const FLOOR_ASK: &str = "🔒 **Floor: Theseus's own state or secrets. Approve?** ";
/// How a budget question opens (theseus-0sg): the session reached its limit.
pub const BUDGET_ASK: &str = "💵 **Budget:** ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// "Theseus is typing…" (Discord shows it for about ten seconds).
    Typing,
    /// Create or edit the notice message for `key`.
    Notice { key: String, card: NoticeCard },
    /// Create the message for `key`, or edit it when it already exists.
    Upsert {
        key: String,
        content: String,
        buttons: Buttons,
    },
}

impl Op {
    /// The message it writes; typing writes none.
    pub fn key(&self) -> Option<&str> {
        match self {
            Op::Typing => None,
            Op::Notice { key, .. } | Op::Upsert { key, .. } => Some(key),
        }
    }
}

/// Where a place's approval cards go (theseus-sgh, spec §3.9 "Approval").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Route {
    /// Here: the place is a trusted channel, or there is no `[approval]`.
    #[default]
    Here,
    /// To the DM with a trusted user, `user`, which `dm` names (`DM @eddie`);
    /// the place (`place`, as `#general`) gets a one-line note that says so
    /// and why it is not trusted (`why`).
    Dm {
        user: u64,
        dm: String,
        place: String,
        why: String,
    },
    /// Nowhere on Discord: the place is not trusted, and no trusted DM is
    /// bound. The note says why, and where to answer.
    Elsewhere { why: String },
}

/// A card as posted: its text, the line its settle names, and whether it is a
/// budget question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardText {
    pub content: String,
    pub line: String,
    pub budget: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolState {
    Proposed,
    Running,
    Waiting,
    Background,
    Done {
        status: String,
        ms: u64,
    },
    /// Declined, superseded, or cancelled before it ran.
    NotRun,
    Answered {
        approved: bool,
        by: String,
    },
    /// A `/stop` ended it, running or waiting (W1): what the operator asked
    /// for, so it reads as the stop's own card does, never as a failure and
    /// apart from a cancel's `not run` (theseus-4uw).
    Stopped {
        by: String,
    },
}

#[derive(Debug, Clone)]
struct ToolLine {
    tool_use_id: String,
    tool: String,
    summary: String,
    correlation_id: Option<String>,
    state: ToolState,
    /// The setting that made it a notice (`enforcement = notify`) when a
    /// notify posture ran it.
    notice: Option<String>,
    /// What the secret broker gave it, by name: `gh got GH_TOKEN`
    /// (theseus-dcy). The gate says it first, and the job's start says what
    /// it actually got.
    granted: Option<String>,
}

#[derive(Debug, Default)]
struct LoopView {
    text: String,
    tools: Vec<ToolLine>,
}

#[derive(Debug)]
struct TurnView {
    turn_id: String,
    loops: BTreeMap<u32, LoopView>,
    /// Ended or failed: its text is the reply's post now, and only its tool
    /// messages are still live.
    ended: bool,
    dirty: bool,
}

#[derive(Default)]
pub struct Renderer {
    turns: VecDeque<TurnView>,
    /// key → the content Discord last received for it.
    emitted: HashMap<String, String>,
    /// key → the "should have asked" menu Discord last received on it.
    menus: HashMap<String, Vec<Asked>>,
    /// tool_use_id → the notice card posted for it (updated when the call
    /// ends) and the call it names.
    notices: HashMap<String, (NoticeCard, Asked)>,
    /// Tool → who tightened it ("should have asked", theseus-sgh), as the
    /// core announced.
    tightened: BTreeMap<String, String>,
    /// `[discord] notice_embeds`: a notified call posts its own card. Off, its
    /// tool line alone carries the notice.
    notice_embeds: bool,
}

/// Where else an approval can be answered when `[approval]` does not say.
pub const ELSEWHERE: &str = "in the web UI or with `theseus confirm`";

impl Renderer {
    /// A place's renderer; `notice_embeds` is the `[discord]` setting.
    pub fn new(notice_embeds: bool) -> Self {
        Self {
            notice_embeds,
            ..Self::default()
        }
    }

    /// True while a turn is running (the place keeps "typing…" alive).
    pub fn busy(&self) -> bool {
        self.turns.back().is_some_and(|t| !t.ended)
    }

    /// The turn running now, if one is (W1: the one a `/stop` stops).
    pub fn running_turn(&self) -> Option<String> {
        self.turns
            .back()
            .filter(|t| !t.ended)
            .map(|t| t.turn_id.clone())
    }

    /// A `/stop` stopped this turn (W1): its text stops streaming where
    /// Discord last saw it, as at a turn's end, and its tool messages still
    /// take their last state.
    pub fn stop(&mut self, turn_id: &str) -> Vec<Op> {
        match self.turn_mut(turn_id) {
            Some(t) if !t.ended => {
                t.ended = true;
                t.dirty = true;
                self.tick()
            }
            _ => vec![],
        }
    }

    /// Feed one notification for this session; returns what to do right away.
    /// Streamed text and tool lines wait for [`Renderer::tick`].
    pub fn on_notification(&mut self, method: &str, p: &Value) -> Vec<Op> {
        match Event::from_notification(method, p) {
            Ok(Some(e)) => self.on_event(&e),
            _ => vec![],
        }
    }

    /// One event for this session, typed (theseus-0g4).
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub fn on_event(&mut self, e: &Event) -> Vec<Op> {
        let turn_id = e.turn_id().unwrap_or("");
        match e {
            Event::TurnStarted(_) => {
                self.turns.push_back(TurnView {
                    turn_id: turn_id.to_string(),
                    loops: BTreeMap::new(),
                    ended: false,
                    dirty: false,
                });
                while self.turns.len() > RECENT_TURNS {
                    self.turns.pop_front();
                }
                vec![Op::Typing]
            }
            Event::ModelDelta(d) => {
                if let Some(t) = self.turn_mut(turn_id) {
                    t.loops
                        .entry(d.loop_index)
                        .or_default()
                        .text
                        .push_str(&d.text);
                    t.dirty = true;
                }
                vec![]
            }
            Event::ToolProposed(p) => {
                let decision = p.gate.decision.as_ref();
                let line = ToolLine {
                    tool_use_id: p.tool_use_id.clone(),
                    tool: p.tool.clone(),
                    summary: summarize(&p.tool, &p.input),
                    correlation_id: None,
                    state: ToolState::Proposed,
                    notice: decision
                        .and_then(|d| d.notify.as_ref())
                        .map(|n| n.setting.clone()),
                    granted: decision.and_then(|d| d.granted.clone()),
                };
                if let Some(t) = self.turn_mut(turn_id) {
                    let li = t.loops.keys().next_back().copied().unwrap_or(0);
                    t.loops.entry(li).or_default().tools.push(line);
                    t.dirty = true;
                }
                vec![]
            }
            Event::ToolStarted(s) => {
                // A job says what the broker actually gave it (theseus-dcy).
                let got = s.granted.as_ref().and_then(Option::as_deref);
                let withheld = s.withheld.as_deref().unwrap_or_default();
                let said = (got.is_some() || !withheld.is_empty()).then(|| {
                    got.into_iter()
                        .chain(withheld.iter().map(String::as_str))
                        .collect::<Vec<_>>()
                        .join("; ")
                });
                let corr = (!s.correlation_id.is_empty()).then(|| s.correlation_id.clone());
                self.update_tool(turn_id, &s.tool_use_id, |l| {
                    l.state = ToolState::Running;
                    if corr.is_some() {
                        l.correlation_id = corr.clone();
                    }
                    if said.is_some() {
                        l.granted = said.clone();
                    }
                });
                vec![]
            }
            Event::ToolEnded(t) => {
                let status = t.status.as_str();
                let ms = t.duration_ms.unwrap_or(0);
                let use_id = &t.tool_use_id;
                // A `/stop` ended it (theseus-4uw): the stopper, not a failure.
                let stopped = t.stopped_by.clone().filter(|_| status == "cancelled");
                let mut ops = vec![];
                if let Some((card, _)) = self.notices.get_mut(use_id) {
                    let outcome = match (status, &stopped) {
                        (_, Some(by)) => format!("⏹️ stopped by {by} · {ms} ms"),
                        ("ok", _) => format!("✅ ok · {ms} ms"),
                        ("background", _) => "⏳ running in the background".to_string(),
                        (other, _) => format!("❌ {other} · {ms} ms"),
                    };
                    if let Some(f) = card.fields.iter_mut().find(|(n, _)| n == "Outcome") {
                        f.1 = outcome;
                    }
                    ops.push(Op::Notice {
                        key: format!("notice:{use_id}"),
                        card: card.clone(),
                    });
                }
                self.update_tool(turn_id, use_id, |l| {
                    l.state = match (status, &stopped) {
                        (_, Some(by)) => ToolState::Stopped { by: by.clone() },
                        // A stop that declined it while it waited said so
                        // already (`confirm.resolved`); its not-run result
                        // keeps that.
                        ("cancelled", None) if matches!(l.state, ToolState::Stopped { .. }) => {
                            l.state.clone()
                        }
                        ("background", _) => ToolState::Background,
                        // A decline keeps saying who declined. A daemon from
                        // before theseus-8az says `denied`.
                        ("declined" | "denied", _) => match &l.state {
                            ToolState::Answered {
                                approved: false, ..
                            } => l.state.clone(),
                            _ => ToolState::NotRun,
                        },
                        _ => ToolState::Done {
                            status: status.to_string(),
                            ms,
                        },
                    };
                });
                ops
            }
            Event::PolicyNotified(_) if !self.notice_embeds => vec![],
            Event::PolicyNotified(n) => {
                let use_id = n.tool_use_id.clone();
                let call = Asked {
                    tool: n.tool.clone(),
                    correlation_id: (!n.correlation_id.is_empty())
                        .then(|| n.correlation_id.clone()),
                };
                let mut card = NoticeCard {
                    title: "🔔 Ran with a notice".into(),
                    color: AMBER,
                    description: format!("`{}` {}", n.tool, summarize(&n.tool, &n.input)),
                    fields: vec![
                        ("What".into(), clip(&n.summary, 1000)),
                        ("Posture".into(), format!("`{}`", n.notice.setting)),
                        ("Outcome".into(), "⏳ running".into()),
                    ]
                    .into_iter()
                    .chain(
                        n.granted
                            .as_deref()
                            .map(|g| ("Secrets".to_string(), format!("🔑 {g}"))),
                    )
                    .collect(),
                    ask: None,
                };
                self.asked_on(&mut card, &call);
                self.notices.insert(use_id.clone(), (card.clone(), call));
                vec![Op::Notice {
                    key: format!("notice:{use_id}"),
                    card,
                }]
            }
            // "Should have asked" (theseus-sgh). A tightening holds for every
            // session, so every place hears of it: each tool message and
            // notice card of that tool says who tightened it and stops
            // offering it; an undo offers it again.
            Event::PolicyTightened(r) | Event::PolicyUntightened(r) => {
                let tool = r.tool.clone();
                if matches!(e, Event::PolicyTightened(_)) {
                    self.tightened.insert(tool.clone(), r.by.clone());
                } else {
                    self.tightened.remove(&tool);
                }
                for t in self.turns.iter_mut() {
                    let shows = t.loops.values().any(|lv| {
                        lv.tools
                            .iter()
                            .any(|l| l.tool == tool && l.notice.is_some())
                    });
                    t.dirty |= shows;
                }
                let mut cards: Vec<(String, NoticeCard, Asked)> = self
                    .notices
                    .iter()
                    .filter(|(_, (_, call))| call.tool == tool)
                    .map(|(id, (card, call))| (id.clone(), card.clone(), call.clone()))
                    .collect();
                cards.sort_by(|a, b| a.0.cmp(&b.0));
                let mut ops = vec![];
                for (use_id, mut card, call) in cards {
                    self.asked_on(&mut card, &call);
                    self.notices.insert(use_id.clone(), (card.clone(), call));
                    ops.push(Op::Notice {
                        key: format!("notice:{use_id}"),
                        card,
                    });
                }
                ops.extend(self.tick());
                ops
            }
            // The card is a post (`card`); here only the tool line waits. It
            // is shown now, not at the next tick, and the stream before it:
            // the card waits for them, so the place reads the text, the call,
            // then its card (theseus-50p).
            Event::ConfirmRequested(req) => {
                if req.budget.is_some() {
                    // No tool line waits: the turn stopped before its call.
                    return self.tick();
                }
                // The newest proposed call of this tool is the one waiting.
                if let Some(t) = self.turns.iter_mut().rev().find(|t| !t.ended) {
                    if let Some(l) = t
                        .loops
                        .values_mut()
                        .rev()
                        .flat_map(|lv| lv.tools.iter_mut().rev())
                        .find(|l| l.tool == req.tool && l.state == ToolState::Proposed)
                    {
                        l.state = ToolState::Waiting;
                        l.correlation_id = Some(req.correlation_id.clone());
                    }
                    t.dirty = true;
                }
                self.tick()
            }
            // The card's settle is a post (`settled`); here the tool line says
            // who answered.
            Event::ConfirmResolved(r) => {
                let by = r.by.as_deref().unwrap_or("the operator").to_string();
                for t in self.turns.iter_mut() {
                    for lv in t.loops.values_mut() {
                        for l in lv.tools.iter_mut() {
                            if l.correlation_id.as_deref() == Some(r.correlation_id.as_str())
                                && l.state == ToolState::Waiting
                            {
                                // Its execution was cancelled before an
                                // answer: no one declined it, and it never ran
                                // (theseus-w98). A `/stop` declined it while it
                                // waited: stopped, by whom, as the stop's card
                                // says, not a decline (theseus-4uw).
                                l.state = if r.cancelled {
                                    ToolState::NotRun
                                } else if r.stopped {
                                    ToolState::Stopped { by: by.clone() }
                                } else {
                                    ToolState::Answered {
                                        approved: r.approved,
                                        by: by.clone(),
                                    }
                                };
                                t.dirty = true;
                            }
                        }
                    }
                }
                vec![]
            }
            Event::LoopEnded(_) => {
                if let Some(t) = self.turn_mut(turn_id) {
                    t.dirty = true;
                }
                self.tick()
            }
            // The reply and a failure are posts; the turn's tool messages take
            // their last state here, and its text stops streaming.
            Event::TurnEnded(_) | Event::TurnFailed(_) => {
                let found = self
                    .turns
                    .iter_mut()
                    .rev()
                    .find(|t| t.turn_id == turn_id || (turn_id.is_empty() && !t.ended));
                match found {
                    Some(t) => {
                        t.ended = true;
                        t.dirty = true;
                        self.tick()
                    }
                    None => vec![],
                }
            }
            _ => vec![],
        }
    }

    /// The keys of a turn's streamed text that Discord has seen: a reply's
    /// post edits these instead of posting its parts again.
    pub fn streamed(&self, turn_id: &str) -> Vec<String> {
        let prefix = format!("{turn_id}:L");
        self.emitted
            .keys()
            .filter(|k| k.starts_with(&prefix) && k.contains(":p"))
            .cloned()
            .collect()
    }

    /// Emit the messages whose rendered text or "should have asked" menu
    /// changed since Discord last saw them.
    pub fn tick(&mut self) -> Vec<Op> {
        let mut rendered = Vec::new();
        let menus = !self.notice_embeds;
        for t in self.turns.iter_mut().filter(|t| t.dirty) {
            rendered.extend(render_turn(t, &self.tightened, menus));
            t.dirty = false;
        }
        let mut ops = Vec::new();
        for m in rendered {
            let before = self.menus.get(&m.key);
            let buttons = match &m.menu {
                Some(menu) if !menu.is_empty() && before != Some(menu) => {
                    Buttons::ShouldHaveAsked(menu.clone())
                }
                Some(menu) if menu.is_empty() && before.is_some() => Buttons::Clear,
                _ => Buttons::Keep,
            };
            if self.emitted.get(&m.key) == Some(&m.content) && buttons == Buttons::Keep {
                continue;
            }
            match m.menu {
                Some(menu) if !menu.is_empty() => {
                    self.menus.insert(m.key.clone(), menu);
                }
                _ => {
                    self.menus.remove(&m.key);
                }
            }
            ops.push(self.upsert(&m.key, m.content, buttons));
        }
        ops
    }

    /// A notice card's "Should have asked" button, or, once its tool asks
    /// first, a line that says who tightened it.
    fn asked_on(&self, card: &mut NoticeCard, call: &Asked) {
        card.fields.retain(|(name, _)| name != TIGHTENED_FIELD);
        match self.tightened.get(&call.tool) {
            Some(by) => {
                card.ask = None;
                card.fields.push((
                    TIGHTENED_FIELD.into(),
                    format!(
                        "🔒 `{}` asks first from now on: tightened by {by}",
                        call.tool
                    ),
                ));
            }
            None => card.ask = Some(call.clone()),
        }
    }

    fn upsert(&mut self, key: &str, content: String, buttons: Buttons) -> Op {
        self.emitted.insert(key.to_string(), content.clone());
        Op::Upsert {
            key: key.to_string(),
            content,
            buttons,
        }
    }

    fn turn_mut(&mut self, turn_id: &str) -> Option<&mut TurnView> {
        self.turns.iter_mut().rev().find(|t| t.turn_id == turn_id)
    }

    /// A call's line is in the turn that proposed it. A call approved after
    /// that turn parked runs in a later turn, whose events name the later
    /// turn, and so does a background job's late result: either finds its
    /// line in the turn that holds it, so the line says how the call ended
    /// (a stop's `⏹️` included, theseus-4uw) instead of keeping `👍 approved`.
    fn update_tool(&mut self, turn_id: &str, tool_use_id: &str, f: impl Fn(&mut ToolLine)) {
        let holds = |t: &TurnView| {
            t.loops
                .values()
                .any(|lv| lv.tools.iter().any(|l| l.tool_use_id == tool_use_id))
        };
        let at = match self
            .turns
            .iter()
            .position(|t| t.turn_id == turn_id && holds(t))
        {
            Some(i) => Some(i),
            None => self.turns.iter().rposition(holds),
        };
        let Some(t) = at.map(|i| &mut self.turns[i]) else {
            return;
        };
        for lv in t.loops.values_mut() {
            if let Some(l) = lv.tools.iter_mut().find(|l| l.tool_use_id == tool_use_id) {
                f(l);
                t.dirty = true;
            }
        }
    }
}

/// The field a notice card gains once its tool asks first.
const TIGHTENED_FIELD: &str = "Should have asked";

/// One message of a turn as rendered: its key, its text, and, on a tool
/// message when menus are on, its "should have asked" choices.
struct Rendered {
    key: String,
    content: String,
    menu: Option<Vec<Asked>>,
}

impl Rendered {
    fn text(key: String, content: String) -> Self {
        Self {
            key,
            content,
            menu: None,
        }
    }
}

/// Every live message a turn shows, in the order Discord should first see
/// them: its streamed text while it runs (then its reply's post owns the
/// text), and its tool messages. `tightened` is tool → who tightened it;
/// `menus` puts a "Should have asked…" select on each tool message that lists
/// a notified call.
fn render_turn(t: &TurnView, tightened: &BTreeMap<String, String>, menus: bool) -> Vec<Rendered> {
    let mut out: Vec<Rendered> = Vec::new();
    for (li, lv) in &t.loops {
        if !t.ended {
            for (i, part) in split_text(&lv.text, PART_LIMIT).into_iter().enumerate() {
                out.push(Rendered::text(format!("{}:L{li}:p{i}", t.turn_id), part));
            }
        }
        if !lv.tools.is_empty() {
            let (asked, tight) = notified_tools(&lv.tools, tightened);
            // A line per notified tool that asks first now, under the calls.
            let said: Vec<String> = tight
                .iter()
                .map(|(tool, by)| {
                    format!("-# 🔒 `{tool}` asks first from now on: tightened by {by}")
                })
                .collect();
            let reserve: usize = said.iter().map(|l| l.len() + 1).sum();
            let mut content = tool_lines(&lv.tools, reserve);
            for l in &said {
                content.push('\n');
                content.push_str(l);
            }
            out.push(Rendered {
                key: format!("{}:L{li}:tools", t.turn_id),
                content,
                menu: menus.then_some(asked),
            });
        }
    }
    out
}

// ---------------------------------------------------------------- posts

/// A reply's messages, in order: each loop's text in its final form, under
/// the keys the stream used, and the footer riding on the last part when it
/// fits, else standing alone (theseus-q4v).
pub fn reply_parts(
    turn_id: &str,
    texts: &[(u32, String)],
    result: Option<&TurnSubmitResult>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (li, text) in texts {
        for (i, part) in split_text(text, PART_LIMIT).into_iter().enumerate() {
            out.push((format!("{turn_id}:L{li}:p{i}"), part));
        }
    }
    if let Some(f) = result.map(footer) {
        match out.last_mut() {
            Some((_, c)) if c.len() + f.len() < DISCORD_LIMIT => {
                c.push('\n');
                c.push_str(&f);
            }
            _ => out.push((format!("{turn_id}:footer"), f)),
        }
    }
    out
}

/// A wake's turn (DD8): its reply opens with each wake's line, so the place
/// sees why Theseus spoke unasked, and so does a turn a task's report started
/// (W1: `📋 task a1b2c3 reported`). The lines ride on the first part when
/// they fit; the session's history has them either way.
pub fn wake_header(parts: &mut [(String, String)], wakes: &[String]) {
    if wakes.is_empty() {
        return;
    }
    let head = wakes
        .iter()
        .map(|w| format!("-# {}", clip(w, 300)))
        .collect::<Vec<_>>()
        .join("\n");
    if let Some((_, first)) = parts.first_mut() {
        if first.len() + head.len() < DISCORD_LIMIT {
            *first = format!("{head}\n{first}");
        }
    }
}

/// This place's pending wakes (DD8), as `/wakes` lists them: the soonest
/// ten, each with its time in the reader's own zone and its note.
pub fn wakes(wakes: &[theseus_protocol::WakeInfo]) -> String {
    if wakes.is_empty() {
        return "No wakes pending here. Ask for one: \"remind me in 10 minutes to …\".".into();
    }
    let mut lines = vec![format!("**Wakes here** ({})", wakes.len())];
    for w in wakes.iter().take(10) {
        let at = w.due_at_ms / 1000;
        let note = w.note.lines().next().unwrap_or_default();
        let busy = match w.state.as_str() {
            "running" | "queued" => " · runs when the current turn ends",
            _ => "",
        };
        lines.push(format!(
            "• `{}` <t:{at}:t> (<t:{at}:R>){busy} · {}",
            w.short,
            clip(note, 120)
        ));
    }
    if wakes.len() > 10 {
        lines.push(format!(
            "-# and {} later; `theseus wakes` lists them all",
            wakes.len() - 10
        ));
    }
    lines.push("-# `/cancel <id>` cancels one.".into());
    lines.join("\n")
}

/// A card's text for the route it takes: `elsewhere` is where else it can be
/// answered ("" for nowhere).
pub fn card(req: &ConfirmRequest, route: &Route, elsewhere: &str) -> CardText {
    // A task's question names the task (DD7), on the card, its note, and its
    // settle, which all carry the line.
    let task = req
        .task
        .as_ref()
        .map(|t| format!("task `{}`: ", t.short))
        .unwrap_or_default();
    if let Some(b) = &req.budget {
        let also = match elsewhere {
            "" => String::new(),
            e => format!(" You can also answer {e}."),
        };
        let asked_for = match route {
            Route::Dm { place, .. } => format!("For {place}. "),
            _ => String::new(),
        };
        return CardText {
            content: format!(
                "{BUDGET_ASK}{}\n-# {asked_for}Approve resets its spend to $0 and the waiting \
                 call goes on; the session's lifetime cost ({}) keeps counting. Decline, or send \
                 a new message, and it keeps waiting.{also}",
                clip(&req.reason, 300),
                dollars(b.lifetime_usd)
            ),
            line: format!(
                "{task}spend reset ({} of the {} limit)",
                dollars(b.spent_usd),
                dollars(b.limit_usd)
            ),
            budget: true,
        };
    }
    let line = format!("{task}`{}` {}", req.tool, summarize(&req.tool, &req.input));
    let mut content = format!("{}{line}", if req.floor { FLOOR_ASK } else { ASK });
    if !req.reason.is_empty() {
        content.push_str(&format!("\n{}", clip(&req.reason, 300)));
    }
    let asked_for = match route {
        Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let also = match elsewhere {
        "" => String::new(),
        e => format!(" · you can also answer {e}"),
    };
    // A call that waits because its session read external text
    // (theseus-9bp): the card's third button trusts the session again.
    let trust = if req.external_text.is_some() {
        " · **Approve + trust session** also lets its later calls run at their postures"
    } else {
        ""
    };
    content.push_str(&format!(
        "\n-# {asked_for}expires <t:{}:R>{trust}{also}",
        req.expires_at_ms / 1000
    ));
    CardText {
        content,
        line: if req.floor {
            format!("{line} (floor)")
        } else {
            line
        },
        budget: false,
    }
}

/// The note a place gets when its card goes elsewhere: to a trusted DM, or
/// nowhere on Discord. None when the card is here.
pub fn card_note(route: &Route, line: &str, elsewhere: &str) -> Option<String> {
    match route {
        Route::Here => None,
        Route::Dm { dm, why, .. } => Some(format!(
            "🔐 Approval for {line} was asked in {dm}: this channel is not a trusted channel \
             ({why})."
        )),
        Route::Elsewhere { why } => {
            let answer = match elsewhere {
                "" => ", and no trusted channel is bound here to answer it".to_string(),
                e => format!(": answer {e}"),
            };
            Some(format!(
                "🔐 {line} waits for approval, and this channel is not a trusted channel \
                 ({why}){answer}."
            ))
        }
    }
}

/// How a card reads once its question closed: who answered, or what closed it.
pub fn settled(closed: &Closed, line: &str, budget: bool) -> String {
    let by = closed.by.as_deref().unwrap_or("the operator");
    let note = closed.note.as_deref().unwrap_or("");
    match closed.how.as_str() {
        "superseded" if budget => format!(
            "⏭️ **Replaced**: a new message came first, and its call asks again if it still does \
             not fit. {line}"
        ),
        "superseded" => format!("⏭️ **Not run**: a new message replaced this request. {line}"),
        "declined" if budget => format!(
            "❎ **Declined** by {by} · {line}; the session keeps waiting, and a new message asks \
             again"
        ),
        "declined" => format!("❎ **Declined** by {by} · {line}"),
        // An approval that trusted the session again says so (theseus-9bp).
        "approved" if !note.is_empty() => format!("✅ **Approved** by {by}, {note} · {line}"),
        "approved" => format!("✅ **Approved** by {by} · {line}"),
        "withdrawn" => format!("↩️ **Withdrawn**: {note} · {line}"),
        "ended" => format!("⏹️ **Closed**: the session's work ended · {line}"),
        // `/stop` (W1): the question closed, and the conversation goes on.
        "stopped" => format!("⏹️ **Not run**: {by} stopped this session's work · {line}"),
        _ if note.is_empty() => format!("⏹️ **Closed** · {line}"),
        _ => format!("⏹️ **Closed**: {} · {line}", clip(note, 200)),
    }
}

/// The note that went with a card, once the card settled: in the place, it
/// says how, and where the card was.
pub fn settled_note(content: &str, dm: Option<&str>) -> String {
    match dm {
        Some(dm) => format!("🔐 {content} (in {dm})"),
        None => format!("🔐 {content}"),
    }
}

/// A turn that failed, as its notice says it. A run of failures posts one
/// notice, at its first failure when the driver retries with backoff, and
/// one more when it parks (theseus-ljr), so the notice says which: `then`
/// is `backoff` or `park`, and `turns` the failed turns in a row. A post
/// written before theseus-ljr has neither.
pub fn failed(class: &str, error: &str, then: Option<&str>, turns: u64) -> String {
    let head = format!("⚠️ **Turn failed** ({class}): {}", clip(error, 600));
    match then {
        Some("backoff") => format!(
            "{head}\n-# Retrying with backoff while it lasts. No more notices for it: the reply \
             comes when the provider answers."
        ),
        Some("park") if turns > 1 => format!(
            "{head}\n-# Failed {turns} times in a row, so nothing retries it now: your next \
             message does."
        ),
        Some("park") => format!("{head}\n-# Nothing retries it now: your next message does."),
        _ => head,
    }
}

/// This place's tasks (DD7), as `/tasks` lists them: the newest ten, each
/// with its state, what it waits on, its spend of its limit, and its age.
pub fn tasks(tasks: &[theseus_protocol::TaskInfo], now_ms: u64) -> String {
    if tasks.is_empty() {
        return "No tasks here yet. Ask for one: \"start a task that …\".".into();
    }
    let mut lines = vec![format!("**Tasks here** ({})", tasks.len())];
    for t in tasks.iter().take(10) {
        let age = now_ms.saturating_sub(t.created_at_ms) / 1000;
        let age = match age {
            0..=59 => format!("{age} s"),
            60..=3599 => format!("{} min", age / 60),
            3600..=86_399 => format!("{} h", age / 3600),
            _ => format!("{} d", age / 86_400),
        };
        let state = match &t.waiting_on {
            Some(w) => format!("{} on {w}", t.state),
            None => t.state.clone(),
        };
        let asks = if t.pending_confirms > 0 {
            " · **needs you**"
        } else {
            ""
        };
        lines.push(format!(
            "• `{}` {state}{asks} · {} of {} · {age} · {}",
            t.short,
            dollars(t.spent_usd),
            dollars(t.limit_usd),
            clip(t.title.as_deref().unwrap_or("untitled"), 80)
        ));
    }
    if tasks.len() > 10 {
        lines.push(format!(
            "-# and {} older; `theseus tasks` lists them all",
            tasks.len() - 10
        ));
    }
    lines.push("-# `/cancel <task>` stops one.".into());
    lines.join("\n")
}

/// A task's report (DD7), as one message: the task by its short id and
/// title, how it ended, its last message (clipped to fit), and what it spent.
pub fn report(body: &Value, said: Option<&str>) -> String {
    let short = body["short"].as_str().unwrap_or("?");
    let title = body["title"]
        .as_str()
        .map(|t| format!(" · {}", clip(t, 80)))
        .unwrap_or_default();
    let reason = body["reason"].as_str().unwrap_or("");
    let head = match body["outcome"].as_str() {
        Some("complete") => format!("📋 **Task `{short}` finished**{title}"),
        Some("cancelled") => format!(
            "⏹️ **Task `{short}` stopped**{title}: {}",
            clip(
                if reason.is_empty() {
                    "cancelled"
                } else {
                    reason
                },
                200
            )
        ),
        _ => format!(
            "⚠️ **Task `{short}` failed**{title}: {}",
            clip(
                if reason.is_empty() {
                    "its turn failed"
                } else {
                    reason
                },
                400
            )
        ),
    };
    let turns = body["turns"].as_u64().unwrap_or(0);
    let secs = body["elapsed_ms"].as_u64().unwrap_or(0) as f64 / 1000.0;
    let took = if secs < 60.0 {
        format!("{secs:.1} s")
    } else {
        format!("{} min {} s", secs as u64 / 60, secs as u64 % 60)
    };
    let facts = format!(
        "-# {turns} turn{} · {} of {} · {took}",
        if turns == 1 { "" } else { "s" },
        dollars(body["spent_usd"].as_f64().unwrap_or(0.0)),
        dollars(body["limit_usd"].as_f64().unwrap_or(0.0)),
    );
    // One message: what it said gets the room the rest leaves.
    let room = DISCORD_LIMIT
        .saturating_sub(head.chars().count() + facts.chars().count() + 8)
        .min(1_700);
    match said {
        Some(s) if !s.trim().is_empty() => format!("{head}\n{}\n{facts}", clip(s.trim(), room)),
        _ => format!("{head}\n{facts}"),
    }
}

/// The line after a restart onto the vault's changed config note (theseus-2fo).
pub fn restarted(at_unix_ms: u64, tables: &[String]) -> String {
    format!(
        "Restarted <t:{}:T> onto the vault's config note, which had changed since my local \
         copy: {}.",
        at_unix_ms / 1000,
        tables.join(", ")
    )
}

/// A post created again after an outage longer than Discord keeps a nonce
/// (theseus-q4v): the channel may already hold it, so it says so.
pub const RESENT: &str =
    "-# ↻ Sent again after an outage: if the same message is just above, this is a copy.";

/// A tool message's distinct notified tools, in the order they first ran:
/// those still offered to "should have asked", each with its newest
/// notified call (at most `MAX_ASKED`), and those that ask first now, with
/// who tightened them.
fn notified_tools(
    tools: &[ToolLine],
    tightened: &BTreeMap<String, String>,
) -> (Vec<Asked>, Vec<(String, String)>) {
    let mut asked: Vec<Asked> = Vec::new();
    let mut tight: Vec<(String, String)> = Vec::new();
    for l in tools.iter().filter(|l| l.notice.is_some()) {
        if let Some(by) = tightened.get(&l.tool) {
            if !tight.iter().any(|(t, _)| *t == l.tool) {
                tight.push((l.tool.clone(), by.clone()));
            }
            continue;
        }
        if let Some(a) = asked.iter_mut().find(|a| a.tool == l.tool) {
            if l.correlation_id.is_some() {
                a.correlation_id = l.correlation_id.clone();
            }
        } else if asked.len() < MAX_ASKED {
            asked.push(Asked {
                tool: l.tool.clone(),
                correlation_id: l.correlation_id.clone(),
            });
        }
    }
    (asked, tight)
}

/// The tool message's lines, in at most `PART_LIMIT - 60 - reserve`
/// characters.
fn tool_lines(tools: &[ToolLine], reserve: usize) -> String {
    let lines: Vec<String> = tools
        .iter()
        .map(|l| {
            let mark = match l.notice.as_deref() {
                Some("") => " · 🔔 notified".to_string(),
                Some(setting) => format!(" · 🔔 notified ({setting})"),
                None => String::new(),
            };
            let key = l
                .granted
                .as_deref()
                .map_or_else(String::new, |g| format!(" · 🔑 {g}"));
            let head = format!("`{}` {}{key}{mark}", l.tool, l.summary);
            match &l.state {
                ToolState::Proposed => format!("▫️ {head}"),
                ToolState::Running => format!("⏳ {head}"),
                ToolState::Waiting => format!("⏸️ {head} · waiting for approval"),
                ToolState::Background => {
                    format!("⏳ {head} · running in the background; the result will come back here")
                }
                ToolState::Done { status, ms } if status == "ok" => format!("✅ {head} · {ms} ms"),
                ToolState::Done { status, ms } => format!("❌ {head} · {status} · {ms} ms"),
                ToolState::NotRun => format!("🚫 {head} · not run"),
                ToolState::Stopped { by } => format!("⏹️ {head} · stopped by {by}"),
                ToolState::Answered { approved: true, by } => {
                    format!("👍 {head} · approved by {by}")
                }
                ToolState::Answered {
                    approved: false,
                    by,
                } => {
                    format!("👎 {head} · declined by {by}")
                }
            }
        })
        .collect();
    // Keep the newest lines when a loop made more calls than one message holds,
    // with room for the line that counts the rest.
    let mut kept: Vec<&String> = Vec::new();
    let mut len = 0;
    let budget = (PART_LIMIT - 60).saturating_sub(reserve);
    for l in lines.iter().rev() {
        if len + l.len() + 1 > budget {
            break;
        }
        len += l.len() + 1;
        kept.push(l);
    }
    kept.reverse();
    let hidden = lines.len() - kept.len();
    let mut s = String::new();
    if hidden > 0 {
        s.push_str(&format!("-# … {hidden} earlier call(s)"));
        // This line may be the only place Discord shows their notices.
        let notified = tools[..hidden]
            .iter()
            .filter(|l| l.notice.is_some())
            .count();
        if notified > 0 {
            s.push_str(&format!(" · 🔔 {notified} notified"));
        }
        s.push('\n');
    }
    s.push_str(
        &kept
            .into_iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    s
}

fn footer(r: &TurnSubmitResult) -> String {
    let mut bits = vec![];
    if !r.profile.is_empty() {
        bits.push(r.profile.clone());
    }
    bits.push(r.model.clone());
    bits.push(format!(
        "{} loop{}",
        r.loops,
        if r.loops == 1 { "" } else { "s" }
    ));
    if r.tool_calls > 0 {
        bits.push(format!(
            "{} tool call{}",
            r.tool_calls,
            if r.tool_calls == 1 { "" } else { "s" }
        ));
    }
    if let Some(c) = r.cost_usd {
        bits.push(format!("${c:.4}"));
    }
    bits.push(format!("{:.1} s", r.elapsed_ms as f64 / 1000.0));
    if r.continuation {
        bits.push("continued".into());
    }
    if r.awaiting_confirm.is_some() {
        bits.push("waiting for your approval".into());
    } else if !matches!(r.stop_reason.as_str(), "" | "no_tool_calls" | "end_turn") {
        bits.push(format!("stopped: {}", r.stop_reason));
    }
    format!("-# {}", bits.join(" · "))
}

/// One short line for a tool call: the argument a person would look for.
pub fn summarize(tool: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let text = if let Some(argv) = input.get("argv").and_then(Value::as_array) {
        argv.iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    } else if let Some(p) = s("pattern") {
        match s("path") {
            Some(d) => format!("{p} in {d}"),
            None => p,
        }
    } else if let Some(p) = s("path").or_else(|| s("rev")).or_else(|| s("file")) {
        p
    } else if tool == "fs.patch" {
        "(patch)".into()
    } else if let Some(u) = s("url") {
        u
    } else if let Some(q) = s("query") {
        format!("\"{q}\"")
    } else {
        serde_json::to_string(input).unwrap_or_default()
    };
    clip(&text.replace('`', "'").replace('\n', " "), 90)
}

/// The notice for a refusal of a Theseus job's process (theseus-6qy), from
/// the `approval.refused` notification: what it tried, from which process
/// of which job, and that nothing moved. It goes where approvals go.
pub fn job_refusal(p: &Value) -> String {
    let tool = str_of(p, "tool");
    let what = match str_of(p, "act").as_str() {
        theseus_protocol::method::POLICY_UNTIGHTEN => format!("undo the tightening of `{tool}`"),
        _ if tool == theseus_protocol::BUDGET_TOOL => "answer the spend reset".to_string(),
        _ => format!("answer the approval of `{tool}`"),
    };
    let a = p.get("asker").cloned().unwrap_or(Value::Null);
    let argv0 = || clip(&str_of(&a, "argv0").replace('`', "'"), 40);
    let from = match (
        a.get("job").and_then(Value::as_str),
        a.get("pid").and_then(Value::as_u64),
        a.get("under_daemon").and_then(Value::as_u64),
        a.get("under_other_daemon").and_then(Value::as_u64),
    ) {
        (Some(job), Some(pid), ..) => {
            format!("`{}` (pid {pid}), a process of job `{job}`", argv0())
        }
        // A job's orphan, whose wrapper died (theseus-z4b).
        (None, Some(pid), Some(daemon), _) => format!(
            "`{}` (pid {pid}), a process under theseusd itself (pid {daemon}), which is a job's \
             orphan",
            argv0()
        ),
        // Under another serving daemon, a scratch one or `--stdio` (theseus-6uo).
        (None, Some(pid), None, Some(daemon)) => format!(
            "`{}` (pid {pid}), a process under another serving theseusd (pid {daemon}), which \
             counts as that daemon's job",
            argv0()
        ),
        _ => format!(
            "a process that could not be traced ({}), which counts as a job's",
            clip(&str_of(&a, "untraceable").replace('`', "'"), 160)
        ),
    };
    let then = match str_of(p, "act").as_str() {
        theseus_protocol::method::POLICY_UNTIGHTEN => "It keeps asking first.",
        _ => "It keeps waiting for your answer.",
    };
    format!(
        "🚨 Refused: {from}, tried to {what} through {}. A job cannot answer an approval. {then}",
        str_of(p, "via")
    )
}

/// Dollars as the narrative says them: `$100`, `$0.45`, `$0.0045`.
fn dollars(usd: f64) -> String {
    theseus_core::narrative::dollars((usd.max(0.0) * 1e6).round() as u64)
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Split streamed text into Discord-sized parts. Cuts prefer a newline, then a
/// space, in the second half of the budget; a code fence left open by a cut is
/// closed at the end of the part and reopened (with its language, in at most
/// `MAX_FENCE` bytes) at the start of the next. Stable under appends: text only
/// grows, so earlier cuts stay put. Every part moves the text on by at least a
/// character, so a budget smaller than the next character never stalls it
/// (theseus-s68).
pub fn split_text(text: &str, limit: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = text;
    let mut carry: Option<String> = None;
    while !rest.is_empty() {
        let prefix = carry.as_ref().map(|f| format!("{f}\n")).unwrap_or_default();
        let budget = limit.saturating_sub(prefix.len() + 4).max(1);
        if rest.len() <= budget {
            parts.push(prefix + rest);
            break;
        }
        let mut cut = floor_boundary(rest, budget);
        if cut == 0 {
            // The budget is smaller than the next character: it goes whole.
            cut = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        let head = &rest[..cut];
        if let Some(i) = head.rfind('\n').filter(|&i| i > budget / 2) {
            cut = i + 1;
        } else if let Some(i) = head.rfind(' ').filter(|&i| i > budget / 2) {
            cut = i + 1;
        }
        let (head, tail) = rest.split_at(cut);
        let mut part = prefix + head;
        carry = open_fence(&part);
        if carry.is_some() {
            if !part.ends_with('\n') {
                part.push('\n');
            }
            part.push_str("```");
        }
        parts.push(part);
        rest = tail;
    }
    parts
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The longest opening line a split reopens a fence with. Longer ones (the
/// language is text anyone can write) are cut to it, so a fence line as long
/// as the limit cannot eat the next part's budget (theseus-s68).
const MAX_FENCE: usize = 32;

/// The opening line of a code fence left open at the end of `s`, if any, in
/// at most `MAX_FENCE` bytes.
fn open_fence(s: &str) -> Option<String> {
    let mut open: Option<&str> = None;
    for line in s.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            open = match open {
                Some(_) => None,
                None => Some(t.trim_end()),
            };
        }
    }
    open.map(|f| f[..floor_boundary(f, MAX_FENCE.min(f.len()))].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal of a Theseus job's process goes to the DM as one line: what
    /// it tried, from which process of which job, and that nothing moved
    /// (theseus-6qy).
    #[test]
    fn a_jobs_refused_answer_is_one_notice() {
        let asker = json!({"pid": 4242, "argv0": "theseus", "job": "act_job", "wrapper_pid": 4200});
        let n = |act: &str, tool: &str, asker: &Value| {
            job_refusal(&json!({"act": act, "tool": tool, "via": "cli", "asker": asker}))
        };
        assert_eq!(
            n("action.confirm", "fs.write", &asker),
            "🚨 Refused: `theseus` (pid 4242), a process of job `act_job`, tried to answer the \
             approval of `fs.write` through cli. A job cannot answer an approval. It keeps \
             waiting for your answer."
        );
        assert!(
            n("action.confirm", "budget.reset", &asker).contains("tried to answer the spend reset")
        );
        let undo = n("policy.untighten", "fs.edit", &asker);
        assert!(
            undo.contains("tried to undo the tightening of `fs.edit`"),
            "{undo}"
        );
        assert!(undo.ends_with("It keeps asking first."), "{undo}");
        let lost = n(
            "action.confirm",
            "fs.write",
            &json!({"untraceable": "pid 9 has exited"}),
        );
        assert!(
            lost.contains(
                "a process that could not be traced (pid 9 has exited), which counts as a job's"
            ),
            "{lost}"
        );
        // A job's orphan, whose wrapper died, under the daemon (theseus-z4b).
        let orphan = n(
            "action.confirm",
            "fs.write",
            &json!({"pid": 4343, "argv0": "theseus", "under_daemon": 4000}),
        );
        assert_eq!(
            orphan,
            "🚨 Refused: `theseus` (pid 4343), a process under theseusd itself (pid 4000), which \
             is a job's orphan, tried to answer the approval of `fs.write` through cli. A job \
             cannot answer an approval. It keeps waiting for your answer."
        );
        // Under another serving daemon (theseus-6uo).
        let other = n(
            "action.confirm",
            "fs.write",
            &json!({"pid": 4343, "argv0": "theseus", "under_other_daemon": 5000}),
        );
        assert!(
            other.starts_with(
                "🚨 Refused: `theseus` (pid 4343), a process under another serving theseusd \
                 (pid 5000), which counts as that daemon's job, tried to answer"
            ),
            "{other}"
        );
    }
    use serde_json::json;

    fn upserts(ops: &[Op]) -> Vec<(String, String)> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Upsert { key, content, .. } => Some((key.clone(), content.clone())),
                Op::Typing | Op::Notice { .. } => None,
            })
            .collect()
    }

    fn result_of(v: Value) -> TurnSubmitResult {
        serde_json::from_value(v).unwrap()
    }

    fn request(v: Value) -> ConfirmRequest {
        serde_json::from_value(v).unwrap()
    }

    fn ended(turn: &str, awaiting: Option<&str>) -> Value {
        json!({"session_id": "s", "turn_id": turn, "loops": 2, "output": "", "stop_reason": "no_tool_calls",
               "provider_stop_reason": null, "model": "claude-sonnet-5", "provider": "anthropic", "profile": "sonnet",
               "usage": {"input_tokens": 1, "output_tokens": 2, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
               "elapsed_ms": 4200, "tool_calls": 1, "cost_usd": 0.0123, "awaiting_confirm": awaiting, "continuation": false})
    }

    #[test]
    fn a_turn_streams_as_edits_and_ends_with_a_footer() {
        let mut r = Renderer::default();
        assert_eq!(
            r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"})),
            vec![Op::Typing]
        );
        assert!(r.busy());
        r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 0, "text": "Hel"}),
        );
        r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 0, "text": "lo"}),
        );
        let first = upserts(&r.tick());
        assert_eq!(first, vec![("t1:L0:p0".into(), "Hello".into())]);
        assert!(r.tick().is_empty(), "nothing changed, nothing sent");
        r.on_notification(
            "tool.proposed",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
            "input": {"path": "src/lib.rs"}, "gate": {"result": {"gate": "allow"}}}),
        );
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 12}),
        );
        let ops =
            upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
        assert_eq!(
            ops,
            vec![(
                "t1:L0:tools".into(),
                "✅ `fs.read` src/lib.rs · 12 ms".into()
            )]
        );
        r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 1, "text": "Done."}),
        );
        // The turn's end is its reply's post (theseus-q4v): the stream stops,
        // and the post's parts edit the streamed messages under their keys.
        assert!(upserts(&r.on_notification("turn.ended", &ended("t1", None))).is_empty());
        assert!(!r.busy());
        let parts = reply_parts(
            "t1",
            &[(0, "Hello".into()), (1, "Done.".into())],
            Some(&result_of(ended("t1", None))),
        );
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], ("t1:L0:p0".into(), "Hello".into()));
        assert_eq!(parts[1].0, "t1:L1:p0");
        assert!(
            parts[1].1.starts_with(
                "Done.\n-# sonnet · claude-sonnet-5 · 2 loops · 1 tool call · $0.0123 · 4.2 s"
            ),
            "{}",
            parts[1].1
        );
        // With no text, the footer stands alone.
        let alone = reply_parts("t2", &[], Some(&result_of(ended("t2", None))));
        assert_eq!(alone.len(), 1);
        assert_eq!(alone[0].0, "t2:footer");
    }

    /// A session at its spend limit asks with its own message and buttons
    /// (theseus-0sg); approve says the spend was reset, decline says it keeps
    /// waiting, and a new message replaces the question.
    #[test]
    fn a_budget_question_asks_with_buttons_and_says_what_each_answer_does() {
        let ask = |corr: &str| {
            json!({"correlation_id": corr, "session_id": "s", "execution_id": "e", "tool": "budget.reset",
                "input": {"spent_usd": 99.48, "limit_usd": 100.0}, "by": "operator", "requested_at_ms": 1, "expires_at_ms": 0,
                "reason": "This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?",
                "budget": {"spent_usd": 99.48, "limit_usd": 100.0, "needed_usd": 1.368, "lifetime_usd": 212.4}})
        };
        let card = card(&request(ask("act_b")), &Route::Here, ELSEWHERE);
        assert!(card.budget);
        assert!(card.content.starts_with(&format!("{BUDGET_ASK}This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?\n")), "{}", card.content);
        assert!(
            card.content
                .contains("lifetime cost ($212.40) keeps counting"),
            "{}",
            card.content
        );
        assert!(
            !card.content.contains("expires"),
            "a budget question does not expire: {}",
            card.content
        );
        assert_eq!(card.line, "spend reset ($99.48 of the $100 limit)");
        for (closed, want) in [
            (Closed::new("approved", Some("discord:eddie")), "✅ **Approved** by discord:eddie · spend reset ($99.48 of the $100 limit)"),
            (Closed::new("declined", Some("discord:eddie")), "❎ **Declined** by discord:eddie · spend reset ($99.48 of the $100 limit); the session keeps waiting, and a new message asks again"),
            (Closed::new("superseded", None), "⏭️ **Replaced**: a new message came first, and its call asks again if it still does not fit. spend reset ($99.48 of the $100 limit)"),
            // A raise withdraws it (theseus-3pj, the stale card of S1).
            (Closed { how: "withdrawn".into(), by: None, note: Some("the spend limit was raised from $100 to $200".into()) },
             "↩️ **Withdrawn**: the spend limit was raised from $100 to $200 · spend reset ($99.48 of the $100 limit)"),
        ] {
            assert_eq!(settled(&closed, &card.line, true), want);
        }
    }

    #[test]
    fn a_confirm_gets_buttons_and_loses_them_when_answered() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        let req = json!({"correlation_id": "act_1", "session_id": "s",
            "execution_id": "e", "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1790000000000u64});
        // The card is a post; the stream marks the tool line, and shows it at
        // once, so that it goes before the card (theseus-50p).
        let tools = upserts(&r.on_notification("confirm.requested", &req));
        assert!(
            tools
                .iter()
                .any(|(k, c)| k == "t1:L0:tools" && c.contains("waiting for approval")),
            "{tools:?}"
        );
        let card = card(&request(req), &Route::Here, ELSEWHERE);
        assert!(card
            .content
            .starts_with("**Approve?** `proc.run` cargo test\nrun cargo test"));
        assert_eq!(card.line, "`proc.run` cargo test");
        // The turn's end has nothing new for it.
        let tools = upserts(&r.on_notification("turn.ended", &ended("t1", Some("act_1"))));
        assert!(!tools.iter().any(|(k, _)| k == "t1:L0:tools"), "{tools:?}");
        assert!(r
            .on_notification(
                "confirm.resolved",
                &json!({"correlation_id": "act_1", "approved": true, "by": "discord:eddie"}),
            )
            .is_empty());
        assert_eq!(
            settled(
                &Closed::new("approved", Some("discord:eddie")),
                &card.line,
                false
            ),
            "✅ **Approved** by discord:eddie · `proc.run` cargo test"
        );
        let tools = upserts(&r.tick());
        assert!(
            tools[0].1.contains("approved by discord:eddie"),
            "{tools:?}"
        );
    }

    /// `tool.proposed` and `policy.notified` for a call that runs under
    /// `enforcement = notify`, shaped as the core sends them.
    fn notified(use_id: &str, cmd: &str) -> (Value, Value) {
        let argv: Vec<&str> = cmd.split(' ').collect();
        let rule = "proc.run — notify (enforcement = notify)";
        let proposed = json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "proc.run",
            "input": {"argv": argv}, "gate": {"result": {"gate": "allow"}, "decision": {"posture": "notify",
            "reason": rule, "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": rule}}}});
        let notice = json!({"session_id": "s", "turn_id": "t1", "tool_use_id": use_id, "tool": "proc.run",
            "input": {"argv": argv}, "summary": format!("run `{cmd}` in /w"), "kind": "notify",
            "setting": "enforcement = notify", "rule": rule});
        (proposed, notice)
    }

    #[test]
    fn with_notice_embeds_a_notified_call_posts_a_card_and_the_card_gets_its_outcome() {
        let mut r = Renderer::new(true);
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "allow"},
            "decision": {"mode": "allow", "posture": "notify", "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}));
        let ops = r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": "t1", "tool_use_id": "u1",
            "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "summary": "run `cargo test` in /w",
            "kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}));
        let Op::Notice { key, card } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!(key, "notice:u1");
        assert_eq!(card.title, "🔔 Ran with a notice");
        assert_eq!(card.color, AMBER);
        assert_eq!(card.description, "`proc.run` cargo test");
        assert_eq!(
            card.fields,
            vec![
                ("What".into(), "run `cargo test` in /w".into()),
                ("Posture".into(), "`enforcement = notify`".into()),
                ("Outcome".into(), "⏳ running".into()),
            ]
        );
        let ops = r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 900}),
        );
        let Op::Notice { card, .. } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!(card.fields[2].1, "✅ ok · 900 ms");
        let lines =
            upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
        assert!(
            lines[0].1.contains("🔔 notified (enforcement = notify)"),
            "{lines:?}"
        );
    }

    /// Without `notice_embeds` (the default, theseus-w4f) a notified call posts
    /// nothing of its own: its tool line says it was notified, under which
    /// setting, and then how it ended.
    #[test]
    fn without_notice_embeds_a_notified_call_rides_on_its_tool_line() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let (proposed, notice) = notified("u1", "cargo test");
        r.on_notification("tool.proposed", &proposed);
        assert_eq!(r.on_notification("policy.notified", &notice), vec![]);
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": "u1"}),
        );
        assert_eq!(
            r.tick(),
            vec![Op::Upsert {
                key: "t1:L0:tools".into(),
                content: "⏳ `proc.run` cargo test · 🔔 notified (enforcement = notify)".into(),
                buttons: Buttons::ShouldHaveAsked(vec![asked("proc.run", None)])
            }]
        );
        let ops = r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "error", "duration_ms": 900}),
        );
        assert_eq!(ops, vec![], "no card to edit");
        assert_eq!(
            r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})),
            vec![Op::Upsert {
                key: "t1:L0:tools".into(),
                content:
                    "❌ `proc.run` cargo test · 🔔 notified (enforcement = notify) · error · 900 ms"
                        .into(),
                buttons: Buttons::Keep
            }]
        );
    }

    /// A call the secret broker gives a secret says so on its tool line
    /// (theseus-dcy): the gate's word first, then what the job's start says it
    /// actually got. A notice card gets a Secrets field.
    #[test]
    fn a_call_given_a_secret_says_so_on_its_tool_line() {
        let mut r = Renderer::new(true);
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["gh", "api", "user"]}, "gate": {"result": {"gate": "allow"},
            "decision": {"posture": "notify", "granted": "gh got GH_TOKEN", "notify": {"kind": "notify",
            "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}));
        let ops = r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": "t1",
            "tool_use_id": "u1", "tool": "proc.run", "input": {"argv": ["gh", "api", "user"]},
            "summary": "run `gh api user` in /w", "kind": "notify", "setting": "enforcement = notify",
            "rule": "proc.run — notify (enforcement = notify)", "granted": "gh got GH_TOKEN"}));
        let Op::Notice { card, .. } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!(
            card.fields[3],
            ("Secrets".into(), "🔑 gh got GH_TOKEN".into())
        );
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "granted": "gh got GH_TOKEN", "withheld": []}),
        );
        let line = |r: &mut Renderer| match r.tick().first() {
            Some(Op::Upsert { content, .. }) => content.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            line(&mut r),
            "⏳ `proc.run` gh api user · 🔑 gh got GH_TOKEN · 🔔 notified (enforcement = notify)"
        );
        // A job that got nothing it was granted says so instead.
        r.on_notification(
            "tool.proposed",
            &json!({"turn_id": "t1", "tool_use_id": "u2", "tool": "proc.run",
            "input": {"argv": ["gh", "pr", "list"]}, "gate": {"decision": {"posture": "open",
            "granted": "gh got GH_TOKEN"}}}),
        );
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": "u2", "withheld": ["gh got no GH_TOKEN"]}),
        );
        assert!(line(&mut r).contains("⏳ `proc.run` gh pr list · 🔑 gh got no GH_TOKEN"));
    }

    fn asked(tool: &str, corr: Option<&str>) -> Asked {
        Asked {
            tool: tool.into(),
            correlation_id: corr.map(str::to_string),
        }
    }

    /// A notified call of any tool proposed and started, as the core sends it.
    fn ran_notified(r: &mut Renderer, use_id: &str, tool: &str, corr: &str) {
        let rule = format!("{tool} — notify (enforcement = notify)");
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": tool,
            "input": {"path": "a"}, "gate": {"result": {"gate": "allow"}, "decision": {"posture": "notify",
            "reason": rule, "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": rule}}}}));
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": use_id, "correlation_id": corr}),
        );
    }

    /// The loop's tool message among `ops`: its text and its components.
    fn tool_message(ops: &[Op]) -> (String, Buttons) {
        ops.iter()
            .find_map(|o| match o {
                Op::Upsert {
                    key,
                    content,
                    buttons,
                } if key == "t1:L0:tools" => Some((content.clone(), buttons.clone())),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no tool message in {ops:?}"))
    }

    /// Without notice embeds (theseus-sgh), a loop's tool message carries one
    /// "Should have asked…" menu: an option per distinct notified tool, each
    /// naming its newest notified call there. A tightening puts who tightened
    /// the tool on the message and takes its option away, and the last one
    /// takes the menu; an undo offers the tool again. At most 25 options.
    #[test]
    fn the_tool_message_offers_one_menu_of_its_distinct_notified_tools() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        ran_notified(&mut r, "u1", "proc.run", "act_1");
        ran_notified(&mut r, "u2", "fs.write", "act_2");
        ran_notified(&mut r, "u3", "proc.run", "act_3");
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u4", "tool": "fs.read",
            "input": {"path": "b"}, "gate": {"result": {"gate": "allow"}, "decision": {"posture": "open"}}}));
        let (_, buttons) = tool_message(&r.tick());
        assert_eq!(
            buttons,
            Buttons::ShouldHaveAsked(vec![
                asked("proc.run", Some("act_3")),
                asked("fs.write", Some("act_2"))
            ]),
            "the open read is not offered"
        );
        assert!(r.tick().is_empty(), "nothing changed, nothing sent");

        let ops = r.on_notification(
            "policy.tightened",
            &json!({"tool": "proc.run", "by": "discord:eddie"}),
        );
        let (content, buttons) = tool_message(&ops);
        assert!(
            content
                .ends_with("\n-# 🔒 `proc.run` asks first from now on: tightened by discord:eddie"),
            "{content}"
        );
        assert_eq!(
            buttons,
            Buttons::ShouldHaveAsked(vec![asked("fs.write", Some("act_2"))])
        );
        let (content, buttons) = tool_message(&r.on_notification(
            "policy.tightened",
            &json!({"tool": "fs.write", "by": "web#1"}),
        ));
        assert!(
            content.contains("-# 🔒 `fs.write` asks first from now on: tightened by web#1"),
            "{content}"
        );
        assert_eq!(buttons, Buttons::Clear, "nothing left to offer");
        let (content, buttons) = tool_message(&r.on_notification(
            "policy.untightened",
            &json!({"tool": "proc.run", "by": "sock#1"}),
        ));
        assert!(!content.contains("`proc.run` asks first"), "{content}");
        assert_eq!(
            buttons,
            Buttons::ShouldHaveAsked(vec![asked("proc.run", Some("act_3"))])
        );
        assert!(
            r.on_notification("policy.tightened", &json!({"tool": "git.log", "by": "x"}))
                .is_empty(),
            "another tool's news leaves the message alone"
        );

        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        for i in 0..30 {
            ran_notified(
                &mut r,
                &format!("u{i}"),
                &format!("tool.n{i}"),
                &format!("act_{i}"),
            );
        }
        let (content, buttons) = tool_message(&r.tick());
        assert!(content.len() <= PART_LIMIT);
        let Buttons::ShouldHaveAsked(options) = buttons else {
            panic!("{buttons:?}")
        };
        assert_eq!(options.len(), MAX_ASKED);
        assert_eq!(options[0], asked("tool.n0", Some("act_0")));
    }

    /// With notice embeds, the tool message has no menu. Each notice card has
    /// a "Should have asked" button for its call instead, and after the press
    /// the card says who tightened the tool and loses the button; an undo
    /// brings the button back.
    #[test]
    fn with_notice_embeds_each_card_gets_a_button_instead() {
        let mut r = Renderer::new(true);
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let (proposed, mut notice) = notified("u1", "cargo test");
        notice["correlation_id"] = json!("act_1");
        r.on_notification("tool.proposed", &proposed);
        let ops = r.on_notification("policy.notified", &notice);
        let Op::Notice { card, .. } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!(card.ask, Some(asked("proc.run", Some("act_1"))));
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "correlation_id": "act_1"}),
        );
        assert_eq!(
            tool_message(&r.tick()).1,
            Buttons::Keep,
            "no menu with embeds"
        );
        let ops = r.on_notification(
            "policy.tightened",
            &json!({"tool": "proc.run", "by": "discord:eddie"}),
        );
        let Some(Op::Notice { key, card }) = ops.first() else {
            panic!("{ops:?}")
        };
        assert_eq!((key.as_str(), card.ask.as_ref()), ("notice:u1", None));
        assert_eq!(
            card.fields.last(),
            Some(&(
                "Should have asked".to_string(),
                "🔒 `proc.run` asks first from now on: tightened by discord:eddie".to_string()
            ))
        );
        let (content, buttons) = tool_message(&ops);
        assert!(content.contains("tightened by discord:eddie"), "{content}");
        assert_eq!(buttons, Buttons::Keep);
        let ops = r.on_notification("policy.untightened", &json!({"tool": "proc.run"}));
        let Some(Op::Notice { card, .. }) = ops.first() else {
            panic!("{ops:?}")
        };
        assert_eq!(card.ask, Some(asked("proc.run", Some("act_1"))));
        assert!(card.fields.iter().all(|(n, _)| n != "Should have asked"));
    }

    /// The Daily Driver's proof (theseus-w4f): thirty notified calls in one
    /// loop post one tool message, edited in place as they run, and nothing
    /// else; the turn's reply is its only other message. The message keeps
    /// the newest lines, each with its notice and outcome, and counts the
    /// notices of the lines it folds.
    #[test]
    fn thirty_notified_calls_post_one_tool_message_edited_in_place() {
        let mut r = Renderer::default();
        assert_eq!(
            r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"})),
            vec![Op::Typing]
        );
        let calls: Vec<(Value, Value)> = (0..30)
            .map(|i| notified(&format!("u{i}"), &format!("echo {i}")))
            .collect();
        // One model response proposes all thirty; they then run in order.
        let mut ops = vec![];
        for (proposed, _) in &calls {
            ops.extend(r.on_notification("tool.proposed", proposed));
        }
        ops.extend(r.tick());
        for (i, (_, notice)) in calls.iter().enumerate() {
            let id = json!(format!("u{i}"));
            ops.extend(r.on_notification("policy.notified", notice));
            ops.extend(
                r.on_notification("tool.started", &json!({"turn_id": "t1", "tool_use_id": id})),
            );
            ops.extend(r.tick());
            ops.extend(r.on_notification(
                "tool.ended",
                &json!({"turn_id": "t1", "tool_use_id": id, "status": "ok", "duration_ms": 3}),
            ));
            ops.extend(r.tick());
        }
        ops.extend(r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
        assert!(
            ops.iter()
                .all(|o| matches!(o, Op::Upsert { key, .. } if key == "t1:L0:tools")),
            "{ops:?}"
        );
        assert!(
            ops.len() > 30,
            "created once, then edited as the calls run: {ops:?}"
        );
        let Some(Op::Upsert { content, .. }) = ops.last() else {
            unreachable!()
        };
        assert!(content.len() <= PART_LIMIT, "{}", content.len());
        let shown: Vec<&str> = content.lines().skip(1).collect();
        let folded = 30 - shown.len();
        assert!(
            content.starts_with(&format!(
                "-# … {folded} earlier call(s) · 🔔 {folded} notified\n"
            )),
            "{content}"
        );
        for (line, i) in shown.iter().zip(folded..) {
            assert_eq!(
                *line,
                format!("✅ `proc.run` echo {i} · 🔔 notified (enforcement = notify) · 3 ms")
            );
        }
        r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 1, "text": "All thirty ran."}),
        );
        let mut end = ended("t1", None);
        end["tool_calls"] = json!(30);
        // The reply is a post (theseus-q4v): the stream sends nothing more,
        // and the post is one message.
        assert!(upserts(&r.on_notification("turn.ended", &end)).is_empty());
        let result: TurnSubmitResult = serde_json::from_value(end).unwrap();
        let keys: Vec<String> = reply_parts("t1", &[(1, "All thirty ran.".into())], Some(&result))
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["t1:L1:p0"]);
    }

    fn waiting_write() -> ConfirmRequest {
        request(json!({"correlation_id": "act_1", "session_id": "s",
            "execution_id": "e", "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1790000000000u64}))
    }

    fn to_dm(why: &str) -> Route {
        Route::Dm {
            user: 159471966640799744,
            dm: "DM @eddie".into(),
            place: "#general".into(),
            why: why.into(),
        }
    }

    /// A place that is not a trusted channel (theseus-sgh) sends its card to
    /// the trusted user's DM and says so in one line here; the answer settles
    /// the card in the DM and the note here.
    #[test]
    fn a_card_for_an_untrusted_place_goes_to_the_dm_with_a_note_here() {
        let route = to_dm("it is not listed in [approval] channels");
        let c = card(&waiting_write(), &route, "with `theseus confirm`");
        assert!(
            c.content
                .starts_with("**Approve?** `proc.run` cargo test\nrun cargo test\n"),
            "{}",
            c.content
        );
        assert!(
            c.content.ends_with(
                "-# for #general · expires <t:1790000000:R> · you can also answer with `theseus confirm`"
            ),
            "{}",
            c.content
        );
        assert_eq!(
            card_note(&route, &c.line, "with `theseus confirm`").unwrap(),
            "🔐 Approval for `proc.run` cargo test was asked in DM @eddie: this channel is not a \
             trusted channel (it is not listed in [approval] channels)."
        );
        let done = settled(
            &Closed::new("approved", Some("discord:eddie")),
            &c.line,
            false,
        );
        assert_eq!(
            done,
            "✅ **Approved** by discord:eddie · `proc.run` cargo test"
        );
        assert_eq!(
            settled_note(&done, Some("DM @eddie")),
            "🔐 ✅ **Approved** by discord:eddie · `proc.run` cargo test (in DM @eddie)"
        );
        // The tool line here says who approved it, as it does for a card here.
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        r.on_notification(
            "confirm.requested",
            &serde_json::to_value(waiting_write()).unwrap(),
        );
        r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_1", "approved": true, "by": "discord:eddie"}),
        );
        let tools = upserts(&r.tick());
        assert!(
            tools[0].1.contains("approved by discord:eddie"),
            "{tools:?}"
        );
    }

    /// A call whose execution was cancelled before an answer reads as not
    /// run, never as declined: no one declined it (theseus-w98).
    #[test]
    fn a_waiting_call_its_cancel_closed_reads_as_not_run() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        r.on_notification(
            "confirm.requested",
            &serde_json::to_value(waiting_write()).unwrap(),
        );
        r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_1", "approved": false, "cancelled": true, "by": "cli"}),
        );
        let tools = upserts(&r.tick());
        assert!(tools[0].1.contains("· not run"), "{tools:?}");
        assert!(!tools[0].1.contains("declined"), "{tools:?}");
    }

    /// A call a `/stop` ended reads `⏹️ … stopped by <who>`, as the stop's own
    /// card says, never `❌ … cancelled`, and apart from a cancel's `🚫 … not
    /// run` (theseus-4uw): one that ran (its result says who stopped it),
    /// and one that waited for approval (the stop's `confirm.resolved`), whose
    /// not-run result keeps the line. A notice card says the same.
    #[test]
    fn a_call_a_stop_ended_reads_as_stopped_not_failed() {
        let mut r = Renderer::new(true);
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["sleep", "30"]}, "gate": {"result": {"gate": "allow"},
            "decision": {"mode": "allow", "posture": "notify", "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}));
        r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": "t1", "tool_use_id": "u1",
            "tool": "proc.run", "input": {"argv": ["sleep", "30"]}, "summary": "run `sleep 30` in /w",
            "kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}));
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "correlation_id": "act_9"}),
        );
        let ops = r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "cancelled",
                    "duration_ms": 2100, "stopped_by": "discord:eddie"}),
        );
        let Op::Notice { card, .. } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!(card.fields[2].1, "⏹️ stopped by discord:eddie · 2100 ms");
        // A waiting call the stop declined.
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u2", "tool": "fs.write",
            "input": {"path": "a.txt", "content": "x"}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        let write = request(json!({"correlation_id": "act_2", "session_id": "s",
            "execution_id": "e", "tool": "fs.write", "input": {"path": "a.txt", "content": "x"},
            "reason": "write a.txt", "by": "operator", "requested_at_ms": 1,
            "expires_at_ms": 1790000000000u64}));
        r.on_notification("confirm.requested", &serde_json::to_value(write).unwrap());
        r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_2", "approved": false, "stopped": true, "by": "discord:eddie"}),
        );
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u2", "status": "cancelled", "duration_ms": 0}),
        );
        let lines =
            upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
        let text = &lines[0].1;
        assert!(
            text.contains("⏹️ `proc.run` sleep 30 · 🔔 notified (enforcement = notify) · stopped by discord:eddie"),
            "{text}"
        );
        assert!(
            text.contains("⏹️ `fs.write` a.txt · stopped by discord:eddie"),
            "{text}"
        );
        assert!(
            !text.contains("❌") && !text.contains("cancelled") && !text.contains("declined"),
            "{text}"
        );
    }

    /// A call approved after its turn parked runs in the next turn, whose
    /// events name that turn: its line, in the turn that proposed it, still
    /// says how it ran and ended (here a stop), not `👍 approved` forever
    /// (theseus-4uw's live check found it so).
    #[test]
    fn an_approved_call_that_runs_in_a_later_turn_updates_its_own_line() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["sleep", "30"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        let sleep = request(json!({"correlation_id": "act_3", "session_id": "s",
            "execution_id": "e", "tool": "proc.run", "input": {"argv": ["sleep", "30"]},
            "reason": "run sleep 30", "by": "operator", "requested_at_ms": 1,
            "expires_at_ms": 1790000000000u64}));
        r.on_notification("confirm.requested", &serde_json::to_value(sleep).unwrap());
        r.on_notification("turn.ended", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_3", "approved": true, "by": "the CLI"}),
        );
        let line = |r: &mut Renderer| upserts(&r.tick())[0].1.clone();
        assert!(
            line(&mut r).contains("👍 `proc.run` sleep 30 · approved by the CLI"),
            "the answer shows first"
        );
        // The continuation runs it.
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t2"}));
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t2", "tool_use_id": "u1", "correlation_id": "act_3"}),
        );
        assert!(
            line(&mut r).contains("⏳ `proc.run` sleep 30"),
            "it runs, on its own line"
        );
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t2", "tool_use_id": "u1", "status": "cancelled",
                    "duration_ms": 7853, "stopped_by": "the CLI"}),
        );
        let ended = line(&mut r);
        assert!(
            ended.contains("⏹️ `proc.run` sleep 30 · stopped by the CLI"),
            "{ended}"
        );
    }

    /// A budget question takes the same route as a tool call.
    #[test]
    fn a_budget_question_for_an_untrusted_place_goes_to_the_dm_too() {
        let route = Route::Dm {
            user: 7,
            dm: "DM @eddie".into(),
            place: "#general".into(),
            why: "cannot be verified".into(),
        };
        let req = request(json!({"correlation_id": "act_b", "session_id": "s",
            "execution_id": "e", "tool": "budget.reset", "input": {}, "by": "operator", "requested_at_ms": 1,
            "expires_at_ms": 0, "reason": "This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?",
            "budget": {"spent_usd": 99.48, "limit_usd": 100.0, "needed_usd": 1.0, "lifetime_usd": 212.4}}));
        let c = card(&req, &route, ELSEWHERE);
        assert!(
            c.content
                .contains("\n-# For #general. Approve resets its spend"),
            "{}",
            c.content
        );
        assert!(
            c.content
                .ends_with("You can also answer in the web UI or with `theseus confirm`."),
            "{}",
            c.content
        );
        let said = card_note(&route, &c.line, ELSEWHERE).unwrap();
        assert!(
            said.starts_with(
                "🔐 Approval for spend reset ($99.48 of the $100 limit) was asked in DM @eddie"
            ),
            "{said}"
        );
    }

    /// Not trusted, and no trusted DM: no card on Discord, only a note that
    /// says where to answer (or that nowhere here can).
    #[test]
    fn with_no_trusted_dm_the_place_gets_only_a_note() {
        for (elsewhere, tail) in [
            (
                "in the web UI or with `theseus confirm`",
                ": answer in the web UI or with `theseus confirm`.",
            ),
            ("", ", and no trusted channel is bound here to answer it."),
        ] {
            let route = Route::Elsewhere {
                why: "it is not listed in [approval] channels".into(),
            };
            let c = card(&waiting_write(), &route, elsewhere);
            assert_eq!(
                card_note(&route, &c.line, elsewhere).unwrap(),
                format!(
                    "🔐 `proc.run` cargo test waits for approval, and this channel is not a \
                     trusted channel (it is not listed in [approval] channels){tail}"
                )
            );
            let done = settled(&Closed::new("declined", Some("sock#3")), &c.line, false);
            assert_eq!(
                settled_note(&done, None),
                "🔐 ❎ **Declined** by sock#3 · `proc.run` cargo test"
            );
        }
        assert!(card_note(&Route::Here, "x", ELSEWHERE).is_none());
    }

    /// With `[approval]`, a card names only the trusted local surfaces; with
    /// none, it names none. Without `[approval]` it reads as before.
    #[test]
    fn a_card_names_where_else_it_can_be_answered() {
        let text = |elsewhere: &str| {
            card(&waiting_write(), &Route::Here, elsewhere)
                .content
                .lines()
                .last()
                .unwrap()
                .to_string()
        };
        assert_eq!(
            text(ELSEWHERE),
            "-# expires <t:1790000000:R> · you can also answer in the web UI or with `theseus confirm`"
        );
        assert_eq!(
            text("in the web UI"),
            "-# expires <t:1790000000:R> · you can also answer in the web UI"
        );
        assert_eq!(text(""), "-# expires <t:1790000000:R>");
    }

    #[test]
    fn a_floor_confirm_says_so_and_names_the_reason() {
        let reason = "read /home/x/.openclaw-1password-service-token: fs.read — approve (floor: /home/x/.openclaw-1password-service-token is Theseus's own state or the 1Password token)";
        let c = card(
            &request(json!({"correlation_id": "act_9",
            "session_id": "s", "execution_id": "e", "tool": "fs.read",
            "input": {"path": "~/.openclaw-1password-service-token"}, "reason": reason, "by": "operator",
            "requested_at_ms": 0, "expires_at_ms": 60_000, "floor": true})),
            &Route::Here,
            ELSEWHERE,
        );
        assert!(c.content.starts_with(FLOOR_ASK), "{}", c.content);
        assert!(
            c.content.contains("fs.read — approve (floor: "),
            "{}",
            c.content
        );
        assert!(c.line.ends_with("(floor)"), "{}", c.line);
    }

    /// A turn the stream never saw (a continuation that ended before the
    /// binding was back) is its reply's post alone: its text and footer.
    #[test]
    fn a_turn_seen_only_at_its_end_is_its_reply() {
        let mut r = Renderer::default();
        let mut end = ended("t9", None);
        end["continuation"] = json!(true);
        assert!(r.on_notification("turn.ended", &end).is_empty());
        assert!(!r.busy());
        let parts = reply_parts(
            "t9",
            &[(0, "Done. The command printed `done`.".into())],
            Some(&result_of(end)),
        );
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, "t9:L0:p0");
        assert!(
            parts[0]
                .1
                .starts_with("Done. The command printed `done`.\n-# sonnet"),
            "{}",
            parts[0].1
        );
        assert!(parts[0].1.contains("continued"));
    }

    /// How a card reads when its question closed without an answer.
    #[test]
    fn a_card_closed_without_an_answer_says_what_closed_it() {
        assert_eq!(
            settled(&Closed::new("superseded", None), "`fs.write` a", false),
            "⏭️ **Not run**: a new message replaced this request. `fs.write` a"
        );
        assert_eq!(
            settled(&Closed::new("ended", None), "`fs.write` a", false),
            "⏹️ **Closed**: the session's work ended · `fs.write` a"
        );
        // `/stop` (W1): the question closed, and the conversation goes on.
        assert_eq!(
            settled(
                &Closed::new("stopped", Some("discord:eddie")),
                "`fs.write` a",
                false
            ),
            "⏹️ **Not run**: discord:eddie stopped this session's work · `fs.write` a"
        );
        let other = Closed {
            how: "closed".into(),
            by: None,
            note: Some("its question is no longer in the store".into()),
        };
        assert_eq!(
            settled(&other, "`fs.write` a", false),
            "⏹️ **Closed**: its question is no longer in the store · `fs.write` a"
        );
    }

    #[test]
    fn a_declined_call_keeps_saying_who_declined_it() {
        // `denied` is the status name before theseus-8az.
        for status in ["declined", "denied"] {
            let mut r = Renderer::default();
            r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
            r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
                "input": {"path": "/etc/hosts"}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
            r.on_notification("confirm.requested", &json!({"correlation_id": "act_9",
                "session_id": "s", "execution_id": "e", "tool": "fs.read", "input": {"path": "/etc/hosts"},
                "reason": "read /etc/hosts: fs.read — approve (/etc/hosts is outside the workspace roots: /w)",
                "by": "operator", "requested_at_ms": 0, "expires_at_ms": 60_000}));
            r.on_notification(
                "confirm.resolved",
                &json!({"correlation_id": "act_9", "approved": false, "by": "eddie"}),
            );
            r.on_notification(
                "tool.ended",
                &json!({"turn_id": "t1", "tool_use_id": "u1", "status": status, "duration_ms": 0}),
            );
            let ops = upserts(&r.on_notification("turn.ended", &ended("t1", None)));
            assert!(
                ops.contains(&(
                    "t1:L0:tools".into(),
                    "👎 `fs.read` /etc/hosts · declined by eddie".into()
                )),
                "{status}: {ops:?}"
            );
        }
    }

    #[test]
    fn a_call_that_never_ran_and_a_failed_turn_say_so() {
        // `denied` is the status name before theseus-8az.
        for status in ["declined", "denied"] {
            let mut r = Renderer::default();
            r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
            r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
                "input": {"path": "~/.ssh/config"}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
            r.on_notification(
                "tool.ended",
                &json!({"turn_id": "t1", "tool_use_id": "u1", "status": status, "duration_ms": 0}),
            );
            let ops = upserts(&r.on_notification(
                "turn.failed",
                &json!({"session_id": "s", "turn_id": "t1", "class": "overloaded", "error": "try later"}),
            ));
            assert!(
                ops.contains(&(
                    "t1:L0:tools".into(),
                    "🚫 `fs.read` ~/.ssh/config · not run".into()
                )),
                "{status}: {ops:?}"
            );
            // The failure itself is a post (theseus-q4v).
            assert_eq!(ops.len(), 1, "{ops:?}");
            assert!(!r.busy());
        }
        assert_eq!(
            failed("overloaded", "try later", None, 1),
            "⚠️ **Turn failed** (overloaded): try later"
        );
        // A run of failures says what follows (theseus-ljr).
        assert!(failed("overloaded", "try later", Some("backoff"), 1)
            .ends_with("Retrying with backoff while it lasts. No more notices for it: the reply comes when the provider answers."));
        assert!(failed("invalid_request", "bad", Some("park"), 2).ends_with(
            "Failed 2 times in a row, so nothing retries it now: your next message does."
        ));
        assert!(failed("over_limit", "too big", Some("park"), 1)
            .ends_with("\n-# Nothing retries it now: your next message does."));
    }

    #[test]
    fn long_text_splits_under_the_limit_and_repairs_fences() {
        let code: String = (0..300).map(|i| format!("let x{i} = {i};\n")).collect();
        let text = format!("Here:\n```rust\n{code}```\nThat is all.");
        let parts = split_text(&text, PART_LIMIT);
        assert!(parts.len() > 1);
        for p in &parts {
            assert!(p.len() <= DISCORD_LIMIT, "{}", p.len());
            assert_eq!(
                p.matches("```").count() % 2,
                0,
                "each part's fences balance: {p}"
            );
        }
        assert!(parts[1].starts_with("```rust\n"));
        // Appending never moves an earlier cut.
        let longer = split_text(&format!("{text} More."), PART_LIMIT);
        assert_eq!(parts[..parts.len() - 1], longer[..parts.len() - 1]);
    }

    #[test]
    fn summaries_pick_the_argument_a_person_looks_for() {
        assert_eq!(
            summarize("proc.run", &json!({"argv": ["cargo", "test", "-q"]})),
            "cargo test -q"
        );
        assert_eq!(
            summarize("fs.grep", &json!({"pattern": "fn main", "path": "src"})),
            "fn main in src"
        );
        assert_eq!(summarize("fs.read", &json!({"path": "a`b"})), "a'b");
        assert!(summarize("fs.write", &json!({"path": "x", "content": "secret body"})) == "x");
        // The web tools (DD5): the URL, or the quoted query.
        assert_eq!(
            summarize(
                "http.fetch",
                &json!({"url": "https://doc.rust-lang.org/std/", "max_bytes": 4096})
            ),
            "https://doc.rust-lang.org/std/"
        );
        assert_eq!(
            summarize(
                "web.search",
                &json!({"query": "ignore WalkParallel", "count": 3})
            ),
            "\"ignore WalkParallel\""
        );
    }

    /// A task's report (DD7) is one message that names the task: a long last
    /// message is clipped to fit, and a cancel or a failure says so.
    #[test]
    fn a_tasks_report_is_one_message_that_names_it() {
        let body = |outcome: &str, reason: Option<&str>| {
            json!({"kind": "report", "task": "ses_x", "short": "a1b2c3", "title": "Run the gate",
                   "outcome": outcome, "reason": reason, "spent_usd": 0.0123, "limit_usd": 2.5,
                   "turns": 3, "elapsed_ms": 64_000})
        };
        let done = report(&body("complete", None), Some("The gate passed."));
        assert_eq!(
            done,
            "📋 **Task `a1b2c3` finished** · Run the gate\nThe gate passed.\n\
             -# 3 turns · $0.0123 of $2.50 · 1 min 4 s"
        );
        let long = report(&body("complete", None), Some(&"word ".repeat(2_000)));
        assert!(long.chars().count() <= DISCORD_LIMIT, "{}", long.len());
        assert!(long.ends_with("-# 3 turns · $0.0123 of $2.50 · 1 min 4 s"));
        let stopped = report(&body("cancelled", Some("cancelled by discord:eddie")), None);
        assert!(
            stopped.starts_with(
                "⏹️ **Task `a1b2c3` stopped** · Run the gate: cancelled by discord:eddie\n-# 3 turns"
            ),
            "{stopped}"
        );
        let failed = report(&body("failed", Some("provider_overloaded (529)")), None);
        assert!(
            failed.starts_with("⚠️ **Task `a1b2c3` failed** · Run the gate: provider_overloaded"),
            "{failed}"
        );
    }

    /// A task's question names the task on its card, and so on its settle.
    #[test]
    fn a_tasks_card_names_the_task() {
        let req = request(json!({
            "correlation_id": "act_1", "session_id": "ses_x", "execution_id": "exe_x",
            "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 60_000,
            "task": {"task_id": "ses_xa1b2c3", "short": "a1b2c3", "title": "Run the gate"}
        }));
        let card = card(&req, &Route::Here, "");
        assert!(
            card.content
                .contains("task `a1b2c3`: `proc.run` cargo test"),
            "{}",
            card.content
        );
        assert_eq!(card.line, "task `a1b2c3`: `proc.run` cargo test");
    }

    /// `/tasks` (DD7): each task by its short id, with its state, what it
    /// waits on, its spend of its limit, its age, and its title.
    #[test]
    fn slash_tasks_lists_each_with_state_and_spend() {
        let t = |short: &str, state: &str, waiting: Option<&str>, asks: u32| {
            theseus_protocol::TaskInfo {
                task_id: format!("ses_{short}"),
                short: short.into(),
                title: Some("Run the gate".into()),
                state: state.into(),
                waiting_on: waiting.map(str::to_string),
                spent_usd: 0.0123,
                limit_usd: 2.5,
                pending_confirms: asks,
                created_at_ms: 1_000,
                ..Default::default()
            }
        };
        let text = tasks(
            &[
                t("a1b2c3", "waiting", Some("an approval"), 1),
                t("d4e5f6", "complete", None, 0),
            ],
            181_000,
        );
        assert_eq!(
            text,
            "**Tasks here** (2)\n\
             • `a1b2c3` waiting on an approval · **needs you** · $0.0123 of $2.50 · 3 min · Run the gate\n\
             • `d4e5f6` complete · $0.0123 of $2.50 · 3 min · Run the gate\n\
             -# `/cancel <task>` stops one."
        );
        assert!(tasks(&[], 0).starts_with("No tasks here yet"));
    }

    /// `/wakes` (DD8): each wake by its short id, its time in the reader's
    /// zone, and its note's first line; one waiting for a busy turn says so.
    #[test]
    fn slash_wakes_lists_each_with_its_time_and_note() {
        let w = |short: &str, state: &str| theseus_protocol::WakeInfo {
            short: short.into(),
            due_at_ms: 1_790_798_700_000,
            note: "check the build\nthen the tests".into(),
            state: state.into(),
            ..Default::default()
        };
        assert_eq!(
            wakes(&[w("3f9a1c", "waiting"), w("b4f566", "running")]),
            "**Wakes here** (2)\n\
             • `3f9a1c` <t:1790798700:t> (<t:1790798700:R>) · check the build\n\
             • `b4f566` <t:1790798700:t> (<t:1790798700:R>) · runs when the current turn ends · \
             check the build\n\
             -# `/cancel <id>` cancels one."
        );
        assert!(wakes(&[]).starts_with("No wakes pending here"));
    }

    /// A wake's turn's reply opens with the wake's line, on its first part;
    /// a part with no room left keeps its text.
    #[test]
    fn a_wakes_reply_opens_with_the_wakes_line() {
        let mut parts = vec![
            ("t1:L0:p0".to_string(), "The build is green.".to_string()),
            ("t1:footer".to_string(), "-# glm".to_string()),
        ];
        wake_header(
            &mut parts,
            &["⏰ wake (set 13:05): check the build".to_string()],
        );
        assert_eq!(
            parts[0].1,
            "-# ⏰ wake (set 13:05): check the build\nThe build is green."
        );
        assert_eq!(parts[1].1, "-# glm");
        let full = "x".repeat(1_990);
        let mut parts = vec![("t2:L0:p0".to_string(), full.clone())];
        wake_header(&mut parts, &["⏰ wake (set 13:05): n".to_string()]);
        assert_eq!(parts[0].1, full, "no room: the text as it was");
        let mut none = vec![("t3:L0:p0".to_string(), "hi".to_string())];
        wake_header(&mut none, &[]);
        assert_eq!(none[0].1, "hi");
    }

    /// `f`'s value, or `None` if it has not returned within `secs`: a split
    /// that stops moving through its text never returns, and a test of it
    /// must fail, not hang.
    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
    }

    /// A budget smaller than the next character, or a carried fence line as
    /// long as the limit, left no room to cut: the cut fell at 0, and the
    /// split pushed empty parts forever without moving (theseus-s68, found by
    /// the property test below). Each part now moves the text on, and a
    /// reopened fence is at most `MAX_FENCE` bytes.
    #[test]
    fn a_split_always_moves_through_multibyte_text_and_long_fences() {
        let parts = within(2, || split_text("中", 4)).expect("split_text(\"中\", 4) returned");
        assert_eq!(parts.concat(), "中");
        let fence = format!("```{}", "x".repeat(PART_LIMIT));
        let text = format!("{fence}\n{}", "中".repeat(1000));
        let parts = within(2, move || split_text(&text, PART_LIMIT))
            .expect("the long fence's split returned");
        for p in &parts {
            assert!(p.len() <= PART_LIMIT, "{}", p.len());
        }
        let kept: usize = parts.iter().map(|p| p.matches('中').count()).sum();
        assert_eq!(kept, 1000, "every character is in some part, once");
    }

    fn fenced() -> impl proptest::strategy::Strategy<Value = String> {
        use proptest::prelude::*;
        let piece = prop_oneof![
            3 => proptest::sample::select(vec![
                "```", "```rust", "\n", " ", "中", "😀", "é", "a", "\n```\n", "  ```py\n",
            ])
            .prop_map(str::to_string),
            1 => any::<String>(),
            1 => "x{0,300}",
        ];
        proptest::collection::vec(piece, 0..40).prop_map(|v| v.concat())
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1500,
            failure_persistence: None,
            ..proptest::test_runner::Config::default()
        })]

        /// Any text splits, at any limit: every part moves the text on, a
        /// part is never empty, it fits a real limit, and appending text
        /// never moves an earlier cut.
        #[test]
        fn any_text_splits_and_keeps_its_earlier_cuts(
            text in fenced(),
            more in fenced(),
            limit in proptest::prop_oneof![1usize..64, 64usize..600, proptest::strategy::Just(PART_LIMIT)],
        ) {
            let split = within(5, move || {
                (split_text(&text, limit), split_text(&format!("{text}{more}"), limit))
            });
            proptest::prop_assert!(split.is_some(), "split_text did not return (limit {})", limit);
            let (parts, longer) = split.unwrap();
            proptest::prop_assert!(parts.iter().all(|p| !p.is_empty()));
            if limit >= 64 {
                for p in &parts {
                    proptest::prop_assert!(p.len() <= limit, "a part of {} bytes, over {}", p.len(), limit);
                }
            }
            if parts.len() > 1 {
                proptest::prop_assert_eq!(&parts[..parts.len() - 1], &longer[..parts.len() - 1]);
            }
        }
    }
}
