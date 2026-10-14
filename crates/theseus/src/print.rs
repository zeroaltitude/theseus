//! The CLI's printer (theseus-7yx, step 10a): it writes the library's lines.
//! A command's lines go out one to a line, whatever their tag. A session's
//! events (`Printer`) put the reply on stdout as it comes and every other line
//! on stderr, with a run of thinking under its label.

use std::io::{self, IsTerminal, Stderr, Stdout, Write};

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
        writeln!(w, "{}", render::diffview::plain(&l.text))?;
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
    /// A diff's rows are painted (theseus-z6au): only on a terminal that
    /// takes colour.
    colour: bool,
}

/// Colour on `err`: a terminal, `NO_COLOR` unset or empty, `TERM` not `dumb`,
/// and no `--plain` (term-render's flag: the text as it always was).
fn colour_on(err: &impl IsTerminal) -> bool {
    let env = |k: &str| std::env::var(k).ok();
    !style::is_plain() && colour_for(err.is_terminal(), env("NO_COLOR"), env("TERM"))
}

fn colour_for(tty: bool, no_color: Option<String>, term: Option<String>) -> bool {
    tty && no_color.is_none_or(|v| v.is_empty()) && term.is_none_or(|t| t != "dumb")
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
        p.colour = colour_on(&io::stderr());
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
            colour: false,
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
        // The router's question: said, and on a terminal answered here.
        if m == theseus_protocol::notify::THREAD_ASKED {
            crate::thread::ask::on_asked(p);
            return;
        }
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
                let _ = writeln!(self.err, "{}", render::diffview::paint(line, self.colour));
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

    fn edit_events() -> Vec<Event> {
        use serde_json::json;
        use theseus_protocol::{ToolDiff, ToolEnded, ToolProposed, ToolStarted};
        let unified = "--- tides.py\n+++ tides.py\n@@ -48,3 +48,3 @@\n     highest = max(h)\n-    return lowest - highest\n+    return highest - lowest\n \n";
        vec![
            Event::ToolProposed(ToolProposed {
                tool_use_id: "t1".into(),
                tool: "fs.edit".into(),
                input: json!({"path": "tides.py", "old_string": "lowest - highest", "new_string": "highest - lowest"}),
                ..ToolProposed::default()
            }),
            Event::ToolStarted(ToolStarted {
                tool_use_id: "t1".into(),
                tool: "fs.edit".into(),
                ..ToolStarted::default()
            }),
            Event::ToolEnded(ToolEnded {
                tool_use_id: "t1".into(),
                tool: "fs.edit".into(),
                status: "ok".into(),
                diff: Some(ToolDiff {
                    path: "tides.py".into(),
                    added: 1,
                    removed: 1,
                    unified: unified.into(),
                    truncated: false,
                }),
                ..ToolEnded::default()
            }),
        ]
    }

    /// theseus-z6au: a tool line says what the call does, not only its tool;
    /// the finished edit shows its diff; and on a pipe none of it carries an
    /// escape or a mark.
    #[test]
    fn a_tool_line_is_words_and_a_finished_edit_shows_its_diff() {
        let mut p = Printer::to(Mode::Watch, false, Vec::new(), Vec::new());
        for e in edit_events() {
            p.on_event(&e);
        }
        let (_, err) = p.into_parts();
        let err = String::from_utf8(err).unwrap();
        assert!(err.starts_with("  → Edit tides.py\n"), "{err}");
        assert!(
            !err.contains("→ fs.edit"),
            "the start names it again: {err}"
        );
        assert!(err.contains("  ⎿ tides.py  +1 −1\n"), "{err}");
        assert!(
            err.contains("    49 -     return lowest - highest\n"),
            "{err}"
        );
        assert!(
            err.contains("    49 +     return highest - lowest\n"),
            "{err}"
        );
        assert!(
            !err.contains('\x1b') && !err.contains(MARKS),
            "no escape on a pipe: {err:?}"
        );
    }

    const MARKS: [char; 2] = ['\u{1}', '\u{2}'];

    #[test]
    fn a_terminal_paints_the_diff_and_marks_the_changed_words() {
        let mut p = Printer::to(Mode::Watch, false, Vec::new(), Vec::new());
        p.colour = true;
        for e in edit_events() {
            p.on_event(&e);
        }
        let (_, err) = p.into_parts();
        let err = String::from_utf8(err).unwrap();
        let del = err.lines().find(|l| l.contains("49 -")).unwrap();
        let add = err.lines().find(|l| l.contains("49 +")).unwrap();
        assert!(
            del.starts_with("\x1b[31m") && del.contains("\x1b[1;97;41mlowest\x1b[0m"),
            "{del:?}"
        );
        assert!(
            add.starts_with("\x1b[32m") && add.contains("\x1b[1;97;42mhighest\x1b[0m"),
            "{add:?}"
        );
    }

    /// Colour only on a terminal, and never under `NO_COLOR` or `TERM=dumb`.
    #[test]
    fn colour_is_for_a_terminal_that_takes_it() {
        let s = |v: &str| Some(v.to_string());
        assert!(colour_for(true, None, s("xterm-256color")));
        assert!(colour_for(true, s(""), None), "an empty NO_COLOR is unset");
        assert!(!colour_for(false, None, s("xterm")), "a pipe");
        assert!(!colour_for(true, s("1"), s("xterm")), "NO_COLOR");
        assert!(!colour_for(true, None, s("dumb")), "TERM=dumb");
    }
}
