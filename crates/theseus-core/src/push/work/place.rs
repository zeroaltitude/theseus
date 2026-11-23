//! Explicit root filtering and the work board's act rule. Every surface
//! receives the same information under its requested roots. Guarded copies
//! are scrubbed and cannot answer a question (`may_answer` stays false):
//! answering still requires the operator's authorized place.

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
    /// Whether this copy passes the scrubber and cannot authorize answers.
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
        routine: None,
        watch: None,
        project: None,
        forum_url: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::work::views::blank as view;
    use theseus_protocol::work::WorkKind;

    /// Forum links are information, not authority to answer.
    #[test]
    fn every_surface_is_given_the_forum_address() {
        let mut v = view("exe_reef", WorkKind::Task, 5);
        v.forum_url = Some("https://discord.com/channels/4242/777".into());
        let scrubber = Scrubber::default();
        let guarded = Scope {
            roots: None,
            guarded: true,
        };
        assert_eq!(guarded.show(&v, &scrubber).forum_url, v.forum_url);
        assert_eq!(
            Scope::default().show(&v, &scrubber).forum_url,
            v.forum_url,
            "a scope that reads private text is given it"
        );
        // The blanked form a view that cannot be scrubbed falls back to too.
        assert_eq!(blank(&v).forum_url, None);
    }
}
