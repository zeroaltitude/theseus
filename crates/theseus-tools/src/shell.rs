//! Just enough of the POSIX shell grammar to find every command a `proc.run`
//! would start, for the consequence rules (spec §3.9). A `bash -c` string is
//! split into its simple commands (through `&&`, `||`, `;`, pipes, subshells,
//! `$(…)`, backticks, and heredoc bodies), and wrappers (`env`, `nohup`,
//! `timeout`, `xargs`, `sudo`, …) are seen through to the program they start.
//!
//! It over-approximates on purpose: a word that might be a command is treated
//! as one, and a working directory that might apply does apply (a target must
//! be judged safe under every directory it could run in). What it cannot see
//! through (unbalanced quotes, a command word built from a variable, nesting
//! past `MAX_DEPTH`) comes out as a command marked `unparsed` or `dynamic`,
//! which the rule table turns into `opaque`.

use std::path::{Path, PathBuf};

/// How deep shells, `$(…)`, and wrappers may nest before the rest is opaque.
pub const MAX_DEPTH: usize = 8;

/// One word of a command, unquoted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Word {
    pub text: String,
    /// Holds an expansion the parser cannot resolve (`$VAR`, `$(…)`, `` `…` ``).
    /// For a simple `$name`/`${name}`, `text` keeps the reference (`$HOME`), so a
    /// path judgment can resolve `$HOME`/`~` even though the word stays dynamic.
    pub dynamic: bool,
    /// Holds an unquoted glob character, expanded at run time.
    pub glob: bool,
    /// Any part of the word was quoted or backslash-escaped. A quoted word is
    /// exactly one word: it never brace-expands, and a quoted dynamic word is a
    /// single operand, never a split-out option (`rm "$f"` deletes one file).
    pub quoted: bool,
    /// Command substitutions inside the word, run before the command.
    subs: Vec<String>,
}

impl Word {
    pub fn lit(s: &str) -> Self {
        Word {
            text: s.into(),
            ..Default::default()
        }
    }
}

/// Where a command runs: every directory it might run in, or unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cwds {
    pub dirs: Vec<PathBuf>,
    /// A `cd` the parser could not follow (`cd "$X"`, `cd -`, `popd`).
    pub unknown: bool,
}

impl Cwds {
    pub fn one(p: &Path) -> Self {
        Cwds {
            dirs: vec![p.to_path_buf()],
            unknown: false,
        }
    }
    pub fn unknown() -> Self {
        Cwds {
            dirs: vec![],
            unknown: true,
        }
    }
    fn union(&mut self, other: &Cwds) {
        self.unknown |= other.unknown;
        for d in &other.dirs {
            if !self.dirs.contains(d) {
                self.dirs.push(d.clone());
            }
        }
    }
    /// `cd <dir>` from every candidate.
    fn cd(&self, w: &Word) -> Cwds {
        if w.dynamic || w.text.is_empty() || w.text == "-" {
            return Cwds::unknown();
        }
        let raw = expand_tilde(&w.text);
        Cwds {
            dirs: self
                .dirs
                .iter()
                .map(|d| crate::paths::normalize(&d.join(&raw)))
                .collect(),
            unknown: self.unknown,
        }
    }
}

pub fn expand_tilde(s: &str) -> PathBuf {
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    if s == "~" {
        home()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(s)
    }
}

/// A command as the rules see it.
#[derive(Debug, Clone)]
pub struct Cmd {
    /// The words; the first is the program, reduced to its file name unless it
    /// was invoked by a relative path (`./x.sh` stays `./x.sh`).
    pub words: Vec<Word>,
    pub cwds: Cwds,
    /// More operands arrive at run time (`xargs`, `find -exec … {} +`).
    pub more_operands: bool,
    /// The parser could not see through this text (the only word is the text).
    pub unparsed: bool,
    /// The wrapper and shell programs this command was reached through, in order
    /// (`sudo`, `bash`, `env`, …), because `expand` replaces `sudo x` with `x`.
    /// The floor and the deny list judge these too.
    pub via: Vec<String>,
    /// The targets of this command's file redirections (`>`, `>>`, `<`, `<>`,
    /// `&>`, `>|`, `2>file`), as words. Not heredoc bodies, here-strings, or
    /// `>&2`-style fd duplications.
    pub redirs: Vec<Word>,
    /// Where it came from, as written, for the notice.
    pub text: String,
}

impl Cmd {
    pub fn prog(&self) -> &str {
        self.words.first().map(|w| w.text.as_str()).unwrap_or("")
    }
    pub fn args(&self) -> &[Word] {
        self.words.get(1..).unwrap_or(&[])
    }
    /// The program word is built from an expansion.
    pub fn dynamic_program(&self) -> bool {
        self.words.first().is_some_and(|w| w.dynamic)
    }
    /// A short option letter, alone or combined (`-rf` has `r`), before `--`.
    pub fn has_short(&self, c: char) -> bool {
        self.args()
            .iter()
            .take_while(|w| w.text != "--")
            .any(|w| is_short_cluster(&w.text) && w.text[1..].contains(c))
    }
    /// A long option, bare or with `=value`, before `--`.
    pub fn has_long(&self, name: &str) -> bool {
        self.args().iter().take_while(|w| w.text != "--").any(|w| {
            w.text == name
                || w.text
                    .strip_prefix(name)
                    .is_some_and(|r| r.starts_with('='))
        })
    }
    /// Any of the given long options, or short letters.
    pub fn has_any(&self, longs: &[&str], shorts: &[char]) -> bool {
        longs.iter().any(|l| self.has_long(l)) || shorts.iter().any(|c| self.has_short(*c))
    }
    /// The value of `--name value` or `--name=value` (the first one).
    pub fn value_of(&self, names: &[&str]) -> Option<&str> {
        let a = self.args();
        for (i, w) in a.iter().enumerate() {
            for n in names {
                if w.text == *n {
                    return a.get(i + 1).map(|v| v.text.as_str());
                }
                if let Some(v) = w.text.strip_prefix(n).and_then(|r| r.strip_prefix('=')) {
                    return Some(v);
                }
                // `-XPOST`: a short option with its value attached.
                if n.len() == 2 && !n.starts_with("--") && w.text.len() > 2 && w.text.starts_with(n)
                {
                    return Some(&w.text[2..]);
                }
            }
        }
        None
    }
    /// Detection (a rule's trigger): could an option-position word carry any of
    /// these long options or short letters? Over-approximates on purpose, as the
    /// parser does for the program word: a long option matches by unique prefix
    /// (git and GNU getopt_long accept unambiguous prefixes; an ambiguous one
    /// errors, so flagging it costs nothing), and an **unquoted** dynamic word
    /// before `--` could expand to any option. A quoted dynamic word is a single
    /// operand, never an option. Never use this for an exemption.
    pub fn opt_detect(&self, longs: &[&str], shorts: &[char]) -> bool {
        self.args().iter().take_while(|w| w.text != "--").any(|w| {
            if w.dynamic {
                return !w.quoted;
            }
            (is_short_cluster(&w.text) && shorts.iter().any(|c| w.text[1..].contains(*c)))
                || longs.iter().any(|l| long_opt_match(&w.text, l, true))
        })
    }

    /// Exact (an exemption, e.g. `--dry-run`, `--staged`): only a literal exact
    /// spelling counts. A dynamic word never satisfies it, and neither does an
    /// abbreviation.
    pub fn opt_exact(&self, longs: &[&str], shorts: &[char]) -> bool {
        self.args().iter().take_while(|w| w.text != "--").any(|w| {
            if w.dynamic {
                return false;
            }
            (is_short_cluster(&w.text) && shorts.iter().any(|c| w.text[1..].contains(*c)))
                || longs.iter().any(|l| long_opt_match(&w.text, l, false))
        })
    }

    /// Positional words (not options), skipping the values of `takes_value`
    /// options; everything after `--` is positional.
    pub fn positionals<'a>(&'a self, takes_value: &[&str]) -> Vec<&'a Word> {
        let mut out = vec![];
        let mut skip = false;
        let mut rest = false;
        for w in self.args() {
            if skip {
                skip = false;
                continue;
            }
            if rest {
                out.push(w);
                continue;
            }
            if w.text == "--" {
                rest = true;
                continue;
            }
            if w.text.starts_with('-') && w.text.len() > 1 {
                if takes_value.contains(&w.text.as_str()) {
                    skip = true;
                }
                continue;
            }
            out.push(w);
        }
        out
    }
}

fn is_short_cluster(s: &str) -> bool {
    s.len() > 1
        && s.starts_with('-')
        && !s.starts_with("--")
        && s[1..].chars().all(|c| c.is_ascii_alphanumeric())
}

/// Does the word `w` (a `--long` or `--long=value`) name the long option
/// `name`? Exact when `!prefix`; when `prefix`, an unambiguous abbreviation
/// counts too: `--forc` names `--force` (at least one character after `--`).
pub fn long_opt_match(w: &str, name: &str, prefix: bool) -> bool {
    let head = w.split('=').next().unwrap_or(w);
    if head == name {
        return true;
    }
    prefix && head.len() > 2 && head.starts_with("--") && name.starts_with(head)
}

/// Every command `argv` would start, run in `cwd`. A direct argv receives its
/// words literally (no shell): no brace expansion, no redirections.
pub fn commands(argv: &[String], cwd: &Path) -> Vec<Cmd> {
    let words: Vec<Word> = argv.iter().map(|a| Word::lit(a)).collect();
    let mut out = vec![];
    expand(
        words,
        Cwds::one(cwd),
        false,
        0,
        argv.join(" ").as_str(),
        &[],
        &mut out,
    );
    out
}

/// Parse shell text into its commands. `via` is the wrapper/shell chain already
/// stepped through; `params` are literal positional parameters for `$1`…, when
/// known (a `bash -c 'script' arg0 args…`).
pub fn parse_script(
    text: &str,
    cwds: &Cwds,
    depth: usize,
    via: &[String],
    params: Option<&[String]>,
    out: &mut Vec<Cmd>,
) {
    if depth > MAX_DEPTH {
        out.push(unparsed(text, cwds, via));
        return;
    }
    match lex(text, params) {
        Ok(toks) => Parser::new(cwds.clone(), depth, via.to_vec(), out).run(toks),
        Err(_) => out.push(unparsed(text, cwds, via)),
    }
}

/// The words of `text` when it is one simple command with no shell syntax
/// around it (no operators, redirections, or heredocs): an argv.
pub fn words_of(text: &str) -> Option<Vec<Word>> {
    let mut out = vec![];
    for t in lex(text, None).ok()? {
        match t {
            Tok::Word(w) => out.push(w),
            _ => return None,
        }
    }
    Some(out)
}

fn unparsed(text: &str, cwds: &Cwds, via: &[String]) -> Cmd {
    Cmd {
        words: vec![Word::lit(text)],
        cwds: cwds.clone(),
        more_operands: false,
        unparsed: true,
        via: via.to_vec(),
        redirs: vec![],
        text: text.into(),
    }
}

// ---------------------------------------------------------------- lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(Word),
    /// `&&`, `||`, `;`, `;;`, `|`, `&`, `(`, `)`, or a newline.
    Op(&'static str),
    /// A file redirection (`>`, `>>`, `<`, `<>`, `&>`, `>|`, `2>file`): the next
    /// word is its target, judged as a path but not run as an argument.
    Redir,
    /// A redirection whose next word is not a file path: a here-string (`<<<`)
    /// body, or an fd duplication (`>&2`, `2>&1`). Consumed, not captured.
    RedirDrop,
    /// `<<` or `<<-`: the next word is a heredoc delimiter.
    HereDoc,
    /// Command substitutions found in a heredoc body.
    Subs(Vec<String>),
}

fn lex(s: &str, params: Option<&[String]>) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let n = c.len();
    let mut i = 0;
    let mut toks = vec![];
    // Heredocs whose bodies start at the next newline: (delimiter, strip tabs, quoted).
    let mut pending: Vec<(String, bool, bool)> = vec![];
    let mut want_delim: Option<bool> = None;
    while i < n {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        match ch {
            ' ' | '\t' | '\r' => i += 1,
            '\\' if next == Some('\n') => i += 2,
            '\n' => {
                toks.push(Tok::Op("\n"));
                i += 1;
                for (delim, strip, quoted) in pending.drain(..) {
                    let mut body = String::new();
                    loop {
                        if i >= n {
                            break;
                        }
                        let end = c[i..].iter().position(|&x| x == '\n').map_or(n, |p| i + p);
                        let line: String = c[i..end].iter().collect();
                        i = (end + 1).min(n);
                        let l = if strip {
                            line.trim_start_matches('\t')
                        } else {
                            line.as_str()
                        };
                        if l == delim {
                            break;
                        }
                        body.push_str(&line);
                        body.push('\n');
                    }
                    if !quoted {
                        toks.push(Tok::Subs(subs_in_text(&body)?));
                    }
                }
            }
            '#' => {
                while i < n && c[i] != '\n' {
                    i += 1;
                }
            }
            '&' if next == Some('&') => {
                toks.push(Tok::Op("&&"));
                i += 2;
            }
            '&' if next == Some('>') => {
                toks.push(Tok::Redir);
                i += 2;
                if c.get(i) == Some(&'>') {
                    i += 1;
                }
            }
            '&' => {
                toks.push(Tok::Op("&"));
                i += 1;
            }
            '|' if next == Some('|') => {
                toks.push(Tok::Op("||"));
                i += 2;
            }
            '|' => {
                toks.push(Tok::Op("|"));
                i += if next == Some('&') { 2 } else { 1 };
            }
            ';' if next == Some(';') => {
                toks.push(Tok::Op(";;"));
                i += 2;
                if c.get(i) == Some(&'&') {
                    i += 1;
                }
            }
            ';' => {
                toks.push(Tok::Op(";"));
                i += if next == Some('&') { 2 } else { 1 };
            }
            '(' => {
                toks.push(Tok::Op("("));
                i += 1;
            }
            ')' => {
                toks.push(Tok::Op(")"));
                i += 1;
            }
            '<' | '>' if next == Some('(') => {
                // Process substitution: a word whose command runs.
                let (inner, end) = balanced(&c, i + 2, '(', ')')?;
                toks.push(Tok::Word(Word {
                    text: c[i..end].iter().collect(),
                    dynamic: true,
                    glob: false,
                    quoted: false,
                    subs: vec![inner],
                }));
                i = end;
            }
            '<' if next == Some('<') && c.get(i + 2) == Some(&'<') => {
                // Here-string `<<<`: the next word is data, not a file.
                toks.push(Tok::RedirDrop);
                i += 3;
            }
            '<' if next == Some('<') => {
                let strip = c.get(i + 2) == Some(&'-');
                toks.push(Tok::HereDoc);
                want_delim = Some(strip);
                i += if strip { 3 } else { 2 };
            }
            '<' | '>' => {
                // `>&` / `<&` is an fd duplication (`2>&1`, `>&2`): drop its word.
                // `>>`, `>|`, `<>` are file redirections: capture the target.
                if c.get(i + 1) == Some(&'&') {
                    toks.push(Tok::RedirDrop);
                    i += 2;
                } else {
                    toks.push(Tok::Redir);
                    i += 1;
                    if matches!(c.get(i), Some('>') | Some('|')) {
                        i += 1;
                    }
                }
            }
            _ => {
                let start = i;
                let (w, end) = lex_word(&c, i, params)?;
                i = end;
                let raw = &c[start..end];
                // `2>` / `10<`: digits right before a redirection are its fd.
                if raw.iter().all(|x| x.is_ascii_digit())
                    && matches!(c.get(i), Some('<') | Some('>'))
                {
                    continue;
                }
                if let Some(strip) = want_delim.take() {
                    // A quoted delimiter means the body is not expanded.
                    let quoted = raw.iter().any(|x| matches!(x, '\'' | '"' | '\\'));
                    pending.push((w.text.clone(), strip, quoted));
                }
                toks.push(Tok::Word(w));
            }
        }
    }
    Ok(toks)
}

const WORD_END: &[char] = &[' ', '\t', '\r', '\n', ';', '&', '|', '(', ')', '<', '>'];

fn lex_word(c: &[char], mut i: usize, params: Option<&[String]>) -> Result<(Word, usize), String> {
    let n = c.len();
    let mut w = Word::default();
    while i < n && !WORD_END.contains(&c[i]) {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        match ch {
            '\\' => {
                w.quoted = true;
                if let Some(x) = next.filter(|x| *x != '\n') {
                    w.text.push(x);
                }
                i = (i + 2).min(n);
            }
            '\'' => {
                w.quoted = true;
                let end = c[i + 1..]
                    .iter()
                    .position(|&x| x == '\'')
                    .map(|p| i + 1 + p)
                    .ok_or("unterminated single quote")?;
                w.text.extend(&c[i + 1..end]);
                i = end + 1;
            }
            '"' => {
                w.quoted = true;
                i = lex_double(c, i + 1, &mut w, params)?;
            }
            '$' => {
                i = lex_dollar(c, i, &mut w, false, params)?;
            }
            '`' => {
                let (inner, end) = backtick(c, i + 1)?;
                w.subs.push(inner);
                w.dynamic = true;
                w.text.push('$');
                i = end;
            }
            '*' | '?' | '[' => {
                w.glob = true;
                w.text.push(ch);
                i += 1;
            }
            _ => {
                w.text.push(ch);
                i += 1;
            }
        }
    }
    Ok((w, i))
}

/// Inside `"…"` from `i`; returns the index after the closing quote.
fn lex_double(
    c: &[char],
    mut i: usize,
    w: &mut Word,
    params: Option<&[String]>,
) -> Result<usize, String> {
    let n = c.len();
    while i < n {
        match c[i] {
            '"' => return Ok(i + 1),
            '\\' => {
                match c.get(i + 1) {
                    Some(x @ ('$' | '`' | '"' | '\\')) => w.text.push(*x),
                    Some('\n') => {}
                    Some(x) => {
                        w.text.push('\\');
                        w.text.push(*x);
                    }
                    None => return Err("unterminated double quote".into()),
                }
                i += 2;
            }
            '$' => i = lex_dollar(c, i, w, true, params)?,
            '`' => {
                let (inner, end) = backtick(c, i + 1)?;
                w.subs.push(inner);
                w.dynamic = true;
                w.text.push('$');
                i = end;
            }
            x => {
                w.text.push(x);
                i += 1;
            }
        }
    }
    Err("unterminated double quote".into())
}

/// A `$` at `i`: `$'…'`, `$((…))`, `$(…)`, `${…}`, `$name`, or a lone `$`.
/// Inside double quotes `$'` and `$"` are a literal `$`. `params` are the
/// positional parameters of a `bash -c 'script' arg0 args…` when they are all
/// literal: `$1`…`$9`, `$@`, and `$*` are substituted, so the script is judged
/// exactly; without them these stay dynamic. A simple `$name`/`${name}` keeps
/// its spelling in `text` (still dynamic) so a path judgment can read `$HOME`.
fn lex_dollar(
    c: &[char],
    i: usize,
    w: &mut Word,
    in_double: bool,
    params: Option<&[String]>,
) -> Result<usize, String> {
    let next = c.get(i + 1).copied();
    match next {
        Some('\'') if !in_double => {
            // ANSI-C quoting: backslash escapes, no expansion.
            w.quoted = true;
            let mut j = i + 2;
            loop {
                match c.get(j) {
                    None => return Err("unterminated $'".into()),
                    Some('\'') => return Ok(j + 1),
                    Some('\\') => {
                        let e = c.get(j + 1).copied().ok_or("unterminated $'")?;
                        w.text.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            '0' => '\0',
                            other => other,
                        });
                        j += 2;
                    }
                    Some(x) => {
                        w.text.push(*x);
                        j += 1;
                    }
                }
            }
        }
        Some('(') if c.get(i + 2) == Some(&'(') => {
            // Arithmetic: no command runs, but the value is unknown.
            let (_, end) = balanced(c, i + 2, '(', ')')?;
            w.dynamic = true;
            w.text.push('$');
            Ok(end)
        }
        Some('(') => {
            let (inner, end) = balanced(c, i + 2, '(', ')')?;
            w.subs.push(inner);
            w.dynamic = true;
            w.text.push('$');
            Ok(end)
        }
        Some('{') => {
            let (inner, end) = balanced(c, i + 2, '{', '}')?;
            // `${1}`, `${@}` substitute like the bare forms when params are known.
            if let Some(v) = params.and_then(|p| positional(&inner, p)) {
                w.text.push_str(&v);
                return Ok(end);
            }
            // `${x:-$(cmd)}` runs cmd.
            w.subs.extend(subs_in_text(&inner)?);
            w.dynamic = true;
            w.text.push_str(&format!("${{{inner}}}"));
            Ok(end)
        }
        Some(x) if x.is_ascii_alphanumeric() || x == '_' => {
            let mut j = i + 1;
            if x.is_ascii_digit() {
                j += 1;
            } else {
                while c
                    .get(j)
                    .is_some_and(|y| y.is_ascii_alphanumeric() || *y == '_')
                {
                    j += 1;
                }
            }
            let name: String = c[i + 1..j].iter().collect();
            if let Some(v) = params.and_then(|p| positional(&name, p)) {
                w.text.push_str(&v);
            } else {
                w.dynamic = true;
                w.text.push('$');
                w.text.push_str(&name);
            }
            Ok(j)
        }
        Some(sym @ ('@' | '*')) => {
            if let Some(p) = params {
                w.text.push_str(&p.join(" "));
            } else {
                w.dynamic = true;
                w.text.push('$');
                w.text.push(sym);
            }
            Ok(i + 2)
        }
        Some(sym @ ('#' | '?' | '$' | '!' | '-')) => {
            w.dynamic = true;
            w.text.push('$');
            w.text.push(sym);
            Ok(i + 2)
        }
        _ => {
            w.text.push('$');
            Ok(i + 1)
        }
    }
}

/// A positional parameter reference resolved against literal args: `1`…`9` →
/// that arg (empty past the end), `@`/`*` → all args joined. `None` for a name
/// that is not positional, so the caller keeps it dynamic.
fn positional(name: &str, params: &[String]) -> Option<String> {
    if name == "@" || name == "*" {
        return Some(params.join(" "));
    }
    let n: usize = name.parse().ok()?;
    if n == 0 {
        return None; // $0 is the script name, not a positional we track.
    }
    Some(params.get(n - 1).cloned().unwrap_or_default())
}

/// From just after an opening `open`, find its match; returns the inner text
/// and the index after the close. Quotes and nested substitutions are skipped.
fn balanced(c: &[char], mut i: usize, open: char, close: char) -> Result<(String, usize), String> {
    let start = i;
    let mut depth = 1usize;
    while i < c.len() {
        match c[i] {
            '\\' => i += 2,
            '\'' => {
                i = c[i + 1..]
                    .iter()
                    .position(|&x| x == '\'')
                    .map(|p| i + 2 + p)
                    .ok_or("unterminated single quote")?;
            }
            '"' => {
                let mut scratch = Word::default();
                i = lex_double(c, i + 1, &mut scratch, None)?;
            }
            '`' => i = backtick(c, i + 1)?.1,
            x if x == open => {
                depth += 1;
                i += 1;
            }
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Ok((c[start..i].iter().collect(), i + 1));
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    Err(format!("unbalanced {open}"))
}

fn backtick(c: &[char], mut i: usize) -> Result<(String, usize), String> {
    let mut inner = String::new();
    while i < c.len() {
        match c[i] {
            '`' => return Ok((inner, i + 1)),
            '\\' if matches!(c.get(i + 1), Some('`' | '\\' | '$')) => {
                inner.push(c[i + 1]);
                i += 2;
            }
            x => {
                inner.push(x);
                i += 1;
            }
        }
    }
    Err("unterminated backtick".into())
}

/// Command substitutions in expanded text (a heredoc body, `${…}`).
fn subs_in_text(s: &str) -> Result<Vec<String>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\\' => i += 2,
            '$' if c.get(i + 1) == Some(&'(') && c.get(i + 2) != Some(&'(') => {
                let (inner, end) = balanced(&c, i + 2, '(', ')')?;
                out.push(inner);
                i = end;
            }
            '`' => {
                let (inner, end) = backtick(&c, i + 1)?;
                out.push(inner);
                i = end;
            }
            _ => i += 1,
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- parser

struct Parser<'a> {
    cwds: Cwds,
    /// The candidates at the start of the current and-or list.
    list_start: Cwds,
    /// Saved at each `(`, unioned back at its `)`.
    stack: Vec<Cwds>,
    depth: usize,
    /// The wrapper/shell chain this script was reached through.
    via: Vec<String>,
    out: &'a mut Vec<Cmd>,
    cur: Vec<Word>,
    text: Vec<String>,
    /// The current simple command's file-redirection targets.
    redirs: Vec<Word>,
    /// A `cd` just finished: where it leads, applied at the next operator.
    pending_cd: Option<Cwds>,
}

impl<'a> Parser<'a> {
    fn new(cwds: Cwds, depth: usize, via: Vec<String>, out: &'a mut Vec<Cmd>) -> Self {
        Parser {
            list_start: cwds.clone(),
            cwds,
            stack: vec![],
            depth,
            via,
            out,
            cur: vec![],
            text: vec![],
            redirs: vec![],
            pending_cd: None,
        }
    }

    fn run(mut self, toks: Vec<Tok>) {
        // `Capture`: the next word is a file-redirection target to keep.
        // `Drop`: the next word is a heredoc delimiter, here-string, or fd dup.
        #[derive(PartialEq)]
        enum Redir {
            None,
            Capture,
            Drop,
        }
        let mut redir = Redir::None;
        for t in toks {
            match t {
                Tok::Word(w) => {
                    for s in &w.subs {
                        parse_script(s, &self.cwds, self.depth + 1, &self.via, None, self.out);
                    }
                    match std::mem::replace(&mut redir, Redir::None) {
                        Redir::Capture => self.redirs.push(w),
                        Redir::Drop => {}
                        Redir::None => self.push_word(w),
                    }
                }
                Tok::Redir => redir = Redir::Capture,
                Tok::RedirDrop | Tok::HereDoc => redir = Redir::Drop,
                Tok::Subs(subs) => {
                    for s in &subs {
                        parse_script(s, &self.cwds, self.depth + 1, &self.via, None, self.out);
                    }
                }
                Tok::Op(op) => {
                    self.finish();
                    self.operator(op);
                }
            }
        }
        self.finish();
    }

    /// Add a word to the current command, brace-expanding an unquoted `{a,b}` or
    /// `{1..3}` into several words first (a direct argv never reaches here).
    fn push_word(&mut self, w: Word) {
        for x in brace_expand_word(w) {
            self.text.push(x.text.clone());
            self.cur.push(x);
        }
    }

    fn operator(&mut self, op: &str) {
        if let Some(to) = self.pending_cd.take() {
            if op == "&&" {
                self.cwds = to;
            } else {
                self.cwds.union(&to);
            }
        }
        match op {
            "&&" => {}
            "||" | "|" => self.cwds.union(&self.list_start.clone()),
            "(" => {
                self.stack.push(self.cwds.clone());
                self.list_start = self.cwds.clone();
            }
            ")" => {
                if let Some(saved) = self.stack.pop() {
                    self.cwds.union(&saved);
                }
                self.cwds.union(&self.list_start.clone());
                self.list_start = self.cwds.clone();
            }
            _ => {
                self.cwds.union(&self.list_start.clone());
                self.list_start = self.cwds.clone();
            }
        }
    }

    fn finish(&mut self) {
        let mut words = std::mem::take(&mut self.cur);
        let text = std::mem::take(&mut self.text).join(" ");
        let redirs = std::mem::take(&mut self.redirs);
        // Reserved words in command position, and the headers of compound
        // commands whose own words are not commands.
        loop {
            let Some(first) = words.first() else { return };
            if first.dynamic {
                break;
            }
            match first.text.as_str() {
                "if" | "then" | "else" | "elif" | "fi" | "do" | "done" | "while" | "until"
                | "!" | "{" | "}" | "esac" | "coproc" => {
                    words.remove(0);
                }
                "time" => {
                    words.remove(0);
                    if words.first().is_some_and(|w| w.text == "-p") {
                        words.remove(0);
                    }
                }
                "function" => {
                    words.drain(..words.len().min(2));
                }
                "for" | "select" | "case" | "in" => return,
                _ => break,
            }
        }
        // Leading assignments: `FOO=1 cmd`.
        while words.first().is_some_and(|w| is_assignment(&w.text)) {
            words.remove(0);
        }
        if words.is_empty() {
            return;
        }
        match words[0].text.as_str() {
            "cd" | "pushd" if !words[0].dynamic => {
                let target = words
                    .iter()
                    .skip(1)
                    .find(|w| !(w.text.starts_with('-') && w.text.len() > 1))
                    .cloned()
                    .unwrap_or_else(|| Word::lit("~"));
                self.pending_cd = Some(self.cwds.cd(&target));
                return;
            }
            "popd" if !words[0].dynamic => {
                self.pending_cd = Some(Cwds::unknown());
                return;
            }
            _ => {}
        }
        let mark = self.out.len();
        expand(
            words,
            self.cwds.clone(),
            false,
            self.depth,
            &text,
            &self.via,
            self.out,
        );
        // The redirection targets belong to this simple command. Over-approximate
        // by attaching them to every command it expanded to (through wrappers or a
        // `bash -c` string), so the floor and deny list judge them wherever they
        // could be written.
        if !redirs.is_empty() {
            for c in &mut self.out[mark..] {
                c.redirs.extend(redirs.iter().cloned());
            }
        }
    }
}

/// Brace-expand one unquoted word (`--{force,}` → `--force`, `--`). A quoted or
/// dynamic word, or one without a brace, is returned unchanged. Past the cap the
/// word becomes dynamic (opaque), never silently one literal.
fn brace_expand_word(w: Word) -> Vec<Word> {
    if w.dynamic || w.quoted || !w.text.contains('{') {
        return vec![w];
    }
    match brace_expand(&w.text) {
        Some(list) if list.len() > 1 => list
            .into_iter()
            .map(|t| {
                let glob = t.contains(['*', '?', '[']);
                Word {
                    text: t,
                    glob,
                    ..Default::default()
                }
            })
            .collect(),
        Some(_) => vec![w],
        None => vec![Word { dynamic: true, ..w }],
    }
}

/// Cartesian brace expansion of `{a,b}` alternatives and `{m..n}` numeric
/// ranges, capped at `BRACE_CAP` results (`None` past the cap). A group with no
/// comma and no range stays literal, as the shell leaves it.
const BRACE_CAP: usize = 64;

fn brace_expand(s: &str) -> Option<Vec<String>> {
    let mut out = vec![];
    expand_braces(s, &mut out)?;
    Some(out)
}

fn expand_braces(s: &str, out: &mut Vec<String>) -> Option<()> {
    let c: Vec<char> = s.chars().collect();
    // The first top-level `{ … }` with a comma or a `..` range.
    let Some(open) = c.iter().position(|&x| x == '{') else {
        push_capped(out, s.to_string())?;
        return Some(());
    };
    let mut depth = 0usize;
    let mut close = None;
    let mut alts: Vec<String> = vec![];
    let mut cur = String::new();
    for (k, &ch) in c.iter().enumerate().skip(open) {
        match ch {
            '{' => {
                depth += 1;
                if depth > 1 {
                    cur.push(ch);
                }
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(k);
                    alts.push(std::mem::take(&mut cur));
                    break;
                }
                cur.push(ch);
            }
            ',' if depth == 1 => alts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    let Some(close) = close else {
        // Unbalanced: leave it literal.
        push_capped(out, s.to_string())?;
        return Some(());
    };
    let prefix: String = c[..open].iter().collect();
    let suffix: String = c[close + 1..].iter().collect();
    let fields = if alts.len() == 1 {
        // No top-level comma: a `{m..n}` range, else a literal group.
        match numeric_range(&alts[0]) {
            Some(r) => r,
            None => {
                push_capped(out, format!("{prefix}{{{}}}{suffix}", alts[0]))?;
                return Some(());
            }
        }
    } else {
        alts
    };
    for f in fields {
        expand_braces(&format!("{prefix}{f}{suffix}"), out)?;
    }
    Some(())
}

fn numeric_range(s: &str) -> Option<Vec<String>> {
    let (a, b) = s.split_once("..")?;
    let a: i64 = a.parse().ok()?;
    let b: i64 = b.parse().ok()?;
    let range: Vec<String> = if a <= b {
        (a..=b).map(|n| n.to_string()).collect()
    } else {
        (b..=a).rev().map(|n| n.to_string()).collect()
    };
    (range.len() <= BRACE_CAP).then_some(range)
}

fn push_capped(out: &mut Vec<String>, s: String) -> Option<()> {
    if out.len() >= BRACE_CAP {
        return None;
    }
    out.push(s);
    Some(())
}

fn is_assignment(s: &str) -> bool {
    let Some((name, _)) = s.split_once('=') else {
        return false;
    };
    let name = name.strip_suffix('+').unwrap_or(name);
    let name = name.split('[').next().unwrap_or(name);
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------- wrappers

const SHELLS: &[&str] = &["bash", "sh", "dash", "zsh", "ksh", "mksh", "ash", "yash"];

/// Programs that start another program, and which of their options take a value.
fn wrapper_opts(p: &str) -> Option<&'static [&'static str]> {
    Some(match p {
        "nohup" | "command" | "builtin" | "setsid" | "chronic" | "unbuffer" | "caffeinate" => &[],
        "exec" => &["-a"],
        "nice" => &["-n", "--adjustment"],
        "ionice" => &["-c", "-n", "--class", "--classdata"],
        "stdbuf" => &["-i", "-o", "-e", "--input", "--output", "--error"],
        "sudo" => &[
            "-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-U", "-T", "--user", "--group",
            "--chdir", "--prompt", "--host",
        ],
        "doas" => &["-u", "-C"],
        "xargs" => &[
            "-a",
            "-d",
            "-E",
            "-e",
            "-I",
            "-L",
            "-l",
            "-n",
            "-P",
            "-s",
            "--arg-file",
            "--delimiter",
            "--max-args",
            "--max-procs",
            "--max-chars",
            "--process-slot-var",
            "--replace",
        ],
        _ => return None,
    })
}

/// See through shells, wrappers, `eval`, `find -exec`, and `ssh` to the
/// commands they start; emit what remains. `via` is the wrapper/shell chain
/// already stepped through, recorded on every command so the floor and deny list
/// can judge a wrapper (`sudo`) the expander otherwise drops.
fn expand(
    mut words: Vec<Word>,
    cwds: Cwds,
    more_operands: bool,
    depth: usize,
    text: &str,
    via: &[String],
    out: &mut Vec<Cmd>,
) {
    if words.is_empty() {
        return;
    }
    if depth > MAX_DEPTH {
        out.push(unparsed(text, &cwds, via));
        return;
    }
    // The program is its file name: `/usr/bin/git` is `git`. A relative path
    // (`./x.sh`, `scripts/gate.sh`) stays as written: it is a script.
    if !words[0].dynamic {
        let p = &words[0].text;
        if p.starts_with('/') {
            if let Some(base) = Path::new(p.as_str()).file_name() {
                words[0].text = base.to_string_lossy().into_owned();
            }
        }
    }
    let prog = words[0].text.clone();
    let emit = |words: Vec<Word>, cwds: Cwds, more: bool, out: &mut Vec<Cmd>| {
        out.push(Cmd {
            words,
            cwds,
            more_operands: more,
            unparsed: false,
            via: via.to_vec(),
            redirs: vec![],
            text: text.into(),
        })
    };
    // Stepping through this program to what it starts: it joins `via`.
    let via_with = |p: &str| {
        let mut v = via.to_vec();
        v.push(p.to_string());
        v
    };
    if words[0].dynamic {
        emit(words, cwds, more_operands, out);
        return;
    }
    if SHELLS.contains(&prog.as_str()) {
        // `bash [-opts] -c 'string' [$0 args…]`, else a script or stdin.
        let mut c_mode = false;
        let mut i = 1;
        while i < words.len() {
            let t = &words[i].text;
            if t == "--" || t == "-" {
                i += 1;
                break;
            }
            if t == "-o"
                || t == "+o"
                || t == "-O"
                || t == "+O"
                || t == "--rcfile"
                || t == "--init-file"
            {
                i += 2;
                continue;
            }
            if t.starts_with("--") {
                i += 1;
                continue;
            }
            if (t.starts_with('-') || t.starts_with('+')) && t.len() > 1 {
                if t[1..].contains('c') {
                    c_mode = true;
                }
                // `-euo pipefail`: an `o` in a cluster takes the next word.
                i += if t[1..].contains(['o', 'O']) { 2 } else { 1 };
                continue;
            }
            break;
        }
        if c_mode {
            match words.get(i) {
                Some(w) if !w.dynamic => {
                    // `bash -c 'script' arg0 args…`: the words after the script
                    // are `$0` and the positional parameters `$1`…, substituted
                    // when every one is literal, so the script is judged exactly.
                    let script = w.text.clone();
                    let rest = &words[i + 1..];
                    let params: Option<Vec<String>> =
                        if !rest.is_empty() && rest.iter().all(|w| !w.dynamic) {
                            Some(rest.iter().skip(1).map(|w| w.text.clone()).collect())
                        } else {
                            None
                        };
                    parse_script(
                        &script,
                        &cwds,
                        depth + 1,
                        &via_with(&prog),
                        params.as_deref(),
                        out,
                    );
                }
                // `bash -c "$X"`: the script is not known.
                _ => emit(words, cwds, more_operands, out),
            }
        } else {
            // A script file, or code on stdin: the rules see neither.
            emit(words, cwds, more_operands, out);
        }
        return;
    }
    if prog == "env" {
        let mut i = 1;
        let mut cwds = cwds;
        while i < words.len() {
            let t = words[i].text.clone();
            if t == "--" {
                i += 1;
                break;
            }
            if t == "-S" || t == "--split-string" {
                // `env -S 'cmd args'`: the string is split into a command.
                if let Some(w) = words.get(i + 1) {
                    let mut rest = w.text.clone();
                    for x in &words[i + 2..] {
                        rest.push(' ');
                        rest.push_str(&x.text);
                    }
                    parse_script(&rest, &cwds, depth + 1, &via_with(&prog), None, out);
                }
                return;
            }
            if t == "-C" || t == "--chdir" {
                if let Some(d) = words.get(i + 1) {
                    cwds = cwds.cd(d);
                }
                i += 2;
                continue;
            }
            if t == "-u" || t == "--unset" {
                i += 2;
                continue;
            }
            if t.starts_with('-') && t.len() > 1 {
                i += 1;
                continue;
            }
            if is_assignment(&t) {
                i += 1;
                continue;
            }
            break;
        }
        expand(
            words.split_off(i.min(words.len())),
            cwds,
            more_operands,
            depth + 1,
            text,
            &via_with(&prog),
            out,
        );
        return;
    }
    if prog == "timeout" {
        // Options, then the duration, then the command.
        let mut i = 1;
        while i < words.len() && words[i].text.starts_with('-') && words[i].text.len() > 1 {
            let t = &words[i].text;
            i += if t == "-s" || t == "-k" || t == "--signal" || t == "--kill-after" {
                2
            } else {
                1
            };
        }
        expand(
            words.split_off((i + 1).min(words.len())),
            cwds,
            more_operands,
            depth + 1,
            text,
            &via_with(&prog),
            out,
        );
        return;
    }
    if prog == "watch" {
        let mut i = 1;
        while i < words.len() && words[i].text.starts_with('-') {
            let t = &words[i].text;
            i += if t == "-n" || t == "-d" || t == "--interval" || t == "-q" || t == "--equexit" {
                2
            } else {
                1
            };
        }
        let s: Vec<&str> = words[i.min(words.len())..]
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        parse_script(&s.join(" "), &cwds, depth + 1, &via_with(&prog), None, out);
        return;
    }
    if prog == "eval" {
        // Opaque in itself, and its text is still read for what it names.
        let s: Vec<&str> = words[1..].iter().map(|w| w.text.as_str()).collect();
        let joined = s.join(" ");
        emit(words, cwds.clone(), more_operands, out);
        parse_script(&joined, &cwds, depth + 1, &via_with(&prog), None, out);
        return;
    }
    if prog == "ssh" {
        // `ssh [opts] host [command…]`: the command runs on another machine.
        const V: &[&str] = &[
            "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o",
            "-p", "-Q", "-R", "-S", "-W", "-w",
        ];
        let mut i = 1;
        while i < words.len() && words[i].text.starts_with('-') {
            i += if V.contains(&words[i].text.as_str()) {
                2
            } else {
                1
            };
        }
        if words.len() > i + 1 {
            let s: Vec<&str> = words[i + 1..].iter().map(|w| w.text.as_str()).collect();
            let joined = s.join(" ");
            emit(words, cwds, more_operands, out);
            parse_script(
                &joined,
                &Cwds::unknown(),
                depth + 1,
                &via_with(&prog),
                None,
                out,
            );
        }
        return;
    }
    if prog == "find" {
        // Starting points come before the first expression word.
        let mut roots = vec![];
        let mut i = 1;
        while i < words.len() {
            let t = &words[i].text;
            if t.starts_with('-') || t == "(" || t == "!" || t == "," {
                break;
            }
            roots.push(words[i].clone());
            i += 1;
        }
        if roots.is_empty() {
            roots.push(Word::lit("."));
        }
        let mut j = i;
        while j < words.len() {
            let t = words[j].text.as_str();
            if matches!(t, "-exec" | "-execdir" | "-ok" | "-okdir") {
                let start = j + 1;
                let mut k = start;
                while k < words.len() && words[k].text != ";" && words[k].text != "+" {
                    k += 1;
                }
                // `{}` is each found path: stand the starting points in for it.
                let mut inner = vec![];
                for w in &words[start..k] {
                    if w.text.contains("{}") {
                        inner.extend(roots.iter().cloned());
                    } else {
                        inner.push(w.clone());
                    }
                }
                expand(
                    inner,
                    cwds.clone(),
                    true,
                    depth + 1,
                    text,
                    &via_with(&prog),
                    out,
                );
                j = k + 1;
                continue;
            }
            j += 1;
        }
        emit(words, cwds, more_operands, out);
        return;
    }
    if let Some(takes) = wrapper_opts(&prog) {
        let mut i = 1;
        while i < words.len() {
            let t = &words[i].text;
            if t == "--" {
                i += 1;
                break;
            }
            if t.starts_with('-') && t.len() > 1 {
                i += if takes.contains(&t.as_str()) { 2 } else { 1 };
                continue;
            }
            break;
        }
        // `sudo -s` / `sudo -i` with no command: a shell.
        if i >= words.len() {
            if prog == "sudo" && words.iter().any(|w| w.text == "-s" || w.text == "-i") {
                out.push(Cmd {
                    words: vec![Word::lit("sh")],
                    cwds,
                    more_operands,
                    unparsed: false,
                    via: via_with(&prog),
                    redirs: vec![],
                    text: text.into(),
                });
            }
            return;
        }
        let more = more_operands || prog == "xargs";
        expand(
            words.split_off(i),
            cwds,
            more,
            depth + 1,
            text,
            &via_with(&prog),
            out,
        );
        return;
    }
    emit(words, cwds, more_operands, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progs(s: &str) -> Vec<String> {
        let mut out = vec![];
        parse_script(s, &Cwds::one(Path::new("/w")), 0, &[], None, &mut out);
        out.iter()
            .map(|c| {
                let words: Vec<&str> = c.words.iter().map(|w| w.text.as_str()).collect();
                format!("{}{}", if c.unparsed { "!" } else { "" }, words.join(" "))
            })
            .collect()
    }

    #[test]
    fn finds_every_command_a_string_would_run() {
        assert_eq!(
            progs("git push -f origin main"),
            vec!["git push -f origin main"]
        );
        assert_eq!(
            progs("cd x && git add . && git commit -m 'a && b' || echo no; ls | wc -l"),
            vec![
                "git add .",
                "git commit -m a && b",
                "echo no",
                "ls",
                "wc -l"
            ]
        );
        assert_eq!(
            progs("echo $(git push -f) `rm -rf a`"),
            vec!["git push -f", "rm -rf a", "echo $ $"]
        );
        assert_eq!(
            progs("(git push --force) & wait"),
            vec!["git push --force", "wait"]
        );
        assert_eq!(
            progs("if true; then git push -f; fi"),
            vec!["true", "git push -f"]
        );
        assert_eq!(
            progs("for f in a b; do rm -rf \"$f\"; done"),
            vec!["rm -rf $f"],
            "a simple $name keeps its spelling (still dynamic)"
        );
        assert_eq!(
            progs("FOO=1 BAR=2 git push -f 2>&1 >/dev/null"),
            vec!["git push -f"]
        );
        assert_eq!(
            progs("env -i A=1 nohup timeout 5 git push -f"),
            vec!["git push -f"]
        );
        assert_eq!(progs("sudo -u me xargs rm -rf < list"), vec!["rm -rf"]);
        assert_eq!(
            progs("bash -lc \"sh -c 'git push -f'\""),
            vec!["git push -f"]
        );
        assert_eq!(progs("/usr/bin/git push"), vec!["git push"]);
        assert_eq!(progs("./deploy.sh now"), vec!["./deploy.sh now"]);
        assert_eq!(
            progs("f() { git push -f; }; f"),
            vec!["f", "git push -f", "f"]
        );
        assert_eq!(
            progs("case $x in a) git push -f ;; esac"),
            vec!["git push -f"]
        );
        assert_eq!(
            progs("cat <<EOF\n$(git push -f)\nEOF\nls"),
            vec!["cat", "git push -f", "ls"]
        );
        assert_eq!(progs("cat <<'EOF'\n$(git push -f)\nEOF"), vec!["cat"]);
        assert_eq!(
            progs("diff <(git push -f) b"),
            vec!["git push -f", "diff <(git push -f) b"]
        );
        assert_eq!(
            progs("bash -euo pipefail -c 'git push -f'"),
            vec!["git push -f"]
        );
        assert_eq!(
            progs("echo \"$'\" ; git push -f; echo \"'\""),
            vec!["echo $'", "git push -f", "echo '"],
            "`$'` inside double quotes is literal and hides nothing"
        );
        assert_eq!(
            progs("eval 'git push -f'"),
            vec!["eval git push -f", "git push -f"]
        );
        assert_eq!(
            progs("find . -name x -exec rm -rf {} +"),
            vec!["rm -rf .", "find . -name x -exec rm -rf {} +"]
        );
        assert_eq!(
            progs("ssh host 'rm -rf /data'"),
            vec!["ssh host rm -rf /data", "rm -rf /data"]
        );
        assert_eq!(progs("echo 'unterminated"), vec!["!echo 'unterminated"]);
        assert_eq!(progs("git push # --force"), vec!["git push"]);
        assert_eq!(
            progs("echo a\\\n && git push -f"),
            vec!["echo a", "git push -f"]
        );
    }

    #[test]
    fn a_cd_moves_later_commands_and_a_failure_keeps_the_old_directory_possible() {
        let cmds = |s: &str| {
            let mut out = vec![];
            parse_script(s, &Cwds::one(Path::new("/w")), 0, &[], None, &mut out);
            out
        };
        let dirs = |c: &Cmd| {
            c.cwds
                .dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
        };
        let c = cmds("cd web && rm -rf node_modules");
        assert_eq!(dirs(&c[0]), vec!["/w/web"]);
        let c = cmds("cd web; rm -rf node_modules");
        assert_eq!(
            dirs(&c[0]),
            vec!["/w", "/w/web"],
            "a failed cd leaves the old directory"
        );
        let c = cmds("(cd /tmp/x && rm -rf a); rm -rf b");
        assert_eq!(dirs(&c[0]), vec!["/tmp/x"]);
        assert!(dirs(&c[1]).contains(&"/w".to_string()), "{:?}", dirs(&c[1]));
        let c = cmds("cd \"$D\" && rm -rf a");
        assert!(c[0].cwds.unknown);
        let c = cmds("env -C sub rm -rf a");
        assert_eq!(dirs(&c[0]), vec!["/w/sub"]);
    }

    #[test]
    fn options_and_positionals() {
        let mut out = vec![];
        parse_script(
            "git push -fu origin +main --push-option=x",
            &Cwds::one(Path::new("/")),
            0,
            &[],
            None,
            &mut out,
        );
        let c = &out[0];
        assert!(c.has_short('f') && c.has_short('u') && !c.has_short('d'));
        assert!(c.has_long("--push-option") && !c.has_long("--push"));
        let p: Vec<&str> = c.positionals(&[]).iter().map(|w| w.text.as_str()).collect();
        assert_eq!(p, vec!["push", "origin", "+main"]);
        let mut out = vec![];
        parse_script(
            "curl -XPOST https://x -d a",
            &Cwds::one(Path::new("/")),
            0,
            &[],
            None,
            &mut out,
        );
        assert_eq!(out[0].value_of(&["-X", "--request"]), Some("POST"));
    }
}
