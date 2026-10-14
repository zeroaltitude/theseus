//! The styled line (theseus-rn7s): a line of spans, each with a style, which the
//! CLI writes as ANSI (SGR for the style, OSC 8 for a link) and the terminal UI
//! turns into its own spans (theseus-tui keeps that conversion: this crate does
//! not link ratatui). Every line is complete in itself: it ends by resetting
//! what it set, so a stream of lines can be cut anywhere between two of them.

use std::fmt::Write as _;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The palette: the terminal's own sixteen, so a theme's choices hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    /// Bright black: comments and rules.
    Gray,
}

impl Color {
    /// The SGR code of this colour as a foreground.
    fn sgr(self) -> u8 {
        match self {
            Color::Red => 31,
            Color::Green => 32,
            Color::Yellow => 33,
            Color::Blue => 34,
            Color::Magenta => 35,
            Color::Cyan => 36,
            Color::Gray => 90,
        }
    }
}

/// How a span looks. The default is plain text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub underline: bool,
    pub strike: bool,
    pub fg: Option<Color>,
    /// The target of an OSC 8 link; the terminal UI has no use for it.
    pub link: Option<Arc<str>>,
}

impl Style {
    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Self::default()
        }
    }

    pub fn italic() -> Self {
        Self {
            italic: true,
            ..Self::default()
        }
    }

    pub fn dim() -> Self {
        Self {
            dim: true,
            ..Self::default()
        }
    }

    pub fn fg(color: Color) -> Self {
        Self {
            fg: Some(color),
            ..Self::default()
        }
    }

    /// Whether this style changes nothing.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    /// The SGR parameters (`1;3;36`), none for a style that sets no
    /// attribute (a link alone).
    fn sgr(&self) -> String {
        let mut codes: Vec<u8> = Vec::new();
        if self.bold {
            codes.push(1);
        }
        if self.dim {
            codes.push(2);
        }
        if self.italic {
            codes.push(3);
        }
        if self.underline {
            codes.push(4);
        }
        if self.strike {
            codes.push(9);
        }
        if let Some(c) = self.fg {
            codes.push(c.sgr());
        }
        codes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(";")
    }
}

/// A run of text in one style. It holds no newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

impl Span {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Style::default())
    }
}

/// One line: spans, left to right.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyledLine {
    pub spans: Vec<Span>,
}

impl StyledLine {
    pub fn new(spans: Vec<Span>) -> Self {
        Self { spans }
    }

    pub fn blank() -> Self {
        Self::default()
    }

    pub fn from_text(text: impl Into<String>, style: Style) -> Self {
        Self {
            spans: vec![Span::new(text, style)],
        }
    }

    /// Appends a span, joining it to the last when the style is the same.
    pub fn push(&mut self, text: &str, style: &Style) {
        if text.is_empty() {
            return;
        }
        match self.spans.last_mut() {
            Some(last) if last.style == *style => last.text.push_str(text),
            _ => self.spans.push(Span::new(text, style.clone())),
        }
    }

    pub fn push_span(&mut self, s: Span) {
        self.push(&s.text, &s.style);
    }

    pub fn extend(&mut self, other: StyledLine) {
        for s in other.spans {
            self.push_span(s);
        }
    }

    /// A line with `prefix` before it.
    pub fn prefixed(mut self, prefix: &Span) -> Self {
        self.spans.insert(0, prefix.clone());
        self
    }

    /// The text alone, with no escape sequence.
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    /// The display width of the text, in terminal columns.
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| text_width(&s.text)).sum()
    }

    pub fn is_blank(&self) -> bool {
        self.spans.iter().all(|s| s.text.is_empty())
    }

    /// The line written for a terminal: SGR around each styled span, OSC 8
    /// around a link, and a reset at the end of whatever it set. A line of
    /// plain spans holds no escape sequence.
    pub fn ansi(&self) -> String {
        let mut out = String::new();
        for s in &self.spans {
            if s.text.is_empty() {
                continue;
            }
            let sgr = s.style.sgr();
            if let Some(url) = &s.style.link {
                let _ = write!(out, "\x1b]8;;{}\x1b\\", osc_safe(url));
            }
            if sgr.is_empty() {
                out.push_str(&s.text);
            } else {
                let _ = write!(out, "\x1b[{sgr}m{}\x1b[0m", s.text);
            }
            if s.style.link.is_some() {
                out.push_str("\x1b]8;;\x1b\\");
            }
        }
        out
    }

    /// The line cut to `max` columns: when it is wider, it ends in `…` at
    /// `max`. Styles hold.
    pub fn truncated(&self, max: usize) -> StyledLine {
        if self.width() <= max {
            return self.clone();
        }
        let mut out = StyledLine::default();
        let room = max.saturating_sub(1);
        let mut used = 0;
        'spans: for s in &self.spans {
            let mut piece = String::new();
            for ch in s.text.chars() {
                let w = ch.width().unwrap_or(0);
                if used + w > room {
                    out.push(&piece, &s.style);
                    break 'spans;
                }
                used += w;
                piece.push(ch);
            }
            out.push(&piece, &s.style);
        }
        if max > 0 {
            out.push("…", &Style::dim());
        }
        out
    }
}

/// A link target with no control character, which could end the sequence
/// early or carry another.
fn osc_safe(url: &str) -> String {
    url.chars().filter(|c| !c.is_control()).collect()
}

/// The display width of `s`, in terminal columns.
pub fn text_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// The display width of a character, in terminal columns.
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// How the renderers write what they draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Styles on spans, and a link as a target on its text (OSC 8 once written
    /// as ANSI).
    Ansi,
    /// No style anywhere: a heading keeps its `#`, a quote its `>`, and a
    /// link its address after its text.
    Plain,
}

/// `--plain` was given: the CLI prints its text as it always did.
static PLAIN: AtomicBool = AtomicBool::new(false);

/// Sets what `--plain` asked for, once, when the arguments are read.
pub fn set_plain(on: bool) {
    PLAIN.store(on, Ordering::Relaxed);
}

/// Whether the CLI's stdout shows styles: a terminal, `NO_COLOR` unset or
/// empty (no-color.org), `TERM` not `dumb`, and no `--plain`.
pub fn styled_stdout() -> bool {
    !PLAIN.load(Ordering::Relaxed) && std::io::stdout().is_terminal() && color_allowed()
}

/// `NO_COLOR` and `TERM`, apart from any stream.
pub fn color_allowed() -> bool {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    !set("NO_COLOR") && std::env::var("TERM").map_or(true, |t| t != "dumb")
}

/// The columns of the terminal: `COLUMNS`, then the stdout terminal's size,
/// else 80.
pub fn terminal_width() -> usize {
    if let Some(n) = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
    {
        return n;
    }
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCGWINSZ writes one `winsize` through the pointer we pass.
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
    if ok && ws.ws_col > 0 {
        usize::from(ws.ws_col)
    } else {
        80
    }
}

/// The width prose wraps at on a terminal of `cols` columns: one short of
/// them, so a line never fills the last column, where a terminal's own wrap
/// begins.
pub fn prose_width(cols: usize) -> usize {
    cols.saturating_sub(1).max(20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_written_as_sgr_and_a_plain_one_holds_no_escape() {
        let mut l = StyledLine::default();
        l.push("a ", &Style::default());
        l.push("bold", &Style::bold());
        l.push(" and ", &Style::default());
        l.push(
            "code",
            &Style {
                fg: Some(Color::Cyan),
                italic: true,
                ..Style::default()
            },
        );
        assert_eq!(l.ansi(), "a \x1b[1mbold\x1b[0m and \x1b[3;36mcode\x1b[0m");
        assert_eq!(l.plain(), "a bold and code");
        assert!(!StyledLine::from_text("x", Style::default())
            .ansi()
            .contains('\x1b'));
    }

    #[test]
    fn a_link_is_osc_8_and_drops_a_control_byte_in_its_target() {
        let style = Style {
            underline: true,
            link: Some(Arc::from("http://a.example/\x1b]x")),
            ..Style::default()
        };
        let l = StyledLine::from_text("see", style);
        assert_eq!(
            l.ansi(),
            "\x1b]8;;http://a.example/]x\x1b\\\x1b[4msee\x1b[0m\x1b]8;;\x1b\\"
        );
    }

    #[test]
    fn a_cut_line_ends_in_a_marker_at_the_width() {
        let mut l = StyledLine::default();
        l.push("abcdef", &Style::bold());
        l.push("ghij", &Style::default());
        let t = l.truncated(8);
        assert_eq!(t.plain(), "abcdefg…");
        assert_eq!(t.width(), 8);
        let wide = StyledLine::from_text("日本語です", Style::default()).truncated(6);
        assert_eq!(wide.plain(), "日本…");
        assert_eq!(l.truncated(20), l);
    }
}
