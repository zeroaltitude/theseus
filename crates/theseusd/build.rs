//! The commit this binary is built from (theseus-9o5n), as `THESEUS_COMMIT`:
//! a constant in the binary, so a start computes nothing to name its build.
//!
//! `THESEUS_COMMIT` in the build's own environment wins (a build outside a git
//! checkout can name its commit); otherwise git's `HEAD`, or nothing at all
//! when there is no git. The script runs again when `HEAD` moves: a commit, a
//! checkout, or a reset changes `HEAD` or the branch it names.

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=THESEUS_COMMIT");
    let commit = std::env::var("THESEUS_COMMIT")
        .ok()
        .filter(|c| !c.trim().is_empty())
        .map(|c| c.trim().to_string())
        .or_else(|| git(&["rev-parse", "HEAD"]));
    // What moves HEAD: HEAD itself, the branch it names (loose, or packed),
    // each where git keeps it (a worktree keeps its HEAD apart from the
    // refs). A path that does not exist is not watched: cargo would run the
    // script on every build.
    let mut watched = vec!["HEAD".to_string(), "packed-refs".to_string()];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in &watched {
        if let Some(p) = git(&["rev-parse", "--path-format=absolute", "--git-path", name]) {
            if Path::new(&p).exists() {
                println!("cargo:rerun-if-changed={p}");
            }
        }
    }
    println!(
        "cargo:rustc-env=THESEUS_COMMIT={}",
        commit.unwrap_or_default()
    );
}
