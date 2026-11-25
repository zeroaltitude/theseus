//! The writers of parts (issue 52): every place a stored record gets parts asks the edge check first
//! (`quotes::edges_hold`, `edges_hold_with`), so a range that begins or ends inside a known value is never stored.
//! The twin of the readers' guard (`quoted::tests::no_reader_reads_content_of_a_parts_result`).
//!
//! **This guard is an inventory, not a proof of data flow.** It reads the product's sources as text. It finds each
//! place that writes parts, by its function, and holds the list of those places and of the function that checks
//! what each writes. It proves three things:
//! - every such place is listed, at its count, so a new one fails here until a reviewer reads it;
//! - each listed checker asks the edge check in its own text, not merely somewhere in its file;
//! - a function that caps parts (which makes new edges) asks the edge check itself.
//!
//! It does not prove that a checker's answer is obeyed, that it checks the very parts that are stored, or that the
//! writer's parts came from the checker it names: for a literal or an assignment that is the list's claim, read by a
//! reviewer. Shapes it cannot see at all:
//! - parts changed by a method on a binding or a field (`parts.push`, `parts.extend`, `std::mem::replace`, `swap`);
//! - an assignment through a binding renamed by a pattern (`UserMessage { parts: p, .. }`, then `*p = made`);
//! - a record decoded from bytes with its parts (serde), and parts carried in a type that is not a holder below;
//! - a holder built by a macro or under another name (a type alias, a re-export under a new name);
//! - code in a test module or a test file;
//! - the cap's own calls of the cap in its own module (`quotes/cap.rs`; every caller elsewhere is counted). A holder
//!   literal or an assignment in that module is still counted.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::quoted::tests::{product, walk};

/// Every function in the product's sources that writes parts, each `<file>::<function> <count> <checker> <why>`,
/// files under `crates/`. A function writes parts when it builds a `ToolResult`, `UserMessage` or `Piece` whose
/// `parts` is not empty, assigns a `parts` field or a deref'd `parts` binding (`x.parts = …`, `*parts = …`), or
/// calls the cap (`cap_with`, `cap_parts`), which makes new edges. The checker is the function, `<file>::<function>`,
/// that asks the edge check on what is written; one that caps is its own checker. Where the check fails, the
/// record keeps its words instead.
const WRITERS: &[&str] = &[
    "theseus-core/src/arrangement.rs::resolve_in 1 theseus-core/src/arrangement.rs::quoted a task's piece, its \
     parts from `quoted` (a look back's through `look_back`, which checks its own)",
    "theseus-core/src/arrangement.rs::quoted 1 theseus-core/src/arrangement.rs::quoted a message's or a reply's \
     quote, capped and checked",
    "theseus-core/src/arrangement.rs::look_back 1 theseus-core/src/arrangement.rs::look_back a look back's own \
     quotes for a task's piece (issue 52), capped and checked",
    "theseus-core/src/toolrun.rs::result_node_in 1 theseus-core/src/toolrun/parts.rs::capped_parts a look back's \
     result, its parts from `capped_parts`",
    "theseus-core/src/toolrun/fit.rs::fit_noted 1 theseus-core/src/toolrun/parts.rs::capped_parts the call's fit \
     note put before a look back's parts in memory, as code; `result_node_in` then caps and checks them",
    "theseus-core/src/toolrun/parts.rs::capped_parts 1 theseus-core/src/toolrun/parts.rs::capped_parts a look \
     back's parts, capped and checked",
];

/// Assignments to a field or binding named `parts` that are not a quote's parts, each
/// `<file>::<function> <count> <what they are>`: the scan cannot tell types apart, so each is listed and read.
const NOT_QUOTES: &[&str] = &[
    "theseus-core/src/attach.rs::keep_file 2 a file's page parts, and their count on its read",
    "theseus-core/src/rpc/context.rs::system_of 1 the system prompt's parts, explained",
    "theseus-core/src/rpc/context.rs::detour_of 1 the system prompt's parts, explained",
];

/// The stored records that hold parts.
const HOLDERS: [&str; 3] = ["ToolResult", "UserMessage", "Piece"];

/// The cap's own module: its calls of the cap are not counted (its wrappers call the cap, and every caller of them
/// is counted where it calls); a holder literal or an assignment in it still is.
const CAP: &str = "theseus-core/src/quotes/cap.rs";

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

/// Whether the word `w` stands at `s[i]`: no name runs into it on either side.
fn word_at(s: &[char], i: usize, w: &str) -> bool {
    let w: Vec<char> = w.chars().collect();
    s[i..].starts_with(&w)
        && (i == 0 || !ident(s[i - 1]))
        && !s.get(i + w.len()).copied().is_some_and(ident)
}

/// The nearest character of `s` before `i` that is not white space.
fn before(s: &[char], i: usize) -> Option<char> {
    s[..i].iter().rev().copied().find(|c| !c.is_whitespace())
}

/// Where `s` (code, [`code`]) builds a record that holds parts with them set not empty, one place per field. A
/// pattern (its last field `..`) sets nothing; `parts` alone, or `parts: <anything but an empty vector>`, sets them.
fn sets(s: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..s.len() {
        let Some(h) = HOLDERS.iter().find(|h| word_at(s, i, h)) else {
            continue;
        };
        let j = (i + h.len()..s.len()).find(|j| !s[*j].is_whitespace());
        let Some(j) = j.filter(|j| s[*j] == '{') else {
            continue;
        };
        let f = fields(s, j);
        if f.last().map(String::as_str) == Some("..") {
            continue;
        }
        let set = |f: &String| match f.strip_prefix("parts") {
            Some("") => true,
            Some(v) => v.trim_start().strip_prefix(':').is_some_and(|v| {
                !v.starts_with(':')
                    && !["Vec::new()", "vec![]", "Default::default()"].contains(&v.trim())
            }),
            None => false,
        };
        out.extend(std::iter::repeat_n(i, f.iter().filter(|f| set(f)).count()));
    }
    out
}

/// Where `s` (code) assigns a field named `parts` or a deref'd binding of that name: `x.parts = …`, `*parts = …`,
/// with any white space (a line break, a comment's place) before the `=`.
fn assigns(s: &[char]) -> Vec<usize> {
    (0..s.len())
        .filter(|i| word_at(s, *i, "parts"))
        .filter(|i| matches!(before(s, *i), Some('.' | '*')))
        .filter(|i| {
            let mut rest =
                (s[i + "parts".len()..].iter().copied()).skip_while(|c| c.is_whitespace());
            rest.next() == Some('=') && !matches!(rest.next(), Some('=' | '>'))
        })
        .collect()
}

/// Where `s` (code) calls `name` (a definition, `fn name(`, is not a call).
fn calls(s: &[char], name: &str) -> Vec<usize> {
    (0..s.len())
        .filter(|i| word_at(s, *i, name) && s.get(i + name.len()) == Some(&'('))
        .filter(|i| {
            let head: String = s[..*i].iter().collect();
            !head.trim_end().ends_with("fn")
        })
        .collect()
}

/// Where `s` (code) calls the cap.
fn caps(s: &[char]) -> Vec<usize> {
    let mut out = calls(s, "cap_with");
    out.extend(calls(s, "cap_parts"));
    out
}

/// Whether `s` (code) asks the edge check.
fn checks(s: &[char]) -> bool {
    !calls(s, "edges_hold").is_empty() || !calls(s, "edges_hold_with").is_empty()
}

/// A function of a source (its code): its name, and its text from `fn` to the end of its body.
struct Func {
    name: String,
    span: Range<usize>,
}

/// Every function of `s` (code) that has a body.
fn funcs(s: &[char]) -> Vec<Func> {
    let mut out = Vec::new();
    for i in 0..s.len() {
        if !word_at(s, i, "fn") {
            continue;
        }
        let j = (i + 2..s.len())
            .find(|j| !s[*j].is_whitespace())
            .unwrap_or(s.len());
        let name: String = s[j..].iter().take_while(|c| ident(**c)).collect();
        if name.is_empty() {
            continue;
        }
        // The body opens at the first `{` outside the signature's brackets; a `;` first is a declaration.
        let mut depth = 0i32;
        let mut open = None;
        for (k, c) in s.iter().enumerate().skip(j + name.len()) {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                ';' if depth == 0 => break,
                '{' if depth == 0 => {
                    open = Some(k);
                    break;
                }
                _ => {}
            }
        }
        let Some(open) = open else {
            continue;
        };
        let (mut d, mut end) = (0, s.len());
        for (k, c) in s.iter().enumerate().skip(open) {
            match c {
                '{' => d += 1,
                '}' => {
                    d -= 1;
                    if d == 0 {
                        end = k + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(Func { name, span: i..end });
    }
    out
}

/// The name of the innermost function of `fs` that holds `at`; `(top)` outside every function.
fn holder_of(fs: &[Func], at: usize) -> String {
    (fs.iter())
        .filter(|f| f.span.contains(&at))
        .max_by_key(|f| f.span.start)
        .map_or_else(|| "(top)".to_string(), |f| f.name.clone())
}

/// `f`'s own text in `s`: its span without the functions nested in it.
fn own(s: &[char], fs: &[Func], f: &Func) -> Vec<char> {
    let inner: Vec<&Range<usize>> = (fs.iter())
        .map(|g| &g.span)
        .filter(|g| g.start > f.span.start && g.end <= f.span.end)
        .collect();
    (f.span.clone())
        .filter(|i| !inner.iter().any(|g| g.contains(i)))
        .map(|i| s[i])
        .collect()
}

/// A source, read: its code and functions.
struct Read {
    code: Vec<char>,
    funcs: Vec<Func>,
}

impl Read {
    fn of(s: &str) -> Read {
        let code: Vec<char> = code(s).chars().collect();
        let funcs = funcs(&code);
        Read { code, funcs }
    }

    /// The own text of the one function named `name`; an error when there is none, or more than one.
    fn own(&self, name: &str) -> Result<Vec<char>, String> {
        let named: Vec<&Func> = self.funcs.iter().filter(|f| f.name == name).collect();
        match named[..] {
            [f] => Ok(own(&self.code, &self.funcs, f)),
            _ => Err(format!("names {} functions", named.len())),
        }
    }
}

/// What is wrong with `writers` and `others` (as [`WRITERS`] and [`NOT_QUOTES`]) for `sources` (each file's product
/// source, by its path under `crates/`). Empty when all is well.
fn faults(sources: &BTreeMap<String, String>, writers: &[&str], others: &[&str]) -> Vec<String> {
    let read: BTreeMap<&String, Read> = sources.iter().map(|(f, s)| (f, Read::of(s))).collect();
    // Each function's places that write parts, and those of them that cap.
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    let mut capping: BTreeSet<String> = BTreeSet::new();
    for (file, r) in &read {
        let cap = match file.as_str() == CAP {
            true => Vec::new(),
            false => caps(&r.code),
        };
        for at in sets(&r.code).into_iter().chain(assigns(&r.code)) {
            *found
                .entry(format!("{file}::{}", holder_of(&r.funcs, at)))
                .or_default() += 1;
        }
        for at in cap {
            let key = format!("{file}::{}", holder_of(&r.funcs, at));
            *found.entry(key.clone()).or_default() += 1;
            capping.insert(key);
        }
    }
    let entry = |r: &&str| {
        let mut w = r.splitn(3, ' ');
        let key = w.next().unwrap_or_default().to_string();
        let n: usize = w.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        (key, n, w.next().unwrap_or_default().to_string())
    };
    let mut out = Vec::new();
    let mut listed: BTreeMap<String, usize> = BTreeMap::new();
    for (key, n, _) in writers.iter().chain(others).map(entry) {
        if listed.insert(key.clone(), n).is_some() {
            out.push(format!("{key}: listed twice"));
        }
    }
    let keys: BTreeSet<&String> = found.keys().chain(listed.keys()).collect();
    for key in keys {
        let (a, b) = (found.get(key), listed.get(key));
        if a != b {
            out.push(format!("{key}: found {a:?}, listed {b:?}"));
        }
    }
    for (key, _, rest) in writers.iter().map(entry) {
        let checker = rest.split(' ').next().unwrap_or_default();
        if capping.contains(&key) && checker != key {
            out.push(format!(
                "{key}: it caps parts, so it checks them itself, not {checker}"
            ));
        }
        let (file, name) = checker.rsplit_once("::").unwrap_or((checker, ""));
        let asks = match read.get(&file.to_string()).map(|r| r.own(name)) {
            Some(Ok(text)) => checks(&text),
            Some(Err(why)) => {
                out.push(format!("{key}: its checker {checker} {why}"));
                continue;
            }
            None => false,
        };
        if !asks {
            out.push(format!(
                "{key}: its checker {checker} asks no edge check in its own text"
            ));
        }
    }
    for (key, _, _) in others.iter().map(entry) {
        if capping.contains(&key) {
            out.push(format!("{key}: it caps parts, so it is a writer of quotes"));
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

/// `sources` with one file's `from` (which it holds once) replaced by `to`.
fn mutated(
    sources: &BTreeMap<String, String>,
    file: &str,
    from: &str,
    to: &str,
) -> BTreeMap<String, String> {
    let mut out = sources.clone();
    let s = out.get_mut(file).unwrap();
    assert_eq!(s.matches(from).count(), 1, "{file} holds {from:?} once");
    *s = s.replace(from, to);
    out
}

/// A stored range is never inside a known value: every function in the product's sources that writes parts is one
/// of [`WRITERS`] or [`NOT_QUOTES`], at its count, and each writer's checker asks the edge check in its own text.
#[test]
fn every_writer_of_parts_is_listed_with_its_edge_check() {
    let faults = faults(&sources(), WRITERS, NOT_QUOTES);
    assert!(
        faults.is_empty(),
        "writers of parts not as listed:\n{}",
        faults.join("\n")
    );
}

/// The look back's piece checks its own parts: with its check taken out of the real source, the guard names that
/// function, though the message's branch in the same file still checks (a guard by file would pass).
#[test]
fn a_look_backs_piece_without_its_own_check_is_named() {
    const FILE: &str = "theseus-core/src/arrangement.rs";
    let real = sources();
    let unchecked = mutated(
        &real,
        FILE,
        "match crate::quotes::edges_hold(store, &parts, cuts) {",
        "match true {",
    );
    assert!(
        unchecked[FILE].contains("edges_hold_with("),
        "the message's branch still checks"
    );
    assert_eq!(
        faults(&unchecked, WRITERS, NOT_QUOTES),
        vec![format!(
            "{FILE}::look_back: its checker {FILE}::look_back asks no edge check in its own text"
        )]
    );
}

/// A planted writer: a message stored with parts it made itself and no edge check. Unlisted, it is named; listed
/// as its own checker, the checker is named; a function that caps and names another checker is named. Listed with a
/// checker that asks, it passes, though nothing here shows that `note` calls `ok`: the guard is an inventory, and
/// that claim is the list's, read by a reviewer. A pattern of the same record, and a literal with no parts, are
/// not writers.
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
        fn trim(s: &Store, p: Vec<Part>, c: &Cuts) -> Vec<Part> { quotes::cap::cap_parts(s, p, 9, rest, c).0 }
    "#;
    let checker = "fn ok(s: &Store, p: &[Part], c: &Cuts) -> bool { quotes::edges_hold(s, p, c) }";
    let sources: BTreeMap<String, String> = [(PLANTED, planted), (CHECKER, checker)]
        .into_iter()
        .map(|(f, s)| (f.to_string(), s.to_string()))
        .collect();
    let (note, trim) = (format!("{PLANTED}::note"), format!("{PLANTED}::trim"));
    let ok = format!("{CHECKER}::ok");
    assert_eq!(
        faults(&sources, &[], &[]),
        vec![
            format!("{note}: found Some(1), listed None"),
            format!("{trim}: found Some(1), listed None"),
        ]
    );
    let own = [
        format!("{note} 1 {note} a note"),
        format!("{trim} 1 {trim} a cut"),
    ];
    assert_eq!(
        faults(&sources, &own.each_ref().map(String::as_str), &[]),
        vec![
            format!("{note}: its checker {note} asks no edge check in its own text"),
            format!("{trim}: its checker {trim} asks no edge check in its own text"),
        ]
    );
    let elsewhere = [
        format!("{note} 1 {ok} a note"),
        format!("{trim} 1 {ok} a cut"),
    ];
    assert_eq!(
        faults(&sources, &elsewhere.each_ref().map(String::as_str), &[]),
        vec![format!(
            "{trim}: it caps parts, so it checks them itself, not {ok}"
        )]
    );
}

/// A planted writer that assigns parts to a record it already holds, by a field (`p.parts = made`) and through a
/// pattern's binding (`*parts = made`), is named, also with its `=` on the next line or after a comment; reading
/// parts, comparing them, or matching them is not writing.
#[test]
fn a_writer_that_assigns_parts_is_named() {
    const PLANTED: &str = "theseus-core/src/planted.rs";
    let planted = r#"
        fn patch(p: &mut Piece, made: Vec<Part>) { p.parts = made; }
        fn mend(n: &mut Node, made: Vec<Part>) {
            if let Body::UserMessage { parts, .. } = &mut n.body { *parts = made; }
        }
        fn wrap(p: &mut Piece, made: Vec<Part>) {
            p.parts
                = made;
        }
        fn remark(n: &mut Node, made: Vec<Part>) {
            if let Body::UserMessage { parts, .. } = &mut n.body { *parts /* the new ones */ = made; }
        }
        fn look(n: &Node, m: &Node) -> bool {
            let same = n.parts == m.parts;
            match n.parts { ref p => same && p.is_empty() }
        }
    "#;
    let sources = BTreeMap::from([(PLANTED.to_string(), planted.to_string())]);
    assert_eq!(
        faults(&sources, &[], &[]),
        vec![
            format!("{PLANTED}::mend: found Some(1), listed None"),
            format!("{PLANTED}::patch: found Some(1), listed None"),
            format!("{PLANTED}::remark: found Some(1), listed None"),
            format!("{PLANTED}::wrap: found Some(1), listed None"),
        ]
    );
}
