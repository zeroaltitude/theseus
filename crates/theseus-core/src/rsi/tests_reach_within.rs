//! What a change can reach inside one package (issue 20): a failed unit test is outside the
//! change unless the change edits its own module or what it is named for, or anything that is
//! not plain source, or something it depends on.

use std::path::Path;

use super::reach::{apart, outside, read, Graph};

fn write(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

/// `core` is the one big package: a library of modules, as files and as directories. `app` depends
/// on it, `util` is below it, `custom` roots its library elsewhere.
fn workspace() -> (tempfile::TempDir, Graph) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    let pkg = |name: &str, more: &str| {
        format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n{more}")
    };
    write(r, "crates/util/Cargo.toml", &pkg("util", ""));
    write(
        r,
        "crates/core/Cargo.toml",
        &pkg("core", "[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    write(
        r,
        "crates/app/Cargo.toml",
        &pkg("app", "[dependencies]\ncore = { path = \"../core\" }\n"),
    );
    write(
        r,
        "crates/custom/Cargo.toml",
        &pkg("custom", "[lib]\npath = \"src/root.rs\"\n"),
    );
    for f in [
        "crates/core/src/lib.rs",
        "crates/core/src/term/mod.rs",
        "crates/core/src/term/pty.rs",
        "crates/core/src/term/tests.rs",
        "crates/core/src/keys.rs",
        "crates/core/src/tests_keys.rs",
        "crates/core/src/tests_config_words.rs",
        "crates/core/src/config/mod.rs",
        "crates/core/src/config/words.rs",
        "crates/core/src/config/tests_answer.rs",
        "crates/core/src/rpc/mcp_card.rs",
        "crates/custom/src/root.rs",
        "crates/custom/src/term.rs",
    ] {
        write(r, f, "\n");
    }
    let g = read(r).unwrap();
    (dir, g)
}

const TERM_TEST: &str = "term::tests::cat_echoes_and_ctrl_c_ends_it";

/// KEY: the case of the second self join: a change to three files of the one package (a config
/// module, an rpc module and a config test file) and a terminal test of that package that fails.
/// The package is reached, but the terminal's module is not what the change edits.
#[test]
fn a_failed_test_in_a_module_the_change_does_not_edit_is_apart() {
    let (d, g) = workspace();
    let change = paths(&[
        "crates/core/src/config/words.rs",
        "crates/core/src/rpc/mcp_card.rs",
        "crates/core/src/config/tests_answer.rs",
    ]);
    assert!(!outside(&g, &change, "core"), "by package it is inside");
    assert!(apart(&g, &change, "core", TERM_TEST, d.path()));
    // A test of a module that is a single file, too.
    assert!(apart(&g, &change, "core", "keys::tests::a_key", d.path()));
}

/// KEY: what the change edits is what it can have broken: the test's own module (its file or its
/// whole subtree, whichever file of it) and the modules it is named for or by.
#[test]
fn a_failed_test_in_the_module_the_change_edits_is_not_apart() {
    let (d, g) = workspace();
    for edited in [
        "crates/core/src/term/pty.rs",
        "crates/core/src/term/tests.rs",
        "crates/core/src/term/mod.rs",
    ] {
        let change = paths(&["crates/core/src/config/words.rs", edited]);
        assert!(!apart(&g, &change, "core", TERM_TEST, d.path()), "{edited}");
    }
    // A module that is one file: the file is the module.
    let change = paths(&["crates/core/src/keys.rs"]);
    assert!(!apart(&g, &change, "core", "keys::tests::a_key", d.path()));
    // By name: `tests_keys` tests `keys`, and the other way about; `tests_config_words` goes with
    // `config` (a directory) through `config_words`'s first word.
    let change = paths(&["crates/core/src/keys.rs"]);
    assert!(!apart(&g, &change, "core", "tests_keys::a_key", d.path()));
    let change = paths(&["crates/core/src/tests_keys.rs"]);
    assert!(!apart(&g, &change, "core", "keys::tests::a_key", d.path()));
    let change = paths(&["crates/core/src/config/words.rs"]);
    assert!(!apart(
        &g,
        &change,
        "core",
        "tests_config_words::x",
        d.path()
    ));
}

/// KEY: a change that is not plain source in the package reaches all of it: a manifest, a build
/// script, a fixture, a file of the crate's own `tests/` or its root, or a source file that is
/// not Rust. And so does one outside every package.
#[test]
fn a_change_that_is_not_plain_source_reaches_the_whole_package() {
    let (d, g) = workspace();
    for other in [
        "crates/core/Cargo.toml",
        "crates/core/build.rs",
        "crates/core/tests/x.rs",
        "crates/core/src/config/golden.txt",
        "crates/core/src/lib.rs",
        "crates/core/src/main.rs",
        "Cargo.lock",
        "scripts/gate.sh",
        ".config/nextest.toml",
        "docs/status.md",
    ] {
        let change = paths(&["crates/core/src/config/words.rs", other]);
        assert!(!apart(&g, &change, "core", TERM_TEST, d.path()), "{other}");
    }
}

/// KEY: a package the change edits something below is inside, and a change in the package above
/// does not reach the one below it.
#[test]
fn a_package_over_an_edited_package_is_judged_by_package() {
    let (d, g) = workspace();
    let below = paths(&["crates/util/src/lib.rs"]);
    assert!(!apart(&g, &below, "core", TERM_TEST, d.path()));
    let both = paths(&["crates/util/src/lib.rs", "crates/core/src/config/words.rs"]);
    assert!(!apart(&g, &both, "core", TERM_TEST, d.path()));
    // `app` depends on `core` and `core` is edited: inside, as before.
    let core = paths(&["crates/core/src/config/words.rs"]);
    assert!(!apart(&g, &core, "app", "x::tests::y", d.path()));
    // A package apart is apart, as before.
    assert!(apart(&g, &core, "custom", "term::tests::y", d.path()));
    assert!(apart(
        &g,
        &paths(&["crates/app/src/lib.rs"]),
        "util",
        "x::y",
        d.path()
    ));
}

/// A test that cannot be placed is inside: another test binary of the package, a test of the
/// crate root, a module with no file, a name that is no module, a library rooted elsewhere, a
/// package the workspace does not have.
#[test]
fn a_test_that_cannot_be_placed_is_not_apart() {
    let (d, g) = workspace();
    let change = paths(&["crates/core/src/config/words.rs"]);
    for (binary, test) in [
        ("core::integration", "a_test"),
        ("core", "a_root_test"),
        ("core", "inline::tests::x"),
        ("core", "../term::x"),
        ("core", "te rm::x"),
        ("core", "::x"),
        ("core::bin/core", "term::tests::x"),
        ("nobody", "term::tests::x"),
    ] {
        assert!(
            !apart(&g, &change, binary, test, d.path()),
            "{binary} {test}"
        );
    }
    // `custom` roots its library at `src/root.rs`: the modules' places are not read.
    let change = paths(&["crates/custom/src/other.rs"]);
    assert!(!apart(&g, &change, "custom", "term::tests::y", d.path()));
}

/// A module that is a link is not a module the tree holds.
#[test]
fn a_module_that_is_a_link_is_not_placed() {
    let (d, g) = workspace();
    let at = d.path().join("crates/core/src");
    std::fs::create_dir_all(d.path().join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(d.path().join("elsewhere"), at.join("linked")).unwrap();
    let change = paths(&["crates/core/src/config/words.rs"]);
    assert!(!apart(&g, &change, "core", "linked::tests::x", d.path()));
}
