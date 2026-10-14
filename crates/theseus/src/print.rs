//! The CLI's printer (theseus-7yx, step 10a): it writes the library's lines.
//! A command's lines go out one to a line, whatever their tag. A session's
//! events (`Printer`) put the reply on stdout as it comes and every other line
//! on stderr, with a run of thinking under its label.

use std::io::{self, Stderr, Stdout, Write};

use serde_json::Value;
use theseus_client::render::md::Markdown;
use theseus_client::render::style::{self, Mode as Look, StyledLine};
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

/// A whole reply on `w`: drawn as markdown when stdout shows styles, else the
/// text and a newline, as it always was (`ask --no-stream`).
pub fn reply(w: &mut impl Write, text: &str) -> io::Result<()> {
    if !style::styled_stdout() {
        return writeln!(w, "{text}");
    }
    let width = style::prose_width(style::terminal_width());
    for l in Markdown::render(text, width, Look::Ansi) {
        writeln!(w, "{}", l.ansi())?;
    }
    Ok(())
}

/// A line under a reply (the turn's status line) on stderr: on a terminal,
/// broken at its spaces to the terminal's width, so no word is cut where the
/// terminal's own wrap would; anywhere else, as it is (theseus-rn7s).
pub fn footer(text: &str) {
    use std::io::IsTerminal;
    if !style::is_plain() && io::stderr().is_terminal() && style::color_allowed() {
        let width = style::prose_width(style::terminal_width());
        for l in render::md::wrap_text(text, width) {
            eprintln!("{l}");
        }
    } else {
        eprintln!("{text}");
    }
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
    /// The calls in flight, so a `←` line can name its call (theseus-d1hi).
    flight: render::Flight,
    /// The reply's handles as numbered marks, once `mark_cites` turns it on
    /// (theseus-ejdk).
    marker: Option<render::cite::Marker>,
    /// The reply drawn as markdown, on a terminal (theseus-rn7s): it goes out
    /// by complete lines, styled. None: the text as it came.
    md: Option<Markdown>,
    /// How many lines `print` was given, for a caller that asks whether an
    /// event showed anything.
    printed: u64,
}

impl Printer {
    /// A printer to the terminal: a reply is drawn as markdown (`ask`, a turn
    /// `confirm` follows, `watch`) when stdout shows styles, and is the text as
    /// it came otherwise (a pipe, `NO_COLOR`, `--plain`).
    pub fn new(mode: Mode, thinking: bool) -> Self {
        let mut p = Self::to(mode, thinking, io::stdout(), io::stderr());
        if matches!(mode, Mode::Text | Mode::Watch) && style::styled_stdout() {
            p.md = Some(Markdown::new(
                style::prose_width(style::terminal_width()),
                Look::Ansi,
            ));
        }
        p
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
            flight: render::Flight::default(),
            marker: None,
            md: None,
            printed: 0,
        }
    }

    /// The lines shown so far (a count only a caller compares with itself).
    pub fn lines_printed(&self) -> u64 {
        self.printed
    }

    /// Draw the reply as markdown at `width`, in ANSI (a test's choice; `new`
    /// decides for a terminal).
    #[cfg(test)]
    pub fn markdown(&mut self, width: usize) {
        self.md = Some(Markdown::new(width, Look::Ansi));
    }

    /// Write rendered lines, each complete.
    fn write_styled(&mut self, lines: &[StyledLine]) {
        for l in lines {
            let _ = writeln!(self.out, "{}", l.ansi());
        }
        let _ = self.out.flush();
    }

    /// Show the reply's handles (`[m:a1b2c3]`) as numbered marks as the text
    /// streams, for `footnotes` to explain once the turn ends (theseus-ejdk).
    pub fn mark_cites(&mut self) {
        self.marker = Some(render::cite::Marker::default());
    }

    /// The footnote block under a reply: each mark's source, with the
    /// command that opens it. Nothing when no handle streamed past.
    pub fn footnotes(&mut self, cites: &[theseus_protocol::cite::Citation]) {
        self.settle();
        let Some(m) = self.marker.as_ref().filter(|m| !m.is_empty()) else {
            return;
        };
        for l in m.notes(cites) {
            let _ = writeln!(self.out, "{}", l.text);
        }
        let _ = self.out.flush();
    }

    /// What it wrote to `out` and to `err`.
    #[cfg(test)]
    pub fn into_parts(self) -> (O, E) {
        (self.out, self.err)
    }

    /// Finish a partial stdout line or thinking run before an event line.
    pub fn settle(&mut self) {
        if let Some(rest) = self.marker.as_mut().map(render::cite::Marker::finish) {
            if !rest.is_empty() {
                self.reply_text(&rest);
            }
        }
        if let Some(md) = self.md.as_mut() {
            let lines = md.finish();
            self.write_styled(&lines);
        }
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
        for line in self.flight.event(e, show) {
            self.print(&line);
        }
    }

    /// A piece of the reply on stdout: drawn when it makes complete lines of
    /// markdown, else as it came.
    fn reply_text(&mut self, shown: &str) {
        if let Some(md) = self.md.as_mut() {
            let lines = md.push(shown);
            self.write_styled(&lines);
            return;
        }
        let _ = self.out.write_all(shown.as_bytes());
        let _ = self.out.flush();
        if !shown.is_empty() {
            self.stdout_mid_line = !shown.ends_with('\n');
        }
    }

    /// One line of a session's events. The reply's text goes on stdout as it
    /// comes, and the thinking's on stderr under its label, each a piece of
    /// its stream; every other line goes on stderr, whole.
    fn print(&mut self, line: &Line) {
        self.printed += 1;
        match line.tag {
            Tag::Reply => {
                if self.thinking_open {
                    let _ = writeln!(self.err);
                    self.thinking_open = false;
                }
                let shown = match self.marker.as_mut() {
                    Some(m) => m.push(&line.text),
                    None => line.text.clone(),
                };
                self.reply_text(&shown);
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

    /// A reply drawn as markdown (theseus-rn7s): lines go out whole as they
    /// complete, a paragraph when its blank line comes, and what is still open
    /// is drawn before an event line, so the reply never lands below a tool
    /// line that came after it.
    #[test]
    fn a_reply_is_drawn_by_whole_lines_and_ends_before_an_event_line() {
        let delta =
            |text: &str| serde_json::json!({"turn_id": "turn_a", "loop_index": 0, "text": text});
        let mut p = Printer::to(Mode::Text, false, Vec::new(), Vec::new());
        p.markdown(20);
        p.on(notify::MODEL_DELTA, &delta("# Tit"));
        p.on(
            notify::MODEL_DELTA,
            &delta("le\n\nSome **bold** words that wrap"),
        );
        p.on(notify::MODEL_DELTA, &delta(" here"));
        p.on(
            notify::TOOL_STARTED,
            &serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a", "tool": "fs.read",
                "tool_use_id": "tu_1", "correlation_id": "act_1", "backend": "inproc"}),
        );
        p.on(notify::MODEL_DELTA, &delta("After the tool.\n"));
        p.settle();
        let (out, err) = p.into_parts();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\x1b[1;4mTitle\x1b[0m\n\nSome \x1b[1mbold\x1b[0m words that\nwrap here\nAfter the tool.\n"
        );
        assert_eq!(String::from_utf8(err).unwrap(), "  → fs.read\n");
    }
}
