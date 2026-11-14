//! `rsi::tree_fmt`: a tree is formatted before the gate, so an unformatted change is
//! never an attempt. Over a real cargo package in a scratch directory: rustfmt itself is
//! the subject, never a stand-in.

use std::path::Path;

use super::tests_tree::fixture;
use super::tree::Tree;
use super::tree_fmt::{format_tree, Formatted};

const UNFORMATTED: &str = "pub fn sail( a:u32,b:u32 )->u32{a+b}\n";
const FORMATTED: &str = "pub fn sail(a: u32, b: u32) -> u32 {\n    a + b\n}\n";

/// A one-crate package at `dir`, holding `source` as its library.
fn package(dir: &Path, source: &str) -> Tree {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"mast\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), source).unwrap();
    Tree {
        slug: "mast".into(),
        branch: "self/20261114-mast".into(),
        dir: dir.to_path_buf(),
        target: dir.join("target"),
    }
}

/// KEY: unformatted source is rewritten before the gate, and the files are named.
#[test]
fn an_unformatted_change_is_formatted_before_the_gate() {
    let f = fixture();
    let scratch = crate::scratch::tempdir();
    let t = package(scratch.path(), UNFORMATTED);
    let done = format_tree(&f.place, &t);
    assert_eq!(
        done,
        Formatted::Fixed(vec!["src/lib.rs".into()]),
        "{done:?}"
    );
    assert_eq!(
        std::fs::read_to_string(scratch.path().join("src/lib.rs")).unwrap(),
        FORMATTED
    );
    // The commit message says so, in plain words.
    assert_eq!(
        done.line().as_deref(),
        Some("Formatted before the gate: src/lib.rs.")
    );
}

/// Already formatted source is left byte for byte, and says nothing.
#[test]
fn formatted_source_is_left_alone() {
    let f = fixture();
    let scratch = crate::scratch::tempdir();
    let t = package(scratch.path(), FORMATTED);
    let before = std::fs::metadata(scratch.path().join("src/lib.rs")).unwrap();
    let done = format_tree(&f.place, &t);
    assert_eq!(done, Formatted::Clean);
    assert!(done.line().is_none());
    let after = std::fs::metadata(scratch.path().join("src/lib.rs")).unwrap();
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
}

/// A tree rustfmt cannot read is not formatted, and not an error: the gate judges it as it
/// is. The attempt must go on, so this returns a value and never panics or fails.
#[test]
fn a_tree_rustfmt_cannot_read_is_left_for_the_gate() {
    let f = fixture();
    let scratch = crate::scratch::tempdir();
    let t = package(scratch.path(), "pub fn broken( {\n");
    match format_tree(&f.place, &t) {
        Formatted::Unavailable(why) => assert!(why.contains("cargo fmt"), "{why}"),
        other => panic!("a source that does not parse is not formatted: {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(scratch.path().join("src/lib.rs")).unwrap(),
        "pub fn broken( {\n"
    );
}

/// No cargo on the machine: the same, said in words.
#[test]
fn a_machine_with_no_cargo_leaves_the_tree_for_the_gate() {
    let mut f = fixture();
    let nowhere = crate::scratch::tempdir();
    f.place.env.push(("PATH".into(), nowhere.path().into()));
    let scratch = crate::scratch::tempdir();
    let t = package(scratch.path(), UNFORMATTED);
    match format_tree(&f.place, &t) {
        Formatted::Unavailable(why) => assert!(why.contains("cargo could not start"), "{why}"),
        other => panic!("no cargo means no formatting: {other:?}"),
    }
}
