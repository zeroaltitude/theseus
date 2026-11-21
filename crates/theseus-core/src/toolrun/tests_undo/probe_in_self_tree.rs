//! The undo probe starts no git in the self loop's own directory (the git-pin review, B1), through
//! a whole core. A session's call that names a file has the daemon ask, at L0 and before any gate
//! has spoken, what the file's repository holds of it; the git program answered by `diff`, which
//! hashes the file through the clean filter the repository's own config names. In a self tree that
//! config is whatever the branch's own code, run by the gate in L1 with the tree writable, made
//! it: a `.git` pointed at a git directory of its own, or a repository left inside the tree.
//!
//! Each case asserts the probe ran (Jev was asked, and the probe runs before the ask is built) and
//! that the marker, a `touch` in the test's own temp dir, was never made. Planted: the probe's
//! `no_git_under` set to none in `toolrun/reversible.rs`, both went red.

use super::self_own::{drive_says, tree};
use super::*;

/// A filter's shape that touches `RAN-<name>` in `base`: the marker's path, and the script's.
fn marker(base: &Path, name: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let mark = base.join(format!("RAN-{name}"));
    let script = base.join(format!("{name}.sh"));
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\nexec cat\n", mark.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (mark, script)
}

/// `dir` made a repository holding `file`, committed, with `filter.x.clean` the script and a
/// `.gitattributes` that selects it; then `file` written again after a second, so git would hash
/// it through the filter.
fn filtered(dir: &Path, file: &str, script: &Path) {
    let git = crate::rsi::tests_tree::git;
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", "one"]);
    git(dir, &["config", "filter.x.clean", script.to_str().unwrap()]);
    std::fs::write(dir.join(".gitattributes"), "* filter=x\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(dir.join(file), WAS).unwrap();
}

/// The self tree `sails`, from the rig's target.
fn sails(t: &Path) -> PathBuf {
    tree(t, "sails")
}

/// KEY: the tree's `.git` points at a git directory of the branch's own, whose config names the
/// filter, and the builder reads a file of its tree.
#[tokio::test]
async fn a_read_in_a_tree_whose_git_was_pointed_elsewhere_runs_nothing_it_names() {
    let mut mark = PathBuf::new();
    let read = |t: &Path| json!({"path": sails(t).join("notes.txt").to_string_lossy()});
    let (r, sid) = drive_says(REVERSIBLE, "self", "fs_read", read, |c| {
        let base = Path::new(&c.server.state_dir)
            .parent()
            .unwrap()
            .to_path_buf();
        let t = sails(&base.join("work/notes.txt"));
        let script;
        (mark, script) = marker(&base, "pointed");
        // A repository of the branch's own, standing in for the git directory it made.
        std::fs::remove_dir_all(t.join(".git")).unwrap();
        filtered(&t, "notes.txt", &script);
        let evil = base.join("evil");
        std::fs::rename(t.join(".git"), &evil).unwrap();
        std::fs::write(t.join(".git"), format!("gitdir: {}\n", evil.display())).unwrap();
    });
    r.turn(&sid).await;
    assert!(
        !r.jev.seen().is_empty(),
        "the probe did not run, so this shows nothing"
    );
    assert!(
        !mark.exists(),
        "the undo probe ran a program the tree's .git named"
    );
}

/// KEY: a repository left inside the tree, with the filter in its own config, and the builder
/// reads a file of it.
#[tokio::test]
async fn a_read_in_a_repository_left_inside_a_tree_runs_nothing_it_names() {
    let mut mark = PathBuf::new();
    let nested = |t: &Path| sails(t).join("vendor/sub");
    let read = move |t: &Path| json!({"path": nested(t).join("x.txt").to_string_lossy()});
    let (r, sid) = drive_says(REVERSIBLE, "self", "fs_read", read, |c| {
        let base = Path::new(&c.server.state_dir)
            .parent()
            .unwrap()
            .to_path_buf();
        let sub = nested(&base.join("work/notes.txt"));
        let script;
        (mark, script) = marker(&base, "nested");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("x.txt"), WAS).unwrap();
        filtered(&sub, "x.txt", &script);
    });
    r.turn(&sid).await;
    assert!(
        !r.jev.seen().is_empty(),
        "the probe did not run, so this shows nothing"
    );
    assert!(
        !mark.exists(),
        "the undo probe ran a program a nested repository named"
    );
}
