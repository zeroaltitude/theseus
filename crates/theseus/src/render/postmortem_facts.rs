//! The postmortem's facts (theseus-m2tiu): one pure walk over a window's
//! ledger rows, oldest first, that says what ran, waited, failed and cost,
//! what was asked and answered, and every restart. It reads no clock and no
//! connection: the window's end is the caller's, so the same rows always
//! make the same report.
//!
//! Every fact has its row: a line carries the position of the row it comes
//! from. What the rows do not say is not guessed: a call that began before
//! the window is unseen until it settles, and a window that ends in the past
//! shows what was open by the rows it read.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;
use theseus_protocol::LedgerEntry;

use super::postmortem::{
    span, usd, Asked, Cost, CostRow, Held, LateWake, Line, Open, OpenTurn, OpenWake, Report,
    Restart, Resumed, Summary,
};

/// A tool call that ran longer than this is a line of its own.
pub const LONG_CALL_MS: u64 = 30_000;
/// A run of quiet turns shorter than this is not folded.
pub const FOLD_MIN: usize = 3;
/// A wake that ran later than this is a line of its own.
const LATE_WAKE_MS: u64 = 5_000;

/// What the report is made from.
pub struct Window<'a> {
    pub rows: &'a [LedgerEntry],
    pub since_ms: u64,
    pub until_ms: u64,
    /// The sessions the window was narrowed to; empty when it was not.
    pub scope: &'a [String],
}

fn text<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn num(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn float(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

/// The first line of a text, cut at 120 characters.
fn first_line(s: &str) -> String {
    let l = s
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut out: String = l.chars().take(120).collect();
    if l.chars().count() > 120 {
        out.push('…');
    }
    out
}

/// A node's id as `msg·<end>`.
fn msg(id: &str) -> String {
    format!("msg·{}", &id[id.len().saturating_sub(8)..])
}

#[derive(PartialEq, Clone, Copy)]
enum Fate {
    Running,
    Ended,
    Failed,
    Refused,
    Interrupted,
}

struct Turn {
    session: String,
    id: String,
    start_ms: u64,
    start_pos: u64,
    trigger: String,
    fate: Fate,
    end_ms: Option<u64>,
    end_pos: u64,
    usd: f64,
    detail: String,
    notable: bool,
}

struct Call {
    session: String,
    tool: String,
    reserved: f64,
    planned_ms: u64,
    planned_pos: u64,
    dispatched_ms: Option<u64>,
    argv: Option<String>,
    settled: Option<&'static str>,
    model: bool,
}

#[derive(Default)]
struct Walk {
    turns: Vec<Turn>,
    open_turn: HashMap<(String, String), usize>,
    /// Lines other than the turns': (line, whether it makes its turn notable).
    lines: Vec<(Line, bool)>,
    calls: HashMap<String, Call>,
    call_order: Vec<String>,
    asks: Vec<Asked>,
    ask_at: HashMap<String, usize>,
    why: HashMap<String, String>,
    resumed: HashSet<String>,
    restarts: Vec<Restart>,
    cur: Option<Restart>,
    stop_at: Option<u64>,
    /// The newest row's time, and what it was when the current start began.
    last_at: u64,
    before_start: u64,
    /// The start in progress is the store's first: no restart.
    first_start: bool,
    cost_session: BTreeMap<String, (f64, u32)>,
    cost_model: BTreeMap<String, (f64, u32)>,
    total: f64,
    wakes: Vec<OpenWake>,
    sessions: Vec<String>,
    tasks: Vec<String>,
    turns_failed: u32,
}

/// The report of a window.
pub fn analyze(w: &Window<'_>) -> Report {
    let mut s = Walk::default();
    for e in w.rows {
        s.row(e);
        s.last_at = s.last_at.max(e.at_unix_ms);
    }
    s.finish(w)
}

impl Walk {
    fn line(&mut self, e: &LedgerEntry, kind: &str, text: String, notable: bool) {
        let l = Line {
            at_ms: e.at_unix_ms,
            position: e.position,
            session: e.session_id.clone(),
            kind: kind.into(),
            text,
            ..Default::default()
        };
        self.lines.push((l, notable));
    }

    /// The restart a start's rows belong to, begun by the first of them: the
    /// turns it interrupted come before its `startup.step` rows.
    fn restart(&mut self) -> &mut Restart {
        if self.cur.is_none() {
            self.before_start = self.last_at;
        }
        self.cur.get_or_insert_with(Restart::default)
    }

    fn touch(&mut self, session: Option<&String>) {
        if let Some(s) = session {
            if !self.sessions.contains(s) {
                self.sessions.push(s.clone());
            }
        }
    }

    /// One row, by the group of kinds it is in.
    fn row(&mut self, e: &LedgerEntry) {
        let _ = self.row_turn(e)
            || self.row_model(e)
            || self.row_task(e)
            || self.row_call(e)
            || self.row_daemon(e);
    }

    fn row_turn(&mut self, e: &LedgerEntry) -> bool {
        let d = &e.data;
        let sid = e.session_id.clone().unwrap_or_default();
        match e.kind.as_str() {
            "turn.started" => self.turn_started(e),
            "turn.ended" => self.turn_end(
                e,
                Fate::Ended,
                format!(
                    "{} loops, {} tool calls{}",
                    num(d, "loops"),
                    num(d, "tool_calls"),
                    match text(d, "stop_reason") {
                        Some(r) if r != "end_turn" => format!(", stopped: {r}"),
                        _ => String::new(),
                    }
                ),
            ),
            "turn.failed" => {
                let why = text(d, "reason").unwrap_or("unknown reason").to_string();
                self.turn_end(e, Fate::Failed, why);
            }
            "turn.refused" => {
                let why = format!(
                    "{}: {}",
                    text(d, "class").unwrap_or("refused"),
                    first_line(text(d, "error").unwrap_or(""))
                );
                self.turn_end(e, Fate::Refused, why);
            }
            "execution.queued" => {
                if let Some(w) = text(d, "why") {
                    self.why.insert(sid, w.to_string());
                }
            }
            "execution.interrupted" => {
                self.resumed.insert(sid.clone());
                // A turn the restart found running ends there, interrupted.
                let at = e.at_unix_ms;
                let idx: Vec<usize> = self
                    .open_turn
                    .iter()
                    .filter(|((s, _), _)| *s == sid)
                    .map(|(_, i)| *i)
                    .collect();
                let mut resumed = Resumed {
                    session: sid,
                    execution: text(d, "execution_id").unwrap_or("").to_string(),
                    turn: None,
                };
                for i in idx {
                    let t = &mut self.turns[i];
                    t.fate = Fate::Interrupted;
                    t.end_ms = Some(at);
                    t.end_pos = e.position;
                    t.notable = true;
                    resumed.turn = Some(t.id.clone());
                    self.open_turn.remove(&(t.session.clone(), t.id.clone()));
                }
                self.restart().requeued.push(resumed);
            }
            _ => return false,
        }
        true
    }

    fn row_model(&mut self, e: &LedgerEntry) -> bool {
        let d = &e.data;
        let sid = e.session_id.clone().unwrap_or_default();
        match e.kind.as_str() {
            "provider.call" => {
                let c = float(d, "cost_usd");
                self.book(&sid, text(d, "model").unwrap_or("?"), c);
            }
            "provider.cut" => {
                let c = float(d, "cost_usd");
                self.book(&sid, text(d, "model").unwrap_or("?"), c);
                let l = format!(
                    "model call cut ({}) after {}, {} estimated",
                    text(d, "by").unwrap_or("stop"),
                    span(num(d, "elapsed_ms")),
                    usd(c)
                );
                self.line(e, "model", l, true);
            }
            "provider.error" => {
                let l = format!(
                    "model call failed ({}{}) after {}: {}",
                    text(d, "class").unwrap_or("error"),
                    if d.get("transient") == Some(&Value::Bool(true)) {
                        ", transient"
                    } else {
                        ""
                    },
                    span(num(d, "elapsed_ms")),
                    first_line(text(d, "message").unwrap_or(text(d, "detail").unwrap_or("")))
                );
                self.line(e, "model", l, true);
            }
            "provider.fallback" => {
                let l = format!(
                    "model call fell back from {} to {}",
                    text(d, "from").or(text(d, "refused_model")).unwrap_or("?"),
                    text(d, "to")
                        .or(text(d, "fallback_model"))
                        .unwrap_or("its fallback")
                ) + &text(d, "node_id")
                    .map(|n| format!(" ({})", msg(n)))
                    .unwrap_or_default();
                self.line(e, "model", l, true);
            }
            "provider.refusal" => {
                let l = format!(
                    "model refused ({})",
                    text(d, "category")
                        .or(text(d, "stop_reason"))
                        .unwrap_or("refusal")
                );
                self.line(e, "model", l, true);
            }
            "spend.ceiling" => {
                let l = format!(
                    "daily spend ceiling reached: {} of {}; stopped {}",
                    usd(float(d, "spent_usd")),
                    usd(float(d, "limit_usd")),
                    text(d, "what").unwrap_or("model calls")
                );
                self.line(e, "spend", l, true);
            }
            "budget.reached" => {
                let l = format!(
                    "spend limit reached: {} of {} (lifetime {})",
                    usd(float(d, "spent_usd")),
                    usd(float(d, "limit_usd")),
                    usd(float(d, "lifetime_usd"))
                );
                self.line(e, "spend", l, true);
            }
            "loop.cap_reached" => {
                let l = format!(
                    "loop cap reached: {} loops (cap {})",
                    num(d, "loops"),
                    num(d, "max_loops")
                );
                self.line(e, "spend", l, true);
            }
            _ => return false,
        }
        true
    }

    fn row_task(&mut self, e: &LedgerEntry) -> bool {
        let d = &e.data;
        let sid = e.session_id.clone().unwrap_or_default();
        match e.kind.as_str() {
            "execution.opened" if text(d, "kind") == Some("task") => {
                self.touch(e.session_id.as_ref());
                if !self.tasks.contains(&sid) {
                    self.tasks.push(sid);
                }
                let l = format!(
                    "task opened, limit {}{}",
                    usd(float(d, "limit_usd")),
                    text(d, "reports_to")
                        .map(|r| format!(", reports to {r}"))
                        .unwrap_or_default()
                );
                self.line(e, "task", l, true);
            }
            "task.created" => {
                let l = format!(
                    "task {} created: {}",
                    text(d, "task").unwrap_or("?"),
                    first_line(text(d, "title").unwrap_or(""))
                );
                self.line(e, "task", l, true);
            }
            "task.ended" => {
                let l = format!(
                    "task {} ended {}{}, spent {} of {}",
                    text(d, "task").unwrap_or("?"),
                    text(d, "state").unwrap_or("?"),
                    text(d, "reason")
                        .map(|r| format!(" ({})", first_line(r)))
                        .unwrap_or_default(),
                    usd(float(d, "spent_usd")),
                    usd(float(d, "limit_usd"))
                );
                self.line(e, "task", l, true);
            }
            "task.closed" => {
                let l = format!(
                    "task {} closed {}",
                    text(d, "task").unwrap_or("?"),
                    text(d, "state").unwrap_or("?")
                );
                self.line(e, "task", l, true);
            }
            _ => return false,
        }
        true
    }

    fn row_call(&mut self, e: &LedgerEntry) -> bool {
        let d = &e.data;
        match e.kind.as_str() {
            "action.planned" => self.planned(e),
            "action.dispatched" => {
                if let Some(c) = text(d, "correlation_id").and_then(|c| self.calls.get_mut(c)) {
                    c.dispatched_ms = Some(e.at_unix_ms);
                }
            }
            "tool.job_started" => {
                let argv = d.get("argv").and_then(Value::as_array).map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                });
                if let Some(c) = text(d, "correlation_id").and_then(|c| self.calls.get_mut(c)) {
                    c.argv = argv.map(|a| first_line(&a));
                }
            }
            "action.succeeded" => self.settled(e, "succeeded"),
            "action.failed" => self.settled(e, "failed"),
            "action.outcome_unknown" => self.settled(e, "unknown"),
            "action.resolved" => self.settled(e, "resolved"),
            "action.cancelled" => self.settled(e, "cancelled"),
            "tool.confirm_requested" => self.asked(e),
            "action.confirm_answered" => self.answered(e),
            "action.expired" => self.expired(e),
            "approval.refused" => {
                let l = format!(
                    "approval refused: {}",
                    first_line(
                        text(d, "why")
                            .or(text(d, "reason"))
                            .unwrap_or("not from a place that may answer")
                    )
                );
                self.line(e, "ask", l, true);
            }
            "policy.tightened" => {
                let l = format!("policy tightened: {}", first_line(&d.to_string()));
                self.line(e, "ask", l, false);
            }
            _ => return false,
        }
        true
    }

    fn row_daemon(&mut self, e: &LedgerEntry) -> bool {
        let d = &e.data;
        let sid = e.session_id.clone().unwrap_or_default();
        match e.kind.as_str() {
            "wake.set" => {
                self.wakes.push(OpenWake {
                    position: e.position,
                    session: sid,
                    wake: text(d, "wake_id").unwrap_or("?").to_string(),
                    due_at_ms: num(d, "due_at_ms"),
                });
            }
            "wake.fired" => self.wake_fired(e),
            "wake.cancelled" | "wake.ended" => {
                let id = text(d, "wake_id").unwrap_or("");
                self.wakes.retain(|x| x.wake != id);
            }
            "server.stopping" => {
                self.stop_at = Some(e.at_unix_ms);
                self.line(e, "daemon", "daemon stopping (clean stop)".into(), false);
            }
            "startup.step" => self.startup_step(e),
            "server.crashed" => {
                let c = first_line(
                    &[text(d, "location"), text(d, "message")]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(": "),
                );
                let r = self.restart();
                r.crashed = true;
                r.crash = Some(c);
            }
            "server.started" => {
                let stop = self.stop_at.take();
                let last = self.before_start;
                let r = self.restart();
                r.position = e.position;
                r.started_at_ms = e.at_unix_ms;
                r.stopped_at_ms = stop;
                let from = stop.unwrap_or(last);
                r.down_ms = (from > 0).then(|| e.at_unix_ms.saturating_sub(from));
                if stop.is_none() && !r.crashed {
                    r.crashed = false;
                }
            }
            "server.serving" => {
                let first = std::mem::take(&mut self.first_start);
                if let Some(mut r) = self.cur.take().filter(|_| !first) {
                    r.serving_at_ms = Some(e.at_unix_ms);
                    let l = format!(
                        "daemon started again{}",
                        match (r.stopped_at_ms, r.crashed) {
                            (Some(_), _) => " after a clean stop",
                            (None, true) => " after a crash",
                            (None, false) => " after it was killed",
                        }
                    );
                    self.line(e, "daemon", l, false);
                    self.restarts.push(r);
                }
            }
            "reconcile" => {
                let rep = d;
                let Some(r) = self.cur.as_mut().or(self.restarts.last_mut()) else {
                    return true;
                };
                for k in ["marked_unknown", "resolved_unknown"] {
                    if let Some(a) = rep.get(k).and_then(Value::as_array) {
                        r.unknown
                            .extend(a.iter().filter_map(Value::as_str).map(String::from));
                    }
                }
            }

            _ => return false,
        }
        true
    }

    fn book(&mut self, session: &str, model: &str, c: f64) {
        self.total += c;
        let s = self.cost_session.entry(session.to_string()).or_default();
        s.0 += c;
        s.1 += 1;
        let m = self.cost_model.entry(model.to_string()).or_default();
        m.0 += c;
        m.1 += 1;
    }

    fn turn_started(&mut self, e: &LedgerEntry) {
        let sid = e.session_id.clone().unwrap_or_default();
        self.touch(e.session_id.as_ref());
        let author = text(&e.data, "author").unwrap_or("?");
        let why = self.why.remove(&sid);
        let was_resumed = self.resumed.remove(&sid);
        let trigger = if was_resumed {
            "resumed after a restart".to_string()
        } else {
            match why.as_deref() {
                Some("wake") => "wake".into(),
                Some("report") => "task report".into(),
                Some("task") => "task start".into(),
                Some("result") => "job result".into(),
                _ => author.to_string(),
            }
        };
        let id = e.turn_id.clone().unwrap_or_default();
        self.open_turn
            .insert((sid.clone(), id.clone()), self.turns.len());
        self.turns.push(Turn {
            session: sid,
            id,
            start_ms: e.at_unix_ms,
            start_pos: e.position,
            trigger,
            fate: Fate::Running,
            end_ms: None,
            end_pos: e.position,
            usd: 0.0,
            detail: String::new(),
            notable: was_resumed,
        });
    }

    fn turn_end(&mut self, e: &LedgerEntry, fate: Fate, detail: String) {
        let sid = e.session_id.clone().unwrap_or_default();
        let id = e.turn_id.clone().unwrap_or_default();
        let Some(i) = self.open_turn.remove(&(sid, id)) else {
            return;
        };
        let t = &mut self.turns[i];
        t.fate = fate;
        t.end_ms = Some(e.at_unix_ms);
        t.end_pos = e.position;
        t.usd = float(&e.data, "cost_usd");
        t.detail = detail;
        t.notable |= fate != Fate::Ended;
        if fate == Fate::Failed || fate == Fate::Refused {
            self.turns_failed += 1;
        }
    }

    fn planned(&mut self, e: &LedgerEntry) {
        let d = &e.data;
        let Some(c) = text(d, "correlation_id") else {
            return;
        };
        let tool = text(d, "tool").unwrap_or("?").to_string();
        let model = tool.starts_with("model") || tool.starts_with("provider");
        self.call_order.push(c.to_string());
        self.calls.insert(
            c.to_string(),
            Call {
                session: e.session_id.clone().unwrap_or_default(),
                tool,
                reserved: float(d, "reserved_usd"),
                planned_ms: e.at_unix_ms,
                planned_pos: e.position,
                dispatched_ms: None,
                argv: None,
                settled: None,
                model,
            },
        );
    }

    fn settled(&mut self, e: &LedgerEntry, how: &'static str) {
        let d = &e.data;
        let Some(id) = text(d, "correlation_id") else {
            return;
        };
        let producer = text(d, "producer").unwrap_or("");
        let dur = num(d, "duration_ms");
        let Some(c) = self.calls.get_mut(id) else {
            // Planned before the window: its settle alone is a fact.
            if how == "failed" && !producer.starts_with("provider:") {
                let l = format!("tool call failed ({producer}) after {}", span(dur));
                self.line(e, "call", l, true);
            }
            return;
        };
        let model = c.model || producer.starts_with("provider:");
        c.model = model;
        c.settled = Some(how);
        if model {
            return;
        }
        let what = match &c.argv {
            Some(a) => format!("`{}` (`{a}`)", c.tool),
            None => format!("`{}`", c.tool),
        };
        let l = match how {
            "failed" => Some(format!(
                "tool call {what} failed ({producer}) after {}",
                span(dur)
            )),
            "unknown" => Some(format!(
                "tool call {what} ended with its outcome unknown after {}",
                span(dur)
            )),
            _ if dur > LONG_CALL_MS => Some(format!("tool call {what} ran {}", span(dur))),
            _ => None,
        };
        if let Some(l) = l {
            self.line(e, "call", l, true);
        }
    }

    fn asked(&mut self, e: &LedgerEntry) {
        let d = &e.data;
        let sid = e.session_id.clone().unwrap_or_default();
        self.touch(e.session_id.as_ref());
        let id = text(d, "correlation_id").unwrap_or("").to_string();
        let tool = text(d, "tool").unwrap_or("?").to_string();
        let reason = first_line(text(d, "reason").unwrap_or(""));
        self.ask_at.insert(id.clone(), self.asks.len());
        self.asks.push(Asked {
            position: e.position,
            session: sid,
            correlation_id: id,
            tool: tool.clone(),
            reason: reason.clone(),
            asked_at_ms: e.at_unix_ms,
            outcome: "open".into(),
            ..Default::default()
        });
        let l = format!(
            "asked to approve `{tool}`{}",
            if reason.is_empty() {
                String::new()
            } else {
                format!(": {reason}")
            }
        );
        self.line(e, "ask", l, true);
    }

    fn answered(&mut self, e: &LedgerEntry) {
        let d = &e.data;
        let Some(&i) = text(d, "correlation_id").and_then(|c| self.ask_at.get(c)) else {
            return;
        };
        let a = &mut self.asks[i];
        let ok = d.get("approved") == Some(&Value::Bool(true));
        a.outcome = if ok { "approved" } else { "declined" }.into();
        a.answered_at_ms = Some(e.at_unix_ms);
        a.answered_position = Some(e.position);
        a.by = text(d, "by").map(String::from);
        a.via = text(d, "via").map(String::from);
        a.latency_ms = Some(e.at_unix_ms.saturating_sub(a.asked_at_ms));
        let l = format!(
            "`{}` {} by {} from {} after {}",
            a.tool,
            a.outcome,
            a.by.as_deref().unwrap_or("?"),
            a.via.as_deref().unwrap_or("?"),
            span(a.latency_ms.unwrap_or(0))
        );
        self.line(e, "ask", l, true);
    }

    fn expired(&mut self, e: &LedgerEntry) {
        let d = &e.data;
        let Some(&i) = text(d, "correlation_id").and_then(|c| self.ask_at.get(c)) else {
            return;
        };
        let a = &mut self.asks[i];
        a.outcome = "expired".into();
        a.answered_at_ms = Some(e.at_unix_ms);
        a.answered_position = Some(e.position);
        a.latency_ms = Some(num(d, "waited_ms"));
        let l = format!(
            "question about `{}` expired unanswered after {}",
            a.tool,
            span(num(d, "waited_ms"))
        );
        self.line(e, "ask", l, true);
    }

    fn wake_fired(&mut self, e: &LedgerEntry) {
        let d = &e.data;
        let id = text(d, "wake_id").unwrap_or("?").to_string();
        let late = num(d, "late_ms");
        let repeats = d.get("every").is_some();
        if !repeats {
            self.wakes.retain(|x| x.wake != id);
        } else if let Some(w) = self.wakes.iter_mut().find(|x| x.wake == id) {
            w.due_at_ms = num(d, "next_due_at_ms");
        }
        let down = d.get("while_down") == Some(&Value::Bool(true));
        if late > LATE_WAKE_MS || down {
            let l = format!(
                "wake {id} fired {} late{}",
                span(late),
                if down {
                    " (it came due while the daemon was down)"
                } else {
                    ""
                }
            );
            self.line(e, "wake", l, true);
            if down {
                let at = e.at_unix_ms;
                let lw = LateWake {
                    position: e.position,
                    at_ms: at,
                    session: e.session_id.clone().unwrap_or_default(),
                    wake: id,
                    late_ms: late,
                };
                let target = match &mut self.cur {
                    Some(r) => Some(r),
                    None => self.restarts.last_mut(),
                };
                if let Some(r) = target {
                    r.wakes_late.push(lw);
                }
            }
        }
    }

    fn startup_step(&mut self, e: &LedgerEntry) {
        let d = &e.data;
        if num(d, "step") == 1 && d.get("last_position").and_then(Value::as_u64) == Some(0) {
            self.first_start = true;
        }
        let r = self.restart();
        match num(d, "step") {
            3 => r.spool_drained = num(d, "drained"),
            4 => {
                if let Some(rep) = d.get("report") {
                    for k in ["marked_unknown", "resolved_unknown"] {
                        if let Some(a) = rep.get(k).and_then(Value::as_array) {
                            r.unknown
                                .extend(a.iter().filter_map(Value::as_str).map(String::from));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// A turn is notable if a notable line falls in its time.
    fn mark_notable(&mut self) {
        for (l, notable) in &self.lines {
            if !*notable {
                continue;
            }
            let Some(s) = &l.session else { continue };
            for t in self.turns.iter_mut().filter(|t| &t.session == s) {
                if l.at_ms >= t.start_ms && t.end_ms.is_none_or(|end| l.at_ms <= end) {
                    t.notable = true;
                }
            }
        }
    }

    /// Every line, the turns' included, in time order, quiet runs folded.
    fn timeline(&self, grouped: bool) -> Vec<Line> {
        let mut lines: Vec<Line> = self.lines.iter().map(|(l, _)| l.clone()).collect();
        for t in &self.turns {
            turn_lines(t, &mut lines);
        }
        lines.sort_by_key(|l| (l.at_ms, l.position));
        fold(lines, grouped)
    }

    /// The reservations still held: planned, never settled (or settled
    /// unknown, which holds the money until it is resolved).
    fn held(&self) -> Vec<Held> {
        let mut held = Vec::new();
        for id in &self.call_order {
            let Some(c) = self.calls.get(id) else {
                continue;
            };
            let unknown = c.settled == Some("unknown");
            if c.reserved > 0.0 && (c.settled.is_none() || unknown) {
                held.push(Held {
                    position: c.planned_pos,
                    session: c.session.clone(),
                    correlation_id: id.clone(),
                    tool: c.tool.clone(),
                    usd: c.reserved,
                    since_ms: c.planned_ms,
                    unknown,
                });
            }
        }
        held
    }

    fn cost(&self, held: &[Held]) -> Cost {
        let rows = |m: &BTreeMap<String, (f64, u32)>| {
            let mut v: Vec<CostRow> = m
                .iter()
                .map(|(k, (u, n))| CostRow {
                    key: k.clone(),
                    usd: *u,
                    calls: *n,
                })
                .collect();
            v.sort_by(|a, b| b.usd.total_cmp(&a.usd).then(a.key.cmp(&b.key)));
            v
        };
        Cost {
            total_usd: self.total,
            by_session: rows(&self.cost_session),
            by_model: rows(&self.cost_model),
            held: held.to_vec(),
            held_usd: held.iter().map(|h| h.usd).sum(),
        }
    }

    fn finish(mut self, w: &Window<'_>) -> Report {
        self.mark_notable();
        let timeline = self.timeline(!w.scope.is_empty());
        let first_failure = timeline
            .iter()
            .find(|l| {
                l.text.contains("FAILED")
                    || l.text.contains("failed")
                    || l.text.contains("was refused")
            })
            .cloned();
        let held = self.held();
        let cost = self.cost(&held);
        let held_usd = cost.held_usd;
        let by_session = cost.by_session.clone();

        let mut open_turns: Vec<OpenTurn> = self
            .turns
            .iter()
            .filter(|t| t.fate == Fate::Running)
            .map(|t| OpenTurn {
                position: t.start_pos,
                session: t.session.clone(),
                turn: t.id.clone(),
                since_ms: t.start_ms,
            })
            .collect();
        open_turns.sort_by_key(|t| t.position);
        let questions: Vec<Asked> = self
            .asks
            .iter()
            .filter(|a| a.outcome == "open")
            .cloned()
            .collect();
        let summary = Summary {
            sessions: self.sessions.clone(),
            tasks: self.tasks.clone(),
            turns: self.turns.len() as u32,
            turns_failed: self.turns_failed,
            spent_usd: self.total,
            top_spenders: by_session.into_iter().take(3).collect(),
            restarts: self.restarts.len() as u32,
            asked: self.asks.len() as u32,
            answered: self
                .asks
                .iter()
                .filter(|a| a.outcome == "approved" || a.outcome == "declined")
                .count() as u32,
            expired: self.asks.iter().filter(|a| a.outcome == "expired").count() as u32,
            first_failure,
        };
        Report {
            since_ms: w.since_ms,
            until_ms: w.until_ms,
            scope: w.scope.to_vec(),
            rows: w.rows.len(),
            summary,
            restarts: self.restarts,
            timeline,
            cost,
            asked: self.asks,
            open: Open {
                turns: open_turns,
                questions,
                wakes: self.wakes,
                reservations: held,
                reserved_usd: held_usd,
            },
        }
    }
}

/// A turn's lines: a start and an end when it was notable (or did not end
/// well), else one quiet line for the fold.
fn turn_lines(t: &Turn, lines: &mut Vec<Line>) {
    let took = span(t.end_ms.unwrap_or(t.start_ms).saturating_sub(t.start_ms));
    let at = t.end_ms.unwrap_or(t.start_ms);
    let line = |kind: &str, at_ms, position, text| Line {
        at_ms,
        position,
        session: Some(t.session.clone()),
        kind: kind.into(),
        text,
        usd: Some(t.usd),
        ..Default::default()
    };
    if !t.notable && t.fate == Fate::Ended {
        let text = format!(
            "turn {} ({}) ended in {took}, {}; {}",
            t.id,
            t.trigger,
            usd(t.usd),
            t.detail
        );
        lines.push(Line {
            folded: 1,
            ..line("quiet", at, t.end_pos, text)
        });
        return;
    }
    lines.push(Line {
        usd: None,
        ..line(
            "turn",
            t.start_ms,
            t.start_pos,
            format!("turn {} started ({})", t.id, t.trigger),
        )
    });
    let end = match t.fate {
        Fate::Ended => format!(
            "turn {} ended in {took}, {}; {}",
            t.id,
            usd(t.usd),
            t.detail
        ),
        Fate::Failed => format!("turn {} FAILED after {took}: {}", t.id, t.detail),
        Fate::Refused => format!("turn {} was refused: {}", t.id, t.detail),
        Fate::Interrupted => format!("turn {} interrupted by a restart of the daemon", t.id),
        Fate::Running => return,
    };
    lines.push(line("turn", at, t.end_pos, end));
}

/// Fold each run of quiet turns (no other line between them; within one
/// session's lines when the report is grouped by session) of `FOLD_MIN` or
/// more into one line; a shorter run stays as its turns' own lines.
fn fold(lines: Vec<Line>, grouped: bool) -> Vec<Line> {
    let mut parts: Vec<(Option<String>, Vec<Line>)> = Vec::new();
    for l in lines {
        let key = if grouped { l.session.clone() } else { None };
        match parts.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(l),
            None => parts.push((key, vec![l])),
        }
    }
    let mut out: Vec<Line> = Vec::new();
    for (_, part) in parts {
        let mut run: Vec<Line> = Vec::new();
        for l in part {
            if l.kind == "quiet" {
                run.push(l);
            } else {
                flush(&mut out, &mut run);
                out.push(l);
            }
        }
        flush(&mut out, &mut run);
    }
    out.sort_by_key(|l| (l.at_ms, l.position));
    out
}

fn flush(out: &mut Vec<Line>, run: &mut Vec<Line>) {
    if run.len() >= FOLD_MIN {
        let usd_sum: f64 = run.iter().map(|l| l.usd.unwrap_or(0.0)).sum();
        let n = run.len();
        out.push(Line {
            at_ms: run[0].at_ms,
            position: run[n - 1].position,
            session: run[0].session.clone(),
            kind: "quiet".into(),
            text: format!("… {n} turns, {}, nothing notable …", usd(usd_sum)),
            folded: n as u32,
            usd: Some(usd_sum),
        });
    } else {
        for mut l in run.drain(..) {
            l.folded = 0;
            out.push(l);
        }
    }
    run.clear();
}

#[cfg(test)]
#[path = "postmortem_facts_tests.rs"]
mod tests;
