//! `look.work`: Theseus tells the owner's own turn what this conversation has going on (issue 28):
//! the tasks it started and what each asks or has spent, what waits on the owner here, and its last
//! messages.
//!
//! A turn ran `theseus tasks`, `theseus status` and `theseus history` in a shell for exactly this:
//! the command line is a screen for the owner's terminal, and the daemon now answers it with this
//! tool (`own_command`). The words are the daemon's own views, read with no shell.
//!
//! **Whose it is.** Only this conversation's, and only what the owner may already see in this
//! place: the tasks this session started (`Core::tasks`), the questions that wait here (the place's
//! own, `chat_acts::waiting_here`: another conversation's never), and this session's own messages.
//! Another session's history, another place's questions, and the whole fleet of tasks are the
//! command line's (`theseus history SESSION`, which runs as it does). **Bounded**: at most 20
//! tasks, 20 messages of at most 600 characters, 8,000 characters in all, and it says what it left
//! out. **Never** an answer: it lists what waits, and the owner answers it.

use std::fmt::Write;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::node::Body;
use crate::Core;

/// The parts of the look, in the order they are given.
const PARTS: [&str; 3] = ["tasks", "needs_you", "history"];
/// How many messages `history` gives when it is not told, and the most it may.
const MESSAGES: usize = 6;
const MESSAGES_MAX: usize = 20;
/// The most tasks listed.
const TASKS_MAX: usize = 20;
/// The most characters of one message, and of the whole answer.
const LINE_CHARS: usize = 600;
const TOTAL_CHARS: usize = 8_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    what: Option<Vec<String>>,
    #[serde(default)]
    messages: Option<u32>,
}

/// What was asked: the parts, in order, and how many messages.
pub(super) struct Ask {
    parts: Vec<&'static str>,
    messages: usize,
}

impl Ask {
    pub(super) fn summary(&self) -> String {
        self.parts.join(", ")
    }
}

pub(super) fn ask_of(input: &Value) -> Result<Ask, String> {
    let i: Input = serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
    let mut parts = Vec::new();
    match i.what.filter(|w| !w.is_empty()) {
        None => parts.extend(PARTS),
        Some(what) => {
            for w in &what {
                let part = PARTS.iter().find(|p| **p == w.trim()).ok_or_else(|| {
                    format!(
                        "`{w}` is not a part of the look: choose from {}",
                        PARTS.join(", ")
                    )
                })?;
                if !parts.contains(part) {
                    parts.push(*part);
                }
            }
            parts.sort_by_key(|p| PARTS.iter().position(|x| x == p));
        }
    }
    let messages = (i.messages.unwrap_or(MESSAGES as u32) as usize).clamp(1, MESSAGES_MAX);
    Ok(Ask { parts, messages })
}

pub(super) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "what": {
                "type": "array",
                "description": "Parts; omit for all.",
                "items": {"type": "string", "enum": PARTS}
            },
            "messages": {
                "type": "integer", "minimum": 1, "maximum": MESSAGES_MAX,
                "description": "How many last messages (default 6)."
            }
        },
        "additionalProperties": false
    })
}

/// The words for each part asked for, about `session` and no other.
pub(super) fn words(core: &Core, session: &str, ask: &Ask) -> String {
    let mut out = String::new();
    for p in &ask.parts {
        let part = match *p {
            "tasks" => tasks(core, session),
            "needs_you" => needs_you(core, session),
            _ => history(core, session, ask.messages),
        };
        out.push_str(&part);
        out.push('\n');
    }
    cut(out, TOTAL_CHARS)
}

fn tasks(core: &Core, session: &str) -> String {
    let list = match core.tasks(Some(session), None) {
        Ok(l) => l,
        Err(_) => return "Tasks: they could not be read just now.\n".into(),
    };
    if list.is_empty() {
        return "Tasks: this conversation has started none.\n".into();
    }
    let mut out = format!("Tasks this conversation started ({}):\n", list.len());
    for t in list.iter().take(TASKS_MAX) {
        out.push_str(&task_line(t));
    }
    if list.len() > TASKS_MAX {
        let _ = writeln!(out, "… and {} older", list.len() - TASKS_MAX);
    }
    out
}

/// One task as a line: its short id, title, state, what it waits on, turns, spend, and what it asks.
fn task_line(t: &theseus_protocol::TaskInfo) -> String {
    let title = t
        .title
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(no title)");
    let waits = t
        .waiting_on
        .as_deref()
        .map(|w| format!(" (waiting on {w})"))
        .unwrap_or_default();
    let asks = match t.pending_confirms {
        0 => String::new(),
        n => format!(" · asks the owner {n}"),
    };
    let ended = t
        .ended_reason
        .as_deref()
        .filter(|r| !r.is_empty())
        .map(|r| format!(" · {}", clip(r, 120)))
        .unwrap_or_default();
    format!(
        "- {} · {} · {}{waits} · {} turn{} · ${:.2} of ${:.2}{asks}{ended}\n",
        t.short,
        clip(title, 100),
        t.state,
        t.turns,
        if t.turns == 1 { "" } else { "s" },
        t.spent_usd,
        t.limit_usd
    )
}

/// One question that waits for the owner, as a line: no answer to it is offered.
fn question_line(q: &theseus_protocol::ConfirmRequest) -> String {
    format!(
        "- {} · {} · {}\n",
        crate::task::short(&q.correlation_id),
        q.tool,
        clip(&q.reason, 200)
    )
}

fn needs_you(core: &Core, session: &str) -> String {
    let waiting = match crate::chat_acts::waiting_here(core, session) {
        Ok(w) => w,
        Err(_) => return "Waiting on the owner: it could not be read just now.\n".into(),
    };
    if waiting.is_empty() {
        return "Waiting on the owner: nothing waits here.\n".into();
    }
    let mut out = format!(
        "Waiting on the owner here ({}); only they answer, this tool answers nothing:\n",
        waiting.len()
    );
    for q in &waiting {
        out.push_str(&question_line(q));
    }
    out
}

fn history(core: &Core, session: &str, messages: usize) -> String {
    let nodes = match core.store.session_nodes(session) {
        Ok(n) => n,
        Err(_) => return "Messages: they could not be read just now.\n".into(),
    };
    let mut lines: Vec<String> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } if !text.trim().is_empty() => {
                let who = match n.origin {
                    crate::node::Origin::Operator => "the owner",
                    crate::node::Origin::Harness => "the harness",
                    _ => "a message",
                };
                Some(format!("{who}: {}", clip(text, LINE_CHARS)))
            }
            Body::AssistantMessage { blocks, model, .. } => {
                let text: String = blocks.iter().filter_map(|b| b["text"].as_str()).collect();
                (!text.trim().is_empty())
                    .then(|| format!("Theseus ({model}): {}", clip(&text, LINE_CHARS)))
            }
            // Another profile's answer, a message of its own (`profile.ask`).
            Body::ToolResult { meta, .. } => {
                theseus_protocol::SubagentAnswer::from_meta(meta).map(|a| {
                    format!(
                        "{} (a subagent, {}): {}",
                        a.profile,
                        a.model,
                        clip(&a.text, LINE_CHARS)
                    )
                })
            }
            _ => None,
        })
        .collect();
    let total = lines.len();
    let kept = lines.split_off(total.saturating_sub(messages));
    if kept.is_empty() {
        return "Messages: this conversation has none yet.\n".into();
    }
    let mut out = format!(
        "The last {} of this conversation's {total} messages:\n",
        kept.len()
    );
    for l in &kept {
        let _ = writeln!(out, "- {l}");
    }
    out
}

/// `s` as one line of at most `max` characters.
fn clip(s: &str, max: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.chars().count() > max {
        true => format!("{}…", one.chars().take(max).collect::<String>()),
        false => one,
    }
}

/// `text` cut to `max` characters, with what was left out said.
fn cut(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…\n[cut at {max} characters; ask for fewer messages or one part]\n")
}

#[cfg(test)]
mod tests;
