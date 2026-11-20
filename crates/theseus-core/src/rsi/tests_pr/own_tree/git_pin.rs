//! Git's own files are not a self session's to write, and a tree whose `.git` was changed gets
//! no git from the daemon (S1 of the fixer's second review; `rsi::tree_git`). The daemon's git in
//! a tree runs outside L1, so a session that could point the tree's `.git` at a directory of its
//! own, or write the repo's config or hooks, would choose what the daemon runs, with no card.
//! Planted: `own_tree::git_refusal` removed from `refusal_at`, each write below ran; the `.git`
//! check removed, the builder's next call committed and gated a tree its `.git` no longer named.

use super::*;

/// The self session of the task `sails`, and its tree, a real worktree of the repo.
fn opened(r: &Rig) -> (String, PathBuf) {
    let sid = task_record(r, "sails");
    let t = crate::rsi::tree::open(&r.f.place, "sails", theseus_protocol::now_unix_ms()).unwrap();
    (sid, t.dir.canonicalize().unwrap())
}

/// Whether the call was refused in so many words, and asked nobody.
fn refused(r: &Rig, sid: &str, said: &str) -> bool {
    said.contains("Not run") && said.contains("git's own") && !asked(r, sid)
}

/// KEY (2a, 2b, 2c): every way a self session's file tools reach git's own files is refused,
/// and nothing is written: its tree's `.git` (written, edited, through a link of an innocent
/// name), the repo's git directory (its config, a hook, the tree's admin files, its attributes),
/// by a link out of the tree and by `..`. Under a posture that asks for everything, where a self
/// session's other writes run with a notice.
#[tokio::test]
async fn a_self_session_writes_no_git_file_of_its_tree_or_the_repo() {
    let r = held(|_| {});
    let (sid, tree) = opened(&r);
    let git = r.f.place.repo.canonicalize().unwrap().join(".git");
    std::os::unix::fs::symlink(&git, tree.join("link")).unwrap();
    std::os::unix::fs::symlink(tree.join(".git"), tree.join("notes.txt")).unwrap();
    std::os::unix::fs::symlink(git.join("hooks/post-merge"), tree.join("plan.txt")).unwrap();
    let up = tree.components().count() - 1;
    let dots: PathBuf = std::iter::repeat_n("..", up).collect();
    let by_dots = tree
        .join(dots)
        .join(git.strip_prefix("/").unwrap())
        .join("config");
    let made = |p: &Path| std::fs::read(p).ok();
    let before: Vec<_> = [tree.join(".git"), git.join("config")]
        .iter()
        .map(|p| made(p))
        .collect();
    let mut ran = Vec::new();
    for (what, path) in [
        ("2a: the tree's .git", tree.join(".git")),
        ("2a: through a link named notes.txt", tree.join("notes.txt")),
        ("2b: the repo's config", git.join("config")),
        ("2b: a hook", git.join("hooks/post-commit")),
        (
            "2b: the tree's admin files",
            git.join("worktrees/sails/gitdir"),
        ),
        ("2b: the repo's attributes", git.join("info/attributes")),
        ("2c: a link out of the tree", tree.join("link/config")),
        ("2c: a link to a hook not yet there", tree.join("plan.txt")),
        ("2c: a `..` out of the tree", by_dots),
    ] {
        let input = json!({"path": path, "content": "gitdir: /elsewhere\n"});
        let said = r.turn(&sid, "fs_write", input).await;
        if !refused(&r, &sid, &said) {
            ran.push(format!("{what}: {said}"));
        }
    }
    // An edit of the file, after a read of it (which is no write, and runs).
    let said = r
        .turn(&sid, "fs_read", json!({"path": tree.join(".git")}))
        .await;
    assert!(said.contains("gitdir"), "{said}");
    let edit =
        json!({"path": tree.join(".git"), "old_string": "gitdir: ", "new_string": "gitdir: /x"});
    let said = r.turn(&sid, "fs_edit", edit).await;
    if !refused(&r, &sid, &said) {
        ran.push(format!("2a, an edit: {said}"));
    }
    // Every arm is tried, so a planted run lists each one that was open.
    assert!(ran.is_empty(), "not refused:\n{}", ran.join("\n"));
    for written in ["hooks/post-commit", "hooks/post-merge", "info/attributes"] {
        assert!(!git.join(written).exists(), "written: {written}");
    }
    let after: Vec<_> = [tree.join(".git"), git.join("config")]
        .iter()
        .map(|p| made(p))
        .collect();
    assert_eq!(before, after, "git's files changed");
    // Its own files, `.gitignore` among them, it writes as before.
    let said = r
        .turn(
            &sid,
            "fs_write",
            json!({"path": tree.join(".gitignore"), "content": "target\n"}),
        )
        .await;
    assert!(!said.contains("Not run"), "{said}");
    assert_eq!(
        std::fs::read_to_string(tree.join(".gitignore")).unwrap(),
        "target\n"
    );
}

/// Git's user-level config is held as the repo's git directory is.
#[test]
fn gits_user_config_is_reached_by_name_and_through_a_link() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    std::fs::create_dir_all(home.join(".config/git")).unwrap();
    std::os::unix::fs::symlink(home.join(".gitconfig"), home.join("innocent")).unwrap();
    let kept = [home.join(".gitconfig"), home.join(".config/git")];
    let reaches = |p: PathBuf| crate::rsi::own_tree::reaches_git(&p, &kept);
    assert!(reaches(home.join(".gitconfig")));
    assert!(reaches(home.join(".config/git/attributes")));
    assert!(reaches(home.join("innocent")));
    assert!(reaches(home.join(".config/x/../git/config")));
    assert!(!reaches(home.join(".config/gitk")));
    assert!(!reaches(home.join("notes.txt")));
}

/// KEY, the finding end to end: a builder, by its own `fs.write` calls, makes a git directory
/// of text files in its tree whose config would run a command, and then points the tree's `.git`
/// at it. That last write is refused. A writer the file tools do not govern (the branch's own
/// code, which the gate runs in L1 with the tree writable) makes the same change; the builder's
/// next call commits nothing, in plain words, its row names the item, the branch and why, and the
/// command never runs.
#[tokio::test]
async fn a_builder_that_points_its_trees_git_elsewhere_runs_nothing_outside_l1() {
    let r = rig(SelfMode::Act, Some("origin"));
    let sid = r.session("self");
    let tree = r.open_and_edit(&sid, "sails.txt").await;
    let branch =
        super::super::super::tests_tree::git(&tree, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let head = super::super::super::tests_tree::git(&tree, &["rev-parse", "HEAD"]);
    let objects = r.f.place.repo.join(".git/objects").canonicalize().unwrap();
    let mark =
        r.f.place
            .state
            .canonicalize()
            .unwrap()
            .join("RAN-OUTSIDE-L1");
    let run = format!("\"touch {}; false\"", mark.display());
    let evil = tree.join(".evil");
    for (p, text) in [
        ("HEAD".to_string(), format!("ref: refs/heads/{branch}\n")),
        (format!("refs/heads/{branch}"), format!("{head}\n")),
        ("objects/info/alternates".into(), format!("{}\n", objects.display())),
        (
            "config".into(),
            format!("[core]\n\trepositoryformatversion = 0\n\tfsmonitor = {run}\n[filter \"evil\"]\n\tclean = {run}\n"),
        ),
    ] {
        let path = evil.join(p);
        let said = r.turn(&sid, "fs_write", json!({"path": path, "content": text})).await;
        assert!(path.exists(), "the builder's own file: {said}");
    }
    let said = r
        .turn(
            &sid,
            "fs_write",
            json!({"path": tree.join(".gitattributes"), "content": "* filter=evil\n"}),
        )
        .await;
    assert!(!said.contains("Not run"), "{said}");
    let link = format!("gitdir: {}\n", evil.display());
    let made = std::fs::read_to_string(tree.join(".git")).unwrap();
    let said = r
        .turn(
            &sid,
            "fs_write",
            json!({"path": tree.join(".git"), "content": link}),
        )
        .await;
    assert!(
        said.contains("Not run") && said.contains("git's own"),
        "{said}"
    );
    assert_eq!(std::fs::read_to_string(tree.join(".git")).unwrap(), made);
    // The other writer.
    std::fs::write(tree.join(".git"), &link).unwrap();
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(
        !mark.exists(),
        "the daemon's git ran the tree's command, outside L1"
    );
    assert!(said.contains(crate::rsi::tree_git::TAMPERED), "{said}");
    let row = r.rows().pop().expect("a row");
    assert_eq!(row["item"], "trim-the-sails");
    assert_eq!(row["branch"], branch.as_str());
    assert_eq!(row["stopped_at"], "commit");
    let why = row["numbers"]["refused"].as_str().unwrap_or_default();
    assert!(why.contains(crate::rsi::tree_git::TAMPERED), "{row}");
    assert!(
        !r.f.upstream.join(format!("refs/heads/{branch}")).exists(),
        "nothing pushed"
    );
}
