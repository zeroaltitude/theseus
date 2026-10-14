//! The join's git steps (theseus-pw1q.9), each a blocking call on a
//! [`Place`]: a detached tree at a commit (for the planted revert and the
//! merge), the change's paths and diff, the dry run (`git merge-tree`), a
//! plain `--no-ff` merge (never a rebase), its signature with the key
//! `user.signingkey` names in `[self] repo`, and the push of main, never
//! forced. Every tree lives under `<state>/self/trees/` with its own build
//! tree, as self-pr's do, and is removed by `rsi::tree::close`.

use anyhow::{bail, Result};

use crate::rsi::tree::{self, Place, Tree};

/// A detached worktree of `rev` named `<what>-<slug of branch>`, with its
/// own build tree. One left by a crash is removed first.
pub fn detached(p: &Place, what: &str, branch: &str, rev: &str) -> Result<Tree> {
    let slug = format!("{what}-{}", tree::slug(branch.trim_start_matches("self/")));
    std::fs::create_dir_all(p.trees())?;
    let trees = std::fs::canonicalize(p.trees())?;
    let dir = trees.join(&slug);
    let target = p.targets().join(&slug);
    let t = Tree {
        slug,
        branch: branch.into(),
        dir,
        target,
    };
    if t.dir.exists() {
        let _ = tree::close(p, &t);
        let _ = std::fs::remove_dir_all(&t.dir);
    }
    let d = t.dir.to_string_lossy().to_string();
    p.git_ok(&p.repo, &["worktree", "add", "-q", "--detach", &d, rev])?;
    std::fs::create_dir_all(&t.target)?;
    Ok(t)
}

/// `refs/remotes/<remote>/main`'s commit.
pub fn main_of(p: &Place, remote: &str) -> Result<String> {
    p.git_ok(
        &p.repo,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/remotes/{remote}/main^{{commit}}"),
        ],
    )
}

/// Fetch `remote`, never prompting.
pub fn fetch(p: &Place, remote: &str) -> Result<()> {
    p.git_ok(&p.repo, &["fetch", "-q", remote])?;
    Ok(())
}

/// The paths the branch changes against `base` (added, changed, deleted).
pub fn paths(p: &Place, base: &str, head: &str) -> Result<Vec<(char, String)>> {
    let out = p.git_ok(
        &p.repo,
        &[
            "diff",
            "--no-renames",
            "--name-status",
            &format!("{base}...{head}"),
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let (s, path) = l.split_once('\t')?;
            Some((s.chars().next()?, path.to_string()))
        })
        .collect())
}

/// The branch's diff against `base`, with `context` lines around each hunk.
pub fn diff(p: &Place, base: &str, head: &str, context: u32) -> Result<String> {
    p.git_ok(
        &p.repo,
        &[
            "diff",
            "--no-color",
            "--no-renames",
            &format!("-U{context}"),
            &format!("{base}...{head}"),
        ],
    )
}

/// The dry run: does `head` merge into `base` without a conflict? The
/// conflicting paths when it does not.
pub fn dry_run(p: &Place, base: &str, head: &str) -> Result<Result<(), String>> {
    let o = p.git(
        &p.repo,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            base,
            head,
        ],
    )?;
    match o.status.code() {
        Some(0) => Ok(Ok(())),
        Some(1) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let paths: Vec<&str> = out.lines().skip(1).filter(|l| !l.is_empty()).collect();
            Ok(Err(format!(
                "it conflicts with main in {}",
                paths.join(", ")
            )))
        }
        _ => bail!(
            "git merge-tree failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
    }
}

/// A plain `--no-ff` merge of `head` into a detached tree at `base`, as
/// Theseus, unsigned (the signature comes after the gate). Its commit.
pub fn merge(p: &Place, t: &Tree, head: &str, message: &str) -> Result<String> {
    p.git_ok(
        &t.dir,
        &[
            "-c",
            "user.name=Theseus",
            "-c",
            "user.email=theseus@localhost",
            "-c",
            "commit.gpgsign=false",
            "merge",
            "-q",
            "--no-ff",
            "--no-verify",
            "-m",
            message,
            head,
        ],
    )?;
    p.git_ok(&t.dir, &["rev-parse", "HEAD"])
}

/// The key `user.signingkey` names in the repo's config, if any.
pub fn signing_key(p: &Place) -> Option<String> {
    p.git_ok(&p.repo, &["config", "--get", "user.signingkey"])
        .ok()
        .filter(|k| !k.trim().is_empty())
}

/// Sign the tree's merge (the same tree and parents, so the gate's verdict
/// holds) with `key`; the signed commit, which must carry its signature.
pub fn sign(p: &Place, t: &Tree, key: &str) -> Result<String> {
    p.git_ok(
        &t.dir,
        &[
            "-c",
            "user.name=Theseus",
            "-c",
            "user.email=theseus@localhost",
            "-c",
            &format!("user.signingkey={key}"),
            "commit",
            "-q",
            "--amend",
            "--no-edit",
            "--no-verify",
            "-S",
        ],
    )?;
    let head = p.git_ok(&t.dir, &["rev-parse", "HEAD"])?;
    if !signed(p, &head)? {
        bail!("the merge {head} carries no signature after signing");
    }
    Ok(head)
}

/// Whether commit `c` carries a signature.
pub fn signed(p: &Place, c: &str) -> Result<bool> {
    let raw = p.git_ok(&p.repo, &["cat-file", "commit", c])?;
    Ok(raw.lines().any(|l| l.starts_with("gpgsig")))
}

/// What a push of main found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pushed {
    Done,
    /// Main moved since `base`: the dry run, the merge and the gate again.
    Moved(String),
}

/// Push the signed `merge` to `remote`'s main, never forced, after checking
/// that main is still `base`. An unsigned commit never goes.
pub fn push_main(p: &Place, t: &Tree, remote: &str, base: &str, merge: &str) -> Result<Pushed> {
    if !signed(p, merge)? {
        bail!("refused: {merge} is not signed, and an unsigned merge never reaches main");
    }
    fetch(p, remote)?;
    let now = main_of(p, remote)?;
    if now != base {
        return Ok(Pushed::Moved(now));
    }
    let spec = format!("{merge}:refs/heads/main");
    let o = p.git(&t.dir, &["push", "-q", "--no-verify", remote, &spec])?;
    if o.status.success() {
        return Ok(Pushed::Done);
    }
    fetch(p, remote)?;
    let now = main_of(p, remote)?;
    if now != base {
        return Ok(Pushed::Moved(now));
    }
    bail!(
        "git refused the push of main: {}",
        String::from_utf8_lossy(&o.stderr).trim()
    )
}
