//! Situations (M6 step 35a, design §2.11): what a compile is for, and
//! whether its request closes.
//!
//! - **The situation** is a compiler input. The compile step tells it from
//!   what it holds (`given`): no compilation yet is a conversation's or a
//!   task's first compile; a session's first compile in this daemon's run,
//!   with nothing new brought, is a resume; a detour is its own; else a
//!   continuation. The compile settles it (`settle`): a compile that made a
//!   new compilation is a recompile, with its trigger (every view turn's first
//!   call makes one, `view_turn`; the transcript's other triggers went with it
//!   on 2026-11-22).
//! - **The check** (`closed`) is a pure pass over the request after the
//!   compile: a set that does not close (a result whose call is absent, a
//!   call with no result) is `Unadmitted`, which the turn fails as
//!   `context_unadmitted` before anything is sent. A call with no recorded
//!   result still gets its synthetic one (a repair), as before, so the set
//!   closes.
//! - **What a situation admits** was a table of piece classes (`admits`):
//!   gone with issue 46. The view decides what a request holds, and the
//!   table checked a model of the transcript's selection that the view's
//!   request is not built from. Old `context.unadmitted` rows that name a
//!   class (`not_admitted`) still read, and say so in words
//!   (`fact::situation::words`).
//!

use std::collections::HashSet;
use std::sync::Mutex;

use serde_json::Value;

use super::{renderable, Compilation};
use crate::node::Node;
use crate::stub::Stub;

pub use theseus_protocol::Situation;

/// The precedence line (§2.11), after the persona in the system's header:
/// fixed text, never a model's or a note's. Static, so it costs each
/// session one `system_changed` recompile, the day it ships.
pub const PRECEDENCE: &str = "When sources disagree, trust them in this order: what this turn's \
tools just returned; the operator's current request; the recent conversation; older conversation \
and summaries; recalled notes, which are dated testimony.";

/// What the compile step tells from what it holds: `first` (no compilation
/// yet), `task`, `resumed` (the session's first compile in this daemon's
/// run, with nothing new brought).
pub fn given(first: bool, task: bool, resumed: bool) -> Situation {
    match (first, task, resumed) {
        (true, true, _) => Situation::TaskStart,
        (true, false, _) => Situation::ConversationStart,
        (false, _, true) => Situation::Resume,
        (false, _, false) => Situation::Continuation,
    }
}

/// The situation a compile settles in: a detour stays one; a new
/// compilation is a first compile when there was none (unless the ring cut
/// it) and else a recompile with its trigger; an append is a continuation
/// or a resume.
pub fn settle(given: &Situation, first: bool, new: bool, trigger: Option<&str>) -> Situation {
    if *given == Situation::Detour {
        return Situation::Detour;
    }
    let ringed = trigger == Some("overflow");
    match (new, first) {
        (true, true) if !ringed => match given {
            Situation::TaskStart => Situation::TaskStart,
            _ => Situation::ConversationStart,
        },
        (true, _) => Situation::Recompile {
            trigger: trigger.unwrap_or("unknown").to_string(),
        },
        (false, _) => match given {
            Situation::Resume => Situation::Resume,
            _ => Situation::Continuation,
        },
    }
}

/// A compilation's prefix and tail, as the render selects them, each node
/// with its position: the prefix is the included renderable nodes at or
/// before its `as_of`, and its recall section wherever it was written; the
/// tail every renderable node after it but the section.
pub(super) type Selected<'n> = (Vec<(u64, &'n Node)>, Vec<(u64, &'n Node)>);

pub(super) fn selected<'n>(c: &Compilation, nodes: &'n [(u64, Stub)]) -> Selected<'n> {
    let included: HashSet<&str> = c.includes.iter().map(String::as_str).collect();
    // Stub fields alone, so only the selected nodes are decoded (step 33).
    let section = |n: &Stub| c.recall_id.as_deref() == Some(n.id.as_str());
    let prefix = nodes
        .iter()
        .filter(|(pos, n)| {
            (*pos <= c.as_of
                && (included.contains(n.id.as_str()) || crate::fork::inherited(c, *pos))
                && renderable(n))
                || section(n)
        })
        .map(|(p, n)| (*p, &**n))
        .collect();
    // A range a split moved stays out of a tail as it does out of a recompile, and a record
    // undone renders no note (theseus-q9b85); a frozen prefix is never changed after the fact.
    let moves = crate::moved::read(nodes);
    let tail = nodes
        .iter()
        .filter(|(pos, n)| *pos > c.as_of && renderable(n) && !section(n) && moves.shows(n))
        .map(|(p, n)| (*p, &**n))
        .collect();
    // A moved range's note stands where the range began.
    (crate::moved::in_place(prefix), tail)
}

/// A request that may not be sent: a set that does not close, or (from the
/// standing check) a dropped record. An old row may also say a piece its
/// situation did not admit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unadmitted {
    /// `unclosed` or `standing_missing`; an old row may say `not_admitted`.
    pub why: &'static str,
    /// The piece, named: `this turn's recall_note rcn_…`, `the result of
    /// call toolu_…`.
    pub piece: String,
    /// What it means, in a sentence.
    pub detail: String,
}

/// Each `tool_result` answers a `tool_use` of the assistant message just
/// before it, and each `tool_use` has its result in the message after it.
pub fn closed(messages: &[Value]) -> Result<(), Unadmitted> {
    let ids = |m: &Value, kind: &str, key: &str| -> Vec<String> {
        m["content"]
            .as_array()
            .map(|bs| {
                bs.iter()
                    .filter(|b| b["type"] == kind)
                    .filter_map(|b| b[key].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let none = Value::Null;
    for (i, m) in messages.iter().enumerate() {
        let before = i.checked_sub(1).map_or(&none, |j| &messages[j]);
        let after = messages.get(i + 1).unwrap_or(&none);
        let calls = |m: &Value| match m["role"] == "assistant" {
            true => ids(m, "tool_use", "id"),
            false => vec![],
        };
        for r in ids(m, "tool_result", "tool_use_id") {
            if !calls(before).contains(&r) {
                return Err(Unadmitted {
                    why: "unclosed",
                    piece: format!("tool_result {r}"),
                    detail: format!("the result of call {r} is sent without its call"),
                });
            }
        }
        let answered = ids(after, "tool_result", "tool_use_id");
        for u in calls(m) {
            if !answered.contains(&u) {
                return Err(Unadmitted {
                    why: "unclosed",
                    piece: format!("tool_use {u}"),
                    detail: format!("call {u} is sent without its result"),
                });
            }
        }
    }
    Ok(())
}

/// The sessions this daemon's run has compiled: a session not among them,
/// compiling with nothing new brought, resumes.
#[derive(Default)]
pub struct RunCompiles {
    seen: Mutex<HashSet<String>>,
}

impl RunCompiles {
    /// Whether `session_id` was compiled in this run before.
    pub fn seen(&self, session_id: &str) -> bool {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(session_id)
    }

    /// `session_id` compiled.
    pub fn mark(&self, session_id: &str) {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_compile_settles_the_situation() {
        assert_eq!(given(true, true, false), Situation::TaskStart);
        assert_eq!(given(true, false, true), Situation::ConversationStart);
        assert_eq!(given(false, false, true), Situation::Resume);
        assert_eq!(given(false, true, false), Situation::Continuation);
        let resume = Situation::Resume;
        assert_eq!(settle(&resume, false, false, None), Situation::Resume);
        assert_eq!(
            settle(&resume, false, true, Some("system_changed")),
            Situation::Recompile {
                trigger: "system_changed".into()
            }
        );
        let task = Situation::TaskStart;
        assert_eq!(settle(&task, true, true, Some("new_session")), task);
        assert_eq!(
            settle(&task, true, true, Some("overflow")),
            Situation::Recompile {
                trigger: "overflow".into()
            }
        );
        assert_eq!(
            settle(&Situation::Detour, true, true, Some("new_session")),
            Situation::Detour
        );
        let wire = serde_json::to_value(Situation::Recompile {
            trigger: "overflow".into(),
        })
        .unwrap();
        assert_eq!(wire, json!({"kind": "recompile", "trigger": "overflow"}));
        let newer: Situation = serde_json::from_value(json!({"kind": "lesson_review"})).unwrap();
        assert_eq!(newer, Situation::Unknown);
    }

    fn assistant(uses: &[&str]) -> Value {
        let content: Vec<Value> = uses
            .iter()
            .map(|u| json!({"type": "tool_use", "id": u, "name": "fs_read", "input": {}}))
            .collect();
        json!({"role": "assistant", "content": content})
    }

    fn results(ids: &[&str]) -> Value {
        let content: Vec<Value> = ids
            .iter()
            .map(|u| json!({"type": "tool_result", "tool_use_id": u, "content": "ok"}))
            .collect();
        json!({"role": "user", "content": content})
    }

    /// A set closes when each result follows its call and each call has its
    /// result; otherwise the check names the piece.
    #[test]
    fn a_set_that_does_not_close_names_its_piece() {
        let ask = json!({"role": "user", "content": [{"type": "text", "text": "read it"}]});
        let ok = [ask.clone(), assistant(&["toolu_a"]), results(&["toolu_a"])];
        assert_eq!(closed(&ok), Ok(()));
        let orphan = [
            ask.clone(),
            assistant(&["toolu_a"]),
            results(&["toolu_a", "toolu_b"]),
        ];
        let e = closed(&orphan).unwrap_err();
        assert_eq!(
            (e.why, e.piece.as_str()),
            ("unclosed", "tool_result toolu_b")
        );
        let unanswered = [
            ask,
            assistant(&["toolu_a", "toolu_c"]),
            results(&["toolu_a"]),
        ];
        let e = closed(&unanswered).unwrap_err();
        assert_eq!(e.piece, "tool_use toolu_c");
        let first = [results(&["toolu_d"])];
        assert_eq!(closed(&first).unwrap_err().piece, "tool_result toolu_d");
    }

    /// An old store's situations still read: every value a compilation or a
    /// failure row was written with, a detour's and a resume's included,
    /// decodes as it was written (issue 46 removed the table, not the label).
    #[test]
    fn an_old_stores_situations_still_decode() {
        let read = |v: Value| serde_json::from_value::<Situation>(v).unwrap();
        assert_eq!(read(json!({"kind": "detour"})), Situation::Detour);
        assert_eq!(read(json!({"kind": "resume"})), Situation::Resume);
        assert_eq!(read(json!({"kind": "task_start"})), Situation::TaskStart);
        assert_eq!(
            read(json!({"kind": "recompile", "trigger": "overflow"})),
            Situation::Recompile {
                trigger: "overflow".into()
            }
        );
    }

    /// An old `context.unadmitted` row that names a piece its situation did
    /// not admit is still said in words, as it was before the table went.
    #[test]
    fn an_old_not_admitted_row_is_still_said() {
        let old = Unadmitted {
            why: "not_admitted",
            piece: "this turn's recall_note rcn_1".into(),
            detail: "a task_start compile does not admit this turn's recall_note rcn_1".into(),
        };
        let said = crate::fact::situation::words(&old);
        assert!(said.contains("admits a piece it may not"), "{said}");
        assert!(
            said.contains("rcn_1") && said.ends_with("Nothing was sent."),
            "{said}"
        );
    }
}
