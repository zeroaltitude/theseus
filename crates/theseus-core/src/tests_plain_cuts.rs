//! Every cut of text in the core goes through `scrub::safe`, which stops at a
//! known value's edge, or says why a plain cut is right there. The scan walks
//! `src/`, leaving out `scrub/` (the helpers' home), test files and inline
//! test modules, and finds the idioms that exist only on text. A found line
//! passes when it, or the line above it, carries `// plain cut: <reason>`.

use std::path::{Path, PathBuf};

/// Whether a line with a plain cut and no marker fails the scan. Until every
/// site is moved or marked, the scan prints the unmarked lines and their
/// count, and fails only on a marker with no reason.
const UNMARKED_FAILS: bool = false;

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

/// The code of the chain that line `k`, a line starting with `.`, goes on:
/// the lines before it back to the chain's first, comment lines left out and
/// each line's own trailing comment too, joined.
fn chain_before(lines: &[(usize, &str)], k: usize) -> String {
    let mut parts = Vec::new();
    for (_, l) in lines[..k].iter().rev() {
        let t = l.trim();
        if t.starts_with("//") {
            continue;
        }
        parts.push(t.split(" //").next().unwrap_or(t).trim_end());
        if !t.starts_with('.') {
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
        let before = if text.starts_with('.') {
            chain_before(&lines, k)
        } else {
            String::new()
        };
        let joined = format!("{before}{text}");
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
";
    let found = scan(code);
    let lines = |v: &[(usize, String)]| v.iter().map(|(n, _)| *n).collect::<Vec<_>>();
    assert_eq!(lines(&found.unmarked), vec![2, 29, 34, 40]);
    assert_eq!(lines(&found.no_reason), vec![14]);
}
