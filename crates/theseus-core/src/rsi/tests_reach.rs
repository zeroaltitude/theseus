//! What a change can reach (issue 20): the workspace read from its manifests, and the packages a
//! diff reaches, conservatively.

use std::path::Path;

use super::reach::{outside, read, Graph};

fn write(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// `base` is the bottom; `mid` depends on it and `top` on `mid` (so `top` only through `mid`);
/// `side` has a dev-dependency on `base` under another name and a path; `lonely` depends on none;
/// `inherit` takes `base` through `[workspace.dependencies]`; `tgt` takes it under a `[target]`.
fn workspace() -> (tempfile::TempDir, Graph) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(
        r,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\", \"tools/odd\"]\n\n[workspace.dependencies]\nbase = { path = \"crates/base\" }\n",
    );
    let pkg = |name: &str, deps: &str| {
        format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n{deps}")
    };
    write(r, "crates/base/Cargo.toml", &pkg("base", ""));
    write(
        r,
        "crates/mid/Cargo.toml",
        &pkg("mid", "[dependencies]\nbase = { path = \"../base\" }\n"),
    );
    write(
        r,
        "crates/top/Cargo.toml",
        &pkg("top", "[dependencies]\nmid = \"1\"\n"),
    );
    write(
        r,
        "crates/side/Cargo.toml",
        &pkg(
            "side",
            "[dev-dependencies]\nunder-another-name = { path = \"../base\", package = \"base\" }\n",
        ),
    );
    write(
        r,
        "crates/lonely/Cargo.toml",
        &pkg("lonely", "[dependencies]\nserde = \"1\"\n"),
    );
    write(
        r,
        "crates/inherit/Cargo.toml",
        &pkg(
            "inherit",
            "[build-dependencies]\nbase = { workspace = true }\n",
        ),
    );
    write(
        r,
        "crates/tgt/Cargo.toml",
        &pkg(
            "tgt",
            "[target.'cfg(unix)'.dependencies]\nbase = { path = \"../base\" }\n",
        ),
    );
    write(r, "tools/odd/Cargo.toml", &pkg("odd", ""));
    let g = read(r).unwrap();
    (dir, g)
}

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

/// KEY: a package is inside when the diff edits it or anything it depends on, through any chain
/// and in any table, however the dependency is spelled; and outside otherwise.
#[test]
fn the_packages_a_diff_reaches_are_those_it_edits_and_everything_that_depends_on_them() {
    let (_dir, g) = workspace();
    let edited_base = paths(&["crates/base/src/lib.rs"]);
    for dependent in ["base", "mid", "top", "side", "inherit", "tgt"] {
        assert!(
            !outside(&g, &edited_base, dependent),
            "{dependent} depends on base"
        );
    }
    for apart in ["lonely", "odd"] {
        assert!(outside(&g, &edited_base, apart), "{apart} does not");
    }
    // Editing the top reaches nothing below it.
    let edited_top = paths(&["crates/top/tests/t.rs"]);
    assert!(!outside(&g, &edited_top, "top"));
    for below in ["base", "mid", "lonely"] {
        assert!(outside(&g, &edited_top, below), "{below}");
    }
}

/// When in doubt it is inside: a path in no package reaches everything, and a package the
/// workspace does not have is never outside.
#[test]
fn a_path_in_no_package_reaches_everything_and_an_unknown_package_is_inside() {
    let (_dir, g) = workspace();
    for path in [
        "Cargo.lock",
        "Cargo.toml",
        "docs/status.md",
        "scripts/gate.sh",
        ".config/nextest.toml",
    ] {
        for package in ["base", "lonely", "odd"] {
            assert!(
                !outside(&g, &paths(&["crates/lonely/src/lib.rs", path]), package),
                "{path} reaches {package}"
            );
        }
    }
    assert!(!outside(
        &g,
        &paths(&["crates/lonely/src/lib.rs"]),
        "not-in-the-workspace"
    ));
    // A directory whose name only starts like a package's is not in it.
    assert!(!outside(
        &g,
        &paths(&["crates/lonely-extra/x.rs"]),
        "lonely"
    ));
}

/// Anything the read cannot place is an error, so the caller treats the failure as inside.
#[test]
fn a_workspace_that_cannot_be_read_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    assert!(read(r).is_err(), "no manifest");
    write(
        r,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/{a,b}\"]\n",
    );
    assert!(read(r).is_err(), "a glob it does not read");
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"../outside\"]\n");
    assert!(read(r).is_err(), "a member that leaves the tree");
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"crates/a\"]\n");
    write(r, "crates/a/Cargo.toml", "not toml at all [");
    assert!(read(r).is_err(), "a manifest that does not parse");
}

/// A manifest that is a link is not followed out of the tree.
#[test]
fn a_manifest_that_is_a_link_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"crates/a\"]\n");
    write(
        r,
        "elsewhere.toml",
        "[package]\nname = \"a\"\nversion = \"1\"\n",
    );
    std::fs::create_dir_all(r.join("crates/a")).unwrap();
    std::os::unix::fs::symlink(r.join("elsewhere.toml"), r.join("crates/a/Cargo.toml")).unwrap();
    assert!(read(r).is_err());
}
