//! `rsi::tree_fmt`: a tree is formatted before the gate, so an unformatted change is never an
//! attempt. Over a real worktree and the real `rustfmt`: it is the subject, never a stand-in.

use super::tests_tree::{fixture, Fixture};
use super::tree::{self, Tree};
use super::tree_fmt::{format_tree, Formatted};

const UNFORMATTED: &str = "pub fn sail( a:u32,b:u32 )->u32{a+b}\n";
const FORMATTED: &str = "pub fn sail(a: u32, b: u32) -> u32 {\n    a + b\n}\n";
const NOW: u64 = 1_792_000_000_000;

/// A tree, with a one-crate package written into it holding `source` as its library (new files,
/// as a model's edit leaves them).
fn package(f: &Fixture, source: &str) -> Tree {
    let t = tree::open(&f.place, "mast", NOW).unwrap();
    std::fs::create_dir_all(t.dir.join("src")).unwrap();
    std::fs::write(
        t.dir.join("Cargo.toml"),
        "[package]\nname = \"mast\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(t.dir.join("src/lib.rs"), source).unwrap();
    t
}

fn read(t: &Tree) -> String {
    std::fs::read_to_string(t.dir.join("src/lib.rs")).unwrap()
}

/// KEY: unformatted source is rewritten before the gate, and the files are named.
#[test]
fn an_unformatted_change_is_formatted_before_the_gate() {
    let f = fixture();
    let t = package(&f, UNFORMATTED);
    let done = format_tree(&f.place, &t);
    assert_eq!(
        done,
        Formatted::Fixed(vec!["src/lib.rs".into()]),
        "{done:?}"
    );
    assert_eq!(read(&t), FORMATTED);
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
    let t = package(&f, FORMATTED);
    let before = std::fs::metadata(t.dir.join("src/lib.rs")).unwrap();
    let done = format_tree(&f.place, &t);
    assert_eq!(done, Formatted::Clean);
    assert!(done.line().is_none());
    let after = std::fs::metadata(t.dir.join("src/lib.rs")).unwrap();
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
}

/// A tree with no Rust change has nothing to format, and runs nothing.
#[test]
fn a_tree_with_no_rust_change_is_clean() {
    let f = fixture();
    let t = tree::open(&f.place, "mast", NOW).unwrap();
    std::fs::write(t.dir.join("notes.txt"), "no rust here\n").unwrap();
    assert_eq!(format_tree(&f.place, &t), Formatted::Clean);
}

/// A tree rustfmt cannot read is not formatted, and not an error: the gate judges it as it
/// is. The attempt must go on, so this returns a value and never panics or fails.
#[test]
fn a_tree_rustfmt_cannot_read_is_left_for_the_gate() {
    let f = fixture();
    let t = package(&f, "pub fn broken( {\n");
    match format_tree(&f.place, &t) {
        Formatted::Unavailable(why) => assert!(why.contains("rustfmt"), "{why}"),
        other => panic!("a source that does not parse is not formatted: {other:?}"),
    }
    assert_eq!(read(&t), "pub fn broken( {\n");
}

/// No rustfmt on the machine: the same, said in words.
#[test]
fn a_machine_with_no_rustfmt_leaves_the_tree_for_the_gate() {
    let mut f = fixture();
    // The tree is opened with git on the path; only then is the path taken away.
    let t = package(&f, UNFORMATTED);
    // A path with git (the changed files are listed with it) and no rustfmt.
    let only_git = crate::scratch::tempdir();
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("git"))
        .find(|g| g.is_file())
        .expect("git is on the path");
    std::os::unix::fs::symlink(git, only_git.path().join("git")).unwrap();
    f.place.env.push(("PATH".into(), only_git.path().into()));
    match format_tree(&f.place, &t) {
        Formatted::Unavailable(why) => assert!(why.contains("rustfmt could not start"), "{why}"),
        other => panic!("no rustfmt means no formatting: {other:?}"),
    }
}

/// KEY (the first review's F1): what the tree says about cargo is not obeyed. A tree that plants an
/// `[alias]` for `cargo fmt` and a `rust-toolchain.toml` naming a toolchain that is not there is
/// formatted all the same, and the alias runs nothing: the old `cargo fmt` call failed here, and
/// with a harmless alias that built the tree's own code it would have run it.
#[test]
fn what_the_tree_says_about_cargo_is_not_obeyed() {
    let f = fixture();
    let t = package(&f, UNFORMATTED);
    let marker = t.dir.join("ran-at-l0");
    std::fs::create_dir_all(t.dir.join(".cargo")).unwrap();
    std::fs::write(
        t.dir.join(".cargo/config.toml"),
        format!(
            "[alias]\nfmt = \"run --quiet --bin touch-it\"\n# {}\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::write(
        t.dir.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"no-such-toolchain-9.9\"\n",
    )
    .unwrap();
    let done = format_tree(&f.place, &t);
    assert_eq!(
        done,
        Formatted::Fixed(vec!["src/lib.rs".into()]),
        "{done:?}"
    );
    assert_eq!(read(&t), FORMATTED);
    assert!(!marker.exists(), "nothing of the tree's ran");
}

/// KEY (the second review's R1): `rustfmt` given a path follows `#[path = "…"] mod m;` and writes
/// through a symlink, so a tree could have it rewrite a file outside it and name that file in the
/// commit. Both are planted here; the outside file is byte for byte unchanged, the tree's own file
/// is formatted, and the commit line holds no path outside the tree.
#[test]
fn a_module_path_or_a_symlink_cannot_reach_a_file_outside_the_tree() {
    let f = fixture();
    let outside = crate::scratch::tempdir();
    let victim = outside.path().join("victim.rs");
    std::fs::write(&victim, UNFORMATTED).unwrap();
    let t = package(
        &f,
        &format!(
            "#[path = \"{}\"]\nmod m;\npub fn sail( a:u32 )->u32{{a}}\n",
            victim.display()
        ),
    );
    std::os::unix::fs::symlink(&victim, t.dir.join("link.rs")).unwrap();
    let done = format_tree(&f.place, &t);
    assert_eq!(
        done,
        Formatted::Fixed(vec!["src/lib.rs".into()]),
        "{done:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        UNFORMATTED,
        "the file outside the tree was not touched"
    );
    let line = done.line().unwrap();
    assert!(!line.contains(outside.path().to_str().unwrap()), "{line}");
    assert!(std::fs::read_to_string(t.dir.join("src/lib.rs"))
        .unwrap()
        .contains("pub fn sail(a: u32) -> u32 {"));
}

/// The tree's own `rustfmt.toml` is read, as `cargo fmt` reads it: a tree whose config asks for a
/// narrower line is formatted to it.
#[test]
fn the_trees_own_rustfmt_config_is_read() {
    let f = fixture();
    let t = package(&f, "pub fn sail(first_argument: u32, second_argument: u32) -> u32 { first_argument + second_argument }\n");
    std::fs::write(t.dir.join("rustfmt.toml"), "max_width = 40\n").unwrap();
    let done = format_tree(&f.place, &t);
    assert_eq!(
        done,
        Formatted::Fixed(vec!["src/lib.rs".into()]),
        "{done:?}"
    );
    let formatted = read(&t);
    assert!(
        formatted.lines().all(|l| l.len() <= 40),
        "max_width = 40 held: {formatted}"
    );
}
