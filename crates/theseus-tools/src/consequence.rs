//! Consequences (spec §3.9): what a call would do to the world, beside what
//! kind of tool it is. One property carries the policy, `irreversible`; the
//! kinds are its reasons, what a notice names ("irreversible:
//! history_rewrite"), and what policy grades (theseus-core, `[consequences]`).
//!
//! Detection is deterministic, in layers:
//! 1. a native toollet declares its consequences in its `Plan`, from its typed
//!    arguments;
//! 2. `proc.run` is matched against `RULES`, a versioned table over the argv
//!    and every command a `bash -c` string would run (`shell`). Each rule
//!    carries examples it must and must not match, run as tests;
//! 3. a call the rules cannot see through (inline interpreter code, `eval`, a
//!    script, a task runner, text the parser cannot read) is `opaque`.
//!
//! A plain `git push` is no consequence. A recursive delete is `bulk_delete`
//! unless every target is regenerable (`regenerable`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::shell::{self, expand_tilde, Cmd, Cwds, Word};

/// The rule table's version: recorded with every detection, so a replay can
/// say which table judged a call. Bumped whenever detection changes: 2a+ made
/// the parser read every word (brace expansion, unresolved words, prefix
/// options, glob-then-`..`); 2a++ made a quoted variable a possible option, gave
/// `$@`/`$*` bash's word semantics, and reached a run-time program word; 2a.3
/// resolves every simple variable reference from the script's literal
/// assignments (through wrappers, `eval`, arrays, `set --`, and embedded refs),
/// reads a literal option prefix on a dynamic word, and judges an unresolvable
/// git subcommand as each subcommand a git rule reads.
pub const RULES_VERSION: &str = "2026-09-28.2";

/// The built-in kinds.
pub mod kind {
    pub const HISTORY_REWRITE: &str = "history_rewrite";
    pub const DESTROY_REMOTE: &str = "destroy_remote";
    pub const BULK_DELETE: &str = "bulk_delete";
    pub const PUBLISH: &str = "publish";
    pub const EXTERNAL_POST: &str = "external_post";
    pub const MERGE: &str = "merge";
    pub const ACCESS_CHANGE: &str = "access_change";
    pub const SPEND: &str = "spend";
    pub const OPAQUE: &str = "opaque";
}

/// A built-in kind and its default grade. The owner may regrade any of them
/// or add more (`[consequences.kinds.<name>]`).
pub struct SeedKind {
    pub name: &'static str,
    pub irreversible: bool,
    pub description: &'static str,
}

pub const SEED_KINDS: &[SeedKind] = &[
    SeedKind {
        name: kind::HISTORY_REWRITE,
        irreversible: true,
        description: "a force push, or deleting a remote branch or tag",
    },
    SeedKind {
        name: kind::DESTROY_REMOTE,
        irreversible: true,
        description: "deleting a repository, release, or bucket; terminating an instance; dropping a database",
    },
    SeedKind {
        name: kind::BULK_DELETE,
        irreversible: true,
        description: "a recursive delete, or a discard of uncommitted work, that no rebuild restores",
    },
    SeedKind {
        name: kind::PUBLISH,
        irreversible: true,
        description: "a release, a package publish, making something public",
    },
    SeedKind {
        name: kind::EXTERNAL_POST,
        irreversible: true,
        description: "posting anywhere other than Theseus's own places",
    },
    SeedKind {
        name: kind::MERGE,
        irreversible: false,
        description: "merging a pull request",
    },
    SeedKind {
        name: kind::ACCESS_CHANGE,
        irreversible: false,
        description: "changing who or what can access something: keys, secrets, IAM, visibility",
    },
    SeedKind {
        name: kind::SPEND,
        irreversible: false,
        description: "starting something that bills",
    },
    SeedKind {
        name: kind::OPAQUE,
        irreversible: false,
        description: "a call the rules cannot see through: inline code, eval, a script, a task runner",
    },
];

/// One consequence of one call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consequence {
    /// What policy grades (`history_rewrite`, `opaque`, or an owner's kind).
    pub kind: String,
    /// Which rule found it: a `RULES` id, `config:<kind>`, or `toollet:<name>`.
    pub rule: String,
    /// The command it was found in, as the parser read it.
    pub detail: String,
}

/// An owner's detection rule from config: an argv prefix for a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRule {
    pub kind: String,
    pub prefix: Vec<String>,
}

/// One row of the table.
pub struct Rule {
    pub id: &'static str,
    pub kind: &'static str,
    /// Why it matches, in words for the notice.
    pub why: &'static str,
    pub test: fn(&Cmd) -> bool,
    /// Shell text it must match, run in the example fixture (see the tests).
    pub must: &'static [&'static str],
    /// Shell text it must not match.
    pub must_not: &'static [&'static str],
}

/// Everything `argv`, run in `cwd`, would do that the rules can name.
pub fn detect(argv: &[String], cwd: &Path, owner: &[OwnerRule]) -> Vec<Consequence> {
    let mut out: Vec<Consequence> = vec![];
    for c in shell::commands(argv, cwd) {
        // The detail shows the resolved command and where each value came from
        // (spec §3.9, step 2a.3), so the operator sees what the gate judged.
        let base = c.display();
        let prov = if c.resolved.is_empty() {
            String::new()
        } else {
            let p = c
                .resolved
                .iter()
                .map(|(k, v)| format!("`{k}` = `{v}`"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(" ({p})")
        };
        let dyn_sub = git_dynamic_sub(&c);
        let mut push = |kind: &str, rule: &str, detail: String| {
            let x = Consequence {
                kind: kind.into(),
                rule: rule.into(),
                detail,
            };
            if !out.contains(&x) {
                out.push(x);
            }
        };
        for r in RULES {
            if (r.test)(&c) {
                let mut detail = format!("{base}{prov}");
                // An unresolvable git subcommand: say which subcommand the rule read.
                if let (Some(sp), Some(sub)) = (&dyn_sub, rule_git_sub(r.id)) {
                    detail = format!("{detail} (possibly: `{sp}` could be `{sub}`)");
                }
                push(r.kind, r.id, detail);
            }
        }
        for o in owner {
            let words: Vec<&str> = c.words.iter().map(|w| w.text.as_str()).collect();
            if !o.prefix.is_empty()
                && words.len() >= o.prefix.len()
                && words.iter().zip(&o.prefix).all(|(a, b)| a == b)
            {
                push(
                    &o.kind,
                    &format!("config:{}", o.kind),
                    format!("{base}{prov}"),
                );
            }
        }
    }
    out
}

pub fn rule(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.id == id)
}

// ---------------------------------------------------------------- regenerable

/// Build outputs a repository regenerates. A recursive delete of one of these
/// (or anything under one) is not `bulk_delete` when the repository ignores it
/// and has nothing tracked there.
pub const BUILD_OUTPUTS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".cache",
    "coverage",
    ".nyc_output",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".venv",
    "venv",
    ".gradle",
    ".dart_tool",
    "_build",
    ".eggs",
];

/// Would deleting the target named by `w`, run in `cwds`, lose only what a
/// rebuild or the temp convention restores? The rule, precisely:
/// - inside a git work tree, the target (after `..`, `~`, and symlinks) must be
///   strictly inside it, not in `.git`, and at or under a directory whose name
///   is in `BUILD_OUTPUTS`, that the repository's own ignore files ignore
///   (every `.gitignore` from the root down, and `.git/info/exclude`; not the
///   operator's global excludes, so the answer is the repository's), and that
///   has nothing tracked in `HEAD`;
/// - outside any work tree, the target must be strictly under `/tmp`;
/// - a target the parser cannot resolve (`$DIR`, an unknown `cd`, `xargs`
///   input) is never regenerable, and a relative target must be regenerable
///   from every directory the command might run in.
///
/// A repository is judged by its own rules wherever it lives, so a clone under
/// `/tmp` keeps its uncommitted work protected.
pub fn regenerable(w: &Word, cwds: &Cwds) -> bool {
    if w.dynamic {
        return false;
    }
    let text = if w.glob {
        // A `..` at or after the first glob component escapes wherever the glob
        // lands (`target/*/../../src` deletes `src`, not `target`): never
        // regenerable. Only the fixed prefix before the glob is judged otherwise.
        let parts: Vec<&str> = w.text.split('/').collect();
        let first_glob = parts
            .iter()
            .position(|p| p.contains(['*', '?', '[']))
            .unwrap_or(0);
        if parts[first_glob..].contains(&"..") {
            return false;
        }
        glob_prefix(&w.text)
    } else {
        w.text.clone()
    };
    let raw = expand_tilde(&text);
    if raw.is_absolute() {
        return regenerable_path(&raw);
    }
    if cwds.unknown || cwds.dirs.is_empty() {
        return false;
    }
    cwds.dirs.iter().all(|d| regenerable_path(&d.join(&raw)))
}

/// The directory part before the first component with a glob (`target/*` →
/// `target`, `*` → ``).
fn glob_prefix(s: &str) -> String {
    let mut out = vec![];
    for part in s.split('/') {
        if part.contains(['*', '?', '[']) {
            break;
        }
        out.push(part);
    }
    out.join("/")
}

pub fn regenerable_path(p: &Path) -> bool {
    let p = paths::canonical_best_effort(p);
    if p.components().any(|c| c.as_os_str() == ".git") {
        return false;
    }
    match work_tree(&p) {
        Some(root) => {
            let Ok(rel) = p.strip_prefix(&root) else {
                return false;
            };
            let mut acc = PathBuf::new();
            for comp in rel.components() {
                acc.push(comp);
                let name = comp.as_os_str().to_string_lossy();
                if BUILD_OUTPUTS.contains(&name.as_ref())
                    && ignored(&root, &acc)
                    && !tracked(&root, &acc)
                {
                    return true;
                }
            }
            false
        }
        None => {
            let tmp = paths::canonical_best_effort(Path::new("/tmp"));
            p != tmp && p.starts_with(&tmp)
        }
    }
}

/// The nearest ancestor holding a `.git` (a directory, or a worktree's file).
fn work_tree(p: &Path) -> Option<PathBuf> {
    let mut d = Some(p);
    while let Some(x) = d {
        if x.join(".git").exists() {
            return Some(x.to_path_buf());
        }
        d = x.parent();
    }
    None
}

/// Does the repository at `root` ignore `rel` (a directory)? The deepest
/// `.gitignore` with a verdict decides, then `.git/info/exclude`.
fn ignored(root: &Path, rel: &Path) -> bool {
    use ignore::gitignore::GitignoreBuilder;
    use ignore::Match;
    let mut files: Vec<(PathBuf, PathBuf)> = vec![];
    let mut dir = rel.parent();
    while let Some(d) = dir {
        files.push((root.join(d), root.join(d).join(".gitignore")));
        dir = d.parent();
    }
    files.push((root.to_path_buf(), root.join(".git/info/exclude")));
    for (base, file) in files {
        if !file.is_file() {
            continue;
        }
        let mut b = GitignoreBuilder::new(&base);
        if b.add(&file).is_some() {
            continue;
        }
        let Ok(gi) = b.build() else { continue };
        let Ok(r) = root.join(rel).strip_prefix(&base).map(Path::to_path_buf) else {
            continue;
        };
        match gi.matched_path_or_any_parents(&r, true) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

/// Does `HEAD` track anything at `rel`? A repository gix cannot read counts as
/// tracked (so nothing there is regenerable); an unborn `HEAD` tracks nothing.
fn tracked(root: &Path, rel: &Path) -> bool {
    let Ok(repo) = gix::open(root) else {
        return true;
    };
    match repo.head() {
        Ok(h) if h.is_unborn() => return false,
        Ok(_) => {}
        Err(_) => return true,
    }
    let Ok(tree) = repo.head_tree() else {
        return true;
    };
    !matches!(tree.lookup_entry_by_path(rel), Ok(None))
}

// ---------------------------------------------------------------- program helpers

fn is(c: &Cmd, names: &[&str]) -> bool {
    names.contains(&c.prog())
}

/// The command from word `i` on, as its own `Cmd` (a subcommand and its args).
fn from(c: &Cmd, i: usize) -> Cmd {
    Cmd {
        words: c.words[i..].to_vec(),
        cwds: c.cwds.clone(),
        more_operands: c.more_operands,
        unparsed: false,
        via: c.via.clone(),
        redirs: c.redirs.clone(),
        resolved: c.resolved.clone(),
        text: c.text.clone(),
    }
}

/// Skip leading options (and the values of `takes_value`) and return the
/// index of the first positional word after `start`.
fn first_positional(c: &Cmd, start: usize, takes_value: &[&str]) -> Option<usize> {
    let mut i = start;
    while i < c.words.len() {
        let t = c.words[i].text.as_str();
        if t.starts_with('-') && t.len() > 1 {
            i += if takes_value.contains(&t) { 2 } else { 1 };
            continue;
        }
        return Some(i);
    }
    None
}

const GIT_GLOBAL_VALUE: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
    "--super-prefix",
    "--config-env",
    "--attr-source",
];

/// git's subcommand, with its arguments.
fn git(c: &Cmd) -> Option<Cmd> {
    if c.prog() != "git" {
        return None;
    }
    first_positional(c, 1, GIT_GLOBAL_VALUE).map(|i| from(c, i))
}

fn git_sub(c: &Cmd, subs: &[&str]) -> Option<Cmd> {
    let s = git(c)?;
    // An unresolvable subcommand (`git $x -f`) could be any of them, so it is
    // judged as each git rule's subcommand; every rule whose options fire flags
    // it (spec §3.9, step 2a.3). A literal subcommand must be in the list.
    (s.words[0].dynamic || subs.contains(&s.prog())).then_some(s)
}

/// The spelling of git's subcommand when it is unresolvable (`git $x -f` → `$x`),
/// for the "possibly" note; `None` when it is literal or absent.
fn git_dynamic_sub(c: &Cmd) -> Option<String> {
    git(c).and_then(|s| {
        s.words
            .first()
            .filter(|w| w.dynamic)
            .map(|w| w.text.clone())
    })
}

/// The git subcommand a rule reads, for the "possibly" note on an unresolvable
/// subcommand.
fn rule_git_sub(id: &str) -> Option<&'static str> {
    Some(match id {
        "git.push.force" | "git.push.delete" => "push",
        "git.reset.hard" => "reset",
        "git.clean.force" => "clean",
        "git.discard.tree" => "checkout",
        _ => return None,
    })
}

/// Where a git command resolves paths: its `-C` directories, joined.
fn git_cwds(c: &Cmd) -> Cwds {
    let mut cwds = c.cwds.clone();
    let mut i = 1;
    while i < c.words.len() {
        let t = c.words[i].text.as_str();
        if t == "-C" {
            if let Some(d) = c.words.get(i + 1) {
                if d.dynamic {
                    return Cwds::unknown();
                }
                let raw = expand_tilde(&d.text);
                cwds.dirs = cwds.dirs.iter().map(|x| x.join(&raw)).collect();
            }
            i += 2;
            continue;
        }
        if !t.starts_with('-') {
            break;
        }
        i += if GIT_GLOBAL_VALUE.contains(&t) { 2 } else { 1 };
    }
    cwds
}

const GH_VALUE: &[&str] = &["-R", "--repo", "--hostname"];

/// gh's command and subcommand: (`pr`, `merge …`).
fn gh(c: &Cmd) -> Option<(String, Option<Cmd>)> {
    if c.prog() != "gh" {
        return None;
    }
    let i = first_positional(c, 1, GH_VALUE)?;
    let cmd = c.words[i].text.clone();
    let sub = first_positional(c, i + 1, GH_VALUE).map(|j| from(c, j));
    Some((cmd, sub))
}

fn gh_is(c: &Cmd, cmd: &str, subs: &[&str]) -> Option<Cmd> {
    match gh(c) {
        Some((x, Some(s))) if x == cmd && subs.contains(&s.prog()) => Some(s),
        _ => None,
    }
}

const AWS_VALUE: &[&str] = &[
    "--profile",
    "--region",
    "--output",
    "--endpoint-url",
    "--query",
    "--color",
    "--ca-bundle",
    "--cli-read-timeout",
    "--cli-connect-timeout",
    "--cli-binary-format",
];

/// aws's service and operation: (`s3`, `rm …`).
fn aws(c: &Cmd) -> Option<(String, Cmd)> {
    if c.prog() != "aws" {
        return None;
    }
    let i = first_positional(c, 1, AWS_VALUE)?;
    let j = first_positional(c, i + 1, AWS_VALUE)?;
    Some((c.words[i].text.clone(), from(c, j)))
}

fn aws_is(c: &Cmd, service: &str, ops: &[&str]) -> bool {
    aws(c).is_some_and(|(s, op)| s == service && ops.contains(&op.prog()))
}

fn dry_run(c: &Cmd) -> bool {
    c.has_long("--dry-run")
}

// ---------------------------------------------------------------- rules: git

const PUSH_VALUE: &[&str] = &["-o", "--push-option", "--repo", "--receive-pack", "--exec"];

/// A dangerous positional operand: a literal `+refspec`/`:ref`, or a dynamic
/// word (quoted or not) that could expand to one. Skips the remote (`skip`).
fn dangerous_refspec(c: &Cmd, skip: usize, mark: char) -> bool {
    c.positionals(PUSH_VALUE)
        .iter()
        .skip(skip)
        .any(|w| w.dynamic || (w.text.len() > 1 && w.text.starts_with(mark)))
}

fn git_push_force(c: &Cmd) -> bool {
    let Some(p) = git_sub(c, &["push"]) else {
        return false;
    };
    // Exemptions are exact: a dry run never counts by abbreviation or a variable.
    if dry_run(&p) || p.opt_exact(&[], &['n']) {
        return false;
    }
    p.opt_detect(&["--force", "--force-with-lease", "--mirror"], &['f'])
        || dangerous_refspec(&p, 1, '+')
}

fn git_push_delete(c: &Cmd) -> bool {
    let Some(p) = git_sub(c, &["push"]) else {
        return false;
    };
    if dry_run(&p) || p.opt_exact(&[], &['n']) {
        return false;
    }
    p.opt_detect(&["--delete", "--prune"], &['d']) || dangerous_refspec(&p, 1, ':')
}

// `reset` and `clean` are dangerous through an option (`--hard`, `-f`), so any
// dynamic word before `--` counts (`opt_detect`): `m=--hard; git reset "$m"`
// waits, and so does the common `git reset "$SHA"` — accepted over-approximation
// (spec §3.9). Exemptions stay exact.
fn git_reset_hard(c: &Cmd) -> bool {
    git_sub(c, &["reset"]).is_some_and(|s| s.opt_detect(&["--hard"], &[]))
}

fn git_clean_force(c: &Cmd) -> bool {
    git_sub(c, &["clean"]).is_some_and(|s| {
        s.opt_detect(&["--force"], &['f']) && !(s.opt_exact(&[], &['n']) || s.has_long("--dry-run"))
    })
}

/// A pathspec that names a tree, not a file: `.`, `:/`, a glob, a directory.
fn tree_pathspec(w: &Word, cwds: &Cwds) -> bool {
    let t = w.text.as_str();
    if w.dynamic
        || matches!(t, "." | "./" | ":/" | ":/." | "*" | ":(top)")
        || t.contains(['*', '?', '['])
    {
        return true;
    }
    if cwds.unknown {
        return true;
    }
    let raw = expand_tilde(t);
    cwds.dirs.iter().any(|d| d.join(&raw).is_dir())
}

// A checkout/restore is destructive through `-f` (which needs no operand:
// `git checkout -f` discards the whole tree) or a tree pathspec. A dynamic word
// counts as a possible `-f` (`opt_detect`), so `git checkout "$branch"` now
// waits — accepted over-approximation, since `-f` cannot be excused by an
// operand the way `rm`'s `-r` can (open question for Eddie).
fn git_discard_tree(c: &Cmd) -> bool {
    let cwds = git_cwds(c);
    if let Some(s) = git_sub(c, &["checkout"]) {
        if s.opt_detect(&["--force"], &['f']) {
            return true;
        }
        let words = s.args();
        if let Some(k) = words.iter().position(|w| w.text == "--") {
            return words[k + 1..].iter().any(|w| tree_pathspec(w, &cwds));
        }
        return words
            .iter()
            .any(|w| matches!(w.text.as_str(), "." | "./" | ":/") || w.text.contains('*'));
    }
    if let Some(s) = git_sub(c, &["restore"]) {
        let staged_only = s.has_any(&["--staged"], &['S']) && !s.has_any(&["--worktree"], &['W']);
        if staged_only {
            return false;
        }
        return s
            .positionals(&["-s", "--source", "--pathspec-from-file"])
            .iter()
            .any(|w| tree_pathspec(w, &cwds));
    }
    false
}

/// Every subcommand git ships (and git-lfs); anything else is an alias or an
/// external `git-*` program, which the rules cannot read.
const GIT_SUBCOMMANDS: &[&str] = &[
    "add",
    "am",
    "annotate",
    "apply",
    "archive",
    "bisect",
    "blame",
    "branch",
    "bugreport",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "citool",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-graph",
    "commit-tree",
    "config",
    "count-objects",
    "credential",
    "describe",
    "diagnose",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "difftool",
    "fast-export",
    "fast-import",
    "fetch",
    "fetch-pack",
    "filter-branch",
    "fmt-merge-msg",
    "for-each-ref",
    "for-each-repo",
    "format-patch",
    "fsck",
    "gc",
    "get-tar-commit-id",
    "grep",
    "gui",
    "hash-object",
    "help",
    "hook",
    "index-pack",
    "init",
    "instaweb",
    "interpret-trailers",
    "lfs",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "mailinfo",
    "mailsplit",
    "maintenance",
    "merge",
    "merge-base",
    "merge-file",
    "merge-index",
    "merge-tree",
    "mergetool",
    "mktag",
    "mktree",
    "multi-pack-index",
    "mv",
    "name-rev",
    "notes",
    "pack-objects",
    "pack-redundant",
    "pack-refs",
    "patch-id",
    "prune",
    "prune-packed",
    "pull",
    "push",
    "range-diff",
    "read-tree",
    "rebase",
    "reflog",
    "remote",
    "repack",
    "replace",
    "replay",
    "request-pull",
    "rerere",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "send-email",
    "send-pack",
    "shortlog",
    "show",
    "show-branch",
    "show-index",
    "show-ref",
    "sparse-checkout",
    "stage",
    "stash",
    "status",
    "stripspace",
    "submodule",
    "switch",
    "symbolic-ref",
    "tag",
    "unpack-file",
    "unpack-objects",
    "update-index",
    "update-ref",
    "update-server-info",
    "var",
    "verify-commit",
    "verify-pack",
    "verify-tag",
    "version",
    "whatchanged",
    "worktree",
    "write-tree",
];

/// Config keys that make git run a program.
fn runs_program(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("alias.")
        || k.starts_with("filter.")
        || k.starts_with("credential")
        || k.ends_with("command")
        || k.ends_with(".cmd")
        || k.ends_with("pager")
        || k.ends_with("editor")
        || k.ends_with("program")
        || k.ends_with("askpass")
        || k.ends_with("textconv")
        || k.ends_with("external")
        || k.ends_with("hookspath")
        || k.ends_with("fsmonitor")
        || k.ends_with("gitproxy")
}

fn git_opaque(c: &Cmd) -> bool {
    if c.prog() != "git" {
        return false;
    }
    // `-c key=value` before the subcommand.
    let mut i = 1;
    while i + 1 < c.words.len() && c.words[i].text.starts_with('-') {
        let t = c.words[i].text.as_str();
        if t == "-c" || t == "--config-env" {
            let kv = &c.words[i + 1];
            if kv.dynamic || runs_program(kv.text.split('=').next().unwrap_or("")) {
                return true;
            }
        }
        i += if GIT_GLOBAL_VALUE.contains(&t) { 2 } else { 1 };
    }
    let Some(s) = git(c) else { return false };
    if s.words[0].dynamic || !GIT_SUBCOMMANDS.contains(&s.prog()) {
        return true;
    }
    match s.prog() {
        "rebase" => s.has_any(&["--exec"], &['x']),
        "bisect" => s.args().first().is_some_and(|w| w.text == "run"),
        "submodule" => s.args().iter().any(|w| w.text == "foreach"),
        "filter-branch" => true,
        _ => false,
    }
}

// ---------------------------------------------------------------- rules: deletes

fn rm_recursive(c: &Cmd) -> bool {
    if c.prog() != "rm" {
        return false;
    }
    // `-r` may be certain (a literal `-r`/`-R`/`--recursive`, or `--recursiv`),
    // or only possible (hidden in a variable, `F=-rf; rm $F dir`). Quoting stops
    // word splitting, not option parsing: `rm "$x" dir` could be `rm -rf dir`.
    let literal_r = c.opt_literal(&["--recursive"], &['r', 'R']);
    if !literal_r && !c.has_dynamic_option() {
        return false;
    }
    if c.more_operands {
        return true;
    }
    let targets = c.positionals(&[]);
    let non_regen = targets.iter().any(|w| !regenerable(w, &c.cwds));
    if literal_r {
        // The flag is certainly present; any non-regenerable target loses work.
        return non_regen;
    }
    // Only a dynamic word could be `-r`. It might be the flag itself, using up
    // one word, so a recursive delete needs a *separate* target: at least two
    // operand words, one of them a non-regenerable target. Alone, `rm "$f"` is
    // either an option deleting nothing or one file with no `-r` — quiet.
    targets.len() >= 2 && non_regen
}

/// find primaries whose next word is a value (a name, path, number, or time),
/// never `-delete` or a command. The value is skipped, so `-name "$p"` does not
/// look like a hidden `-delete`.
const FIND_VALUE_PRIMARIES: &[&str] = &[
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-wholename",
    "-iwholename",
    "-lname",
    "-ilname",
    "-regex",
    "-iregex",
    "-newer",
    "-anewer",
    "-cnewer",
    "-newermt",
    "-newerat",
    "-newerct",
    "-type",
    "-xtype",
    "-size",
    "-perm",
    "-user",
    "-group",
    "-uid",
    "-gid",
    "-inum",
    "-links",
    "-mtime",
    "-atime",
    "-ctime",
    "-mmin",
    "-amin",
    "-cmin",
    "-used",
    "-fstype",
    "-samefile",
    "-maxdepth",
    "-mindepth",
    "-regextype",
];

fn find_delete(c: &Cmd) -> bool {
    if c.prog() != "find" {
        return false;
    }
    let args = c.args();
    // Roots: leading path words. A dynamic word is a root only in first position
    // (the search path); a later dynamic word is a possible expression primary
    // (`find . "$x"` could be `find . -delete`).
    let mut roots_end = 0;
    while roots_end < args.len() {
        let t = args[roots_end].text.as_str();
        if t.starts_with('-') || t == "(" || t == "!" || t == "," {
            break;
        }
        if args[roots_end].dynamic && roots_end > 0 {
            break;
        }
        roots_end += 1;
    }
    // Walk the expression for a delete: a literal `-delete`; `-exec`/`-ok` whose
    // command is `rm` or a dynamic word; or any dynamic word in a primary
    // position (it could be `-delete` or `-exec rm`). A value-primary's value is
    // data and is skipped.
    let mut deletes = false;
    let mut j = roots_end;
    while j < args.len() {
        let t = args[j].text.as_str();
        if matches!(t, "-exec" | "-execdir" | "-ok" | "-okdir") {
            if let Some(cmd0) = args.get(j + 1) {
                if cmd0.dynamic || Path::new(&cmd0.text).file_name().is_some_and(|n| n == "rm") {
                    deletes = true;
                }
            }
            j += 1;
            while j < args.len() && args[j].text != ";" && args[j].text != "+" {
                j += 1;
            }
            j += 1;
            continue;
        }
        if t == "-delete" {
            deletes = true;
        } else if FIND_VALUE_PRIMARIES.contains(&t) {
            j += 1; // its value is data, not a primary
        } else if args[j].dynamic {
            deletes = true;
        }
        j += 1;
    }
    if !deletes {
        return false;
    }
    let dot = Word::lit(".");
    let mut roots: Vec<&Word> = args[..roots_end].iter().collect();
    if roots.is_empty() {
        roots.push(&dot);
    }
    roots.iter().any(|w| !regenerable(w, &c.cwds))
}

fn rsync_dest(c: &Cmd) -> Option<Word> {
    if c.prog() != "rsync" || !c.args().iter().any(|w| w.text.starts_with("--delete")) {
        return None;
    }
    c.positionals(&[
        "-e",
        "--rsh",
        "--exclude",
        "--include",
        "--filter",
        "-f",
        "--files-from",
        "--rsync-path",
        "--password-file",
        "--port",
        "--bwlimit",
        "--chmod",
        "--chown",
        "--log-file",
        "--backup-dir",
        "--suffix",
        "--temp-dir",
        "-T",
        "--compare-dest",
        "--copy-dest",
        "--link-dest",
    ])
    .last()
    .map(|w| (*w).clone())
}

fn remote_path(t: &str) -> bool {
    t.starts_with("rsync://")
        || t.split_once(':')
            .is_some_and(|(h, _)| !h.is_empty() && !h.contains('/'))
}

fn rsync_delete_local(c: &Cmd) -> bool {
    rsync_dest(c).is_some_and(|d| !remote_path(&d.text) && !regenerable(&d, &c.cwds))
}

fn rsync_delete_remote(c: &Cmd) -> bool {
    rsync_dest(c).is_some_and(|d| remote_path(&d.text))
}

// ---------------------------------------------------------------- rules: gh

fn gh_pr_merge(c: &Cmd) -> bool {
    gh_is(c, "pr", &["merge"]).is_some()
}

fn gh_pr_close_delete(c: &Cmd) -> bool {
    gh_is(c, "pr", &["close"]).is_some_and(|s| s.has_any(&["--delete-branch"], &['d']))
}

fn gh_repo_sync_force(c: &Cmd) -> bool {
    gh_is(c, "repo", &["sync"]).is_some_and(|s| s.has_long("--force"))
}

fn visibility(s: &Cmd) -> Option<String> {
    s.value_of(&["--visibility"])
        .map(|v| v.to_ascii_lowercase())
}

fn gh_publish(c: &Cmd) -> bool {
    gh_is(c, "release", &["create", "upload"]).is_some()
        || gh_is(c, "repo", &["edit"]).is_some_and(|s| visibility(&s).as_deref() == Some("public"))
        || gh_is(c, "repo", &["create"])
            .is_some_and(|s| s.has_long("--public") || visibility(&s).as_deref() == Some("public"))
        || gh_is(c, "gist", &["create"]).is_some_and(|s| s.has_any(&["--public"], &['p']))
}

fn gh_post(c: &Cmd) -> bool {
    gh_is(c, "issue", &["create", "comment", "edit"]).is_some()
        || gh_is(c, "pr", &["create", "comment", "review", "edit"]).is_some()
        || gh_is(c, "gist", &["create"]).is_some_and(|s| !s.has_any(&["--public"], &['p']))
}

fn gh_destroy(c: &Cmd) -> bool {
    gh_is(c, "repo", &["delete"]).is_some()
        || gh_is(c, "release", &["delete", "delete-asset"]).is_some()
        || gh_is(c, "issue", &["delete"]).is_some()
        || gh_is(c, "gist", &["delete"]).is_some()
}

fn gh_access(c: &Cmd) -> bool {
    gh_is(c, "ssh-key", &["add", "delete"]).is_some()
        || gh_is(c, "gpg-key", &["add", "delete"]).is_some()
        || gh_is(c, "secret", &["set", "delete", "remove"]).is_some()
        || gh_is(c, "repo", &["deploy-key"]).is_some_and(|s| {
            s.args()
                .first()
                .is_some_and(|w| matches!(w.text.as_str(), "add" | "delete"))
        })
        || gh_is(c, "repo", &["edit"]).is_some_and(|s| {
            matches!(
                visibility(&s).as_deref(),
                Some("private") | Some("internal")
            )
        })
}

const GH_COMMANDS: &[&str] = &[
    "agent-task",
    "alias",
    "api",
    "attestation",
    "auth",
    "browse",
    "cache",
    "co",
    "codespace",
    "completion",
    "config",
    "copilot",
    "extension",
    "gist",
    "gpg-key",
    "help",
    "issue",
    "label",
    "org",
    "pr",
    "preview",
    "project",
    "release",
    "repo",
    "ruleset",
    "run",
    "search",
    "secret",
    "ssh-key",
    "status",
    "variable",
    "version",
    "workflow",
];

fn gh_opaque(c: &Cmd) -> bool {
    let Some((cmd, sub)) = gh(c) else {
        return false;
    };
    if !GH_COMMANDS.contains(&cmd.as_str()) {
        return true;
    }
    if cmd == "api" {
        let method = c
            .value_of(&["-X", "--method"])
            .map(|m| m.to_ascii_uppercase());
        let fields = c.has_any(&["--field", "--raw-field", "--input"], &['f', 'F']);
        return match method.as_deref() {
            Some("GET") => false,
            Some(_) => true,
            None => fields,
        };
    }
    (cmd == "workflow" && sub.as_ref().is_some_and(|s| s.prog() == "run"))
        || (cmd == "run" && sub.as_ref().is_some_and(|s| s.prog() == "rerun"))
}

// ---------------------------------------------------------------- rules: packages, clouds, http

fn pkg_publish(c: &Cmd) -> bool {
    if dry_run(c) {
        return false;
    }
    let sub = |names: &[&str]| {
        first_positional(c, 1, &[]).is_some_and(|i| names.contains(&c.words[i].text.as_str()))
    };
    match c.prog() {
        "npm" | "pnpm" | "bun" | "cargo" | "poetry" | "uv" | "flit" | "hatch" => sub(&["publish"]),
        "yarn" => {
            sub(&["publish"]) || (sub(&["npm"]) && c.args().iter().any(|w| w.text == "publish"))
        }
        "twine" => sub(&["upload"]),
        "gem" => sub(&["push"]),
        "docker" | "podman" => {
            sub(&["push"])
                || c.args()
                    .windows(2)
                    .any(|w| w[0].text == "image" && w[1].text == "push")
                || (c.args().iter().any(|w| w.text == "build")
                    && c.args().iter().any(|w| w.text == "--push"))
        }
        _ => false,
    }
}

fn npm_unpublish(c: &Cmd) -> bool {
    is(c, &["npm", "pnpm"])
        && first_positional(c, 1, &[]).is_some_and(|i| c.words[i].text == "unpublish")
}

fn aws_iam_write(c: &Cmd) -> bool {
    const VERBS: &[&str] = &[
        "create-",
        "delete-",
        "put-",
        "attach-",
        "detach-",
        "update-",
        "add-",
        "remove-",
        "tag-",
        "untag-",
        "set-",
        "upload-",
        "change-",
        "enable-",
        "deactivate-",
        "reset-",
        "resync-",
    ];
    aws(c).is_some_and(|(s, op)| s == "iam" && VERBS.iter().any(|v| op.prog().starts_with(v)))
}

fn aws_access_write(c: &Cmd) -> bool {
    aws_is(
        c,
        "s3api",
        &[
            "put-bucket-policy",
            "put-bucket-acl",
            "put-object-acl",
            "delete-bucket-policy",
            "put-public-access-block",
            "delete-public-access-block",
        ],
    ) || aws_is(
        c,
        "ec2",
        &[
            "authorize-security-group-ingress",
            "authorize-security-group-egress",
            "revoke-security-group-ingress",
            "revoke-security-group-egress",
        ],
    ) || aws_is(c, "kms", &["put-key-policy", "create-grant"])
}

fn aws_spend(c: &Cmd) -> bool {
    aws_is(
        c,
        "ec2",
        &[
            "run-instances",
            "request-spot-instances",
            "purchase-reserved-instances-offering",
            "allocate-hosts",
        ],
    ) || aws_is(c, "rds", &["create-db-instance", "create-db-cluster"])
        || aws_is(c, "eks", &["create-cluster"])
}

fn aws_destroy(c: &Cmd) -> bool {
    let Some((s, op)) = aws(c) else { return false };
    match (s.as_str(), op.prog()) {
        ("s3", "rb") => true,
        ("s3", "rm") => op.has_long("--recursive"),
        ("s3", "sync") => {
            op.has_long("--delete")
                && op
                    .positionals(&[
                        "--exclude",
                        "--include",
                        "--acl",
                        "--storage-class",
                        "--sse",
                        "--sse-kms-key-id",
                        "--grants",
                    ])
                    .last()
                    .is_some_and(|w| w.text.starts_with("s3://"))
        }
        ("s3api", "delete-bucket") => true,
        ("ec2", "terminate-instances") => true,
        ("rds", "delete-db-instance" | "delete-db-cluster") => true,
        ("dynamodb", "delete-table") => true,
        ("cloudformation", "delete-stack") => true,
        ("lambda", "delete-function") => true,
        ("ecr", "delete-repository") => true,
        ("eks", "delete-cluster") => true,
        ("kms", "schedule-key-deletion") => true,
        ("secretsmanager", "delete-secret") => true,
        ("route53", "delete-hosted-zone") => true,
        _ => false,
    }
}

fn infra_destroy(c: &Cmd) -> bool {
    let sub = |names: &[&str]| {
        first_positional(c, 1, &[]).is_some_and(|i| names.contains(&c.words[i].text.as_str()))
    };
    match c.prog() {
        "terraform" | "tofu" => {
            sub(&["destroy"]) || (sub(&["apply"]) && c.args().iter().any(|w| w.text == "-destroy"))
        }
        "kubectl" => sub(&["delete"]),
        "helm" => sub(&["uninstall", "delete"]),
        _ => false,
    }
}

fn http_post(c: &Cmd) -> bool {
    const MUTATING: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];
    match c.prog() {
        "curl" => {
            c.has_any(
                &[
                    "--data",
                    "--data-raw",
                    "--data-binary",
                    "--data-urlencode",
                    "--data-ascii",
                    "--json",
                    "--form",
                    "--form-string",
                    "--upload-file",
                ],
                &['d', 'F', 'T'],
            ) || c
                .value_of(&["-X", "--request"])
                .is_some_and(|m| MUTATING.contains(&m.to_ascii_uppercase().as_str()))
        }
        "wget" => {
            c.has_any(
                &["--post-data", "--post-file", "--body-data", "--body-file"],
                &[],
            ) || c
                .value_of(&["--method"])
                .is_some_and(|m| MUTATING.contains(&m.to_ascii_uppercase().as_str()))
        }
        _ => false,
    }
}

fn mail_send(c: &Cmd) -> bool {
    is(c, &["sendmail", "mail", "mailx", "mutt", "msmtp", "swaks"])
        || git_sub(c, &["send-email"]).is_some()
}

// ---------------------------------------------------------------- rules: opaque

const INTERPRETERS: &[&str] = &[
    "python",
    "python2",
    "python3",
    "pypy",
    "pypy3",
    "node",
    "nodejs",
    "deno",
    "bun",
    "perl",
    "ruby",
    "php",
    "lua",
    "luajit",
    "Rscript",
    "osascript",
    "pwsh",
    "powershell",
    "tclsh",
    "expect",
];

fn interpreter(p: &str) -> bool {
    INTERPRETERS.contains(&p)
        || p.strip_prefix("python3.")
            .is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()))
}

fn info_only(c: &Cmd) -> bool {
    !c.args().is_empty()
        && c.args().iter().all(|w| {
            matches!(
                w.text.as_str(),
                "--version" | "-V" | "-v" | "--help" | "-h" | "-version"
            )
        })
}

fn inline_code(c: &Cmd) -> bool {
    let p = c.prog();
    if !interpreter(p) {
        return false;
    }
    match p {
        "node" | "nodejs" | "bun" => c.has_any(&["--eval", "--print"], &['e', 'p']),
        "deno" => c.args().first().is_some_and(|w| w.text == "eval"),
        "perl" => c.has_short('e') || c.has_short('E'),
        "ruby" | "lua" | "luajit" | "Rscript" | "osascript" => c.has_short('e'),
        "php" => c.has_short('r'),
        "pwsh" | "powershell" => c.args().iter().any(|w| {
            let t = w.text.to_ascii_lowercase();
            t == "-c" || t == "-command" || t == "-encodedcommand" || t == "-e" || t == "-ec"
        }),
        "expect" => c.has_short('c'),
        // Python: `-c` stops option parsing; `-W`/`-X` take a value.
        _ => c
            .words
            .iter()
            .skip(1)
            .take_while(|w| w.text.starts_with('-'))
            .any(|w| is_cluster_with(&w.text, 'c')),
    }
}

fn is_cluster_with(t: &str, ch: char) -> bool {
    t.len() > 1
        && t.starts_with('-')
        && !t.starts_with("--")
        && !t.starts_with("-W")
        && !t.starts_with("-X")
        && t[1..].contains(ch)
}

const SCRIPT_EXT: &[&str] = &[
    ".sh", ".bash", ".zsh", ".py", ".js", ".mjs", ".cjs", ".ts", ".rb", ".pl", ".php",
];

fn script(c: &Cmd) -> bool {
    let p = c.prog();
    if c.unparsed || c.dynamic_program() {
        return false;
    }
    // A program by relative path, or named like a script.
    if p.contains('/') || SCRIPT_EXT.iter().any(|e| p.ends_with(e)) {
        return true;
    }
    const SHELLS: &[&str] = &["bash", "sh", "dash", "zsh", "ksh", "mksh", "ash", "yash"];
    if SHELLS.contains(&p) {
        // `-c` strings were already parsed into their commands; what is left
        // here is a script file, stdin, or a `-c` whose text is not known.
        return !info_only(c);
    }
    if interpreter(p) {
        if inline_code(c) || info_only(c) {
            return false;
        }
        return match p {
            "deno" => c.args().first().is_some_and(|w| w.text == "run"),
            "bun" => c
                .args()
                .first()
                .is_some_and(|w| w.text != "test" && w.text != "install" && w.text != "add"),
            _ => true,
        };
    }
    false
}

fn runner(c: &Cmd) -> bool {
    let first = first_positional(
        c,
        1,
        &[
            "-C",
            "-f",
            "--file",
            "-j",
            "--prefix",
            "-w",
            "--workspace",
            "--filter",
            "--dir",
            "--cwd",
            "--manifest-path",
            "-p",
            "--package",
        ],
    )
    .map(|i| c.words[i].text.as_str());
    let has = |names: &[&str]| first.is_some_and(|f| names.contains(&f));
    match c.prog() {
        "make" | "gmake" | "bmake" | "just" | "rake" | "task" | "mage" | "ninja" | "nox"
        | "tox" | "invoke" | "gradle" | "mvn" => !info_only(c),
        "npx" | "bunx" | "uvx" | "pnpx" => true,
        "npm" => has(&[
            "run",
            "run-script",
            "rum",
            "urn",
            "test",
            "t",
            "tst",
            "start",
            "stop",
            "restart",
            "exec",
            "x",
            "create",
            "init",
        ]),
        "pnpm" | "yarn" => {
            const BUILTIN: &[&str] = &[
                "install",
                "i",
                "add",
                "remove",
                "rm",
                "uninstall",
                "up",
                "update",
                "upgrade",
                "list",
                "ls",
                "why",
                "outdated",
                "audit",
                "info",
                "view",
                "pack",
                "publish",
                "link",
                "unlink",
                "config",
                "cache",
                "store",
                "prune",
                "dedupe",
                "import",
                "rebuild",
                "licenses",
                "version",
                "help",
                "login",
                "logout",
                "whoami",
                "owner",
                "tag",
                "workspaces",
                "bin",
                "root",
                "npm",
                "unpublish",
            ];
            first.is_some_and(|f| !BUILTIN.contains(&f)) && !info_only(c)
        }
        "bun" => has(&["run", "x", "create"]),
        "cargo" => has(&["run", "xtask", "make"]),
        "go" => has(&["run", "generate"]),
        "deno" => has(&["task"]),
        "uv" | "poetry" | "pipenv" | "hatch" | "pdm" | "conda" | "pipx" | "dotnet" | "swift" => {
            has(&["run"])
        }
        "terraform" | "tofu" => has(&["apply"]) && !c.args().iter().any(|w| w.text == "-destroy"),
        _ => false,
    }
}

fn eval_like(c: &Cmd) -> bool {
    is(c, &["eval", "source", "."])
}

/// A command whose real effect the rule table cannot read, so a floor mention in
/// its raw text is all a gate has to go on: unparsed text, a program word built
/// from an expansion, inline interpreter code (`python3 -c …`), `eval`/`source`,
/// or a command run on another machine (`ssh host …`). The floor scans these for
/// a floor path, `theseusd`, or `op <subcommand>` (spec §3.9, step 2a+).
pub fn unreadable(c: &Cmd) -> bool {
    c.unparsed || c.dynamic_program() || inline_code(c) || eval_like(c) || remote_command(c)
}

fn shell_opaque(c: &Cmd) -> bool {
    c.unparsed || c.dynamic_program()
}

fn remote_command(c: &Cmd) -> bool {
    c.prog() == "ssh"
}

// ---------------------------------------------------------------- the table

use kind::*;

pub static RULES: &[Rule] = &[
    Rule {
        id: "git.push.force",
        kind: HISTORY_REWRITE,
        why: "a force push replaces the remote's history",
        test: git_push_force,
        must: &[
            "git push --force",
            "git push -f origin main",
            "git push origin main --force-with-lease",
            "git push --force-with-lease=main:abc123 origin main",
            "git push origin +main",
            "git push origin +HEAD:refs/heads/main",
            "git -C somedir push -fu origin feature",
            "git push --mirror backup",
            "/usr/bin/git push -f",
            // Unique-prefix long options, which git accepts.
            "git push --mirro backup",
            "git push origin main --force-with-leas",
        ],
        must_not: &[
            "git push",
            "git push origin main",
            "git push -u origin feature",
            "git push --dry-run --force",
            "git push -n -f",
            "git push -o +notarefspec origin main",
            "git fetch origin +main:main",
            "git pull --force",
            "echo git push -f",
            // A real long option that is not an abbreviation of a force flag.
            "git push --follow-tags",
        ],
    },
    Rule {
        id: "git.push.delete",
        kind: HISTORY_REWRITE,
        why: "deleting a remote branch or tag",
        test: git_push_delete,
        must: &[
            "git push origin --delete feature",
            "git push -d origin v1.0",
            "git push origin :feature",
            "git push origin :refs/tags/v1.0",
            "git push --prune origin refs/heads/*:refs/heads/*",
        ],
        must_not: &[
            "git push origin main:main",
            "git push origin :",
            "git push origin HEAD",
            "git push --dry-run -d origin feature",
            "git branch -d feature",
        ],
    },
    Rule {
        id: "gh.pr.close.delete_branch",
        kind: HISTORY_REWRITE,
        why: "closing a pull request deletes its unmerged branch",
        test: gh_pr_close_delete,
        must: &["gh pr close 12 --delete-branch", "gh pr close -d 12"],
        must_not: &["gh pr close 12", "gh pr merge 12 --delete-branch"],
    },
    Rule {
        id: "gh.repo.sync.force",
        kind: HISTORY_REWRITE,
        why: "a forced sync replaces the fork branch's history",
        test: gh_repo_sync_force,
        must: &["gh repo sync owner/fork --force"],
        must_not: &["gh repo sync", "gh repo sync owner/fork -b main"],
    },
    Rule {
        id: "git.reset.hard",
        kind: BULK_DELETE,
        why: "`reset --hard` discards uncommitted work",
        test: git_reset_hard,
        must: &[
            "git reset --hard",
            "git reset --hard HEAD~1",
            "git -C somedir reset --hard origin/main",
            "git reset --har HEAD~1",
        ],
        must_not: &[
            "git reset",
            "git reset --soft HEAD~1",
            "git reset HEAD README.md",
            "git reset --keep HEAD~1",
        ],
    },
    Rule {
        id: "git.clean.force",
        kind: BULK_DELETE,
        why: "`git clean` deletes untracked files",
        test: git_clean_force,
        must: &[
            "git clean -fdx",
            "git clean -f",
            "git clean -xdf",
            "git clean --force -d",
        ],
        must_not: &[
            "git clean -n",
            "git clean -ndx",
            "git clean -f --dry-run",
            "git status",
        ],
    },
    Rule {
        id: "git.discard.tree",
        kind: BULK_DELETE,
        why: "restoring a tree from the index or a commit discards its uncommitted changes",
        test: git_discard_tree,
        must: &[
            "git checkout -- .",
            "git checkout .",
            "git checkout HEAD -- src",
            "git restore .",
            "git restore --worktree --staged .",
            "git restore -s HEAD~2 src",
            "git checkout -f main",
            "git -C somedir restore .",
        ],
        must_not: &[
            "git checkout main",
            "git checkout -b feature",
            "git restore --staged .",
            "git checkout -- README.md",
            "git restore src/lib.rs",
            "git switch main",
        ],
    },
    Rule {
        id: "rm.recursive",
        kind: BULK_DELETE,
        why: "a recursive delete of work no rebuild restores",
        test: rm_recursive,
        must: &[
            "rm -rf somedir",
            "rm -r src",
            "rm -Rf ./somedir",
            "rm --recursive --force somedir",
            "rm -rf .",
            "rm -rf ~",
            "rm -rf /",
            "rm -rf \"$DIR\"",
            "rm -rf target somedir",
            "rm -rf target/../somedir",
            "rm -rf tracked/dist",
            "rm -rf build",
            "rm -rf *",
            "rm -rf .git",
            "cd target; rm -rf somedir",
            "ls | xargs rm -rf",
            // A `..` at or after the first glob component escapes the glob.
            "rm -rf target/*/../../src",
            // A unique-prefix long option.
            "rm --recursiv -f somedir",
        ],
        must_not: &[
            "rm -rf target",
            "rm -rf target/debug",
            "rm -rf ./target/",
            "rm -rf node_modules",
            "rm -rf web/node_modules",
            "rm -rf dist",
            "rm -rf target/*",
            "cd target && rm -rf debug",
            "cd target && rm -rf somedir",
            "rm -rf /tmp/theseus-rule-scratch/build",
            "rm README.md",
            "rm -f notes.md",
            "rm -rf",
        ],
    },
    Rule {
        id: "find.delete",
        kind: BULK_DELETE,
        why: "`find` deletes everything its expression matches",
        test: find_delete,
        must: &[
            "find . -name '*.orig' -delete",
            "find somedir -type f -delete",
            "find . -exec rm {} \\;",
            "find src -name x -execdir /bin/rm -f {} +",
        ],
        must_not: &[
            "find . -name '*.rs'",
            "find target -name '*.d' -delete",
            "find /tmp/theseus-rule-scratch -mtime +7 -delete",
            "find . -exec grep -l x {} +",
        ],
    },
    Rule {
        id: "rsync.delete.local",
        kind: BULK_DELETE,
        why: "`rsync --delete` removes destination files the source lacks",
        test: rsync_delete_local,
        must: &["rsync -a --delete src/ somedir/", "rsync -a --delete-after x/ ./somedir"],
        must_not: &[
            "rsync -a src/ somedir/",
            "rsync -a --delete src/ target/site/",
            "rsync -a --delete src/ host:/srv/site",
        ],
    },
    Rule {
        id: "rsync.delete.remote",
        kind: DESTROY_REMOTE,
        why: "`rsync --delete` removes files on the remote the source lacks",
        test: rsync_delete_remote,
        must: &[
            "rsync -az --delete ./site/ web@host:/srv/site/",
            "rsync --delete-after -a x rsync://host/mod/",
        ],
        must_not: &["rsync -a site/ host:/srv/", "rsync -a --delete src/ somedir/"],
    },
    Rule {
        id: "gh.pr.merge",
        kind: MERGE,
        why: "merging a pull request",
        test: gh_pr_merge,
        must: &["gh pr merge 12 --squash", "gh pr merge --auto --delete-branch"],
        must_not: &["gh pr view 12", "gh pr list --state merged", "git merge feature"],
    },
    Rule {
        id: "gh.access",
        kind: ACCESS_CHANGE,
        why: "changing keys, secrets, or a repository's visibility",
        test: gh_access,
        must: &[
            "gh ssh-key add key.pub",
            "gh gpg-key delete ABC123",
            "gh secret set TOKEN --body x",
            "gh repo deploy-key add k.pub",
            "gh repo edit --visibility private",
        ],
        must_not: &[
            "gh ssh-key list",
            "gh secret list",
            "gh repo deploy-key list",
            "gh repo edit --description x",
        ],
    },
    Rule {
        id: "aws.iam.write",
        kind: ACCESS_CHANGE,
        why: "changing IAM users, roles, keys, or policies",
        test: aws_iam_write,
        must: &[
            "aws iam create-user --user-name x",
            "aws iam attach-role-policy --role-name r --policy-arn a",
            "aws --profile p iam put-user-policy --user-name u",
            "aws iam delete-access-key --access-key-id k",
        ],
        must_not: &[
            "aws iam list-users",
            "aws iam get-role --role-name r",
            "aws s3 ls",
        ],
    },
    Rule {
        id: "aws.access.write",
        kind: ACCESS_CHANGE,
        why: "changing a bucket policy, a security group, or a key policy",
        test: aws_access_write,
        must: &[
            "aws s3api put-bucket-policy --bucket b --policy file://p.json",
            "aws ec2 authorize-security-group-ingress --group-id g",
            "aws kms put-key-policy --key-id k",
        ],
        must_not: &[
            "aws s3api get-bucket-policy --bucket b",
            "aws ec2 describe-security-groups",
        ],
    },
    Rule {
        id: "aws.spend",
        kind: SPEND,
        why: "starting something that bills",
        test: aws_spend,
        must: &[
            "aws ec2 run-instances --image-id ami-1 --count 1",
            "aws --region us-west-2 rds create-db-instance --db-instance-identifier x",
        ],
        must_not: &[
            "aws ec2 describe-instances",
            "aws ec2 stop-instances --instance-ids i-1",
        ],
    },
    Rule {
        id: "gh.publish",
        kind: PUBLISH,
        why: "a release, or making a repository or gist public",
        test: gh_publish,
        must: &[
            "gh release create v1.0 --notes x",
            "gh release upload v1.0 a.tgz",
            "gh repo edit --visibility public",
            "gh repo edit --visibility=public --accept-visibility-change-consequences",
            "gh repo create x --public",
            "gh gist create -p notes.md",
        ],
        must_not: &[
            "gh release list",
            "gh release view v1",
            "gh repo create x --private",
            "gh repo edit --visibility private",
            "gh gist create notes.md",
        ],
    },
    Rule {
        id: "pkg.publish",
        kind: PUBLISH,
        why: "publishing a package or an image",
        test: pkg_publish,
        must: &[
            "npm publish",
            "npm publish --access public",
            "cargo publish -p theseus-core",
            "docker push ghcr.io/x/y:1",
            "docker buildx build --push -t x .",
            "docker image push x",
            "pnpm publish --no-git-checks",
            "yarn npm publish",
            "twine upload dist/*",
            "gem push x.gem",
        ],
        must_not: &[
            "npm publish --dry-run",
            "cargo publish --dry-run",
            "cargo package",
            "npm pack",
            "docker pull x",
            "docker build -t x .",
            "npm install",
        ],
    },
    Rule {
        id: "gh.destroy",
        kind: DESTROY_REMOTE,
        why: "deleting a repository, release, issue, or gist",
        test: gh_destroy,
        must: &[
            "gh repo delete owner/repo --yes",
            "gh release delete v1 -y",
            "gh release delete-asset v1 a.tgz",
            "gh issue delete 5",
            "gh gist delete abc",
        ],
        must_not: &["gh repo view", "gh repo list", "gh release view v1"],
    },
    Rule {
        id: "aws.destroy",
        kind: DESTROY_REMOTE,
        why: "deleting a bucket, instance, database, stack, or key",
        test: aws_destroy,
        must: &[
            "aws s3 rb s3://bucket --force",
            "aws s3 rm s3://bucket/prefix --recursive",
            "aws s3 sync ./site s3://bucket --delete",
            "aws ec2 terminate-instances --instance-ids i-1",
            "aws --region us-west-2 rds delete-db-instance --db-instance-identifier x",
            "aws kms schedule-key-deletion --key-id k",
        ],
        must_not: &[
            "aws s3 rm s3://bucket/one-key",
            "aws s3 ls s3://bucket",
            "aws s3 sync ./site s3://bucket",
            "aws s3 sync s3://bucket ./local --delete",
            "aws ec2 describe-instances",
        ],
    },
    Rule {
        id: "npm.unpublish",
        kind: DESTROY_REMOTE,
        why: "removing a published package",
        test: npm_unpublish,
        must: &["npm unpublish pkg@1.0.0", "npm unpublish --force"],
        must_not: &["npm publish", "npm deprecate x msg"],
    },
    Rule {
        id: "infra.destroy",
        kind: DESTROY_REMOTE,
        why: "destroying infrastructure or cluster resources",
        test: infra_destroy,
        must: &[
            "terraform destroy -auto-approve",
            "terraform apply -destroy",
            "kubectl delete deployment web",
            "helm uninstall web",
        ],
        must_not: &["terraform plan", "kubectl get pods", "helm list"],
    },
    Rule {
        id: "gh.post",
        kind: EXTERNAL_POST,
        why: "posting an issue, pull request, comment, review, or gist",
        test: gh_post,
        must: &[
            "gh issue create --title t --body b",
            "gh issue comment 3 --body hi",
            "gh pr comment 5 -b x",
            "gh pr review 5 --approve",
            "gh pr create --fill",
            "gh pr edit 5 --title x",
            "gh gist create notes.md",
        ],
        must_not: &[
            "gh issue list",
            "gh issue view 3 --comments",
            "gh pr view 5",
            "gh pr checks 5",
            "gh gist create -p notes.md",
        ],
    },
    Rule {
        id: "http.post",
        kind: EXTERNAL_POST,
        why: "sending data to a URL",
        test: http_post,
        must: &[
            "curl -X POST https://hooks.example.com/x -d '{}'",
            "curl -d a=1 https://example.com",
            "curl --json '{}' https://api.example.com",
            "curl -XDELETE https://api.example.com/x",
            "curl -sS -F file=@a.txt https://example.com",
            "wget --post-data=a https://example.com",
        ],
        must_not: &[
            "curl https://example.com",
            "curl -sSfL -o out.tgz https://example.com/x.tgz",
            "curl -I https://example.com",
            "curl -X GET https://example.com",
            "wget https://example.com/x",
        ],
    },
    Rule {
        id: "mail.send",
        kind: EXTERNAL_POST,
        why: "sending mail",
        test: mail_send,
        must: &["git send-email --to x@example.com 0001.patch", "sendmail -t"],
        must_not: &["git format-patch -1", "git log --author=mail"],
    },
    Rule {
        id: "opaque.inline",
        kind: OPAQUE,
        why: "inline interpreter code the rules cannot read",
        test: inline_code,
        must: &[
            "python -c 'import os'",
            "python3 -u -c 'print(1)'",
            "python3.12 -c 'x'",
            "node -e 'require(\"fs\")'",
            "perl -e 'unlink glob q(*)'",
            "ruby -e 'puts 1'",
            "deno eval 'Deno.exit(0)'",
            "bun -e x",
            "php -r 'echo 1;'",
        ],
        must_not: &[
            "python --version",
            "node --version",
            "python3 manage.py check",
            "perl -v",
        ],
    },
    Rule {
        id: "opaque.script",
        kind: OPAQUE,
        why: "a script or module the rules cannot read",
        test: script,
        must: &[
            "bash deploy.sh",
            "sh",
            "./x.sh",
            "scripts/gate.sh",
            "python3 manage.py migrate",
            "python -m pip install x",
            "node scripts/build.mjs",
            "deploy.sh prod",
            "bash -c \"$CMD\"",
            "sudo -s",
        ],
        must_not: &[
            "bash -c 'ls'",
            "python3 --version",
            "ls ./x.sh",
            "cat scripts/gate.sh",
            "bash --version",
        ],
    },
    Rule {
        id: "opaque.runner",
        kind: OPAQUE,
        why: "a target or script defined in a file the rules cannot read (a Makefile, package.json, a workflow)",
        test: runner,
        must: &[
            "make",
            "make deploy",
            "just release",
            "npm run build",
            "npm test",
            "npx some-tool",
            "pnpm dlx create-x",
            "yarn build",
            "cargo run --release",
            "cargo xtask dist",
            "go run ./cmd/x",
            "terraform apply",
            "uv run pytest",
        ],
        must_not: &[
            "npm install",
            "npm ci",
            "cargo build",
            "cargo test",
            "go build ./...",
            "yarn install",
            "yarn",
            "pnpm install",
            "terraform plan",
            "make --version",
        ],
    },
    Rule {
        id: "opaque.eval",
        kind: OPAQUE,
        why: "`eval` or `source` runs text the rules only partly see",
        test: eval_like,
        must: &["eval \"$X\"", "eval 'git status'", "source env.sh", ". ./env.sh"],
        must_not: &["echo eval", "ls ."],
    },
    Rule {
        id: "opaque.remote",
        kind: OPAQUE,
        why: "a command run on another machine",
        test: remote_command,
        must: &["ssh host 'ls /srv'", "ssh -p 22 user@host uptime"],
        must_not: &["ssh-keygen -t ed25519", "ssh -V", "ssh host"],
    },
    Rule {
        id: "opaque.shell",
        kind: OPAQUE,
        why: "shell text the parser cannot see through",
        test: shell_opaque,
        must: &["$CMD --force", "echo 'unterminated", "\"$(which git)\" push"],
        must_not: &["echo $HOME", "git push \"$BRANCH\""],
    },
    Rule {
        id: "opaque.git",
        kind: OPAQUE,
        why: "a git alias, extension, or configured program the rules cannot read",
        test: git_opaque,
        must: &[
            "git pf",
            "git -c alias.x='!rm -rf ~' x",
            "git -c core.sshCommand='ssh -i k' push",
            "git rebase -x 'make test' main",
            "git submodule foreach 'git status'",
            "git bisect run ./test.sh",
        ],
        must_not: &[
            "git status",
            "git -c user.name=x commit -m m",
            "git log -p",
            "git lfs push origin main",
            "git rebase main",
        ],
    },
    Rule {
        id: "opaque.gh",
        kind: OPAQUE,
        why: "a GitHub API write, workflow run, or gh extension the rules cannot read",
        test: gh_opaque,
        must: &[
            "gh api -X POST repos/o/r/issues -f title=x",
            "gh api repos/o/r/pulls/1/merge -X PUT",
            "gh api --method DELETE repos/o/r",
            "gh api graphql -f query=x",
            "gh workflow run deploy.yml",
            "gh my-extension run",
        ],
        must_not: &[
            "gh api repos/o/r",
            "gh api user --jq .login",
            "gh pr list",
            "gh workflow list",
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        assert!(
            st.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&st.stderr)
        );
    }

    /// The fixture every example runs in: a repository that ignores its build
    /// outputs, with tracked work beside them.
    fn fixture() -> Option<tempfile::TempDir> {
        if Command::new("git").arg("--version").output().is_err() {
            return None;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let w = |p: &str, s: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        };
        w(
            ".gitignore",
            "target/\nnode_modules/\n/dist/\nbuild/\n*.log\n",
        );
        w("README.md", "readme\n");
        w("notes.md", "notes\n");
        w("src/lib.rs", "// lib\n");
        w("somedir/a.txt", "work\n");
        w("tracked/dist/app.js", "// committed output\n");
        w("build/keep.txt", "force-added\n");
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["add", "."]);
        git(root, &["add", "-f", "build/keep.txt"]);
        git(root, &["commit", "-q", "-m", "fixture"]);
        w("target/debug/x", "built\n");
        w("node_modules/pkg/index.js", "dep\n");
        w("web/node_modules/x/index.js", "dep\n");
        w("dist/app.js", "built\n");
        Some(d)
    }

    fn fires(rule: &Rule, text: &str, root: &Path) -> (bool, bool, Option<bool>) {
        let cwds = Cwds::one(root);
        let mut parsed = vec![];
        shell::parse_script(text, &cwds, 0, &[], None, &mut parsed);
        let as_text = parsed.iter().any(|c| (rule.test)(c));
        let bash = shell::commands(&["bash".into(), "-c".into(), text.into()], root)
            .iter()
            .any(|c| (rule.test)(c));
        // A single plain command also runs as a direct argv, without a shell.
        let direct = shell::words_of(text)
            .filter(|ws| ws.iter().all(|w| !w.dynamic && !w.glob))
            .map(|ws| {
                let argv: Vec<String> = ws.iter().map(|w| w.text.clone()).collect();
                shell::commands(&argv, root).iter().any(|c| (rule.test)(c))
            });
        (as_text, bash, direct)
    }

    #[test]
    fn every_rule_passes_its_examples() {
        let Some(d) = fixture() else { return };
        let root = d.path().canonicalize().unwrap();
        let mut failures = vec![];
        for r in RULES {
            assert!(
                !r.must.is_empty() && !r.must_not.is_empty(),
                "{} needs examples both ways",
                r.id
            );
            assert!(
                SEED_KINDS.iter().any(|k| k.name == r.kind),
                "{}: unknown kind {}",
                r.id,
                r.kind
            );
            for ex in r.must {
                let (t, b, a) = fires(r, ex, &root);
                if !(t && b && a.unwrap_or(true)) {
                    failures.push(format!(
                        "{} must match `{ex}` (text {t}, bash -c {b}, argv {a:?})",
                        r.id
                    ));
                }
            }
            for ex in r.must_not {
                let (t, b, a) = fires(r, ex, &root);
                if t || b || a.unwrap_or(false) {
                    failures.push(format!(
                        "{} must not match `{ex}` (text {t}, bash -c {b}, argv {a:?})",
                        r.id
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        let mut ids: Vec<&str> = RULES.iter().map(|r| r.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), RULES.len(), "rule ids are unique");
    }

    #[test]
    fn detect_names_each_command_and_reads_owner_rules() {
        let Some(d) = fixture() else { return };
        let root = d.path().canonicalize().unwrap();
        let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let found = detect(
            &argv(&[
                "bash",
                "-c",
                "cargo build && git push -f origin main; rm -rf target",
            ]),
            &root,
            &[],
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, "history_rewrite");
        assert_eq!(found[0].rule, "git.push.force");
        assert_eq!(found[0].detail, "git push -f origin main");
        assert!(
            detect(&argv(&["git", "push"]), &root, &[]).is_empty(),
            "a plain push is no consequence"
        );
        let owner = [OwnerRule {
            kind: "deploy".into(),
            prefix: argv(&["./deploy.sh", "prod"]),
        }];
        let found = detect(
            &argv(&["sh", "-c", "./deploy.sh prod --now"]),
            &root,
            &owner,
        );
        let kinds: Vec<&str> = found.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, vec!["opaque", "deploy"], "{found:?}");
        assert_eq!(found[1].rule, "config:deploy");
    }

    /// Spellings a model could reach for that must not hide a force push:
    /// quoting, wrappers, compound commands, substitutions, global options.
    #[test]
    fn no_spelling_hides_a_force_push() {
        let root = Path::new("/tmp");
        let hidden: Vec<&str> = [
            "g\"it\" push -f",
            "$'git' push -f",
            "\\git push -f",
            "command git push -f",
            "exec git push -f",
            "nice -n 5 git push -f",
            "IFS=x; git push -f",
            "{ git push -f; }",
            "git push -f &",
            "x=$(git push -f)",
            "echo | git push -f",
            "if git push -f; then :; fi",
            "while ! git push -f; do sleep 1; done",
            "timeout -s KILL 10 git push -f",
            "env -- git push -f",
            "git -c push.default=current push -f",
            "git --git-dir=.git push -f",
            "xargs -I{} git push -f origin {}",
            "bash -xc 'git push -f'",
            "bash -c -- 'git push -f'",
            "bash --norc -c 'git push -f'",
            "sudo -E git push -f",
            "nohup bash -c \"git push -f\" &",
            "(cd sub && git push origin +main)",
            "git push origin main:main +dev:dev",
            "echo \"$(git push --force)\"",
            "cat <<EOF\n$(git push -f)\nEOF",
            "find . -maxdepth 0 -exec git push -f \\;",
            // 2a+: over-approximation over every word, not just the program.
            "git push origin main --{force,}",    // brace expansion
            "F=--force; git push origin main $F", // an unquoted variable option
            "git push origin main -$(printf f)",  // a substitution in an option
            "git push origin \"$BRANCH\"",        // a dynamic refspec operand
            "git push --mirro backup",            // a unique-prefix long option
            "git reset --har",                    // (bulk_delete, but still irreversible)
        ]
        .into_iter()
        .filter(|t| {
            !detect(&["bash".into(), "-c".into(), (*t).into()], root, &[])
                .iter()
                .any(|c| matches!(c.kind.as_str(), "history_rewrite" | "bulk_delete"))
        })
        .collect();
        assert!(
            hidden.is_empty(),
            "these hid an irreversible call: {hidden:#?}"
        );
        // `bash -c 'script' arg0 args…`: the positional parameters are literal.
        assert!(
            detect(
                &[
                    "bash".into(),
                    "-c".into(),
                    "git push origin main \"$@\"".into(),
                    "_".into(),
                    "-f".into(),
                ],
                root,
                &[],
            )
            .iter()
            .any(|c| c.kind == kind::HISTORY_REWRITE),
            "\"$@\" is substituted with the literal args, so -f is seen exactly"
        );
    }

    /// The recursive-delete siblings of the force-push test: no spelling hides a
    /// `bulk_delete`, and the precise rules keep common safe calls quiet.
    #[test]
    fn no_spelling_hides_a_recursive_delete() {
        let Some(d) = fixture() else { return };
        let root = d.path().canonicalize().unwrap();
        let fires = |t: &str| {
            detect(&["bash".into(), "-c".into(), t.into()], &root, &[])
                .iter()
                .any(|c| c.kind == kind::BULK_DELETE)
        };
        let hidden: Vec<&str> = [
            "rm -rf target/{,../src}", // a brace alternative escapes the build output
            "F=-rf; rm $F somedir",    // -r hidden in an unquoted variable
            "rm -rf somedir/{a,b}/../..", // a brace then `..` climbs to the work tree
            "rm --recursiv -f somedir", // a unique-prefix long option
            "rm -rf target/*/../../src", // a glob then `..` escapes to tracked work
        ]
        .into_iter()
        .filter(|t| !fires(t))
        .collect();
        assert!(
            hidden.is_empty(),
            "these hid a recursive delete: {hidden:#?}"
        );
        // Precision: a quoted operand with no `-r` deletes one file, and a plain
        // push or a build-output delete is not a consequence.
        assert!(
            !fires("for f in *.log; do rm \"$f\"; done"),
            "a quoted single operand with no -r is one file, not a bulk delete"
        );
        assert!(
            !fires("rm -rf target/{debug,release}"),
            "both are build outputs"
        );
        let force = |t: &str| {
            detect(&["bash".into(), "-c".into(), t.into()], &root, &[])
                .iter()
                .any(|c| c.kind == kind::HISTORY_REWRITE)
        };
        assert!(
            !force("git push origin main"),
            "a plain push is no consequence"
        );
        assert!(
            !force("git push --follow-tags"),
            "not an abbreviation of a force flag"
        );
    }

    /// Step 2a++: quoting stops word splitting, not option parsing, so a quoted
    /// variable can be an option; and `$@`/`$*` expand with bash's word
    /// semantics. Each review spelling is caught, and the safe calls stay quiet.
    #[test]
    fn a_quoted_variable_can_be_an_option_and_at_expands_per_argument() {
        let Some(d) = fixture() else { return };
        let root = d.path().canonicalize().unwrap();
        let has = |argv: &[&str], kind: &str| {
            detect(
                &argv.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                &root,
                &[],
            )
            .iter()
            .any(|c| c.kind == kind)
        };
        let bash = |t: &str, kind: &str| has(&["bash", "-c", t], kind);
        // A quoted variable is one word that could be any single option.
        let missed: Vec<&str> = [
            ("x=-rf; rm \"$x\" somedir", kind::BULK_DELETE),
            ("m=--hard; git reset \"$m\"", kind::BULK_DELETE),
            ("m=--hard; git reset \"$m\" HEAD~3", kind::BULK_DELETE),
            ("f=--force; git push \"$f\"", kind::HISTORY_REWRITE),
            (
                "f=--force; git push \"$f\" origin main",
                kind::HISTORY_REWRITE,
            ),
            ("x=-delete; find . \"$x\"", kind::BULK_DELETE),
        ]
        .into_iter()
        .filter(|(t, k)| !bash(t, k))
        .map(|(t, _)| t)
        .collect();
        assert!(
            missed.is_empty(),
            "a quoted variable hid an option: {missed:#?}"
        );
        // `"$@"` is one word per argument, so `-rf` and `somedir` arrive as two
        // words; joined into one they would hide (`-rf somedir` is no `-r`).
        assert!(
            has(
                &["bash", "-c", "rm \"$@\"", "_", "-rf", "somedir"],
                kind::BULK_DELETE
            ),
            "\"$@\" expands to -rf and somedir as separate words"
        );
        assert!(
            has(
                &["bash", "-c", "rm $@", "_", "-rf", "somedir"],
                kind::BULK_DELETE
            ),
            "an unquoted $@ splits into -rf and somedir"
        );
        assert!(
            has(
                &["bash", "-c", "git push origin main \"$@\"", "_", "--force"],
                kind::HISTORY_REWRITE
            ),
            "\"$@\" carries --force through as its own word"
        );
        // `"$*"` joins into one word: `rm -rf "$*"` deletes one path (with a
        // space), which is still a recursive delete of non-regenerable work.
        assert!(
            has(
                &["bash", "-c", "rm -rf \"$*\"", "_", "a", "b"],
                kind::BULK_DELETE
            ),
            "\"$*\" is one joined operand under -rf"
        );
        // Precision: these stay quiet.
        assert!(!bash(
            "for f in *.log; do rm \"$f\"; done",
            kind::BULK_DELETE
        ));
        assert!(!bash("git push origin main", kind::HISTORY_REWRITE));
        assert!(!bash("rm -rf target", kind::BULK_DELETE));
        assert!(!bash("find . -name \"$p\" -print", kind::BULK_DELETE));
    }

    /// Step 2a.3: resolving every simple variable reference from the script's
    /// literal assignments makes a hidden option, operand, or subcommand exact,
    /// and makes an honest script with literal values exact instead of a wait.
    #[test]
    fn resolution_from_literal_assignments_reaches_every_word() {
        let Some(d) = fixture() else { return };
        let root = d.path().canonicalize().unwrap();
        let has = |argv: &[&str], kind: &str| {
            detect(
                &argv.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                &root,
                &[],
            )
            .iter()
            .any(|c| c.kind == kind)
        };
        let bash = |t: &str, kind: &str| has(&["bash", "-c", t], kind);
        // Must catch: resolution reaches an option or operand carried in a
        // variable, an array, `set --`, a literal option prefix on a dynamic
        // word, and a git subcommand (resolved, or judged as each when it is not).
        let missed: Vec<&str> = [
            ("DIR=src; rm -rf \"$DIR\"", kind::BULK_DELETE),
            ("set -- -rf somedir; rm \"$@\"", kind::BULK_DELETE),
            ("a=(-rf somedir); rm \"${a[@]}\"", kind::BULK_DELETE),
            ("x=\"-rf somedir\"; rm $x", kind::BULK_DELETE),
            ("rm -r$(printf f) somedir", kind::BULK_DELETE),
            ("x=push; git $x -f origin main", kind::HISTORY_REWRITE),
            ("git $x -f origin main", kind::HISTORY_REWRITE),
        ]
        .into_iter()
        .filter(|(t, k)| !bash(t, k))
        .map(|(t, _)| t)
        .collect();
        assert!(missed.is_empty(), "resolution missed: {missed:#?}");
        assert!(bash(
            "for b in main dev; do git push --force origin $b; done",
            kind::HISTORY_REWRITE
        ));
        // Precision: with literal values the honest calls are exact and quiet.
        let over: Vec<&str> = [
            (
                "BRANCH=main; git push origin \"$BRANCH\"",
                kind::HISTORY_REWRITE,
            ),
            ("SHA=abc123; git reset $SHA", kind::BULK_DELETE),
            ("DIR=target; rm -rf \"$DIR\"", kind::BULK_DELETE),
            ("for f in *.log; do rm \"$f\"; done", kind::BULK_DELETE),
            (
                "for b in main dev; do git push origin $b; done",
                kind::HISTORY_REWRITE,
            ),
        ]
        .into_iter()
        .filter(|(t, k)| bash(t, k))
        .map(|(t, _)| t)
        .collect();
        assert!(over.is_empty(), "resolution over-refused: {over:#?}");
        // The detail shows the resolved command and where a value came from.
        let cs = detect(
            &[
                "bash".into(),
                "-c".into(),
                "F=--force; git push origin main $F".into(),
            ],
            &root,
            &[],
        );
        let f = cs.iter().find(|c| c.kind == kind::HISTORY_REWRITE).unwrap();
        assert_eq!(
            f.detail, "git push origin main --force (`$F` = `--force`)",
            "{}",
            f.detail
        );
        // An unresolvable git subcommand names the subcommand each rule read.
        let cs = detect(
            &["bash".into(), "-c".into(), "git $x -f origin main".into()],
            &root,
            &[],
        );
        let f = cs.iter().find(|c| c.kind == kind::HISTORY_REWRITE).unwrap();
        assert!(
            f.detail.contains("possibly: `$x` could be `push`"),
            "{}",
            f.detail
        );
    }
}
