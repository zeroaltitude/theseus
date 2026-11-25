//! A cut of text in the core written in one of the recognised spellings below
//! goes through `scrub::safe`, which stops at a known value's edge, or says
//! why a plain cut is right there. The scan walks `src/`, leaving out
//! `scrub/` (the helpers' home), test files and inline test modules, and
//! reads each line's code (its string and character literals blanked, its
//! comments left out) for the spellings that exist only on text. A found
//! line passes when it, or the line above it, carries
//! `// plain cut: <reason>`.
//!
//! It is a check of spellings, not of meaning. Not seen:
//! - an iterator bound to a name and cut on a later statement
//!   (`let cs = s.chars(); cs.take(n)`);
//! - a cut after another adapter (`.chars().filter(..).take(n)`), or a
//!   second cut on a line whose first is marked: one marker passes the line;
//! - `truncate`, `pop`, `split_off`, `drain`, a range slice (`&s[..n]`) or a
//!   format precision (`{:.8}`) on text, unless a boundary step above is
//!   spelled out;
//! - a helper given `Cuts::none()`, which cuts as plainly as `take` does;
//! - helpers in other crates but the three listed, and a bare imported name;
//! - a line of a string literal that runs over several lines, read as code.
//!
//! Read wrongly: `.get(..` on a slice that is not text, and
//! `is_char_boundary` that checks an offset and cuts nothing, are listed;
//! such a line is marked with its reason. Files named `tests*` or
//! `*_tests.rs`, and the text between a column-0 `#[cfg(test)]` + `mod x {`
//! and the next column-0 `}`, are skipped by their shape, not by what
//! compiles.

use std::path::{Path, PathBuf};

/// Whether a line with a plain cut and no marker fails the scan. Every site
/// is moved or marked, so it does: the scan prints the unmarked lines and
/// their count, and fails on any of them as on a marker with no reason.
const UNMARKED_FAILS: bool = true;

/// The ways of shortening that exist only on text: a byte cut of text needs a
/// character-boundary step, which is here too.
const IDIOMS: [&str; 10] = [
    "chars().take(",
    "chars().skip(",
    "chars().rev().take(",
    "char_indices().nth(",
    "is_char_boundary(",
    "floor_char_boundary",
    ".get(..",
    "cut_marked(",
    "words::cut(",
    "words::clip(",
];

const MARKER: &str = "// plain cut:";

/// What a scan of one file found: lines with a plain cut and no marker, and
/// markers with no reason, each as `line: text`.
#[derive(Default)]
struct Found {
    unmarked: Vec<(usize, String)>,
    no_reason: Vec<(usize, String)>,
}

/// The lines of `code` that are not in an inline test module: one opened by a
/// `#[cfg(test)]` at the start of a line, then `mod <name> {`, up to its
/// closing `}` at the start of a line.
fn live_lines(code: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = code.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i] == "#[cfg(test)]" {
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with("#[") {
                j += 1;
            }
            if j < lines.len() && lines[j].starts_with("mod ") && lines[j].ends_with('{') {
                while j < lines.len() && lines[j] != "}" {
                    j += 1;
                }
                i = j + 1;
                continue;
            }
        }
        out.push((i + 1, lines[i]));
        i += 1;
    }
    out
}

/// Whether `line` carries the marker; `Some(false)` when its reason is empty.
fn marker(line: &str) -> Option<bool> {
    line.find(MARKER)
        .map(|at| !line[at + MARKER.len()..].trim().is_empty())
}

/// `line`'s code: the text of its string and character literals blanked out,
/// and its comments (`//` to the end, `/* .. */` within the line) left out.
fn code_of(line: &str) -> String {
    let c: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < c.len() {
        let next = c.get(i + 1).copied();
        match c[i] {
            '/' if next == Some('/') => break,
            '/' if next == Some('*') => {
                i += 2;
                while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
                out.push(' ');
                continue;
            }
            '"' => {
                i += 1;
                while i < c.len() && c[i] != '"' {
                    i += if c[i] == '\\' { 2 } else { 1 };
                }
                out.push_str("\"\"");
            }
            'r' if matches!(next, Some('"' | '#'))
                && !out.ends_with(|p: char| p.is_alphanumeric() || p == '_') =>
            {
                let mut j = i + 1;
                while c.get(j) == Some(&'#') {
                    j += 1;
                }
                if c.get(j) != Some(&'"') {
                    out.push('r');
                    i += 1;
                    continue;
                }
                let close: Vec<char> = std::iter::once('"')
                    .chain(c[i + 1..j].iter().copied())
                    .collect();
                j += 1;
                while j < c.len() && !c[j..].starts_with(&close) {
                    j += 1;
                }
                i = j + close.len();
                out.push_str("\"\"");
                continue;
            }
            '\'' => {
                // A character literal (`'x'`, `'\n'`, `'\u{2026}'`); a
                // lifetime has no closing quote.
                let end = if next == Some('\\') {
                    c.get(i + 2..)
                        .and_then(|r| r.iter().position(|&d| d == '\''))
                        .map(|p| i + 2 + p)
                } else {
                    (c.get(i + 2) == Some(&'\'')).then_some(i + 2)
                };
                match end {
                    Some(e) => {
                        out.push_str("' '");
                        i = e + 1;
                        continue;
                    }
                    None => out.push('\''),
                }
            }
            ch => out.push(ch),
        }
        i += 1;
    }
    out
}

/// The code of the chain that line `k`, a line starting with `.`, goes on:
/// the lines before it back to the chain's first, comment lines left out and
/// each line's own comments and literals too, joined.
fn chain_before(lines: &[(usize, &str)], k: usize) -> String {
    let mut parts = Vec::new();
    for (_, l) in lines[..k].iter().rev() {
        let t = l.trim();
        if t.starts_with("//") {
            continue;
        }
        let code = code_of(t);
        let code = code.trim_end().to_string();
        let link = code.starts_with('.');
        parts.push(code);
        if !link {
            break;
        }
    }
    parts.reverse();
    parts.concat()
}

fn scan(code: &str) -> Found {
    let mut found = Found::default();
    let lines = live_lines(code);
    for (k, (n, line)) in lines.iter().enumerate() {
        if marker(line) == Some(false) {
            found.no_reason.push((*n, line.trim().to_string()));
        }
        let text = line.trim_start();
        if text.starts_with("//") {
            continue;
        }
        // A chain rustfmt broke over lines, comment lines among its links:
        // the line found is the one where an idiom ends, its cutting call.
        let code = code_of(text);
        let before = if code.starts_with('.') {
            chain_before(&lines, k)
        } else {
            String::new()
        };
        let joined = format!("{before}{code}");
        let ends_here = |i: &&str| {
            joined
                .match_indices(*i)
                .any(|(at, _)| at + i.len() > before.len())
        };
        if !IDIOMS.iter().any(ends_here) {
            continue;
        }
        let above = k.checked_sub(1).map(|p| lines[p].1);
        let marked = marker(line).is_some() || above.and_then(marker).is_some();
        if !marked {
            found.unmarked.push((*n, line.trim().to_string()));
        }
    }
    found
}

/// The core's source files the scan reads, relative to `src/`, with their
/// text.
fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut todo: Vec<PathBuf> = vec![root.clone()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).expect("read the crate's source") {
            let p = e.expect("a source entry").path();
            let rel = p.strip_prefix(&root).unwrap().to_path_buf();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let test_file = name.starts_with("tests") || name.ends_with("_tests.rs");
            if rel == Path::new("scrub") || test_file {
                continue;
            }
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).expect("read a source file");
                out.push((rel.display().to_string(), text));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_plain_cut_of_text_in_core_says_why() {
    let files = sources();
    assert!(files.len() > 100, "the scan read {} files", files.len());
    let (mut unmarked, mut no_reason) = (Vec::new(), Vec::new());
    for (file, text) in &files {
        let found = scan(text);
        unmarked.extend(
            found
                .unmarked
                .into_iter()
                .map(|(n, l)| format!("{file}:{n}: {l}")),
        );
        no_reason.extend(
            found
                .no_reason
                .into_iter()
                .map(|(n, l)| format!("{file}:{n}: {l}")),
        );
    }
    println!("plain cuts with no reason given: {}", unmarked.len());
    for line in &unmarked {
        println!("  {line}");
    }
    assert!(
        no_reason.is_empty(),
        "a `{MARKER}` marker gives no reason:\n{}",
        no_reason.join("\n")
    );
    assert!(
        !UNMARKED_FAILS || unmarked.is_empty(),
        "cut through `scrub::safe`, or say why a plain cut is right with `{MARKER} <reason>`:\n{}",
        unmarked.join("\n")
    );
}

#[test]
fn the_scan_finds_a_plain_cut_and_reads_its_marker() {
    let code = "\
fn a(s: &str) -> String {
    s.chars().take(5).collect()
}
fn b(s: &str) -> &str {
    // plain cut: an id, never a value
    &s[..s.char_indices().nth(4).unwrap().0]
}
fn c(s: &str) -> String {
    s.chars()
        .skip(3) // plain cut: a local display
        .collect()
}
fn d(s: &str) -> String {
    words::clip(s, 9) // plain cut:
}
fn e(s: &str) -> String {
    s.chars()
        .rev()
        .collect()
}
// s.chars().take(1) in a comment is not code
#[cfg(test)]
mod tests {
    fn f(s: &str) -> String {
        s.chars().take(5).collect()
    }
}
fn g(s: &str) -> Option<&str> {
    s.get(..4)
}
fn h(s: &str) -> String {
    s.chars()
        .rev()
        .take(8)
        .collect::<String>()
}
fn i(s: &str) -> String {
    s.chars() // each one
        // the first eight
        .take(8)
        .collect()
}
fn j(s: &str) -> String {
    s.chars()
        .rev() // plain cut: a local display
        .take(8)
        .collect::<String>()
}
fn k(s: &str) -> String {
    s.chars() // each one
        // plain cut: a local display
        .take(8)
        .collect()
}
fn l(s: &str) -> usize {
    let m = \"use s.chars().take(4)\";
    let n = 1; // s.chars().take(2)
    let k = /* s.chars().take(3) */ 2;
    m.len() + n + k + s.len()
}
fn m(v: &[u8]) -> Option<&[u8]> {
    v.get(..2)
}
fn n(s: &str) -> String {
    let q = '\"';
    s.chars().take(1).chain(std::iter::once(q)).collect()
}
fn o(s: &str) -> &str {
    let r = r#\"s.chars().take(9)\"#;
    &s[..r.len().min(s.len())]
}
";
    let found = scan(code);
    let lines = |v: &[(usize, String)]| v.iter().map(|(n, _)| *n).collect::<Vec<_>>();
    // `l` and `o` hold the spellings only in literals and comments; `m`'s
    // `.get(..` on bytes is listed, as the module says; `n`'s character
    // literal `'"'` opens no string.
    assert_eq!(lines(&found.unmarked), vec![2, 29, 34, 40, 62, 66]);
    assert_eq!(lines(&found.no_reason), vec![14]);
}
