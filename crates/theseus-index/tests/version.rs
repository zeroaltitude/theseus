//! `theseus-index --version` (theseus-t7ra): every shipped binary answers it,
//! as `theseusd`, `theseus`, `theseus-tui`, and `theseus-sim` do, so a check by
//! hand of what an install holds asks each the same way.

use std::process::Command;

#[test]
fn version_prints_the_binarys_name_and_the_workspaces_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_theseus-index"))
        .arg("--version")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "exit {:?}: {stdout}{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout.trim_end(),
        format!("theseus-index {}", env!("CARGO_PKG_VERSION"))
    );
}
