//! Formatting before the gate (issue 6, the loop's failure handling, condition 2).
//!
//! The gate's first phase refuses unformatted code, so a build whose only fault was
//! formatting spent a whole gate on it, and counted as a failed attempt. Now the attempt
//! formats the tree first ([`format_tree`], run between the model's last edit and the
//! commit), so what the gate sees is already formatted, and an unformatted change is
//! never an attempt.
//!
//! **It runs `rustfmt` itself, never `cargo`** (the first review's F1), **and never lets it name
//! a file** (the second review's R1). A tree is the model's own writing, and `cargo` obeys what the
//! tree says: a `[alias]` in its `.cargo/config.toml` takes over `cargo fmt` (it is an external
//! subcommand), and its `rust-toolchain.toml` picks the compiler. Either would run the branch's
//! code here at L0, before any review, with the daemon's environment, while the rule is that a self
//! build's code runs in L1 and nowhere else. And `rustfmt` given a file *path* follows every
//! out-of-line module the file declares (`#[path = "/anywhere.rs"] mod m;`) and writes through a
//! symlink, so a tree could have it rewrite any `.rs` file the daemon's user can write, and name
//! that file in the commit. So each changed `.rs` file is read by the daemon, formatted through
//! **stdin** (which follows no module), and written back by the daemon only when it differs, only
//! if it is a regular file (never a symlink) whose real path is inside the tree. `rustfmt` reads
//! only source and its own config, which is taken from the tree's directory by `--config-path`
//! (`rustfmt.toml`, which no option makes execute anything); it runs from a directory outside the
//! tree, with the edition read from the tree's `Cargo.toml` as text and the toolchain main pins
//! (`rust-toolchain.toml` at `origin/main`, read from git, never from the tree).
//!
//! A tree that cannot be formatted (no rustfmt on the machine, a file that does not parse) is
//! not an error: the gate judges it as before, and says so itself.

use std::io::Write;
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

/// Each changed `.rs` file of the tree, formatted through stdin and written back when it
/// differs. Blocking: child processes.
pub fn format_tree(p: &Place, tree: &Tree) -> Formatted {
    let files = match changed_rust_files(p, tree) {
        Ok(f) => f,
        Err(why) => return Formatted::Unavailable(why),
    };
    let Ok(root) = tree.dir.canonicalize() else {
        return Formatted::Unavailable("the tree's directory could not be read".into());
    };
    let run = Run {
        place: p,
        tree,
        edition: edition_of(tree),
        toolchain: pinned_toolchain(p),
    };
    let (mut fixed, mut unreadable) = (Vec::new(), Vec::new());
    for file in &files {
        match run.format_file(file, &root) {
            Ok(false) => {}
            Ok(true) => fixed.push(rel(file, tree)),
            Err(why) => unreadable.push(why),
        }
    }
    match (fixed.is_empty(), unreadable.first()) {
        (false, _) => {
            fixed.sort();
            Formatted::Fixed(fixed)
        }
        (true, Some(why)) => Formatted::Unavailable(why.clone()),
        (true, None) => Formatted::Clean,
    }
}

/// A file's path relative to the tree (a path git named, so always inside it).
fn rel(file: &Path, tree: &Tree) -> String {
    file.strip_prefix(&tree.dir)
        .unwrap_or(file)
        .display()
        .to_string()
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
    tree: &'a Tree,
    edition: String,
    toolchain: Option<String>,
}

impl Run<'_> {
    /// Format one file through stdin and write it back if it differs. Whether it changed; an
    /// error says why it was left for the gate (a file that does not parse, rustfmt missing).
    /// A symlink, a file outside the tree and a file that is not a regular file are never
    /// touched, and are not an error: they are not the tree's source.
    fn format_file(&self, file: &Path, root: &Path) -> Result<bool, String> {
        let meta = std::fs::symlink_metadata(file)
            .map_err(|e| format!("{}: {e}", rel(file, self.tree)))?;
        if !meta.file_type().is_file() {
            return Ok(false);
        }
        match file.canonicalize() {
            Ok(real) if real.starts_with(root) => {}
            _ => return Ok(false),
        }
        let source = std::fs::read(file).map_err(|e| format!("{}: {e}", rel(file, self.tree)))?;
        let out = self.rustfmt(&source)?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "rustfmt could not read {}: {}",
                rel(file, self.tree),
                err.lines().next().unwrap_or("it printed nothing")
            ));
        }
        if out.stdout == source {
            return Ok(false);
        }
        std::fs::write(file, &out.stdout).map_err(|e| format!("{}: {e}", rel(file, self.tree)))?;
        Ok(true)
    }

    /// One `rustfmt` over `source` on stdin, its formatted text on stdout, from the state directory
    /// (outside every tree, so no `rust-toolchain.toml` of a tree is read).
    fn rustfmt(&self, source: &[u8]) -> Result<std::process::Output, String> {
        if let Some(chan) = &self.toolchain {
            let out = self.once(Some(chan), source)?;
            // Not the rustup proxy, or the pinned toolchain is not installed: the default one.
            if out.status.success() || !String::from_utf8_lossy(&out.stderr).contains("toolchain") {
                return Ok(out);
            }
        }
        self.once(None, source)
    }

    fn once(&self, chan: Option<&str>, source: &[u8]) -> Result<std::process::Output, String> {
        let mut c = Command::new("rustfmt");
        if let Some(chan) = chan {
            c.arg(format!("+{chan}"));
        }
        c.args([
            "--edition",
            &self.edition,
            "--emit",
            "stdout",
            "--config-path",
        ])
        .arg(&self.tree.dir)
        .current_dir(&self.place.state)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        for (k, v) in &self.place.env {
            c.env(k, v);
        }
        let mut child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
            .map_err(|e| format!("rustfmt could not start: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A write the child cut short (it refused the input) is its exit's to say.
            let _ = stdin.write_all(source);
        }
        child
            .wait_with_output()
            .map_err(|e| format!("rustfmt did not finish: {e}"))
    }
}
