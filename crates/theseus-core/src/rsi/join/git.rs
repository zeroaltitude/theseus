//! The join's git steps (theseus-pw1q.9), each a blocking call on a
//! [`Place`]: a detached tree at a commit (for the planted revert and the
//! merge), the change's paths and diff, the dry run (`git merge-tree`), a
//! plain `--no-ff` merge (never a rebase), its signature with the key
//! `user.signingkey` names in `[self] repo`, and the push of main, never
//! forced. Every tree lives under `<state>/self/trees/` with its own build
//! tree, as self-pr's do, and is removed by `rsi::tree::close`.

use std::os::unix::process::CommandExt;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use theseus_kernel::children::{self, Kind};

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

/// The longest one fetch may take (theseus-6vlr4). Git has no total or
/// connect bound of its own: `http.lowSpeedTime` counts only once a transfer
/// has begun, and an ssh remote's `ConnectTimeout` is the user's config, so a
/// black-holed connection waits on the kernel's timeouts, minutes. A fetch of
/// a repository this size is seconds, so a minute is a remote that is not
/// answering; the callers go on with the refs they have (the door says so) or
/// fail (a revert, a join), and try again at their next step.
pub const FETCH_LIMIT: Duration = Duration::from_secs(60);

/// Fetch `remote`, never prompting, for at most [`FETCH_LIMIT`].
pub fn fetch(p: &Place, remote: &str) -> Result<()> {
    fetch_within(p, remote, FETCH_LIMIT)
}

/// [`fetch`] with its own bound. When the bound fires the fetch **fails**, and
/// git's whole process group is killed and reaped: `git fetch` runs a
/// transport helper (`git-remote-https`, `ssh`) that outlives a signal sent
/// to git alone, so the child leads its own group and the group gets the
/// SIGKILL. The wait is a timed receive on a watcher thread, not a poll.
pub fn fetch_within(p: &Place, remote: &str, limit: Duration) -> Result<()> {
    let args = ["fetch", "-q", remote];
    let mut c = p.git_command(&p.repo, &args);
    c.process_group(0);
    let mut child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
        .context("git could not start")?;
    let pid = child.id();
    // The pipes are drained on threads of their own, so the wait below is on the child alone: a
    // descendant that left the killed group and holds a pipe open cannot hold the fetch past its
    // bound (theseus-6vlr4 join fix).
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let (done, finished) = mpsc::channel::<()>();
    let watcher = std::thread::spawn(move || {
        let timed_out = finished.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout);
        if timed_out {
            // SAFETY: a plain signal to the group this child leads. The pid
            // is ours: the child is still unreaped here unless it exited in
            // the instant the bound fired, and the group then holds nothing.
            unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
        }
        timed_out
    });
    // Reaps the child, which the kill ends.
    let status = child.wait();
    drop(done);
    let timed_out = watcher.join().unwrap_or(false);
    if timed_out {
        // The readers are left to end on their own when the last holder of a pipe closes it.
        drop((stdout, stderr));
        bail!(
            "git fetch {remote} did not finish in {}s and was killed",
            limit.as_secs_f64()
        );
    }
    let status = status?;
    let _ = stdout.join();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    Ok(())
}

/// Read a child's pipe to its end on a thread of its own.
fn drain<R: std::io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = pipe {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    })
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
        // A git before 2.38 has no `--write-tree` (a usage error): the system's
        // git, which the daemon's PATH may find first. The dry run is then a
        // merge in a throwaway tree.
        Some(129) => dry_run_in_tree(p, base, head),
        _ => bail!(
            "git merge-tree failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
    }
}

/// The dry run with plain git: `head` merged into a throwaway detached tree at
/// `base` without a commit, its conflicting paths read, the merge aborted and
/// the tree closed.
pub(super) fn dry_run_in_tree(p: &Place, base: &str, head: &str) -> Result<Result<(), String>> {
    let t = detached(p, "dry", "dry-run", base)?;
    let merged = p.git(&t.dir, &["merge", "--no-commit", "--no-ff", head]);
    let r = match merged {
        Ok(o) if o.status.success() => Ok(Ok(())),
        Ok(_) => {
            let paths = p.git_ok(&t.dir, &["diff", "--name-only", "--diff-filter=U"])?;
            let paths: Vec<&str> = paths.lines().filter(|l| !l.is_empty()).collect();
            Ok(Err(format!(
                "it conflicts with main in {}",
                paths.join(", ")
            )))
        }
        Err(e) => Err(e),
    };
    let _ = p.git(&t.dir, &["merge", "--abort"]);
    let _ = tree::close(p, &t);
    r
}

/// A plain `--no-ff` merge of `head` into a detached tree at `base`, as the
/// repo's identity ([`Place::identity`]), unsigned (the signature comes after
/// the gate). Its commit.
pub fn merge(p: &Place, t: &Tree, head: &str, message: &str) -> Result<String> {
    let ident = p.ident_args();
    let mut args: Vec<&str> = ident.iter().map(String::as_str).collect();
    args.extend([
        "-c",
        "commit.gpgsign=false",
        "merge",
        "-q",
        "--no-ff",
        "--no-verify",
        "-m",
        message,
        head,
    ]);
    p.git_ok(&t.dir, &args)?;
    p.git_ok(&t.dir, &["rev-parse", "HEAD"])
}

/// The key `user.signingkey` names in the repo's config, if any.
pub fn signing_key(p: &Place) -> Option<String> {
    p.git_ok(&p.repo, &["config", "--get", "user.signingkey"])
        .ok()
        .filter(|k| !k.trim().is_empty())
}

/// Sign the tree's merge (the same tree and parents, so the gate's verdict
/// holds, and the repo's identity, so the signature matches the key's user ID) with `key`; the signed commit, which must carry its signature.
pub fn sign(p: &Place, t: &Tree, key: &str) -> Result<String> {
    let ident = p.ident_args();
    let mut args: Vec<&str> = ident.iter().map(String::as_str).collect();
    let signingkey = format!("user.signingkey={key}");
    args.extend([
        "-c",
        &signingkey,
        "commit",
        "-q",
        "--amend",
        "--no-edit",
        "--no-verify",
        "-S",
    ]);
    p.git_ok(&t.dir, &args)?;
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
