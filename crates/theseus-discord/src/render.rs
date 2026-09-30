//! A session's event stream as Discord messages. Pure: notifications in,
//! operations out, so the whole rendering is tested without Discord.
//!
//! A turn becomes: each loop's streamed text as one or more messages edited in
//! place (split under Discord's 2000-character limit, with code fences closed
//! and reopened across a split); one message per loop listing its tool calls,
//! updated as each runs; a confirm message with Approve and Decline buttons;
//! and a footer line with the turn's model, loops, tools, dollars, and time.
//! Rendering is a diff: every tick re-renders the live turns and emits only the
//! messages whose text changed, so a burst of deltas costs one edit.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde_json::Value;
use theseus_protocol::{ConfirmRequest, TurnSubmitResult};

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
    /// The same, in the DM with `user`: an approval card this place may not
    /// carry (theseus-sgh).
    InDm {
        user: u64,
        key: String,
        content: String,
        buttons: Buttons,
    },
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

/// A card asked for: its message key, the line it describes, whether it is
/// a budget question, the DM it went to (user, label), and whether the place
/// has a note about it.
#[derive(Debug, Clone)]
struct Card {
    key: String,
    line: String,
    budget: bool,
    dm: Option<(u64, String)>,
    note: bool,
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
    footer: Option<String>,
    failure: Option<String>,
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
    /// correlation id → the card asked for it.
    confirms: HashMap<String, Card>,
    /// tool_use_id → the notice card posted for it (updated when the call
    /// ends) and the call it names.
    notices: HashMap<String, (NoticeCard, Asked)>,
    /// Tool → who tightened it ("should have asked", theseus-sgh), as the
    /// core announced.
    tightened: BTreeMap<String, String>,
    /// `[discord] notice_embeds`: a notified call posts its own card. Off, its
    /// tool line alone carries the notice.
    notice_embeds: bool,
    /// Where this place's approval cards go (`set_route`).
    route: Route,
    /// Where else an approval can be answered, as a card says it ("in the web
    /// UI or with `theseus confirm`"); None says that, and "" says nothing.
    elsewhere: Option<String>,
}

impl Renderer {
    /// A place's renderer; `notice_embeds` is the `[discord]` setting.
    pub fn new(notice_embeds: bool) -> Self {
        Self {
            notice_embeds,
            ..Self::default()
        }
    }

    /// Where this place's next approval card goes (theseus-sgh).
    pub fn set_route(&mut self, route: Route) {
        self.route = route;
    }

    /// Where else an approval can be answered, for a card to say: the
    /// trusted local surfaces when `[approval]` is set ("" for none).
    pub fn set_elsewhere(&mut self, elsewhere: impl Into<String>) {
        self.elsewhere = Some(elsewhere.into());
    }

    fn elsewhere(&self) -> &str {
        self.elsewhere
            .as_deref()
            .unwrap_or("in the web UI or with `theseus confirm`")
    }

    /// True while a turn is running (the place keeps "typing…" alive).
    pub fn busy(&self) -> bool {
        self.turns.back().is_some_and(|t| !t.ended)
    }

    /// Feed one notification for this session; returns what to do right away.
    /// Streamed text and tool lines wait for [`Renderer::tick`].
    pub fn on_notification(&mut self, method: &str, p: &Value) -> Vec<Op> {
        let turn_id = p.get("turn_id").and_then(Value::as_str).unwrap_or("");
        match method {
            "turn.started" => {
                self.turns.push_back(TurnView {
                    turn_id: turn_id.to_string(),
                    loops: BTreeMap::new(),
                    footer: None,
                    failure: None,
                    ended: false,
                    dirty: false,
                });
                while self.turns.len() > RECENT_TURNS {
                    self.turns.pop_front();
                }
                vec![Op::Typing]
            }
            "model.delta" => {
                let li = p.get("loop_index").and_then(Value::as_u64).unwrap_or(0) as u32;
                let text = p.get("text").and_then(Value::as_str).unwrap_or("");
                if let Some(t) = self.turn_mut(turn_id) {
                    t.loops.entry(li).or_default().text.push_str(text);
                    t.dirty = true;
                }
                vec![]
            }
            "tool.proposed" => {
                let tool = p.get("tool").and_then(Value::as_str).unwrap_or("?");
                let input = p.get("input").cloned().unwrap_or(Value::Null);
                let state = ToolState::Proposed;
                let notice = p
                    .pointer("/gate/decision/notify")
                    .filter(|n| n.is_object())
                    .map(|n| str_of(n, "setting"));
                let line = ToolLine {
                    tool_use_id: str_of(p, "tool_use_id"),
                    tool: tool.to_string(),
                    summary: summarize(tool, &input),
                    correlation_id: None,
                    state,
                    notice,
                };
                if let Some(t) = self.turn_mut(turn_id) {
                    let li = t.loops.keys().next_back().copied().unwrap_or(0);
                    t.loops.entry(li).or_default().tools.push(line);
                    t.dirty = true;
                }
                vec![]
            }
            "tool.started" => {
                let corr = p
                    .get("correlation_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.update_tool(turn_id, &str_of(p, "tool_use_id"), |l| {
                    l.state = ToolState::Running;
                    if corr.is_some() {
                        l.correlation_id = corr.clone();
                    }
                });
                vec![]
            }
            "tool.ended" => {
                let status = str_of(p, "status");
                let ms = p.get("duration_ms").and_then(Value::as_u64).unwrap_or(0);
                let use_id = str_of(p, "tool_use_id");
                let mut ops = vec![];
                if let Some((card, _)) = self.notices.get_mut(&use_id) {
                    let outcome = match status.as_str() {
                        "ok" => format!("✅ ok · {ms} ms"),
                        "background" => "⏳ running in the background".to_string(),
                        other => format!("❌ {other} · {ms} ms"),
                    };
                    if let Some(f) = card.fields.iter_mut().find(|(n, _)| n == "Outcome") {
                        f.1 = outcome;
                    }
                    ops.push(Op::Notice {
                        key: format!("notice:{use_id}"),
                        card: card.clone(),
                    });
                }
                self.update_tool(turn_id, &use_id, |l| {
                    l.state = match status.as_str() {
                        "background" => ToolState::Background,
                        // A decline keeps saying who declined. A daemon from
                        // before theseus-8az says `denied`.
                        "declined" | "denied" => match &l.state {
                            ToolState::Answered {
                                approved: false, ..
                            } => l.state.clone(),
                            _ => ToolState::NotRun,
                        },
                        _ => ToolState::Done {
                            status: status.clone(),
                            ms,
                        },
                    };
                });
                ops
            }
            "policy.notified" if !self.notice_embeds => vec![],
            "policy.notified" => {
                let tool = p.get("tool").and_then(Value::as_str).unwrap_or("?");
                let input = p.get("input").cloned().unwrap_or(Value::Null);
                let use_id = str_of(p, "tool_use_id");
                let call = Asked {
                    tool: tool.to_string(),
                    correlation_id: p
                        .get("correlation_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                let mut card = NoticeCard {
                    title: "🔔 Ran with a notice".into(),
                    color: AMBER,
                    description: format!("`{tool}` {}", summarize(tool, &input)),
                    fields: vec![
                        ("What".into(), clip(&str_of(p, "summary"), 1000)),
                        ("Posture".into(), format!("`{}`", str_of(p, "setting"))),
                        ("Outcome".into(), "⏳ running".into()),
                    ],
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
            "policy.tightened" | "policy.untightened" => {
                let tool = str_of(p, "tool");
                if method == "policy.tightened" {
                    self.tightened.insert(tool.clone(), str_of(p, "by"));
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
            "confirm.requested" => {
                let Ok(req) = serde_json::from_value::<ConfirmRequest>(p.clone()) else {
                    return vec![];
                };
                if let Some(b) = &req.budget {
                    // No tool line waits: the turn stopped before its call.
                    let corr = req.correlation_id.clone();
                    let also = match self.elsewhere() {
                        "" => String::new(),
                        e => format!(" You can also answer {e}."),
                    };
                    let asked_for = match &self.route {
                        Route::Dm { place, .. } => format!("For {place}. "),
                        _ => String::new(),
                    };
                    let content = format!(
                        "{BUDGET_ASK}{}\n-# {asked_for}Approve resets its spend to $0 and the waiting \
                         call goes on; the session's lifetime cost ({}) keeps counting. Decline, or \
                         send a new message, and it keeps waiting.{also}",
                        clip(&req.reason, 300),
                        dollars(b.lifetime_usd)
                    );
                    let line = format!(
                        "spend reset ({} of the {} limit)",
                        dollars(b.spent_usd),
                        dollars(b.limit_usd)
                    );
                    return self.ask(corr, content, line, true);
                }
                // The newest proposed call of this tool is the one waiting.
                let corr = req.correlation_id.clone();
                if let Some(t) = self.turns.iter_mut().rev().find(|t| !t.ended) {
                    if let Some(l) = t
                        .loops
                        .values_mut()
                        .rev()
                        .flat_map(|lv| lv.tools.iter_mut().rev())
                        .find(|l| l.tool == req.tool && l.state == ToolState::Proposed)
                    {
                        l.state = ToolState::Waiting;
                        l.correlation_id = Some(corr.clone());
                    }
                    t.dirty = true;
                }
                let line = format!("`{}` {}", req.tool, summarize(&req.tool, &req.input));
                let mut content = format!("{}{line}", if req.floor { FLOOR_ASK } else { ASK });
                if !req.reason.is_empty() {
                    content.push_str(&format!("\n{}", clip(&req.reason, 300)));
                }
                let asked_for = match &self.route {
                    Route::Dm { place, .. } => format!("for {place} · "),
                    _ => String::new(),
                };
                let also = match self.elsewhere() {
                    "" => String::new(),
                    e => format!(" · you can also answer {e}"),
                };
                content.push_str(&format!(
                    "\n-# {asked_for}expires <t:{}:R>{also}",
                    req.expires_at_ms / 1000
                ));
                let line = if req.floor {
                    format!("{line} (floor)")
                } else {
                    line
                };
                self.ask(corr, content, line, false)
            }
            "confirm.resolved" => {
                let corr = str_of(p, "correlation_id");
                let approved = p.get("approved").and_then(Value::as_bool).unwrap_or(false);
                let superseded = p
                    .get("superseded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let by = p
                    .get("by")
                    .and_then(Value::as_str)
                    .unwrap_or("the operator")
                    .to_string();
                for t in self.turns.iter_mut() {
                    for lv in t.loops.values_mut() {
                        for l in lv.tools.iter_mut() {
                            if l.correlation_id.as_deref() == Some(corr.as_str())
                                && l.state == ToolState::Waiting
                            {
                                l.state = ToolState::Answered {
                                    approved,
                                    by: by.clone(),
                                };
                                t.dirty = true;
                            }
                        }
                    }
                }
                let Some(card) = self.confirms.remove(&corr) else {
                    return vec![];
                };
                let (line, budget) = (&card.line, card.budget);
                let content = if budget && superseded {
                    format!("⏭️ **Replaced**: a new message came first, and its call asks again if it still does not fit. {line}")
                } else if budget && !approved {
                    format!("❎ **Declined** by {by} · {line}; the session keeps waiting, and a new message asks again")
                } else if superseded {
                    format!("⏭️ **Not run**: a new message replaced this request. {line}")
                } else if approved {
                    format!("✅ **Approved** by {by} · {line}")
                } else {
                    format!("❎ **Declined** by {by} · {line}")
                };
                self.settle(card, content)
            }
            "loop.ended" => {
                if let Some(t) = self.turn_mut(turn_id) {
                    t.dirty = true;
                }
                self.tick()
            }
            "turn.ended" => {
                let Ok(r) = serde_json::from_value::<TurnSubmitResult>(p.clone()) else {
                    return vec![];
                };
                match self.turn_mut(&r.turn_id) {
                    Some(t) => {
                        t.footer = Some(footer(&r));
                        t.ended = true;
                        t.dirty = true;
                    }
                    None => {
                        // It started before this place was watching (a daemon
                        // restart racing the binding): show its final text.
                        let mut loops = BTreeMap::new();
                        loops.insert(
                            0,
                            LoopView {
                                text: r.output.clone(),
                                tools: vec![],
                            },
                        );
                        self.turns.push_back(TurnView {
                            turn_id: r.turn_id.clone(),
                            loops,
                            footer: Some(footer(&r)),
                            failure: None,
                            ended: true,
                            dirty: true,
                        });
                        while self.turns.len() > RECENT_TURNS {
                            self.turns.pop_front();
                        }
                    }
                }
                self.tick()
            }
            "turn.failed" => {
                let class = p.get("class").and_then(Value::as_str).unwrap_or("error");
                let error = clip(p.get("error").and_then(Value::as_str).unwrap_or(""), 600);
                let text = format!("⚠️ **Turn failed** ({class}): {error}");
                match self
                    .turns
                    .iter_mut()
                    .rev()
                    .find(|t| t.turn_id == turn_id || (turn_id.is_empty() && !t.ended))
                {
                    Some(t) => {
                        t.failure = Some(text);
                        t.ended = true;
                        t.dirty = true;
                        self.tick()
                    }
                    None => {
                        let key = format!("failed:{}", theseus_protocol::now_unix_ms());
                        vec![self.upsert(&key, text, Buttons::Keep)]
                    }
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

    /// Post a card for `corr` where the route says: here, in the DM with a
    /// note here, or only a note here.
    fn ask(&mut self, corr: String, content: String, line: String, budget: bool) -> Vec<Op> {
        let key = format!("confirm:{corr}");
        let note_key = format!("approval:{corr}");
        let mut card = Card {
            key: key.clone(),
            line: line.clone(),
            budget,
            dm: None,
            note: false,
        };
        let ops = match self.route.clone() {
            Route::Here => vec![self.upsert(&key, content, Buttons::Confirm(corr.clone()))],
            Route::Dm { user, dm, why, .. } => {
                card.dm = Some((user, dm.clone()));
                card.note = true;
                vec![
                    Op::InDm {
                        user,
                        key,
                        content,
                        buttons: Buttons::Confirm(corr.clone()),
                    },
                    self.upsert(
                        &note_key,
                        format!(
                            "🔐 Approval for {line} was asked in {dm}: this channel is not a \
                             trusted channel ({why})."
                        ),
                        Buttons::Keep,
                    ),
                ]
            }
            Route::Elsewhere { why } => {
                card.note = true;
                let answer = match self.elsewhere() {
                    "" => ", and no trusted channel is bound here to answer it".to_string(),
                    e => format!(": answer {e}"),
                };
                vec![self.upsert(
                    &note_key,
                    format!(
                        "🔐 {line} waits for approval, and this channel is not a trusted \
                         channel ({why}){answer}."
                    ),
                    Buttons::Keep,
                )]
            }
        };
        self.confirms.insert(corr, card);
        ops
    }

    /// A card answered: settled where it was posted, and its note updated.
    fn settle(&mut self, card: Card, content: String) -> Vec<Op> {
        let note_key = card.key.replacen("confirm:", "approval:", 1);
        match (&card.dm, card.note) {
            (Some((user, dm)), _) => vec![
                Op::InDm {
                    user: *user,
                    key: card.key.clone(),
                    content: content.clone(),
                    buttons: Buttons::Clear,
                },
                self.upsert(&note_key, format!("🔐 {content} (in {dm})"), Buttons::Keep),
            ],
            (None, true) => vec![self.upsert(&note_key, format!("🔐 {content}"), Buttons::Keep)],
            (None, false) => vec![self.upsert(&card.key, content, Buttons::Clear)],
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

    fn update_tool(&mut self, turn_id: &str, tool_use_id: &str, f: impl Fn(&mut ToolLine)) {
        if let Some(t) = self.turn_mut(turn_id) {
            for lv in t.loops.values_mut() {
                if let Some(l) = lv.tools.iter_mut().find(|l| l.tool_use_id == tool_use_id) {
                    f(l);
                    t.dirty = true;
                }
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

/// Every message a turn shows, in the order Discord should first see them.
/// `tightened` is tool → who tightened it; `menus` puts a "Should have
/// asked…" select on each tool message that lists a notified call.
fn render_turn(t: &TurnView, tightened: &BTreeMap<String, String>, menus: bool) -> Vec<Rendered> {
    let mut out: Vec<Rendered> = Vec::new();
    for (li, lv) in &t.loops {
        for (i, part) in split_text(&lv.text, PART_LIMIT).into_iter().enumerate() {
            out.push(Rendered::text(format!("{}:L{li}:p{i}", t.turn_id), part));
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
    if let Some(f) = &t.failure {
        out.push(Rendered::text(format!("{}:failed", t.turn_id), f.clone()));
    }
    if let Some(f) = &t.footer {
        // The footer rides on the last text part when it fits, else stands alone.
        match out.last_mut() {
            Some(m) if m.key.contains(":p") && m.content.len() + f.len() < DISCORD_LIMIT => {
                m.content.push('\n');
                m.content.push_str(f);
            }
            _ => out.push(Rendered::text(format!("{}:footer", t.turn_id), f.clone())),
        }
    }
    out
}

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
            let head = format!("`{}` {}{mark}", l.tool, l.summary);
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
    let from = match (
        a.get("job").and_then(Value::as_str),
        a.get("pid").and_then(Value::as_u64),
    ) {
        (Some(job), Some(pid)) => format!(
            "`{}` (pid {pid}), a process of job `{job}`",
            clip(&str_of(&a, "argv0").replace('`', "'"), 40)
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

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Split streamed text into Discord-sized parts. Cuts prefer a newline, then a
/// space, in the second half of the budget; a code fence left open by a cut is
/// closed at the end of the part and reopened (with its language) at the start
/// of the next. Stable under appends: text only grows, so earlier cuts stay put.
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

/// The opening line of a code fence left open at the end of `s`, if any.
fn open_fence(s: &str) -> Option<String> {
    let mut open: Option<String> = None;
    for line in s.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            open = match open {
                Some(_) => None,
                None => Some(t.trim_end().to_string()),
            };
        }
    }
    open
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
    }
    use serde_json::json;

    fn upserts(ops: &[Op]) -> Vec<(String, String)> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Upsert { key, content, .. } => Some((key.clone(), content.clone())),
                Op::Typing | Op::Notice { .. } | Op::InDm { .. } => None,
            })
            .collect()
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
        let ops = upserts(&r.on_notification("turn.ended", &ended("t1", None)));
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t1:L1:p0");
        assert!(
            ops[0].1.starts_with(
                "Done.\n-# sonnet · claude-sonnet-5 · 2 loops · 1 tool call · $0.0123 · 4.2 s"
            ),
            "{}",
            ops[0].1
        );
        assert!(!r.busy());
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
        for (answer, want) in [
            (json!({"approved": true, "by": "discord:eddie"}), "✅ **Approved** by discord:eddie · spend reset ($99.48 of the $100 limit)"),
            (json!({"approved": false, "by": "discord:eddie"}), "❎ **Declined** by discord:eddie · spend reset ($99.48 of the $100 limit); the session keeps waiting, and a new message asks again"),
            (json!({"approved": false, "superseded": true}), "⏭️ **Replaced**: a new message came first, and its call asks again if it still does not fit. spend reset ($99.48 of the $100 limit)"),
        ] {
            let mut r = Renderer::default();
            r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
            match &r.on_notification("confirm.requested", &ask("act_b"))[..] {
                [Op::Upsert { key, content, buttons }] => {
                    assert_eq!(key, "confirm:act_b");
                    assert!(content.starts_with(&format!("{BUDGET_ASK}This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?\n")), "{content}");
                    assert!(content.contains("lifetime cost ($212.40) keeps counting"), "{content}");
                    assert!(!content.contains("expires"), "a budget question does not expire: {content}");
                    assert_eq!(buttons, &Buttons::Confirm("act_b".into()));
                }
                other => panic!("{other:?}"),
            }
            let mut p = answer.clone();
            p["correlation_id"] = json!("act_b");
            match &r.on_notification("confirm.resolved", &p)[..] {
                [Op::Upsert { key, content, buttons }] => {
                    assert_eq!((key.as_str(), content.as_str()), ("confirm:act_b", want));
                    assert_eq!(buttons, &Buttons::Clear);
                }
                other => panic!("{answer}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_confirm_gets_buttons_and_loses_them_when_answered() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        let ops = r.on_notification("confirm.requested", &json!({"correlation_id": "act_1", "session_id": "s",
            "execution_id": "e", "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1790000000000u64}));
        match &ops[0] {
            Op::Upsert {
                key,
                content,
                buttons,
            } => {
                assert_eq!(key, "confirm:act_1");
                assert!(content.starts_with("**Approve?** `proc.run` cargo test\nrun cargo test"));
                assert_eq!(buttons, &Buttons::Confirm("act_1".into()));
            }
            other => panic!("{other:?}"),
        }
        let tools = upserts(&r.on_notification("turn.ended", &ended("t1", Some("act_1"))));
        assert!(tools
            .iter()
            .any(|(k, c)| k == "t1:L0:tools" && c.contains("waiting for approval")));
        let ops = r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_1", "approved": true, "by": "discord:eddie"}),
        );
        assert_eq!(
            ops,
            vec![Op::Upsert {
                key: "confirm:act_1".into(),
                content: "✅ **Approved** by discord:eddie · `proc.run` cargo test".into(),
                buttons: Buttons::Clear
            }]
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
        let keys: Vec<String> = upserts(&r.on_notification("turn.ended", &end))
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["t1:L1:p0"]);
    }

    fn waiting_write(r: &mut Renderer) -> Vec<Op> {
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        r.on_notification("confirm.requested", &json!({"correlation_id": "act_1", "session_id": "s",
            "execution_id": "e", "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1790000000000u64}))
    }

    /// A place that is not a trusted channel (theseus-sgh) sends its card to
    /// the trusted user's DM and says so in one line here; the answer settles
    /// the card in the DM and the note here.
    #[test]
    fn a_card_for_an_untrusted_place_goes_to_the_dm_with_a_note_here() {
        let mut r = Renderer::default();
        r.set_route(Route::Dm {
            user: 159471966640799744,
            dm: "DM @eddie".into(),
            place: "#general".into(),
            why: "it is not listed in [approval] channels".into(),
        });
        r.set_elsewhere("with `theseus confirm`");
        let ops = waiting_write(&mut r);
        let [Op::InDm {
            user,
            key,
            content,
            buttons,
        }, Op::Upsert {
            key: note_key,
            content: note,
            buttons: Buttons::Keep,
        }] = &ops[..]
        else {
            panic!("{ops:?}")
        };
        assert_eq!((*user, key.as_str()), (159471966640799744, "confirm:act_1"));
        assert!(
            content.starts_with("**Approve?** `proc.run` cargo test\nrun cargo test\n"),
            "{content}"
        );
        assert!(
            content.ends_with("-# for #general · expires <t:1790000000:R> · you can also answer with `theseus confirm`"),
            "{content}"
        );
        assert_eq!(buttons, &Buttons::Confirm("act_1".into()));
        assert_eq!(note_key, "approval:act_1");
        assert_eq!(
            note,
            "🔐 Approval for `proc.run` cargo test was asked in DM @eddie: this channel is not a \
             trusted channel (it is not listed in [approval] channels)."
        );
        assert!(
            !ops.iter().any(|o| matches!(
                o,
                Op::Upsert {
                    buttons: Buttons::Confirm(_),
                    ..
                }
            )),
            "no buttons here"
        );
        let ops = r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_1", "approved": true, "by": "discord:eddie"}),
        );
        assert_eq!(
            ops,
            vec![
                Op::InDm {
                    user: 159471966640799744,
                    key: "confirm:act_1".into(),
                    content: "✅ **Approved** by discord:eddie · `proc.run` cargo test".into(),
                    buttons: Buttons::Clear
                },
                Op::Upsert {
                    key: "approval:act_1".into(),
                    content:
                        "🔐 ✅ **Approved** by discord:eddie · `proc.run` cargo test (in DM @eddie)"
                            .into(),
                    buttons: Buttons::Keep
                }
            ]
        );
        // The tool line here says who approved it, as it does for a card here.
        let tools = upserts(&r.tick());
        assert!(
            tools[0].1.contains("approved by discord:eddie"),
            "{tools:?}"
        );
    }

    /// A budget question takes the same route as a tool call.
    #[test]
    fn a_budget_question_for_an_untrusted_place_goes_to_the_dm_too() {
        let mut r = Renderer::default();
        r.set_route(Route::Dm {
            user: 7,
            dm: "DM @eddie".into(),
            place: "#general".into(),
            why: "cannot be verified".into(),
        });
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let ops = r.on_notification("confirm.requested", &json!({"correlation_id": "act_b", "session_id": "s",
            "execution_id": "e", "tool": "budget.reset", "input": {}, "by": "operator", "requested_at_ms": 1,
            "expires_at_ms": 0, "reason": "This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?",
            "budget": {"spent_usd": 99.48, "limit_usd": 100.0, "needed_usd": 1.0, "lifetime_usd": 212.4}}));
        match &ops[..] {
            [Op::InDm {
                user: 7,
                key,
                content,
                buttons: Buttons::Confirm(c),
            }, Op::Upsert {
                key: note,
                content: said,
                ..
            }] => {
                assert_eq!(
                    (key.as_str(), c.as_str(), note.as_str()),
                    ("confirm:act_b", "act_b", "approval:act_b")
                );
                assert!(
                    content.contains("\n-# For #general. Approve resets its spend"),
                    "{content}"
                );
                assert!(
                    content
                        .ends_with("You can also answer in the web UI or with `theseus confirm`."),
                    "{content}"
                );
                assert!(said.starts_with("🔐 Approval for spend reset ($99.48 of the $100 limit) was asked in DM @eddie"), "{said}");
            }
            other => panic!("{other:?}"),
        }
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
            let mut r = Renderer::default();
            r.set_route(Route::Elsewhere {
                why: "it is not listed in [approval] channels".into(),
            });
            r.set_elsewhere(elsewhere);
            let ops = waiting_write(&mut r);
            assert_eq!(
                ops,
                vec![Op::Upsert {
                    key: "approval:act_1".into(),
                    content: format!(
                        "🔐 `proc.run` cargo test waits for approval, and this channel is not a \
                         trusted channel (it is not listed in [approval] channels){tail}"
                    ),
                    buttons: Buttons::Keep
                }]
            );
            let ops = r.on_notification(
                "confirm.resolved",
                &json!({"correlation_id": "act_1", "approved": false, "by": "sock#3"}),
            );
            assert_eq!(
                upserts(&ops),
                [(
                    "approval:act_1".into(),
                    "🔐 ❎ **Declined** by sock#3 · `proc.run` cargo test".into()
                )]
            );
        }
    }

    /// With `[approval]`, a card names only the trusted local surfaces; with
    /// none, it names none. Without `[approval]` it reads as before.
    #[test]
    fn a_card_names_where_else_it_can_be_answered() {
        let text = |elsewhere: Option<&str>| {
            let mut r = Renderer::default();
            if let Some(e) = elsewhere {
                r.set_elsewhere(e);
            }
            match &waiting_write(&mut r)[..] {
                [Op::Upsert {
                    content,
                    buttons: Buttons::Confirm(_),
                    ..
                }] => content.lines().last().unwrap().to_string(),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(
            text(None),
            "-# expires <t:1790000000:R> · you can also answer in the web UI or with `theseus confirm`"
        );
        assert_eq!(
            text(Some("in the web UI")),
            "-# expires <t:1790000000:R> · you can also answer in the web UI"
        );
        assert_eq!(text(Some("")), "-# expires <t:1790000000:R>");
    }

    #[test]
    fn a_floor_confirm_says_so_and_names_the_reason() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let reason = "read /home/x/.openclaw-1password-service-token: fs.read — approve (floor: /home/x/.openclaw-1password-service-token is Theseus's own state or the 1Password token)";
        let ops = upserts(&r.on_notification("confirm.requested", &json!({"correlation_id": "act_9",
            "session_id": "s", "execution_id": "e", "tool": "fs.read",
            "input": {"path": "~/.openclaw-1password-service-token"}, "reason": reason, "by": "operator",
            "requested_at_ms": 0, "expires_at_ms": 60_000, "floor": true})));
        let (_, content) = ops.iter().find(|(k, _)| k == "confirm:act_9").unwrap();
        assert!(content.starts_with(FLOOR_ASK), "{content}");
        assert!(content.contains("fs.read — approve (floor: "), "{content}");
    }

    #[test]
    fn a_turn_seen_only_at_its_end_still_shows_its_text() {
        let mut r = Renderer::default();
        let mut end = ended("t9", None);
        end["output"] = json!("Done. The command printed `done`.");
        end["continuation"] = json!(true);
        let ops = upserts(&r.on_notification("turn.ended", &end));
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t9:L0:p0");
        assert!(
            ops[0]
                .1
                .starts_with("Done. The command printed `done`.\n-# sonnet"),
            "{}",
            ops[0].1
        );
        assert!(ops[0].1.contains("continued"));
        assert!(!r.busy());
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
            assert!(ops.contains(&(
                "t1:failed".into(),
                "⚠️ **Turn failed** (overloaded): try later".into()
            )));
            assert!(!r.busy());
        }
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
    }
}
