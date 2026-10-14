//! A sealed item's check: the exam's check language (`theseus-exam`'s
//! `check.rs`), the part that reads a reply. A check is lines; every line
//! must hold. A line is clauses joined by ` or `, and holds when any does:
//!
//! ```text
//! reply has "7433"            the reply contains it
//! reply lacks "fjall" | "sled"  it contains none of them
//! reply has word "redb"       a whole word: no letter or digit beside it
//! ```
//!
//! Strings compare case-insensitively after typographic quotes and dashes
//! fold to ASCII and whitespace runs collapse to one space (the exam's own
//! `normalize`). The exam's other subjects (`calls`, `file`) and regexes need
//! a run's tool calls and workspace, which a scorer that sees a reply alone
//! has not got, so a line that uses one is refused at `put`, never scored
//! wrong. A parse error names the line's number and what was wrong, never the
//! line: an item's text does not leave through an error.

use anyhow::{bail, Result};

/// Folded, lower-cased, whitespace collapsed: what patterns compare.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        let c = match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => '"',
            '\u{2010}'..='\u{2014}' | '\u{2212}' => '-',
            '\u{00A0}' | '\u{202F}' => ' ',
            c => c,
        };
        if c.is_whitespace() {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.extend(c.to_lowercase());
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verb {
    Has,
    Lacks,
}

#[derive(Debug, Clone)]
struct Clause {
    verb: Verb,
    word: bool,
    /// Normalized.
    patterns: Vec<String>,
}

/// A parsed check.
#[derive(Debug, Clone)]
pub struct Check {
    lines: Vec<Vec<Clause>>,
}

impl Check {
    pub fn parse(text: &str) -> Result<Self> {
        let mut lines = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let n = i + 1;
            let mut clauses = Vec::new();
            for part in split_or(line) {
                clauses.push(
                    clause(part.trim()).map_err(|why| anyhow::anyhow!("check line {n}: {why}"))?,
                );
            }
            lines.push(clauses);
        }
        if lines.is_empty() {
            bail!("the check has no line");
        }
        Ok(Self { lines })
    }

    /// Every pattern the check holds, normalized: the filter's needles.
    pub fn patterns(&self) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .flatten()
            .flat_map(|c| c.patterns.iter().map(String::as_str))
    }

    /// Whether every line holds for `reply`.
    pub fn holds(&self, reply: &str) -> bool {
        let text = normalize(reply);
        self.lines.iter().all(|line| {
            line.iter().any(|c| {
                let found = |p: &String| {
                    if c.word {
                        has_word(&text, p)
                    } else {
                        text.contains(p.as_str())
                    }
                };
                match c.verb {
                    Verb::Has => c.patterns.iter().any(found),
                    Verb::Lacks => !c.patterns.iter().any(found),
                }
            })
        })
    }
}

/// `pat` in `text` with no letter or digit on either side.
fn has_word(text: &str, pat: &str) -> bool {
    if pat.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(at) = text[from..].find(pat) {
        let s = from + at;
        let e = s + pat.len();
        let before = text[..s]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after = text[e..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = s + text[s..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// ` or ` outside quotes.
fn split_or(line: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted, mut esc) = (Vec::new(), 0, false, false);
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            _ if esc => esc = false,
            b'\\' if quoted => esc = true,
            b'"' => quoted = !quoted,
            b' ' if !quoted && line[i..].starts_with(" or ") => {
                out.push(&line[start..i]);
                start = i + 4;
                i += 3;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&line[start..]);
    out
}

fn clause(s: &str) -> std::result::Result<Clause, &'static str> {
    let rest = s
        .strip_prefix("reply ")
        .ok_or("only `reply` lines are scored")?;
    let (verb, rest) = if let Some(r) = rest.strip_prefix("has ") {
        (Verb::Has, r)
    } else if let Some(r) = rest.strip_prefix("lacks ") {
        (Verb::Lacks, r)
    } else {
        return Err("expected `has` or `lacks` after `reply`");
    };
    let (word, rest) = match rest.trim_start().strip_prefix("word ") {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let mut patterns = Vec::new();
    let mut rest = rest.trim();
    loop {
        let (text, after) = quoted(rest)?;
        let norm = normalize(&text);
        if norm.is_empty() {
            return Err("a pattern is empty");
        }
        patterns.push(norm);
        let after = after.trim_start();
        if after.is_empty() {
            break;
        }
        rest = after
            .strip_prefix('|')
            .ok_or("expected `|` or the end of the line after a pattern")?
            .trim_start();
    }
    Ok(Clause {
        verb,
        word,
        patterns,
    })
}

/// A leading `"…"` string, its escapes read, and what follows it.
fn quoted(s: &str) -> std::result::Result<(String, &str), &'static str> {
    let inner = s
        .strip_prefix('"')
        .ok_or("a pattern must be a quoted string")?;
    let mut out = String::new();
    let mut chars = inner.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &inner[i + 1..])),
            '\\' => match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                Some((_, 'n')) => out.push('\n'),
                _ => return Err("an unknown escape in a pattern"),
            },
            c => out.push(c),
        }
    }
    Err("a pattern is not closed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_lacks_word_and_or_hold_as_the_exams_do() {
        let c = Check::parse(
            "# a comment\nreply has \"7433\"\nreply lacks \"fjall\" | \"sled\"\n\
             reply has word \"redb\" or reply has \"redb2\"\n",
        )
        .unwrap();
        assert!(c.holds("The port is 7433, stored in REDB."));
        assert!(
            !c.holds("The port is 7433, stored in redbx."),
            "not a whole word"
        );
        assert!(
            !c.holds("7433 redb and sled"),
            "lacks fails when any is found"
        );
        assert!(
            c.holds("7433 \u{201C}redb\u{201D}"),
            "typographic quotes fold"
        );
        assert!(c.holds("7433\n\n  redb"), "whitespace collapses");
    }

    #[test]
    fn a_line_the_scorer_cannot_score_is_refused_without_its_text() {
        for bad in [
            "calls has \"x\"",
            "reply has /re/",
            "reply has",
            "reply has \"open",
            "reply holds \"x\"",
            "file \"a\" exists",
        ] {
            let e = Check::parse(&format!("reply has \"ok\"\n{bad}")).unwrap_err();
            let e = format!("{e:#}");
            assert!(e.starts_with("check line 2: "), "{e}");
            assert!(!e.contains("calls") && !e.contains("/re/"), "{e}");
        }
        assert!(Check::parse("# only a comment\n").is_err());
    }
}
