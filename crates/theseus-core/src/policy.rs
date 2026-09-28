//! Tool policy (spec §3.9): the three-band gate — allow, confirm, deny — over
//! what a call will touch, not over strings. Evaluated after the toollet has
//! validated its input and named its resources (`Plan`), before anything runs.
//! Most restrictive wins. Deny reasons are written for the model and the
//! operator to read ("path /etc/passwd is outside the workspace roots").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use theseus_kernel::{Authority, Policy, PolicyDecision, Proposal};
use theseus_protocol::ConsequenceTag;
use theseus_tools::consequence::{self, Consequence, OwnerRule};
use theseus_tools::shell::{self, Cmd, Word};
use theseus_tools::{paths, Access, Plan, Tool, ToolClass, ToolCtx};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Allow,
    Confirm,
    Deny,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Allow => "allow",
            Mode::Confirm => "confirm",
            Mode::Deny => "deny",
        }
    }
}

/// `[policy].enforcement`: the operator's posture toward the gate's stops, as
/// one setting so they can never combine incoherently: a call against the
/// policy never meets less friction than a call that only needs approval, and
/// an irreversible call never meets less friction than either (spec §3.9).
///
/// | enforcement | needs approval     | irreversible      | against policy            |
/// |-------------|--------------------|-------------------|---------------------------|
/// | `strict`    | waits for Approve  | waits for Approve | refused                   |
/// | `ask`       | waits for Approve  | waits for Approve | waits for Approve, marked |
/// | `notify`    | runs, amber notice | waits for Approve | refused                   |
/// | `open`      | runs, amber notice | waits for Approve | runs, red notice          |
///
/// A call both irreversible and against the policy takes the stricter
/// treatment: refused under `strict` and `notify`, a marked wait under `ask`
/// and `open`. The floor is refused at every level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    #[default]
    Strict,
    Ask,
    Notify,
    Open,
}

impl Enforcement {
    pub fn as_str(self) -> &'static str {
        match self {
            Enforcement::Strict => "strict",
            Enforcement::Ask => "ask",
            Enforcement::Notify => "notify",
            Enforcement::Open => "open",
        }
    }
}

/// Why a call ran that the policy alone would have stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// `approval_skipped` or `off_policy`.
    pub kind: String,
    /// The setting that let it run, e.g. `enforcement = notify`.
    pub setting: String,
    /// What the policy said (the confirm or deny reason).
    pub rule: String,
    /// The consequences it named (never an irreversible one: those wait).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consequences: Vec<ConsequenceTag>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub mode: Mode,
    pub reason: String,
    /// Set when the enforcement level let the call run instead of asking or refusing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<Notice>,
    /// A floor denial: no setting lifts it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
    /// A confirm that stands in for a refusal (`enforcement = ask`, or an
    /// irreversible call against the policy under `open`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub against_policy: bool,
    /// What the call would do to the world, graded (spec §3.9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub consequences: Vec<ConsequenceTag>,
    /// A consequence is irreversible: the call waits at every level.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub irreversible: bool,
    /// The rule table that read the argv (`proc.run`), for replay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<&'static str>,
}

impl Decision {
    fn new(mode: Mode, reason: String) -> Self {
        Self {
            mode,
            reason,
            notify: None,
            floor: false,
            against_policy: false,
            consequences: vec![],
            irreversible: false,
            rules: None,
        }
    }
}

/// The consequence kinds the gate grades: the built-in seeds
/// (`theseus_tools::consequence::SEED_KINDS`) with the owner's
/// `[consequences.kinds]` merged over them.
#[derive(Debug, Clone)]
pub struct Kinds {
    /// kind → irreversible.
    pub grades: BTreeMap<String, bool>,
    pub descriptions: BTreeMap<String, String>,
    /// The owner's argv prefixes (`[consequences.kinds.<k>].argv`).
    pub owner_rules: Vec<OwnerRule>,
}

impl Default for Kinds {
    fn default() -> Self {
        Self::built_in()
    }
}

impl Kinds {
    pub fn built_in() -> Self {
        Self {
            grades: consequence::SEED_KINDS
                .iter()
                .map(|k| (k.name.to_string(), k.irreversible))
                .collect(),
            descriptions: consequence::SEED_KINDS
                .iter()
                .map(|k| (k.name.to_string(), k.description.to_string()))
                .collect(),
            owner_rules: vec![],
        }
    }

    pub fn from_config(cfg: &crate::config::ConsequencesConfig) -> Self {
        let mut k = Self::built_in();
        for (name, c) in &cfg.kinds {
            let grade = c
                .irreversible
                .unwrap_or_else(|| k.grades.get(name).copied().unwrap_or(false));
            k.grades.insert(name.clone(), grade);
            match &c.description {
                Some(d) => {
                    k.descriptions.insert(name.clone(), d.clone());
                }
                None => {
                    k.descriptions.entry(name.clone()).or_default();
                }
            }
            for p in &c.argv {
                k.owner_rules.push(OwnerRule {
                    kind: name.clone(),
                    prefix: p.clone(),
                });
            }
        }
        k
    }

    /// A kind nobody graded needs approval; only a graded kind is irreversible.
    pub fn irreversible(&self, kind: &str) -> bool {
        self.grades.get(kind).copied().unwrap_or(false)
    }

    pub fn tag(&self, c: &Consequence) -> ConsequenceTag {
        ConsequenceTag {
            kind: c.kind.clone(),
            irreversible: self.irreversible(&c.kind),
            rule: c.rule.clone(),
            detail: c.detail.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolPolicy {
    /// Canonical workspace roots.
    pub roots: Vec<PathBuf>,
    /// Canonical protected paths, denied even under a root.
    pub deny_paths: Vec<PathBuf>,
    pub read: Mode,
    pub write: Mode,
    pub run: Mode,
    /// `proc.run` argv prefixes that run without confirmation.
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that never run.
    pub deny_argv: Vec<Vec<String>>,
    /// Per-tool mode overrides by canonical name.
    pub overrides: BTreeMap<String, Mode>,
    /// Who confirms (the execution's principal).
    pub confirmer: String,
    pub enforcement: Enforcement,
    /// Denied whatever the settings say: Theseus's store, spool, and bindings
    /// file, and the 1Password CLI's credentials (canonical paths).
    pub floor_paths: Vec<PathBuf>,
    /// Programs denied whatever the settings say (`theseusd`, `op`).
    pub floor_argv: Vec<Vec<String>>,
    /// Consequence kinds and their grades (`[consequences]`).
    pub kinds: Kinds,
}

/// Programs no setting can make Theseus run: its own binary (spec §3.21, the
/// kernel is off limits to the agent) and the 1Password CLI (every secret).
pub fn floor_argv() -> Vec<Vec<String>> {
    vec![vec!["theseusd".into()], vec!["op".into()]]
}

/// Flags that never run unconfirmed, whatever the allow list says: they make
/// an innocent-looking command write a file (`--output`), read any file
/// (`--no-index`), or run a program configured elsewhere (`--ext-diff`,
/// `--textconv`, `--exec`, `--upload-pack`, `-c`).
const NEVER_UNCONFIRMED: &[&str] = &[
    "--output",
    "--no-index",
    "--ext-diff",
    "--textconv",
    "--exec",
    "--upload-pack",
    "--receive-pack",
    "--config-env",
    "-c",
];

/// The command's arguments that name paths, resolved against `cwd`: absolute,
/// `~`, `.`-relative, or containing a separator; `--opt=value` is judged by
/// its value. Plain words (`status`, `-la`) are not paths.
fn path_args(argv: &[String], cwd: &std::path::Path) -> Vec<(String, PathBuf)> {
    argv.iter()
        .skip(1)
        .filter_map(|a| {
            let v = match a.split_once('=') {
                Some((k, v)) if k.starts_with('-') => v,
                _ if a.starts_with('-') => return None,
                _ => a.as_str(),
            };
            let looks =
                v.starts_with('/') || v.starts_with('~') || v.starts_with('.') || v.contains('/');
            if v.is_empty() || !looks {
                return None;
            }
            let expanded = PathBuf::from(shellexpand::tilde(v).into_owned());
            let full = if expanded.is_absolute() {
                expanded
            } else {
                cwd.join(expanded)
            };
            Some((a.clone(), paths::canonical_best_effort(&full)))
        })
        .collect()
}

fn prefix_match(argv: &[String], prefix: &[String]) -> bool {
    !prefix.is_empty() && argv.len() >= prefix.len() && argv.iter().zip(prefix).all(|(a, p)| a == p)
}

/// The program name without its directory: `/usr/bin/sudo` is `sudo`.
fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// The daemon's HOME (spec §L0: jobs keep HOME), for resolving `~`, `$HOME`, and
/// `${HOME}` in a path judgment only.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// A word begins at a filesystem root: `/`, `~`, `$HOME`, or `${HOME}`.
fn is_rooted(text: &str) -> bool {
    text.starts_with('/')
        || text == "~"
        || text.starts_with("~/")
        || text == "$HOME"
        || text.starts_with("$HOME/")
        || text == "${HOME}"
        || text.starts_with("${HOME}/")
}

/// A word that names a path (not a plain operand like `status`): rooted, `.`-
/// relative, or containing a separator.
fn looks_like_path(v: &str) -> bool {
    !v.is_empty() && (is_rooted(v) || v.starts_with('.') || v.contains('/'))
}

/// The value of an argument that could be a path: `--opt=value` gives its value,
/// a bare `-flag` is not a path, everything else is itself.
fn arg_path_value(t: &str) -> Option<&str> {
    match t.split_once('=') {
        Some((k, v)) if k.starts_with('-') => Some(v),
        _ if t.starts_with('-') => None,
        _ => Some(t),
    }
}

/// Resolve `text` to a concrete path if it names one and its only unresolved
/// part is a leading `~`, `$HOME`, or `${HOME}` (the daemon HOME). Returns
/// `None` for a word with any other expansion the gate cannot resolve.
fn resolve_path_word(text: &str, cwd: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let expanded: String = if text == "$HOME" || text == "${HOME}" {
        home?.to_string_lossy().into_owned()
    } else if let Some(rest) = text.strip_prefix("$HOME/") {
        home?.join(rest).to_string_lossy().into_owned()
    } else if let Some(rest) = text.strip_prefix("${HOME}/") {
        home?.join(rest).to_string_lossy().into_owned()
    } else if text == "~" || text.starts_with("~/") {
        shellexpand::tilde(text).into_owned()
    } else {
        text.to_string()
    };
    if expanded.contains('$') {
        return None;
    }
    let p = PathBuf::from(&expanded);
    let full = if p.is_absolute() { p } else { cwd.join(p) };
    Some(paths::canonical_best_effort(&full))
}

/// Every path a command could touch as (display, canonical): its path-like
/// arguments and its redirection targets, resolved against every directory it
/// might run in (a rooted word needs no directory).
fn command_paths(cmd: &Cmd, home: Option<&Path>) -> Vec<(String, PathBuf)> {
    let resolved = |text: &str| -> Vec<PathBuf> {
        if is_rooted(text) {
            resolve_path_word(text, Path::new("/"), home)
                .into_iter()
                .collect()
        } else {
            cmd.cwds
                .dirs
                .iter()
                .filter_map(|d| resolve_path_word(text, d, home))
                .collect()
        }
    };
    let mut out = vec![];
    for w in cmd.args() {
        // A glob is judged by its pattern (`command_glob_words`), not as a
        // literal path with the metacharacters taken verbatim.
        if w.glob {
            continue;
        }
        if let Some(v) = arg_path_value(&w.text) {
            if looks_like_path(v) {
                out.extend(resolved(v).into_iter().map(|p| (w.text.clone(), p)));
            }
        }
    }
    // A redirection target is a path even when it is a bare word (`> log`).
    for w in &cmd.redirs {
        if w.glob {
            continue;
        }
        out.extend(resolved(&w.text).into_iter().map(|p| (w.text.clone(), p)));
    }
    out
}

/// The directories a command might run in, canonical (`cd X && …` runs in X).
fn command_dirs(cmd: &Cmd) -> Vec<PathBuf> {
    cmd.cwds
        .dirs
        .iter()
        .map(|d| paths::canonical_best_effort(d))
        .collect()
}

/// A short, readable form of a command for a reason string: its resolved words,
/// and where each value it resolved came from (spec §3.9, step 2a.3).
fn cmd_display(cmd: &Cmd) -> String {
    let base = cmd.display();
    if cmd.resolved.is_empty() {
        base
    } else {
        let p = cmd
            .resolved
            .iter()
            .map(|(k, v)| format!("`{k}` = `{v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{base} ({p})")
    }
}

/// The `op` CLI subcommands a mention scan looks for after the word `op`.
const OP_SUBCOMMANDS: &[&str] = &[
    "read",
    "inject",
    "run",
    "item",
    "vault",
    "document",
    "signin",
    "signout",
    "whoami",
    "account",
    "user",
    "group",
    "connect",
    "service-account",
    "events-api",
    "plugin",
    "completion",
    "update",
];

/// Does `text` mention `word` as a whole word (not inside a longer identifier)?
fn mentions_word(text: &str, word: &str) -> bool {
    let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
    text.match_indices(word).any(|(i, _)| {
        boundary(text[..i].chars().next_back()) && boundary(text[i + word.len()..].chars().next())
    })
}

/// Does `text` mention `op <subcommand>` (the CLI, by any path)? The word `op`
/// may be followed by any run of separators — whitespace, quotes, commas,
/// brackets, parentheses, or shell operators — then a subcommand, so inline code
/// like `subprocess.run(["op", "read", …])` counts, not only `op read`.
fn mentions_op(text: &str) -> bool {
    let toks: Vec<&str> = text
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '(' | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | ','
                        | ';'
                        | '&'
                        | '|'
                        | '"'
                        | '\''
                        | '`'
                        | '='
                )
        })
        .filter(|t| !t.is_empty())
        .collect();
    toks.windows(2)
        .any(|w| basename(w[0]) == "op" && OP_SUBCOMMANDS.contains(&w[1]))
}

/// The contents of `"…"` and `'…'` string literals in `text`, for the floor's
/// scan of inline code: a literal whose content is exactly a floor program (or
/// ends in `/op` or `/theseusd`) is a call to it (`subprocess.run(["op", …])`,
/// `system("/usr/bin/op")`).
fn quoted_literals(text: &str) -> Vec<String> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < cs.len() {
        let q = cs[i];
        if q == '"' || q == '\'' {
            if let Some(k) = cs[i + 1..].iter().position(|&c| c == q) {
                out.push(cs[i + 1..i + 1 + k].iter().collect());
                i += k + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Is a floor program named as a string literal in `text`?
fn floor_program_literal(text: &str) -> bool {
    quoted_literals(text)
        .iter()
        .any(|s| s == "op" || s == "theseusd" || s.ends_with("/op") || s.ends_with("/theseusd"))
}

/// Path components (the `Normal` parts) of an absolute path, glob characters and
/// all; the root and any prefix are dropped so two paths compare component-wise.
fn path_comps(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Does one glob component pattern match a literal path component? Supports `*`,
/// `?`, and `[…]`; `*` and `?` match a leading dot, to over-approximate
/// `dotglob`. A `[…]` class is over-approximated as a single-character wildcard.
fn glob_seg_match(pat: &str, lit: &str) -> bool {
    fn m(p: &[char], s: &[char]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some(('*', rest)) => m(rest, s) || (!s.is_empty() && m(p, &s[1..])),
            Some(('?', rest)) => !s.is_empty() && m(rest, &s[1..]),
            Some(('[', rest)) => {
                let after = rest
                    .iter()
                    .position(|&c| c == ']')
                    .map(|k| &rest[k + 1..])
                    .unwrap_or(&[]);
                !s.is_empty() && m(after, &s[1..])
            }
            Some((c, rest)) => !s.is_empty() && s[0] == *c && m(rest, &s[1..]),
        }
    }
    m(
        &pat.chars().collect::<Vec<_>>(),
        &lit.chars().collect::<Vec<_>>(),
    )
}

/// Resolve a leading `~`, `$HOME`, or `${HOME}` (the daemon HOME) in a glob
/// pattern; the rest, glob characters included, is left untouched.
fn resolve_glob_home(pattern: &str, home: Option<&Path>) -> String {
    if pattern == "~" || pattern.starts_with("~/") {
        return shellexpand::tilde(pattern).into_owned();
    }
    if let Some(h) = home {
        if pattern == "$HOME" || pattern == "${HOME}" {
            return h.display().to_string();
        }
        if let Some(r) = pattern.strip_prefix("$HOME/") {
            return h.join(r).to_string_lossy().into_owned();
        }
        if let Some(r) = pattern.strip_prefix("${HOME}/") {
            return h.join(r).to_string_lossy().into_owned();
        }
    }
    pattern.to_string()
}

/// Could the glob `pattern` (run in `cwd`, `~`/`$HOME` resolved) name `target`
/// or something inside it? Match component by component. The pattern must have
/// at least as many components as the target: a shorter pattern names an
/// ancestor, left to the deny list as a non-glob ancestor already is, so
/// `ls ~/*` stays allowed.
fn glob_reaches(pattern: &str, cwd: &Path, home: Option<&Path>, target: &Path) -> bool {
    let base = resolve_glob_home(pattern, home);
    let full = if Path::new(&base).is_absolute() {
        PathBuf::from(&base)
    } else {
        cwd.join(&base)
    };
    let canon = paths::canonical_best_effort(&full);
    let pat = path_comps(&canon);
    let tgt = path_comps(target);
    if pat.is_empty() || pat.len() < tgt.len() {
        return false;
    }
    tgt.iter().zip(&pat).all(|(t, p)| glob_seg_match(p, t))
}

/// The glob path words of a command: its path-like glob arguments and glob
/// redirection targets, judged by pattern (not resolved to a concrete path).
fn command_glob_words(cmd: &Cmd) -> Vec<&Word> {
    let mut out = vec![];
    for w in cmd.args() {
        if w.glob {
            if let Some(v) = arg_path_value(&w.text) {
                if looks_like_path(v) {
                    out.push(w);
                }
            }
        }
    }
    for w in &cmd.redirs {
        if w.glob {
            out.push(w);
        }
    }
    out
}

/// Could any glob path word of `cmd` name `target` or something inside it, under
/// any directory the command might run in?
fn cmd_glob_reaches<'a>(cmd: &'a Cmd, home: Option<&Path>, target: &Path) -> Option<&'a Word> {
    command_glob_words(cmd).into_iter().find(|w| {
        if is_rooted(&w.text) {
            glob_reaches(&w.text, Path::new("/"), home, target)
        } else {
            cmd.cwds
                .dirs
                .iter()
                .any(|d| glob_reaches(&w.text, d, home, target))
        }
    })
}

/// The spellings of a floor path a mention scan searches raw text for: its
/// canonical absolute form, its `~/` and `$HOME/` forms, and — for a file — its
/// file name.
fn floor_path_forms(f: &Path, home: Option<&Path>) -> Vec<String> {
    let mut forms = vec![f.display().to_string()];
    if let Some(h) = home {
        if let Ok(rel) = f.strip_prefix(h) {
            let rel = rel.display();
            forms.push(format!("~/{rel}"));
            forms.push(format!("$HOME/{rel}"));
            forms.push(format!("${{HOME}}/{rel}"));
        }
    }
    if f.is_file() {
        if let Some(name) = f.file_name() {
            forms.push(name.to_string_lossy().into_owned());
        }
    }
    forms
}

/// The program name without its directory: `/usr/bin/sudo` matches `sudo`.
fn normalized_argv(argv: &[String]) -> Vec<String> {
    let mut v = argv.to_vec();
    if let Some(first) = v.first_mut() {
        if let Some(base) = std::path::Path::new(first.as_str()).file_name() {
            *first = base.to_string_lossy().into_owned();
        }
    }
    v
}

impl ToolPolicy {
    pub fn class_mode(&self, tool: &dyn Tool) -> Mode {
        if let Some(m) = self.overrides.get(tool.name()) {
            return *m;
        }
        match tool.class() {
            ToolClass::Read => self.read,
            ToolClass::Write => self.write,
            ToolClass::Run => self.run,
        }
    }

    /// The gate's answer at the operator's enforcement level (the table on
    /// `Enforcement`): the policy's band, then the consequences' column. The
    /// floor is decided first and never lifted.
    pub fn decide(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        use Enforcement::*;
        let d = self.decide_by_policy(tool, plan);
        if d.floor {
            return d;
        }
        let (tags, rules) = self.consequences(plan);
        let irreversible = tags.iter().any(|t| t.irreversible);
        let named = ConsequenceTag::summary(&tags);
        let setting = format!("enforcement = {}", self.enforcement.as_str());
        let what = if d.mode == Mode::Confirm {
            d.reason.clone()
        } else {
            format!("{}: {}", plan.summary, d.reason)
        };
        let notice = |kind: &str, rule: String| Notice {
            kind: kind.into(),
            setting: setting.clone(),
            rule,
            consequences: tags.clone(),
        };
        let mut out = match (d.mode, irreversible, self.enforcement) {
            // Against the policy and irreversible: the stricter treatment.
            (Mode::Deny, true, Strict | Notify) => {
                Decision::new(Mode::Deny, format!("{} (and it is {named})", d.reason))
            }
            (Mode::Deny, true, Ask | Open) => Decision {
                against_policy: true,
                ..Decision::new(
                    Mode::Confirm,
                    format!("against policy and {named}: {}", d.reason),
                )
            },
            (Mode::Deny, false, Ask) => Decision {
                against_policy: true,
                ..Decision::new(Mode::Confirm, format!("against policy: {}", d.reason))
            },
            (Mode::Deny, false, Open) => Decision {
                notify: Some(notice("off_policy", d.reason.clone())),
                against_policy: true,
                ..Decision::new(Mode::Allow, format!("ran against policy: {}", d.reason))
            },
            (Mode::Deny, false, Strict | Notify) => d,
            // Irreversible: waits for approval at every level.
            (Mode::Allow | Mode::Confirm, true, _) => Decision::new(
                Mode::Confirm,
                format!("{named}: an irreversible call waits for approval at every enforcement level ({setting}). {what}"),
            ),
            // Needs approval, by the tool's class or by a consequence.
            (Mode::Confirm, false, Notify | Open) => Decision {
                notify: Some(notice("approval_skipped", d.reason.clone())),
                ..Decision::new(Mode::Allow, format!("ran without approval: {}", d.reason))
            },
            (Mode::Allow, false, Notify | Open) if !tags.is_empty() => Decision {
                notify: Some(notice("approval_skipped", format!("{named}: {what}"))),
                ..Decision::new(Mode::Allow, format!("ran without approval: {named}: {what}"))
            },
            (Mode::Allow, false, Strict | Ask) if !tags.is_empty() => {
                Decision::new(Mode::Confirm, format!("{named}: {what}"))
            }
            (Mode::Allow | Mode::Confirm, false, _) => d,
        };
        out.consequences = tags;
        out.irreversible = irreversible;
        out.rules = rules;
        out
    }

    /// Every consequence of the plan, graded: the toollet's own (layer 1)
    /// and, for an argv, the rule table's and the owner's (layer 2, which also
    /// names what it cannot see through as `opaque`).
    pub fn consequences(&self, plan: &Plan) -> (Vec<ConsequenceTag>, Option<&'static str>) {
        let mut found: Vec<Consequence> = plan.consequences.clone();
        let mut rules = None;
        if let Some(argv) = &plan.argv {
            for c in consequence::detect(argv, &self.exec_cwd(plan), &self.kinds.owner_rules) {
                if !found.contains(&c) {
                    found.push(c);
                }
            }
            rules = Some(consequence::RULES_VERSION);
        }
        (found.iter().map(|c| self.kinds.tag(c)).collect(), rules)
    }

    /// Where a program runs: its `Exec` resource, else the first root.
    fn exec_cwd(&self, plan: &Plan) -> PathBuf {
        plan.resources
            .iter()
            .find(|r| r.access == Access::Exec)
            .map(|r| paths::canonical_best_effort(&r.path))
            .or_else(|| self.roots.first().cloned())
            .unwrap_or_default()
    }

    /// The policy alone: floor, protected paths, roots, class, argv lists.
    /// The floor, checked first and refused at every level (spec §3.9): Theseus's
    /// own state and binary, and the 1Password CLI and its credentials. It judges
    /// every command a call starts, not just the top-level program: a floor
    /// program among a command's words or (for a one-word entry) its `via` chain;
    /// a floor path among a command's path arguments, redirection targets, or the
    /// directories it runs in; and a floor mention inside code the gate cannot
    /// parse. `$HOME`/`${HOME}`/`~` resolve to the daemon HOME for path judgments.
    fn floor_over_commands(&self, cmds: &[Cmd], home: Option<&Path>) -> Option<Decision> {
        let floor = |reason: String| Decision {
            floor: true,
            ..Decision::new(Mode::Deny, reason)
        };
        let never = "no setting allows it";
        for cmd in cmds {
            // The program is matched by its file name, so a relative path reaches
            // the floor just as the top-level check reduces an absolute one:
            // `./target/release/theseusd` and `target/debug/theseusd` are both
            // `theseusd`.
            let mut words: Vec<String> = cmd.words.iter().map(|w| w.text.clone()).collect();
            if !cmd.dynamic_program() {
                if let Some(first) = words.first_mut() {
                    *first = basename(first).to_string();
                }
            }
            for f in &self.floor_argv {
                if prefix_match(&words, f) {
                    return Some(floor(format!(
                        "`{}` is never run: `{}` is on the floor (Theseus's own binary or the 1Password CLI); {never}",
                        cmd_display(cmd),
                        f.join(" ")
                    )));
                }
                if f.len() == 1 && cmd.via.iter().any(|p| basename(p) == f[0]) {
                    return Some(floor(format!(
                        "`{}` (inside `{}`) is never run: `{}` is on the floor; {never}",
                        f[0],
                        cmd.via
                            .iter()
                            .map(|p| basename(p))
                            .collect::<Vec<_>>()
                            .join(" "),
                        f[0]
                    )));
                }
            }
            if !self.floor_paths.is_empty() {
                for (disp, p) in command_paths(cmd, home) {
                    if let Some(f) = self.floor_paths.iter().find(|f| paths::within(&p, f)) {
                        return Some(floor(format!(
                            "`{}` names `{disp}`, which is on the floor ({}: Theseus's own or its secrets); {never}",
                            cmd_display(cmd),
                            f.display()
                        )));
                    }
                }
                for d in command_dirs(cmd) {
                    if let Some(f) = self.floor_paths.iter().find(|f| paths::within(&d, f)) {
                        return Some(floor(format!(
                            "`{}` runs inside the floor ({}); {never}",
                            cmd_display(cmd),
                            f.display()
                        )));
                    }
                }
                // A glob path argument or redirection target, judged by pattern.
                for f in &self.floor_paths {
                    if let Some(w) = cmd_glob_reaches(cmd, home, f) {
                        return Some(floor(format!(
                            "`{}` globs `{}` into the floor ({}: it could name it or something inside it); {never}",
                            cmd_display(cmd),
                            w.text,
                            f.display()
                        )));
                    }
                }
            }
            if consequence::unreadable(cmd) {
                if let Some(reason) = self.floor_mention(cmd, home) {
                    return Some(floor(reason));
                }
            }
        }
        None
    }

    /// A floor mention in a command the gate cannot parse (inline interpreter
    /// code, `eval`, a dynamic program, a remote command, or unreadable text).
    /// The reason says the refusal came from an unparsable mention, so a model
    /// can rephrase an innocent call. It looks for, in the command's raw text:
    /// a floor path (any form); `op://`, a 1Password secret reference; a floor
    /// program named as a string literal (`"op"`, `'theseusd'`, `"/usr/bin/op"`);
    /// `theseusd` as a word; or `op` followed by a subcommand across separators
    /// (`"op", "read"`). For a dynamic program word it also scans the word's own
    /// spelling, substitutions included (`$(echo op)`, `` `which theseusd` ``).
    fn floor_mention(&self, cmd: &Cmd, home: Option<&Path>) -> Option<String> {
        let text = &cmd.text;
        let unparsable =
            "in code the gate cannot parse; rephrase so the gate can see it does not touch the floor";
        if cmd.dynamic_program() {
            let spell = cmd.program_spelling();
            for name in ["theseusd", "op"] {
                if mentions_word(&spell, name) {
                    return Some(format!(
                        "`{}` builds its program from `{}`, which names `{name}` (on the floor) {unparsable}",
                        cmd_display(cmd),
                        spell.trim()
                    ));
                }
            }
        }
        for f in &self.floor_paths {
            for form in floor_path_forms(f, home) {
                if text.contains(&form) {
                    return Some(format!(
                        "`{}` mentions `{form}` (on the floor) {unparsable}",
                        cmd_display(cmd)
                    ));
                }
            }
        }
        if text.contains("op://") {
            return Some(format!(
                "`{}` mentions an `op://` secret reference {unparsable}",
                cmd_display(cmd)
            ));
        }
        if floor_program_literal(text) {
            return Some(format!(
                "`{}` names a floor program as a string literal {unparsable}",
                cmd_display(cmd)
            ));
        }
        if mentions_word(text, "theseusd") {
            return Some(format!(
                "`{}` mentions `theseusd` {unparsable}",
                cmd_display(cmd)
            ));
        }
        if mentions_op(text) {
            return Some(format!(
                "`{}` mentions the `op` CLI {unparsable}",
                cmd_display(cmd)
            ));
        }
        None
    }

    /// The deny list over every command a call starts, the same traversal as the
    /// floor but following the ladder: `deny_argv` over a command's words and
    /// `via`, `deny_paths` over its path arguments, redirection targets, and the
    /// directories it runs in. No mention scan (that is the floor's alone).
    fn deny_over_commands(&self, cmds: &[Cmd], home: Option<&Path>) -> Option<Decision> {
        for cmd in cmds {
            // The program is matched by its file name, as the floor does.
            let mut words: Vec<String> = cmd.words.iter().map(|w| w.text.clone()).collect();
            if !cmd.dynamic_program() {
                if let Some(first) = words.first_mut() {
                    *first = basename(first).to_string();
                }
            }
            for e in &self.deny_argv {
                if prefix_match(&words, e) {
                    return Some(Decision::new(
                        Mode::Deny,
                        format!(
                            "`{}` matches the deny list entry `{}`",
                            cmd_display(cmd),
                            e.join(" ")
                        ),
                    ));
                }
                if e.len() == 1 && cmd.via.iter().any(|p| basename(p) == e[0]) {
                    return Some(Decision::new(
                        Mode::Deny,
                        format!(
                            "`{}` runs `{}` (a wrapper on the deny list)",
                            cmd_display(cmd),
                            e[0]
                        ),
                    ));
                }
            }
            if !self.deny_paths.is_empty() {
                for (disp, p) in command_paths(cmd, home) {
                    if let Some(d) = self.deny_paths.iter().find(|d| paths::within(&p, d)) {
                        return Some(Decision::new(
                            Mode::Deny,
                            format!(
                                "argument `{disp}` is protected ({} is on the deny list)",
                                d.display()
                            ),
                        ));
                    }
                }
                for dir in command_dirs(cmd) {
                    if let Some(d) = self.deny_paths.iter().find(|d| paths::within(&dir, d)) {
                        return Some(Decision::new(
                            Mode::Deny,
                            format!(
                                "`{}` runs inside a protected directory ({} is on the deny list)",
                                cmd_display(cmd),
                                d.display()
                            ),
                        ));
                    }
                }
                for d in &self.deny_paths {
                    if let Some(w) = cmd_glob_reaches(cmd, home, d) {
                        return Some(Decision::new(
                            Mode::Deny,
                            format!(
                                "`{}` globs `{}` into a protected path ({} is on the deny list)",
                                cmd_display(cmd),
                                w.text,
                                d.display()
                            ),
                        ));
                    }
                }
            }
        }
        None
    }

    fn decide_by_policy(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        let floor = |reason: String| Decision {
            floor: true,
            ..Decision::new(Mode::Deny, reason)
        };
        let home = home_dir();
        let cmds: Vec<Cmd> = plan
            .argv
            .as_ref()
            .map(|argv| shell::commands(argv, &self.exec_cwd(plan)))
            .unwrap_or_default();
        // The floor, first and refused at every level.
        for r in &plan.resources {
            let p = paths::canonical_best_effort(&r.path);
            if let Some(f) = self.floor_paths.iter().find(|f| paths::within(&p, f)) {
                return floor(format!(
                    "{} is Theseus's own or holds its secrets ({}); no setting allows it",
                    p.display(),
                    f.display()
                ));
            }
        }
        if let Some(d) = self.floor_over_commands(&cmds, home.as_deref()) {
            return d;
        }
        // The deny list and the roots (over the plan's own resources).
        for r in &plan.resources {
            let p = paths::canonical_best_effort(&r.path);
            if let Some(d) = self.deny_paths.iter().find(|d| paths::within(&p, d)) {
                return Decision::new(
                    Mode::Deny,
                    format!(
                        "{} is protected ({} is on the deny list)",
                        p.display(),
                        d.display()
                    ),
                );
            }
            if !self.roots.iter().any(|root| paths::within(&p, root)) {
                let roots: Vec<String> =
                    self.roots.iter().map(|r| r.display().to_string()).collect();
                return Decision::new(
                    Mode::Deny,
                    format!(
                        "{} is outside the workspace roots ({})",
                        p.display(),
                        if roots.is_empty() {
                            "none configured: set [tools].projects_dir".into()
                        } else {
                            roots.join(", ")
                        }
                    ),
                );
            }
        }
        let mut mode = self.class_mode(tool);
        let mut reason = format!(
            "{} is `{}` for {} tools",
            tool.name(),
            mode.as_str(),
            tool.class().as_str()
        );
        if let Some(argv) = &plan.argv {
            let nargv = normalized_argv(argv);
            // The deny list judges every command the call starts, and its `via`.
            if let Some(d) = self.deny_over_commands(&cmds, home.as_deref()) {
                return d;
            }
            // The resources above are only the working directory; a command's
            // arguments can name any path, so they are judged too.
            let args = path_args(argv, &self.exec_cwd(plan));
            if mode != Mode::Deny {
                if let Some(p) = self.allow_argv.iter().find(|p| prefix_match(&nargv, p)) {
                    let entry = format!(
                        "`{}` matches the allow list entry `{}`",
                        argv.join(" "),
                        p.join(" ")
                    );
                    let flag = argv.iter().skip(1).find(|a| {
                        NEVER_UNCONFIRMED
                            .iter()
                            .any(|f| a.as_str() == *f || a.starts_with(&format!("{f}=")))
                    });
                    let outside = args
                        .iter()
                        .find(|(_, p)| !self.roots.iter().any(|root| paths::within(p, root)));
                    match (flag, outside) {
                        (Some(f), _) => {
                            reason = format!("{entry}, but `{f}` never runs unconfirmed")
                        }
                        (None, Some((a, _))) => {
                            reason = format!(
                                "{entry}, but its argument `{a}` is outside the workspace roots"
                            )
                        }
                        (None, None) => {
                            mode = Mode::Allow;
                            reason = entry;
                        }
                    }
                }
            }
        }
        if mode == Mode::Confirm {
            reason = format!("{}: {}", plan.summary, reason);
        }
        Decision::new(mode, reason)
    }

    /// Does any resource of this plan write?
    pub fn writes(plan: &Plan) -> bool {
        plan.resources.iter().any(|r| r.access == Access::Write)
    }
}

/// How a decision treated a call, in the words `policy.replay` prints.
pub fn treatment(mode: &str, notice: Option<&str>) -> &'static str {
    match (mode, notice) {
        ("allow", Some("off_policy")) => "ran against policy",
        ("allow", Some(_)) => "ran without approval",
        ("allow", None) => "ran",
        ("confirm", _) => "waited",
        _ => "refused",
    }
}

impl ToolPolicy {
    /// `policy.rules`: the kinds as graded and every detection rule.
    pub fn rules_info(&self) -> theseus_protocol::PolicyRulesResult {
        use theseus_protocol::{KindInfo, RuleInfo};
        let seed = |n: &str| consequence::SEED_KINDS.iter().find(|k| k.name == n);
        let kinds = self
            .kinds
            .grades
            .iter()
            .map(|(name, irreversible)| KindInfo {
                name: name.clone(),
                irreversible: *irreversible,
                description: self
                    .kinds
                    .descriptions
                    .get(name)
                    .cloned()
                    .unwrap_or_default(),
                source: match seed(name) {
                    Some(k)
                        if k.irreversible == *irreversible
                            && self.kinds.descriptions.get(name).map(String::as_str)
                                == Some(k.description) =>
                    {
                        "built_in"
                    }
                    Some(_) => "regraded",
                    None => "config",
                }
                .into(),
            })
            .collect();
        let mut rules: Vec<RuleInfo> = consequence::RULES
            .iter()
            .map(|r| RuleInfo {
                id: r.id.into(),
                kind: r.kind.into(),
                why: r.why.into(),
                must: r.must.iter().map(|x| x.to_string()).collect(),
                must_not: r.must_not.iter().map(|x| x.to_string()).collect(),
                source: "built_in".into(),
            })
            .collect();
        rules.extend(self.kinds.owner_rules.iter().map(|o| RuleInfo {
            id: format!("config:{}", o.kind),
            kind: o.kind.clone(),
            why: format!("argv starts with `{}`", o.prefix.join(" ")),
            must: vec![],
            must_not: vec![],
            source: "config".into(),
        }));
        theseus_protocol::PolicyRulesResult {
            rules_version: consequence::RULES_VERSION.into(),
            enforcement: self.enforcement.as_str().into(),
            kinds,
            rules,
        }
    }

    /// Judge one past call again: what the stored gate record says happened,
    /// and what the current gate (rules, kinds, and settings) would decide for
    /// the same plan. Read-only; `None` for a call that never passed
    /// validation or whose tool is gone.
    pub fn replay_call(
        &self,
        tool: &dyn Tool,
        ctx: &ToolCtx,
        input: &serde_json::Value,
        gate: &serde_json::Value,
    ) -> Option<(
        theseus_protocol::ReplayVerdict,
        theseus_protocol::ReplayVerdict,
        String,
    )> {
        use theseus_protocol::ReplayVerdict;
        if gate["validated"] != serde_json::Value::Bool(true) {
            return None;
        }
        let d = &gate["decision"];
        let then = ReplayVerdict {
            treatment: treatment(
                d["mode"].as_str().unwrap_or("deny"),
                d["notify"]["kind"].as_str(),
            )
            .into(),
            consequences: serde_json::from_value(d["consequences"].clone()).unwrap_or_default(),
            rules: d["rules"].as_str().map(String::from),
        };
        // The plan as judged then (its resources and argv), with the
        // toollet's current declarations.
        let replanned = tool.plan(input, ctx).ok();
        let mut plan: Plan = match serde_json::from_value(gate["plan"].clone()) {
            Ok(p) => p,
            Err(_) => replanned.clone()?,
        };
        if let Some(p) = replanned {
            plan.consequences = p.consequences;
        }
        let now_d = self.decide(tool, &plan);
        let now = ReplayVerdict {
            treatment: treatment(
                now_d.mode.as_str(),
                now_d.notify.as_ref().map(|n| n.kind.as_str()),
            )
            .into(),
            consequences: now_d.consequences,
            rules: now_d.rules.map(String::from),
        };
        Some((then, now, plan.summary))
    }
}

/// Did the verdict change: another treatment, or other kinds or grades?
pub fn verdict_changed(
    then: &theseus_protocol::ReplayVerdict,
    now: &theseus_protocol::ReplayVerdict,
) -> bool {
    let kinds = |v: &theseus_protocol::ReplayVerdict| {
        let mut k: Vec<(String, bool)> = v
            .consequences
            .iter()
            .map(|t| (t.kind.clone(), t.irreversible))
            .collect();
        k.sort();
        k.dedup();
        k
    };
    then.treatment != now.treatment || kinds(then) != kinds(now)
}

/// The kernel's gate ordering (transform → validate → policy) over a toollet:
/// `validate` is the toollet's own typed parse, `decide` is `ToolPolicy`.
pub struct GatePolicy<'a> {
    pub tool: &'a dyn Tool,
    pub ctx: &'a ToolCtx,
    pub policy: &'a ToolPolicy,
    pub plan: std::sync::Mutex<Option<Plan>>,
    pub decision: std::sync::Mutex<Option<Decision>>,
}

impl<'a> GatePolicy<'a> {
    pub fn new(tool: &'a dyn Tool, ctx: &'a ToolCtx, policy: &'a ToolPolicy) -> Self {
        Self {
            tool,
            ctx,
            policy,
            plan: Default::default(),
            decision: Default::default(),
        }
    }
}

impl Policy for GatePolicy<'_> {
    fn validate(&self, p: &Proposal) -> Result<(), String> {
        let plan = self.tool.plan(&p.args, self.ctx)?;
        *self.plan.lock().unwrap() = Some(plan);
        Ok(())
    }
    fn decide(&self, _p: &Proposal, _auth: &Authority) -> PolicyDecision {
        let plan = self.plan.lock().unwrap().clone().unwrap_or_default();
        let d = self.policy.decide(self.tool, &plan);
        let out = match d.mode {
            Mode::Allow => PolicyDecision::Allow,
            Mode::Confirm => PolicyDecision::Confirm {
                by: self.policy.confirmer.clone(),
            },
            Mode::Deny => PolicyDecision::Deny {
                reason: d.reason.clone(),
            },
        };
        *self.decision.lock().unwrap() = Some(d);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use theseus_tools::{Resource, Retry};

    struct T(ToolClass);
    impl Tool for T {
        fn name(&self) -> &'static str {
            "fs.test"
        }
        fn description(&self) -> &'static str {
            ""
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        fn class(&self) -> ToolClass {
            self.0
        }
        fn retry(&self) -> Retry {
            Retry::SafeToRepeat
        }
        fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
            Ok(Plan::default())
        }
    }

    fn policy(root: &std::path::Path) -> ToolPolicy {
        ToolPolicy {
            roots: vec![root.to_path_buf()],
            deny_paths: vec![root.join("secret")],
            read: Mode::Allow,
            write: Mode::Confirm,
            run: Mode::Confirm,
            allow_argv: vec![vec!["git".into(), "status".into()]],
            deny_argv: vec![vec!["sudo".into()]],
            overrides: BTreeMap::new(),
            confirmer: "operator".into(),
            enforcement: Enforcement::Strict,
            floor_paths: vec![root.join("state")],
            floor_argv: floor_argv(),
            kinds: Kinds::built_in(),
        }
    }

    fn plan(path: std::path::PathBuf, access: Access, argv: Option<Vec<&str>>) -> Plan {
        Plan {
            resources: vec![Resource { path, access }],
            argv: argv.map(|v| v.into_iter().map(String::from).collect()),
            summary: "s".into(),
            consequences: vec![],
        }
    }

    #[test]
    fn roots_deny_list_argv_lists_and_class_modes() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let p = policy(&root);
        let r = T(ToolClass::Read);
        let w = T(ToolClass::Write);
        let x = T(ToolClass::Run);
        assert_eq!(
            p.decide(&r, &plan(root.join("a.rs"), Access::Read, None))
                .mode,
            Mode::Allow
        );
        assert_eq!(
            p.decide(&w, &plan(root.join("a.rs"), Access::Write, None))
                .mode,
            Mode::Confirm
        );
        let out = p.decide(&r, &plan("/etc/passwd".into(), Access::Read, None));
        assert_eq!(out.mode, Mode::Deny);
        assert!(
            out.reason.contains("outside the workspace roots"),
            "{}",
            out.reason
        );
        let out = p.decide(&r, &plan(root.join("secret/key"), Access::Read, None));
        assert_eq!(out.mode, Mode::Deny);
        assert!(out.reason.contains("protected"));
        assert_eq!(
            p.decide(
                &x,
                &plan(
                    root.clone(),
                    Access::Exec,
                    Some(vec!["git", "status", "-s"])
                )
            )
            .mode,
            Mode::Allow
        );
        assert_eq!(
            p.decide(
                &x,
                &plan(root.clone(), Access::Exec, Some(vec!["cargo", "test"]))
            )
            .mode,
            Mode::Confirm
        );
        let out = p.decide(
            &x,
            &plan(
                root.clone(),
                Access::Exec,
                Some(vec!["/usr/bin/sudo", "ls"]),
            ),
        );
        assert_eq!(out.mode, Mode::Deny, "{}", out.reason);
        // Arguments are judged too: a protected one is denied for any command;
        // an allow-listed command runs unconfirmed only with every path inside
        // the roots and no write- or exec-capable flag.
        let run = |argv: Vec<&str>| p.decide(&x, &plan(root.clone(), Access::Exec, Some(argv)));
        let secret = format!("{}/secret/key", root.display());
        let out = run(vec!["cat", &secret]);
        assert_eq!(out.mode, Mode::Deny, "{}", out.reason);
        assert!(out.reason.contains("is protected"), "{}", out.reason);
        let out = run(vec!["git", "status", "--output=/tmp/x"]);
        assert_eq!(out.mode, Mode::Confirm, "{}", out.reason);
        assert!(
            out.reason.contains("never runs unconfirmed"),
            "{}",
            out.reason
        );
        let out = run(vec!["git", "status", "/etc"]);
        assert_eq!(out.mode, Mode::Confirm, "{}", out.reason);
        assert!(
            out.reason.contains("outside the workspace roots"),
            "{}",
            out.reason
        );
        assert_eq!(
            run(vec!["git", "status", ".."]).mode,
            Mode::Confirm,
            "the parent of the root"
        );
        assert_eq!(
            run(vec!["git", "status", "src/lib.rs"]).mode,
            Mode::Allow,
            "inside the root"
        );
        assert_eq!(run(vec!["git", "status", "-s"]).mode, Mode::Allow);
        // The enforcement ladder: every level is coherent, and the floor holds.
        let at = |e: Enforcement| {
            let mut q = p.clone();
            q.enforcement = e;
            q
        };
        let write = plan(root.join("a.rs"), Access::Write, None);
        let outside = plan("/etc/passwd".into(), Access::Read, None);
        let sudo = plan(root.clone(), Access::Exec, Some(vec!["sudo", "ls"]));
        let s = at(Enforcement::Strict);
        assert_eq!(s.decide(&w, &write).mode, Mode::Confirm);
        assert_eq!(s.decide(&r, &outside).mode, Mode::Deny);
        let a = at(Enforcement::Ask);
        assert_eq!(a.decide(&w, &write).mode, Mode::Confirm);
        let out = a.decide(&r, &outside);
        assert_eq!(
            out.mode,
            Mode::Confirm,
            "ask: a refusal becomes a marked confirm"
        );
        assert!(
            out.against_policy && out.reason.starts_with("against policy"),
            "{}",
            out.reason
        );
        let n = at(Enforcement::Notify);
        let out = n.decide(&w, &write);
        assert_eq!(out.mode, Mode::Allow);
        assert_eq!(out.notify.as_ref().unwrap().kind, "approval_skipped");
        assert_eq!(out.notify.unwrap().setting, "enforcement = notify");
        assert_eq!(
            n.decide(&r, &outside).mode,
            Mode::Deny,
            "notify never lifts a refusal"
        );
        let o = at(Enforcement::Open);
        let out = o.decide(&r, &outside);
        assert_eq!(out.mode, Mode::Allow);
        let nt = out.notify.unwrap();
        assert_eq!(nt.kind, "off_policy");
        assert!(
            nt.rule.contains("outside the workspace roots"),
            "{}",
            nt.rule
        );
        assert_eq!(
            o.decide(&x, &sudo).mode,
            Mode::Allow,
            "open runs the deny list too"
        );
        for e in [
            Enforcement::Strict,
            Enforcement::Ask,
            Enforcement::Notify,
            Enforcement::Open,
        ] {
            let q = at(e);
            for (what, pl) in [
                ("state", plan(root.join("state/store"), Access::Read, None)),
                (
                    "theseusd",
                    plan(root.clone(), Access::Exec, Some(vec!["theseusd", "config"])),
                ),
                (
                    "op",
                    plan(
                        root.clone(),
                        Access::Exec,
                        Some(vec!["/usr/bin/op", "read", "x"]),
                    ),
                ),
            ] {
                let out = q.decide(&x, &pl);
                assert_eq!(
                    out.mode,
                    Mode::Deny,
                    "the floor holds at {}: {what}",
                    e.as_str()
                );
                assert!(out.floor && out.notify.is_none() && !out.against_policy);
            }
        }
        let mut p2 = p.clone();
        p2.overrides.insert("fs.test".into(), Mode::Allow);
        assert_eq!(
            p2.decide(&w, &plan(root.join("a.rs"), Access::Write, None))
                .mode,
            Mode::Allow
        );
    }

    /// A native toollet that declares a consequence from its arguments (layer 1).
    struct Publishes;
    impl Tool for Publishes {
        fn name(&self) -> &'static str {
            "pkg.publish"
        }
        fn description(&self) -> &'static str {
            ""
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        fn class(&self) -> ToolClass {
            ToolClass::Read
        }
        fn retry(&self) -> Retry {
            Retry::NonRepeatable
        }
        fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
            Ok(Plan::default())
        }
    }

    #[test]
    fn irreversible_waits_at_every_level_and_the_stricter_treatment_wins() {
        use Enforcement::*;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let x = T(ToolClass::Run);
        let at = |e: Enforcement| {
            let mut q = policy(&root);
            q.enforcement = e;
            q
        };
        let run = |argv: Vec<&str>| plan(root.clone(), Access::Exec, Some(argv));
        let force = run(vec!["git", "push", "--force", "origin", "main"]);
        let shell = run(vec!["bash", "-c", "git push -f origin main"]);
        for e in [Strict, Ask, Notify, Open] {
            for pl in [&force, &shell] {
                let out = at(e).decide(&x, pl);
                assert_eq!(
                    out.mode,
                    Mode::Confirm,
                    "irreversible waits at {}",
                    e.as_str()
                );
                assert!(out.irreversible && !out.against_policy && out.notify.is_none());
                assert_eq!(out.consequences[0].kind, "history_rewrite");
                assert_eq!(out.consequences[0].rule, "git.push.force");
                assert!(
                    out.reason.starts_with("irreversible: history_rewrite"),
                    "{}",
                    out.reason
                );
                assert_eq!(out.rules, Some(consequence::RULES_VERSION));
            }
        }
        // A plain push is no consequence: at notify it runs with the usual notice.
        let out = at(Notify).decide(&x, &run(vec!["git", "push", "origin", "main"]));
        assert_eq!(out.mode, Mode::Allow);
        assert!(out.consequences.is_empty() && !out.irreversible);
        assert_eq!(out.notify.unwrap().kind, "approval_skipped");
        // Irreversible and against the policy (`sudo` is on the deny list):
        // refused under strict and notify, a marked wait under ask and open.
        let both = run(vec!["sudo", "git", "push", "-f"]);
        for (e, mode) in [
            (Strict, Mode::Deny),
            (Ask, Mode::Confirm),
            (Notify, Mode::Deny),
            (Open, Mode::Confirm),
        ] {
            let out = at(e).decide(&x, &both);
            assert_eq!(out.mode, mode, "both at {}: {}", e.as_str(), out.reason);
            assert!(
                out.irreversible && out.notify.is_none(),
                "never runs unasked at {}",
                e.as_str()
            );
            assert_eq!(out.against_policy, mode == Mode::Confirm);
            assert!(
                out.reason.contains("irreversible: history_rewrite"),
                "{}",
                out.reason
            );
        }
        // The floor is decided first, whatever the call would also do.
        let mut floor = run(vec!["theseusd", "config"]);
        floor.argv = Some(vec!["op".into(), "item".into(), "get".into()]);
        for e in [Strict, Ask, Notify, Open] {
            let out = at(e).decide(&x, &floor);
            assert!(out.floor && out.mode == Mode::Deny, "{}", e.as_str());
        }
        // A needs-approval kind raises an allowed call, and notices name it.
        let mut p = policy(&root);
        p.allow_argv.push(vec!["gh".into()]);
        let merge = run(vec!["gh", "pr", "merge", "12"]);
        let out = p.decide(&x, &merge);
        assert_eq!(out.mode, Mode::Confirm, "strict: {}", out.reason);
        assert!(
            out.reason.starts_with("needs approval: merge"),
            "{}",
            out.reason
        );
        p.enforcement = Notify;
        let out = p.decide(&x, &merge);
        assert_eq!(out.mode, Mode::Allow);
        let n = out.notify.unwrap();
        assert_eq!(n.consequences[0].kind, "merge");
        assert!(!n.consequences[0].irreversible);
        // `opaque` keeps today's treatment (needs approval) and is named.
        let out = at(Notify).decide(&x, &run(vec!["python3", "-c", "import os"]));
        assert_eq!(out.mode, Mode::Allow);
        assert_eq!(
            ConsequenceTag::summary(&out.notify.unwrap().consequences),
            "needs approval: opaque"
        );
        assert_eq!(
            at(Strict).decide(&x, &run(vec!["make", "deploy"])).mode,
            Mode::Confirm
        );
        // The owner regrades a kind: merges now wait under notify too.
        let mut q = at(Notify);
        let mut cfg = crate::config::ConsequencesConfig::default();
        cfg.kinds.insert(
            "merge".into(),
            crate::config::KindConfig {
                irreversible: Some(true),
                ..Default::default()
            },
        );
        cfg.kinds.insert(
            "deploy".into(),
            crate::config::KindConfig {
                irreversible: Some(true),
                argv: vec![vec!["./deploy.sh".into()]],
                ..Default::default()
            },
        );
        q.kinds = Kinds::from_config(&cfg);
        assert_eq!(
            q.decide(&x, &run(vec!["gh", "pr", "merge", "1"])).mode,
            Mode::Confirm
        );
        let out = q.decide(&x, &run(vec!["bash", "-c", "./deploy.sh --now"]));
        assert_eq!(out.mode, Mode::Confirm);
        assert_eq!(
            ConsequenceTag::summary(&out.consequences),
            "irreversible: deploy · needs approval: opaque"
        );
        // Layer 1: a toollet's own declaration is graded the same way, even
        // for a class the policy allows.
        let mut pl = Plan::default();
        pl.consequences.push(Consequence {
            kind: "publish".into(),
            rule: "toollet:pkg.publish".into(),
            detail: "publish pkg 1.0".into(),
        });
        let out = at(Open).decide(&Publishes, &pl);
        assert_eq!(out.mode, Mode::Confirm);
        assert!(out.irreversible && out.rules.is_none());
    }

    /// Step 2a+: the floor and the deny list judge every command a call starts,
    /// not just the top-level program, and the floor scans code it cannot parse.
    #[test]
    fn the_floor_and_deny_list_judge_every_command_a_call_starts() {
        use Enforcement::*;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let x = T(ToolClass::Run);
        let at = |e: Enforcement| {
            let mut q = policy(&root);
            q.enforcement = e;
            q
        };
        let exec = |argv: Vec<&str>| plan(root.clone(), Access::Exec, Some(argv));
        let store_file = format!("{}/state/store/0001", root.display());
        let state_dir = format!("{}/state", root.display());
        // Every one of these reaches the floor through a wrapper, a shell string,
        // an argument, a redirection, or a directory it runs in.
        let bash_op = "op read op://v/i/f".to_string();
        let bash_theseusd = "theseusd config".to_string();
        let bash_cat = format!("cat {store_file}");
        let cat_redir = format!("cat < {store_file}");
        let cd_floor = format!("cd {state_dir} && cat config");
        let floors: Vec<Vec<&str>> = vec![
            vec!["env", "op", "read", "op://v/i/f"], // op through a wrapper
            vec!["bash", "-c", &bash_op],            // op inside a shell string
            vec!["bash", "-c", &bash_theseusd],      // theseusd inside a shell string
            vec!["cat", &store_file],                // a floor path as an argument
            vec!["bash", "-c", &bash_cat],           // a floor path inside a string
            vec!["bash", "-c", &cat_redir],          // a floor path as a redirection
            vec!["bash", "-c", &cd_floor],           // a command that runs in the floor
        ];
        for argv in &floors {
            for e in [Strict, Ask, Notify, Open] {
                let out = at(e).decide(&x, &exec(argv.clone()));
                assert!(
                    out.floor && out.mode == Mode::Deny && out.notify.is_none(),
                    "floor must hold at {} for {argv:?}: {}",
                    e.as_str(),
                    out.reason
                );
            }
        }
        // The deny list, blind inside shell strings before, now follows the
        // ladder: `sudo` inside `bash -c` is refused under strict and notify.
        let bash_sudo = exec(vec!["bash", "-c", "sudo ls"]);
        assert_eq!(at(Strict).decide(&x, &bash_sudo).mode, Mode::Deny);
        assert_eq!(at(Notify).decide(&x, &bash_sudo).mode, Mode::Deny);
        let out = at(Open).decide(&x, &bash_sudo);
        assert_eq!(
            out.mode,
            Mode::Allow,
            "open runs the deny list inside a string"
        );
        assert_eq!(out.notify.unwrap().kind, "off_policy");
        // A mention inside code the gate cannot parse is a floor refusal, and the
        // reason says so, so a model can rephrase.
        let py_op = exec(vec![
            "python3",
            "-c",
            "import os; os.system(\"op read op://v/i\")",
        ]);
        let out = at(Notify).decide(&x, &py_op);
        assert!(out.floor && out.mode == Mode::Deny, "{}", out.reason);
        assert!(out.reason.contains("cannot parse"), "{}", out.reason);
        let py_td = exec(vec!["python3", "-c", "call theseusd now"]);
        assert!(at(Open).decide(&x, &py_td).floor);
        // Benign inline code is not floored (it is opaque, needs approval).
        let py_ok = exec(vec!["python3", "-c", "print(1)"]);
        assert!(!at(Notify).decide(&x, &py_ok).floor);
        // `$HOME`, `${HOME}`, and `~` resolve to the daemon HOME for the floor.
        if let Some(home) = home_dir() {
            let mut q = policy(&root);
            let hf = paths::canonical_best_effort(&home.join(".theseus-2aplus-floortest"));
            q.floor_paths.push(hf);
            for spelling in [
                "cat $HOME/.theseus-2aplus-floortest/x",
                "cat ${HOME}/.theseus-2aplus-floortest/x",
                "cat ~/.theseus-2aplus-floortest/x",
            ] {
                for e in [Strict, Open] {
                    q.enforcement = e;
                    let out = q.decide(&x, &exec(vec!["bash", "-c", spelling]));
                    assert!(
                        out.floor,
                        "HOME floor at {} for {spelling}: {}",
                        e.as_str(),
                        out.reason
                    );
                }
            }
        }
    }

    /// Step 2a++: the floor closes on a program word built at run time, natural
    /// inline code, a relative program path, and a glob through a floor path.
    #[test]
    fn round_two_floor_closes_run_time_programs_inline_code_relative_paths_and_globs() {
        use Enforcement::*;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let x = T(ToolClass::Run);
        let at = |e: Enforcement| {
            let mut q = policy(&root);
            q.enforcement = e;
            q
        };
        let exec = |argv: Vec<&str>| plan(root.clone(), Access::Exec, Some(argv));
        let rs = root.display().to_string();
        // A: run-time program words, natural inline code, relative program paths.
        let a_prog_var = "x=op; $x read op://v/i/f".to_string();
        let a_prog_sub = "$(echo op) read op://v/i/f".to_string();
        let a_prog_td = "x=theseusd; $x --version".to_string();
        let a_py =
            "import subprocess; subprocess.run([\"op\",\"read\",\"op://v/i/f\"])".to_string();
        let a_rel = "./target/release/theseusd config".to_string();
        let a_rel2 = "target/debug/theseusd config".to_string();
        // A4: a glob through a floor path (`state` is the floor here).
        let a_glob1 = format!("cat {rs}/sta*/store/wal");
        let a_glob2 = format!("cat {rs}/*/store/wal");
        let floors: Vec<Vec<&str>> = vec![
            vec!["bash", "-c", &a_prog_var],
            vec!["bash", "-c", &a_prog_sub],
            vec!["bash", "-c", &a_prog_td],
            vec!["python3", "-c", &a_py],
            vec!["perl", "-e", "system(\"op\", \"read\", \"op://v/i/f\")"],
            vec!["bash", "-c", &a_rel],
            vec!["bash", "-c", &a_rel2],
            vec!["bash", "-c", &a_glob1],
            vec!["bash", "-c", &a_glob2],
        ];
        for argv in &floors {
            for e in [Strict, Ask, Notify, Open] {
                let out = at(e).decide(&x, &exec(argv.clone()));
                assert!(
                    out.floor && out.mode == Mode::Deny && out.notify.is_none(),
                    "floor must hold at {} for {argv:?}: {}",
                    e.as_str(),
                    out.reason
                );
            }
        }
        // Precision: a glob whose fewer components name an ancestor stays off the
        // floor (`ls ~/*` is not `~/.theseus/store`); a benign relative program
        // and a benign inline print are not floored.
        let allowed_glob = exec(vec!["bash", "-c", "ls ~/*"]);
        assert!(!at(Open).decide(&x, &allowed_glob).floor);
        let ok_rel = exec(vec!["bash", "-c", "./scripts/build.sh"]);
        assert!(!at(Notify).decide(&x, &ok_rel).floor);
        let ok_py = exec(vec!["python3", "-c", "print(1)"]);
        assert!(!at(Notify).decide(&x, &ok_py).floor);
        // `grep -r "op://" crates` and `rg 'op read' docs` are parsed working
        // commands, not inline code, so the mention scan never runs on them.
        for argv in [
            vec!["grep", "-r", "op://", "crates"],
            vec!["rg", "op read", "docs"],
        ] {
            let out = at(Open).decide(&x, &exec(argv));
            assert!(
                !out.floor,
                "a parsed command is not mention-scanned: {}",
                out.reason
            );
        }
    }

    /// Step 2a.3: the floor resolves a variable used as the program through a
    /// wrapper, `eval`, a nested `bash -c`, or an embedded reference, and stays
    /// off a benign resolved program.
    #[test]
    fn round_three_floor_resolves_variables_through_wrappers_and_reparses() {
        use Enforcement::*;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let x = T(ToolClass::Run);
        let at = |e: Enforcement| {
            let mut q = policy(&root);
            q.enforcement = e;
            q
        };
        let exec = |argv: Vec<&str>| plan(root.clone(), Access::Exec, Some(argv));
        // A variable reaches the program through a wrapper, `eval`, a nested
        // `bash -c`, or an embedded reference; each resolves to a floor program.
        let floors = [
            "x=op; command $x whoami",
            "x=op; exec \"$x\" whoami",
            "x=op; env $x whoami",
            "x=op; timeout 5 $x whoami",
            "x=op; eval \"$x whoami\"",
            "x=op; bash -c \"$x whoami\"",
            "x=o; y=p; $x$y whoami",
            "a=these; b=usd; command $a$b --version",
        ];
        for spelling in floors {
            for e in [Strict, Ask, Notify, Open] {
                let out = at(e).decide(&x, &exec(vec!["bash", "-c", spelling]));
                assert!(
                    out.floor && out.mode == Mode::Deny && out.notify.is_none(),
                    "floor must hold at {} for `{spelling}`: {}",
                    e.as_str(),
                    out.reason
                );
            }
        }
        // Precision: a variable that resolves to a harmless program is not floored.
        let ok = exec(vec!["bash", "-c", "x=ls; $x -la"]);
        assert!(
            !at(Open).decide(&x, &ok).floor,
            "{}",
            at(Open).decide(&x, &ok).reason
        );
        // The refusal reason shows the resolved program and where it came from.
        let out = at(Notify).decide(&x, &exec(vec!["bash", "-c", "x=op; $x read op://v/i"]));
        assert!(out.floor, "{}", out.reason);
        assert!(
            out.reason.contains("op read op://v/i") && out.reason.contains("`$x` = `op`"),
            "reason shows the resolved command and provenance: {}",
            out.reason
        );
    }

    /// FAST (spec v0.36): the gate must stay well under a millisecond even on a
    /// long, nested `bash -c` string. Prints the measured per-call time; the
    /// bound is loose (debug build) to catch only a pathological regression.
    #[test]
    fn decide_is_fast_on_a_long_shell_string_and_a_plain_argv() {
        use std::time::Instant;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let p = policy(&root);
        let x = T(ToolClass::Run);
        // ~2 KB, 30 commands, with wrappers, redirections, and nested
        // substitutions — the shape a rule-heavy loop produces.
        let mut cmds = vec![];
        let rootd = root.display();
        for i in 0..30 {
            cmds.push(format!(
                "sudo env A=1 git -C sub{i} status > out{i}.log 2>&1 && x=$(echo $(basename {rootd}/p{i}))"
            ));
        }
        let script = cmds.join(" ; ");
        assert!(script.len() > 1800, "script is {} bytes", script.len());
        // Step 2a.3: 20 literal assignments and 30 references over the long
        // string, so resolution runs on every command.
        let mut heavy = cmds.clone();
        for i in 0..20 {
            heavy.push(format!("v{i}=val{i}"));
        }
        for i in 0..30 {
            heavy.push(format!("git push origin \"$v{}\"", i % 20));
        }
        let heavy_script = heavy.join(" ; ");
        let bash = plan(
            root.clone(),
            Access::Exec,
            Some(vec!["bash", "-c", script.as_str()]),
        );
        let resolve = plan(
            root.clone(),
            Access::Exec,
            Some(vec!["bash", "-c", heavy_script.as_str()]),
        );
        let plain = plan(
            root.clone(),
            Access::Exec,
            Some(vec!["git", "push", "--force", "origin", "main"]),
        );
        let time = |pl: &Plan| {
            let iters = 200u32;
            // Warm up, then measure.
            for _ in 0..20 {
                std::hint::black_box(p.decide(&x, pl));
            }
            let t = Instant::now();
            for _ in 0..iters {
                std::hint::black_box(p.decide(&x, pl));
            }
            t.elapsed() / iters
        };
        let bash_per = time(&bash);
        let resolve_per = time(&resolve);
        let plain_per = time(&plain);
        println!(
            "FAST decide: bash -c (~{}B) = {bash_per:?}, +20 assigns/30 refs (~{}B) = {resolve_per:?}, plain argv = {plain_per:?}",
            script.len(),
            heavy_script.len()
        );
        assert!(
            bash_per < std::time::Duration::from_millis(5),
            "decide on a long bash -c string was {bash_per:?} (debug); expected well under 1ms in release"
        );
        assert!(
            resolve_per < std::time::Duration::from_millis(8),
            "decide with resolution was {resolve_per:?} (debug); expected well under 1ms in release"
        );
        assert!(
            plain_per < std::time::Duration::from_millis(2),
            "{plain_per:?}"
        );
    }
}
