//! A test core's directory goes with the process that made it (theseus-5hr6i).

use std::process::Command;

use super::tests::test_core;
use crate::scratch::PREFIX;

const CHILD: &str = "THESEUS_SCRATCH_CHILD";

/// The child's half: a core made and leaked, as a test hands its core to a
/// task that never ends. It does nothing outside the parent's run.
#[test]
fn a_leaked_test_core_for_the_scratch_parent() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let core = test_core("hello");
    println!("SCRATCH_DIR={}", core.cfg.server.state_dir);
    std::mem::forget(core);
}

#[test]
fn a_test_core_leaves_no_dir_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "rpc::tests_scratch::a_leaked_test_core_for_the_scratch_parent",
        ])
        .args(["--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("TMPDIR", tmp.path())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let made = said
        .split_once("SCRATCH_DIR=")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("the child made no core: {said}"));
    let made = std::path::Path::new(made);
    assert!(
        made.starts_with(tmp.path()),
        "{made:?} is not under {:?}",
        tmp.path()
    );
    assert!(made
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(PREFIX));
    let left: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().flatten().collect();
    assert!(left.is_empty(), "the child left {left:?}");
}
