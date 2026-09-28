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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
    /// exactly one word: it never brace-expands, and it never word-splits. It
    /// may still be an option: quoting stops word splitting, not option parsing,
    /// so a quoted dynamic word is one word that could be any single option
    /// (`git reset "$m"`). Detection reflects that; see `opt_detect`.
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
    /// Variable references resolved from the script's literal assignments, as
    /// `(spelling, value)` pairs (`$F` → `--force`), so a notice or refusal can
    /// show where each resolved value came from (spec §3.9, step 2a.3).
    pub resolved: Vec<(String, String)>,
    /// Where it came from, as written, for the notice.
    pub text: String,
}

impl Cmd {
    pub fn prog(&self) -> &str {
        self.words.first().map(|w| w.text.as_str()).unwrap_or("")
    }
    /// The command as the rules read it: its resolved words, or the raw text when
    /// the parser could not see through it.
    pub fn display(&self) -> String {
        if self.unparsed {
            self.text.clone()
        } else {
            self.words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
    pub fn args(&self) -> &[Word] {
        self.words.get(1..).unwrap_or(&[])
    }
    /// The program word is built from an expansion.
    pub fn dynamic_program(&self) -> bool {
        self.words.first().is_some_and(|w| w.dynamic)
    }
    /// The program word's full spelling, including any command substitutions
    /// inside it, so the floor can scan a dynamic program word (`$(echo op)`,
    /// `` `which theseusd` ``, `$x$y`) for a floor program name (spec §3.9).
    pub fn program_spelling(&self) -> String {
        match self.words.first() {
            Some(w) => {
                let mut s = w.text.clone();
                for sub in &w.subs {
                    s.push(' ');
                    s.push_str(sub);
                }
                s
            }
            None => String::new(),
        }
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
    /// errors, so flagging it costs nothing), and **any dynamic word** before
    /// `--` could expand to this option. Quoting stops word splitting, not option
    /// parsing: a quoted dynamic word is exactly one word, but that one word may
    /// be any single option (`m=--hard; git reset "$m"`). A rule that must stay
    /// quiet when the dynamic word is the *only* operand (e.g. `rm "$f"`, one
    /// file with no `-r`) guards that itself; see `rm_recursive`. Never use this
    /// for an exemption.
    pub fn opt_detect(&self, longs: &[&str], shorts: &[char]) -> bool {
        self.args().iter().take_while(|w| w.text != "--").any(|w| {
            if w.dynamic {
                return true;
            }
            (is_short_cluster(&w.text) && shorts.iter().any(|c| w.text[1..].contains(*c)))
                || longs.iter().any(|l| long_opt_match(&w.text, l, true))
        })
    }

    /// Is there a dynamic word before `--`? A dynamic word could expand to any
    /// option, so a rule whose danger is purely an option (`git reset --hard`,
    /// `git clean -f`) must treat one as a possible trigger. Distinct from
    /// `opt_detect` only in intent: this asks "could an option hide here?"
    pub fn has_dynamic_option(&self) -> bool {
        self.args()
            .iter()
            .take_while(|w| w.text != "--")
            .any(|w| w.dynamic)
    }

    /// A *certain* option match. It answers "is this option definitely present?",
    /// where `opt_detect` also says "yes" for a dynamic word that merely could be
    /// it. Used where a rule must separate a certain flag from a possible one
    /// (`rm_recursive`: is `-r` definitely here, or only maybe in a variable?). A
    /// dynamic word's literal prefix counts (`-r$(printf f)` certainly has `-r`),
    /// but a word that begins with a reference (`$x`) does not.
    pub fn opt_literal(&self, longs: &[&str], shorts: &[char]) -> bool {
        self.args().iter().take_while(|w| w.text != "--").any(|w| {
            let head = literal_head(w);
            (is_short_cluster(head) && shorts.iter().any(|c| head[1..].contains(*c)))
                || longs.iter().any(|l| long_opt_match(head, l, true))
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

/// The certain leading option text of a word: the whole word when literal, or the
/// prefix before the first `$` of a dynamic word (`-r$(printf f)` certainly
/// carries `-r`; `--hard$x` certainly `--hard`). Empty when a dynamic word starts
/// with a reference (`$x`), which carries no certain option (spec §3.9, step 2a.3).
fn literal_head(w: &Word) -> &str {
    if w.dynamic {
        w.text.split('$').next().unwrap_or("")
    } else {
        &w.text
    }
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
        None,
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
    parse_script_in(text, cwds, depth, via, params, None, out)
}

/// `parse_script`, threading the enclosing scope's literal assignments `parent`
/// so a subshell or `eval` can resolve a variable defined outside it (spec §3.9,
/// step 2a.3). A `bash -c` string passes `None`: it is a fresh shell, and the
/// outer shell already expanded its argument.
#[allow(clippy::too_many_arguments)]
fn parse_script_in(
    text: &str,
    cwds: &Cwds,
    depth: usize,
    via: &[String],
    params: Option<&[String]>,
    parent: Option<Rc<VarCtx>>,
    out: &mut Vec<Cmd>,
) {
    if depth > MAX_DEPTH {
        out.push(unparsed(text, cwds, via));
        return;
    }
    match lex(text, params) {
        Ok(toks) => {
            let vars = collect_vars(&toks, parent);
            Parser::new(cwds.clone(), depth, via.to_vec(), out, vars).run(toks)
        }
        Err(_) => out.push(unparsed(text, cwds, via)),
    }
}

/// How far a name is resolved from the script's literal assignments (spec §3.9,
/// step 2a.3). A name absent from the map, or mapped to `None`, is unresolvable
/// and stays dynamic; the environment is unknown (except `HOME`, resolved for
/// path judgments only, which is never in this map unless the script assigns it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarVal {
    /// A scalar's candidate values (the union across branches).
    Scalar(Vec<String>),
    /// An array's candidate element-lists; only literal elements (an element with
    /// a substitution, a glob, or another reference poisons the array).
    Array(Vec<Vec<String>>),
}

/// A script's literal assignments, for resolving simple variable references
/// everywhere they appear. Built once per parsed script (`collect_vars`); a
/// subshell or `eval` links to its enclosing scope through `parent` (a cheap
/// reference-count, not a copy), and a name it does not define is looked up
/// there. A `bash -c` string is a fresh scope (no parent), because the outer
/// shell expands its argument before it is re-parsed.
#[derive(Debug)]
pub struct VarCtx {
    /// name → its values, or `None` when any assignment to it is not literal.
    vars: HashMap<String, Option<VarVal>>,
    /// The enclosing scope, for a subshell or `eval`.
    parent: Option<Rc<VarCtx>>,
    /// The positional parameters (`$1`…, `$@`, `$*`), or `None` when unknown: a
    /// dynamic `set --`, a `shift`, or a function in the script (its body sees a
    /// caller's arguments, which this flat parser cannot scope).
    positionals: Option<Vec<String>>,
    /// The script assigns `IFS`, so unquoted word-splitting is unpredictable: an
    /// unquoted resolved value is left dynamic.
    ifs_assigned: bool,
}

impl VarCtx {
    /// A name's value, defined in this scope or an enclosing one; a local
    /// definition (even a poisoning one) shadows the parent, as in bash.
    fn lookup(&self, name: &str) -> Option<&Option<VarVal>> {
        let mut ctx = self;
        loop {
            if let Some(v) = ctx.vars.get(name) {
                return Some(v);
            }
            ctx = ctx.parent.as_deref()?;
        }
    }
    /// A name's scalar candidates, if it resolves to a scalar.
    fn scalar(&self, name: &str) -> Option<&[String]> {
        match self.lookup(name) {
            Some(Some(VarVal::Scalar(v))) => Some(v),
            _ => None,
        }
    }
    /// A name's array candidates, if it resolves to an array.
    fn array(&self, name: &str) -> Option<&[Vec<String>]> {
        match self.lookup(name) {
            Some(Some(VarVal::Array(v))) => Some(v),
            _ => None,
        }
    }
}

/// The name and literal value of a scalar assignment word (`name=value`,
/// `name+=value`, `name[i]=value`); `None` value when the right side is dynamic.
fn parse_assignment(w: &Word) -> Option<(String, Option<String>)> {
    if !is_assignment(&w.text) {
        return None;
    }
    let (lhs, value) = w.text.split_once('=')?;
    let name = lhs.strip_suffix('+').unwrap_or(lhs);
    let name = name.split('[').next().unwrap_or(name);
    let val = if w.dynamic {
        None
    } else {
        Some(value.to_string())
    };
    Some((name.to_string(), val))
}

/// Record a scalar assignment. A literal value joins the candidate set; a dynamic
/// value, or a conflict with an array of the same name, poisons it (`None`).
fn record_scalar(map: &mut HashMap<String, Option<VarVal>>, name: String, val: Option<String>) {
    let slot = map
        .entry(name)
        .or_insert_with(|| Some(VarVal::Scalar(vec![])));
    match (slot.as_mut(), val) {
        (_, None) => *slot = None,
        (Some(VarVal::Scalar(list)), Some(v)) => {
            if !list.contains(&v) {
                list.push(v);
            }
        }
        (Some(VarVal::Array(_)), Some(_)) => *slot = None, // scalar/array conflict
        (None, Some(_)) => {}                              // already poisoned
    }
}

/// Record an array assignment. Every element must be a literal (no substitution,
/// glob, or reference), or the name is poisoned; a conflict with a scalar of the
/// same name poisons it too.
fn record_array(map: &mut HashMap<String, Option<VarVal>>, name: &str, elems: &[Word]) {
    let literal: Option<Vec<String>> = elems
        .iter()
        .map(|e| (!e.dynamic && !e.glob && e.subs.is_empty()).then(|| e.text.clone()))
        .collect();
    let slot = map
        .entry(name.to_string())
        .or_insert_with(|| Some(VarVal::Array(vec![])));
    match (slot.as_mut(), literal) {
        (_, None) => *slot = None,
        (Some(VarVal::Array(cands)), Some(list)) => {
            if !cands.contains(&list) {
                cands.push(list);
            }
        }
        (Some(VarVal::Scalar(_)), Some(_)) => *slot = None, // scalar/array conflict
        (None, Some(_)) => {}
    }
}

/// Collect a script's literal assignments (spec §3.9, step 2a.3), inheriting
/// `parent` (a subshell or `eval` sees the enclosing scope). Scalars, arrays,
/// `for` loop variables, and `set --` positionals all count, each unioned across
/// branches. Only assignments in command position, and the arguments of
/// `export`/`declare`/`local`/`readonly`/`typeset`, are scalar assignments.
fn collect_vars(toks: &[Tok], parent: Option<Rc<VarCtx>>) -> Rc<VarCtx> {
    let mut vars: HashMap<String, Option<VarVal>> = HashMap::new();
    let mut positionals = parent.as_ref().and_then(|p| p.positionals.clone());
    let mut ifs_assigned = parent.as_ref().is_some_and(|p| p.ifs_assigned);
    let mut cmd_start = true;
    let mut assign_cmd = false;
    let mut skip_target = false;
    let mut has_func = false;
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Op(op) => {
                // A function definition (`name ()`): the positionals its body
                // sees are a caller's, unknown to this flat parser.
                if *op == "("
                    && matches!(toks.get(i + 1), Some(Tok::Op(")")))
                    && i > 0
                    && matches!(&toks[i - 1], Tok::Word(_))
                {
                    has_func = true;
                }
                cmd_start = true;
                assign_cmd = false;
                skip_target = false;
            }
            Tok::Subs(_) => {
                cmd_start = true;
                assign_cmd = false;
                skip_target = false;
            }
            Tok::Redir | Tok::RedirDrop | Tok::HereDoc => skip_target = true,
            Tok::Array { name, elems } => {
                record_array(&mut vars, name, elems);
                // An array assignment holds command position, like a scalar one.
            }
            Tok::Word(w) => {
                if skip_target {
                    skip_target = false;
                    i += 1;
                    continue;
                }
                let assignment = parse_assignment(w);
                if cmd_start {
                    if let Some((name, val)) = assignment {
                        if name == "IFS" {
                            ifs_assigned = true;
                        }
                        record_scalar(&mut vars, name, val);
                        i += 1;
                        continue; // leading assignments keep command position
                    }
                    if !w.dynamic {
                        match w.text.as_str() {
                            "export" | "declare" | "local" | "readonly" | "typeset" => {
                                assign_cmd = true;
                            }
                            "for" | "select" => {
                                i = record_for(toks, i, &mut vars);
                                cmd_start = false;
                                continue;
                            }
                            "set" => {
                                if let Some(ni) = record_set(toks, i, &mut positionals) {
                                    i = ni;
                                    cmd_start = false;
                                    continue;
                                }
                            }
                            "shift" => positionals = None,
                            "function" => has_func = true,
                            _ => {}
                        }
                    }
                    cmd_start = false;
                } else if assign_cmd {
                    if let Some((name, val)) = assignment {
                        if name == "IFS" {
                            ifs_assigned = true;
                        }
                        record_scalar(&mut vars, name, val);
                    }
                }
            }
        }
        i += 1;
    }
    if has_func {
        positionals = None;
    }
    Rc::new(VarCtx {
        vars,
        parent,
        positionals,
        ifs_assigned,
    })
}

/// A `for name in w1 w2 …` (or `select`) header at token `i`: record `name`'s
/// candidate values (the listed words), poisoned if the list has a dynamic word
/// or a glob. Returns the index to resume at (the terminator after the list).
fn record_for(toks: &[Tok], i: usize, map: &mut HashMap<String, Option<VarVal>>) -> usize {
    let name = match toks.get(i + 1) {
        Some(Tok::Word(w)) if !w.dynamic && is_name(&w.text) => w.text.clone(),
        _ => return i + 1,
    };
    if !matches!(toks.get(i + 2), Some(Tok::Word(w)) if w.text == "in") {
        // `for name; do …` iterates the positionals; not resolved here.
        return i + 1;
    }
    let mut j = i + 3;
    let mut words = vec![];
    let mut poison = false;
    while let Some(Tok::Word(w)) = toks.get(j) {
        if w.dynamic || w.glob || !w.subs.is_empty() {
            poison = true;
        } else {
            words.push(w.text.clone());
        }
        j += 1;
    }
    if poison || words.is_empty() {
        map.insert(name, None);
    } else {
        for v in words {
            record_scalar(map, name.clone(), Some(v));
        }
    }
    j
}

/// A `set` command at token `i`. Only `set -- w1 w2 …` is handled (it redefines
/// the positionals): literal words set them, a dynamic word makes them unknown.
/// Returns the resume index, or `None` if this is some other `set` (e.g. `set -e`).
fn record_set(toks: &[Tok], i: usize, positionals: &mut Option<Vec<String>>) -> Option<usize> {
    if !matches!(toks.get(i + 1), Some(Tok::Word(w)) if w.text == "--") {
        return None;
    }
    let mut j = i + 2;
    let mut words = vec![];
    let mut ok = true;
    while let Some(Tok::Word(w)) = toks.get(j) {
        if w.dynamic || w.glob || !w.subs.is_empty() {
            ok = false;
        } else {
            words.push(w.text.clone());
        }
        j += 1;
    }
    *positionals = ok.then_some(words);
    Some(j)
}

/// A bare identifier (a legal variable name).
fn is_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------- resolution

/// The cap on resolved words/candidates for one reference or command; past it a
/// word or command stays as written (dynamic), never a silent single value.
const RESOLVE_CAP: usize = 64;

/// Does `s` hold an unquoted glob metacharacter?
fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// Split an unquoted resolved value on whitespace (default IFS), each field a
/// literal word carrying the glob flag when it holds a metacharacter.
fn split_into_words(v: &str) -> Vec<Word> {
    v.split_whitespace()
        .map(|p| Word {
            text: p.to_string(),
            glob: has_glob(p),
            ..Default::default()
        })
        .collect()
}

/// The words `${name[@]}`/`${name[*]}` or `$@`/`$*` expand to, given the element
/// list and whether the reference used `*` and was quoted (spec §3.9): `"$*"`
/// joins to one word; every other form is one word per element.
fn list_words(elems: &[String], star: bool, quoted: bool) -> Vec<Word> {
    if star && quoted {
        return vec![Word {
            text: elems.join(" "),
            quoted: true,
            ..Default::default()
        }];
    }
    elems
        .iter()
        .map(|e| Word {
            text: e.clone(),
            quoted,
            glob: !quoted && has_glob(e),
            ..Default::default()
        })
        .collect()
}

/// A whole-word array reference `${name[@]}` / `${name[*]}`: the name and whether
/// it used `*`. `None` for anything else (a scalar, an embedded reference).
fn array_ref(text: &str) -> Option<(&str, bool)> {
    let inner = text.strip_prefix("${")?.strip_suffix('}')?;
    let (name, sub) = inner.split_once('[')?;
    let star = match sub {
        "@]" => false,
        "*]" => true,
        _ => return None,
    };
    is_name(name).then_some((name, star))
}

/// A whole-word positional list `$@`/`$*`/`${@}`/`${*}`: whether it used `*`.
fn at_ref(text: &str) -> Option<bool> {
    match text {
        "$@" | "${@}" => Some(false),
        "$*" | "${*}" => Some(true),
        _ => None,
    }
}

/// Resolve the `$` reference at `c[i]` to its candidate scalar values, and the
/// index after it. `${name}`, `$name`, and `$1`… (a positional) resolve; an
/// array-typed name, `$@`/`$*`, an arithmetic or brace-modifier form, and an
/// unassigned name do not (the caller keeps the word dynamic).
fn scalar_ref_at(c: &[char], i: usize, ctx: &VarCtx) -> Option<(Vec<String>, usize)> {
    let (name, end): (String, usize) = if c.get(i + 1) == Some(&'{') {
        let close = c[i + 2..].iter().position(|&x| x == '}')? + i + 2;
        (c[i + 2..close].iter().collect(), close + 1)
    } else {
        let mut j = i + 1;
        if c.get(j).is_some_and(|x| x.is_ascii_digit()) {
            j += 1; // a single-digit positional
        } else {
            while c
                .get(j)
                .is_some_and(|y| y.is_ascii_alphanumeric() || *y == '_')
            {
                j += 1;
            }
        }
        (c[i + 1..j].iter().collect(), j)
    };
    if let Ok(n) = name.parse::<usize>() {
        if n == 0 {
            return None;
        }
        let p = ctx.positionals.as_ref()?;
        return Some((vec![p.get(n - 1).cloned().unwrap_or_default()], end));
    }
    if !is_name(&name) {
        return None;
    }
    Some((ctx.scalar(&name)?.to_vec(), end))
}

/// Substitute every `$` reference in `text` from `ctx`, returning the product of
/// candidate strings, or `None` when any reference is unresolvable (so the word
/// stays dynamic). Literal characters pass through unchanged.
fn resolve_scalar_text(text: &str, ctx: &VarCtx) -> Option<Vec<String>> {
    let c: Vec<char> = text.chars().collect();
    let mut results = vec![String::new()];
    let mut i = 0;
    while i < c.len() {
        if c[i] == '$' {
            let (values, ni) = scalar_ref_at(&c, i, ctx)?;
            let mut next = Vec::new();
            for r in &results {
                for v in &values {
                    next.push(format!("{r}{v}"));
                    if next.len() > RESOLVE_CAP {
                        return None;
                    }
                }
            }
            results = next;
            i = ni;
        } else {
            for r in results.iter_mut() {
                r.push(c[i]);
            }
            i += 1;
        }
    }
    Some(results)
}

/// Resolve one word to its possible expansions, each a sequence of words (a
/// scalar with several candidates, or a value that word-splits, yields more than
/// one). A word with no resolvable reference is returned unchanged.
fn resolve_word(w: &Word, ctx: &VarCtx) -> Vec<Vec<Word>> {
    let keep = || vec![vec![w.clone()]];
    if !w.dynamic || !w.subs.is_empty() {
        return keep();
    }
    if let Some((name, star)) = array_ref(&w.text) {
        return match ctx.array(name) {
            Some(cands) if !cands.is_empty() => cands
                .iter()
                .map(|e| list_words(e, star, w.quoted))
                .collect(),
            _ => keep(),
        };
    }
    if let Some(star) = at_ref(&w.text) {
        return match &ctx.positionals {
            Some(p) => vec![list_words(p, star, w.quoted)],
            None => keep(),
        };
    }
    match resolve_scalar_text(&w.text, ctx) {
        Some(values) => {
            let mut out = vec![];
            for v in values {
                if w.quoted {
                    out.push(vec![Word {
                        text: v,
                        quoted: true,
                        ..Default::default()
                    }]);
                } else if ctx.ifs_assigned {
                    return keep(); // unquoted splitting is unpredictable
                } else {
                    out.push(split_into_words(&v));
                }
            }
            out
        }
        None => keep(),
    }
}

/// Resolve every word of one simple command from the script's literal
/// assignments (spec §3.9, step 2a.3), returning each possible resolved word-list
/// with the `(spelling, value)` pairs it resolved (for the notice). Usually one
/// list; more when a name has several candidates. A command with no resolvable
/// reference, or one past `RESOLVE_CAP`, is returned as written.
type Resolved = (Vec<Word>, Vec<(String, String)>);

fn resolve_command(words: &[Word], ctx: &VarCtx) -> Vec<Resolved> {
    if !words.iter().any(|w| w.dynamic && w.subs.is_empty()) {
        return vec![(words.to_vec(), vec![])];
    }
    let mut lists: Vec<Resolved> = vec![(vec![], vec![])];
    for w in words {
        let alts = resolve_word(w, ctx);
        let resolvable = w.dynamic && w.subs.is_empty();
        let mut next = Vec::new();
        for (base_words, base_prov) in &lists {
            for alt in &alts {
                let mut ws = base_words.clone();
                ws.extend(alt.iter().cloned());
                let mut prov = base_prov.clone();
                // A word actually resolved when its alternative is not just itself.
                if resolvable && !(alt.len() == 1 && alt[0] == *w) {
                    let value = alt
                        .iter()
                        .map(|w| w.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    prov.push((w.text.clone(), value));
                }
                next.push((ws, prov));
                if next.len() > RESOLVE_CAP {
                    return vec![(words.to_vec(), vec![])];
                }
            }
        }
        lists = next;
    }
    lists
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
        resolved: vec![],
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
    /// An array assignment `name=(w1 w2 …)` (or `name+=(…)`): its elements, for
    /// `collect_vars`; it starts no command. Substitutions in the elements are
    /// still parsed for what they run.
    Array {
        name: String,
        elems: Vec<Word>,
    },
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
                // `name=(…)` / `name+=(…)`, the `(` right after the `name=` word
                // (no space): an array assignment, not a subshell.
                let arr = (i > 0 && !c[i - 1].is_whitespace())
                    .then(|| match toks.last() {
                        Some(Tok::Word(w)) => array_assign_prefix(&w.text),
                        _ => None,
                    })
                    .flatten();
                if let Some(name) = arr {
                    toks.pop();
                    let (elems, end) = lex_array(&c, i + 1)?;
                    toks.push(Tok::Array { name, elems });
                    i = end;
                } else {
                    toks.push(Tok::Op("("));
                    i += 1;
                }
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
                // `$@`/`$*` (and `${@}`/`${*}`, quoted or not), standing alone,
                // expand to the known positional parameters with bash's word
                // semantics: `"$@"` and unquoted `$@`/`$*` to one word per
                // argument, `"$*"` to one word joined by spaces.
                if let Some(p) = params {
                    if let Some((words, end)) = positional_list(&c, i, p) {
                        i = end;
                        for w in words {
                            toks.push(Tok::Word(w));
                        }
                        continue;
                    }
                }
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

/// A standalone `$@`/`$*`/`${@}`/`${*}`, quoted or not, at index `i`, expanded
/// against the literal positional parameters `params`. Returns the words and the
/// index after the token, or `None` when the token is not one of these standing
/// alone (it must end at a word boundary). `"$@"`, `$@`, `$*`, `${@}`, `${*}`
/// yield one word per argument; only `"$*"` (and `"${*}"`) joins into one word.
fn positional_list(c: &[char], i: usize, params: &[String]) -> Option<(Vec<Word>, usize)> {
    let g = |k: usize| c.get(k).copied();
    let (quoted, sym, end) = if g(i) == Some('"') {
        match (g(i + 1), g(i + 2), g(i + 3), g(i + 4), g(i + 5)) {
            (Some('$'), Some(s @ ('@' | '*')), Some('"'), _, _) => (true, s, i + 4),
            (Some('$'), Some('{'), Some(s @ ('@' | '*')), Some('}'), Some('"')) => (true, s, i + 6),
            _ => return None,
        }
    } else if g(i) == Some('$') {
        match (g(i + 1), g(i + 2), g(i + 3)) {
            (Some(s @ ('@' | '*')), _, _) => (false, s, i + 2),
            (Some('{'), Some(s @ ('@' | '*')), Some('}')) => (false, s, i + 4),
            _ => return None,
        }
    } else {
        return None;
    };
    if !(end >= c.len() || WORD_END.contains(&c[end])) {
        return None; // embedded in a larger word; leave it to lex_word (joined)
    }
    let words = if quoted && sym == '*' {
        vec![Word {
            text: params.join(" "),
            quoted: true,
            ..Default::default()
        }]
    } else {
        params
            .iter()
            .map(|a| Word {
                text: a.clone(),
                quoted,
                ..Default::default()
            })
            .collect()
    };
    Some((words, end))
}

/// The name of an assignment word whose value is about to be an array literal:
/// `a=` → `a`, `a+=` → `a`. `None` for anything that is not a bare `name=`/
/// `name+=` (an empty right-hand side, the `(` supplying the value).
fn array_assign_prefix(text: &str) -> Option<String> {
    let lhs = text.strip_suffix('=')?;
    let lhs = lhs.strip_suffix('+').unwrap_or(lhs);
    (!lhs.is_empty()
        && !lhs.starts_with(|c: char| c.is_ascii_digit())
        && lhs.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
    .then(|| lhs.to_string())
}

/// The element words of an array literal, from just after the opening `(` to the
/// matching `)`. Elements are whitespace-separated words (each lexed like any
/// word, so quotes and substitutions are handled); returns the index after `)`.
fn lex_array(c: &[char], mut i: usize) -> Result<(Vec<Word>, usize), String> {
    let n = c.len();
    let mut elems = vec![];
    loop {
        while i < n && matches!(c[i], ' ' | '\t' | '\r' | '\n') {
            i += 1;
        }
        match c.get(i) {
            None => return Err("unterminated array".into()),
            Some(')') => return Ok((elems, i + 1)),
            Some(_) => {
                let (w, ni) = lex_word(c, i, None)?;
                if ni == i {
                    return Err("array element stalled".into());
                }
                elems.push(w);
                i = ni;
            }
        }
    }
}

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
    /// This script's literal assignments, for resolving variable references. An
    /// `Rc` so a subshell or `eval` links to it without copying it.
    vars: Rc<VarCtx>,
}

impl<'a> Parser<'a> {
    fn new(
        cwds: Cwds,
        depth: usize,
        via: Vec<String>,
        out: &'a mut Vec<Cmd>,
        vars: Rc<VarCtx>,
    ) -> Self {
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
            vars,
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
                        // A `$(…)` body is a subshell: it inherits this scope.
                        parse_script_in(
                            s,
                            &self.cwds,
                            self.depth + 1,
                            &self.via,
                            None,
                            Some(self.vars.clone()),
                            self.out,
                        );
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
                        parse_script_in(
                            s,
                            &self.cwds,
                            self.depth + 1,
                            &self.via,
                            None,
                            Some(self.vars.clone()),
                            self.out,
                        );
                    }
                }
                Tok::Array { elems, .. } => {
                    // An array assignment starts no command; its elements'
                    // substitutions still run in this scope.
                    for e in &elems {
                        for s in &e.subs {
                            parse_script_in(
                                s,
                                &self.cwds,
                                self.depth + 1,
                                &self.via,
                                None,
                                Some(self.vars.clone()),
                                self.out,
                            );
                        }
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
        // Resolve every simple variable reference from this script's literal
        // assignments (spec §3.9, step 2a.3), before `expand` sees the words: a
        // program reached through a wrapper, an option or operand carried in a
        // variable, an array, a `$@`, or an embedded reference is judged for what
        // it became. Each resolved word-list is expanded on its own; a word with
        // an unresolvable reference stays dynamic (the floor scans its spelling).
        // The redirection targets belong to this simple command, so they are
        // attached to every command it expanded to (a wrapper or `bash -c` may
        // move where they are written).
        for (ws, prov) in resolve_command(&words, &self.vars) {
            let mark = self.out.len();
            expand(
                ws,
                self.cwds.clone(),
                false,
                self.depth,
                &text,
                &self.via,
                Some(&self.vars),
                self.out,
            );
            for c in &mut self.out[mark..] {
                c.redirs.extend(redirs.iter().cloned());
                if !prov.is_empty() {
                    c.resolved = prov.clone();
                }
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
#[allow(clippy::too_many_arguments)]
fn expand(
    mut words: Vec<Word>,
    cwds: Cwds,
    more_operands: bool,
    depth: usize,
    text: &str,
    via: &[String],
    vars: Option<&Rc<VarCtx>>,
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
            resolved: vec![],
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
                    parse_script_in(
                        &script,
                        &cwds,
                        depth + 1,
                        &via_with(&prog),
                        params.as_deref(),
                        None,
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
                    parse_script_in(&rest, &cwds, depth + 1, &via_with(&prog), None, None, out);
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
            vars,
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
            vars,
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
        parse_script_in(
            &s.join(" "),
            &cwds,
            depth + 1,
            &via_with(&prog),
            None,
            None,
            out,
        );
        return;
    }
    if prog == "eval" {
        // Opaque in itself, and its text is still read for what it names. `eval`
        // runs in the current shell, so it inherits this scope's assignments.
        let s: Vec<&str> = words[1..].iter().map(|w| w.text.as_str()).collect();
        let joined = s.join(" ");
        emit(words, cwds.clone(), more_operands, out);
        parse_script_in(
            &joined,
            &cwds,
            depth + 1,
            &via_with(&prog),
            None,
            vars.cloned(),
            out,
        );
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
            parse_script_in(
                &joined,
                &Cwds::unknown(),
                depth + 1,
                &via_with(&prog),
                None,
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
                    vars,
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
                    resolved: vec![],
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
            vars,
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
            vec!["rm -rf a", "rm -rf b"],
            "a for-loop variable resolves to each listed value (step 2a.3)"
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

    /// Step 2a.3: a simple variable reference resolves from the script's literal
    /// assignments everywhere — through a wrapper, `eval`, a nested `bash -c`, an
    /// array, `set --`, and an embedded reference — with bash's word semantics.
    #[test]
    fn resolves_variables_everywhere() {
        // Through wrappers and re-parses: the resolved program reaches `expand`.
        assert_eq!(progs("x=op; command $x whoami"), vec!["op whoami"]);
        assert_eq!(progs("x=op; exec \"$x\" whoami"), vec!["op whoami"]);
        assert_eq!(progs("x=op; env $x whoami"), vec!["op whoami"]);
        assert_eq!(progs("x=op; timeout 5 $x whoami"), vec!["op whoami"]);
        assert_eq!(
            progs("x=op; eval \"$x whoami\""),
            vec!["eval op whoami", "op whoami"]
        );
        assert_eq!(progs("x=op; bash -c \"$x whoami\""), vec!["op whoami"]);
        // Embedded references take the product of candidates.
        assert_eq!(
            progs("a=these; b=usd; $a$b --version"),
            vec!["theseusd --version"]
        );
        // An unquoted value word-splits; a quoted one is a single word.
        assert_eq!(progs("x=\"-rf somedir\"; rm $x"), vec!["rm -rf somedir"]);
        assert_eq!(
            progs("x=\"-rf somedir\"; rm \"$x\""),
            vec!["rm -rf somedir"]
        );
        // An array, quoted `[@]`, is one word per element; `set --` redefines `$@`
        // (the `set` command itself is emitted, harmlessly).
        assert_eq!(
            progs("a=(-rf somedir); rm \"${a[@]}\""),
            vec!["rm -rf somedir"]
        );
        assert_eq!(
            progs("set -- -rf somedir; rm \"$@\""),
            vec!["set -- -rf somedir", "rm -rf somedir"]
        );
        // A `for` list of literals gives one command per value.
        assert_eq!(
            progs("for b in main dev; do git push origin $b; done"),
            vec!["git push origin main", "git push origin dev"]
        );
        // Unresolvable: an unassigned name, an IFS-split value, a glob `for` list,
        // and a `set --` with a dynamic word all stay dynamic (spelling kept).
        assert_eq!(progs("echo $UNSET"), vec!["echo $UNSET"]);
        assert_eq!(progs("IFS=,; x=a,b; rm $x"), vec!["rm $x"]);
        assert_eq!(progs("for f in *.log; do rm \"$f\"; done"), vec!["rm $f"]);
        assert_eq!(progs("set -- $y; rm \"$@\""), vec!["set -- $y", "rm $@"]);
        // A `$name` is not resolved from a dynamic assignment (it is poisoned).
        assert_eq!(progs("x=$(id -u); echo $x"), vec!["id -u", "echo $x"]);
    }
}
