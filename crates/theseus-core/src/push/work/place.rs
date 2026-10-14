//! The place rule for the work board (theseus-hkx30). A connection that
//! reads no private text (Discord's binding, the MCP server, a connection no
//! listener named) asks `work.watch` for roots of a shared place, and hears
//! about those roots alone. What it hears passes the session's scrubber: a
//! title, a job's argv, a question's words and a task's report are text a
//! person or a tool wrote. It may not answer a question from here
//! (`may_answer` false): the answer comes from the operator's private place.
//!
//! The board itself is one: a view is built once and its line is shared by
//! every connection that sees the whole board. A scoped connection gets its
//! own copy, cut to its roots and scrubbed.

use std::collections::HashSet;

use serde_json::Value;
use theseus_protocol::notices::Notice;
use theseus_protocol::work::WorkView;

use crate::scrub::Scrubber;

/// What one `work.watch` connection sees.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// The roots (execution ids) it may see; None for the whole board.
    pub roots: Option<HashSet<String>>,
    /// Whether what it reads passes the scrubber and may not answer: set for
    /// a surface that reads no private text.
    pub guarded: bool,
}

impl Scope {
    /// Whether this scope sees everything the board holds, as it holds it.
    pub fn is_whole(&self) -> bool {
        self.roots.is_none() && !self.guarded
    }

    /// Whether a view under `root` is one it sees.
    pub fn sees(&self, root: &str) -> bool {
        self.roots.as_ref().is_none_or(|r| r.contains(root))
    }

    /// These views as this scope sees them: the ones under its roots, scrubbed
    /// when guarded.
    pub fn cut(&self, views: &[WorkView], scrubber: &Scrubber) -> Vec<WorkView> {
        views
            .iter()
            .filter(|v| self.sees(&v.root))
            .map(|v| self.show(v, scrubber))
            .collect()
    }

    /// One view as this scope sees it.
    pub fn show(&self, v: &WorkView, scrubber: &Scrubber) -> WorkView {
        if !self.guarded {
            return v.clone();
        }
        let mut v = scrubbed(v, scrubber);
        // A job's last output line is for private places alone (theseus-e1hu4).
        if let Some(d) = v.doing.as_mut() {
            d.last_line = None;
        }
        if let Some(q) = v.question.as_mut() {
            q.may_answer = false;
        }
        v
    }

    /// A notice as this scope hears it: the scrubbed one, when guarded.
    pub fn notice(&self, n: &Notice, scrubber: &Scrubber) -> Notice {
        if !self.guarded {
            return n.clone();
        }
        let value = scrub_value(serde_json::to_value(n).unwrap_or(Value::Null), scrubber);
        serde_json::from_value(value).unwrap_or_else(|_| Notice {
            line: String::new(),
            ..n.clone()
        })
    }

    /// The ids among `gone` (id, root) that are under its roots.
    pub fn gone(&self, gone: &[(String, String)]) -> Vec<String> {
        gone.iter()
            .filter(|(_, root)| self.sees(root))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// `v` with every string passed through the scrubber, keys kept.
fn scrubbed(v: &WorkView, scrubber: &Scrubber) -> WorkView {
    let value = serde_json::to_value(v).unwrap_or(Value::Null);
    let value = scrub_value(value, scrubber);
    serde_json::from_value(value).unwrap_or_else(|_| blank(v))
}

/// What a guarded scope is given of a view it cannot scrub: its identity and
/// its place in the tree, none of its words.
fn blank(v: &WorkView) -> WorkView {
    WorkView {
        title: String::new(),
        question: None,
        doing: None,
        progress: None,
        report: None,
        woke: None,
        attention: theseus_protocol::Attention {
            label: String::new(),
            ..v.attention.clone()
        },
        ..v.clone()
    }
}

fn scrub_value(v: Value, scrubber: &Scrubber) -> Value {
    match v {
        Value::String(s) => Value::String(scrubber.scrub(&s).0),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| scrub_value(x, scrubber)).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| (k, scrub_value(x, scrubber)))
                .collect(),
        ),
        other => other,
    }
}
