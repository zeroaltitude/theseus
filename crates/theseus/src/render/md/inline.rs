//! Markdown's inline text and its word wrap (theseus-rn7s): emphasis, code, links,
//! strike and escapes into styled spans, and spans into lines that break at
//! spaces by display width. A word longer than a line is the only thing cut
//! inside.

use std::sync::Arc;

use super::super::style::{char_width, Color, Mode, Span, Style, StyledLine};

/// Nesting deeper than this is left as it is written.
const DEPTH: usize = 6;

/// Styles in a heading, a link, a code span.
pub fn code_style() -> Style {
    Style::fg(Color::Cyan)
}

fn link_style(url: &str) -> Style {
    Style {
        underline: true,
        fg: Some(Color::Blue),
        link: Some(Arc::from(url)),
        ..Style::default()
    }
}

/// `src` as spans in `base`, markup taken off. A `\n` in `src` stays in a
/// span: `wrap` breaks the line there.
pub fn inline(src: &str, base: &Style, mode: Mode) -> Vec<Span> {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Inline {
        mode,
        out: Vec::new(),
    };
    p.run(&chars, base, 0);
    p.out
}

struct Inline {
    mode: Mode,
    out: Vec<Span>,
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

fn text_of(s: &[char]) -> String {
    s.iter().collect()
}

impl Inline {
    fn push(&mut self, text: &str, style: &Style) {
        if text.is_empty() {
            return;
        }
        match self.out.last_mut() {
            Some(last) if last.style == *style => last.text.push_str(text),
            _ => self.out.push(Span::new(text, style.clone())),
        }
    }

    fn run(&mut self, s: &[char], st: &Style, depth: usize) {
        let mut i = 0;
        let mut lit = String::new();
        while i < s.len() {
            let next =
                Self::escape(s, i, &mut lit).or_else(|| self.special(s, i, st, depth, &mut lit));
            match next {
                Some(n) => i = n,
                None => {
                    lit.push(s[i]);
                    i += 1;
                }
            }
        }
        self.push(&lit, st);
    }

    /// `\*` is a `*`; a backslash before anything else is itself.
    fn escape(s: &[char], i: usize, lit: &mut String) -> Option<usize> {
        if s[i] == '\\' && s.get(i + 1).copied().is_some_and(is_punct) {
            lit.push(s[i + 1]);
            return Some(i + 2);
        }
        None
    }

    /// A construct that starts at `i`: it flushes the text before it. None
    /// leaves `s[i]` as plain text.
    fn special(
        &mut self,
        s: &[char],
        i: usize,
        st: &Style,
        depth: usize,
        lit: &mut String,
    ) -> Option<usize> {
        let mut at = Inline {
            mode: self.mode,
            out: Vec::new(),
        };
        let next = match s[i] {
            '`' => at.code(s, i, st),
            '*' | '_' if depth < DEPTH => at.emphasis(s, i, st, depth),
            '~' if depth < DEPTH => at.strike(s, i, st, depth),
            '[' if depth < DEPTH => at.link(s, i, st, depth, false),
            '!' if depth < DEPTH && s.get(i + 1) == Some(&'[') => {
                at.link(s, i + 1, st, depth, true)
            }
            '<' => at.autolink(s, i, st),
            'h' => at.bare_url(s, i, st),
            _ => None,
        }?;
        let flushed = std::mem::take(lit);
        self.push(&flushed, st);
        for sp in at.out {
            self.push(&sp.text, &sp.style);
        }
        Some(next)
    }

    /// A code span: the same run of backticks to close it.
    fn code(&mut self, s: &[char], i: usize, st: &Style) -> Option<usize> {
        let n = s[i..].iter().take_while(|c| **c == '`').count();
        let mut j = i + n;
        while j < s.len() {
            if s[j] == '`' {
                let run = s[j..].iter().take_while(|c| **c == '`').count();
                if run == n {
                    let inner = text_of(&s[i + n..j]).replace('\n', " ");
                    let both = inner.starts_with(' ') && inner.ends_with(' ') && inner.trim() != "";
                    let inner = if both {
                        &inner[1..inner.len() - 1]
                    } else {
                        &inner[..]
                    };
                    let (text, style) = match self.mode {
                        Mode::Ansi => (inner.to_string(), merge(st, &code_style())),
                        Mode::Plain => (format!("`{inner}`"), st.clone()),
                    };
                    self.push(&text, &style);
                    return Some(j + run);
                }
                j += run;
            } else {
                j += 1;
            }
        }
        None
    }

    /// `*x*`, `**x**`, `***x***`, and the same with `_`.
    fn emphasis(&mut self, s: &[char], i: usize, st: &Style, depth: usize) -> Option<usize> {
        let c = s[i];
        let run = s[i..].iter().take_while(|x| **x == c).count();
        let after = s.get(i + run).copied();
        if run > 3 || after.is_none_or(char::is_whitespace) {
            return None;
        }
        if c == '_' && i > 0 && s[i - 1].is_alphanumeric() {
            return None;
        }
        let close = closer(s, i + run, c, run)?;
        let style = Style {
            bold: st.bold || run >= 2,
            italic: st.italic || run != 2,
            ..st.clone()
        };
        let inner = &s[i + run..close];
        self.run(inner, &style, depth + 1);
        Some(close + run)
    }

    fn strike(&mut self, s: &[char], i: usize, st: &Style, depth: usize) -> Option<usize> {
        if s.get(i + 1) != Some(&'~') || s.get(i + 2).is_none_or(|c| c.is_whitespace() || *c == '~')
        {
            return None;
        }
        let close = closer(s, i + 2, '~', 2)?;
        let style = Style {
            strike: true,
            ..st.clone()
        };
        self.run(&s[i + 2..close], &style, depth + 1);
        Some(close + 2)
    }

    /// `[text](url)` and `![alt](url)`; `at` is the `[`.
    fn link(
        &mut self,
        s: &[char],
        at: usize,
        st: &Style,
        depth: usize,
        image: bool,
    ) -> Option<usize> {
        let text_end = matching(s, at, '[', ']')?;
        if s.get(text_end + 1) != Some(&'(') {
            return None;
        }
        let url_end = matching(s, text_end + 1, '(', ')')?;
        let target = text_of(&s[text_end + 2..url_end]);
        let url = target
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|c| c == '<' || c == '>');
        if url.is_empty() {
            return None;
        }
        let mut text: Vec<char> = s[at + 1..text_end].to_vec();
        if image {
            let alt: String = text.iter().collect();
            text = format!("[image: {alt}]").chars().collect();
        }
        self.linked(&text, url, st, depth);
        Some(url_end + 1)
    }

    /// `text` as a link to `url`: a target on the text in ANSI, the address
    /// after the text otherwise.
    fn linked(&mut self, text: &[char], url: &str, st: &Style, depth: usize) {
        match self.mode {
            Mode::Ansi => {
                let style = merge(st, &link_style(url));
                self.run(text, &style, depth + 1);
            }
            Mode::Plain => {
                self.run(text, st, depth + 1);
                if text_of(text) != url {
                    self.push(&format!(" ({url})"), st);
                }
            }
        }
    }

    fn autolink(&mut self, s: &[char], i: usize, st: &Style) -> Option<usize> {
        let end = s[i..].iter().position(|c| *c == '>')? + i;
        let url = text_of(&s[i + 1..end]);
        let known = ["http://", "https://", "mailto:"];
        if url.contains(char::is_whitespace) || !known.iter().any(|k| url.starts_with(k)) {
            return None;
        }
        let text: Vec<char> = url.chars().collect();
        self.linked(&text, &url, st, DEPTH);
        Some(end + 1)
    }

    /// A bare address in running text: linked in ANSI, text as it is
    /// otherwise.
    fn bare_url(&mut self, s: &[char], i: usize, st: &Style) -> Option<usize> {
        if i > 0 && (s[i - 1].is_alphanumeric() || s[i - 1] == '/') {
            return None;
        }
        let rest = text_of(&s[i..s.len().min(i + 8)]);
        if !rest.starts_with("http://") && !rest.starts_with("https://") {
            return None;
        }
        let mut end = i;
        while end < s.len() && !s[end].is_whitespace() && !matches!(s[end], '<' | '>' | '"') {
            end += 1;
        }
        while end > i + 8
            && matches!(
                s[end - 1],
                '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '\''
            )
        {
            end -= 1;
        }
        let url = text_of(&s[i..end]);
        let text: Vec<char> = url.chars().collect();
        match self.mode {
            Mode::Ansi => self.run_flat(&text, &merge(st, &link_style(&url))),
            Mode::Plain => self.run_flat(&text, st),
        }
        Some(end)
    }

    /// Text with no markup read in it.
    fn run_flat(&mut self, text: &[char], style: &Style) {
        self.push(&text_of(text), style);
    }
}

/// `st` with `extra`'s attributes on top.
fn merge(st: &Style, extra: &Style) -> Style {
    Style {
        bold: st.bold || extra.bold,
        italic: st.italic || extra.italic,
        dim: st.dim || extra.dim,
        underline: st.underline || extra.underline,
        strike: st.strike || extra.strike,
        fg: extra.fg.or(st.fg),
        link: extra.link.clone().or_else(|| st.link.clone()),
    }
}

/// The `close` bracket matching the `open` at `at`, skipping escapes and code
/// spans.
fn matching(s: &[char], at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut j = at;
    while j < s.len() {
        match s[j] {
            '\\' => j += 1,
            '`' => j = skip_code(s, j),
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// The index of the last character of the code span that starts at `j`, or of
/// the opening run when it never closes.
fn skip_code(s: &[char], j: usize) -> usize {
    let n = s[j..].iter().take_while(|c| **c == '`').count();
    let mut k = j + n;
    while k < s.len() {
        let run = s[k..].iter().take_while(|c| **c == '`').count();
        if run == 0 {
            k += 1;
        } else if run == n {
            return k + run - 1;
        } else {
            k += run;
        }
    }
    j + n - 1
}

/// Where the closing run of `n` of `c` starts, from `from`: its last `n`
/// characters when the run is longer, after a character that is not a space,
/// and for `_` before one that is not a letter.
fn closer(s: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    let mut j = from;
    while j < s.len() {
        if s[j] == '\\' {
            j += 2;
        } else if s[j] == '`' {
            j = skip_code(s, j) + 1;
        } else if s[j] == c {
            let run = s[j..].iter().take_while(|x| **x == c).count();
            let last = j + run - n;
            let prev_ok = last > from && !s[last - 1].is_whitespace();
            let next_ok = c != '_' || s.get(j + run).is_none_or(|x| !x.is_alphanumeric());
            let pairs = run == n || (run == 3 && n < 3);
            if pairs && prev_ok && next_ok {
                return Some(last);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// Spans broken into lines of at most `width` columns, at spaces. Runs of
/// spaces are one space; a `\n` ends the line; a word wider than a line is
/// cut at the width.
pub fn wrap(spans: &[Span], width: usize) -> Vec<StyledLine> {
    let mut w = Wrapper::new(width.max(1));
    for sp in spans {
        w.span(sp);
    }
    w.finish()
}

struct Wrapper {
    width: usize,
    lines: Vec<StyledLine>,
    cur: StyledLine,
    cur_w: usize,
    word: Vec<(String, Style)>,
    /// The style of the space waiting between two words.
    space: Option<Style>,
    /// Whether the current line started with a word (a leading space is
    /// dropped).
    any: bool,
}

impl Wrapper {
    fn new(width: usize) -> Self {
        Self {
            width,
            lines: Vec::new(),
            cur: StyledLine::default(),
            cur_w: 0,
            word: Vec::new(),
            space: None,
            any: false,
        }
    }

    fn span(&mut self, sp: &Span) {
        let mut run = String::new();
        let mut spacing: Option<bool> = None;
        for ch in sp.text.chars() {
            let is_space = ch.is_whitespace();
            if spacing.is_some_and(|s| s != is_space) {
                self.run(&mut run, spacing == Some(true), &sp.style);
            }
            spacing = Some(is_space);
            run.push(ch);
        }
        if let Some(s) = spacing {
            self.run(&mut run, s, &sp.style);
        }
    }

    fn run(&mut self, run: &mut String, space: bool, style: &Style) {
        let text = std::mem::take(run);
        if !space {
            self.word.push((text, style.clone()));
        } else if text.contains('\n') {
            self.flush_word();
            self.newline();
            self.any = false;
        } else {
            self.flush_word();
            self.space = Some(style.clone());
        }
    }

    fn newline(&mut self) {
        self.lines.push(std::mem::take(&mut self.cur));
        self.cur_w = 0;
        self.space = None;
    }

    fn flush_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let word = std::mem::take(&mut self.word);
        let ww: usize = word
            .iter()
            .map(|(t, _)| super::super::style::text_width(t))
            .sum();
        if self.cur_w > 0 && self.cur_w + 1 + ww <= self.width {
            let sp = self.space.take().unwrap_or_default();
            self.cur.push(" ", &sp);
            self.cur_w += 1;
        } else if self.cur_w > 0 {
            self.newline();
        }
        self.space = None;
        self.any = true;
        for (text, style) in &word {
            self.place(text, style);
        }
    }

    /// Characters of a word, starting a new line where the width runs out.
    fn place(&mut self, text: &str, style: &Style) {
        let mut piece = String::new();
        for ch in text.chars() {
            let cw = char_width(ch);
            if self.cur_w + cw > self.width && self.cur_w > 0 {
                self.cur.push(&std::mem::take(&mut piece), style);
                self.newline();
            }
            piece.push(ch);
            self.cur_w += cw;
        }
        self.cur.push(&piece, style);
    }

    fn finish(mut self) -> Vec<StyledLine> {
        self.flush_word();
        if self.cur_w > 0 || self.any || self.lines.is_empty() {
            self.lines.push(self.cur);
        }
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(src: &str) -> String {
        inline(src, &Style::default(), Mode::Plain)
            .iter()
            .map(|s| s.text.as_str())
            .collect()
    }

    fn ansi(src: &str) -> String {
        let spans = inline(src, &Style::default(), Mode::Ansi);
        StyledLine::new(spans).ansi()
    }

    #[test]
    fn emphasis_code_and_strike_are_styled_and_their_marks_go() {
        assert_eq!(
            ansi("a **b** *c* ***d*** ~~e~~ `f`"),
            "a \x1b[1mb\x1b[0m \x1b[3mc\x1b[0m \x1b[1;3md\x1b[0m \x1b[9me\x1b[0m \x1b[36mf\x1b[0m"
        );
        assert_eq!(
            ansi("**a *b* c**"),
            "\x1b[1ma \x1b[0m\x1b[1;3mb\x1b[0m\x1b[1m c\x1b[0m"
        );
        assert_eq!(plain("a **b** `f`"), "a b `f`");
        assert_eq!(plain("`` a`b ``"), "`a`b`");
    }

    #[test]
    fn marks_that_do_not_pair_are_text() {
        assert_eq!(
            plain("snake_case_name and 2 * 3 * 4"),
            "snake_case_name and 2 * 3 * 4"
        );
        assert_eq!(plain("**open"), "**open");
        assert_eq!(plain("a * b"), "a * b");
        assert_eq!(plain(r"\*lit\* \\ \q"), r"*lit* \ \q");
        assert_eq!(plain("`unclosed"), "`unclosed");
        assert_eq!(plain("_a_ b"), "a b");
    }

    #[test]
    fn a_link_is_a_target_on_a_terminal_and_an_address_after_its_text_otherwise() {
        assert_eq!(
            ansi("see [the docs](https://x.example/a_b) now"),
            "see \x1b]8;;https://x.example/a_b\x1b\\\x1b[4;34mthe docs\x1b[0m\x1b]8;;\x1b\\ now"
        );
        assert_eq!(
            plain("see [the docs](https://x.example/a_b) now"),
            "see the docs (https://x.example/a_b) now"
        );
        assert_eq!(
            plain("[https://x.example](https://x.example)"),
            "https://x.example"
        );
        assert_eq!(plain("<https://x.example/p>."), "https://x.example/p.");
        assert_eq!(
            plain("go to https://x.example/p, then"),
            "go to https://x.example/p, then"
        );
        assert!(
            ansi("go to https://x.example/p, then").contains("\x1b]8;;https://x.example/p\x1b\\")
        );
        assert_eq!(plain("![a cat](c.png)"), "[image: a cat] (c.png)");
        assert_eq!(plain("[no](dest"), "[no](dest");
        assert_eq!(plain("[a [b](u)](v)"), "a b (u) (v)");
    }

    fn lines(src: &str, width: usize) -> Vec<String> {
        wrap(&inline(src, &Style::default(), Mode::Plain), width)
            .iter()
            .map(StyledLine::plain)
            .collect()
    }

    #[test]
    fn words_wrap_at_spaces_and_only_a_long_one_is_cut() {
        assert_eq!(lines("aaa bbb ccc ddd", 7), ["aaa bbb", "ccc ddd"]);
        assert_eq!(lines("aaa  bbb   ccc", 20), ["aaa bbb ccc"]);
        assert_eq!(lines("abcdefghij kl", 4), ["abcd", "efgh", "ij", "kl"]);
        assert_eq!(lines("  lead and trail  ", 40), ["lead and trail"]);
        assert_eq!(lines("one\ntwo", 40), ["one", "two"]);
        assert_eq!(lines("日本語 日本語", 6), ["日本語", "日本語"]);
        assert_eq!(lines("", 10), [""]);
    }

    #[test]
    fn a_styled_word_keeps_its_style_across_a_break() {
        let spans = inline("x **bold words here** y", &Style::default(), Mode::Ansi);
        let l = wrap(&spans, 10);
        assert_eq!(l[0].ansi(), "x \x1b[1mbold\x1b[0m");
        assert_eq!(l[1].ansi(), "\x1b[1mwords here\x1b[0m");
        assert_eq!(l[2].ansi(), "y");
    }
}
