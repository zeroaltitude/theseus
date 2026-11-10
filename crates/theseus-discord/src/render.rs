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
//!   of deltas costs one edit. A turn's tool messages are bounded
//!   (theseus-8wm): its first `STEP_LOOPS - 1` loops keep one each, and every
//!   loop from there on shares the step's message, which goes out when it is
//!   due (a loop's end, a call waiting for or getting an answer, the turn's
//!   end) rather than at each call's every state. So a builder's step of
//!   fifty tool loops costs its place six messages, not fifty.
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
pub(crate) mod answer_v2;
mod approval;
mod ask;
mod board;
pub(crate) mod board_v2;
pub(crate) mod card_v2;
pub(crate) mod cite;
mod deferred;
mod held;
mod heldquestion;
pub(crate) mod history;
pub(crate) mod horizon;
mod preview;
pub(crate) mod process;
mod reply;
pub(crate) mod snapshot;
mod step;
pub(crate) mod task_view;
#[cfg(test)]
mod tests_pin;
pub(crate) mod v2;
mod wakes;
pub use approval::strip_ask;
pub use ask::{details as question_details, needs_you};
pub use board::{board, tasks_here, BOARD_HEAD, BOARD_KEY};
pub use reply::{reply_parts, thread_tag, wake_header};
use step::{render_turn, step_loop, STEP_ROLL};
pub use wakes::wakes;

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
    /// One button per option of an `owner_ask` question (theseus-j7cje).
    Ask(String, Vec<String>),
    /// A needs-you card (theseus-suvpe): the question's choices, else Approve
    /// and Decline, and always Details.
    Card(String, Vec<String>),
    /// Approve, the question's grant, and Decline (theseus-drjs).
    ConfirmGrant(String, theseus_protocol::grants::GrantOffer),
    /// A "Should I have asked?" select menu (theseus-sgh): an option per notified tool, `MAX_ASKED`.
    ShouldHaveAsked(Vec<Asked>),
    /// A Jev notice's right / wrong / noise, for this judgment.
    JevLabel(String),
    /// A reminder's Done and three snoozes, for this wake (theseus-8ux6x).
    Reminder(String),
    /// A routine's or a watch's post: this wake, and the answers `(id, label)` it offers (theseus-j90w2).
    Horizon(String, Vec<(String, String)>),
    /// An expired card's "Ask again" for this question (theseus-27p6w).
    Again(String),
    /// A proposed seal's Seal this thread and Not needed, for this thread (theseus-f2g0).
    Seal(String),
    /// A proposed link's Link these threads and Not needed, for this pair (theseus-98bz.12).
    Link(String, String),
    /// A mint's card (theseus-xzxqv): Undo, for this write (the judgment's id).
    OntoUndo(String),
    /// A new persona's card (theseus-utfnl): Write its soul and Decline, for this persona.
    PersonaSoul(String),
    /// A self note's Drop for each started item `(id, title)` and its Halt (theseus-o1h3f.7).
    SelfNote(Vec<(String, String)>),
    /// The self-halt note's Resume (theseus-6s7ll).
    SelfResume,
    /// A capture's card (theseus-slvl.5): `(mode, id, everywhere)`, a kept record's Undo or an ask's keep and not.
    Standing(String, String, bool),
    /// A live owner-card question (theseus-6f8t0): `(ask, proposal id)`.
    CardQuestion(String, String),
    /// A content label's one Remove button, for this `(session, flag)` (theseus-9ymz0).
    ContentRemove(String, String),
    /// Accept and Decline (39b's layer-1 card): Approve's and Decline's ids.
    Accept(String),
    /// A reply's cited sources, for the session whose recall gave them, and the turn whose
    /// message it offers to make a task (theseus-bh3t.5).
    Sources(String, Vec<cite::Source>, Option<String>),
    /// Remove every component.
    Clear,
    /// Components V2 (theseus-2j2ka): the write's `content` is the components as JSON,
    /// sent with the V2 flag and no text (`courier/v2.rs`).
    V2,
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
    /// A "Make actions like this ask in the future" button for this call;
    /// None once its tool asks first (theseus-sgh).
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

/// Where a place's approval cards go (the place rule, theseus-zmgb).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Route {
    /// Here: the place is private, so the owner's answer here counts.
    #[default]
    Here,
    /// To the DM with an owner, `user`, which `dm` names (`DM @zeroaltitude`); the
    /// place (`place`, as `#general`) gets a one-line note that says so and
    /// why an answer here would not count (`why`).
    Dm {
        user: u64,
        dm: String,
        place: String,
        why: String,
    },
    /// Nowhere on Discord: the place is shared, and no DM with an owner is
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
    /// It runs in L1 (M4 17b), and what it may reach (18c): `no network`, or
    /// `egress: github.com:443`; its summary gains what it wrote to scratch,
    /// and the hosts it reached.
    l1: Option<String>,
    /// Its notice's score once its judgment lands (M5 step 24): `risk 12%
    /// (shadow)`.
    risk: Option<String>,
    /// Its end comes back late, in a turn of its own: it went to the
    /// background, or it waited for an answer. theseus-9zif keeps its turn
    /// alive for exactly that, so the step's shared message must not fold
    /// its line away (theseus-8wm): that line is the only place the late
    /// end has to land.
    late: bool,
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
    /// The step's message is due (theseus-8wm): a loop ended, a call waits
    /// for an answer or got one, or the turn ended. A call's own states in
    /// between ride along at the next of those, so a long step costs the
    /// place about an edit a loop instead of one for every state it passes
    /// through. A turn whose loops all keep their own message is unaffected.
    step_due: bool,
}

impl TurnView {
    /// Does the turn roll its later loops into the step's message?
    fn rolls(&self) -> bool {
        self.loops
            .keys()
            .next_back()
            .is_some_and(|li| *li > STEP_ROLL)
    }
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

/// Where else an approval can be answered: the owner's own surfaces, the web
/// UI on (`theseus_core::approval::elsewhere`).
pub const ELSEWHERE: &str = "in the web UI or with `theseus answer`";

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
                t.step_due = true;
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
                    step_due: false,
                });
                self.drop_past_recent();
                vec![Op::Typing]
            }
            // A loop's first text shows at once, not at the next tick
            // (theseus-ck0n): the rest of its stream waits for the tick, as
            // Discord limits a message's edits.
            Event::ModelDelta(d) => {
                let Some(t) = self.turn_mut(turn_id) else {
                    return vec![];
                };
                let lv = t.loops.entry(d.loop_index).or_default();
                let first = lv.text.trim().is_empty();
                lv.text.push_str(&d.text);
                t.dirty = true;
                if first && !lv.text.trim().is_empty() {
                    return self.tick();
                }
                vec![]
            }
            // The loop's whole answer as its stream ends (theseus-ck0n),
            // shown at once: before the settle's frame and the reply's post,
            // whose edit only seals it and adds the footer. Its text is the
            // whole text, so a dropped delta never shows.
            Event::ModelAnswered(d) => {
                let Some(t) = self.turn_mut(turn_id) else {
                    return vec![];
                };
                if t.ended {
                    return vec![];
                }
                let lv = t.loops.entry(d.loop_index).or_default();
                if !d.text.trim().is_empty() {
                    lv.text.clone_from(&d.text);
                }
                t.dirty = true;
                self.tick()
            }
            Event::ToolProposed(p) => {
                let decision = p.gate.decision.as_ref();
                // An AWS call's line names its operation, region, and
                // account, from its plan (row 29, C1).
                let aws = p.gate.plan.as_ref().and_then(|pl| pl.aws.as_ref());
                let line = ToolLine {
                    tool_use_id: p.tool_use_id.clone(),
                    tool: p.tool.clone(),
                    summary: aws.map_or_else(|| summarize(&p.tool, &p.input), aws_summary),
                    correlation_id: None,
                    state: ToolState::Proposed,
                    notice: decision
                        .and_then(|d| d.notify.as_ref())
                        .map(|n| n.setting.clone()),
                    granted: decision.and_then(|d| d.granted.clone()),
                    l1: (decision.and_then(|d| d.class.as_deref()) == Some("l1")).then(|| {
                        theseus_protocol::sandbox::reach(&theseus_protocol::sandbox::egress_in(
                            &p.gate.proposal.policy_context,
                        ))
                    }),
                    risk: None,
                    late: false,
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
                let l1 = (s.class.as_deref() == Some("l1"))
                    .then(|| theseus_protocol::sandbox::reach(s.egress.as_deref().unwrap_or(&[])));
                self.update_tool(turn_id, &s.tool_use_id, |l| {
                    l.state = ToolState::Running;
                    l.l1 = l1.clone().or(l.l1.take());
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
                let verified = theseus_protocol::cancel::note(t.verified.as_deref());
                let (mut ops, key) = (vec![], self.notice_key(use_id));
                if let Some((card, _)) = self.notices.get_mut(use_id) {
                    let outcome = match (status, &stopped) {
                        (_, Some(by)) => format!("⏹️ stopped by {by}{verified} · {ms} ms"),
                        ("ok", _) => format!("✅ ok · {ms} ms"),
                        ("background", _) => "⏳ running in the background".to_string(),
                        (other, _) => format!("❌ {other} · {ms} ms"),
                    };
                    if let Some(f) = card.fields.iter_mut().find(|(n, _)| n == "Outcome") {
                        f.1 = outcome;
                    }
                    ops.push(Op::Notice {
                        key,
                        card: card.clone(),
                    });
                }
                let ended: Vec<String> = t.scratch.iter().chain(&t.reached).cloned().collect();
                self.update_tool(turn_id, use_id, |l| {
                    l.summary.extend(ended.iter().map(|s| format!(" · {s}")));
                    l.state = match (status, &stopped) {
                        (_, Some(by)) => ToolState::Stopped {
                            by: format!("{by}{verified}"),
                        },
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
                    // Its end will come back in a turn of its own, so the
                    // step's message keeps its line from here on.
                    l.late |= matches!(l.state, ToolState::Background);
                });
                ops
            }
            // A notified call's score (M5 step 24): its tool line gains it,
            // and its notice card, when it posted one.
            Event::JudgeScored(j) => {
                let line = j.line();
                self.update_tool(turn_id, &j.tool_use_id, |l| l.risk = Some(line.clone()));
                let key = self.notice_key(&j.tool_use_id);
                let Some((card, _)) = self.notices.get_mut(&j.tool_use_id) else {
                    return vec![];
                };
                match card.fields.iter_mut().find(|(n, _)| n == "Risk") {
                    Some(f) => f.1 = line,
                    None => card.fields.push(("Risk".into(), line)),
                }
                vec![Op::Notice {
                    key,
                    card: card.clone(),
                }]
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
                    key: self.notice_key(&use_id),
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
                    t.step_due |= shows;
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
                        key: self.notice_key(&use_id),
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
                        // Its answer can come in a later turn, so the step's
                        // message keeps its line from here on.
                        l.late = true;
                    }
                    t.dirty = true;
                    // A call waiting for an answer is shown at once, however
                    // deep in the step it is (theseus-8wm).
                    t.step_due = true;
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
                                t.step_due = true;
                            }
                        }
                    }
                }
                vec![]
            }
            Event::LoopEnded(_) => {
                if let Some(t) = self.turn_mut(turn_id) {
                    t.dirty = true;
                    t.step_due = true;
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
                        t.step_due = true;
                        self.tick()
                    }
                    None => vec![],
                }
            }
            _ => vec![],
        }
    }

    /// Emit the messages whose rendered text or "should have asked" menu
    /// changed since Discord last saw them.
    pub fn tick(&mut self) -> Vec<Op> {
        let mut rendered = Vec::new();
        let menus = !self.notice_embeds;
        for t in self.turns.iter_mut().filter(|t| t.dirty) {
            let due = t.step_due;
            rendered.extend(
                render_turn(t, &self.tightened, menus)
                    .into_iter()
                    .map(|m| (due, m)),
            );
            t.dirty = false;
            t.step_due = false;
        }
        let mut ops = Vec::new();
        for (due, m) in rendered {
            // The step's message waits for its next due moment (a loop's end,
            // an answer, the turn's end); its first post never waits, so the
            // place sees the step go on (theseus-8wm).
            if m.rolled && !due && self.emitted.contains_key(&m.key) {
                continue;
            }
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

    /// A notice card's "Make actions like this ask in the future" button, or,
    /// once its tool asks first, a line that says who tightened it.
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
                // The turn is over, so no loop's end is coming to carry this
                // onto the step's message: a late result is due now
                // (theseus-8wm).
                t.step_due |= t.ended;
            }
        }
    }
}

/// The field a notice card gains once its tool asks first.
const TIGHTENED_FIELD: &str = "Asks first now";

// ---------------------------------------------------------------- posts

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
    if let Some(c) = &req.change {
        return board::change_card(req, c, &task, route, elsewhere);
    }
    if req.tool == theseus_protocol::work::ask::OWNER_ASK {
        return ask::card(req, &task, route, elsewhere);
    }
    match &req.plain {
        Some(p) => approval::card(req, p, route, elsewhere),
        None => approval::classic(req, &task, route, elsewhere),
    }
}

/// The note a place gets when its card goes elsewhere: to an owner's DM, or
/// nowhere on Discord. None when the card is here.
pub fn card_note(route: &Route, line: &str, elsewhere: &str) -> Option<String> {
    match route {
        Route::Here => None,
        Route::Dm { dm, why, .. } => {
            Some(format!("🔐 Approval for {line} was asked in {dm}: {why}."))
        }
        Route::Elsewhere { why } => {
            let answer = match elsewhere {
                "" => ", and no DM with an owner is bound here to answer it".to_string(),
                e => format!(": answer {e}"),
            };
            Some(format!("🔐 {line} waits for approval: {why}{answer}."))
        }
    }
}

/// How a card reads once its question closed: who answered, or what closed it.
pub fn settled(closed: &Closed, line: &str, budget: bool) -> String {
    // An agent's answer says so (theseus-5rvss).
    let by = closed
        .by
        .as_deref()
        .map_or_else(|| "the operator".into(), theseus_protocol::by_words);
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
        "answered" | "asked_expired" => ask::settled(closed, line),
        "withdrawn" => format!("↩️ **Withdrawn**: {note} · {line}"),
        "ended" => format!("⏹️ **Closed**: the session's work ended · {line}"),
        // `/stop` (W1): the question closed, and the conversation goes on.
        "stopped" => format!("⏹️ **Not run**: {by} stopped this session's work · {line}"),
        // Nobody answered by the time the card gave (theseus-830).
        "expired" => {
            format!("⌛ **Expired**: nobody answered within {note}, so it did not run · {line}")
        }
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
        // Posted by no notice since theseus-36re, but a post written before.
        Some("backoff") if class == "stopping" => {
            format!("{head}\n-# Not sent as the daemon stopped: its next start retries it.")
        }
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
        let state = deferred::state_word(t);
        let asks = if t.pending_confirms > 0 {
            " · **needs you**"
        } else {
            ""
        };
        // The pieces it was started from (M5 27).
        let mut pieces = t
            .arrangement
            .as_ref()
            .map_or(String::new(), |a| format!(" · 📎 {}", a.pieces.len()));
        // A check names what it checks (M5 28a).
        if let Some(c) = &t.check {
            pieces.push_str(&format!(" · 🔍 `{}`", c.checked_short));
        }
        lines.push(format!(
            "• `{}` {state}{asks}{pieces} · {} of {} · {age} · {}",
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
pub fn report(body: &Value, said: Option<&str>, guests: usize, place: Option<u64>) -> String {
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
    // The task's own post or channel, so a tap opens it.
    let head = match place {
        Some(id) => format!("{head} · <#{id}>"),
        None => head,
    };
    // A check's basis, beside its report (M5 28a).
    let head = match body["check"].as_str() {
        Some(c) => format!("{head}\n-# {}", clip(c, 300)),
        None => head,
    };
    let turns = body["turns"].as_u64().unwrap_or(0);
    let secs = body["elapsed_ms"].as_u64().unwrap_or(0) as f64 / 1000.0;
    let took = if secs < 60.0 {
        format!("{secs:.1} s")
    } else {
        format!("{} min {} s", secs as u64 / 60, secs as u64 % 60)
    };
    let guest = match guests {
        0 => String::new(),
        n => format!(" · {n} guest comment{}", if n == 1 { "" } else { "s" }),
    };
    let facts = format!(
        "-# {turns} turn{} · {} of {} · {took}{guest}",
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
    let title = format!(
        "Restarted <t:{}:T> onto the vault's config note",
        at_unix_ms / 1000
    );
    let why = format!(
        "the note had changed since my local copy: {}",
        tables.join(", ")
    );
    theseus_core::owner_note::note(theseus_core::owner_note::Kind::Fyi, &title, &why, "")
}

/// A glide's post (38b): another session's words, in parts under Discord's
/// limit, the last ending with the line that names the session.
pub fn glide(body: &Value) -> Vec<String> {
    let text = body["text"].as_str().unwrap_or("").trim_end();
    let session = body["session"].as_str().unwrap_or("?");
    split_text(
        &format!("{text}\n-# ↪ posted from Theseus session `{session}`"),
        PART_LIMIT,
    )
}

/// A post created again after an outage longer than Discord keeps a nonce
/// (theseus-q4v): the channel may already hold it, so it says so.
pub const RESENT: &str =
    "-# ↻ Sent again after an outage: if the same message is just above, this is a copy.";

fn footer(r: &TurnSubmitResult) -> String {
    let mut bits = vec![];
    if !r.profile.is_empty() {
        bits.push(r.profile.clone());
    }
    // A held turn made no model call (theseus-2rvwv).
    bits.push(["held: no model ran", &r.model][usize::from(r.stop_reason != "held")].into());
    // A model the operator named, not Jev's verdict (theseus-h66v4.3).
    bits.extend(
        r.route
            .as_ref()
            .and_then(|t| t.pin_footer())
            .map(Into::into),
    );
    // Where routing ran it: ⬆️ or ⬇️ on the reply corrects it (theseus-q31l).
    bits.extend(crate::runtime::route::routed(r));
    // Which thread it ran in, with a move handle when the router was unsure
    // (theseus-rha2: `/move` runs the message again where it belongs).
    bits.extend(r.thread.as_ref().and_then(|t| t.words()));
    // Where Jev filed the session (theseus-xzxqv): `filed: topic › a › b (new)`.
    bits.extend(r.filed.clone());
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
    // The session's total with this reply's cost (theseus-c0bb).
    bits.extend(r.cost_words());
    bits.push(format!("{:.1} s", r.elapsed_ms as f64 / 1000.0));
    if r.continuation {
        bits.push("continued".into());
    }
    // A reply recall fed (M6 30b: canary and live; nothing in shadow).
    if r.recalled > 0 {
        bits.push(format!("🧠 {} recalled", r.recalled));
    }
    if r.awaiting_confirm.is_some() {
        bits.push("waiting for your approval".into());
    } else if !matches!(
        r.stop_reason.as_str(),
        "" | "no_tool_calls" | "end_turn" | "held"
    ) {
        bits.push(format!("stopped: {}", r.stop_reason));
    }
    // What the session carries, and the way off it (theseus-emqbm).
    bits.extend(labels_bit(&r.labels));
    // The turn in the cockpit (theseus-x9swu): a masked link, its brackets
    // keeping Discord from an embed, on this line and no other.
    if let Some(u) = &r.inspect_url {
        bits.push(format!("[inspect](<{u}>)"));
    }
    let lines = r.harness_lines(); // a refusal's fallback, a nudge (theseus-7gir.18, theseus-fh0u)
    let fell: String = lines.iter().map(|l| format!("-# {l}\n")).collect();
    format!("{fell}-# {}", bits.join(" · "))
}

/// The footer's label bit (theseus-emqbm): the labels the session carries now,
/// each with a link that removes that one from that session.
///
/// The owner asked that a message always show the labels it is carrying and
/// give a way off them at any time, "without a big button": the **Remove**
/// button on the message that raises a label is reachable only at the moment
/// it goes up, so the footer carries a link instead. The flag's word is always
/// shown; the link rides beside it, and is left out when the daemon serves no
/// web UI and `[cockpit] public_url` names none, so the label is still said.
///
/// **A session with no labels gets `None`**, so the footer is byte for byte
/// the footer it was: nothing is appended, no separator and no empty bracket.
/// `render::tests::a_session_with_no_labels_has_the_footer_it_always_had`
/// holds that, and the locked DM flow's golden pins it.
fn labels_bit(labels: &[theseus_protocol::LabelLink]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let each: Vec<String> = labels
        .iter()
        .map(|l| match &l.remove_url {
            Some(u) => format!("{} ([remove](<{u}>))", l.flag),
            None => l.flag.clone(),
        })
        .collect();
    Some(format!("labels: {}", each.join(", ")))
}

/// One short line for a tool call: the argument a person would look for.
/// An AWS call's tool line (row 29, C1): `cloudformation:DescribeStacks ·
/// us-west-2 · account 111122223333`, with the resources its input names.
pub fn aws_summary(a: &theseus_protocol::AwsPlan) -> String {
    let mut text = format!(
        "{}:{} · {} · account {}",
        a.service, a.operation, a.region, a.account
    );
    if !a.resources.is_empty() {
        text.push_str(&format!(" · {}", a.resources.join(", ")));
    }
    clip(&text.replace('`', "'").replace('\n', " "), 120)
}

pub fn summarize(tool: &str, input: &Value) -> String {
    theseus_protocol::term::words::argument(tool, input)
}

/// Dollars as the narrative says them: `$100`, `$0.45`, `$0.0045`.
fn dollars(usd: f64) -> String {
    theseus_core::narrative::dollars((usd.max(0.0) * 1e6).round() as u64)
}

pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// `s` in at most `max` characters, cut in its middle: a third of them from
/// its start, the rest from its end, where a card's reason says why
/// (theseus-94a6: a shared place's clause ends a private fetch's reason, after
/// its URL). The start is on the card's line too.
pub(crate) fn clip_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let head = max / 3;
    let mut out: String = s.chars().take(head).collect();
    out.push('…');
    out.extend(s.chars().skip(n - (max - head - 1)));
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
    // Under test, a split that stops moving panics at once, where it would push
    // empty parts until memory ran out (theseus-dsmp): each turn of the loop
    // moves the text on by at least a byte, or ends it.
    #[cfg(test)]
    let mut turns = 0usize;
    while !rest.is_empty() {
        #[cfg(test)]
        {
            turns += 1;
            assert!(
                turns <= text.len(),
                "split_text stopped moving through its text"
            );
        }
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
mod tests;
