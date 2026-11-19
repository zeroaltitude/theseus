//! `look.work`: Theseus tells the owner's own turn what work it has going (issue 28): all of it, as the
//! command line's `tasks` and `status` do.
//!
//! A turn ran `theseus tasks`, `theseus status` and `theseus wake list` in a shell for exactly this: the
//! command line is a screen for the owner's terminal, and the daemon now answers it with this tool
//! (`own_command`). The words are the daemon's own views, read with no shell.
//!
//! **Whose it is: Theseus's, whole** (the owner, 2026-11-19: "the scope should be global for tasks,
//! projects, etc. It's weird if you get bad answers to things you feel like you know about"). The
//! tasks of every conversation, each with where it was started; what waits on the owner anywhere, each
//! with where; the open projects; the routines; and the reminders and check-ins coming. This
//! conversation's come first. The views are the ones the command line reads (`Core::tasks`,
//! `confirm_list`, `project_list`, `routine_list`, `wakes`), so nothing is given that `theseus tasks`,
//! `status` and `wake list` would not print. **History stays this conversation's**: its last
//! messages; another conversation's are `theseus history SESSION`, which runs as it does. A call may
//! say `scope: here` for this conversation alone.
//!
//! **Another conversation's words** are marked as such in the answer, and held back when that
//! conversation holds data marked anything but open (`reader.rs`); the answer is then judged as every
//! tool's result is.
//!
//! **The wall is unchanged** (`look::refusal`): the owner's own turn in a private place, nobody else.
//!
//! **Bounded**, and it says what it left out: per list at most 10 to 15 lines for this conversation
//! and 15 for the rest (see the constants), a message of at most 600 characters (20 at most), a part
//! of at most 8,000 characters, and 24,000 in all (a deferred tool: the result is paid only when it is
//! called). **Never** an answer: it lists what waits, and the owner answers it.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::Core;

mod history;
mod horizon;
mod owed;
mod reader;
mod running;
mod tasks;

/// The parts of the look, in the order they are given.
const PARTS: [&str; 7] = [
    "running",
    "tasks",
    "needs_you",
    "projects",
    "routines",
    "coming",
    "history",
];
/// The parts that are not tied to one conversation: `scope: here` leaves them out.
const WHOLE_ONLY: [&str; 2] = ["projects", "routines"];
/// How many messages `history` gives when it is not told, and the most it may.
const MESSAGES: usize = 6;
const MESSAGES_MAX: usize = 20;
/// The most tasks listed, from this conversation and from the rest, and the most the read takes from the
/// task view for the whole (the newest open ones; the rest are counted, not read).
const TASKS_HERE_MAX: usize = 10;
const TASKS_ELSEWHERE_MAX: usize = 15;
const TASKS_READ_MAX: usize = 60;
/// The most conversations running, cards, and thread questions listed.
const RUNNING_MAX: usize = 12;
const CARDS_MAX: usize = 10;
const THREAD_ASKS_MAX: usize = 10;
/// The most questions listed, from this place and from the rest.
const WAITING_HERE_MAX: usize = 10;
const WAITING_ELSEWHERE_MAX: usize = 15;
/// The most projects, routines, and reminders and check-ins listed.
const PROJECTS_MAX: usize = 12;
const ROUTINES_MAX: usize = 12;
const COMING_MAX: usize = 12;
/// The most characters of one message, of one part, and of the whole answer.
const LINE_CHARS: usize = 600;
const TOTAL_CHARS: usize = 8_000;
const ANSWER_CHARS: usize = 24_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    what: Option<Vec<String>>,
    #[serde(default)]
    messages: Option<u32>,
    #[serde(default)]
    scope: Option<String>,
}

/// What was asked: the parts, in order, how many messages, and whether it is this conversation's alone.
pub(super) struct Ask {
    parts: Vec<&'static str>,
    messages: usize,
    here: bool,
}

impl Ask {
    pub(super) fn summary(&self) -> String {
        let scope = if self.here {
            "this conversation"
        } else {
            "all"
        };
        format!("{} ({scope})", self.parts.join(", "))
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
    let here = match i.scope.as_deref().map(str::trim) {
        None | Some("all") => false,
        Some("here") => true,
        Some(other) => {
            return Err(format!(
                "`{other}` is not a scope: choose `all` (the default) or `here`"
            ))
        }
    };
    let messages = (i.messages.unwrap_or(MESSAGES as u32) as usize).clamp(1, MESSAGES_MAX);
    Ok(Ask {
        parts,
        messages,
        here,
    })
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
            "scope": {"enum": ["all", "here"]},
            "messages": {
                "type": "integer", "minimum": 1, "maximum": MESSAGES_MAX
            }
        },
        "additionalProperties": false
    })
}

/// The words for each part asked for: Theseus's whole work, this conversation's first, and its own
/// messages (`session`'s).
pub(super) async fn words(
    core: &std::sync::Arc<Core>,
    session: &str,
    ask: &Ask,
    restricted: bool,
) -> String {
    let r = reader::Reader::new(core, session, restricted);
    let mut out = String::new();
    for p in &ask.parts {
        let part = match (*p, ask.here) {
            (part, true) if WHOLE_ONLY.contains(&part) => format!(
                "{}: not tied to one conversation; ask for scope `all` to see them.\n",
                part[..1].to_uppercase() + &part[1..]
            ),
            ("running", _) => running::running(core, &r, ask.here).await,
            ("tasks", _) => tasks::tasks(&r, ask.here),
            ("needs_you", _) => tasks::needs_you(&r, ask.here),
            ("projects", _) => horizon::projects(&r),
            ("routines", _) => horizon::routines(&r),
            ("coming", _) => horizon::coming(&r, ask.here),
            _ => history::history(core, session, ask.messages),
        };
        out.push_str(&cut(part, TOTAL_CHARS));
        out.push('\n');
    }
    cut(out, ANSWER_CHARS)
}

/// `s` as one line of at most `max` characters: control characters and line breaks gone.
fn clip(s: &str, max: usize) -> String {
    let plain: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one: String = plain.split_whitespace().collect::<Vec<_>>().join(" ");
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
