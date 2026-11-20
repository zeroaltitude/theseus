//! The commit this program is built from, as `THESEUS_COMMIT` (the same script as `theseusd`'s): `theseus-sim stamp`
//! prints it, so a test that spawns the simulator can tell a stale copy from a current one (issue 28).
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
    // A file in `OUT_DIR` and not `rustc-env`: an environment value is part of every target's build, so the
    // library would recompile at each commit as the program does; a file only the program includes does not.
    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    std::fs::write(Path::new(&out).join("commit"), commit.unwrap_or_default())
        .expect("writing the commit");
}
