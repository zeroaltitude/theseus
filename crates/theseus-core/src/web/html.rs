//! HTML to readable text (DD5): headings as `#` lines, paragraphs, list
//! items, links as `text (url)`, and `pre` kept as it is; script, style,
//! noscript, svg, and a page's controls dropped; entities decoded.
//!
//! Hand-written, with no parser crate: it reads tags one at a time and keeps
//! a little state (a `pre`, a list, a link), never a tree, so markup that is
//! broken or cut off by the byte cap still gives its text.

use reqwest::Url;

/// A page's text, and its title.
#[derive(Debug, Default, PartialEq)]
pub struct Page {
    pub title: Option<String>,
    pub text: String,
}

/// Elements whose content is dropped: scripts and styles, what a browser
/// shows only without scripts, drawings, and a page's controls.
const DROPPED: &[&str] = &[
    "script", "style", "noscript", "svg", "template", "button", "select", "iframe", "object",
    "canvas", "video", "audio",
];

/// Elements that end a line and leave a blank one.
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "main",
    "aside",
    "table",
    "blockquote",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "details",
    "summary",
    "dl",
    "address",
    "hr",
];

/// Elements that end a line.
const LINES: &[&str] = &["br", "tr", "dt", "dd", "caption", "thead", "tbody", "tfoot"];

/// The text of `html`. Relative links resolve against `base`, a page's
/// final URL, or its `<base href>`.
pub fn to_text(html: &str, base: Option<&Url>) -> Page {
    let mut w = Writer {
        base: base.cloned(),
        ..Writer::default()
    };
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            w.text(&decode(rest));
            break;
        };
        if lt > 0 {
            w.text(&decode(&rest[..lt]));
        }
        rest = w.tag(&rest[lt..]);
    }
    w.flush();
    Page {
        title: w.title,
        text: w.out,
    }
}

#[derive(Default)]
struct Writer {
    out: String,
    /// The line being written.
    line: String,
    /// What starts the line once it has a word: a heading's `#`s, a list
    /// item's marker.
    prefix: String,
    /// A space is owed before the next word.
    space: bool,
    /// A blank line is owed before the next line.
    blank: bool,
    /// Inside this many `pre`s: text is kept as it is.
    pre: u32,
    /// Each open list: `None` for `ul`, the next number for `ol`.
    lists: Vec<Option<u32>>,
    /// The open link: its URL, and its text so far.
    link: Option<(Option<String>, String)>,
    base: Option<Url>,
    title: Option<String>,
}

impl Writer {
    fn text(&mut self, s: &str) {
        if self.pre > 0 {
            self.line.push_str(s);
            if let Some((_, t)) = &mut self.link {
                t.push_str(s);
            }
            return;
        }
        let mut rest = s;
        loop {
            let word = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if word.len() != rest.len() {
                self.space = true;
            }
            if word.is_empty() {
                break;
            }
            let end = word
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(word.len());
            if self.line.is_empty() {
                self.line = std::mem::take(&mut self.prefix);
            }
            if self.space && !self.line.is_empty() && !self.line.ends_with(' ') {
                self.line.push(' ');
            }
            if let Some((_, t)) = &mut self.link {
                if self.space && !t.is_empty() {
                    t.push(' ');
                }
                t.push_str(&word[..end]);
            }
            self.space = false;
            self.line.push_str(&word[..end]);
            rest = &word[end..];
        }
    }

    /// End the line.
    fn flush(&mut self) {
        let l = self.line.trim_end();
        if !l.is_empty() {
            if self.blank && !self.out.is_empty() {
                self.out.push('\n');
            }
            self.out.push_str(l);
            self.out.push('\n');
            self.blank = false;
        }
        self.line.clear();
        self.prefix.clear();
        self.space = false;
    }

    /// End the line, and owe a blank one.
    fn block(&mut self) {
        self.flush();
        self.blank = true;
    }

    /// Read the tag at the start of `s` and act on it; return what follows it.
    fn tag<'a>(&mut self, s: &'a str) -> &'a str {
        if let Some(body) = s.strip_prefix("<!--") {
            return body.find("-->").map_or("", |e| &body[e + 3..]);
        }
        let b = s.as_bytes();
        let closing = b.get(1) == Some(&b'/');
        let at = if closing { 2 } else { 1 };
        match b.get(at) {
            Some(c) if c.is_ascii_alphabetic() => {}
            // `<!doctype …>`, `<?xml …?>`, `</ >`: markup with no text.
            Some(b'!' | b'?') => return s.find('>').map_or("", |e| &s[e + 1..]),
            // A `<` that starts no tag is text.
            _ => {
                self.text("<");
                return &s[1..];
            }
        }
        let name_end = s[at..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':'))
            .map_or(s.len(), |e| e + at);
        let name = s[at..name_end].to_ascii_lowercase();
        let (attrs, after) = match tag_end(&s[name_end..]) {
            Some(e) => (&s[name_end..name_end + e], &s[name_end + e + 1..]),
            None => (&s[name_end..], ""),
        };
        if closing {
            self.close(&name);
            return after;
        }
        if DROPPED.contains(&name.as_str()) {
            // `<svg … />` closes itself: nothing to skip.
            if attrs.trim_end().ends_with('/') {
                return after;
            }
            return skip_to_end(after, &name);
        }
        match name.as_str() {
            "title" => {
                let (inner, rest) = raw_until(after, "title");
                if self.title.is_none() {
                    let t = collapse(&decode(inner));
                    self.title = (!t.is_empty()).then_some(t);
                }
                return rest;
            }
            "textarea" => {
                let (inner, rest) = raw_until(after, "textarea");
                self.text(&decode(inner));
                return rest;
            }
            "base" => {
                if let Some(href) = attr(attrs, "href") {
                    self.base = match &self.base {
                        Some(b) => b.join(&href).ok(),
                        None => Url::parse(&href).ok(),
                    };
                }
            }
            "a" => {
                self.end_link();
                let url = attr(attrs, "href").and_then(|h| self.link_url(&h));
                self.link = Some((url, String::new()));
            }
            "pre" => {
                self.block();
                self.pre += 1;
                // A newline just inside `<pre>` is not content.
                if let Some(r) = after
                    .strip_prefix("\r\n")
                    .or_else(|| after.strip_prefix('\n'))
                {
                    return r;
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.block();
                let level = name.as_bytes()[1] - b'0';
                self.prefix = format!("{} ", "#".repeat(level.into()));
            }
            "ul" | "ol" => {
                self.block();
                let start = attr(attrs, "start").and_then(|s| s.parse().ok());
                self.lists.push((name == "ol").then(|| start.unwrap_or(1)));
            }
            "li" => {
                self.flush();
                let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => "- ".into(),
                };
                self.prefix = format!("{indent}{marker}");
            }
            "td" | "th" => {
                if !self.line.trim().is_empty() {
                    self.line.push_str(" | ");
                }
            }
            "img" => {
                if let Some(alt) = attr(attrs, "alt").filter(|a| !a.trim().is_empty()) {
                    self.text(&format!(" [{}] ", alt.trim()));
                }
            }
            n if n == "br" && self.pre > 0 => self.line.push('\n'),
            n if BLOCKS.contains(&n) => self.block(),
            n if LINES.contains(&n) => self.flush(),
            _ => {}
        }
        after
    }

    fn close(&mut self, name: &str) {
        match name {
            "a" => self.end_link(),
            "pre" => {
                self.flush();
                self.pre = self.pre.saturating_sub(1);
                self.blank = true;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.block(),
            "ul" | "ol" => {
                self.lists.pop();
                self.block();
            }
            "li" => self.flush(),
            n if BLOCKS.contains(&n) => self.block(),
            n if LINES.contains(&n) => self.flush(),
            _ => {}
        }
    }

    /// A link's URL, absolute, when it is worth showing: not an anchor on
    /// this page, and not a script.
    fn link_url(&self, href: &str) -> Option<String> {
        let href = href.trim();
        let lower = href.to_ascii_lowercase();
        if href.is_empty()
            || href.starts_with('#')
            || lower.starts_with("javascript:")
            || lower.starts_with("data:")
        {
            return None;
        }
        Some(match &self.base {
            Some(b) => b.join(href).map_or_else(|_| href.to_string(), String::from),
            None => href.to_string(),
        })
    }

    /// Close the open link: its URL follows its text, unless the text says it.
    fn end_link(&mut self) {
        let Some((Some(url), text)) = self.link.take() else {
            return;
        };
        let text = text.trim();
        if text.is_empty() || text == url || self.pre > 0 {
            return;
        }
        self.line.push_str(&format!(" ({url})"));
        self.space = false;
    }
}

/// Where a tag's `>` is, outside a quoted attribute value.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// What follows the end tag `</name>`, skipping everything before it.
fn skip_to_end<'a>(s: &'a str, name: &str) -> &'a str {
    raw_until(s, name).1
}

/// The text before the end tag `</name>` (any case), and what follows it.
fn raw_until<'a>(s: &'a str, name: &str) -> (&'a str, &'a str) {
    let close = format!("</{name}");
    let mut from = 0;
    while let Some(i) = s[from..].find("</") {
        let at = from + i;
        let end = at + close.len();
        if s.len() >= end && s[at..end].eq_ignore_ascii_case(&close) {
            let rest = &s[end..];
            return (&s[..at], rest.find('>').map_or("", |e| &rest[e + 1..]));
        }
        from = at + 2;
    }
    (s, "")
}

/// An attribute's value, entities decoded.
fn attr(attrs: &str, want: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .find(|c: char| c.is_ascii_whitespace() || c == '=' || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..end];
        rest = rest[end..].trim_start();
        let value = if let Some(v) = rest.strip_prefix('=') {
            let v = v.trim_start();
            let (value, next) = match v.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let body = &v[1..];
                    let e = body.find(q).unwrap_or(body.len());
                    (&body[..e], body.get(e + 1..).unwrap_or(""))
                }
                _ => {
                    let e = v.find(|c: char| c.is_ascii_whitespace()).unwrap_or(v.len());
                    (&v[..e], &v[e..])
                }
            };
            rest = next;
            Some(value)
        } else {
            None
        };
        if name.eq_ignore_ascii_case(want) {
            return value.map(|v| decode(v).into_owned());
        }
    }
}

/// `s` with each run of whitespace one space, trimmed.
fn collapse(s: &str) -> String {
    s.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text with its character references decoded: `&amp;`, `&#39;`, `&#x27;`,
/// and the named ones pages use. One it does not know stays as it is.
pub fn decode(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('&') {
        return s.into();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map_or(rest.len(), |e| e + 1);
        let (name, semi) = (&rest[1..end], rest[end..].starts_with(';'));
        match semi.then(|| char_ref(name)).flatten() {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.into()
}

fn char_ref(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let n = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(n).filter(|c| *c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        // A non-breaking space is a space in text.
        "nbsp" | "ensp" | "emsp" | "thinsp" => ' ',
        "shy" | "zwnj" | "zwj" | "lrm" | "rlm" => '\u{200b}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "minus" => '−',
        "lsquo" => '‘',
        "rsquo" => '’',
        "sbquo" => '‚',
        "ldquo" => '“',
        "rdquo" => '”',
        "bdquo" => '„',
        "laquo" => '«',
        "raquo" => '»',
        "lsaquo" => '‹',
        "rsaquo" => '›',
        "bull" => '•',
        "middot" => '·',
        "times" => '×',
        "divide" => '÷',
        "deg" => '°',
        "plusmn" => '±',
        "para" => '¶',
        "sect" => '§',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "rArr" => '⇒',
        "lArr" => '⇐',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        "asymp" => '≈',
        "infin" => '∞',
        "dagger" => '†',
        "Dagger" => '‡',
        "prime" => '′',
        "Prime" => '″',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup2" => '²',
        "sup3" => '³',
        "micro" => 'µ',
        "iexcl" => '¡',
        "iquest" => '¿',
        "check" => '✓',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(html: &str) -> String {
        to_text(
            html,
            Url::parse("https://example.com/docs/page.html")
                .ok()
                .as_ref(),
        )
        .text
    }

    #[test]
    fn headings_paragraphs_lists_and_links_become_lines() {
        let html = r##"<!DOCTYPE html><html><head><title>The &amp; Page</title>
            <meta charset="utf-8"><style>body { color: red }</style></head>
            <body><h1>Main  heading</h1><p>First <b>para</b>graph,
            with a <a href="other.html">relative link</a> and an
            <a href="#top">anchor</a>.</p>
            <ul><li>one</li><li>two <a href="https://rust-lang.org/">Rust</a>
              <ol><li>inner</li><li>again</li></ol></li></ul>
            <h3>Small</h3><p>Last&nbsp;one &lt;b&gt; &#39;q&#x27; &mdash; &bogus; done</p></body></html>"##;
        let p = to_text(
            html,
            Url::parse("https://example.com/docs/page.html")
                .ok()
                .as_ref(),
        );
        assert_eq!(p.title.as_deref(), Some("The & Page"));
        assert_eq!(
            p.text,
            "# Main heading\n\n\
             First paragraph, with a relative link (https://example.com/docs/other.html) and an anchor.\n\n\
             - one\n\
             - two Rust (https://rust-lang.org/)\n\n  \
             1. inner\n  \
             2. again\n\n\
             ### Small\n\n\
             Last one <b> 'q' — &bogus; done\n"
        );
    }

    #[test]
    fn scripts_styles_and_drawings_vanish_and_pre_stays_as_it_is() {
        let html = "<p>before</p><script>if (a < b) { document.write('</div><p>not text</p>') }</script>\
            <noscript>Enable JavaScript</noscript><svg><text>drawn</text></svg>\
            <pre>\nfn main() {\n    let x = 1 &lt; 2;   // <a href=\"x\">kept</a>\n}\n</pre><p>after</p>\
            <SCRIPT type=\"text/javascript\">var s = \"</p>\";</SCRIPT><button>Copy</button>";
        assert_eq!(
            text(html),
            "before\n\nfn main() {\n    let x = 1 < 2;   // kept\n}\n\nafter\n"
        );
    }

    #[test]
    fn broken_or_cut_markup_still_gives_its_text() {
        assert_eq!(text("<p>a < b and c > d</p>"), "a < b and c > d\n");
        assert_eq!(text("<p>cut off <a href=\"/x\">link"), "cut off link\n");
        assert_eq!(text("<p>open comment <!-- never closed"), "open comment\n");
        assert_eq!(text("<div>unclosed <span>tags"), "unclosed tags\n");
        assert_eq!(text("<td>a</td><td>b</td>"), "a | b\n");
        assert_eq!(text("plain text, no tags"), "plain text, no tags\n");
        assert_eq!(
            text("<a href='https://x.io/' title=\"a > b\">https://x.io/</a>"),
            "https://x.io/\n"
        );
    }

    #[test]
    fn entities_decode_and_unknown_ones_stay() {
        assert_eq!(decode("a &amp;&amp; b"), "a && b");
        assert_eq!(
            decode("&#128512; &#x1F600; &#0; &#xZZ;"),
            "😀 😀 &#0; &#xZZ;"
        );
        assert_eq!(decode("AT&T &copy 2026"), "AT&T &copy 2026");
        assert_eq!(decode("no refs"), "no refs");
    }
}
