//! The check language: deterministic assertions on an exam answer (§2.9's
//! "the check"). A check is lines; every line must hold. A line is one or more
//! clauses joined by ` or `, and holds when any clause does:
//!
//! ```text
//! # comments and blank lines are skipped
//! reply has "7433"                         the reply contains it
//! reply lacks "fjall" | "sled"             it contains none of them
//! reply has word "redb"                    a whole word: no letter or digit beside it
//! reply has /\bmv\s+-f\b/                  a regex
//! reply lacks /\brm\s+\S*lock/i            flags: i (case), s (dot matches newline), m, x
//! calls has "&&"                           any tool call (its tool name and JSON input)
//! call proc.run has "&&" or reply has "&&" one tool's calls; `or` joins clauses
//! file "notes/slash.md" has "/wakes"       a file under the run's workspace
//! file "LOCK.stale" absent                 `exists` and `absent` take no pattern
//! ```
//!
//! - **Strings** are compared case-insensitively, after folding typographic
//!   quotes and dashes to ASCII and collapsing runs of whitespace to one
//!   space, so `"gate.sh && git"` matches across a line break. Escapes: `\"`,
//!   `\\`, `\n`.
//! - **Regexes** run on the text with quotes and dashes folded but case and
//!   whitespace kept: add `i` for case.
//! - `has a | b` holds when any pattern is found; `lacks a | b` when none is.
//! - A file path is relative, with no `..`; a file that cannot be read fails
//!   `has` and passes `lacks`.
//!
//! Parsing is strict (an unknown word is an error that names it and the
//! line), so a typo in an item fails the exam's validation, never a run.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde_json::Value;

/// One tool call of an answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub tool: String,
    pub input: Value,
}

/// What a check reads: the reply's text, the calls, and the workspace.
#[derive(Debug, Clone, Default)]
pub struct Answer {
    pub reply: String,
    pub calls: Vec<Call>,
    /// The run's workspace, for `file` lines; none means every file is unread.
    pub root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Subject {
    Reply,
    Calls,
    Call(String),
    File(String),
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// Normalized, lower-cased.
    Text(String),
    Word(String),
    Regex(Regex),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Has,
    Lacks,
    Exists,
    Absent,
}

#[derive(Debug, Clone)]
pub struct Clause {
    pub subject: Subject,
    pub verb: Verb,
    pub patterns: Vec<Pattern>,
}

/// One line: its clauses, any of which makes it hold, and its source.
#[derive(Debug, Clone)]
pub struct Line {
    pub source: String,
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub lines: Vec<Line>,
}

/// A line's verdict, with its source, for the run's record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LineResult {
    pub line: String,
    pub pass: bool,
}

/// Fold what models and keyboards vary: typographic quotes, dashes, the
/// non-breaking space.
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => '"',
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
            '\u{00A0}' | '\u{202F}' => ' ',
            c => c,
        })
        .collect()
}

/// Folded, lower-cased, whitespace collapsed: what text patterns compare.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in fold(s).chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.extend(c.to_lowercase());
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `needle` (normalized) in `hay` (normalized) with no word character on
/// either side.
pub(crate) fn has_word(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let at = from + i;
        let end = at + needle.len();
        let before = hay[..at].chars().next_back();
        let after = hay[end..].chars().next();
        if !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char) {
            return true;
        }
        from = at + hay[at..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

impl Pattern {
    /// Found in `raw` (and its normalized form `norm`).
    fn found(&self, raw: &str, norm: &str) -> bool {
        match self {
            Pattern::Text(t) => norm.contains(t.as_str()),
            Pattern::Word(w) => has_word(norm, w),
            Pattern::Regex(r) => r.is_match(&fold(raw)),
        }
    }
}

/// A tool call as the check sees it: the tool's name, then its input as JSON.
pub fn call_text(c: &Call) -> String {
    format!("{} {}", c.tool, c.input)
}

impl Clause {
    fn holds(&self, a: &Answer) -> bool {
        let texts: Vec<String> = match &self.subject {
            Subject::Reply => vec![a.reply.clone()],
            Subject::Calls => a.calls.iter().map(call_text).collect(),
            Subject::Call(t) => a
                .calls
                .iter()
                .filter(|c| &c.tool == t)
                .map(call_text)
                .collect(),
            Subject::File(p) => {
                let file = a.root.as_ref().map(|r| r.join(p));
                match self.verb {
                    Verb::Exists => return file.is_some_and(|f| f.is_file()),
                    Verb::Absent => return !file.is_some_and(|f| f.exists()),
                    _ => file
                        .and_then(|f| std::fs::read_to_string(f).ok())
                        .into_iter()
                        .collect(),
                }
            }
        };
        let found = texts.iter().any(|raw| {
            let norm = normalize(raw);
            self.patterns.iter().any(|p| p.found(raw, &norm))
        });
        match self.verb {
            Verb::Has => found,
            Verb::Lacks => !found,
            Verb::Exists | Verb::Absent => unreachable!("only files exist"),
        }
    }
}

impl Check {
    pub fn parse(src: &str) -> Result<Check> {
        let mut lines = Vec::new();
        for (n, raw) in src.lines().enumerate() {
            let t = raw.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let clauses = parse_line(t).with_context(|| format!("check line {}: {t}", n + 1))?;
            lines.push(Line {
                source: t.to_string(),
                clauses,
            });
        }
        if lines.is_empty() {
            bail!("a check needs at least one line");
        }
        Ok(Check { lines })
    }

    /// Every line's verdict, in order.
    pub fn run(&self, a: &Answer) -> Vec<LineResult> {
        self.lines
            .iter()
            .map(|l| LineResult {
                line: l.source.clone(),
                pass: l.clauses.iter().any(|c| c.holds(a)),
            })
            .collect()
    }

    /// Every line holds.
    pub fn passes(&self, a: &Answer) -> bool {
        self.run(a).iter().all(|r| r.pass)
    }
}

/// A tokenizer for one line: words, quoted strings, regexes, `|`.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    Re(String, String),
    Bar,
}

fn tokens(line: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let mut it = line.chars().peekable();
    while let Some(&c) = it.peek() {
        if c.is_whitespace() {
            it.next();
        } else if c == '|' {
            it.next();
            out.push(Tok::Bar);
        } else if c == '"' {
            it.next();
            let mut s = String::new();
            loop {
                match it.next() {
                    None => bail!("a string is not closed: add a \""),
                    Some('"') => break,
                    Some('\\') => match it.next() {
                        Some('"') => s.push('"'),
                        Some('\\') => s.push('\\'),
                        Some('n') => s.push('\n'),
                        Some(o) => bail!("unknown escape \\{o} (use \\\", \\\\, or \\n)"),
                        None => bail!("a string is not closed: add a \""),
                    },
                    Some(o) => s.push(o),
                }
            }
            out.push(Tok::Str(s));
        } else if c == '/' {
            it.next();
            let mut s = String::new();
            loop {
                match it.next() {
                    None => bail!("a regex is not closed: add a /"),
                    Some('/') => break,
                    Some('\\') => {
                        // `\/` is a slash; any other escape is the regex's.
                        match it.next() {
                            Some('/') => s.push('/'),
                            Some(o) => {
                                s.push('\\');
                                s.push(o);
                            }
                            None => bail!("a regex is not closed: add a /"),
                        }
                    }
                    Some(o) => s.push(o),
                }
            }
            let mut flags = String::new();
            while let Some(&f) = it.peek() {
                if f.is_ascii_alphabetic() {
                    flags.push(f);
                    it.next();
                } else {
                    break;
                }
            }
            out.push(Tok::Re(s, flags));
        } else {
            let mut w = String::new();
            while let Some(&o) = it.peek() {
                if o.is_whitespace() || o == '"' || o == '|' {
                    break;
                }
                w.push(o);
                it.next();
            }
            out.push(Tok::Word(w));
        }
    }
    Ok(out)
}

fn regex(src: &str, flags: &str) -> Result<Regex> {
    let mut b = regex::RegexBuilder::new(src);
    for f in flags.chars() {
        match f {
            'i' => b.case_insensitive(true),
            's' => b.dot_matches_new_line(true),
            'm' => b.multi_line(true),
            'x' => b.ignore_whitespace(true),
            o => bail!("unknown regex flag {o} (use i, s, m, x)"),
        };
    }
    b.build()
        .with_context(|| format!("the regex /{src}/ does not compile"))
}

fn check_path(p: &str) -> Result<()> {
    let path = Path::new(p);
    if p.is_empty() || path.is_absolute() {
        bail!("a file path must be relative to the workspace: {p:?}");
    }
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("a file path may not leave the workspace: {p:?}");
    }
    Ok(())
}

fn parse_line(line: &str) -> Result<Vec<Clause>> {
    let toks = tokens(line)?;
    // Split on the word `or` (a bare word, never inside a string).
    let mut groups: Vec<Vec<Tok>> = vec![Vec::new()];
    for t in toks {
        if t == Tok::Word("or".into()) {
            groups.push(Vec::new());
        } else {
            groups.last_mut().expect("one group").push(t);
        }
    }
    groups.into_iter().map(parse_clause).collect()
}

fn parse_clause(toks: Vec<Tok>) -> Result<Clause> {
    let mut it = toks.into_iter().peekable();
    let subject = match it.next() {
        Some(Tok::Word(w)) if w == "reply" => Subject::Reply,
        Some(Tok::Word(w)) if w == "calls" => Subject::Calls,
        Some(Tok::Word(w)) if w == "call" => match it.next() {
            Some(Tok::Word(t)) if !matches!(t.as_str(), "has" | "lacks" | "exists" | "absent") => {
                Subject::Call(t)
            }
            _ => bail!("`call` needs a tool name, as in `call proc.run has \"&&\"`"),
        },
        Some(Tok::Word(w)) if w == "file" => match it.next() {
            Some(Tok::Str(p)) => {
                check_path(&p)?;
                Subject::File(p)
            }
            _ => bail!("`file` needs a quoted path, as in `file \"notes.md\" has \"x\"`"),
        },
        None => bail!("an empty clause (check the `or`s)"),
        Some(o) => {
            bail!("unknown subject {o:?}: use reply, calls, call <tool>, or file \"<path>\"")
        }
    };
    let verb = match it.next() {
        Some(Tok::Word(w)) if w == "has" => Verb::Has,
        Some(Tok::Word(w)) if w == "lacks" => Verb::Lacks,
        Some(Tok::Word(w)) if w == "exists" => Verb::Exists,
        Some(Tok::Word(w)) if w == "absent" => Verb::Absent,
        Some(o) => bail!("unknown verb {o:?}: use has, lacks, exists, or absent"),
        None => bail!("a clause needs a verb: has, lacks, exists, or absent"),
    };
    let is_file = matches!(subject, Subject::File(_));
    if matches!(verb, Verb::Exists | Verb::Absent) {
        if !is_file {
            bail!("only a file can exist or be absent");
        }
        if let Some(t) = it.next() {
            bail!("`exists` and `absent` take no pattern, found {t:?}");
        }
        return Ok(Clause {
            subject,
            verb,
            patterns: Vec::new(),
        });
    }
    let mut patterns = Vec::new();
    loop {
        let p = match it.next() {
            Some(Tok::Str(s)) => {
                let n = normalize(&s);
                if n.is_empty() {
                    bail!("an empty string matches everything");
                }
                Pattern::Text(n)
            }
            Some(Tok::Word(w)) if w == "word" => match it.next() {
                Some(Tok::Str(s)) => {
                    let n = normalize(&s);
                    if n.is_empty() {
                        bail!("an empty word matches nothing");
                    }
                    Pattern::Word(n)
                }
                _ => bail!("`word` needs a quoted string, as in `word \"redb\"`"),
            },
            Some(Tok::Re(src, flags)) => Pattern::Regex(regex(&src, &flags)?),
            Some(o) => {
                bail!("expected a pattern (a \"string\", word \"…\", or /regex/), found {o:?}")
            }
            None => bail!("`has` and `lacks` need a pattern"),
        };
        patterns.push(p);
        match it.next() {
            None => break,
            Some(Tok::Bar) => continue,
            Some(o) => bail!("expected `|`, `or`, or the line's end after a pattern, found {o:?}"),
        }
    }
    Ok(Clause {
        subject,
        verb,
        patterns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(s: &str) -> Answer {
        Answer {
            reply: s.into(),
            ..Default::default()
        }
    }

    fn pass(check: &str, a: &Answer) -> bool {
        Check::parse(check).unwrap().passes(a)
    }

    #[test]
    fn has_and_lacks_compare_folded_lowercased_collapsed_text() {
        let a =
            reply("The Observatory binds   127.0.0.1:7433.\nRun `scripts/gate.sh\n&& git commit`.");
        assert!(pass(r#"reply has "7433""#, &a));
        assert!(pass(r#"reply has "OBSERVATORY binds""#, &a));
        assert!(
            pass(r#"reply has "gate.sh && git commit""#, &a),
            "across the line break"
        );
        assert!(!pass(r#"reply has "8080""#, &a));
        assert!(pass(r#"reply lacks "8080""#, &a));
        assert!(!pass(r#"reply lacks "7433""#, &a));
        // Typographic quotes and dashes fold to ASCII, both sides.
        let b = reply("Use \u{201C}redb\u{201D} \u{2014} never \u{2018}fjall\u{2019}.");
        assert!(pass(r#"reply has "\"redb\" - never 'fjall'""#, &b));
    }

    #[test]
    fn alternatives_any_for_has_none_for_lacks() {
        let a = reply("We keep it in places.toml.");
        assert!(pass(r#"reply has "bindings.toml" | "places.toml""#, &a));
        assert!(!pass(r#"reply has "bindings.toml" | "hosts.toml""#, &a));
        assert!(pass(r#"reply lacks "bindings.toml" | "hosts.toml""#, &a));
        assert!(!pass(r#"reply lacks "bindings.toml" | "places.toml""#, &a));
    }

    #[test]
    fn word_needs_no_letter_or_digit_beside_it() {
        let a = reply("Port 74330 is not it; $80 is the limit; redbird flies; use redb.");
        assert!(!pass(r#"reply has word "7433""#, &a));
        assert!(
            pass(r#"reply has word "80""#, &a),
            "a $ is not a word character"
        );
        assert!(
            pass(r#"reply has word "redb""#, &a),
            "the second occurrence counts"
        );
        assert!(!pass(r#"reply has word "bird""#, &a));
        assert!(pass(r#"reply lacks word "74""#, &a));
        assert!(
            pass(r#"reply has word "port 74330""#, &a),
            "a phrase as a word"
        );
    }

    #[test]
    fn regexes_keep_case_unless_flagged() {
        let a = reply("cp target/release/theseusd ~/.local/bin/.theseusd.new && mv -f ~/.local/bin/.theseusd.new ~/.local/bin/theseusd");
        assert!(pass(r"reply has /\bmv\s+-f\b/", &a));
        assert!(!pass(r"reply has /\bMV\b/", &a));
        assert!(pass(r"reply has /\bMV\b/i", &a));
        assert!(pass(r"reply has /\.local\/bin/", &a), "an escaped slash");
        let rm = reply("Run: rm -f state/store/LOCK.stale");
        let trash = reply("Run `trash state/store/LOCK.stale`; never rm for this.");
        let check = r"reply lacks /\brm\s+(-\w+\s+)*\S*lock\.stale/i";
        assert!(!pass(check, &rm));
        assert!(pass(check, &trash), "naming rm is not running it");
        // Typographic dashes fold before a regex runs.
        assert!(pass(r"reply has /mv -f/", &reply("mv \u{2013}f x y")));
    }

    #[test]
    fn or_holds_when_any_clause_does_and_every_line_must_hold() {
        let a = Answer {
            reply: "Chain them.".into(),
            calls: vec![Call {
                tool: "proc.run".into(),
                input: json!({"argv": ["bash", "-c", "scripts/gate.sh && git commit -S -F msg.txt"]}),
            }],
            root: None,
        };
        assert!(pass(
            r#"reply has "&&" or call proc.run has "gate.sh &&""#,
            &a
        ));
        assert!(!pass(r#"reply has "&&" or call fs.write has "&&""#, &a));
        assert!(pass(r#"calls has "-F msg.txt""#, &a));
        assert!(
            pass(r#"calls has "proc.run""#, &a),
            "the tool's name is part of the call's text"
        );
        let two = "reply has \"chain\"\n# a comment\n\ncall proc.run lacks \"gate.sh;\"";
        assert!(pass(two, &a));
        let r = Check::parse("reply has \"chain\"\nreply has \"nope\"")
            .unwrap()
            .run(&a);
        assert_eq!(
            r,
            vec![
                LineResult {
                    line: "reply has \"chain\"".into(),
                    pass: true
                },
                LineResult {
                    line: "reply has \"nope\"".into(),
                    pass: false
                },
            ]
        );
    }

    #[test]
    fn file_assertions_read_under_the_workspace_only() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("notes")).unwrap();
        std::fs::write(d.path().join("notes/slash.md"), "Add /wakes (bare).").unwrap();
        let a = Answer {
            root: Some(d.path().to_path_buf()),
            ..Default::default()
        };
        assert!(pass(r#"file "notes/slash.md" has "/wakes""#, &a));
        assert!(pass(r#"file "notes/slash.md" lacks "/theseus-""#, &a));
        assert!(pass(r#"file "notes/slash.md" exists"#, &a));
        assert!(pass(r#"file "./notes/missing.md" absent"#, &a));
        assert!(!pass(r#"file "notes/missing.md" exists"#, &a));
        assert!(
            !pass(r#"file "notes/missing.md" has "x""#, &a),
            "an unread file has nothing"
        );
        assert!(pass(r#"file "notes/missing.md" lacks "x""#, &a));
        assert!(
            !pass(r#"file "notes" exists"#, &a),
            "a directory is not a file"
        );
        // Without a workspace every file is unread.
        let none = Answer::default();
        assert!(pass(r#"file "notes/slash.md" absent"#, &none));
        assert!(!pass(r#"file "notes/slash.md" has "/wakes""#, &none));
    }

    #[test]
    fn malformed_checks_are_refused_naming_the_line_and_the_fault() {
        let err = |src: &str| format!("{:#}", Check::parse(src).unwrap_err());
        for (src, says) in [
            ("", "at least one line"),
            ("# only a comment", "at least one line"),
            ("reply has \"open", "not closed"),
            ("reply has /open", "not closed"),
            ("reply contains \"x\"", "unknown verb"),
            ("answer has \"x\"", "unknown subject"),
            ("reply has", "need a pattern"),
            ("reply has \"\"", "empty string"),
            ("reply has \"a\" \"b\"", "expected `|`"),
            ("reply has \"a\" |", "need a pattern"),
            ("reply has /(/", "does not compile"),
            ("reply has /x/q", "unknown regex flag"),
            ("reply has \"\\t\"", "unknown escape"),
            ("reply exists", "only a file"),
            ("file \"/etc/passwd\" has \"root\"", "relative"),
            ("file \"../x\" exists", "leave the workspace"),
            ("file \"a\" exists \"x\"", "take no pattern"),
            ("call has \"x\"", "tool name"),
            ("reply has \"x\" or", "empty clause"),
            ("reply has word", "quoted string"),
        ] {
            let e = err(src);
            assert!(e.contains(says), "{src:?}: {e}");
            if !src.trim().is_empty() && !src.starts_with('#') {
                assert!(e.contains("check line 1"), "{src:?}: {e}");
            }
        }
    }

    #[test]
    fn normalize_folds_and_collapses() {
        assert_eq!(normalize("  A\u{00A0}B\t\nC  "), "a b c");
        assert_eq!(normalize("\u{201C}x\u{201D}\u{2014}y"), "\"x\"-y");
        assert_eq!(normalize(""), "");
    }
}
