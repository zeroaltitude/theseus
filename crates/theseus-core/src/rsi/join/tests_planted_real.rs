//! The planted revert through the REAL runner (`planted::Cargo`, which runs `cargo nextest`) on a
//! tiny real crate in a throwaway repository: the exit codes the step reads are the ones nextest
//! gives, not the ones a stand-in says. Three changes: with the fix undone, a test that fails on its
//! assertion (100, `failed`) and a test that only stops compiling (101, `build-broke`); and tests
//! that are not in the build at all, which the first run finds (4, `not-found`).
//!
//! Host-dependent: it needs `cargo nextest`, and skips with a plain reason where there is none.
//! It runs the host path of the runner (a test place has no jail); the jail's path hands back the
//! exit code the same way (`Jail::run_blocking`, whose exit-127 case `theseusd`'s `self_build`
//! tests pin).

use std::process::{Command, Stdio};

use super::planted::{self, Cargo, Outcome};
use super::tests_planted::commits;
use crate::rsi::tests_tree::fixture;

const MANIFEST: &str = "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

fn nextest_here() -> bool {
    Command::new("cargo")
        .args(["nextest", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The planted revert of a change of the tiny crate, from `base`'s files to `head`'s, run for
/// real; none when there is no `cargo nextest` here.
fn proved(base: &[(&str, &str)], head: &[(&str, &str)]) -> Option<Outcome> {
    if !nextest_here() {
        eprintln!("skipped: `cargo nextest` is not installed on this host");
        return None;
    }
    let f = fixture();
    let (b, h) = commits(&f, base, head);
    Some(planted::run(&f.place, &Cargo, "self/x", &b, &h).unwrap())
}

const BASE: &[(&str, &str)] = &[
    ("Cargo.toml", MANIFEST),
    ("src/lib.rs", "pub fn f() -> u32 {\n    1\n}\n"),
];

#[test]
fn a_test_that_fails_on_its_assertion_without_the_fix_is_failed() {
    let head = [
        (
            "src/lib.rs",
            "pub fn f() -> u32 {\n    2\n}\n#[cfg(test)]\nmod tests_f;\n",
        ),
        (
            "src/tests_f.rs",
            "use super::f;\n#[test]\nfn f_is_two() {\n    assert_eq!(f(), 2);\n}\n",
        ),
    ];
    let Some(o) = proved(BASE, &head) else { return };
    assert_eq!(o.name(), "failed", "{o:?}");
    assert!(o.proved());
}

#[test]
fn a_test_that_only_stops_compiling_without_the_fix_is_build_broke() {
    let head = [
        (
            "src/lib.rs",
            "pub fn f() -> u32 {\n    1\n}\npub fn g() -> u32 {\n    3\n}\n#[cfg(test)]\nmod tests_g;\n",
        ),
        (
            "src/tests_g.rs",
            "use super::g;\n#[test]\nfn g_is_three() {\n    assert_eq!(g(), 3);\n}\n",
        ),
    ];
    let Some(o) = proved(BASE, &head) else { return };
    assert_eq!(o.name(), "build-broke", "{o:?}");
    assert!(o.proved());
}

#[test]
fn tests_that_are_not_in_the_build_are_not_found() {
    let head = [(
        "src/tests_h.rs",
        "#[test]\nfn h_runs() {\n    assert_eq!(1, 1);\n}\n",
    )];
    let Some(o) = proved(BASE, &head) else { return };
    assert_eq!(o.name(), "not-found", "{o:?}");
    assert!(!o.proved());
}
