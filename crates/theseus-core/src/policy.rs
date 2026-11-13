//! Tool policy (spec §3.9). The finite, named list of tools (and, when MCP
//! lands, of MCP servers) is the surface the operator controls, so the gate
//! decides per tool and never infers what a command does. Every tool and every
//! MCP inherits one posture, with per-tool and per-MCP overrides:
//!
//! | posture   | what happens                       |
//! |-----------|------------------------------------|
//! | `open`    | run                                |
//! | `notify`  | run, and post a structured notice  |
//! | `approve` | wait for the operator's approval   |
//!
//! Nothing the gate decides refuses a call (theseus-8az): the strongest answer
//! is to wait for the operator, who may approve or decline.
//!
//! Evaluated after the toollet has validated its input and named its
//! resources (`Plan`), before anything runs. The first match wins:
//!
//! 1. the floor (Theseus's own binary and state, the 1Password CLI and its
//!    token, and an AWS guardrail: AWS design §3.6) waits for approval at
//!    every posture, and is marked as the floor;
//! 2. an AWS session mint (STS `AssumeRole*` and its kin, theseus-a3s3), a URL
//!    whose host is a private address (DD5), and an AWS call that deletes what
//!    holds state (§3.9) wait for approval. The roots wait for nothing: a
//!    read, a working directory, an argument or a write outside them follows
//!    the tool's posture, whichever tool makes it (theseus-ewi, theseus-m9fdg),
//!    and the decision names the path as `<verb> outside your project folder:
//!    <path> (roots: ...)`, a label and no gate. An `open` posture is raised
//!    to `notify` there, never higher, so the access is never silent;
//! 3. the operator's explicit allow (`allow_argv`) runs, when the call and
//!    every path argument are inside the roots, and nothing on `notify_argv`
//!    matches. An entry is a prefix (`["ls"]` also runs `ls -la src`), so
//!    entries should be narrow. The notify list (`notify_argv`) is read first
//!    and raises the posture to at least `notify`, never higher, with the
//!    decision naming the program: it labels rather than gates, as the roots
//!    do, since the environment Theseus is given is safe by definition, so
//!    `sudo` is worth hearing about and is not worth waiting on (the owner,
//!    2026-11-03 09:01);
//! 4. otherwise the tool's posture: `[policy.tools]`, then for an MCP tool
//!    `[policy.mcp]` "server/tool" and "server", for an AWS call
//!    `[policy.aws]` "service:Operation", "service", and its class (AWS
//!    design §3.9), then `[policy].enforcement`.
//!    A runtime tightening ("should have asked", theseus-sgh) applies here,
//!    after the config's posture, and the stricter of the two wins. So a
//!    tightening never loosens anything, and a config that already asks is
//!    unchanged. Like a config `approve`, it leaves the allow list alone.
//!
//! Reasons are written for the model and the operator to read
//! ("proc.run — approve (enforcement = approve)").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use theseus_tools::{paths, Access, Plan, Tool};

pub mod grants;
pub mod modes;

/// What the gate does with a call: `[policy].enforcement` (the posture every
/// tool and MCP inherits) and each `[policy.tools]` / `[policy.mcp]` override.
/// A config naming any other value fails to load, and the error lists these.
/// They are ordered from the least strict to the strictest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    /// Run.
    #[default]
    Open,
    /// Run, and post a structured notice.
    Notify,
    /// Wait for the operator's approval.
    Approve,
}

impl Posture {
    pub const ALL: [Posture; 3] = [Posture::Open, Posture::Notify, Posture::Approve];

    pub fn as_str(self) -> &'static str {
        match self {
            Posture::Open => "open",
            Posture::Notify => "notify",
            Posture::Approve => "approve",
        }
    }

    /// A posture by its name.
    pub fn parse(s: &str) -> Option<Posture> {
        Posture::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

/// A runtime tightening as the gate reads it (theseus-sgh): the posture a
/// "should have asked" press recorded for one tool, and who pressed.
#[derive(Debug, Clone, Copy)]
pub struct Tightened<'a> {
    pub posture: Posture,
    pub by: &'a str,
}

/// A tool's posture now: the config's, or a tightening's when that is
/// stricter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostureNow {
    pub posture: Posture,
    /// What chose it: the config's setting, or `tightened by <who>`.
    pub setting: String,
    /// What the config alone says.
    pub config: Posture,
    pub config_setting: String,
}

impl PostureNow {
    /// A tightening chose the posture.
    pub fn tightened(&self) -> bool {
        self.posture != self.config
    }

    /// Why, as a reason's parenthesis says it: the setting, and under a
    /// tightening what the config says as well.
    pub(crate) fn why(&self) -> String {
        if self.tightened() {
            format!("{}; the config says {}", self.setting, self.config_setting)
        } else {
            self.setting.clone()
        }
    }
}

/// The structured notice a `notify` posture posts: to the session's channel
/// (Discord, web UI, CLI) and to the ledger (`tool.notified`). The protocol's
/// type, since the gate's record carries it (theseus-0g4).
pub use theseus_protocol::Notice;

/// The policy's verdict on a call. The gate's record keeps it as a
/// `GateDecision` (`Decision::record`), which is its form on the wire.
#[derive(Debug, Clone)]
pub struct Decision {
    /// `Approve` waits for the operator; `Open` and `Notify` run.
    pub posture: Posture,
    pub reason: String,
    /// The notice a `notify` posture posts.
    pub notify: Option<Notice>,
    /// The floor asked: the call touches Theseus's own binary or state, or the
    /// 1Password CLI or token. No setting makes it run unasked.
    pub floor: bool,
    /// What the secret broker gives the call, by name: `gh got GH_TOKEN`
    /// (theseus-dcy). Its tool line and its notice say so.
    pub granted: Option<String>,
    /// What the call reaches outside the roots, worded (theseus-m9fdg): the
    /// labels `decide_with` folded into `reason`, carried so a layer that
    /// loosens the decision keeps them without reading the plan again.
    pub outside: Vec<String>,
}

impl Decision {
    /// The verdict as the gate's record keeps it (theseus-0g4).
    pub fn record(&self) -> theseus_protocol::GateDecision {
        theseus_protocol::GateDecision {
            posture: Some(self.posture.as_str().into()),
            mode: None,
            reason: self.reason.clone(),
            notify: self.notify.clone(),
            floor: self.floor,
            granted: self.granted.clone(),
            // The gate names an L1 call's class (M4 17b).
            class: None,
        }
    }

    pub(crate) fn new(posture: Posture, reason: String) -> Self {
        Self {
            posture,
            reason,
            notify: None,
            floor: false,
            granted: None,
            outside: vec![],
        }
    }

    /// This decision with `label` in its reason and its notice, at the
    /// posture it had: a label never changes what the gate decided
    /// (theseus-m9fdg). The label goes inside the reason's parenthesis, where
    /// `narrative::gate_why` reads the rule from.
    pub fn labelled(mut self, label: &str) -> Self {
        let put = |text: &str| match text.strip_suffix(')') {
            Some(head) => format!("{head}; {label})"),
            None => format!("{text} ({label})"),
        };
        self.reason = put(&self.reason);
        if let Some(n) = &mut self.notify {
            n.rule = put(&n.rule);
        }
        self
    }

    /// This decision at no looser a posture than `need` (theseus-dcy): a call
    /// given a secret runs at least at the secret's posture, whatever chose
    /// its own. `why` is the parenthesis of the new reason, and `setting` the
    /// notice's setting. A decision as strict already is kept.
    pub fn at_least(
        self,
        need: Posture,
        why: &str,
        setting: &str,
        tool: &str,
        summary: &str,
    ) -> Self {
        if need <= self.posture {
            return self;
        }
        let reason = format!("{tool} — {} ({why})", need.as_str());
        let granted = self.granted;
        match need {
            Posture::Approve => Decision {
                granted,
                ..Decision::new(need, format!("{summary}: {reason}"))
            },
            Posture::Notify => Decision {
                notify: Some(Notice {
                    kind: "notify".into(),
                    setting: setting.into(),
                    rule: reason.clone(),
                }),
                granted,
                ..Decision::new(need, reason)
            },
            Posture::Open => Decision {
                granted,
                ..Decision::new(need, reason)
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolPolicy {
    /// Canonical workspace roots: what a call reaches outside them follows the
    /// posture and is labelled with its path (theseus-m9fdg).
    pub roots: Vec<PathBuf>,
    /// `proc.run` argv prefixes that run (open) when every path argument is
    /// inside the roots and no `notify_argv` entry matches.
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that run at a posture raised to at least
    /// `notify`, with the program named.
    pub notify_argv: Vec<Vec<String>>,
    /// The posture every tool and MCP inherits (`[policy].enforcement`).
    pub enforcement: Posture,
    /// Per-tool postures by canonical name (`[policy.tools]`).
    pub tools: BTreeMap<String, Posture>,
    /// Per-MCP postures by `server/tool` or `server` (`[policy.mcp]`).
    pub mcp: BTreeMap<String, Posture>,
    /// An AWS call's postures (`[policy.aws]`, AWS design §3.9): by
    /// `service:Operation`, by `service`, then by class (`read`).
    pub aws: BTreeMap<String, Posture>,
    /// Who confirms (the execution's principal).
    pub confirmer: String,
    /// Paths that always wait for approval: Theseus's store, spool, and
    /// bindings file, and the 1Password CLI's credentials and token (canonical).
    pub floor_paths: Vec<PathBuf>,
    /// Programs that always wait for approval (`theseusd`, `op`).
    pub floor_argv: Vec<Vec<String>>,
    /// A URL whose host is a private address: `Ask` waits for approval,
    /// `Open` judges it as any other (`[policy] private_addresses`,
    /// theseus-7gir.20).
    pub private_addresses: crate::web::net::PrivateAddresses,
}

/// An MCP tool's canonical name is `mcp:<server>/<tool>`.
pub const MCP_PREFIX: &str = "mcp:";

/// Programs Theseus never runs unasked, whatever the posture: its own binary
/// (spec §3.21, the kernel is off limits to the agent) and the 1Password CLI
/// (every secret).
pub fn floor_argv() -> Vec<Vec<String>> {
    vec![vec!["theseusd".into()], vec!["op".into()]]
}

/// What the floor's paths hold, in the operator's words.
const FLOOR_STATE: &str = "Theseus's own state or the 1Password token";

/// One word of a command, as the path it names, resolved against `cwd`:
/// absolute, `~`, `.`-relative, or containing a separator; `--opt=value` is
/// judged by its value. Plain words (`status`, `-la`) are not paths.
fn path_word(a: &str, cwd: &Path) -> Option<PathBuf> {
    let v = match a.split_once('=') {
        Some((k, v)) if k.starts_with('-') => v,
        _ if a.starts_with('-') => return None,
        _ => a,
    };
    let looks = v.starts_with('/') || v.starts_with('~') || v.starts_with('.') || v.contains('/');
    if v.is_empty() || !looks {
        return None;
    }
    let expanded = crate::config::expand_home(v);
    let full = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    Some(paths::canonical_best_effort(&full))
}

/// The command's arguments that name paths, resolved against `cwd`.
fn path_args(argv: &[String], cwd: &Path) -> Vec<(String, PathBuf)> {
    argv.iter()
        .skip(1)
        .filter_map(|a| path_word(a, cwd).map(|p| (a.clone(), p)))
        .collect()
}

/// The letters a short flag carries to mean "what follows is code": `-c`, a
/// command, as POSIX `sh` spells it; `-e`, an expression to evaluate, as the
/// interpreters after it do; `-E`, which is `-e` with the language's later
/// features switched on (`perl -E`) and runs exactly as `-e` does; and `-S`,
/// `env`'s own "split this string into a command line".
///
/// The letters that are deliberately NOT here, because the line between them
/// is the whole of the key's cost: `-p` would make `mkdir -p <dir>` a line
/// and `-r` would make `cp -r <dir>` one, so `node -p` and `php -r` are
/// NAMED in `tests_floor_shapes::RESIDUE` rather than paid for on every
/// ordinary line. `-E` and `-S` earned their upper case by executing under
/// that file's own oracle; neither is a letter an ordinary line passes a
/// path to.
const CODE_LETTERS: [char; 4] = ['c', 'e', 'E', 'S'];

/// The long spellings of the same thing. A long flag means only what it says
/// and nothing by its letters, which is why `--print` can be here while `-p`
/// cannot.
const CODE_FLAGS: [&str; 6] = [
    "--command",
    "--eval",
    "--exec",
    "--execute",
    "--print",
    "--split-string",
];

/// Whether a word is a flag whose ARGUMENT is code: a single-dash cluster
/// holding a code letter (`-c`, `-lc`, `-ec`, `-ic`, `-norc`), or a long
/// code flag. A double dash that is not one of those is not one.
fn introduces_code(w: &str) -> bool {
    match w.starts_with("--") {
        true => CODE_FLAGS.contains(&w),
        false => w
            .strip_prefix('-')
            .is_some_and(|f| f.chars().any(|c| CODE_LETTERS.contains(&c))),
    }
}

/// The code a long flag carries itself, `--command=<line>`.
fn code_in_flag(w: &str) -> Option<&str> {
    let (name, line) = w.split_once('=')?;
    CODE_FLAGS.contains(&name).then_some(line)
}

/// The code a short cluster carries ATTACHED to its own flag,
/// `python3 -c<line>`. `getopt` gives an option its argument from the rest of
/// its cluster, so the line is one argv word together with the flag and there
/// is no next word to read — one SPACE apart from `python3 -c <line>`, and
/// unheld until this was written (the round-2 reviewer's F-2).
///
/// A remainder that is only more flag letters is read as more flags, which is
/// what `getopt` does when those letters take no argument: `bash -ec <line>`
/// sets errexit and the line is still `-c`'s. So the remainder is a line only
/// when it carries something no flag cluster carries — a quote, a bracket, a
/// space, a slash, a dot. The cost of that test is a line that is one bare
/// word (`bash -cls`): it remains a flag cluster rather than code. That is a
/// parsing boundary, not a claim that a one-word program is harmless;
/// `shell_argvs` states the separate line rule and its `find -exec`
/// counter-example.
fn code_attached(w: &str) -> Option<&str> {
    let cluster = w.strip_prefix('-').filter(|c| !c.starts_with('-'))?;
    let at = cluster.find(|c| CODE_LETTERS.contains(&c))?;
    let rest = &cluster[at + 1..];
    (!rest.is_empty() && !rest.chars().all(char::is_alphanumeric)).then_some(rest)
}

/// The words of this argv that a program will INTERPRET, each a line of text
/// to read for the paths its words name rather than a path itself.
///
/// The property, and it is the whole of the rule: **a word is code because
/// the flag in front of it says so**, not because the program in front of
/// that flag is on a list of shells. `sh -c <line>` is POSIX and every
/// interpreter since has spelled it `-c` or `-e`; a single-dash cluster
/// carries the letter alongside other flags (`-lc`, `-ec`, `-ic`), and the
/// long spelling is `--command`/`--eval`, with or without its own `=<line>`.
///
/// Keying on the flag holds one CLASS where keying on the program held a set
/// of spellings. `sh -lc`, `env bash -c`, `timeout 5 /bin/bash -c`,
/// `/usr/bin/env zsh -ic`, `python3 -c`, `perl -e`, a wrapper in front of any
/// of them, and an interpreter this project has never heard of are all the
/// same case now, and not one of them is a name anyone has to maintain. That
/// matters because the list was the defect: a floor keyed on five shell names
/// held `bash -c` and missed `bash -lc`, which bought the owner the cost of
/// the floor and three quarters of its protection.
///
/// Over-recognising is cheap and safe on purpose. A word that is not a line —
/// `tar -czf <archive>`, `grep -c <pattern>` — is still only split and each
/// piece judged by `path_word`, the rule an `argv` word has always taken, so
/// the cost is a few canonicalisations and never a new notion of what a path
/// is. Nothing is expanded and nothing is run.
///
/// What is OUTSIDE the class is code in a position no flag introduces at all:
/// `awk '<program>'`, `ssh <host> '<line>'`. (`find … -exec <program> … \;`
/// was listed here and does not belong: `-exec` is a cluster carrying `e` and
/// `c`, so a flag DOES introduce it — a PROGRAM, which is
/// `introduces_a_program`'s answer and not this key's.) That gap is
/// measured rather than described — `tests_floor_shapes`'s
/// `every_spelling_that_really_runs_a_line_is_held` runs each generated
/// spelling for real and asserts the floor holds every one that executed, so
/// a member of the residue nobody here thought of is a red, not a surprise.
fn code_lines(argv: &[String]) -> Vec<&str> {
    let mut out: Vec<&str> = vec![];
    for (i, w) in argv.iter().enumerate().skip(1) {
        let line = match code_in_flag(w).or_else(|| code_attached(w)) {
            Some(line) => Some(line),
            // The next word, unless it is itself a flag: `sh -e -c <line>`
            // sets errexit, and the line is still the `-c` argument.
            None if introduces_code(w) => argv
                .get(i + 1)
                .filter(|n| !n.starts_with('-'))
                .map(String::as_str),
            None => None,
        };
        if let Some(line) = line.filter(|l| !l.is_empty() && !out.contains(l)) {
            out.push(line);
        }
    }
    out
}

/// A line of text split into the commands a shell would run it as, each as
/// its argv: quoting (`'`, `"`, `\`) groups a word; whitespace and a
/// redirect end a word; and `;`, `|`, `&`, a newline, a subshell's
/// parentheses and a substitution (`$(…)`, backticks) start a new command,
/// whose own words are read in their turn.
///
/// It expands nothing and runs nothing: `$D/AGENTS.md` stays one word whose
/// value only the shell knows. Every word it yields is judged by `path_word`,
/// the same rule an `argv` word takes, and every command's first word by
/// `floor_argv`, the same rule a program takes — so reading a line adds no
/// new notion of what a path or a program is. It only stops the spelling of
/// a call deciding whether the gate sees one.
fn shell_commands(line: &str) -> Vec<Vec<String>> {
    const WORD_ENDS: [char; 2] = ['<', '>'];
    /// What chains one command to the next.
    const CHAIN_ENDS: [char; 3] = [';', '|', '&'];
    /// What groups a command inside another: a subshell, a substitution.
    const GROUP_ENDS: [char; 3] = ['(', ')', '`'];
    // A chain character chains commands only in text that has whitespace
    // somewhere, because passing an argument to anything needs a space: in
    // text with NO whitespace at all, `|` is a substitution expression's own
    // delimiter and not a pipe. Reading it as a pipe held
    // `sed -i -e 's|<floored config>|/tmp/other.toml|' notes.md` and
    // `perl -pe` likewise — ordinary lines that touch nothing floored, two of
    // the round-2 reviewer's seven false positives (F-1, mechanism 2). A
    // whitespace-free chain can still spell bare command words; whether any
    // particular program can affect the vault is a program-semantic question,
    // not a reason to exempt grouping. Grouping is NOT exempted,
    // because an interpreter's own one-liner really does use it
    // (`python3 -c 'open("/x","a").write("y")'` names its path inside
    // parentheses and no space appears in it).
    let chains = line.chars().any(char::is_whitespace);
    let mut out: Vec<Vec<String>> = vec![];
    let mut argv: Vec<String> = vec![];
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    macro_rules! end_word {
        () => {
            if !word.is_empty() {
                argv.push(std::mem::take(&mut word));
            }
        };
    }
    macro_rules! end_command {
        () => {{
            end_word!();
            if !argv.is_empty() {
                out.push(std::mem::take(&mut argv));
            }
        }};
    }
    while let Some(c) = chars.next() {
        match (c, quote) {
            ('\\', Some('\'')) => word.push('\\'),
            ('\\', _) => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            ('\'' | '"', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('$', q) if q != Some('\'') && matches!(chars.peek(), Some('(' | '{')) => {
                chars.next();
                end_command!();
            }
            (c, None)
                if GROUP_ENDS.contains(&c) || c == '\n' || (chains && CHAIN_ENDS.contains(&c)) =>
            {
                end_command!()
            }
            (c, None) if c.is_whitespace() || WORD_ENDS.contains(&c) => end_word!(),
            (c, _) => word.push(c),
        }
    }
    end_command!();
    out
}

/// Every word of a line, for the paths they name. Each word is also offered
/// by its comma-separated pieces, with the whole word kept, for the text
/// that is not a shell line at all — a REPL line `term.send` types, an MCP
/// call's arguments — where a comma separates values and a path that holds
/// one must still be offered unbroken.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = vec![];
    for word in shell_commands(line).into_iter().flatten() {
        if word.contains(',') {
            words.extend(word.split(',').filter(|p| !p.is_empty()).map(String::from));
        }
        words.push(word);
    }
    words
}

/// The argv of each command a line runs, for the programs the floor never
/// runs unasked (`floor_argv`: Theseus's own binary, the 1Password CLI). The
/// leading `VAR=value` words a shell sets for one command are dropped, so
/// `op` is still the program of `OP_ACCOUNT=x op item get …`.
///
/// **A LINE gives up its programs; a single WORD a flag happened to introduce
/// does not.** `grep -c theseusd <log>` makes `theseusd` the argument of a
/// code flag, and reading that one word as a command held an ordinary health
/// check — four of the round-2 reviewer's seven false positives (F-1,
/// mechanism 1), none of them held by main or by the head round 1 reviewed.
///
/// The cost is named rather than hidden: the one-word spelling `bash -c op`
/// no longer reaches the program floor. That is the cheap side of the trade
/// **as a line** — a one-word LINE runs its program with no arguments of its
/// own.
///
/// It is NOT true that such a program can therefore ask nothing of the vault,
/// and the sentence that said so here was false for one real spelling: a flag
/// whose argument is a program takes that program's arguments from the SIBLING
/// argv words, so `find <dir> -exec op item get {} \;` is a one-word line
/// (`op`) whose program runs with real arguments and reaches the 1Password CLI
/// (the round-3 reviewer's probe `R3B-PLANTC`). It is answered in the floor's
/// MATCH (`introduces_a_program`, `runs_program`), because the defect was
/// never that the line was short; `a_one_word_line_is_a_word_not_a_program`
/// pins both halves of the line rule and that case.
fn shell_argvs(line: &str) -> Vec<Vec<String>> {
    let commands = shell_commands(line);
    if commands.iter().map(Vec::len).sum::<usize>() < 2 {
        return vec![];
    }
    commands
        .into_iter()
        .map(|argv| {
            let assignments = argv
                .iter()
                .position(|w| {
                    !w.split_once('=')
                        .is_some_and(|(k, _)| !k.is_empty() && !k.starts_with('-'))
                })
                .unwrap_or(argv.len());
            normalized_argv(&argv[assignments..])
        })
        .filter(|argv| !argv.is_empty())
        .collect()
}

/// What the gate reads of a call's plan (`ToolPolicy::view`).
struct View {
    /// Its resources made canonical.
    resources: Vec<PathBuf>,
    /// Its own argv, the program by base name: the notify and allow lists.
    nargv: Vec<String>,
    /// The path words its own argv names: the roots label and the allow list.
    args: Vec<(String, PathBuf)>,
    /// Every argv it RUNS, its own, each step's, and each command of a line it
    /// runs: the floor's programs, read at every program position
    /// (`runs_program`).
    argvs: Vec<Vec<String>>,
    /// Every argv of text it HANDS on — what `term.send` types, an MCP call's
    /// arguments: the floor's programs, read at a command's first word only
    /// (`prefix_match`). Narrower than `argvs` on purpose, and the reason is
    /// in [`ToolPolicy::floor_view`].
    handed_argvs: Vec<Vec<String>>,
    /// Every path word any shape of the call names: the floor's paths. A
    /// superset of `args` (`ToolPolicy::floor_view`).
    floor_args: Vec<(String, PathBuf)>,
    /// How many lines nested inside this call's lines the floor did NOT read,
    /// because reading them would pass [`LINES_READ`]. Nonzero holds the call:
    /// a ceiling that drops work must fail CLOSED (`ToolPolicy::floor`).
    unread_lines: usize,
}

/// The floor's half of [`View`] ([`ToolPolicy::floor_view`]), named rather
/// than returned as a tuple. Each field is documented on `View`.
struct FloorView {
    argvs: Vec<Vec<String>>,
    handed_argvs: Vec<Vec<String>>,
    args: Vec<(String, PathBuf)>,
    unread_lines: usize,
}

/// How many nested lines the floor reads of one call, so that a line which
/// nests lines cannot cost the gate unbounded work.
///
/// Crossing it HOLDS the call. It used to truncate in silence, and 63 decoy
/// nested calls in one `proc.run {command}` then hid a payload that reached
/// the owner's config at `posture=Open` with `fails_open=true` — a measured
/// bypass of the one layer that is meant to have none (the round-2 reviewer's
/// F-3). A bound that turns the protection off instead of erring is worse
/// than no bound, so the floor now says it ran out of reading rather than
/// passing what it did not read. `a_call_that_nests_past_the_bound_is_held`
/// is the test, at the boundary and not near it.
const LINES_READ: usize = 64;

pub(crate) fn prefix_match(argv: &[String], prefix: &[String]) -> bool {
    !prefix.is_empty() && argv.len() >= prefix.len() && argv.iter().zip(prefix).all(|(a, p)| a == p)
}

/// A flag whose argument is a PROGRAM rather than a line: `find`'s `-exec`
/// and `-execdir`, and the long `--exec`/`--execute` [`CODE_FLAGS`] holds. The
/// flag's own name says it, which is the only thing read here.
///
/// It needs its own answer because an `exec` flag's command takes its
/// arguments from the SIBLING argv words, so reading it as a line yields one
/// word (`op`) and a one-word line is deliberately not a program
/// ([`shell_argvs`]) — which is why `find <dir> -exec op item get {} \;` reached
/// the 1Password CLI with nothing asked.
fn introduces_a_program(w: &str) -> bool {
    w.starts_with('-') && w.trim_start_matches('-').starts_with("exec")
}

/// The code letters a MEASURED false positive needs the PROGRAM-position
/// exclusion for; priced in `a_code_cluster_wrapper_does_not_hide_a_floored_program`.
const PROGRAM_HIDING_LETTERS: [char; 2] = ['c', 'e'];

/// The reserved words no COMMAND can follow, so an argv beginning with one AND
/// carrying the `in` its grammar requires puts its later words in no program
/// position. **The words left out and the `in` test are the point of the list,
/// not gaps in it**: `a_shell_keyword_hides_no_program` pins both ways.
const WORDS_NO_COMMAND_FOLLOWS: [&str; 2] = ["for", "case"];

/// Whether a flag hides the NEXT WORD from the program match, which
/// [`introduces_code`] answered too widely: **`getopt` gives an option its
/// argument from the remainder of its own cluster**, so the next word is the
/// flag's only when the code letter is the cluster's LAST character.
fn hides_the_next_program(w: &str) -> bool {
    match w.strip_prefix('-').filter(|c| !c.starts_with('-')) {
        Some(cluster) => cluster.ends_with(PROGRAM_HIDING_LETTERS),
        None => CODE_FLAGS.contains(&w),
    }
}

/// Whether this command RUNS the floored program `f`, read at every position
/// a program can stand in with no program name consulted: the argv's first
/// word, and every later word that is not itself a flag and that no code flag
/// introduced — except an `exec` flag, which introduces one.
///
/// Nothing says list-free that `nohup` execs its tail, so rather than keep a
/// list of wrappers this reads every position a program could occupy and
/// accepts the over-hold (`a_wrapper_does_not_hide_a_floored_program`):
/// **18 of 23** of this repository's own `scripts/` lines, pinned by
/// `a_word_list_naming_a_floored_program_is_held`. **`23`, not the `24` three
/// reports in this family carried** — that `24` was a hardcoded literal in the
/// measuring harness's own `eprintln!`, so a wrong denominator survived being
/// measured. **And that corpus is blind to the class the code-letter
/// narrowing below creates, where the over-hold goes `1` → `19`, `+18`, so the
/// whole trade is `36 of 46` across both corpora** (46 distinct cases, one
/// shared; `19` held → `36`, `+17`) — bought with twelve live
/// `fails_open=true` paths to the credential tool closed. Excluding a code flag's
/// argument keeps that trade from growing (`grep -c theseusd <log>` is a
/// pattern, four of round 2's seven false positives), but **which flag does
/// the hiding is [`hides_the_next_program`]'s question, not
/// [`introduces_code`]'s**, and reading them as one left `sudo -E op item get
/// --reveal` live.
fn runs_program(argv: &[String], f: &[String]) -> bool {
    // Both reserved words are reserved only in a compound their own `in`
    // clause follows, so a `for` with no `in` is not it and its words stay
    // program positions -- which keeps `{argv:["for","op",...]}` held.
    let word_list_head = argv
        .first()
        .is_some_and(|w| WORDS_NO_COMMAND_FOLLOWS.contains(&w.as_str()))
        && argv.iter().any(|w| w == "in");
    !word_list_head
        && (0..argv.len()).any(|i| {
            !argv[i].starts_with('-')
                && (i == 0
                    || !hides_the_next_program(&argv[i - 1])
                    || introduces_a_program(&argv[i - 1]))
                && prefix_match(&argv[i..], f)
        })
}

/// What of an AWS call waits for the operator at every posture, in the
/// gate's words: an AWS session mint (theseus-a3s3; the owner, 2026-10-07:
/// the operator approves each, and no config makes it looser), or deleting
/// what holds state, a stack, or a change set that replaces or removes it
/// (§3.9's approve list).
fn aws_asks(a: &theseus_tools::AwsPlan) -> Option<String> {
    if let Some(what) = &a.session_mint {
        return Some(what.clone());
    }
    a.destructive.then(|| {
        "destructive: it deletes, replaces, or removes something that holds state".to_string()
    })
}

/// The program name without its directory: `/usr/bin/sudo` matches `sudo`.
pub(crate) fn normalized_argv(argv: &[String]) -> Vec<String> {
    let mut v = argv.to_vec();
    if let Some(first) = v.first_mut() {
        if let Some(base) = Path::new(first.as_str()).file_name() {
            *first = base.to_string_lossy().into_owned();
        }
    }
    v
}

impl ToolPolicy {
    /// A tool's posture and the setting that chose it: `[policy.tools]`, then
    /// for an MCP tool (`mcp:<server>/<tool>`) `[policy.mcp]` "server/tool"
    /// and "server" (an extension's, `ext-<name>`, then notify), for an AWS tool its own class's `[policy.aws]` line
    /// (`aws::tool_class`), then `[policy].enforcement`. An AWS call's own
    /// lines, by its operation, service, and class, are the gate's to read
    /// (`decide_with`).
    pub fn posture(&self, name: &str) -> (Posture, String) {
        if let Some(p) = self.tools.get(name) {
            return (*p, format!("[policy.tools] \"{name}\" = {}", p.as_str()));
        }
        if let Some(full) = name.strip_prefix(MCP_PREFIX) {
            let server = full.split_once('/').map_or(full, |(s, _)| s);
            for key in [full, server] {
                if let Some(p) = self.mcp.get(key) {
                    return (*p, format!("[policy.mcp] \"{key}\" = {}", p.as_str()));
                }
            }
            // An extension's tools run at notify unless its line says
            // otherwise (M7 43b), never looser than the enforcement.
            if server.starts_with(crate::extend::SERVER_PREFIX) {
                let p = self.enforcement.max(Posture::Notify);
                return (p, format!("an extension's default ({})", p.as_str()));
            }
        }
        if let Some(class) = crate::aws::tool_class(name) {
            if let Some(p) = self.aws.get(class) {
                return (*p, format!("[policy.aws] {class} = {}", p.as_str()));
            }
        }
        (
            self.enforcement,
            format!("enforcement = {}", self.enforcement.as_str()),
        )
    }

    /// An AWS call's own line (AWS design §3.9): `[policy.aws]` for its
    /// operation, then for its service, then for its class (`read`, `write`,
    /// `run`). None: the tool's posture decides.
    fn aws_line(
        &self,
        a: &theseus_tools::AwsPlan,
        class: Option<theseus_tools::ToolClass>,
    ) -> Option<(Posture, String)> {
        let op = format!("{}:{}", a.service, a.operation);
        let class = class.map(|c| c.as_str());
        for k in [Some(op.as_str()), Some(a.service.as_str()), class]
            .into_iter()
            .flatten()
        {
            if let Some(p) = self.aws.get(k) {
                let key = if Some(k) == class {
                    k.to_string()
                } else {
                    format!("\"{k}\"")
                };
                return Some((*p, format!("[policy.aws] {key} = {}", p.as_str())));
            }
        }
        None
    }

    /// A tool's posture now: the config's posture, then a tightening, and
    /// the stricter one wins (theseus-sgh).
    pub fn posture_now(&self, name: &str, tightened: Option<Tightened<'_>>) -> PostureNow {
        Self::now_from(self.posture(name), tightened)
    }

    /// The config's posture and its setting, then a tightening.
    pub(crate) fn now_from(
        (config, config_setting): (Posture, String),
        tightened: Option<Tightened<'_>>,
    ) -> PostureNow {
        match tightened {
            Some(t) if t.posture > config => PostureNow {
                posture: t.posture,
                setting: format!("tightened by {}", t.by),
                config,
                config_setting,
            },
            _ => PostureNow {
                posture: config,
                setting: config_setting.clone(),
                config,
                config_setting,
            },
        }
    }

    /// The gate's answer for one call from the config alone.
    pub fn decide(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        self.decide_with(tool, plan, None)
    }

    /// The gate's answer for one call, with the tool's tightening if it has
    /// one; the order is in the module doc.
    pub fn decide_with(
        &self,
        tool: &dyn Tool,
        plan: &Plan,
        tightened: Option<Tightened<'_>>,
    ) -> Decision {
        let name = tool.name();
        let v = self.view(plan);
        let View {
            ref resources,
            ref nargv,
            ref args,
            ..
        } = v;
        let argv: &[String] = plan.argv.as_deref().unwrap_or(&[]);
        if let Some(d) = self.preempted(tool, plan, &v) {
            return d;
        }
        // Nothing outside the roots waits for that alone: the posture below
        // decides, and the decision names the path (theseus-m9fdg).
        let outside = self.outside_roots(plan, resources, args);
        // The notify list: the program runs, and is never silent about it.
        // Read before the allow list, so an argv on both is still heard
        // (the stricter of the two wins, as a tightening does).
        let notify_listed = self
            .notify_argv
            .iter()
            .find(|p| prefix_match(nargv, p))
            .map(|p| {
                format!(
                    "`{}` matches the notify list entry `{}`",
                    argv.join(" "),
                    p.join(" ")
                )
            });
        // The allow list runs a command outright when it and its path
        // arguments stay inside the roots; otherwise the tool's posture
        // decides.
        if let Some(p) = self.allow_argv.iter().find(|p| prefix_match(nargv, p)) {
            if outside.is_empty() && notify_listed.is_none() {
                return Decision::new(
                    Posture::Open,
                    format!(
                        "{name} — open (`{}` matches the allow list entry `{}`)",
                        argv.join(" "),
                        p.join(" ")
                    ),
                );
            }
        }
        // An AWS call with no `[policy.tools]` line takes its operation's,
        // its service's, or its class's `[policy.aws]` line first.
        let line = plan
            .aws
            .as_ref()
            .filter(|_| !self.tools.contains_key(name))
            .and_then(|a| self.aws_line(a, plan.class));
        let now = match line {
            Some(line) => Self::now_from(line, tightened),
            None => self.posture_now(name, tightened),
        };
        // An access outside the roots is at least a notice, so an `open`
        // line (the read tools') cannot make it silent; it is never raised
        // past `notify`, because the roots label and never block
        // (theseus-m9fdg).
        let now = if now.posture == Posture::Open && !outside.is_empty() {
            PostureNow {
                posture: Posture::Notify,
                setting: "outside [tools].roots".into(),
                ..now
            }
        } else {
            now
        };
        // A notify-listed program is at least a notice, for the same reason
        // and in the same way: raised from `open` alone, never past `notify`,
        // and the reason names the program (the owner, 2026-11-03 09:01).
        let now = match (&notify_listed, now.posture) {
            (Some(_), Posture::Open) => PostureNow {
                posture: Posture::Notify,
                setting: "[policy].notify_argv".into(),
                ..now
            },
            _ => now,
        };
        let labels = outside.clone();
        let why = std::iter::once(now.why())
            .chain(notify_listed)
            .chain(outside)
            .collect::<Vec<_>>()
            .join("; ");
        let reason = format!("{name} — {} ({why})", now.posture.as_str());
        let d = match now.posture {
            Posture::Approve => Decision::new(now.posture, format!("{}: {reason}", plan.summary)),
            Posture::Notify => Decision {
                notify: Some(Notice {
                    kind: "notify".into(),
                    setting: now.setting,
                    rule: reason.clone(),
                }),
                ..Decision::new(now.posture, reason)
            },
            Posture::Open => Decision::new(now.posture, reason),
        };
        Decision {
            outside: labels,
            ..d
        }
    }

    /// What the gate reads of a call's plan.
    ///
    /// `resources`, `nargv` and `args` are what the roots label, the notify
    /// list and the allow list have always read: the plan's resources made
    /// canonical, its own argv with the program by base name, and the path
    /// arguments that argv's words name. `argvs` and `floor_args` are the
    /// floor's, and they are wider on purpose — see `floor_view`.
    fn view(&self, plan: &Plan) -> View {
        let resources: Vec<PathBuf> = plan
            .resources
            .iter()
            .map(|r| paths::canonical_best_effort(&r.path))
            .collect();
        // A command's resources are only its working directory; its arguments
        // can name any path, so they are judged too.
        let cwds: Vec<PathBuf> = plan
            .resources
            .iter()
            .zip(&resources)
            .filter(|(r, _)| r.access == Access::Exec)
            .map(|(_, p)| p.clone())
            .collect();
        let cwd = cwds
            .first()
            .cloned()
            .or_else(|| self.roots.first().cloned())
            .unwrap_or_default();
        let argv: &[String] = plan.argv.as_deref().unwrap_or(&[]);
        let nargv = normalized_argv(argv);
        let args = if argv.is_empty() {
            vec![]
        } else {
            path_args(argv, &cwd)
        };
        let f = Self::floor_view(plan, &cwds, &cwd, args.clone());
        View {
            resources,
            nargv,
            args,
            argvs: f.argvs,
            handed_argvs: f.handed_argvs,
            floor_args: f.args,
            unread_lines: f.unread_lines,
        }
    }

    /// What the FLOOR reads of a call beyond the argv's own words, so that a
    /// floored path is held whichever shape of a call names it (the floor
    /// reviewer's F1, 2026-11-10, proven as `b6floor-probe.sh` probe 1): a
    /// `proc.run {command}` line, a `proc.run {steps}` batch, the text
    /// `term.send` types, and an MCP call's arguments all reached a floored
    /// path with nothing asked, while the same path as an `argv` word was
    /// held.
    ///
    /// The four shapes, and where each was blind:
    ///
    /// - **`argv`** — read since the floor existed, by `path_args`.
    /// - **`command`** — `bash -c <line>`, where `path_args` skipped `-c` as a
    ///   flag and took the whole line as one candidate path. The line's own
    ///   words are read now (`shell_line`, `shell_words`).
    /// - **`steps`** — each step's argv, which `view` never read at all,
    ///   because a batch sets `Plan::steps` and leaves `Plan::argv` `None`.
    ///   So neither a step's path words nor a step's PROGRAM (`op`,
    ///   `theseusd`: `floor_argv`) reached the floor. Both do now.
    /// - **`lines`** — text a call hands to another program to read: what
    ///   `term.send` types, an MCP call's arguments.
    ///
    /// It widens the floor and nothing else. The roots label and the allow
    /// list still read the argv's own words (`View::args`), because they
    /// label and loosen rather than hold, and widening them would change
    /// what the owner sees of every shell line he already lives with.
    fn floor_view(
        plan: &Plan,
        cwds: &[PathBuf],
        cwd: &Path,
        mut args: Vec<(String, PathBuf)>,
    ) -> FloorView {
        let own = plan.argv.as_deref().unwrap_or(&[]);
        let steps = plan.steps.as_deref().unwrap_or(&[]);
        let runs = || std::iter::once(own).chain(steps.iter().map(|s| s.as_slice()));
        // A relative word is resolved against every directory the call runs
        // in, since a batch's steps need not share one.
        let roots: Vec<&Path> = match cwds.is_empty() {
            true => vec![cwd],
            false => cwds.iter().map(PathBuf::as_path).collect(),
        };
        // One word is judged once, however many times a line names it: a
        // loop or a long argument list repeats paths, and canonicalising a
        // path costs syscalls.
        let mut seen: Vec<String> = vec![];
        let mut word = |w: &str, args: &mut Vec<(String, PathBuf)>| {
            if seen.iter().any(|s| s == w) {
                return;
            }
            seen.push(w.to_string());
            for root in &roots {
                if let Some(p) = path_word(w, root) {
                    args.push((w.to_string(), p));
                }
            }
        };
        // Every argv the call runs, its own and each step's, and the program
        // of every command inside a line it runs: `proc.run {steps}` left
        // `Plan::argv` `None`, so a batch bypassed the program floor
        // outright, and `bash -c "op item get …"` named no program at all.
        let mut argvs: Vec<Vec<String>> = runs()
            .filter(|a| !a.is_empty())
            .map(normalized_argv)
            .collect();
        // Every word of every argv the call runs, judged as a path. Two of
        // those the gate never judged: the PROGRAM, because `path_args`
        // skips argv's first word, and a program inside Theseus's own state
        // is the floor's business as much as a file it writes; and every
        // word of every step, because the gate read `Plan::argv` alone.
        for a in runs().flatten() {
            word(a, &mut args);
        }
        // Every line of text the call runs or hands on, read for the paths
        // its words name, and then every line THOSE lines interpret in their
        // turn: `bash -c "bash -lc '<line>'"` keeps its inner line inside one
        // quoted word, which is no path, so one pass reads the outer line and
        // stops exactly where the spelling put a shell. A step's line counts
        // too: a batch may hold a `bash -c` step of its own.
        //
        // A line the call RUNS gives the program floor the programs of the
        // commands inside it (`shell_argvs`): `proc.run {steps}` left
        // `Plan::argv` `None`, so a batch bypassed the program floor
        // outright, and `bash -c "op item get …"` named no program at all.
        //
        // Text the call only HANDS on -- what `term.send` types, an MCP call's
        // arguments -- gives its programs too, and that is this row. It gave
        // paths alone, on the reasoning that the gate is not the thing
        // deciding what a REPL will do with a word; what overturned that is
        // the measurement that `term.open {argv:["bash"]}` then
        // `term.send {text:"op item get x --reveal\n"}` was `floor=false
        // fails_open=true` -- a live path to the 1Password CLI with the floor
        // open, and a self session typing it anyway. Deciding CORRECTLY what
        // the far end does with the text needs the list of shell names this
        // row deleted; CLOSING it does not, because `shell_argvs` derives the
        // argv with no program name consulted.
        //
        // The two are read at different WIDTHS, and that is the boundary: a
        // line the call runs is a command we chose, so every program position
        // of it is read (`runs_program`); handed text is read at a command's
        // FIRST word (`prefix_match`), so the cost is text whose command
        // BEGINS `op` or `theseusd`, not every sentence with `op` in the
        // middle. Prose is the common case of text typed at a prompt and the
        // uncommon case of a command we chose to run.
        //
        // The ceiling (`LINES_READ`) is there so a line that nests lines
        // cannot cost the gate unbounded work; nothing anyone writes comes
        // near it, and a call that does come near it is HELD rather than read
        // in part.
        let mut unread_lines = 0usize;
        let mut handed_argvs: Vec<Vec<String>> = vec![];
        let mut lines: Vec<(String, bool)> = runs()
            .flat_map(code_lines)
            .map(|l| (l.to_string(), true))
            .chain(plan.lines.iter().map(|l| (l.clone(), false)))
            .collect();
        let mut next = 0;
        while let Some((line, runs_it)) = lines.get(next).cloned() {
            next += 1;
            for w in shell_words(&line) {
                word(&w, &mut args);
            }
            let inner = shell_argvs(&line);
            for nested in inner.iter().flat_map(|a| code_lines(a)) {
                if lines.iter().any(|(l, _)| l == nested) {
                    continue;
                }
                match lines.len() < LINES_READ {
                    true => lines.push((nested.to_string(), runs_it)),
                    false => unread_lines += 1,
                }
            }
            match runs_it {
                true => argvs.extend(inner),
                false => handed_argvs.extend(inner),
            }
        }
        FloorView {
            argvs,
            handed_argvs,
            args,
            unread_lines,
        }
    }

    /// The answers that come before the allow list and the posture: the floor,
    /// a layer-1 change, a private address, and an AWS call's own asks. Each
    /// waits for approval whatever the tool's posture says.
    fn preempted(&self, tool: &dyn Tool, plan: &Plan, v: &View) -> Option<Decision> {
        let name = tool.name();
        // The floor: Theseus's own state and the vault, and an AWS
        // guardrail (AWS design §3.6), which asks at every posture.
        let guardrail = plan.aws.as_ref().and_then(|a| a.guardrail.clone());
        if let Some(what) = self.floor(v).or(guardrail) {
            return Some(Decision {
                floor: true,
                ..Decision::new(
                    Posture::Approve,
                    format!("{}: {name} — approve (floor: {what})", plan.summary),
                )
            });
        }
        // The operator's authority (39a's layer 1): a task's objective,
        // acceptance, or abandonment waits for the operator's yes at every
        // posture, as the floor does.
        if let Some(what) = &plan.authority {
            return Some(Decision::new(
                Posture::Approve,
                format!("{}: {name} — approve (layer 1: {what})", plan.summary),
            ));
        }
        if let Some(why) = self.private_address(plan) {
            return Some(Decision::new(
                Posture::Approve,
                format!("{}: {name} — approve ({why})", plan.summary),
            ));
        }
        // The approve list's AWS half (§3.9), and an AWS session mint
        // (theseus-a3s3): whatever `[policy.aws]` or `[policy.tools]` says.
        if let Some(why) = plan.aws.as_ref().and_then(aws_asks) {
            return Some(Decision::new(
                Posture::Approve,
                format!("{}: {name} — approve ({why})", plan.summary),
            ));
        }
        None
    }

    /// Whether a call of a self session that waits for approval may go ahead
    /// with a notice instead (theseus-cudar; the owner, 2026-10-19 20:08 and
    /// 2026-10-20 07:26: self-improvement fails open, no answer means go
    /// ahead). Only the soft holds open: a tool's own `approve` posture (the
    /// roots hold nothing, theseus-m9fdg). Everything the owner keeps stays a
    /// wait: the floor (store, spool, bindings, the 1Password token, sealed
    /// sets), a task's layer 1, an AWS call, and a private address. Since
    /// 1.0.4 the approve lists are gone, so nothing they used to hold —
    /// `~/.ssh`, a program on `approve_argv` — reaches here to be weighed.
    pub(crate) fn fails_open(&self, plan: &Plan) -> bool {
        if plan.authority.is_some() || plan.aws.is_some() {
            return false;
        }
        if self.private_address(plan).is_some() {
            return false;
        }
        self.floor(&self.view(plan)).is_none()
    }

    /// Whether only the tool's posture would decide this call: nothing the
    /// floor, a layer-1 change, the approve lists or an AWS rule asks for
    /// comes first. An edits mode (`modes`) loosens that alone.
    pub(crate) fn posture_only(&self, tool: &dyn Tool, plan: &Plan) -> bool {
        self.preempted(tool, plan, &self.view(plan)).is_none()
    }

    /// Every path argument of a command is inside the roots, as an allow
    /// entry requires.
    pub(crate) fn args_within_roots(&self, plan: &Plan) -> bool {
        self.view(plan)
            .args
            .iter()
            .all(|(_, p)| self.within_roots(p))
    }

    fn within_roots(&self, p: &Path) -> bool {
        self.roots.iter().any(|root| paths::within(p, root))
    }

    /// The roots as a label shows them.
    fn roots_shown(&self) -> String {
        if self.roots.is_empty() {
            "none configured, set [tools].projects_dir".into()
        } else {
            let roots: Vec<String> = self.roots.iter().map(|r| r.display().to_string()).collect();
            roots.join(", ")
        }
    }

    /// The label for one path the call reaches outside the roots, in the
    /// owner's words: `read outside your project folder: /etc/hosts (roots:
    /// /w)` (theseus-m9fdg; the owner, 2026-10-30 12:04: "I think this also
    /// should be a 'i did this outside of my project folder' notify, not a
    /// block", and "That whole system of project folders should work that
    /// way generally"). The roots are a label, not a gate: the label rides on
    /// the decision the posture made, in its reason, its notice and so the
    /// ledger, and never changes the posture.
    pub(crate) fn outside_label(&self, access: Access, p: &Path) -> String {
        let verb = match access {
            Access::Read => "read",
            Access::Write => "wrote",
            Access::Exec => "ran",
        };
        format!(
            "{verb} outside your project folder: {} (roots: {})",
            p.display(),
            self.roots_shown()
        )
    }

    /// The labels of what a call reaches outside the roots: its resources
    /// (a path read or written, a working directory) and a command's path
    /// arguments, each once.
    fn outside_roots(
        &self,
        plan: &Plan,
        resources: &[PathBuf],
        args: &[(String, PathBuf)],
    ) -> Vec<String> {
        let mut seen: Vec<&Path> = vec![];
        let mut labels = vec![];
        let reached = resources
            .iter()
            .zip(plan.resources.iter().map(|r| r.access))
            .chain(args.iter().map(|(_, p)| (p, Access::Exec)));
        for (p, access) in reached {
            if !self.within_roots(p) && !seen.contains(&p.as_path()) {
                seen.push(p);
                labels.push(self.outside_label(access, p));
            }
        }
        labels
    }

    /// What of the call is on the floor: a resource, the program, or a path
    /// argument.
    fn floor(&self, v: &View) -> Option<String> {
        let on = |p: &Path| self.floor_paths.iter().any(|f| paths::within(p, f));
        if let Some(p) = v.resources.iter().find(|p| on(p)) {
            return Some(format!("{} is {FLOOR_STATE}", p.display()));
        }
        // A command the call RUNS is read at every program position; text it
        // only HANDS on, at a command's first word (`floor_view`).
        if let Some(f) = self.floor_argv.iter().find(|f| {
            v.argvs.iter().any(|a| runs_program(a, f))
                || v.handed_argvs.iter().any(|a| prefix_match(a, f))
        }) {
            return Some(format!(
                "`{}` is Theseus's own binary or the 1Password CLI",
                f.join(" ")
            ));
        }
        if let Some((a, _)) = v.floor_args.iter().find(|(_, p)| on(p)) {
            return Some(format!("argument `{a}` is {FLOOR_STATE}"));
        }
        // Last, because a call whose floored path WAS found should be told
        // which one: the floor ran out of reading, so it holds rather than
        // passes the part it did not read. 63 decoy nested calls used to hide
        // a payload behind this ceiling in silence.
        (v.unread_lines > 0).then(|| {
            format!(
                "{} line(s) of this call nest past the {LINES_READ} the floor reads, so it is \
                 held rather than read in part",
                v.unread_lines
            )
        })
    }

    /// A URL whose host is a private address (DD5), unless
    /// `private_addresses` is open: the one thing left between the floor and
    /// the posture that waits for approval on its own.
    ///
    /// Nothing a path is or is under waits any more. Until 1.0.4 this read an
    /// approve list of paths and of programs too; the owner retired both on
    /// 2026-11-03 ("why have a configurable list of special directories? We
    /// state that the environment theseus is presented is safe by
    /// definition"), and `notify_argv` now notices the programs rather than
    /// waiting on them. Nothing outside the roots is listed either: a read, a
    /// working directory or a write there follows the tool's posture,
    /// whichever tool makes it, and `outside_roots` names the path
    /// (theseus-ewi for writes, the owner, 2026-09-30: one rule for writes
    /// outside the roots; theseus-m9fdg for the rest), and a notify at least:
    /// an `open` line cannot make it silent. The floor still refuses, for
    /// every tool.
    fn private_address(&self, plan: &Plan) -> Option<String> {
        let ask = self.private_addresses == crate::web::net::PrivateAddresses::Ask;
        plan.url
            .as_deref()
            .filter(|_| ask)
            .and_then(crate::web::net::private_url)
            .map(|why| format!("{why}, and a private address waits for approval"))
    }

    /// Does any resource of this plan write?
    pub fn writes(plan: &Plan) -> bool {
        plan.resources.iter().any(|r| r.access == Access::Write)
    }
}

// This module's own tests live in `policy/tests.rs` (theseus-st2wz): 1,196 of
// this file's lines were its tests, and its `scripts/long-files.txt` note named
// moving them out as the right fix. The module path is unchanged.
#[cfg(test)]
mod tests;
