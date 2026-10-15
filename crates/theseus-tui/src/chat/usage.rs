//! `/context` and `/cost` (theseus-wy6v.2): how full the context is, by part,
//! and what the conversation has cost, turn by turn, against the day's
//! ceiling. `/context` is one `context.explain` call, worded by
//! `theseus_protocol::fill` (the short form of `theseus why`); `/cost` is one
//! `health` call for the day, and the turns the chat itself has seen end.

use serde_json::{json, Value};
use theseus_protocol::context::ContextExplainResult;
use theseus_protocol::{fill, method, DayCeilingBudget, TurnSubmitResult};

use super::state::{call, Chat, Effect, Purpose};
use super::style::Sty;

/// The turns `/cost` lists; the rest are summed.
const LISTED: usize = 10;

pub(super) fn context(chat: &mut Chat, _args: &str) -> Vec<Effect> {
    match chat.session.clone() {
        Some(sid) => vec![call(
            method::CONTEXT_EXPLAIN,
            json!({ "session_id": sid }),
            Purpose::Context,
        )],
        None => {
            chat.flash("the conversation is not open yet");
            Vec::new()
        }
    }
}

pub(super) fn cost(_chat: &mut Chat, _args: &str) -> Vec<Effect> {
    vec![call(method::HEALTH, Value::Null, Purpose::Cost)]
}

impl Chat {
    /// A turn ended: how full the context was at its last request, and what
    /// it cost.
    pub(super) fn note_usage(&mut self, r: &TurnSubmitResult) {
        if let (Some(t), Some(w)) = (r.context_tokens, r.context_window) {
            self.fill = Some((t, w));
        }
        self.turn_costs.push(r.cost_usd);
    }

    /// `context.explain`'s answer, short.
    pub(super) fn say_context(&mut self, v: Value) {
        let Ok(x) = serde_json::from_value::<ContextExplainResult>(v) else {
            self.say_words(
                Sty::Yellow,
                "the daemon's answer about the context could not be read",
            );
            return;
        };
        // The status line takes the explained total too, so a resumed
        // conversation shows its share before its next turn.
        if let (Some(w), false) = (x.window, x.parts.is_empty()) {
            self.fill = Some((x.parts.iter().map(|p| p.tokens).sum(), w));
        }
        self.gap();
        for l in fill::context_short(&x) {
            let sty = if l.starts_with('`') {
                Sty::Dim
            } else {
                Sty::Plain
            };
            self.say_plain(sty, &l);
        }
        self.gap();
    }

    /// `health`'s answer, for the day; the turns are the chat's own.
    pub(super) fn say_cost(&mut self, v: Value) {
        let day = v
            .get("day_spend")
            .and_then(|d| serde_json::from_value::<DayCeilingBudget>(d.clone()).ok());
        let lines = cost_lines(&self.turn_costs, self.cost, day.as_ref());
        self.gap();
        for l in lines {
            self.say_plain(Sty::Plain, &l);
        }
        self.gap();
    }
}

/// Four decimals under a dollar, two from a dollar up, as the turn lines say it.
fn money(x: f64) -> String {
    if (x * 1e4).round() < 1e4 {
        format!("${x:.4}")
    } else {
        format!("${x:.2}")
    }
}

/// The conversation's cost: the last [`LISTED`] turns this chat saw end, each
/// on a line, and everything before them in one, so the lines sum to
/// `session_total`; then the day against its ceiling.
pub fn cost_lines(
    turns: &[Option<f64>],
    session_total: f64,
    day: Option<&DayCeilingBudget>,
) -> Vec<String> {
    let first = turns.len().saturating_sub(LISTED);
    let shown = &turns[first..];
    let listed: f64 = shown.iter().map(|c| c.unwrap_or(0.0)).sum();
    let total = session_total.max(listed);
    let mut out = vec![format!("this conversation: {}", money(total))];
    let before = (total - listed).max(0.0);
    if before > 1e-6 || first > 0 {
        let what = match first {
            0 => "before this chat opened".to_string(),
            n => format!("{n} earlier turns and what came before this chat"),
        };
        out.push(format!("  {:>10}  {what}", money(before)));
    }
    for (i, c) in shown.iter().enumerate() {
        let words = c.map_or("not priced".to_string(), money);
        out.push(format!("  {words:>10}  turn {}", first + i + 1));
    }
    out.push(match day {
        Some(d) if d.ceiling_usd > 0.0 => format!(
            "today: {} of the {} daily ceiling ({:.0}%)",
            money(d.spent_usd),
            money(d.ceiling_usd),
            d.spent_usd * 100.0 / d.ceiling_usd
        ),
        Some(d) => format!("today: {}", money(d.spent_usd)),
        None => "today: the daemon did not say".to_string(),
    });
    if let Some(l) = theseus_client::render::day::first_line(day) {
        out.push(l);
    }
    out
}
