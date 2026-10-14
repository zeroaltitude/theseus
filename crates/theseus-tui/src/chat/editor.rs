//! The input line: a multi-line buffer with a cursor, a history of this
//! directory's messages, and pasted text folded into one chip.
//!
//! A paste of more than [`FOLD_LINES`] lines goes into the buffer as one
//! private-use character (`CHIP_BASE + n`), which the buffer edits like any
//! other (one backspace removes it), the dock draws as
//! `[pasted 60 lines · 2.4 KB · ctrl+o shows it]`, and [`Editor::take`] writes
//! back whole, so the message goes as one `turn.submit` with the paste in
//! place. A paste is never keys (theseus-8hcg): it comes through
//! [`Editor::paste`], not [`Editor::insert`].

use unicode_width::UnicodeWidthChar;

/// A paste of more lines than this folds.
pub const FOLD_LINES: usize = 5;
/// The first private-use character a chip is written as.
const CHIP_BASE: u32 = 0xE000;
/// Messages kept in history for a directory.
pub const HISTORY_KEPT: usize = 200;
/// The widest the prompt's prefix is: `› `.
pub const PREFIX: usize = 2;

/// A message taken from the line.
pub struct Taken {
    /// As it is sent, every paste in place.
    pub text: String,
    /// As the transcript shows it, every paste a chip.
    pub shown: String,
}

#[derive(Debug, Default)]
pub struct Editor {
    buf: Vec<char>,
    cur: usize,
    pastes: Vec<String>,
    hist: Vec<String>,
    at: Option<usize>,
    draft: Vec<char>,
}

/// One row of the input as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub text: String,
    /// The buffer index of its first character.
    start: usize,
    /// Display columns before each character, for the cursor's moves.
    cols: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub rows: Vec<Row>,
    /// The cursor's row and column (after the prefix).
    pub cursor: (usize, usize),
}

impl Editor {
    pub fn with_history(hist: Vec<String>) -> Self {
        Self {
            hist,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn history(&self) -> &[String] {
        &self.hist
    }

    /// One typed character. A control character is not text.
    pub fn insert(&mut self, c: char) {
        if c.is_control() && c != '\n' {
            return;
        }
        self.leave_history();
        self.buf.insert(self.cur, c);
        self.cur += 1;
    }

    pub fn newline(&mut self) {
        self.insert('\n');
    }

    /// Typed or pasted short text, inserted as it is.
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert(c);
        }
    }

    /// A paste: folded into a chip when it is longer than [`FOLD_LINES`]
    /// lines, else inserted as text. Line ends are `\n`.
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.lines().count() > FOLD_LINES {
            self.leave_history();
            let id = self.pastes.len() as u32;
            self.pastes.push(text);
            if let Some(c) = char::from_u32(CHIP_BASE + id) {
                self.buf.insert(self.cur, c);
                self.cur += 1;
            }
        } else {
            self.insert_str(&text);
        }
    }

    pub fn backspace(&mut self) {
        if self.cur > 0 {
            self.leave_history();
            self.cur -= 1;
            self.buf.remove(self.cur);
        }
    }

    pub fn delete(&mut self) {
        if self.cur < self.buf.len() {
            self.leave_history();
            self.buf.remove(self.cur);
        }
    }

    /// `\` before the cursor, then Enter: the backslash goes and a newline
    /// takes its place. False when there is no backslash to take.
    pub fn continue_line(&mut self) -> bool {
        if self.cur > 0 && self.buf[self.cur - 1] == '\\' {
            self.buf.remove(self.cur - 1);
            self.cur -= 1;
            self.newline();
            true
        } else {
            false
        }
    }

    pub fn left(&mut self) {
        self.cur = self.cur.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cur = (self.cur + 1).min(self.buf.len());
    }

    pub fn home(&mut self) {
        while self.cur > 0 && self.buf[self.cur - 1] != '\n' {
            self.cur -= 1;
        }
    }

    pub fn end(&mut self) {
        while self.cur < self.buf.len() && self.buf[self.cur] != '\n' {
            self.cur += 1;
        }
    }

    pub fn word_left(&mut self) {
        while self.cur > 0 && self.buf[self.cur - 1].is_whitespace() {
            self.cur -= 1;
        }
        while self.cur > 0 && !self.buf[self.cur - 1].is_whitespace() {
            self.cur -= 1;
        }
    }

    pub fn word_right(&mut self) {
        while self.cur < self.buf.len() && self.buf[self.cur].is_whitespace() {
            self.cur += 1;
        }
        while self.cur < self.buf.len() && !self.buf[self.cur].is_whitespace() {
            self.cur += 1;
        }
    }

    /// Ctrl-U: the line's start to the cursor.
    pub fn kill_to_start(&mut self) {
        let end = self.cur;
        self.home();
        self.leave_history();
        self.buf.drain(self.cur..end);
    }

    /// Ctrl-K: the cursor to the line's end.
    pub fn kill_to_end(&mut self) {
        let from = self.cur;
        self.end();
        self.leave_history();
        self.buf.drain(from..self.cur);
        self.cur = from;
    }

    /// Ctrl-W: the word before the cursor.
    pub fn kill_word(&mut self) {
        let end = self.cur;
        self.word_left();
        self.leave_history();
        self.buf.drain(self.cur..end);
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.cur = 0;
        self.at = None;
    }

    /// The text as it will be sent, chips written back whole.
    pub fn text(&self) -> String {
        self.buf.iter().map(|&c| self.expand(c)).collect()
    }

    fn expand(&self, c: char) -> String {
        match self.chip(c) {
            Some(i) => self.pastes[i].clone(),
            None => c.to_string(),
        }
    }

    fn chip(&self, c: char) -> Option<usize> {
        let n = (c as u32).checked_sub(CHIP_BASE)? as usize;
        (n < self.pastes.len()).then_some(n)
    }

    /// The message: its text whole, with the buffer emptied and the message
    /// remembered in history (unless it is a repeat of the last). `shown` is
    /// the same with each paste folded to its chip, for the transcript.
    pub fn take(&mut self) -> Taken {
        let text = self.text();
        let mut shown = String::new();
        for (at, &c) in self.buf.iter().enumerate() {
            match self.chip(c) {
                Some(i) => {
                    shown.push_str(&self.chip_summary(i));
                    // A chip stands apart from the words after it.
                    if self.buf.get(at + 1).is_some_and(|n| !n.is_whitespace()) {
                        shown.push(' ');
                    }
                }
                None => shown.push(c),
            }
        }
        self.clear();
        self.pastes.clear();
        let t = text.trim().to_string();
        if !t.is_empty() && self.hist.last() != Some(&t) && t.len() <= 4096 {
            self.hist.push(t);
            if self.hist.len() > HISTORY_KEPT {
                self.hist.remove(0);
            }
        }
        Taken { text, shown }
    }

    /// The paste under the cursor or just before it, whole (Ctrl-O).
    pub fn paste_at_cursor(&self) -> Option<&str> {
        let at = |i: usize| self.buf.get(i).and_then(|&c| self.chip(c));
        let found = at(self.cur)
            .or_else(|| self.cur.checked_sub(1).and_then(at))
            .or_else(|| self.buf.iter().rev().find_map(|&c| self.chip(c)));
        found.map(|i| self.pastes[i].as_str())
    }

    fn chip_summary(&self, i: usize) -> String {
        let p = &self.pastes[i];
        let lines = p.lines().count();
        let kb = p.len() as f64 / 1024.0;
        format!("[pasted {lines} lines · {kb:.1} KB]")
    }

    fn chip_words(&self, i: usize) -> String {
        let summary = self.chip_summary(i);
        format!("{} · ctrl+o shows it]", summary.trim_end_matches(']'))
    }

    fn leave_history(&mut self) {
        self.at = None;
    }

    // History: up from the first row recalls older messages, down from the
    // last row comes back to the draft.

    pub fn history_prev(&mut self) {
        if self.hist.is_empty() {
            return;
        }
        let i = match self.at {
            None => {
                self.draft = self.buf.clone();
                self.hist.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.at = Some(i);
        self.buf = self.hist[i].chars().collect();
        self.cur = self.buf.len();
    }

    pub fn history_next(&mut self) {
        let Some(i) = self.at else { return };
        if i + 1 < self.hist.len() {
            self.at = Some(i + 1);
            self.buf = self.hist[i + 1].chars().collect();
        } else {
            self.at = None;
            self.buf = std::mem::take(&mut self.draft);
        }
        self.cur = self.buf.len();
    }

    /// The buffer's rows for a terminal `width` columns wide, the prompt's
    /// two columns taken off.
    pub fn layout(&self, width: usize) -> Layout {
        let inner = width.saturating_sub(PREFIX).max(4);
        let fresh = |start| Row {
            text: String::new(),
            start,
            cols: Vec::new(),
        };
        let mut rows: Vec<Row> = Vec::new();
        let mut row = fresh(0);
        let mut col = 0;
        let mut cursor = (0, 0);
        for i in 0..=self.buf.len() {
            let c = self.buf.get(i).copied();
            let shown = match c {
                Some('\n') | None => String::new(),
                Some(c) => match self.chip(c) {
                    Some(n) => self.chip_words(n),
                    None => c.to_string(),
                },
            };
            // Where the character would be drawn: on the next row when its
            // first column does not fit.
            let w0 = shown.chars().next().map_or(0, |c| c.width().unwrap_or(0));
            if col + w0 > inner || (c.is_none() && col >= inner) {
                rows.push(std::mem::replace(&mut row, fresh(i)));
                col = 0;
            }
            if i == self.cur {
                cursor = (rows.len(), col);
            }
            if c == Some('\n') {
                rows.push(std::mem::replace(&mut row, fresh(i + 1)));
                col = 0;
                continue;
            }
            for (n, ch) in shown.chars().enumerate() {
                let w = ch.width().unwrap_or(0);
                if n > 0 && col + w > inner {
                    rows.push(std::mem::replace(&mut row, fresh(i)));
                    col = 0;
                }
                row.text.push(ch);
                col += w;
            }
            if c.is_some() {
                row.cols.push(col);
            }
        }
        rows.push(row);
        Layout { rows, cursor }
    }

    /// Move the cursor up a row in a layout of `width`; false when it is on
    /// the first (the caller recalls history).
    pub fn up(&mut self, width: usize) -> bool {
        let l = self.layout(width);
        if l.cursor.0 == 0 {
            return false;
        }
        self.cur = self.index_at(&l, l.cursor.0 - 1, l.cursor.1);
        true
    }

    /// Move the cursor down a row; false when it is on the last.
    pub fn down(&mut self, width: usize) -> bool {
        let l = self.layout(width);
        if l.cursor.0 + 1 >= l.rows.len() {
            return false;
        }
        self.cur = self.index_at(&l, l.cursor.0 + 1, l.cursor.1);
        true
    }

    fn index_at(&self, l: &Layout, row: usize, col: usize) -> usize {
        let r = &l.rows[row];
        let mut idx = r.start;
        for (n, &end) in r.cols.iter().enumerate() {
            if end > col {
                break;
            }
            idx = r.start + n + 1;
        }
        idx.min(self.buf.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Editor {
        let mut e = Editor::default();
        e.insert_str(s);
        e
    }

    #[test]
    fn a_long_paste_is_one_chip_and_goes_back_whole() {
        let log: String = (1..=60).map(|n| format!("line {n}\n")).collect();
        let mut e = Editor::default();
        e.paste(&log);
        e.insert_str("what went wrong?");
        let rows = e.layout(80).rows;
        assert_eq!(rows.len(), 1, "one chip and the question share a row");
        assert!(
            rows[0].text.starts_with("[pasted 60 lines · "),
            "{}",
            rows[0].text
        );
        assert!(rows[0].text.contains("ctrl+o shows it]"));
        assert!(rows[0].text.ends_with("what went wrong?"));
        let sent = e.take();
        assert_eq!(sent.text, format!("{log}what went wrong?"));
        assert!(
            sent.shown.starts_with("[pasted 60 lines · "),
            "{}",
            sent.shown
        );
        assert!(
            sent.shown.ends_with("KB] what went wrong?"),
            "{}",
            sent.shown
        );
        assert!(e.is_empty());
    }

    #[test]
    fn a_short_paste_is_text_and_a_chip_is_one_backspace() {
        let mut e = Editor::default();
        e.paste("a\nb\nc");
        assert_eq!(e.text(), "a\nb\nc");
        let mut f = Editor::default();
        f.paste(&"x\n".repeat(9));
        f.backspace();
        assert!(f.is_empty());
        assert_eq!(f.text(), "");
    }

    #[test]
    fn crlf_pastes_count_their_lines_once() {
        let mut e = Editor::default();
        e.paste(&"x\r\n".repeat(5));
        assert_eq!(e.text(), "x\nx\nx\nx\nx\n", "five lines do not fold");
        let mut f = Editor::default();
        f.paste(&"x\r\n".repeat(6));
        assert!(f.layout(80).rows[0].text.starts_with("[pasted 6 lines"));
    }

    #[test]
    fn a_backslash_then_enter_is_a_newline() {
        let mut e = typed("one\\");
        assert!(e.continue_line());
        e.insert_str("two");
        assert_eq!(e.text(), "one\ntwo");
        assert!(!typed("plain").continue_line());
    }

    #[test]
    fn rows_wrap_at_the_width_and_the_cursor_follows() {
        let mut e = typed("abcdefghij");
        let l = e.layout(8); // 6 columns after the prompt
        assert_eq!(
            l.rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["abcdef", "ghij"]
        );
        assert_eq!(l.cursor, (1, 4));
        assert!(e.up(8));
        assert_eq!(e.layout(8).cursor, (0, 4));
        assert!(!e.up(8), "the first row is history's");
        assert!(e.down(8));
        assert!(!e.down(8));
    }

    #[test]
    fn up_and_down_walk_the_directorys_history_and_back_to_the_draft() {
        let mut e = Editor::with_history(vec!["first".into(), "second".into()]);
        e.insert_str("draft");
        e.history_prev();
        assert_eq!(e.text(), "second");
        e.history_prev();
        assert_eq!(e.text(), "first");
        e.history_prev();
        assert_eq!(e.text(), "first");
        e.history_next();
        e.history_next();
        assert_eq!(e.text(), "draft");
    }

    #[test]
    fn a_taken_message_is_remembered_once() {
        let mut e = typed("hello");
        e.take();
        e.insert_str("hello");
        e.take();
        assert_eq!(e.history(), ["hello"]);
    }

    #[test]
    fn editing_moves_by_word_and_line() {
        let mut e = typed("one two\nthree");
        e.home();
        e.insert('>');
        assert_eq!(e.text(), "one two\n>three");
        e.word_left();
        e.kill_word();
        assert_eq!(e.text(), "one >three");
        let mut f = typed("abc def");
        f.kill_word();
        assert_eq!(f.text(), "abc ");
        f.kill_to_start();
        assert!(f.is_empty());
    }

    #[test]
    fn a_control_character_is_not_text() {
        let mut e = Editor::default();
        e.insert('\u{7}');
        e.insert('\t');
        assert!(e.is_empty());
    }
}
