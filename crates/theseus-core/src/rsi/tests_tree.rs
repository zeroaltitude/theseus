//! `rsi::tree` (theseus-pw1q.8) over a throwaway repository: an upstream
//! (bare, the remote) with a tiny stand-in `scripts/gate.sh` on main, and a
//! clone of it as the repo. The stand-in fails `broken` trees, prints the
//! keel's words for `keel` trees, and records the tree and build tree it ran
//! for. Every git call is hermetic: no global or system config is read.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::tree::{self, Place, Refused, Verdict};
use crate::scrub::Scrubber;

/// main's gate: judges the tree it is given, never its own directory.
const GATE: &str = r#"#!/bin/bash
cd "${THESEUS_GATE_ROOT:-$(dirname "$0")/..}"
echo "stand-in gate: root $PWD target $CARGO_TARGET_DIR nobench $THESEUS_GATE_NO_BENCH session ${THESEUS_SESSION:-none}"
if [ -f keel.txt ]; then echo "gate: FAILED in keel"; exit 1; fi
if [ -f broken.txt ]; then echo "gate: FAILED in suite"; exit 1; fi
echo "gate: ok"
"#;

/// A planted weakened gate: what a tree could write over its own copy.
const WEAK: &str = "#!/bin/bash\necho 'gate: ok'\nexit 0\n";

pub(super) fn hermetic() -> Vec<(OsString, OsString)> {
    [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "Fixture"),
        ("GIT_AUTHOR_EMAIL", "fixture@example.invalid"),
        ("GIT_COMMITTER_NAME", "Fixture"),
        ("GIT_COMMITTER_EMAIL", "fixture@example.invalid"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(hermetic())
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// The upstream, the repo cloned from it, and a state dir apart from both.
pub(super) struct Fixture {
    pub(super) place: Place,
    pub(super) upstream: PathBuf,
    _dir: tempfile::TempDir,
}

pub(super) fn fixture() -> Fixture {
    fixture_with(GATE)
}

pub(super) fn fixture_with(gate: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("scripts")).unwrap();
    std::fs::write(seed.join("scripts/gate.sh"), gate).unwrap();
    std::fs::write(seed.join("README.md"), "the ship\n").unwrap();
    std::fs::write(seed.join(".gitignore"), "target\n").unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-q", "-m", "seed"]);
    let upstream = root.join("upstream.git");
    git(
        &root,
        &["clone", "-q", "--bare", "seed", upstream.to_str().unwrap()],
    );
    git(&root, &["clone", "-q", "upstream.git", "repo"]);
    let state = root.join("state");
    std::fs::create_dir_all(&state).unwrap();
    Fixture {
        place: Place {
            repo: root.join("repo"),
            state,
            env: hermetic(),
        },
        upstream,
        _dir: dir,
    }
}

const NOW: u64 = 1_792_000_000_000;

/// The tree is outside the repo's own checkout, on a new self branch off
/// origin/main, with a build tree of its own under the state dir; and the
/// repo's own work tree is untouched.
#[test]
fn a_tree_is_apart_from_the_repo_with_its_own_target() {
    let f = fixture();
    let t = tree::open(&f.place, "Fix the mast!", NOW).unwrap();
    let repo = f.place.repo.canonicalize().unwrap();
    assert_eq!(t.slug, "fix-the-mast");
    assert!(t.branch.starts_with("self/") && t.branch.ends_with("-fix-the-mast"));
    assert!(!t.dir.starts_with(&repo), "{t:?} is inside the repo");
    assert_eq!(
        t.dir,
        f.place.trees().canonicalize().unwrap().join("fix-the-mast")
    );
    assert_eq!(t.target, f.place.targets().join("fix-the-mast"));
    assert!(t.target.is_dir() && !t.target.starts_with(&repo) && !t.target.starts_with(&t.dir));
    assert_eq!(
        git(&t.dir, &["rev-parse", "HEAD"]),
        git(&repo, &["rev-parse", "origin/main"])
    );
    assert_eq!(git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
    assert_eq!(tree::find(&f.place, "Fix the mast!"), Some(t.clone()));
    assert!(
        tree::open(&f.place, "fix the mast", NOW).is_err(),
        "one tree an item"
    );
    // A second tree for the same name a day takes the next branch name.
    tree::close(&f.place, &t).unwrap();
    let t2 = tree::open(&f.place, "fix the mast", NOW).unwrap();
    assert_eq!(t2.branch, format!("{}-2", t.branch));
}

/// A state dir inside the repo's checkout would put the trees inside it:
/// refused before a worktree is added.
#[test]
fn trees_inside_the_repo_are_refused() {
    let mut f = fixture();
    f.place.state = f.place.repo.join("state");
    let e = tree::open(&f.place, "fix the mast", NOW).unwrap_err();
    assert!(format!("{e:#}").contains("inside the repo"), "{e:#}");
    let repo = f.place.repo.canonicalize().unwrap();
    assert_eq!(git(&repo, &["worktree", "list"]).lines().count(), 1);
}

/// The gate that judges is main's copy, run on the tree: a tree that writes
/// a weakened gate.sh over its own is still judged red by main's.
#[tokio::test]
async fn the_gate_is_mains_copy_even_when_the_tree_edits_its_own() {
    let f = fixture();
    let t = tree::open(&f.place, "weak gate", NOW).unwrap();
    std::fs::write(t.dir.join("scripts/gate.sh"), WEAK).unwrap();
    std::fs::write(t.dir.join("broken.txt"), "a failing change\n").unwrap();
    tree::commit(&f.place, &t, "self: weaken the gate (weak-gate)", "body")
        .unwrap()
        .unwrap();
    let run = tree::gate(&f.place, &t).await.unwrap();
    assert_eq!(run.verdict, Verdict::Red, "{}", run.tail);
    let log = std::fs::read_to_string(&run.log).unwrap();
    assert!(log.contains("gate: FAILED in suite"), "{log}");
    let blob = git(&f.place.repo, &["rev-parse", "origin/main:scripts/gate.sh"]);
    assert_eq!(run.blob, blob);
    assert!(!run.log.starts_with(&t.dir), "the log outlives the tree");
    // It ran on the tree, with the tree's own target, no bench, and none of
    // the daemon's own variables.
    assert!(
        log.contains(&format!(
            "root {} target {} nobench 1 session none",
            t.dir.display(),
            t.target.display()
        )),
        "{log}"
    );
    // Its build tree is reached at target/, as the benches look for it.
    assert_eq!(std::fs::read_link(t.dir.join("target")).unwrap(), t.target);
    // Fixed, the same tree is green.
    std::fs::remove_file(t.dir.join("broken.txt")).unwrap();
    tree::commit(&f.place, &t, "self: fix (weak-gate)", "body")
        .unwrap()
        .unwrap();
    assert_eq!(
        tree::gate(&f.place, &t).await.unwrap().verdict,
        Verdict::Green
    );
}

/// The keel's failure is its own verdict.
#[tokio::test]
async fn a_keel_failure_is_the_keels_verdict() {
    let f = fixture();
    let t = tree::open(&f.place, "loosen", NOW).unwrap();
    std::fs::write(t.dir.join("keel.txt"), "a loosened test\n").unwrap();
    tree::commit(&f.place, &t, "self: loosen (loosen)", "body").unwrap();
    assert_eq!(
        tree::gate(&f.place, &t).await.unwrap().verdict,
        Verdict::Keel
    );
}

/// A green branch pushes to refs/heads/self/* on the remote, and nothing
/// else moves there; main is refused by name, and so is any name outside
/// self/.
#[test]
fn a_push_goes_to_self_only_and_main_is_refused() {
    let f = fixture();
    let t = tree::open(&f.place, "trim the sails", NOW).unwrap();
    std::fs::write(t.dir.join("sails.txt"), "trimmed\n").unwrap();
    let head = tree::commit(&f.place, &t, "self: trim (trim)", "body")
        .unwrap()
        .unwrap();
    let clean = Scrubber::default();
    let scrub = |s: &str| clean.scrub(s).1;
    let main_before = git(&f.upstream, &["rev-parse", "refs/heads/main"]);
    for bad in [
        "main",
        "refs/heads/main",
        "self/",
        "self/../main",
        "+self/x",
        "self/x:main",
    ] {
        assert!(
            matches!(
                tree::push(&f.place, &t, "origin", bad, &scrub),
                Err(Refused::NotSelf(_))
            ),
            "{bad}"
        );
    }
    assert_eq!(
        git(&f.upstream, &["rev-parse", "refs/heads/main"]),
        main_before
    );
    let spec = tree::push(&f.place, &t, "origin", &t.branch, &scrub).unwrap();
    assert_eq!(spec, format!("refs/heads/{0}:refs/heads/{0}", t.branch));
    assert_eq!(
        git(
            &f.upstream,
            &["rev-parse", &format!("refs/heads/{}", t.branch)]
        ),
        head
    );
    assert_eq!(
        git(&f.upstream, &["rev-parse", "refs/heads/main"]),
        main_before
    );
    assert_eq!(
        git(&f.upstream, &["for-each-ref", "--format=%(refname)"]),
        format!("refs/heads/main\nrefs/heads/{}", t.branch)
    );
}

/// A token's shape in what the branch adds stops the push, and so does one
/// in a commit message; nothing reaches the remote.
#[test]
fn a_scrub_hit_stops_the_push() {
    let f = fixture();
    let clean = Scrubber::default();
    let scrub = |s: &str| clean.scrub(s).1;
    // Built from parts: a whole one would be refused by the host on push.
    let token = format!("ghp_{}", "Zq7RkV2mW9xLpT4sNd8YbHc3FgJe6UaQo1Ki");
    for (slug, file, msg) in [
        (
            "in a file",
            format!("token = \"{token}\"\n"),
            "self: add (x)".to_string(),
        ),
        (
            "in a message",
            "fine\n".to_string(),
            format!("self: add {token} (x)"),
        ),
    ] {
        let t = tree::open(&f.place, slug, NOW).unwrap();
        std::fs::write(t.dir.join("cfg.txt"), file).unwrap();
        tree::commit(&f.place, &t, &msg, "body").unwrap().unwrap();
        let r = tree::push(&f.place, &t, "origin", &t.branch, &scrub);
        assert!(matches!(r, Err(Refused::Scrub { .. })), "{slug}: {r:?}");
        let up = git(&f.upstream, &["for-each-ref", "--format=%(refname)"]);
        assert_eq!(up, "refs/heads/main", "{slug}");
    }
}

/// Main's own scrub check, when main has one, stops a push too.
#[test]
fn mains_scrub_script_stops_the_push() {
    let f = fixture();
    let seed = f.place.repo.parent().unwrap().join("seed");
    std::fs::write(
        seed.join("scripts/scrub.sh"),
        "#!/bin/bash\n! grep -rq 'Captain Invented' \"$1\" --exclude-dir=.git\n",
    )
    .unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-q", "-m", "scrub"]);
    git(&seed, &["push", "-q", f.upstream.to_str().unwrap(), "main"]);
    git(&f.place.repo, &["fetch", "-q", "origin"]);
    let t = tree::open(&f.place, "a name", NOW).unwrap();
    std::fs::write(t.dir.join("crew.txt"), "Captain Invented\n").unwrap();
    tree::commit(&f.place, &t, "self: crew (a-name)", "body").unwrap();
    let none = |_: &str| 0;
    let r = tree::push(&f.place, &t, "origin", &t.branch, &none);
    assert!(
        matches!(&r, Err(Refused::Scrub { by, .. }) if by.contains("scrub.sh")),
        "{r:?}"
    );
}

/// Close removes the tree and its build tree; the branch stays.
#[tokio::test]
async fn close_removes_the_tree_and_its_target_and_keeps_the_branch() {
    let f = fixture();
    let t = tree::open(&f.place, "close me", NOW).unwrap();
    std::fs::write(t.dir.join("a.txt"), "a\n").unwrap();
    let head = tree::commit(&f.place, &t, "self: a (close-me)", "b")
        .unwrap()
        .unwrap();
    tree::gate(&f.place, &t).await.unwrap();
    std::fs::write(t.target.join("built"), "x").unwrap();
    tree::close(&f.place, &t).unwrap();
    assert!(!t.dir.exists() && !t.target.exists());
    assert_eq!(tree::find(&f.place, "close me"), None);
    assert_eq!(
        tree::branches(&f.place).unwrap(),
        vec![(t.branch.clone(), head)]
    );
}

/// A branch that changes an exam's file is never pushed: an exam item or
/// the exam crate's data (the proposer rule of theseus-pw1q.11, a join fix).
#[test]
fn a_push_that_touches_the_exams_is_refused() {
    let f = fixture();
    let clean = Scrubber::default();
    let scrub = |s: &str| clean.scrub(s).1;
    let t = tree::open(&f.place, "loosen the exam", NOW).unwrap();
    std::fs::create_dir_all(t.dir.join("crates/theseus-exam/exam")).unwrap();
    std::fs::write(
        t.dir.join("crates/theseus-exam/exam/exam-v2.toml"),
        "easier\n",
    )
    .unwrap();
    tree::commit(&f.place, &t, "self: loosen (x)", "body")
        .unwrap()
        .unwrap();
    let r = tree::push(&f.place, &t, "origin", &t.branch, &scrub);
    assert!(matches!(r, Err(Refused::Exams(_))), "{r:?}");
    let up = git(&f.upstream, &["for-each-ref", "--format=%(refname)"]);
    assert_eq!(up, "refs/heads/main");
}
