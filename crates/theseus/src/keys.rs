//! `watch --interactive`'s input (theseus-tnky). A terminal in its usual line
//! mode shows a program nothing until Enter, so the first keystroke of a
//! message, the moment the daemon could start warming what it will wait for,
//! is invisible. On a terminal, then, the watch reads bytes (the terminal
//! out of canonical mode and echo, signals still on, restored on every way
//! out, a panic's included) and keeps the line itself: printable text,
//! backspace, Ctrl-U (the line), Ctrl-W (a word), Enter, and Ctrl-D on an
//! empty line to end. Arrow keys and other escape sequences are swallowed, as
//! the terminal's own line mode would have put them in the line as noise.
//! Anything else (a pipe, a file) is read by lines, as before.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::OnceLock;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader, Lines, Stdin};

/// What the input gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Input {
    /// A first key of a line: someone began to type.
    Typing,
    /// A whole line, without its end.
    Line(String),
    /// The end of input.
    Eof,
}

/// The input: a terminal's keys, or lines.
pub enum Source {
    Keys(Keys),
    Lines(Lines<BufReader<Stdin>>),
}

impl Source {
    /// The terminal's keys when stdin is one (and its mode can be changed),
    /// else its lines.
    pub fn open() -> Self {
        match Terminal::enter() {
            Some(t) => Self::Keys(Keys::new(t)),
            None => Self::Lines(BufReader::new(tokio::io::stdin()).lines()),
        }
    }

    pub async fn next(&mut self) -> std::io::Result<Input> {
        match self {
            Self::Keys(k) => k.next().await,
            Self::Lines(l) => Ok(l.next_line().await?.map_or(Input::Eof, Input::Line)),
        }
    }
}

/// The terminal's original modes, for a panic's hook (a release build aborts
/// on one, which runs no destructor).
static ORIGINAL: OnceLock<libc::termios> = OnceLock::new();

/// The terminal out of line mode, put back on drop.
pub struct Terminal;

impl Terminal {
    fn enter() -> Option<Self> {
        if !std::io::stdin().is_terminal() {
            return None;
        }
        // SAFETY: termios is plain data, and fd 0 is a terminal (checked).
        let orig = unsafe {
            let mut t = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(0, &mut t) != 0 {
                return None;
            }
            t
        };
        let mut raw = orig;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        // SAFETY: as above.
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        if ORIGINAL.set(orig).is_ok() {
            let hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                restore();
                hook(info);
            }));
        }
        Some(Self)
    }
}

fn restore() {
    if let Some(orig) = ORIGINAL.get() {
        // SAFETY: a saved termios, put back on the terminal it came from.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, orig);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        restore();
    }
}

/// The key reader and the line it keeps.
pub struct Keys {
    _term: Terminal,
    queue: VecDeque<u8>,
    line: Vec<u8>,
    /// Inside an escape sequence: `1` after ESC, `2` inside `ESC [`.
    esc: u8,
    stdin: tokio::io::Stdin,
}

impl Keys {
    fn new(term: Terminal) -> Self {
        Self {
            _term: term,
            queue: VecDeque::new(),
            line: Vec::new(),
            esc: 0,
            stdin: tokio::io::stdin(),
        }
    }

    pub async fn next(&mut self) -> std::io::Result<Input> {
        loop {
            while let Some(b) = self.queue.pop_front() {
                if let Some(i) = self.feed(b) {
                    return Ok(i);
                }
            }
            let mut buf = [0u8; 256];
            let n = self.stdin.read(&mut buf).await?;
            if n == 0 {
                return Ok(Input::Eof);
            }
            self.queue.extend(&buf[..n]);
        }
    }

    /// One byte: what, if anything, it makes of the input.
    fn feed(&mut self, b: u8) -> Option<Input> {
        match self.esc {
            1 => {
                self.esc = if b == b'[' { 2 } else { 0 };
                return None;
            }
            // A sequence ends at its final byte.
            2 => {
                if (0x40..=0x7e).contains(&b) {
                    self.esc = 0;
                }
                return None;
            }
            _ => {}
        }
        match b {
            0x1b => self.esc = 1,
            b'\r' | b'\n' => {
                echo(b"\n");
                let line = String::from_utf8_lossy(&std::mem::take(&mut self.line)).into_owned();
                return Some(Input::Line(line));
            }
            0x7f | 0x08 => self.erase_char(),
            0x15 => self.erase_all(),
            0x17 => self.erase_word(),
            0x04 if self.line.is_empty() => return Some(Input::Eof),
            b'\t' | 0x20..=0x7e | 0x80..=0xff => {
                let first = self.line.is_empty();
                self.line.push(b);
                echo(&[b]);
                // Typing is told on the line's first byte: a multi-byte
                // character's later bytes are no new keystroke.
                return first.then_some(Input::Typing);
            }
            _ => {}
        }
        None
    }

    fn erase_char(&mut self) {
        // One character: its continuation bytes first.
        let mut any = false;
        while let Some(b) = self.line.pop() {
            any = true;
            if b & 0xc0 != 0x80 {
                break;
            }
        }
        if any {
            echo(b"\x08 \x08");
        }
    }

    fn erase_all(&mut self) {
        while !self.line.is_empty() {
            self.erase_char();
        }
    }

    fn erase_word(&mut self) {
        while self.line.last() == Some(&b' ') {
            self.erase_char();
        }
        while self.line.last().is_some_and(|b| *b != b' ') {
            self.erase_char();
        }
    }
}

fn echo(bytes: &[u8]) {
    let mut out = std::io::stderr().lock();
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader with no terminal: the line's editing alone.
    fn keys() -> Keys {
        Keys {
            _term: Terminal,
            queue: VecDeque::new(),
            line: Vec::new(),
            esc: 0,
            stdin: tokio::io::stdin(),
        }
    }

    fn typed(k: &mut Keys, s: &[u8]) -> Vec<Input> {
        s.iter().filter_map(|b| k.feed(*b)).collect()
    }

    #[test]
    fn the_first_byte_of_a_line_is_typing_and_enter_ends_it() {
        let mut k = keys();
        assert_eq!(
            typed(&mut k, b"hi there\r"),
            [Input::Typing, Input::Line("hi there".into())]
        );
        assert_eq!(
            typed(&mut k, b"again\n"),
            [Input::Typing, Input::Line("again".into())],
            "the next line begins again"
        );
    }

    #[test]
    fn editing_keys_change_the_line_and_a_multibyte_character_goes_whole() {
        let mut k = keys();
        let got = typed(
            &mut k,
            "tide pool\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}é\u{7f}x\r".as_bytes(),
        );
        assert_eq!(got, [Input::Typing, Input::Line("tidex".into())]);
        let got = typed(&mut k, b"one two\x17three\x15four\r");
        // Each line's first byte tells; the client's spell keeps it to one.
        assert_eq!(
            got,
            [Input::Typing, Input::Typing, Input::Line("four".into())]
        );
    }

    #[test]
    fn escape_sequences_are_swallowed_and_ctrl_d_ends_only_an_empty_line() {
        let mut k = keys();
        assert_eq!(
            typed(&mut k, b"a\x1b[Db\x1b[1;5Cc\r"),
            [Input::Typing, Input::Line("abc".into())]
        );
        assert_eq!(typed(&mut k, b"\x04"), [Input::Eof]);
        assert_eq!(
            typed(&mut k, b"x\x04\r"),
            [Input::Typing, Input::Line("x".into())]
        );
    }
}
