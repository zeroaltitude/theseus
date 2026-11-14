//! `undo_resolve`'s tests (`p5kln-state-builder`): the precedence, pure, and
//! the probe against real repositories.
//!
//! The git half builds a throwaway repository per test under the process's
//! temporary directory, with the operator's global and system git config left
//! out (AGENTS.md, Item 7) and signing off, so the fixtures are hermetic and
//! a locked signing agent cannot hang them. Every name in them is invented.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

/// A throwaway directory that deletes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        // The pid and a counter: unique within a run, and within the one
        // process nextest gives each test.
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "theseus-undo-resolve-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("the scratch directory");
        Self(d)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `git` in `dir`, with nothing of the machine's configuration: the test's own
/// identity, no signing, no global or system file.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).expect("the fixture file");
}

/// A repository with one committed file, `kept.txt`.
fn a_repo(tag: &str) -> Scratch {
    let s = Scratch::new(tag);
    git(s.path(), &["init", "-q", "-b", "main"]);
    write(&s.path().join("kept.txt"), "the committed bytes\n");
    git(s.path(), &["add", "kept.txt"]);
    git(s.path(), &["commit", "-q", "-m", "the first commit"]);
    s
}

// ---- the precedence, pure ------------------------------------------------

#[test]
fn the_strongest_true_way_back_is_the_word() {
    // Absent beats everything: a path that holds nothing can lose nothing.
    let mut f = Found::new("a.txt", "write");
    f.exists = false;
    f.tracked = Tracked::Clean;
    f.read_this_session = true;
    assert_eq!(recovery_of(&f), Recovery::Absent);

    // A clean tracked file: the strongest real recovery.
    let mut f = Found::new("a.txt", "write");
    f.tracked = Tracked::Clean;
    f.read_this_session = true;
    assert_eq!(recovery_of(&f), Recovery::TrackedClean);

    // The session's own record holds the CURRENT bytes, where a dirty tree
    // holds only older committed ones, so it ranks above `tracked_dirty`.
    let mut f = Found::new("a.txt", "write");
    f.tracked = Tracked::Dirty;
    f.read_this_session = true;
    assert_eq!(recovery_of(&f), Recovery::ReadThisSession);
    f.read_this_session = false;
    assert_eq!(recovery_of(&f), Recovery::TrackedDirty);

    // And over a file nothing holds at all, either way.
    let mut f = Found::new("a.txt", "write");
    f.tracked = Tracked::Untracked;
    f.read_this_session = true;
    assert_eq!(recovery_of(&f), Recovery::ReadThisSession);
    f.read_this_session = false;
    assert_eq!(recovery_of(&f), Recovery::Untracked);

    let mut f = Found::new("/elsewhere/a.txt", "write");
    f.tracked = Tracked::NotARepo;
    assert_eq!(recovery_of(&f), Recovery::OutsideRepo);

    // A probe that could not tell says so, and the pack reads it as no copy.
    assert_eq!(
        recovery_of(&Found::new("a.txt", "write")),
        Recovery::Unknown
    );
}

#[test]
fn regenerable_is_only_ever_a_callers_word_and_ranks_under_a_real_copy() {
    // The probe never sets it (the module header says why), but a caller that
    // knows the project rebuilds the path may, and then it beats "nothing
    // holds a copy" and loses to every real copy.
    let mut f = Found::new("target/debug/app", "write");
    f.tracked = Tracked::Untracked;
    f.regenerable = true;
    assert_eq!(recovery_of(&f), Recovery::Regenerable);
    f.tracked = Tracked::Dirty;
    assert_eq!(recovery_of(&f), Recovery::TrackedDirty);
    f.tracked = Tracked::Clean;
    assert_eq!(recovery_of(&f), Recovery::TrackedClean);
    // And `find` leaves it false, whatever the path looks like: a location
    // rule is what this classifier replaces.
    let mut p = GitCli::default();
    let f = find(&mut p, "target/debug/app", "write", false);
    assert!(!f.regenerable, "the probe must not guess regenerability");
}

#[test]
fn a_targets_word_and_its_access_ride_into_the_state_unchanged() {
    let mut one = Found::new("src/main.rs", "write");
    one.tracked = Tracked::Clean;
    let mut two = Found::new("notes.txt", "read");
    two.tracked = Tracked::Untracked;
    let ts = targets_of(&[one, two]);
    assert_eq!(ts.len(), 2, "every target the plan named is in the state");
    assert_eq!(ts[0].path, "src/main.rs");
    assert_eq!(ts[0].access, "write");
    assert_eq!(ts[0].recovery, Recovery::TrackedClean);
    assert_eq!(ts[1].recovery, Recovery::Untracked);
    assert!(targets_of(&[]).is_empty());
}

#[test]
fn an_l1_program_calls_write_loses_nothing_on_this_machine() {
    // The pack reads the whole call's `sandbox`, not a per-target word: step
    // one says an `l1` program call "writes into a scratch layer thrown away
    // when the call ends, so it loses nothing on this machine whatever the
    // command looks like". So what the resolver owes is the class the gate
    // bound, and this is the pair.
    assert_eq!(sandbox_of(true, true), Sandbox::L1);
    assert_eq!(sandbox_of(true, false), Sandbox::L0);
    // A call that is not a program is neither, however its class reads.
    assert_eq!(sandbox_of(false, true), Sandbox::NotAProgram);
    assert_eq!(sandbox_of(false, false), Sandbox::NotAProgram);
    // And the target words are unaffected by it: the two facts are apart, so
    // an L1 call whose target is untracked still says `untracked` and the pack
    // settles it from `sandbox`. A resolver that folded L1 into the word would
    // make an L0 call on the same path unreadable.
    let mut f = Found::new("src/main.rs", "write");
    f.tracked = Tracked::Untracked;
    assert_eq!(recovery_of(&f), Recovery::Untracked);
}

#[test]
fn a_working_directory_is_never_called_scratch_by_a_guess() {
    let root = PathBuf::from("/invented/repo");
    assert_eq!(cwd_kind_of(Some(&root), Some(false)), CwdKind::RepoClean);
    assert_eq!(cwd_kind_of(Some(&root), Some(true)), CwdKind::RepoDirty);
    assert_eq!(cwd_kind_of(Some(&root), None), CwdKind::Unknown);
    assert_eq!(cwd_kind_of(None, None), CwdKind::NotARepo);
    assert_eq!(cwd_kind_of(None, Some(true)), CwdKind::NotARepo);
}

// ---- the probe, against real repositories --------------------------------

#[test]
fn a_clean_tree_and_a_dirty_one_give_tracked_clean_and_tracked_dirty() {
    let s = a_repo("clean-dirty");
    let kept = s.path().join("kept.txt");
    let mut p = GitCli::default();

    // The same path, the same tool, the same arguments: the answer is opposite
    // on either side of one uncommitted edit. That pair is the whole reason
    // this module exists.
    let clean = find(&mut p, &kept.to_string_lossy(), "write", false);
    assert_eq!(clean.tracked, Tracked::Clean);
    assert_eq!(recovery_of(&clean), Recovery::TrackedClean);

    write(&kept, "edited and not committed\n");
    let mut p = GitCli::default();
    let dirty = find(&mut p, &kept.to_string_lossy(), "write", false);
    assert_eq!(dirty.tracked, Tracked::Dirty);
    assert_eq!(recovery_of(&dirty), Recovery::TrackedDirty);

    // A staged change is dirty too: HEAD is what a checkout would restore.
    git(s.path(), &["add", "kept.txt"]);
    let mut p = GitCli::default();
    let staged = find(&mut p, &kept.to_string_lossy(), "write", false);
    assert_eq!(staged.tracked, Tracked::Dirty);
}

#[test]
fn a_target_the_session_already_read_gives_read_this_session() {
    let s = a_repo("read-session");
    let loose = s.path().join("loose.txt");
    write(&loose, "nothing in the repository holds this\n");
    let mut p = GitCli::default();

    // Untracked, so nothing on the machine holds a copy …
    let unread = find(&mut p, &loose.to_string_lossy(), "write", false);
    assert_eq!(unread.tracked, Tracked::Untracked);
    assert_eq!(recovery_of(&unread), Recovery::Untracked);

    // … unless this session read exactly these bytes, which its own record
    // still holds. The caller answers that: the transcript is the core's.
    let read = find(&mut p, &loose.to_string_lossy(), "write", true);
    assert_eq!(recovery_of(&read), Recovery::ReadThisSession);
}

#[test]
fn a_path_under_no_repository_and_one_that_is_not_there_yet() {
    let s = Scratch::new("outside");
    let loose = s.path().join("loose.txt");
    write(&loose, "outside every repository\n");
    let mut p = GitCli::default();
    let f = find(&mut p, &loose.to_string_lossy(), "write", false);
    assert_eq!(f.tracked, Tracked::NotARepo);
    assert_eq!(recovery_of(&f), Recovery::OutsideRepo);

    // A path that is not there: creating it is undone by deleting it, and
    // this holds inside a repository as well as outside one.
    let gone = s.path().join("not-yet.txt");
    let f = find(&mut p, &gone.to_string_lossy(), "write", false);
    assert!(!f.exists);
    assert_eq!(recovery_of(&f), Recovery::Absent);

    let r = a_repo("absent-in-repo");
    let mut p = GitCli::default();
    let f = find(
        &mut p,
        &r.path().join("new.txt").to_string_lossy(),
        "write",
        false,
    );
    assert_eq!(recovery_of(&f), Recovery::Absent);
}

#[test]
fn an_ignored_file_is_untracked_and_never_regenerable() {
    // The decision in the module header, held by a test: git holds no copy of
    // an ignored file, so `untracked` is true of it, and the location rule
    // that would call it `regenerable` is not made.
    let s = a_repo("ignored");
    write(&s.path().join(".gitignore"), "build/\n");
    std::fs::create_dir_all(s.path().join("build")).expect("the build directory");
    let out = s.path().join("build").join("app");
    write(&out, "a build output\n");
    let mut p = GitCli::default();
    let f = find(&mut p, &out.to_string_lossy(), "write", false);
    assert_eq!(f.tracked, Tracked::Untracked);
    assert!(!f.regenerable);
    assert_eq!(recovery_of(&f), Recovery::Untracked);
}

#[test]
fn the_working_directory_reads_clean_then_dirty_from_the_same_probe() {
    let s = a_repo("cwd");
    let mut p = GitCli::default();
    let (root, dirty) = p.repo(s.path());
    assert!(root.is_some(), "the fixture is a repository");
    assert_eq!(dirty, Some(false));
    assert_eq!(cwd_kind_of(root.as_deref(), dirty), CwdKind::RepoClean);

    // A probe caches per call, so the same probe keeps its answer; a later
    // call gets a probe of its own and sees the edit.
    write(&s.path().join("kept.txt"), "edited\n");
    let (root, dirty) = p.repo(s.path());
    assert_eq!(dirty, Some(false), "one call's probe is one call's answer");
    let mut fresh = GitCli::default();
    let (root2, dirty2) = fresh.repo(s.path());
    assert_eq!(root2, root);
    assert_eq!(dirty2, Some(true));
    assert_eq!(cwd_kind_of(root2.as_deref(), dirty2), CwdKind::RepoDirty);

    // Outside every repository.
    let out = Scratch::new("cwd-outside");
    let mut p = GitCli::default();
    assert_eq!(p.repo(out.path()), (None, None));
    assert_eq!(cwd_kind_of(None, None), CwdKind::NotARepo);
}

/// A repository whose own `.git/config` names a command for git to run, and
/// a marker file that command writes. Returns the marker's path, which must
/// not exist after a probe.
#[cfg(unix)]
fn a_repo_that_runs_a_command(tag: &str) -> (Scratch, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let s = a_repo(tag);
    let marker = s.path().join("the-command-ran");
    let script = s.path().join("run-me.sh");
    // Exit 1 so git falls back to its ordinary path: the point is that the
    // command RAN, not that it broke the answer. A hostile one would not be
    // so polite, which is exactly why the probe must never reach it.
    write(
        &script,
        &format!("#!/bin/sh\necho ran >> {}\nexit 1\n", marker.display()),
    );
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("the script is executable");
    // `core.fsmonitor` with a non-boolean value is a command git runs to ask
    // what changed, and it lives in the repository's OWN config -- which a
    // clone does NOT carry, so the delivery is an archive unpacked with its
    // `.git` intact, a config an earlier tool call wrote, or a repository
    // already on disk. `core.hooksPath` is set beside it for the same reason.
    git(
        s.path(),
        &["config", "core.fsmonitor", &script.to_string_lossy()],
    );
    git(
        s.path(),
        &["config", "core.hooksPath", &s.path().to_string_lossy()],
    );
    (s, marker)
}

#[test]
#[cfg(unix)]
fn a_repository_cannot_run_its_own_monitor_or_hook_through_the_probe() {
    // THE POINT OF THIS TEST. The probe addresses a tree the MODEL named, on
    // the gate's own path, before any gate has spoken -- so a monitor or a
    // hook that tree's `.git/config` names must not be made to run. Without
    // the hardening flags on every invocation this fails on `tracked`'s first
    // `ls-files`: git asks the named command what changed, and a repository
    // sitting on disk has just run code of its own choosing in the daemon.
    //
    // AND NO MORE THAN THAT CLASS. A content filter driver the tree names in
    // its own `.gitattributes`, with a command in its own config, is STILL
    // run by `diff` and `status`; `GitCli::git` says so and says why there is
    // no command-line close for it. A pass here is not the class closed.
    let (s, marker) = a_repo_that_runs_a_command("hostile-config");
    let kept = s.path().join("kept.txt");

    let mut p = GitCli::default();
    let clean = find(&mut p, &kept.to_string_lossy(), "write", false);
    assert!(
        !marker.exists(),
        "the repository's own command ran during the probe"
    );
    // And the hardening costs the answer nothing: the word is what the same
    // fixture gives without the hostile config.
    assert_eq!(clean.tracked, Tracked::Clean);
    assert_eq!(recovery_of(&clean), Recovery::TrackedClean);

    // The other two invocations reach it as well, so exercise them: the
    // working-directory probe (`status`) and the dirty half of `tracked`
    // (`diff`).
    write(&kept, "edited and not committed\n");
    let mut p = GitCli::default();
    let dirty = find(&mut p, &kept.to_string_lossy(), "write", false);
    let (root, is_dirty) = p.repo(s.path());
    assert!(
        !marker.exists(),
        "the repository's own command ran during the probe"
    );
    assert_eq!(dirty.tracked, Tracked::Dirty);
    assert_eq!(root.as_deref(), Some(s.path()));
    assert_eq!(is_dirty, Some(true));

    // And `check-ignore`, which `undo_regen` asks of a path its table knows:
    // a `target/` beside a `Cargo.toml`, ignored.
    write(&s.path().join(".gitignore"), "target/\n");
    write(
        &s.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\n",
    );
    std::fs::create_dir_all(s.path().join("target")).expect("the target directory");
    let out = s.path().join("target").join("app");
    write(&out, "a build output\n");
    let mut p = GitCli::default();
    let f = find(&mut p, &out.to_string_lossy(), "write", false);
    assert!(
        !marker.exists(),
        "the repository's own command ran during the probe"
    );
    assert!(f.regenerable, "the fixture is the table's own first line");
}

#[test]
fn a_repository_with_no_commit_yet_reads_dirty_not_clean() {
    // There is no HEAD to restore from, so the committed bytes a checkout
    // would bring back do not exist. `dirty` is the true weaker answer and
    // `clean` would be a false claim that something holds a copy.
    let s = Scratch::new("no-head");
    git(s.path(), &["init", "-q", "-b", "main"]);
    let f = s.path().join("staged.txt");
    write(&f, "added, never committed\n");
    git(s.path(), &["add", "staged.txt"]);
    let mut p = GitCli::default();
    let found = find(&mut p, &f.to_string_lossy(), "write", false);
    assert_eq!(found.tracked, Tracked::Dirty);
    assert_eq!(recovery_of(&found), Recovery::TrackedDirty);
}
