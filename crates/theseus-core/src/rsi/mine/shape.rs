//! Normalization: each call becomes a shape (§2.2). A call's tool and its
//! arguments, with every value turned into a typed slot (`$PATH`, `$REF`,
//! `$NUM`, `$URLHOST`, `$CRATE`, `$STR`), so two calls that differ only in
//! their values have the same shape and the miner can count them together.
//!
//! The shape holds no value. The values ride beside it, in memory, as [`Raw`]
//! (which prints and serializes as nothing), for the slot classification that
//! follows; what the miner stores is the shape's text and a keyed digest of
//! each value (`read.rs`).
//!
//! - **Programs** keep their name and up to two subcommand words for the known
//!   multi-command programs (`git merge`, `cargo nextest run`, `gh pr view`).
//!   Flags keep their names, as a sorted set; a flag that takes a value keeps
//!   it as a slot beside it (`-p $CRATE`). `steps` arrays and the `&&`/`;`/`|`
//!   of a `command` line are one shape each.
//! - **Results** are a class: `ok`, or `err:<exit class>`, or the status when
//!   the call did not run, followed by the features small extractors find in
//!   the result's text (`conflict`, `rerere-resolved`, …). The branches split
//!   on the class.
//! - **Authored** calls carry text the model wrote (an edit's body, a file's
//!   content, a long message). They are not part of any tool: they cut the
//!   stretches the patterns are found in.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A long value is the model's own writing, past this many characters.
pub const AUTHORED_CHARS: usize = 160;

/// The type of a slot's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotType {
    /// A file path, with its top directory and extension class.
    Path,
    /// A branch, tag, remote ref, commit, or other name of a thing.
    Reference,
    /// A number.
    Num,
    /// The host of a URL.
    UrlHost,
    /// A cargo package.
    Crate,
    /// Any other short string.
    Str,
}

impl SlotType {
    /// The placeholder's name in a shape's text.
    pub fn placeholder(self) -> &'static str {
        match self {
            SlotType::Path => "$PATH",
            SlotType::Reference => "$REF",
            SlotType::Num => "$NUM",
            SlotType::UrlHost => "$URLHOST",
            SlotType::Crate => "$CRATE",
            SlotType::Str => "$STR",
        }
    }

    /// The name of an input of this type in a skeleton spec.
    pub fn input_name(self) -> &'static str {
        match self {
            SlotType::Path => "path",
            SlotType::Reference => "ref",
            SlotType::Num => "num",
            SlotType::UrlHost => "url",
            SlotType::Crate => "crate",
            SlotType::Str => "text",
        }
    }
}

/// A raw value: held in memory for the slot classification, and never
/// printed, serialized or stored.
#[derive(Clone, PartialEq, Eq)]
pub struct Raw(String);

impl Raw {
    pub fn new(s: impl Into<String>) -> Self {
        Raw(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Raw(<{} chars>)", self.0.chars().count())
    }
}

/// One slot of a shape: its type and class (a path's `top/**.ext`), never its
/// value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotShape {
    #[serde(rename = "type")]
    pub ty: SlotType,
    /// A path's class, `crates/**.rs`; empty for any other type.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
}

/// A call's shape: what two calls with the same shape have in common.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub tool: String,
    /// The shape's text, `proc.run[git merge --no-ff $REF]`: the key the
    /// miner counts.
    pub text: String,
    /// The program and its subcommand words (`git merge`); empty for a tool
    /// that is not a program.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head: String,
    /// The flags, sorted, with a `=` after one that takes a value (`-p=`
    /// then its slot, `--no-ff` alone).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// The slots, in the order the text shows them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<SlotShape>,
    /// The model's own writing is in it: it cuts a stretch.
    pub authored: bool,
}

/// One normalized call: its shape and, in memory only, its values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Norm {
    pub shape: Shape,
    /// One value for each of the shape's slots.
    pub values: Vec<Raw>,
}

/// What the normalizer knows of where a call ran.
#[derive(Debug, Clone, Default)]
pub struct Where {
    /// The repository's root (the git top level of the call's directory):
    /// a path inside it is relative to it, `$REPO`.
    pub root: Option<String>,
}

/// Calls that face outward or spend, never part of a stretch (§2.3.6): a
/// minted tool never speaks for the owner or spends. Prefixes of the tool's
/// canonical name.
pub const NEVER: &[&str] = &[
    "message",
    "email",
    "discord",
    "channel.",
    "aws.call",
    "aws.hands",
    "aws.stack",
    "extend.",
    "self.",
    "owner_ask",
    "plan_present",
    "plan_step",
    "memory.write",
    "memory.label",
    "memory.retract",
    "fact.",
    "thread.",
    "standing.",
    "routine.",
    "watch",
    "wake.",
    "task.",
    "web.post",
    "http.post",
    "mcp:",
    "mcp__",
    "hands.",
];

/// Programs that send something out when they run.
const OUTWARD: &[&str] = &[
    "curl", "wget", "mail", "sendmail", "mutt", "ssh", "scp", "rsync",
];

/// Whether `tool` is one the miner never mines.
pub fn never_tool(tool: &str) -> bool {
    NEVER.iter().any(|p| tool.starts_with(p))
}

/// Whether a normalized call is one the miner never mines.
pub fn never_mined(n: &Norm) -> bool {
    never_tool(&n.shape.tool)
        || (n.shape.tool == "proc.run"
            && OUTWARD
                .iter()
                .any(|p| n.shape.head.split(' ').next() == Some(*p)))
}

/// Programs whose first word is a subcommand, and the subcommands whose own
/// first argument is one more word of it.
const SUBCOMMANDS: &[(&str, &[&str])] = &[
    (
        "git",
        &[
            "stash",
            "remote",
            "rerere",
            "worktree",
            "submodule",
            "bisect",
            "notes",
            "reflog",
        ],
    ),
    ("cargo", &["nextest"]),
    ("npm", &["run"]),
    ("pnpm", &["run"]),
    ("yarn", &["run"]),
    ("gh", &["pr", "issue", "run", "repo", "release", "workflow"]),
    (
        "docker",
        &["compose", "container", "image", "network", "volume"],
    ),
    ("bd", &["dep"]),
    ("kubectl", &[]),
    ("systemctl", &[]),
    ("rustup", &[]),
    ("go", &[]),
    ("pip", &[]),
    ("uv", &[]),
    ("make", &[]),
    ("terraform", &[]),
    ("helm", &[]),
];

/// Flags that take the next argument as their value.
const VALUE_FLAGS: &[&str] = &[
    "-p",
    "--package",
    "-C",
    "-c",
    "-m",
    "--message",
    "-b",
    "-o",
    "--output",
    "-f",
    "--file",
    "-n",
    "--max-count",
    "--since",
    "--until",
    "--author",
    "--format",
    "--pretty",
    "--repo",
    "--branch",
    "--config",
    "--target",
    "--features",
    "-j",
    "--jobs",
    "-e",
    "--bin",
    "--test",
    "--example",
    "--manifest-path",
    "--profile",
    "-R",
    "--base",
    "--head",
    "--title",
    "--body",
    "--label",
    "--remote",
    "--upstream",
    "-u",
    "--set-upstream-to",
    "-t",
    "--filter",
    "-E",
    "--workspace-dir",
];

/// Flags whose value is a cargo package.
const CRATE_FLAGS: &[&str] = &["-p", "--package"];

/// The long words of a model's call that mark a name as a flag: letters,
/// digits and dashes only, so a secret glued to a flag (`--token=sk-…`) is a
/// value, never a flag name.
fn flag_name(arg: &str) -> Option<&str> {
    let body = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-'))?;
    let ok = !body.is_empty()
        && body.len() <= 32
        && body.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(arg)
}

fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 24
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_hex(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// The host of a URL or scp-style address, if `v` is one.
fn url_host(v: &str) -> Option<String> {
    let rest = v
        .strip_prefix("https://")
        .or_else(|| v.strip_prefix("http://"))
        .or_else(|| v.strip_prefix("ssh://"))
        .or_else(|| v.strip_prefix("git://"));
    let host = match rest {
        Some(r) => r.split('/').next().unwrap_or(""),
        None => {
            let (user, r) = v.split_once('@')?;
            if user != "git" || !r.contains(':') {
                return None;
            }
            r.split(':').next().unwrap_or("")
        }
    };
    let host = host.rsplit('@').next().unwrap_or(host);
    (!host.is_empty()).then(|| host.to_string())
}

/// A path's class: its top directory and its extension, `crates/**.rs`; a
/// path outside the repository is `$ABS`, which no constant may be.
fn path_class(v: &str, w: &Where) -> String {
    let rel = match (&w.root, v.strip_prefix('/')) {
        (Some(root), Some(_)) => v
            .strip_prefix(root.trim_end_matches('/'))
            .map(|r| r.trim_start_matches('/')),
        (None, Some(_)) => None,
        _ => Some(v.trim_start_matches("./")),
    };
    let Some(rel) = rel else {
        return "$ABS".into();
    };
    if rel.starts_with('~') || rel.starts_with("..") {
        return "$ABS".into();
    }
    let top = rel.split('/').next().unwrap_or("");
    let multi = rel.contains('/');
    let ext = rel
        .rsplit('/')
        .next()
        .and_then(|f| f.rsplit_once('.'))
        .map(|(_, e)| e)
        .filter(|e| !e.is_empty() && e.len() <= 6 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("");
    match (multi, ext.is_empty()) {
        (true, false) => format!("{top}/**.{ext}"),
        (true, true) => format!("{top}/**"),
        (false, false) => format!("*.{ext}"),
        (false, true) => top.to_string(),
    }
}

/// A value's slot type and class.
pub fn type_of(v: &str, w: &Where) -> SlotShape {
    let mk = |ty, class: &str| SlotShape {
        ty,
        class: class.into(),
    };
    if url_host(v).is_some() {
        return mk(SlotType::UrlHost, "");
    }
    if !v.is_empty() && v.len() <= 12 && v.chars().all(|c| c.is_ascii_digit()) {
        return mk(SlotType::Num, "");
    }
    let refish = is_hex(v)
        || v.starts_with("HEAD")
        || v.starts_with("origin/")
        || v.starts_with("refs/")
        || v.starts_with("upstream/");
    let has_ext = v
        .rsplit('/')
        .next()
        .and_then(|f| f.rsplit_once('.'))
        .is_some_and(|(stem, e)| {
            !stem.is_empty()
                && (1..=6).contains(&e.len())
                && e.chars().all(|c| c.is_ascii_alphabetic())
        });
    if !refish
        && (v.starts_with('/')
            || v.starts_with("./")
            || v.starts_with("../")
            || v.starts_with('~')
            || has_ext
            || v.matches('/').count() >= 3)
        && !v.contains(' ')
    {
        return mk(SlotType::Path, &path_class(v, w));
    }
    let name = v.len() <= 80
        && !v.is_empty()
        && v.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '~' | '^' | '@')
        });
    if name {
        return mk(SlotType::Reference, "");
    }
    mk(SlotType::Str, "")
}

/// A slot's text in a shape.
fn slot_text(s: &SlotShape) -> String {
    if s.class.is_empty() {
        s.ty.placeholder().to_string()
    } else {
        format!("{}:{}", s.ty.placeholder(), s.class)
    }
}

/// One argument of a program, after its head: a flag (with its value when it
/// takes one) or a positional value.
enum Arg {
    Flag(String, Option<String>),
    Pos(String),
}

fn split_args(rest: &[String]) -> Vec<Arg> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut flags_over = false;
    while i < rest.len() {
        let a = &rest[i];
        i += 1;
        if flags_over || a == "-" || !a.starts_with('-') {
            out.push(Arg::Pos(a.clone()));
            continue;
        }
        if a == "--" {
            flags_over = true;
            continue;
        }
        if a.len() > 1 && a[1..].chars().all(|c| c.is_ascii_digit()) {
            out.push(Arg::Pos(a.clone()));
            continue;
        }
        if let Some((f, v)) = a.split_once('=') {
            match flag_name(f) {
                Some(f) => out.push(Arg::Flag(f.to_string(), Some(v.to_string()))),
                None => out.push(Arg::Pos(a.clone())),
            }
            continue;
        }
        let Some(f) = flag_name(a) else {
            out.push(Arg::Pos(a.clone()));
            continue;
        };
        let takes = VALUE_FLAGS.contains(&f);
        if takes && i < rest.len() {
            out.push(Arg::Flag(f.to_string(), Some(rest[i].clone())));
            i += 1;
        } else {
            out.push(Arg::Flag(f.to_string(), None));
        }
    }
    out
}

fn base(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// An argv as a shape: the program, its subcommand words, its flags as a
/// sorted set, then its positional values.
fn program(argv: &[String], w: &Where) -> Option<Norm> {
    let first = argv.first()?;
    let prog = base(first).to_string();
    let mut rest: Vec<String> = argv[1..].to_vec();
    let mut head = vec![prog.clone()];
    if let Some((_, second)) = SUBCOMMANDS.iter().find(|(p, _)| *p == prog) {
        // git's `-C dir` and `-c k=v` come before the subcommand.
        let mut lead: Vec<String> = Vec::new();
        while prog == "git" && rest.len() >= 2 && (rest[0] == "-C" || rest[0] == "-c") {
            lead.extend(rest.drain(..2));
        }
        if rest.first().is_some_and(|a| is_word(a)) {
            let sub = rest.remove(0);
            let two = second.contains(&sub.as_str());
            head.push(sub);
            if two && rest.first().is_some_and(|a| is_word(a)) {
                head.push(rest.remove(0));
            }
        }
        // The lead flags are part of the arguments.
        let mut full = lead;
        full.extend(rest);
        rest = full;
    }
    let args = split_args(&rest);
    let mut flags: Vec<(String, Option<SlotShape>, Option<Raw>)> = Vec::new();
    let mut pos: Vec<(SlotShape, Raw)> = Vec::new();
    let mut authored = false;
    for a in args {
        match a {
            Arg::Flag(f, v) => {
                let (slot, raw) = match v {
                    Some(v) => {
                        authored |= is_long(&v);
                        let ty = if CRATE_FLAGS.contains(&f.as_str()) {
                            SlotShape {
                                ty: SlotType::Crate,
                                class: String::new(),
                            }
                        } else {
                            type_of(&v, w)
                        };
                        (Some(ty), Some(Raw::new(v)))
                    }
                    None => (None, None),
                };
                flags.push((f, slot, raw));
            }
            Arg::Pos(v) => {
                authored |= is_long(&v);
                pos.push((type_of(&v, w), Raw::new(v)));
            }
        }
    }
    flags.sort_by(|a, b| a.0.cmp(&b.0));
    let mut text = head.join(" ");
    let mut slots = Vec::new();
    let mut values = Vec::new();
    let mut flag_names = Vec::new();
    for (f, slot, raw) in flags {
        text.push(' ');
        text.push_str(&f);
        if let (Some(s), Some(r)) = (slot, raw) {
            flag_names.push(format!("{f}="));
            text.push(' ');
            text.push_str(&slot_text(&s));
            slots.push(s);
            values.push(r);
        } else {
            flag_names.push(f);
        }
    }
    for (s, r) in pos {
        text.push(' ');
        text.push_str(&slot_text(&s));
        slots.push(s);
        values.push(r);
    }
    let head = head.join(" ");
    let text = format!(
        "proc.run[{text}]{}",
        if authored { " AUTHORED" } else { "" }
    );
    Some(Norm {
        shape: Shape {
            tool: "proc.run".into(),
            text,
            head,
            flags: flag_names,
            slots,
            authored,
        },
        values,
    })
}

fn is_long(v: &str) -> bool {
    v.chars().count() > AUTHORED_CHARS || v.contains('\n')
}

fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array()?
        .iter()
        .map(|s| s.as_str().map(str::to_string))
        .collect()
}

/// A `command` line as its simple commands: split on `&&`, `||`, `;` and `|`
/// outside quotes, each split into words, redirections dropped. `None` for a
/// line a shell must read (a substitution, a here-document, a subshell).
pub fn command_line(line: &str) -> Option<Vec<Vec<String>>> {
    if line.contains("$(") || line.contains('`') || line.contains("<<") || line.contains('\n') {
        return None;
    }
    let mut cmds: Vec<Vec<String>> = vec![];
    let mut cur: Vec<String> = vec![];
    let mut word = String::new();
    let mut has_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    let flush = |word: &mut String, has: &mut bool, cur: &mut Vec<String>| {
        if *has {
            cur.push(std::mem::take(word));
            *has = false;
        }
    };
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                has_word = true;
            }
            (None, '\\') => {
                word.push(chars.next()?);
                has_word = true;
            }
            (None, c) if c.is_whitespace() => flush(&mut word, &mut has_word, &mut cur),
            (None, '&' | '|' | ';') => {
                flush(&mut word, &mut has_word, &mut cur);
                if matches!(c, '&' | '|') && chars.peek() == Some(&c) {
                    chars.next();
                } else if c == '&' {
                    // A lone `&` is a background job or a redirect's `>&`.
                    return None;
                }
                if !cur.is_empty() {
                    cmds.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => {
                word.push(c);
                has_word = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    flush(&mut word, &mut has_word, &mut cur);
    if !cur.is_empty() {
        cmds.push(cur);
    }
    for c in &mut cmds {
        let mut kept = Vec::new();
        let mut skip = false;
        for w in c.drain(..) {
            if skip {
                skip = false;
                continue;
            }
            let redirect = w.trim_start_matches(|c: char| c.is_ascii_digit());
            if redirect.starts_with('>') || redirect.starts_with('<') {
                skip = redirect == ">" || redirect == ">>" || redirect == "<";
                continue;
            }
            kept.push(w);
        }
        *c = kept;
    }
    cmds.retain(|c| !c.is_empty());
    Some(cmds)
}

/// The authored shape of a tool whose input holds the model's own writing.
fn authored(tool: &str, path: Option<&str>, w: &Where) -> Norm {
    let (slots, values) = match path {
        Some(p) => (vec![type_of(p, w)], vec![Raw::new(p)]),
        None => (vec![], vec![]),
    };
    let t = slots.first().map(slot_text).unwrap_or_default();
    Norm {
        shape: Shape {
            tool: tool.into(),
            text: if t.is_empty() {
                format!("{tool}[] AUTHORED")
            } else {
                format!("{tool}[{t}] AUTHORED")
            },
            head: String::new(),
            flags: vec![],
            slots,
            authored: true,
        },
        values,
    }
}

/// Tools whose input is the model's writing, whatever its length.
const WRITES: &[&str] = &[
    "fs.edit",
    "fs.write",
    "fs.patch",
    "text.write",
    "notebook.edit",
];

/// A tool with no special shape: its sorted keys with a typed slot for each
/// short value.
fn generic(tool: &str, input: &Value, w: &Where) -> Norm {
    let mut slots = Vec::new();
    let mut values = Vec::new();
    let mut parts = Vec::new();
    let mut authored = false;
    if let Some(obj) = input.as_object() {
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        for k in keys {
            let v = &obj[k];
            let (ty, raw) = match v {
                Value::String(s) if is_long(s) => {
                    authored = true;
                    continue;
                }
                Value::String(s) => (type_of(s, w), s.clone()),
                Value::Number(n) => (type_of(&n.to_string(), w), n.to_string()),
                Value::Bool(b) => {
                    parts.push(format!("{k}={b}"));
                    continue;
                }
                _ => continue,
            };
            parts.push(format!("{k}={}", slot_text(&ty)));
            slots.push(ty);
            values.push(Raw::new(raw));
        }
    }
    let text = format!(
        "{tool}[{}]{}",
        parts.join(" "),
        if authored { " AUTHORED" } else { "" }
    );
    Norm {
        shape: Shape {
            tool: tool.into(),
            text,
            head: String::new(),
            flags: vec![],
            slots,
            authored,
        },
        values,
    }
}

/// A call's normalized shapes: one for most calls, one for each step of a
/// `proc.run` with `steps` or a `command` line with several commands.
pub fn normalize(tool: &str, input: &Value, w: &Where) -> Vec<Norm> {
    if WRITES.contains(&tool) {
        let path = input.get("path").and_then(Value::as_str);
        return vec![authored(tool, path, w)];
    }
    if tool != "proc.run" {
        return vec![generic(tool, input, w)];
    }
    let mut out = Vec::new();
    let mut argvs: Vec<Vec<String>> = Vec::new();
    if let Some(steps) = input.get("steps").and_then(Value::as_array) {
        argvs.extend(steps.iter().filter_map(|s| strings(s.get("argv")?)));
    } else if let Some(a) = input.get("argv").and_then(strings) {
        argvs.push(a);
    } else if let Some(line) = input.get("command").and_then(Value::as_str) {
        match command_line(line) {
            Some(cmds) if !is_long(line) => argvs.extend(cmds),
            _ => {
                return vec![Norm {
                    shape: Shape {
                        tool: "proc.run".into(),
                        text: "proc.run[sh $STR] AUTHORED".into(),
                        head: "sh".into(),
                        flags: vec![],
                        slots: vec![SlotShape {
                            ty: SlotType::Str,
                            class: String::new(),
                        }],
                        authored: true,
                    },
                    values: vec![Raw::new(line)],
                }]
            }
        }
    }
    for a in &argvs {
        if let Some(n) = program(a, w) {
            out.push(n);
        }
    }
    if out.is_empty() {
        out.push(generic(tool, input, w));
    }
    out
}

/// What a result's text shows (§2.2's table), sorted.
pub fn features(text: &str) -> Vec<&'static str> {
    let mut f = Vec::new();
    let conflict = text.contains("CONFLICT") || text.contains("Automatic merge failed");
    let resolved = text.contains("using previous resolution");
    if conflict {
        f.push("conflict");
    }
    if resolved {
        f.push("rerere-resolved");
    }
    if conflict && text.contains("Recorded preimage for") {
        f.push("rerere-left");
    }
    let test_fail = text
        .lines()
        .any(|l| l.trim_start().starts_with("FAIL ") || l.trim_start().starts_with("FAIL\t"))
        || text.contains("test result: FAILED");
    if test_fail {
        f.push("test-fail");
    }
    if text.contains("nothing to commit") {
        f.push("nothing-to-commit");
    }
    if text.contains("non-fast-forward")
        || text.contains("Not possible to fast-forward")
        || text.contains("cannot fast-forward")
    {
        f.push("non-ff");
    }
    if text.contains("Authentication failed")
        || text.contains("Permission denied (publickey)")
        || text.contains("could not read Username")
        || text.contains("Bad credentials")
    {
        f.push("auth");
    }
    f.sort_unstable();
    f
}

/// The class of a result: `ok`, `err:<exit class>`, or the status of a call
/// that did not run, then the features, joined with `+`.
pub fn result_class(status: &str, is_error: bool, exit: Option<i64>, text: &str) -> String {
    let mut c = match (status, is_error, exit) {
        ("ok", false, Some(0) | None) => "ok".to_string(),
        ("ok" | "error", _, Some(e)) if e != 0 => match e {
            1 => "err:1".into(),
            2 => "err:2".into(),
            126 | 127 => format!("err:{e}"),
            e if e > 128 => "err:signal".into(),
            _ => "err:other".into(),
        },
        ("ok", true, _) | ("error", _, _) => "err".into(),
        (s, _, _) => s.to_string(),
    };
    for f in features(text) {
        c.push('+');
        c.push_str(f);
    }
    c
}

#[cfg(test)]
mod tests;
