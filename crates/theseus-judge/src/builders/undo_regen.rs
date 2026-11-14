//! "Trivially recreatable", from a CLOSED table (`p5kln-location-facts`, row
//! B6 `theseus-2yowd`; the owner, 2026-11-13 11:28).
//!
//! WHY A TABLE AND NOT A RULE. `undo_resolve`'s header refused to resolve
//! `regenerable` at all, and the refusal was right about the thing it was
//! aimed at: "git ignores it and its first component is a build-output name"
//! is a LOCATION rule, which is the exact thing this classifier replaced. But
//! the owner named being-recreatable as one of the three facts code should
//! state, so the answer is not "never" — it is "only where the project itself
//! says so". Every entry here names a MANIFEST that must be PRESENT, and the
//! manifest is the project's own statement that it builds this directory. An
//! ignored directory beside no manifest stays `untracked`, which is the safe
//! word: a wrong `untracked` only holds a call that was safe, where a wrong
//! `regenerable` runs one that was not.
//!
//! THE TABLE IS CLOSED, AND THAT IS THE POINT. It is three lines, each saying
//! what recreates the path, and a reader can argue with all three at once.
//! Anything not in it is not regenerable however much it looks like a build
//! output: `dist/`, `build/`, `.venv/`, `out/`, an ignored `.env`, an ignored
//! data directory. A table that grows every time somebody meets a new
//! ecosystem is the approve list coming back in a new costume, so growing it
//! is a decision with a reviewer, not a convenience.
//!
//! THE TWO CONDITIONS, AND THE ORDER THEY ARE ASKED IN. A path qualifies only
//! when the repository IGNORES it **and** the table matches. The table is
//! tested FIRST, because it is pure path arithmetic plus at most two
//! `exists` calls, and `git check-ignore` is a process. So the common case —
//! every target that is not a build output — pays nothing for this module,
//! and the budget the probe's own header watches (`on_path_p95`, 300 ms) is
//! unmoved for it. The ignore test is asked only of a path the table has
//! already recognised.
//!
//! WHY IGNORED IS REQUIRED AT ALL, when the manifest already says the project
//! builds it. Because a path can be *named* `target` and be checked in — some
//! repositories commit a `target/` of fixtures — and then the bytes are the
//! repository's, not the build's. `git check-ignore` consults the index by
//! default, so a tracked path is not reported ignored, which is exactly the
//! discrimination wanted here.

use std::path::{Component, Path};

use super::undo_resolve::Probe;

/// What must sit beside a build directory for the project to be saying it
/// builds it.
#[derive(Debug, Clone, Copy)]
enum Needs {
    /// One of these files, in the directory that holds the build directory.
    Manifest(&'static [&'static str]),
    /// A manifest AND one of these lockfiles, both beside it. Without a lock
    /// the install is not reproducible — it would fetch whatever is newest —
    /// so the directory is not the same bytes again and does not qualify.
    ManifestAndLock(&'static [&'static str], &'static [&'static str]),
    /// The compiled module's own source, beside the cache directory: for
    /// `<dir>/__pycache__/x.cpython-312.pyc` that is `<dir>/x.py`. An
    /// orphaned `.pyc` whose source is gone is NOT recreatable, and this is
    /// the condition that tells them apart.
    PySource,
}

/// One line of the closed table.
#[derive(Debug, Clone, Copy)]
struct Build {
    /// The path component that names the output. Matched as a WHOLE
    /// component, so `src/target_notes.txt` is not a build path.
    dir: &'static str,
    needs: Needs,
    /// What recreates it. Kept for the reader of this table, and asserted
    /// non-empty by `the_table_is_closed_and_every_line_says_what_recreates_it`,
    /// so a new entry cannot be added without saying this.
    by: &'static str,
}

/// **THE CLOSED TABLE.** One line per entry; each says what recreates it.
const TABLE: &[Build] = &[
    Build {
        dir: "target",
        needs: Needs::Manifest(&["Cargo.toml"]),
        by: "cargo build, from the crate beside it",
    },
    Build {
        dir: "node_modules",
        needs: Needs::ManifestAndLock(
            &["package.json"],
            &[
                "package-lock.json",
                "npm-shrinkwrap.json",
                "yarn.lock",
                "pnpm-lock.yaml",
                "bun.lockb",
                "bun.lock",
            ],
        ),
        by: "the package manager's install, to the lockfile's exact versions",
    },
    Build {
        dir: "__pycache__",
        needs: Needs::PySource,
        by: "the Python interpreter, on the next import of the source beside it",
    },
];

/// Whether the project's own build makes `path` again: the table matches and
/// the repository ignores it. The order is the module header's — the table
/// first, because the ignore test is a process.
pub fn regenerable(probe: &mut dyn Probe, path: &Path) -> bool {
    recreated_by(probe, path).is_some()
}

/// **What** recreates `path`, in the table's own words, or `None` when
/// nothing in the table does. This is the reader of each line's `by` (the
/// reader rule, AGENTS.md: nothing is declared without its reader), and it is
/// what a hold's sentence would say if it ever names the reason — "the
/// project's own build makes it again, by `cargo build`" is a better thing to
/// tell a model than a bare word.
pub fn recreated_by(probe: &mut dyn Probe, path: &Path) -> Option<&'static str> {
    let by = table_says(probe, path)?;
    (probe.ignored(path) == Some(true)).then_some(by)
}

/// The `by` of the one line of the table that recognises `path`, when the
/// manifest that line requires is present. The first matching component wins,
/// so a path under `target/` is judged by `target/`'s line and a
/// `node_modules` nested inside one is judged by the outer one.
fn table_says(probe: &mut dyn Probe, path: &Path) -> Option<&'static str> {
    let mut held = std::path::PathBuf::new();
    for c in path.components() {
        let Component::Normal(name) = c else {
            held.push(c.as_os_str());
            continue;
        };
        if let Some(b) = TABLE.iter().find(|b| name == std::ffi::OsStr::new(b.dir)) {
            // `held` is the directory that HOLDS the build directory, which
            // is where the manifest has to be. An empty `held` is the
            // relative current directory, which is what `.` reads as.
            let beside = if held.as_os_str().is_empty() {
                std::path::PathBuf::from(".")
            } else {
                held.clone()
            };
            return met(probe, b.needs, &beside, path).then_some(b.by);
        }
        held.push(name);
    }
    None
}

/// Whether any of `names` is a file in `dir`.
fn any_of(probe: &mut dyn Probe, dir: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| probe.exists(&dir.join(n)))
}

/// Whether `needs` is met beside `beside`, for the target `path`.
fn met(probe: &mut dyn Probe, needs: Needs, beside: &Path, path: &Path) -> bool {
    match needs {
        Needs::Manifest(m) => any_of(probe, beside, m),
        Needs::ManifestAndLock(m, lock) => any_of(probe, beside, m) && any_of(probe, beside, lock),
        // The cache directory itself, with no file named under it, has no one
        // source to point at; the directory is recreated by importing
        // whatever is beside it, so the condition is that at least one source
        // is there. For a named `.pyc` under it, that source is the precise
        // one the name gives.
        Needs::PySource => match py_source(beside, path) {
            Some(src) => probe.exists(&src),
            None => probe.exists(&beside.join("__init__.py")),
        },
    }
}

/// The source a `__pycache__` entry was compiled from:
/// `<beside>/__pycache__/x.cpython-312.pyc` is `<beside>/x.py`. `None` for
/// the cache directory itself, or a name that is not a `.pyc`.
fn py_source(beside: &Path, path: &Path) -> Option<std::path::PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".pyc")?;
    // `x.cpython-312` and `x.cpython-312.opt-1` both come from `x.py`.
    let base = stem.split('.').next()?;
    (!base.is_empty()).then(|| beside.join(format!("{base}.py")))
}

#[cfg(test)]
#[path = "undo_regen/tests.rs"]
mod tests;
