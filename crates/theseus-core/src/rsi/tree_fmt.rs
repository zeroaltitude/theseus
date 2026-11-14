//! Formatting before the gate (issue 6, the loop's failure handling, condition 2).
//!
//! The gate's first phase refuses unformatted code, so a build whose only fault was
//! formatting spent a whole gate on it, and counted as a failed attempt. Now the attempt
//! formats the tree first ([`format_tree`], run between the model's last edit and the
//! commit), so what the gate sees is already formatted, and an unformatted change is
//! never an attempt.
//!
//! **It runs `rustfmt` itself, never `cargo`** (the first review's F1). A tree is the model's
//! own writing, and `cargo` obeys what the tree says: a `[alias]` in its `.cargo/config.toml`
//! takes over `cargo fmt` (it is an external subcommand), and its `rust-toolchain.toml` picks
//! the compiler. Either would run the branch's code here at L0, before any review, with the
//! daemon's environment, while the rule is that a self build's code runs in L1 and nowhere
//! else. `rustfmt` only reads source and its own config (`rustfmt.toml`, found from each
//! file), so it is run from a directory outside the tree, over the changed `.rs` files by
//! absolute path, with the edition read from the tree's `Cargo.toml` as text and the toolchain
//! main pins (`rust-toolchain.toml` at `origin/main`, read from git, never from the tree).
//!
//! A tree that cannot be formatted (no rustfmt on the machine, a file that does not parse) is
//! not an error: the gate judges it as before, and says so itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use theseus_kernel::children::{self, Kind};

use super::tree::{Place, Tree};

/// What formatting a tree did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Formatted {
    /// Already formatted; nothing changed.
    Clean,
    /// These files were reformatted (paths inside the tree, sorted).
    Fixed(Vec<String>),
    /// Formatting did not run: why. The gate judges the tree as it is.
    Unavailable(String),
}

impl Formatted {
    /// The line a commit message carries when files were reformatted.
    pub fn line(&self) -> Option<String> {
        match self {
            Formatted::Fixed(files) => {
                Some(format!("Formatted before the gate: {}.", files.join(", ")))
            }
            _ => None,
        }
    }
}

/// `rustfmt` over the tree's changed `.rs` files, written only when `--check` says something is
/// unformatted. Blocking: child processes.
pub fn format_tree(p: &Place, tree: &Tree) -> Formatted {
    let files = match changed_rust_files(p, tree) {
        Ok(f) => f,
        Err(why) => return Formatted::Unavailable(why),
    };
    if files.is_empty() {
        return Formatted::Clean;
    }
    let run = Run {
        place: p,
        edition: edition_of(tree),
        toolchain: pinned_toolchain(p),
        files: &files,
    };
    let check = match run.rustfmt(true) {
        Ok(o) => o,
        Err(why) => return Formatted::Unavailable(why),
    };
    if check.status.success() {
        return Formatted::Clean;
    }
    let stdout = String::from_utf8_lossy(&check.stdout).into_owned();
    let diffs = diffs_in(&stdout, tree);
    if diffs.is_empty() {
        // A failure that names no diff is an error rustfmt reports (a parse error), not
        // formatting to apply.
        let err = String::from_utf8_lossy(&check.stderr);
        return Formatted::Unavailable(format!(
            "rustfmt could not read the tree: {}",
            err.lines().next().unwrap_or("it printed nothing")
        ));
    }
    match run.rustfmt(false) {
        Ok(o) if o.status.success() => Formatted::Fixed(diffs),
        Ok(o) => Formatted::Unavailable(format!(
            "rustfmt failed ({})",
            o.status
                .code()
                .map_or_else(|| "a signal".into(), |c| format!("exit {c}"))
        )),
        Err(why) => Formatted::Unavailable(why),
    }
}

/// The `.rs` files the tree changes against `origin/main` (committed, edited or new), as
/// absolute paths that exist.
fn changed_rust_files(p: &Place, tree: &Tree) -> Result<Vec<PathBuf>, String> {
    let git = |args: &[&str]| {
        p.git_ok(&tree.dir, args)
            .map_err(|e| format!("git could not list the tree's changes: {e:#}"))
    };
    let mut names = git(&["diff", "--name-only", "origin/main"])?;
    names.push('\n');
    names.push_str(&git(&["ls-files", "--others", "--exclude-standard"])?);
    let mut files: Vec<PathBuf> = names
        .lines()
        .map(str::trim)
        .filter(|n| n.ends_with(".rs"))
        .map(|n| tree.dir.join(n))
        .filter(|f| f.is_file())
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

/// The edition the tree's `Cargo.toml` names (read as text, never run), else 2021.
fn edition_of(tree: &Tree) -> String {
    std::fs::read_to_string(tree.dir.join("Cargo.toml"))
        .ok()
        .and_then(|t| {
            t.lines()
                .filter_map(|l| l.trim().strip_prefix("edition"))
                .filter_map(|l| l.trim_start().strip_prefix('='))
                .map(|v| v.trim().trim_matches('"').to_string())
                .find(|v| v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty())
        })
        .unwrap_or_else(|| "2021".into())
}

/// The toolchain main pins, from main's own `rust-toolchain.toml` as git has it: never the
/// tree's, which the model may have rewritten. None when main pins none.
fn pinned_toolchain(p: &Place) -> Option<String> {
    let text = p
        .git_ok(
            &p.repo,
            &["show", "refs/remotes/origin/main:rust-toolchain.toml"],
        )
        .ok()?;
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("channel"))
        .filter_map(|l| l.trim_start().strip_prefix('='))
        .map(|v| v.trim().trim_matches('"').to_string())
        .find(|v| !v.is_empty())
}

struct Run<'a> {
    place: &'a Place,
    edition: String,
    toolchain: Option<String>,
    files: &'a [PathBuf],
}

impl Run<'_> {
    /// One `rustfmt` over the files, from the state directory (outside every tree, so no
    /// `rust-toolchain.toml` of a tree is read), checking or writing.
    fn rustfmt(&self, check: bool) -> Result<std::process::Output, String> {
        if let Some(chan) = &self.toolchain {
            let out = self.once(Some(chan), check)?;
            // Not the rustup proxy, or the pinned toolchain is not installed: the default one.
            if out.status.success() || !String::from_utf8_lossy(&out.stderr).contains("toolchain") {
                return Ok(out);
            }
        }
        self.once(None, check)
    }

    fn once(&self, chan: Option<&str>, check: bool) -> Result<std::process::Output, String> {
        let mut c = Command::new("rustfmt");
        if let Some(chan) = chan {
            c.arg(format!("+{chan}"));
        }
        c.args(["--edition", &self.edition]);
        if check {
            c.arg("--check");
        }
        c.args(self.files)
            .current_dir(&self.place.state)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.place.env {
            c.env(k, v);
        }
        let child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
            .map_err(|e| format!("rustfmt could not start: {e}"))?;
        child
            .wait_with_output()
            .map_err(|e| format!("rustfmt did not finish: {e}"))
    }
}

/// The files `--check` printed a `Diff in <path>:<line>` for, relative to the tree, sorted and
/// without repeats.
fn diffs_in(check_output: &str, tree: &Tree) -> Vec<String> {
    let mut files: Vec<String> = check_output
        .lines()
        .filter_map(|l| l.strip_prefix("Diff in "))
        .map(|l| l.trim_end_matches(':'))
        // `path:line` (this rustfmt) or `path at line N` (older ones).
        .filter_map(|l| {
            l.split_once(" at line ")
                .map(|(path, _)| path)
                .or_else(|| l.rsplit_once(':').map(|(path, _)| path))
        })
        .map(|path| {
            let path = Path::new(path);
            path.strip_prefix(&tree.dir)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect();
    files.sort();
    files.dedup();
    files
}
