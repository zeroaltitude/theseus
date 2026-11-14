//! Formatting before the gate (issue 6, the loop's failure handling, condition 2).
//!
//! The gate's first phase refuses unformatted code, so a build whose only fault was
//! formatting spent a whole gate on it, and counted as a failed attempt. Now the attempt
//! formats the tree first ([`format_tree`], run between the model's last edit and the
//! commit), so what the gate sees is already formatted, and an unformatted change is
//! never an attempt.
//!
//! `cargo fmt` reads the tree's source and runs only rustfmt: it builds nothing and runs
//! none of the branch's code, so it runs here at L0 beside the git calls, while the gate
//! that does run the branch's code stays in L1. A tree that cannot be formatted (no cargo
//! on the machine, a source file rustfmt cannot parse) is not an error: the gate judges
//! it as before, and says so itself.

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

/// `cargo fmt --all` in the tree, run only when `--check` says something is unformatted.
/// Blocking: a child process.
pub fn format_tree(p: &Place, tree: &Tree) -> Formatted {
    let check = match cargo_fmt(p, tree, true) {
        Ok(o) => o,
        Err(why) => return Formatted::Unavailable(why),
    };
    if check.status.success() {
        return Formatted::Clean;
    }
    let stdout = String::from_utf8_lossy(&check.stdout).into_owned();
    let files = diffs_in(&stdout, tree);
    if files.is_empty() {
        // A failure that names no diff is an error rustfmt reports (a parse error),
        // not formatting to apply.
        let err = String::from_utf8_lossy(&check.stderr);
        return Formatted::Unavailable(format!(
            "cargo fmt could not read the tree: {}",
            err.lines().next().unwrap_or("it printed nothing")
        ));
    }
    match cargo_fmt(p, tree, false) {
        Ok(o) if o.status.success() => Formatted::Fixed(files),
        Ok(o) => Formatted::Unavailable(format!(
            "cargo fmt failed ({})",
            o.status
                .code()
                .map_or_else(|| "a signal".into(), |c| format!("exit {c}"))
        )),
        Err(why) => Formatted::Unavailable(why),
    }
}

fn cargo_fmt(p: &Place, tree: &Tree, check: bool) -> Result<std::process::Output, String> {
    let mut c = Command::new("cargo");
    c.args(["fmt", "--all"]);
    if check {
        c.args(["--", "--check"]);
    }
    c.current_dir(&tree.dir)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &p.env {
        c.env(k, v);
    }
    let child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
        .map_err(|e| format!("cargo could not start: {e}"))?;
    child
        .wait_with_output()
        .map_err(|e| format!("cargo fmt did not finish: {e}"))
}

/// The files `--check` printed a `Diff in <path>:<line>` for, relative to the tree, sorted
/// and without repeats.
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
            let path = std::path::Path::new(path);
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
