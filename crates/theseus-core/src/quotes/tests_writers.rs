//! The writers of parts (issue 52): every place a stored record gets parts asks the edge check first
//! (`quotes::edges_hold`, `edges_hold_with`), so a range that begins or ends inside a known value is never stored.
//! The twin of the readers' guard (`quoted::tests::no_reader_reads_content_of_a_parts_result`): a new writer fails
//! here until it is listed with the file that checks what it writes.

use std::collections::{BTreeMap, BTreeSet};

use crate::quoted::tests::{product, walk};

/// Every place in the product's sources (outside a test module) that sets parts or caps them, each
/// `<file under crates/> <count> <the file that checks them> <why>`. A place sets parts when it writes a
/// `ToolResult`, `UserMessage` or `Piece` whose `parts` is not empty; it caps them when it calls the cap
/// (`cap_with`, `cap_parts`), which makes new edges. The checking file calls `edges_hold` or `edges_hold_with`
/// on what is written; where the check fails, the record keeps its words instead.
const WRITERS: &[&str] = &[
    "theseus-core/src/arrangement.rs 2 theseus-core/src/arrangement.rs a task's piece: a message's or a reply's \
     quote, capped and checked (`quoted`), stored by `resolve`",
    "theseus-core/src/quotes/cap.rs 1 theseus-core/src/toolrun/parts.rs `cap_parts`, the cap over the store: its \
     one caller, the runtime's `capped_parts`, checks",
    "theseus-core/src/toolrun.rs 1 theseus-core/src/toolrun/parts.rs a look back's result, its parts from \
     `capped_parts`, which checks them",
    "theseus-core/src/toolrun/parts.rs 1 theseus-core/src/toolrun/parts.rs a look back's parts, capped and \
     checked (`capped_parts`)",
];

/// The stored records that hold parts.
const HOLDERS: [&str; 3] = ["ToolResult", "UserMessage", "Piece"];

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `s` with its comments taken out and every string and character literal emptied (`""`, `' '`), so a scan of
/// it sees only code: a brace, a comma or a name inside one is not the code's.
fn code(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let (mut out, mut i) = (String::with_capacity(s.len()), 0);
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    while i < c.len() {
        let before = i.checked_sub(1).map_or(' ', |j| c[j]);
        match (c[i], at(i + 1)) {
            ('/', '/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
            }
            ('/', '*') => {
                i += 2;
                while i < c.len() && !(c[i] == '*' && at(i + 1) == '/') {
                    i += 1;
                }
                i += 2;
            }
            ('r', '#' | '"')
                if !ident(before) || (before == 'b' && !ident(at(i.wrapping_sub(2)))) =>
            {
                let mut j = i + 1;
                while at(j) == '#' {
                    j += 1;
                }
                if at(j) != '"' {
                    out.push(c[i]);
                    i += 1;
                    continue;
                }
                let close: String = std::iter::once('"')
                    .chain(c[i + 1..j].iter().copied())
                    .collect();
                let rest: String = c[j + 1..].iter().collect();
                let len = rest
                    .find(&close)
                    .map_or(c.len() - j - 1, |k| rest[..k].chars().count());
                out.push_str("\"\"");
                i = j + 1 + len + close.chars().count();
            }
            ('"', _) => {
                i += 1;
                while i < c.len() && c[i] != '"' {
                    i += if c[i] == '\\' { 2 } else { 1 };
                }
                out.push_str("\"\"");
                i += 1;
            }
            ('\'', '\\') => {
                i += 3;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                out.push_str("' '");
                i += 1;
            }
            ('\'', _) if at(i + 2) == '\'' => {
                out.push_str("' '");
                i += 3;
            }
            (ch, _) => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

/// The fields of the braced body that opens at `s[open]` (a `{`), split at its own commas, each trimmed: what a
/// struct literal or pattern names, its nested brackets left whole.
fn fields(s: &[char], open: usize) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0usize);
    for &c in &s[open + 1..] {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => break,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur).trim().to_string());
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// How many of `s`'s literals of a record that holds parts set them not empty (`s` is [`code`]). A pattern (its
/// last field `..`) sets nothing; `parts` alone, or `parts: <anything but an empty vector>`, sets them.
fn sets(s: &str) -> usize {
    let s: Vec<char> = s.chars().collect();
    let mut n = 0;
    for i in 0..s.len() {
        if i > 0 && ident(s[i - 1]) {
            continue;
        }
        let Some(h) = HOLDERS
            .iter()
            .find(|h| s[i..].starts_with(&h.chars().collect::<Vec<_>>()))
        else {
            continue;
        };
        let j = (i + h.len()..s.len()).find(|j| !s[*j].is_whitespace());
        let Some(j) = j.filter(|j| s[*j] == '{') else {
            continue;
        };
        let f = fields(&s, j);
        if f.last().map(String::as_str) == Some("..") {
            continue;
        }
        n += (f.iter())
            .filter(|f| match f.strip_prefix("parts") {
                Some("") => true,
                Some(v) => v.trim_start().strip_prefix(':').is_some_and(|v| {
                    !v.starts_with(':')
                        && !["Vec::new()", "vec![]", "Default::default()"].contains(&v.trim())
                }),
                None => false,
            })
            .count();
    }
    n
}

/// How many calls of the cap `s` makes (`s` is [`code`]; a definition is not a call).
fn caps(s: &str) -> usize {
    ["cap_with(", "cap_parts("]
        .iter()
        .flat_map(|name| s.match_indices(name))
        .filter(|(i, _)| {
            !s[..*i].ends_with(|c: char| ident(c)) && !s[..*i].trim_end().ends_with("fn")
        })
        .count()
}

/// Whether `s` asks the edge check (`s` is [`code`]; a definition is not an ask).
fn checks(s: &str) -> bool {
    ["edges_hold(", "edges_hold_with("].iter().any(|name| {
        (s.match_indices(name)).any(|(i, _)| {
            !s[..i].ends_with(|c: char| ident(c)) && !s[..i].trim_end().ends_with("fn")
        })
    })
}

/// What is wrong with `listed` for `sources` (each file's product source, by its path under `crates/`): each file
/// whose writers are not as listed, then each listed checker that does not ask the edge check. Empty when all
/// is well.
fn faults(sources: &BTreeMap<String, String>, listed: &[&str]) -> Vec<String> {
    let found: BTreeMap<&String, usize> = (sources.iter())
        .map(|(f, s)| (f, code(s)))
        .map(|(f, s)| (f, sets(&s) + caps(&s)))
        .filter(|(_, n)| *n > 0)
        .collect();
    let listed: BTreeMap<String, (usize, String)> = (listed.iter())
        .map(|r| {
            let mut w = r.splitn(4, ' ');
            let file = w.next().unwrap_or_default().to_string();
            let n = w.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            (file, (n, w.next().unwrap_or_default().to_string()))
        })
        .collect();
    let files: BTreeSet<&String> = found.keys().copied().chain(listed.keys()).collect();
    let mut out: Vec<String> = (files.into_iter())
        .map(|f| (f, found.get(f).copied(), listed.get(f).map(|(n, _)| *n)))
        .filter(|(_, a, b)| a != b)
        .map(|(f, a, b)| format!("{f}: found {a:?}, listed {b:?}"))
        .collect();
    for (file, (_, checker)) in &listed {
        if !sources.get(checker).is_some_and(|s| checks(&code(s))) {
            out.push(format!("{file}: its checker {checker} asks no edge check"));
        }
    }
    out
}

/// Each product source of every crate, by its path under `crates/`, cut at its first inline test module.
fn sources() -> BTreeMap<String, String> {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for c in std::fs::read_dir(&crates).unwrap().flatten() {
        let src = c.path().join("src");
        if src.is_dir() {
            walk(&src, &mut files);
        }
    }
    (files.into_iter())
        .map(|p| {
            let s = std::fs::read_to_string(&p).unwrap();
            let rel = (p.strip_prefix(&crates).unwrap().to_string_lossy()).replace('\\', "/");
            (rel, product(&s).to_string())
        })
        .collect()
}

/// A stored range is never inside a known value: every writer of parts in the product's sources is one of
/// [`WRITERS`], at its count, and the file named as its checker asks the edge check.
#[test]
fn every_writer_of_parts_is_listed_with_its_edge_check() {
    let faults = faults(&sources(), WRITERS);
    assert!(
        faults.is_empty(),
        "writers of parts not as listed:\n{}",
        faults.join("\n")
    );
}

/// A planted writer: a message stored with parts it made itself and no edge check. Unlisted, it is named; listed
/// as its own checker, the checker is named; listed with a checker that asks, it passes. A pattern of the same
/// record, and a literal with no parts, are not writers.
#[test]
fn a_writer_that_sets_parts_without_the_edge_check_is_named() {
    const PLANTED: &str = "theseus-core/src/planted.rs";
    const CHECKER: &str = "theseus-core/src/checker.rs";
    let planted = r#"
        fn note(id: &str, parts: Vec<Part>) -> Body {
            if let Body::UserMessage { text, .. } = &old { drop(text); }
            let none = Body::UserMessage { text: String::new(), attachments: Vec::new(), parts: Vec::new() };
            Body::UserMessage { text: String::new(), attachments: Vec::new(), parts }
        }
    "#;
    let checker = "fn ok(s: &Store, p: &[Part], c: &Cuts) -> bool { quotes::edges_hold(s, p, c) }";
    let sources: BTreeMap<String, String> = [(PLANTED, planted), (CHECKER, checker)]
        .into_iter()
        .map(|(f, s)| (f.to_string(), s.to_string()))
        .collect();
    assert_eq!(
        faults(&sources, &[]),
        vec![format!("{PLANTED}: found Some(1), listed None")]
    );
    let own = format!("{PLANTED} 1 {PLANTED} a note");
    assert_eq!(
        faults(&sources, &[own.as_str()]),
        vec![format!(
            "{PLANTED}: its checker {PLANTED} asks no edge check"
        )]
    );
    let checked = format!("{PLANTED} 1 {CHECKER} a note");
    assert_eq!(faults(&sources, &[checked.as_str()]), Vec::<String>::new());
}
