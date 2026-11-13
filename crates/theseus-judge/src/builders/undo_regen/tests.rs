//! The closed build-path table (`p5kln-location-facts`): what qualifies, and
//! the three things that must NOT, against real repositories.
//!
//! Every fixture is a throwaway repository under the process's temporary
//! directory, with the operator's global and system git configuration left out
//! (AGENTS.md, Item 7) and signing off, so the fixtures are hermetic and a
//! locked signing agent cannot hang them. Every name in them is invented.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::builders::undo_resolve::{find, recovery_of, GitCli};
use crate::builders::Recovery;

/// A throwaway directory that deletes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "theseus-undo-regen-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("the scratch directory");
        Self(d)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, text: &str) {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).expect("the fixture directory");
    }
    std::fs::write(path, text).expect("the fixture file");
}

/// A repository whose `.gitignore` ignores everything the tests reach for, so
/// that "git ignores it" is never the thing under test: the TABLE is.
fn a_repo(tag: &str) -> Scratch {
    let s = Scratch::new(tag);
    git(s.path(), &["init", "-q", "-b", "main"]);
    write(
        &s.path().join(".gitignore"),
        "target/\nnode_modules/\n__pycache__/\n.env\ndata/\ndist/\n",
    );
    write(&s.path().join("kept.txt"), "the committed bytes\n");
    git(s.path(), &["add", "kept.txt", ".gitignore"]);
    git(s.path(), &["commit", "-q", "-m", "the first commit"]);
    s
}

fn is_regenerable(path: &Path) -> bool {
    let mut p = GitCli::default();
    regenerable(&mut p, path)
}

// ---- what qualifies ------------------------------------------------------

#[test]
fn target_beside_a_cargo_manifest_is_regenerable_and_without_one_is_not() {
    let s = a_repo("target");
    write(&s.path().join("target/debug/app"), "a build output\n");

    // No `Cargo.toml` yet: the project has not said it builds this, so the
    // word is not given however much the path looks like a build directory.
    assert!(
        !is_regenerable(&s.path().join("target/debug/app")),
        "an ignored `target/` beside no manifest is not regenerable"
    );

    // The manifest present is the project's own statement.
    write(&s.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
    assert!(is_regenerable(&s.path().join("target/debug/app")));
    // The directory itself, as `rm -rf target` names it.
    assert!(is_regenerable(&s.path().join("target")));
}

#[test]
fn rm_rf_target_beside_a_cargo_toml_resolves_to_regenerable_end_to_end() {
    // The bar the card names: `rm -rf target` beside a `Cargo.toml` is
    // `regenerable`, read through `find` exactly as the gate's path reads it.
    let s = a_repo("rmrf");
    write(&s.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
    write(&s.path().join("target/debug/app"), "a build output\n");
    let mut p = GitCli::default();
    let f = find(
        &mut p,
        &s.path().join("target").to_string_lossy(),
        "write",
        false,
    );
    assert!(f.regenerable, "the table should recognise it");
    assert_eq!(recovery_of(&f), Recovery::Regenerable);
}

#[test]
fn node_modules_needs_a_lockfile_as_well_as_a_manifest() {
    let s = a_repo("node");
    let under = s.path().join("node_modules/left-pad/index.js");
    write(&under, "module.exports = 0\n");

    // A `package.json` alone is not enough: without a lock the install is not
    // reproducible, so the directory is not the same bytes again.
    write(&s.path().join("package.json"), "{\"name\":\"x\"}\n");
    assert!(
        !is_regenerable(&under),
        "a manifest without a lockfile does not qualify"
    );

    write(&s.path().join("package-lock.json"), "{}\n");
    assert!(is_regenerable(&under));
    assert!(is_regenerable(&s.path().join("node_modules")));
}

#[test]
fn a_pycache_entry_needs_the_source_it_was_compiled_from() {
    let s = a_repo("pyc");
    let pyc = s.path().join("pkg/__pycache__/thing.cpython-312.pyc");
    write(&pyc, "\x00compiled\n");

    // An orphaned `.pyc` whose source is gone is NOT recreatable, and that is
    // the whole discrimination this entry makes.
    assert!(!is_regenerable(&pyc), "no source beside it");

    write(&s.path().join("pkg/thing.py"), "x = 1\n");
    assert!(is_regenerable(&pyc));
    // An optimised name comes from the same source.
    let opt = s.path().join("pkg/__pycache__/thing.cpython-312.opt-1.pyc");
    write(&opt, "\x00compiled\n");
    assert!(is_regenerable(&opt));
}

// ---- what must NOT qualify, which is the card's own bar -------------------

#[test]
fn an_ignored_env_file_is_never_regenerable() {
    // The card's first negative. `.env` is ignored by every repository that
    // has one and nothing recreates it: it is the one file whose loss is
    // unrecoverable AND whose path reads like throwaway configuration.
    let s = a_repo("env");
    write(&s.path().join(".env"), "API_TOKEN_PLACEHOLDER=\n");
    // Even with every manifest in the table present beside it.
    write(&s.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
    write(&s.path().join("package.json"), "{\"name\":\"x\"}\n");
    write(&s.path().join("package-lock.json"), "{}\n");
    assert!(!is_regenerable(&s.path().join(".env")));

    let mut p = GitCli::default();
    let f = find(
        &mut p,
        &s.path().join(".env").to_string_lossy(),
        "write",
        false,
    );
    assert!(!f.regenerable);
    assert_eq!(
        recovery_of(&f),
        Recovery::Untracked,
        "an ignored file git holds no copy of stays `untracked`"
    );
}

#[test]
fn an_ignored_data_directory_is_never_regenerable() {
    // The card's second negative, and the more dangerous of the two: a
    // directory full of the only copy of something, ignored because it is
    // large, beside a manifest that builds something else entirely.
    let s = a_repo("data");
    write(&s.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
    write(&s.path().join("data/measurements.csv"), "1,2,3\n");
    assert!(!is_regenerable(&s.path().join("data")));
    assert!(!is_regenerable(&s.path().join("data/measurements.csv")));

    let mut p = GitCli::default();
    let f = find(
        &mut p,
        &s.path().join("data").to_string_lossy(),
        "write",
        false,
    );
    assert!(!f.regenerable);
    assert_eq!(recovery_of(&f), Recovery::Untracked);
}

#[test]
fn a_tracked_target_directory_is_the_repositorys_bytes_and_not_the_builds() {
    // Why "git ignores it" is required as well as the table: a repository
    // that COMMITS a `target/` of fixtures holds those bytes itself, and the
    // build does not make them again. `check-ignore` consults the index, so
    // a tracked path is not reported ignored, which is what catches this.
    let s = Scratch::new("tracked-target");
    git(s.path(), &["init", "-q", "-b", "main"]);
    write(&s.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
    write(
        &s.path().join("target/fixture.bin"),
        "a committed fixture\n",
    );
    git(s.path(), &["add", "-A"]);
    git(s.path(), &["commit", "-q", "-m", "a committed target"]);
    assert!(!is_regenerable(&s.path().join("target/fixture.bin")));

    let mut p = GitCli::default();
    let f = find(
        &mut p,
        &s.path().join("target/fixture.bin").to_string_lossy(),
        "write",
        false,
    );
    assert_eq!(recovery_of(&f), Recovery::TrackedClean);
}

#[test]
fn the_table_matches_a_whole_component_and_not_a_prefix() {
    // `src/target_notes.txt` is not a build path, and neither is
    // `targets/one.json`: a substring is not a component.
    let s = a_repo("component");
    write(&s.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
    write(&s.path().join("src/target_notes.txt"), "notes\n");
    write(&s.path().join("targets/one.json"), "{}\n");
    assert!(!is_regenerable(&s.path().join("src/target_notes.txt")));
    assert!(!is_regenerable(&s.path().join("targets/one.json")));
}

#[test]
fn a_path_under_no_repository_is_never_regenerable() {
    // Outside every repository nothing ignores anything, so the second
    // condition cannot be met and the word is not given.
    let s = Scratch::new("outside");
    write(&s.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
    write(&s.path().join("target/debug/app"), "a build output\n");
    assert!(!is_regenerable(&s.path().join("target/debug/app")));
}

#[test]
fn the_table_is_closed_and_every_line_says_what_recreates_it() {
    // A "zero X" bar is met by an empty set, so assert the positive count
    // too (the builders' brief): three lines, and each carries its `by`.
    assert_eq!(TABLE.len(), 3, "the table is closed: three lines");
    let names: Vec<&str> = TABLE.iter().map(|b| b.dir).collect();
    assert_eq!(names, vec!["target", "node_modules", "__pycache__"]);
    for b in TABLE {
        assert!(
            !b.by.trim().is_empty(),
            "{} must say what recreates it",
            b.dir
        );
    }
    // And the names a reader might expect to be here are deliberately not.
    for absent in ["dist", "build", "out", ".venv", "vendor", "node-gyp"] {
        assert!(
            !names.contains(&absent),
            "{absent} is not in the table: growing it is a decision with a reviewer"
        );
    }
}
