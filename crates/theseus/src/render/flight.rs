//! A session's calls in flight, for the `←` line (theseus-d1hi): the programs
//! and writes of one response run together and end in any order, so when
//! another call of the same tool is still running, a call's `←` line names
//! its subject (its argv's head, or its path), and three `proc.run`s can be
//! told apart. One call alone keeps its line as it was.

use std::collections::HashMap;

use serde_json::Value;
use theseus_protocol::{Event, ToolProposed};

use super::{clip, event, Line, Show, Tag};

/// How much of a subject the `←` line shows.
const SUBJECT_CHARS: usize = 40;

/// The calls a client has seen proposed and not yet seen end: each one's
/// tool and subject, by its `tool_use_id`.
#[derive(Debug, Default)]
pub struct Flight {
    calls: HashMap<String, (String, String)>,
}

impl Flight {
    /// `render::event`'s lines for `e`, the `←` line with its call's subject
    /// when another call of its tool is in flight.
    pub fn event(&mut self, e: &Event, show: Show) -> Vec<Line> {
        let mut lines = event(e, show);
        match e {
            Event::ToolProposed(p) => {
                self.calls
                    .insert(p.tool_use_id.clone(), (p.tool.clone(), subject(p)));
            }
            Event::ToolEnded(t) => {
                let twin = self
                    .calls
                    .iter()
                    .any(|(id, (tool, _))| *id != t.tool_use_id && *tool == t.tool);
                // A job answered in the background is still running: its late
                // result ends it.
                let mine = match t.status.as_str() {
                    "background" => self.calls.get(&t.tool_use_id).cloned(),
                    _ => self.calls.remove(&t.tool_use_id),
                };
                let head = format!("  ← {} ", t.tool);
                if let (true, Some((_, s))) = (twin, mine.filter(|(_, s)| !s.is_empty())) {
                    for l in lines.iter_mut().filter(|l| l.tag == Tag::Tool) {
                        if let Some(rest) = l.text.strip_prefix(&head) {
                            l.text = format!("{head}[{s}] {rest}");
                        }
                    }
                }
            }
            _ => {}
        }
        lines
    }
}

/// A call's subject: its program's argv (a batch's first step), else the
/// path, terminal, pattern, or URL its input names.
fn subject(p: &ToolProposed) -> String {
    let plan = p.gate.plan.as_ref();
    let argv = plan.and_then(|pl| {
        pl.argv
            .clone()
            .or_else(|| pl.steps.as_ref().and_then(|s| s.first().cloned()))
    });
    if let Some(a) = argv {
        return clip(&a.join(" "), SUBJECT_CHARS);
    }
    ["path", "terminal", "pattern", "url"]
        .iter()
        .find_map(|k| p.input.get(*k).and_then(Value::as_str))
        .map(|s| clip(s, SUBJECT_CHARS))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use theseus_protocol::{Plan, ToolEnded};

    fn proposed(id: &str, tool: &str, argv: Option<&[&str]>, input: Value) -> Event {
        let mut p = ToolProposed {
            tool_use_id: id.into(),
            tool: tool.into(),
            input,
            ..ToolProposed::default()
        };
        p.gate.plan = argv.map(|a| Plan {
            argv: Some(a.iter().map(|s| s.to_string()).collect()),
            ..Plan::default()
        });
        Event::ToolProposed(p)
    }

    fn ended(id: &str, tool: &str, status: &str) -> Event {
        Event::ToolEnded(ToolEnded {
            tool_use_id: id.into(),
            tool: tool.into(),
            status: status.into(),
            duration_ms: Some(2001),
            exit_code: Some(0),
            ..ToolEnded::default()
        })
    }

    fn tool_lines(f: &mut Flight, e: &Event) -> Vec<String> {
        f.event(e, Show::default())
            .into_iter()
            .filter(|l| l.tag == Tag::Tool)
            .map(|l| l.text)
            .collect()
    }

    #[test]
    fn a_calls_end_names_its_subject_only_beside_another_of_its_tool() {
        let mut f = Flight::default();
        for (id, secs) in [("a", "1"), ("b", "2")] {
            tool_lines(
                &mut f,
                &proposed(id, "proc.run", Some(&["sleep", secs]), json!({})),
            );
        }
        tool_lines(
            &mut f,
            &proposed("w", "fs.write", None, json!({"path": "a.txt"})),
        );
        assert_eq!(
            tool_lines(&mut f, &ended("b", "proc.run", "ok")),
            ["  ← proc.run [sleep 2] ok · exit 0 · 2001 ms · 0 B"]
        );
        // The last of its tool, and the only write: their lines as before.
        assert_eq!(
            tool_lines(&mut f, &ended("a", "proc.run", "ok")),
            ["  ← proc.run ok · exit 0 · 2001 ms · 0 B"]
        );
        assert_eq!(
            tool_lines(&mut f, &ended("w", "fs.write", "ok")),
            ["  ← fs.write ok · exit 0 · 2001 ms · 0 B"]
        );
    }

    #[test]
    fn a_job_answered_in_the_background_is_still_in_flight() {
        let mut f = Flight::default();
        for id in ["a", "b"] {
            tool_lines(
                &mut f,
                &proposed(id, "fs.write", None, json!({"path": format!("{id}.txt")})),
            );
        }
        tool_lines(&mut f, &ended("a", "fs.write", "background"));
        assert_eq!(
            tool_lines(&mut f, &ended("b", "fs.write", "ok")),
            ["  ← fs.write [b.txt] ok · exit 0 · 2001 ms · 0 B"]
        );
    }
}
