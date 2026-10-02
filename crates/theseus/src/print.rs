//! The CLI's printer (theseus-7yx, step 10a): it writes the library's lines.
//! A command's lines go out one to a line, whatever their tag. A session's
//! events (`Printer`) put the reply on stdout as it comes and every other line
//! on stderr, with a run of thinking under its label.

use std::io::{self, Stderr, Stdout, Write};

use serde_json::Value;
use theseus_client::render::{self, Line, Show, Tag};
use theseus_protocol::Event;

/// What a `Printer` shows of a session's events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The reply streamed on stdout as it comes, and each event on stderr:
    /// `ask`, and the turn `confirm` follows.
    Text,
    /// Each event on stderr, and none of the reply, which the command prints
    /// once at the end: `ask --no-stream`.
    Quiet,
    /// `Text`, plus each turn's start and end and every context decision:
    /// `watch`.
    Watch,
    /// Nothing: the command prints its result as JSON (`--json`).
    Json,
}

/// Each line's text, one to a line. Outside a session's events, the CLI
/// prints every tag alike.
pub fn lines(w: &mut impl Write, lines: &[Line]) -> io::Result<()> {
    for l in lines {
        writeln!(w, "{}", l.text)?;
    }
    Ok(())
}

/// Prints a session's live events for a terminal: the model's text on `out`
/// (stdout), everything else (tools, confirmations, context decisions) on
/// `err` (stderr).
pub struct Printer<O: Write = Stdout, E: Write = Stderr> {
    /// What the events show; nothing under `--json`.
    show: Option<Show>,
    out: O,
    err: E,
    stdout_mid_line: bool,
    thinking_open: bool,
}

impl Printer {
    /// A printer to the terminal.
    pub fn new(mode: Mode, thinking: bool) -> Self {
        Self::to(mode, thinking, io::stdout(), io::stderr())
    }
}

impl<O: Write, E: Write> Printer<O, E> {
    /// `thinking`: the model's thinking summaries too, on `err`, as they
    /// stream.
    pub fn to(mode: Mode, thinking: bool, out: O, err: E) -> Self {
        let show = |reply, turns| {
            Some(Show {
                reply,
                thinking,
                turns,
            })
        };
        Self {
            show: match mode {
                Mode::Text => show(true, false),
                Mode::Quiet => show(false, false),
                Mode::Watch => show(true, true),
                Mode::Json => None,
            },
            out,
            err,
            stdout_mid_line: false,
            thinking_open: false,
        }
    }

    /// What it wrote to `out` and to `err`.
    #[cfg(test)]
    pub fn into_parts(self) -> (O, E) {
        (self.out, self.err)
    }

    /// Finish a partial stdout line or thinking run before an event line.
    pub fn settle(&mut self) {
        if self.thinking_open {
            let _ = writeln!(self.err);
            self.thinking_open = false;
        }
        if self.stdout_mid_line {
            let _ = writeln!(self.out);
            let _ = self.out.flush();
            self.stdout_mid_line = false;
        }
    }

    /// One notification, as the connection hands it over.
    pub fn on(&mut self, m: &str, p: &Value) {
        if let Ok(Some(e)) = Event::from_notification(m, p) {
            self.on_event(&e);
        }
    }

    pub fn on_event(&mut self, e: &Event) {
        let Some(show) = self.show else {
            return;
        };
        for line in render::event(e, show) {
            self.print(&line);
        }
    }

    /// One line of a session's events. The reply's text goes on stdout as it
    /// comes, and the thinking's on stderr under its label, each a piece of
    /// its stream; every other line goes on stderr, whole.
    fn print(&mut self, line: &Line) {
        match line.tag {
            Tag::Reply => {
                if self.thinking_open {
                    let _ = writeln!(self.err);
                    self.thinking_open = false;
                }
                let _ = self.out.write_all(line.text.as_bytes());
                let _ = self.out.flush();
                if !line.text.is_empty() {
                    self.stdout_mid_line = !line.text.ends_with('\n');
                }
            }
            Tag::Thinking => {
                if !self.thinking_open {
                    self.settle();
                    let _ = write!(self.err, "  (thinking) ");
                    self.thinking_open = true;
                }
                let _ = write!(self.err, "{}", line.text.replace('\n', "\n             "));
            }
            _ => {
                self.settle();
                let _ = writeln!(self.err, "{}", line.text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::notify;

    /// A printer's mode (theseus-0g4): `Text` streams the reply and ends its
    /// partial line before an event line; `Quiet` prints the events and none
    /// of the reply; `Watch` adds each turn's start; `Json` prints nothing.
    #[test]
    fn each_mode_prints_what_it_says() {
        let delta =
            |text: &str| serde_json::json!({"turn_id": "turn_a", "loop_index": 0, "text": text});
        let events = [
            (
                notify::TURN_STARTED,
                serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a", "continuation": true}),
            ),
            (notify::MODEL_THINKING, delta("hm")),
            (notify::MODEL_DELTA, delta("Half a line")),
            (
                notify::TOOL_STARTED,
                serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a", "tool": "fs.read",
                    "tool_use_id": "tu_1", "correlation_id": "act_1", "backend": "inproc"}),
            ),
            (notify::MODEL_DELTA, delta(" and the rest\n")),
        ];
        let run = |mode, thinking| {
            let mut p = Printer::to(mode, thinking, Vec::new(), Vec::new());
            for (m, v) in &events {
                p.on(m, v);
            }
            p.settle();
            let (out, err) = p.into_parts();
            (
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        let reply = "Half a line\n and the rest\n".to_string();
        assert_eq!(
            run(Mode::Text, true),
            (reply.clone(), "  (thinking) hm\n  → fs.read\n".to_string())
        );
        assert_eq!(
            run(Mode::Quiet, true),
            (String::new(), "  (thinking) hm\n  → fs.read\n".to_string())
        );
        assert_eq!(
            run(Mode::Watch, false),
            (
                reply,
                "── turn turn_a (continuation)\n  → fs.read\n".to_string()
            )
        );
        assert_eq!(run(Mode::Json, true), (String::new(), String::new()));
    }
}
