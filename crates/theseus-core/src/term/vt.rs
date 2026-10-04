//! A terminal's screen (theseus-n88g.4): a small VT model of Theseus's own,
//! fed the bytes a program writes to its pty, and read as rows of text.
//!
//! What it models: printable text (UTF-8, one cell a character), CR, LF,
//! VT and FF as LF, backspace, tab stops every 8 columns, the deferred wrap
//! at the right margin, the cursor moves of CSI (`A`–`H`, `d`, `f`, `` ` ``),
//! erase in line and display (`K`, `J`, `X`), insert and delete of lines and
//! characters (`L`, `M`, `@`, `P`), scrolling (`S`, `T`, index, reverse
//! index), the scroll region (`r`), saving the cursor (`ESC 7`/`8`, `s`/`u`),
//! the alternate screen (`?1049`, `?1047`, `?47`), autowrap (`?7`), and the
//! answers a program may wait for: the cursor's position (`6n`), status
//! (`5n`), and the device attributes (`c`, `>c`).
//!
//! What it leaves out: colors and every other attribute (SGR is read and
//! dropped), wide characters (a CJK character or an emoji takes one cell,
//! not two), combining marks (each takes a cell of its own), insert mode
//! (`4h`), origin mode (`?6`), tab stops set by the program (`HTS`, `TBC`),
//! character sets (`ESC (` is read and dropped, so line drawing shows as
//! the ASCII letters it is sent as), double-width lines, and the mouse.
//! Every escape it does not know is read whole and dropped, so it never
//! shows as text.

use std::collections::VecDeque;

/// The most lines kept that scrolled off the top of the main screen.
pub const SCROLLBACK: usize = 2000;

/// The main screen's cells and cursor, kept while the alternate one shows.
type Saved = (Vec<Vec<char>>, (usize, usize));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// `ESC (` and its kin: the next character names a character set.
    Charset,
    Csi,
    /// OSC, DCS, SOS, PM, APC: a string up to BEL or ST; `true` once an ESC
    /// was read in it.
    Str(bool),
}

/// A screen: its cells, its cursor, and what scrolled off its top.
#[derive(Debug, Clone)]
pub struct Screen {
    rows: usize,
    cols: usize,
    grid: Vec<Vec<char>>,
    row: usize,
    col: usize,
    /// At the right margin with the last character written there: the next
    /// printable wraps first.
    wrap_next: bool,
    saved: (usize, usize),
    /// The scroll region, inclusive.
    top: usize,
    bottom: usize,
    autowrap: bool,
    /// The main screen's cells and cursor while the alternate one shows.
    main: Option<Saved>,
    state: State,
    params: String,
    utf8: Vec<u8>,
    /// What the program asked of the terminal: written back to its pty.
    replies: Vec<u8>,
    scrollback: VecDeque<String>,
    /// Lines that scrolled off the top of the main screen since it began.
    scrolled: u64,
}

impl Screen {
    pub fn new(rows: u16, cols: u16) -> Self {
        let (rows, cols) = (usize::from(rows.max(1)), usize::from(cols.max(1)));
        Self {
            rows,
            cols,
            grid: vec![vec![' '; cols]; rows],
            row: 0,
            col: 0,
            wrap_next: false,
            saved: (0, 0),
            top: 0,
            bottom: rows - 1,
            autowrap: true,
            main: None,
            state: State::Ground,
            params: String::new(),
            utf8: Vec::new(),
            replies: Vec::new(),
            scrollback: VecDeque::new(),
            scrolled: 0,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        (self.rows as u16, self.cols as u16)
    }

    /// The cursor, 0-based: row, then column.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Whether the alternate screen shows (a full-screen program: `vim`,
    /// `less`).
    pub fn alternate(&self) -> bool {
        self.main.is_some()
    }

    /// Each row as text, its trailing blanks cut.
    pub fn rows(&self) -> Vec<String> {
        self.grid
            .iter()
            .map(|r| r.iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    /// Lines that scrolled off the top of the main screen since it began.
    pub fn scrolled(&self) -> u64 {
        self.scrolled
    }

    /// The last `n` lines that scrolled off (at most `SCROLLBACK`), oldest
    /// first.
    pub fn scrollback_tail(&self, n: usize) -> Vec<String> {
        let skip = self.scrollback.len().saturating_sub(n);
        self.scrollback.iter().skip(skip).cloned().collect()
    }

    /// What the program asked of the terminal since the last take: the
    /// answers to write back to it.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Feed the bytes a program wrote.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        if !self.utf8.is_empty() {
            if (0x80..0xc0).contains(&b) {
                self.utf8.push(b);
                if self.utf8.len() == utf8_len(self.utf8[0]) {
                    let c = std::str::from_utf8(&self.utf8)
                        .ok()
                        .and_then(|s| s.chars().next())
                        .unwrap_or('\u{fffd}');
                    self.utf8.clear();
                    self.char(c);
                }
                return;
            }
            // A sequence cut short.
            self.utf8.clear();
            self.char('\u{fffd}');
        }
        match b {
            0..=0x7f => self.char(char::from(b)),
            0xc2..=0xf4 => self.utf8.push(b),
            _ => self.char('\u{fffd}'),
        }
    }

    fn char(&mut self, c: char) {
        match self.state {
            State::Ground => self.ground(c),
            State::Escape => self.escape(c),
            State::Charset => self.state = State::Ground,
            State::Csi => self.csi_byte(c),
            State::Str(esc) => match c {
                '\x07' => self.state = State::Ground,
                '\\' if esc => self.state = State::Ground,
                '\x1b' => self.state = State::Str(true),
                _ => self.state = State::Str(false),
            },
        }
    }

    fn ground(&mut self, c: char) {
        match c {
            '\x1b' => self.state = State::Escape,
            '\r' => {
                self.col = 0;
                self.wrap_next = false;
            }
            '\n' | '\x0b' | '\x0c' => self.linefeed(),
            '\x08' => {
                self.col = self.col.saturating_sub(1);
                self.wrap_next = false;
            }
            '\t' => {
                self.col = ((self.col / 8 + 1) * 8).min(self.cols - 1);
                self.wrap_next = false;
            }
            c if (c as u32) < 0x20 || ('\u{7f}'..='\u{9f}').contains(&c) => {}
            c => self.put(c),
        }
    }

    fn escape(&mut self, c: char) {
        self.state = State::Ground;
        match c {
            '[' => {
                self.params.clear();
                self.state = State::Csi;
            }
            ']' | 'P' | 'X' | '^' | '_' => self.state = State::Str(false),
            '(' | ')' | '*' | '+' | '-' | '.' | '/' | '#' | '%' => self.state = State::Charset,
            '7' => self.saved = (self.row, self.col),
            '8' => self.restore(),
            'D' => self.linefeed(),
            'E' => {
                self.col = 0;
                self.linefeed();
            }
            'M' => self.reverse_index(),
            'c' => {
                let scrollback = std::mem::take(&mut self.scrollback);
                let scrolled = self.scrolled;
                *self = Screen::new(self.rows as u16, self.cols as u16);
                self.scrollback = scrollback;
                self.scrolled = scrolled;
            }
            '\x1b' => self.state = State::Escape,
            _ => {}
        }
    }

    fn csi_byte(&mut self, c: char) {
        match c {
            '\x1b' => self.state = State::Escape,
            // A control inside a sequence acts and the sequence goes on.
            c if (c as u32) < 0x20 => self.ground(c),
            '0'..='?' | ' '..='/' => {
                // A runaway sequence is dropped, not kept growing.
                if self.params.len() < 64 {
                    self.params.push(c);
                }
            }
            '@'..='~' => {
                self.state = State::Ground;
                let params = std::mem::take(&mut self.params);
                self.csi(&params, c);
            }
            _ => self.state = State::Ground,
        }
    }

    fn csi(&mut self, raw: &str, f: char) {
        let private = raw.chars().next().filter(|c| "?<=>".contains(*c));
        let body = raw.trim_start_matches(['?', '<', '=', '>']);
        // A sequence with intermediates (`CSI 2 SP q`, the cursor's shape)
        // changes nothing here.
        if body.contains(|c: char| (' '..='/').contains(&c)) {
            return;
        }
        let ps: Vec<u32> = body
            .split([';', ':'])
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect();
        if private.is_some() && !matches!(f, 'h' | 'l' | 'c') {
            return;
        }
        if !matches!(f, 'n' | 'c' | 'm' | 'h' | 'l' | 't') {
            self.wrap_next = false;
        }
        if !self.moves(f, &ps) && !self.edits(f, &ps) {
            self.other(f, private, &ps);
        }
    }

    /// The cursor's moves, and the scroll region: whether `f` was one.
    fn moves(&mut self, f: char, ps: &[u32]) -> bool {
        let (rows, cols) = (self.rows, self.cols);
        let p = |i: usize| ps.get(i).copied().unwrap_or(0);
        let n = |i: usize| (p(i).max(1) as usize).min(10_000);
        match f {
            'A' => {
                let min = if self.row >= self.top { self.top } else { 0 };
                self.row = self.row.saturating_sub(n(0)).max(min);
            }
            'B' | 'e' => {
                let max = if self.row <= self.bottom {
                    self.bottom
                } else {
                    rows - 1
                };
                self.row = (self.row + n(0)).min(max);
            }
            'C' | 'a' => self.col = (self.col + n(0)).min(cols - 1),
            'D' => self.col = self.col.saturating_sub(n(0)),
            'E' => {
                self.row = (self.row + n(0)).min(rows - 1);
                self.col = 0;
            }
            'F' => {
                self.row = self.row.saturating_sub(n(0));
                self.col = 0;
            }
            'G' | '`' => self.col = (n(0) - 1).min(cols - 1),
            'H' | 'f' => {
                self.row = (n(0) - 1).min(rows - 1);
                self.col = (n(1) - 1).min(cols - 1);
            }
            'd' => self.row = (n(0) - 1).min(rows - 1),
            'r' => {
                let top = n(0) - 1;
                let bottom = if p(1) == 0 {
                    rows - 1
                } else {
                    (p(1) as usize - 1).min(rows - 1)
                };
                if top < bottom {
                    self.top = top;
                    self.bottom = bottom;
                }
                self.row = 0;
                self.col = 0;
            }
            's' => self.saved = (self.row, self.col),
            'u' => self.restore(),
            _ => return false,
        }
        true
    }

    /// The erases, inserts, deletes, and scrolls: whether `f` was one.
    fn edits(&mut self, f: char, ps: &[u32]) -> bool {
        let (rows, cols) = (self.rows, self.cols);
        let p = |i: usize| ps.get(i).copied().unwrap_or(0);
        let n = |i: usize| (p(i).max(1) as usize).min(10_000);
        let in_region = (self.top..=self.bottom).contains(&self.row);
        match f {
            'J' => {
                let (from, to) = match p(0) {
                    0 => (self.row + 1, rows),
                    1 => (0, self.row),
                    _ => (0, rows),
                };
                match p(0) {
                    0 => self.erase_line(self.row, self.col, cols),
                    1 => self.erase_line(self.row, 0, self.col + 1),
                    _ => {}
                }
                for r in from..to {
                    self.erase_line(r, 0, cols);
                }
            }
            'K' => match p(0) {
                0 => self.erase_line(self.row, self.col, cols),
                1 => self.erase_line(self.row, 0, self.col + 1),
                _ => self.erase_line(self.row, 0, cols),
            },
            'X' => self.erase_line(self.row, self.col, self.col + n(0)),
            'L' | 'M' if in_region => {
                for _ in 0..n(0).min(rows) {
                    let (gone, at) = match f {
                        'L' => (self.bottom, self.row),
                        _ => (self.row, self.bottom),
                    };
                    self.grid.remove(gone);
                    self.grid.insert(at, vec![' '; cols]);
                }
                self.col = 0;
            }
            'P' | '@' => {
                let (col, line) = (self.col, &mut self.grid[self.row]);
                for _ in 0..n(0).min(cols - col) {
                    if f == 'P' {
                        line.remove(col);
                        line.push(' ');
                    } else {
                        line.pop();
                        line.insert(col, ' ');
                    }
                }
            }
            'S' => self.scroll_up(n(0)),
            // `CSI T` with more than one parameter is the mouse's.
            'T' if ps.len() <= 1 => self.scroll_down(n(0)),
            _ => return false,
        }
        true
    }

    /// Modes, and the answers a program waits for.
    fn other(&mut self, f: char, private: Option<char>, ps: &[u32]) {
        let p0 = ps.first().copied().unwrap_or(0);
        match f {
            'h' | 'l' if private == Some('?') => {
                for m in ps {
                    self.mode(*m, f == 'h');
                }
            }
            'n' if p0 == 6 => {
                let at = format!("\x1b[{};{}R", self.row + 1, self.col + 1);
                self.replies.extend_from_slice(at.as_bytes());
            }
            'n' if p0 == 5 => self.replies.extend_from_slice(b"\x1b[0n"),
            'c' if p0 == 0 && private.is_none() => {
                self.replies.extend_from_slice(b"\x1b[?1;2c");
            }
            'c' if p0 == 0 && private == Some('>') => {
                self.replies.extend_from_slice(b"\x1b[>0;0;0c");
            }
            _ => {}
        }
    }

    fn mode(&mut self, m: u32, on: bool) {
        match m {
            7 => self.autowrap = on,
            47 | 1047 | 1049 => {
                if on && self.main.is_none() {
                    if m == 1049 {
                        self.saved = (self.row, self.col);
                    }
                    let blank = vec![vec![' '; self.cols]; self.rows];
                    let main = std::mem::replace(&mut self.grid, blank);
                    self.main = Some((main, (self.row, self.col)));
                } else if !on {
                    if let Some((main, at)) = self.main.take() {
                        self.grid = main;
                        (self.row, self.col) = if m == 1049 { self.saved } else { at };
                    }
                }
                self.wrap_next = false;
            }
            _ => {}
        }
    }

    fn restore(&mut self) {
        (self.row, self.col) = self.saved;
        self.row = self.row.min(self.rows - 1);
        self.col = self.col.min(self.cols - 1);
        self.wrap_next = false;
    }

    fn put(&mut self, c: char) {
        if self.wrap_next {
            self.wrap_next = false;
            if self.autowrap {
                self.col = 0;
                self.linefeed();
            }
        }
        self.grid[self.row][self.col] = c;
        if self.col + 1 == self.cols {
            self.wrap_next = true;
        } else {
            self.col += 1;
        }
    }

    fn linefeed(&mut self) {
        self.wrap_next = false;
        if self.row == self.bottom {
            self.scroll_up(1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_next = false;
        if self.row == self.top {
            self.scroll_down(1);
        } else {
            self.row = self.row.saturating_sub(1);
        }
    }

    /// The region's lines move up by `n`; a line that leaves the top of the
    /// main screen's whole height is kept in the scrollback.
    fn scroll_up(&mut self, n: usize) {
        let keep = self.top == 0 && self.main.is_none();
        for _ in 0..n.min(self.bottom - self.top + 1) {
            let gone = self.grid.remove(self.top);
            self.grid.insert(self.bottom, vec![' '; self.cols]);
            if keep {
                self.scrollback
                    .push_back(gone.iter().collect::<String>().trim_end().to_string());
                if self.scrollback.len() > SCROLLBACK {
                    self.scrollback.pop_front();
                }
                self.scrolled += 1;
            }
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n.min(self.bottom - self.top + 1) {
            self.grid.remove(self.bottom);
            self.grid.insert(self.top, vec![' '; self.cols]);
        }
    }

    fn erase_line(&mut self, row: usize, from: usize, to: usize) {
        let line = &mut self.grid[row];
        let to = to.min(line.len());
        for cell in line.iter_mut().take(to).skip(from) {
            *cell = ' ';
        }
    }
}

/// A UTF-8 sequence's length, by its first byte.
fn utf8_len(lead: u8) -> usize {
    match lead {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}
