//! The bytes that put the chat on the terminal, inline and append-only.
//!
//! Finished lines are written once, as lines of the terminal's own scrollback
//! (the terminal scrolls as they come), so search, copy and scrolling work as
//! they always do, in tmux too. The dock below them is the only thing ever
//! redrawn: to draw, the cursor goes up to the dock's first row, the dock is
//! erased, the new lines are written, and the dock is drawn again under them.
//! Nothing above the dock is touched.
//!
//! A frame is wrapped in the terminal's synchronized-output mode (2026) so a
//! terminal that knows it shows the frame whole.

use super::dock::Dock;
use super::style::render;

/// What is on the terminal now.
#[derive(Debug, Default)]
pub struct Screen {
    /// The rows of the dock as last drawn.
    drawn: usize,
    /// The row of the dock the cursor was left on.
    at: usize,
}

/// A finished line that is this clears the screen and homes the cursor (a
/// rewind starts the conversation again on a clean screen, theseus-23rk). It
/// is written as it is, with no newline after it.
pub const CLEAR: &str = "\x1b[2J\x1b[H";

const BEGIN: &str = "\x1b[?2026h";
const END: &str = "\x1b[?2026l";

impl Screen {
    /// Up to the dock's first row and everything below erased.
    fn erase(&self, out: &mut String) {
        if self.drawn > 0 {
            if self.at > 0 {
                out.push_str(&format!("\x1b[{}A", self.at));
            }
            out.push('\r');
            out.push_str("\x1b[J");
        }
    }

    /// One frame: the finished lines `scroll`, then the dock. Rows are cut
    /// to `width` columns.
    pub fn frame(&mut self, scroll: &[String], dock: &Dock, width: usize, color: bool) -> Vec<u8> {
        let mut out = String::from(BEGIN);
        self.erase(&mut out);
        for l in scroll {
            if l == CLEAR {
                out.push_str(l);
                continue;
            }
            out.push_str(l);
            out.push_str("\x1b[K\r\n");
        }
        // After the lines the cursor is at a row's start with nothing below.
        self.draw(&mut out, dock, width, color);
        out.push_str(END);
        out.into_bytes()
    }

    fn draw(&mut self, out: &mut String, dock: &Dock, width: usize, color: bool) {
        let last = dock.rows.len().saturating_sub(1);
        for (i, row) in dock.rows.iter().enumerate() {
            out.push_str(&render(row, width, color));
            if i < last {
                out.push_str("\r\n");
            }
        }
        let up = last.saturating_sub(dock.cursor.0);
        if up > 0 {
            out.push_str(&format!("\x1b[{up}A"));
        }
        out.push('\r');
        if dock.cursor.1 > 0 {
            out.push_str(&format!(
                "\x1b[{}C",
                dock.cursor.1.min(width.saturating_sub(1))
            ));
        }
        self.drawn = dock.rows.len();
        self.at = dock.cursor.0.min(last);
    }

    /// Take the dock off the terminal, leaving the cursor where it began.
    pub fn clear(&mut self) -> Vec<u8> {
        let mut out = String::from(BEGIN);
        self.erase(&mut out);
        out.push_str(END);
        self.drawn = 0;
        self.at = 0;
        out.into_bytes()
    }
}
