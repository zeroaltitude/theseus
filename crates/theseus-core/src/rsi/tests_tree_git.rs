//! `tree_git`: the daemon's own git in a self tree reads the repo's git directory and runs no
//! program the tree's writer chose. Each measure has its own test, and each was planted (the
//! measure removed, the test red, the measure restored):
//! - the explicit git directory: `a_pinned_git_reads_the_repos_git_directory_not_the_trees`;
//! - the `.git` check: `a_tree_whose_git_file_was_rewritten_gets_no_git`;
//! - hooks off: `no_hook_runs_on_a_commit_or_a_checkout`;
//! - the monitor off: `the_file_system_monitor_never_runs`;
//! - the editors: `no_editor_runs`;
//! - a nested repository: `a_tree_that_holds_a_repository_of_its_own_is_not_committed`;
//! - the one way git is spawned: `every_daemon_git_in_rsi_goes_through_the_one_command`;
//! - an L0 removal after L1 code ran in the tree: `rv_undo_fix_removes_nothing_outside_the_tree`.
//!
//! A marker command here is a script that touches one file in the test's own temp dir.

use super::tests_tree::{fixture, git, Fixture};
use super::tree::{self, Tree};
use super::tree_git;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_792_000_000_000;

fn opened() -> (Fixture, Tree) {
    let f = fixture();
    let t = tree::open(&f.place, "trim the sails", NOW).unwrap();
    (f, t)
}

/// The marker's path, and a script that touches it and passes stdin through (a filter's shape).
fn marker(f: &Fixture, name: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let mark = f.place.state.join(format!("RAN-{name}"));
    let script = f.place.state.join(format!("{name}.sh"));
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\nexec cat\n", mark.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (mark, script)
}

/// A git directory of text files inside the tree, as a file tool could write it: its HEAD and
/// branch, the repo's objects by `alternates`, and `config`. Then the tree's `.git` names it.
fn point_elsewhere(f: &Fixture, t: &Tree, config: &str) {
    let evil = t.dir.join(".evil");
    let head = git(&t.dir, &["rev-parse", "HEAD"]);
    let objects = f.place.repo.join(".git/objects").canonicalize().unwrap();
    for (p, text) in [
        (
            "HEAD".to_string(),
            format!("ref: refs/heads/{}\n", t.branch),
        ),
        (format!("refs/heads/{}", t.branch), format!("{head}\n")),
        (
            "objects/info/alternates".into(),
            format!("{}\n", objects.display()),
        ),
        ("config".into(), config.to_string()),
    ] {
        let at = evil.join(p);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    std::fs::write(t.dir.join(".git"), format!("gitdir: {}\n", evil.display())).unwrap();
}

fn ran(mark: &Path) -> bool {
    mark.exists()
}

/// `git add -A` as the daemon would run it in the tree, its `.git` check skipped (`located`
/// alone), or with `-C` and nothing named (the control). Hermetic, and asserted to succeed.
fn add_all(f: &Fixture, t: &Tree, pinned: bool) {
    let placed = match pinned {
        true => tree_git::located(&f.place.repo, &t.dir).unwrap().0,
        false => vec![],
    };
    let mut args: Vec<String> = tree_git::HARDENED.iter().map(|a| a.to_string()).collect();
    args.extend(placed.iter().map(|a| a.to_string_lossy().into_owned()));
    args.extend(["add".into(), "-A".into()]);
    git(&t.dir, &args.iter().map(String::as_str).collect::<Vec<_>>());
}

/// KEY (the explicit git directory): a tree whose `.git` names a directory with a filter of its
/// writer's, selected by the tree's own `.gitattributes`: `git add` by plain `-C` runs it (the
/// control, which shows the rig would see it), and the daemon's placed git does not.
#[test]
fn a_pinned_git_reads_the_repos_git_directory_not_the_trees() {
    let (f, t) = opened();
    let (mark, script) = marker(&f, "filter");
    point_elsewhere(
        &f,
        &t,
        &format!(
            "[core]\n\trepositoryformatversion = 0\n[filter \"evil\"]\n\tclean = {}\n",
            script.display()
        ),
    );
    std::fs::write(t.dir.join(".gitattributes"), "*.txt filter=evil\n").unwrap();
    std::fs::write(t.dir.join("sails.txt"), "trimmed\n").unwrap();
    add_all(&f, &t, true);
    assert!(
        !ran(&mark),
        "the daemon's git ran a filter the tree's .git named"
    );
    // The control: the same call found its git directory through the tree, and ran it.
    add_all(&f, &t, false);
    assert!(
        ran(&mark),
        "the control did not run the filter, so the rig shows nothing"
    );
}

/// KEY (the `.git` check): a tree whose `.git` was rewritten, replaced by a link (even to the
/// right place) or by a directory gets no git at all, in plain words, and nothing it named runs.
#[test]
fn a_tree_whose_git_file_was_rewritten_gets_no_git() {
    let (f, t) = opened();
    let (mark, script) = marker(&f, "fsmonitor");
    let made = std::fs::read_to_string(t.dir.join(".git")).unwrap();
    assert!(
        f.place.git_ok(&t.dir, &["status", "--porcelain"]).is_ok(),
        "an intact tree"
    );
    point_elsewhere(
        &f,
        &t,
        &format!(
            "[core]\n\trepositoryformatversion = 0\n\tfsmonitor = {}\n",
            script.display()
        ),
    );
    let e = f.place.git_ok(&t.dir, &["add", "-A"]).unwrap_err();
    assert!(format!("{e:#}").contains(tree_git::TAMPERED), "{e:#}");
    assert!(!ran(&mark));
    // The commit the step would make stops the same way.
    let e = tree::commit(&f.place, &t, "s", "b").unwrap_err();
    assert!(format!("{e:#}").contains(tree_git::TAMPERED), "{e:#}");
    // A link to the very file git made is not the file git made.
    let admin = f.place.repo.join(".git/worktrees").join(&t.slug);
    std::fs::write(t.dir.join("gitfile"), &made).unwrap();
    std::fs::remove_file(t.dir.join(".git")).unwrap();
    std::os::unix::fs::symlink(t.dir.join("gitfile"), t.dir.join(".git")).unwrap();
    assert!(!tree_git::intact(&t.dir, &admin), "a link");
    std::fs::remove_file(t.dir.join(".git")).unwrap();
    std::fs::create_dir(t.dir.join(".git")).unwrap();
    assert!(!tree_git::intact(&t.dir, &admin), "a directory");
    std::fs::remove_dir(t.dir.join(".git")).unwrap();
    std::fs::write(t.dir.join(".git"), &made).unwrap();
    assert!(tree_git::intact(&t.dir, &admin), "what git made, put back");
    assert!(!ran(&mark));
}

/// KEY (hooks off): executable hooks in the repo's own hooks directory, where a commit and a
/// checkout would fire them, never run for the daemon (`--no-verify` leaves both).
#[test]
fn no_hook_runs_on_a_commit_or_a_checkout() {
    let (f, mut t) = opened();
    let (mark, script) = marker(&f, "hook");
    let hooks = f.place.repo.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for h in [
        "post-commit",
        "post-checkout",
        "pre-commit",
        "reference-transaction",
    ] {
        std::os::unix::fs::symlink(&script, hooks.join(h)).unwrap();
    }
    std::fs::write(t.dir.join("sails.txt"), "trimmed\n").unwrap();
    assert!(tree::commit(&f.place, &t, "s", "b").unwrap().is_some());
    tree::next_branch(&f.place, &mut t).unwrap();
    assert!(!ran(&mark), "a hook ran");
}

/// The file-system monitor the repo's config names never runs for the daemon.
#[test]
fn the_file_system_monitor_never_runs() {
    let (f, t) = opened();
    let (mark, script) = marker(&f, "fsmonitor");
    git(
        &f.place.repo,
        &["config", "core.fsmonitor", script.to_str().unwrap()],
    );
    std::fs::write(t.dir.join("sails.txt"), "trimmed\n").unwrap();
    f.place.git_ok(&t.dir, &["add", "-A"]).unwrap();
    f.place.git_ok(&t.dir, &["status", "--porcelain"]).unwrap();
    assert!(!ran(&mark), "the monitor ran");
}

/// The editors the repo's config names never run: a call that asks for one takes the message
/// as it is.
#[test]
fn no_editor_runs() {
    let (f, t) = opened();
    let (mark, script) = marker(&f, "editor");
    git(
        &f.place.repo,
        &["config", "core.editor", script.to_str().unwrap()],
    );
    git(
        &f.place.repo,
        &["config", "sequence.editor", script.to_str().unwrap()],
    );
    f.place
        .git_ok(
            &t.dir,
            &["commit", "--amend", "--allow-empty", "-q", "--no-verify"],
        )
        .unwrap();
    assert!(!ran(&mark), "an editor ran");
}

/// KEY (a nested repository): a repository inside the tree, with a filter in its own config
/// over its own files, is never committed: `git add` would run `git status` in it by its own
/// config, which no `-c` of the daemon's reaches. A second commit, once its file's timestamp
/// moved, is where the filter would run.
#[test]
fn a_tree_that_holds_a_repository_of_its_own_is_not_committed() {
    let (f, t) = opened();
    let (mark, script) = marker(&f, "nested");
    let sub = t.dir.join("vendor/sub");
    std::fs::create_dir_all(&sub).unwrap();
    git(&sub, &["init", "-q"]);
    std::fs::write(sub.join(".gitattributes"), "*.txt filter=evil\n").unwrap();
    std::fs::write(sub.join("f.txt"), "one\n").unwrap();
    git(&sub, &["add", "-A"]);
    git(&sub, &["commit", "-q", "-m", "sub"]);
    git(
        &sub,
        &["config", "filter.evil.clean", script.to_str().unwrap()],
    );
    let mut said = Vec::new();
    for _ in 0..2 {
        said.push(tree::commit(&f.place, &t, "s", "b").map_err(|e| format!("{e:#}")));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(sub.join("f.txt"), "one\n").unwrap();
    }
    assert!(!ran(&mark), "a nested repository's filter ran");
    for s in said {
        assert!(
            s.as_ref().is_err_and(|e| e.contains(tree_git::NESTED)),
            "{s:?}"
        );
    }
}

/// A directory that is no worktree of the repo (a reviewer's clone of it, say) gets no git.
#[test]
fn a_directory_that_is_no_worktree_of_the_repo_gets_no_git() {
    let f = fixture();
    let other = f.place.state.join("a-clone");
    git(
        &f.place.state,
        &[
            "clone",
            "-q",
            f.upstream.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    let e = f.place.git_ok(&other, &["status"]).unwrap_err();
    assert!(format!("{e:#}").contains("is not a worktree of"), "{e:#}");
    assert!(
        f.place.git_ok(&f.place.repo, &["status"]).is_ok(),
        "the repo itself"
    );
}

/// A `.git` component, in any case, wherever it is in a path; a name that only starts so is not.
#[test]
fn a_dot_git_component_is_found_wherever_it_is() {
    for p in [
        "/t/.git",
        "/t/.git/config",
        "/t/sub/.GIT/hooks/x",
        "rel/.Git",
    ] {
        assert!(tree_git::names_dot_git(Path::new(p)), "{p}");
    }
    for p in ["/t/.gitignore", "/t/.github/x", "/t/x.git", "/t/git"] {
        assert!(!tree_git::names_dot_git(Path::new(p)), "{p}");
    }
}

/// The choke point: no file of `rsi/` but this module's own and the mint's fixtures (a
/// repository the daemon builds in a temp dir it made, never a self tree) spawns git itself.
#[test]
fn every_daemon_git_in_rsi_goes_through_the_one_command() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/rsi");
    let allowed = ["tree_git.rs", "mint/validate/fixtures.rs"];
    let mut todo = vec![root.clone()];
    let mut raw = Vec::new();
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(&root).unwrap().display().to_string();
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "rs")
                && !rel.split('/').any(|c| c.starts_with("tests"))
                && !allowed.contains(&rel.as_str())
            {
                let text = std::fs::read_to_string(&p).unwrap();
                let spawned = [
                    "Command::new(\"git\")",
                    "Command::new(\"git\".",
                    "new(\"git\")",
                ];
                if spawned.iter().any(|s| text.contains(s)) {
                    raw.push(rel);
                }
            }
        }
    }
    assert!(
        raw.is_empty(),
        "git spawned outside `Place::git_command`: {raw:?}"
    );
}

/// PROBE (git-pin review r1, note 2): `undo_fix` removes a path the branch added with
/// `std::fs::remove_file` at L0, after the branch's tests ran in L1 with the tree read-write. A
/// test that replaced the path's parent directory with a link would make that removal land
/// outside the tree; the step stops instead, in plain words.
#[test]
fn rv_undo_fix_removes_nothing_outside_the_tree() {
    let f = fixture();
    let t = tree::open(&f.place, "planted probe", NOW).unwrap();
    let outside = f.place.state.join("owners-files");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("added.txt"), "the owner's\n").unwrap();
    // What the branch's test code would leave behind in L1: `src` is now a link out.
    std::fs::create_dir_all(t.dir.join("src")).ok();
    std::fs::remove_dir_all(t.dir.join("src")).unwrap();
    std::os::unix::fs::symlink(&outside, t.dir.join("src")).unwrap();
    let paths = vec![('A', "src/added.txt".to_string())];
    let said = super::join::planted::undo_fix(&f.place, &t, "HEAD", &paths);
    assert!(
        outside.join("added.txt").exists(),
        "undo_fix removed a file outside the tree through a link"
    );
    let e = format!("{:#}", said.unwrap_err());
    assert!(e.contains("lies through a link in the tree"), "{e}");
    // A path the branch added under no link is still removed; a link it added is removed as a
    // link, and its target is untouched.
    std::fs::remove_file(t.dir.join("src")).unwrap();
    std::fs::create_dir_all(t.dir.join("src")).unwrap();
    std::fs::write(t.dir.join("src/added.txt"), "the branch's\n").unwrap();
    std::os::unix::fs::symlink(outside.join("added.txt"), t.dir.join("src/link")).unwrap();
    let paths = vec![
        ('A', "src/added.txt".to_string()),
        ('A', "src/link".to_string()),
    ];
    let undone = super::join::planted::undo_fix(&f.place, &t, "HEAD", &paths).unwrap();
    assert_eq!(undone.len(), 2);
    assert!(!t.dir.join("src/added.txt").exists());
    assert!(t.dir.join("src/link").symlink_metadata().is_err());
    assert!(outside.join("added.txt").exists(), "the link's target");
}

/// The words a tampered tree stops with say how to clear it, naming the tree's item.
#[test]
fn a_tampered_trees_words_say_how_to_clear_it() {
    let (f, t) = opened();
    std::fs::write(t.dir.join(".git"), "gitdir: /elsewhere\n").unwrap();
    let e = format!("{:#}", f.place.git_ok(&t.dir, &["status"]).unwrap_err());
    assert!(e.contains(tree_git::TAMPERED), "{e}");
    assert!(e.contains("remove the tree for trim-the-sails"), "{e}");
    assert!(e.contains("git worktree prune"), "{e}");
}
