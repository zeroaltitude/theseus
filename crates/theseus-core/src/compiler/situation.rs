//! Situations (M6 step 35a, design §2.11): what a compile is for, and what
//! its request may admit.
//!
//! - **The situation** is a compiler input. The compile step tells it from
//!   what it holds (`given`): no compilation yet is a conversation's or a
//!   task's first compile; a session's first compile in this daemon's run,
//!   with nothing new brought, is a resume; a detour is its own; else a
//!   continuation. The compile settles it (`settle`): a compile whose own
//!   triggers made a new compilation (a model, system or tools change, an
//!   operator's request, the ring) is a recompile, with its trigger.
//! - **What each admits** is the table `admits`, built from what the code
//!   admitted before it, so no turn that passed fails: the classes of a
//!   request's pieces, and whether a recall is this turn's (new) or written
//!   before. Lessons are reserved for 35b (row 63) and admitted nowhere.
//! - **The check** (`check`) is a pure pass over the request's pieces after
//!   the compile: a piece its situation does not admit, or a set that does
//!   not close (a result whose call is absent, a call with no result, an
//!   assembled `recall_id` whose node is gone), is `Unadmitted`, which the
//!   turn fails as `context_unadmitted` before anything is sent. A call
//!   with no recorded result still gets its synthetic one (a repair), as
//!   before, so the set closes.
//!
//! The books (P8) will be classes a situation admits: notes and summaries
//! feed the diary.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::{renderable, Compilation, Compiled};
use crate::node::{Body, Node};

pub use theseus_protocol::Situation;

/// The precedence line (§2.11), after the persona in the system's header:
/// fixed text, never a model's or a note's. Static, so it costs each
/// session one `system_changed` recompile, the day it ships.
pub const PRECEDENCE: &str = "When sources disagree, trust them in this order: what this turn's \
tools just returned; the operator's current request; the recent conversation; older conversation \
and summaries; recalled notes, which are dated testimony.";

/// The class of one piece of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// A person's message, a wake, a task's report, or its brief.
    Inbound,
    /// The model's answer.
    Reply,
    /// A call's result, after the answer that made the call.
    ToolResult,
    /// A background job's result, delivered as a line after its call was
    /// answered.
    LateResult,
    /// A call's synthetic result: none was recorded.
    Repair,
    /// A task's arrangement (M5 27).
    Arrangement,
    /// A compaction's summary, first in its prefix (30c).
    Summary,
    /// An assembled prefix's recall section (30c).
    RecallSection,
    /// A recall note in the conversation (30b).
    RecallNote,
    /// The task view, the last block of the request's last message (39a).
    TaskView,
    /// A lesson: reserved for 35b (row 63), which admits it by scope.
    Lesson,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Reply => "reply",
            Self::ToolResult => "tool_result",
            Self::LateResult => "late_result",
            Self::Repair => "repair",
            Self::Arrangement => "arrangement",
            Self::Summary => "summary",
            Self::RecallSection => "recall_section",
            Self::RecallNote => "recall_note",
            Self::TaskView => "task_view",
            Self::Lesson => "lesson",
        }
    }
}

/// Whether `situation` admits a piece of `class`; `new` is a recall this
/// turn brought, not yet written. The table (§2.11, from the code):
///
/// | Situation | Admits |
/// |---|---|
/// | conversation start | the transcript (messages, replies, results), a new recall note, earlier notes, a summary, the task view |
/// | task start | the brief, its arrangement, a new assembled recall section, the task view |
/// | continuation | the prefix and the tail (its arrangement, summary, and section included), a new recall note |
/// | recompile | all of it, a new section (a compaction's) and a new note included |
/// | resume | the manifest's prefix and the tail; no new recall |
/// | detour | the last exchanges and the message: no recall, summary, arrangement, or task view |
///
/// A conversation's first compile may carry replies, earlier notes, and
/// a summary: a session whose turns each took a detour (25e) wrote them
/// but never a compilation of its own.
pub fn admits(situation: &Situation, class: Class, new: bool) -> bool {
    use Class::*;
    let common = matches!(class, Inbound | Reply | ToolResult | LateResult | Repair);
    match situation {
        Situation::ConversationStart => common || matches!(class, Summary | TaskView | RecallNote),
        Situation::TaskStart => {
            common || matches!(class, Arrangement | TaskView) || (class == RecallSection && new)
        }
        Situation::Continuation => {
            common
                || matches!(class, Arrangement | Summary | TaskView | RecallNote)
                || (class == RecallSection && !new)
        }
        Situation::Recompile { .. } | Situation::Unknown => class != Lesson,
        Situation::Resume => {
            common
                || matches!(class, Arrangement | Summary | TaskView)
                || (matches!(class, RecallNote | RecallSection) && !new)
        }
        Situation::Detour => common,
    }
}

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

pub(super) fn selected<'n>(c: &Compilation, nodes: &'n [(u64, Arc<Node>)]) -> Selected<'n> {
    let included: HashSet<&str> = c.includes.iter().map(String::as_str).collect();
    let section = |n: &Node| c.recall_id.as_deref() == Some(n.id.as_str());
    let prefix = nodes
        .iter()
        .filter(|(pos, n)| {
            (*pos <= c.as_of && included.contains(n.id.as_str()) && renderable(n)) || section(n)
        })
        .map(|(p, n)| (*p, &**n))
        .collect();
    let tail = nodes
        .iter()
        .filter(|(pos, n)| *pos > c.as_of && renderable(n) && !section(n))
        .map(|(p, n)| (*p, &**n))
        .collect();
    (prefix, tail)
}

/// One piece of a request: its class, the node it renders (none for a
/// repair's or the task view's), and whether it is a recall this turn
/// brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub class: Class,
    pub node_id: Option<String>,
    pub new: bool,
}

impl Piece {
    fn named(&self) -> String {
        let new = if self.new { "this turn's " } else { "" };
        match &self.node_id {
            Some(id) => format!("{new}{} {id}", self.class.as_str()),
            None => format!("{new}{}", self.class.as_str()),
        }
    }
}

/// The pieces of `compiled`'s request, from its compilation's selection
/// over `nodes`: each rendered node's, each repair's, and the task view's
/// when `task_view`. A recall positioned past `last_position` is new: the
/// turn holds it until its plan frame writes it.
pub fn pieces(
    compiled: &Compiled,
    nodes: &[(u64, Arc<Node>)],
    last_position: u64,
    task_view: bool,
) -> Vec<Piece> {
    let c = &compiled.compilation;
    let (prefix, tail) = selected(c, nodes);
    let mut out = Vec::new();
    let all = prefix
        .iter()
        .map(|e| (e, true))
        .chain(tail.iter().map(|e| (e, false)));
    for (&(pos, n), in_prefix) in all {
        let class = match &n.body {
            Body::UserMessage { .. } => Class::Inbound,
            Body::AssistantMessage { .. } => Class::Reply,
            Body::ToolResult { late: false, .. } => Class::ToolResult,
            Body::ToolResult { late: true, .. } => Class::LateResult,
            Body::Arrangement { .. } => Class::Arrangement,
            // In a tail a summary renders nothing: its range is still in
            // the prefix.
            Body::Summary { .. } if in_prefix => Class::Summary,
            Body::Recall { .. } if c.recall_id.as_deref() == Some(n.id.as_str()) => {
                Class::RecallSection
            }
            Body::Recall { .. } => Class::RecallNote,
            _ => continue,
        };
        let new = matches!(class, Class::RecallNote | Class::RecallSection) && pos > last_position;
        out.push(Piece {
            class,
            node_id: Some(n.id.clone()),
            new,
        });
    }
    out.extend(compiled.repairs.iter().map(|id| Piece {
        class: Class::Repair,
        node_id: Some(id.clone()),
        new: false,
    }));
    if task_view {
        out.push(Piece {
            class: Class::TaskView,
            node_id: None,
            new: false,
        });
    }
    out
}

/// A request that may not be sent: a piece its situation does not admit,
/// or a set that does not close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unadmitted {
    /// `not_admitted` or `unclosed`.
    pub why: &'static str,
    /// The piece, named: `this turn's recall_note rcn_…`, `the result of
    /// call toolu_…`.
    pub piece: String,
    /// What it means, in a sentence.
    pub detail: String,
}

/// The check after the compile: every piece admitted by `compiled`'s
/// situation, and the set closed. Pure: it reads what it is given.
pub fn check(
    compiled: &Compiled,
    nodes: &[(u64, Arc<Node>)],
    last_position: u64,
    task_view: bool,
) -> Result<(), Unadmitted> {
    let situation = &compiled.situation;
    if let Some(id) = compiled.compilation.recall_id.as_deref() {
        if !nodes.iter().any(|(_, n)| n.id == id) {
            return Err(Unadmitted {
                why: "unclosed",
                piece: format!("recall_section {id}"),
                detail: format!(
                    "the compilation renders recall section {id} first in its prefix, and no such \
                     node is in the session"
                ),
            });
        }
    }
    for p in pieces(compiled, nodes, last_position, task_view) {
        if !admits(situation, p.class, p.new) {
            return Err(Unadmitted {
                why: "not_admitted",
                piece: p.named(),
                detail: format!(
                    "a {} compile does not admit {}",
                    situation.kind(),
                    p.named()
                ),
            });
        }
    }
    closed(&compiled.request.messages)
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
    use Class::*;

    /// Each situation admits its classes (§2.11's table, from the code):
    /// one row per class, one column per situation, for a recall written
    /// before (`old`) and one this turn brought (`new`).
    #[test]
    fn each_situation_admits_its_classes() {
        let situations = [
            Situation::ConversationStart,
            Situation::TaskStart,
            Situation::Continuation,
            Situation::Recompile {
                trigger: "system_changed".into(),
            },
            Situation::Resume,
            Situation::Detour,
        ];
        // conversation start, task start, continuation, recompile, resume, detour
        let table: &[(Class, bool, [bool; 6])] = &[
            (Inbound, false, [true, true, true, true, true, true]),
            (Reply, false, [true, true, true, true, true, true]),
            (ToolResult, false, [true, true, true, true, true, true]),
            (LateResult, false, [true, true, true, true, true, true]),
            (Repair, false, [true, true, true, true, true, true]),
            (Arrangement, false, [false, true, true, true, true, false]),
            (Summary, false, [true, false, true, true, true, false]),
            (
                RecallSection,
                false,
                [false, false, true, true, true, false],
            ),
            (
                RecallSection,
                true,
                [false, true, false, true, false, false],
            ),
            (RecallNote, false, [true, false, true, true, true, false]),
            (RecallNote, true, [true, false, true, true, false, false]),
            (TaskView, false, [true, true, true, true, true, false]),
            (Lesson, false, [false; 6]),
        ];
        for (class, new, want) in table {
            for (s, w) in situations.iter().zip(want) {
                assert_eq!(
                    admits(s, *class, *new),
                    *w,
                    "{} admits {}{}",
                    s.kind(),
                    if *new { "a new " } else { "" },
                    class.as_str()
                );
            }
        }
    }

    /// The step's word, then the compile's: a new compilation of a session
    /// that had one is a recompile with its trigger, and so is a first
    /// compile the ring cut; a detour stays one.
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
}
