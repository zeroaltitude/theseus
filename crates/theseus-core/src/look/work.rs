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
//! **Another conversation's words** are marked as such in the answer, and held back from a turn whose
//! model the labels could keep off when that conversation holds data marked anything but open
//! (`reader.rs`); the answer is then judged as every tool's result is.
//!
//! **The wall is unchanged** (`look::refusal`): the owner's own turn in a private place, nobody else.
//!
//! **Bounded**, and it says what it left out: per list at most 10 to 15 lines for this conversation
//! and 15 for the rest (see the constants), a message of at most 600 characters (20 at most), a part
//! of at most 8,000 characters, and 24,000 in all (a deferred tool: the result is paid only when it is
//! called). **Never** an answer: it lists what waits, and the owner answers it. Every cut, of a line, a part
//! or the answer, stops at a known value's edge (the call's `Cuts`), so the answer holds each value the
//! scrubber knows whole or not at all. When the snapshot is settled (no value still loading) and neither a
//! text nor its one-line form holds a known value, the cut falls where it fell before, byte for byte. While a
//! value is still loading every cut falls at a blank, or leaves the ellipsis alone when there is none before
//! it, in text that holds no known value too, as every cut of a snapshot then does.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::scrub::Cuts;
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
/// The most conversations running, and cards, listed.
const RUNNING_MAX: usize = 12;
const CARDS_MAX: usize = 10;
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
/// messages (`session`'s); and the messages the history part shows (what the result copies, `copies`): those
/// some of whose words its own cuts leave in the answer ([`assemble`]). Every cut stops at the edge of a value
/// `cuts` knows.
pub(super) async fn words(
    core: &std::sync::Arc<Core>,
    who: &Caller<'_>,
    ask: &Ask,
    cuts: &Cuts,
) -> Result<(String, Vec<String>), String> {
    let session = who.session;
    let r = reader::Reader::new(core, session, cuts);
    let mut parts = Vec::new();
    for p in &ask.parts {
        let mut shown = Vec::new();
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
            _ => {
                let (words, spans) = history::history(core, session, ask.messages, cuts);
                shown = spans;
                words
            }
        };
        parts.push((part, shown));
    }
    // Another conversation's marked words, given to this one: it takes their flags on, before the result
    // is written. With content judging switched off (`[content_based_security] session_flags = false`)
    // no source raises a label, so none is taken.
    let at = theseus_protocol::now_unix_ms();
    let flags = r.taken();
    if !flags.is_empty() && !core.runner.judge.content_is_off() {
        let labels = flags
            .iter()
            .copied()
            .map(|f| {
                (
                    crate::content::deterministic::from_look(f, who.turn, at),
                    at,
                )
            })
            .collect();
        core.runner.judge.land_deterministic(session, labels);
        // A label that was not written is no label: words given without their flags are the gap this
        // closes, so the look fails and gives none.
        let now = crate::content_wait::flags_of(&core.store, session);
        if flags
            .iter()
            .any(|f| !now.as_ref().is_some_and(|n| n.has(*f)))
        {
            return Err(
                "look.work could not take on the flags of the conversations whose marked \
                words it would give, so it gave none: try again"
                    .into(),
            );
        }
    }
    let (said, shown) = assemble(parts, cuts);
    Ok((said, shown.into_iter().map(|(id, _, _)| id).collect()))
}

/// A message a part shows: its node id, and where its words are, in characters, `from` to `to`.
pub(super) type Shown = (String, usize, usize);

/// The answer from its parts, in order: each cut to [`TOTAL_CHARS`], then the whole to [`ANSWER_CHARS`], each
/// cut at a known value's edge (`cuts`); and the messages some of whose words both cuts leave in it, each with
/// where those words are in the answer, up to where each cut really fell (D6 round 6). A message a cut leaves
/// out is no copy.
fn assemble(parts: Vec<(String, Vec<Shown>)>, cuts: &Cuts) -> (String, Vec<Shown>) {
    let mut out = String::new();
    let mut shown = Vec::new();
    for (part, spans) in parts {
        let at = out.chars().count();
        let (part, kept) = cut_kept(part, TOTAL_CHARS, cuts);
        shown.extend(within(spans, kept).map(|(id, from, to)| (id, at + from, at + to)));
        out.push_str(&part);
        out.push('\n');
    }
    let (out, kept) = cut_kept(out, ANSWER_CHARS, cuts);
    let shown = within(shown, kept).collect();
    (out, shown)
}

/// The spans that begin within the first `kept` characters, each ended there at the latest: what a cut that keeps
/// those characters leaves of them.
fn within(spans: Vec<Shown>, kept: usize) -> impl Iterator<Item = Shown> {
    (spans.into_iter())
        .filter(move |(_, from, _)| *from < kept)
        .map(move |(id, from, to)| (id, from, to.min(kept)))
}

/// Who asks: the conversation and the turn.
pub(super) struct Caller<'a> {
    pub session: &'a str,
    pub turn: &'a str,
}

/// `s` as one line of at most `max` characters: control characters and line breaks gone. The cut never falls
/// inside a known value (`cuts`), and a value the rewrite would change is cut away before the rewrite.
fn clip(s: &str, max: usize, cuts: &Cuts) -> String {
    clip_kept(s, max, cuts).0
}

/// [`clip`], and the bytes of `s` it kept: the line is [`clip1`] of them, then `…` when anything of `s` but
/// blanks is left out after them. They hold each known value whole or none of it.
fn clip_kept(s: &str, max: usize, cuts: &Cuts) -> (String, std::ops::Range<usize>) {
    let kept = cuts.unchanged(s, |c| c.is_control() || c.is_whitespace());
    let short = kept < s.len();
    let head = &s[..kept];
    let one = clip1(head);
    let one = one.trim();
    // plain cut: a budget only; `cuts.before` below sets where the line ends
    match one.char_indices().nth(max) {
        Some((at, _)) => {
            let p = cuts.before(one, at);
            (
                format!("{}…", &one[..p]),
                crate::view::text::kept::source(head, p, |c| c),
            )
        }
        None => {
            let range = crate::view::text::kept::source(head, one.len(), |c| c);
            match short {
                true => (format!("{one}…"), range),
                false => (one.to_string(), range),
            }
        }
    }
}

/// The form Clip1: `s` with each run of blanks (control characters and whitespace) one space, nothing
/// trimmed; every other character as it is. [`clip`]'s line is it, trimmed, then cut.
/// Frozen once a stored reference names it: a change adds a new numbered form, never edits this one.
pub(crate) fn clip1(s: &str) -> String {
    crate::view::text::kept::one_line(s, |c| c)
}

/// `text` cut to `max` characters, with what was left out said.
#[cfg(test)]
fn cut(text: String, max: usize, cuts: &Cuts) -> String {
    cut_kept(text, max, cuts).0
}

/// [`cut`], and how many characters of `text` it kept: the cut stops at a known value's edge (`cuts`).
fn cut_kept(text: String, max: usize, cuts: &Cuts) -> (String, usize) {
    let (head, cut) = cuts.head(&text, max);
    let kept = head.chars().count();
    match cut {
        false => (text, kept),
        true => (
            format!("{head}…\n[cut at {max} characters; ask for fewer messages or one part]\n"),
            kept,
        ),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_cut;
