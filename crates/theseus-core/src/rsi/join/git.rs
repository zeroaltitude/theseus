//! The join's git steps (theseus-pw1q.9), each a blocking call on a
//! [`Place`]: a detached tree at a commit (for the planted revert and the
//! merge), the change's paths and diff, the dry run (`git merge-tree`), a
//! plain `--no-ff` merge (never a rebase), its signature with the key
//! `user.signingkey` names in `[self] repo`, and the push of main, never
//! forced. Every tree lives under `<state>/self/trees/` with its own build
//! tree, as self-pr's do, and is removed by `rsi::tree::close`.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use theseus_kernel::children::{self, Kind};

use crate::rsi::tree::{self, Place, Tree};

/// The name a detached tree of `what` for `branch` has under the trees and the build trees.
pub fn slug_of(what: &str, branch: &str) -> String {
    format!("{what}-{}", tree::slug(branch.trim_start_matches("self/")))
}

/// A detached worktree of `rev` named `<what>-<slug of branch>`, with its
/// own build tree.
///
/// Only the daemon that owns the trees opens one (`trees_lock`, issue 41), and
/// only under a trees directory inside the state directory. What stands at the
/// path is decided before anything is done to it, and every refusal is the
/// open's failure, naming it (issue 41, round 3):
/// - a path this daemon has open already (`guard::opened_here`, its lifetime
///   not ended) is refused, never reopened over;
/// - a tree a previous process of the loop made and left (its `created` mark,
///   and git lists that same worktree) is closed with `tree::close`; when the
///   close refuses (a gate's lease, git) the open fails and the tree stays: no
///   delete stands in for it;
/// - anything else at the path (no mark, a reservation, a worktree that is not
///   the one the mark names) is in the way and is left as it is.
///
/// The name is reserved (`guard::mark`) before the tree exists and the mark is
/// rewritten as `created` once `git worktree add` has made it
/// (`guard::mark_created`); `tree::close` removes it with the tree.
pub fn detached(p: &Place, what: &str, branch: &str, rev: &str) -> Result<Tree> {
    let slug = slug_of(what, branch);
    if p.owner.is_none() {
        bail!(
            "{}: this daemon opens no {what} tree",
            crate::rsi::trees_lock::NOT_OWNER
        );
    }
    std::fs::create_dir_all(p.trees())?;
    if !p.trees_inside() {
        bail!(
            "the self trees directory {} leads out of the state directory: no {what} tree is opened there",
            p.trees().display()
        );
    }
    let trees = std::fs::canonicalize(p.trees())?;
    let dir = trees.join(&slug);
    let target = p.targets().join(&slug);
    let t = Tree {
        slug,
        branch: branch.into(),
        dir,
        target,
    };
    // Claimed before anything is done at the path, so a start's sweep never takes it for a dead daemon's; a path
    // this daemon has open already is refused here.
    super::guard::claim(&t.dir).map_err(anyhow::Error::msg)?;
    let opened = (|| -> Result<()> {
        if std::fs::symlink_metadata(&t.dir).is_ok() {
            make_way(p, &t)?;
        }
        super::guard::mark(p, &t.slug, what)?;
        let d = t.dir.to_string_lossy().to_string();
        if let Err(e) = p.git_ok(&p.repo, &["worktree", "add", "-q", "--detach", &d, rev]) {
            super::guard::unmark(p, &t.slug);
            return Err(e);
        }
        let at = p.git_ok(&t.dir, &["rev-parse", "HEAD"])?;
        if let Err(e) = super::guard::mark_created(p, &t.slug, &t.dir, &at) {
            // A tree whose mark could not say what it is: closed again, and the open fails. The claim is held through
            // the close; the open's failure below is its one release.
            let _ = tree::close_holding_claim(p, &t);
            return Err(e);
        }
        std::fs::create_dir_all(&t.target)?;
        Ok(())
    })();
    if let Err(e) = opened {
        super::guard::closed_here(&t.dir);
        return Err(e);
    }
    Ok(t)
}

/// Something stands at `t`'s path: closed when it is a tree a previous process of the loop made and left, else the
/// open fails, naming why, and the path is left as it is.
fn make_way(p: &Place, t: &Tree) -> Result<()> {
    let Some(m) = super::guard::mark_of(p, &t.slug) else {
        bail!(
            "{} is in the way and the loop did not open it: it is left as it is",
            t.dir.display()
        );
    };
    if let Err(why) = super::guard::is_the_made_tree(p, &m, &t.dir) {
        bail!(
            "{} is in the way, marked, but not the tree the loop made ({why}): it is left as it is",
            t.dir.display()
        );
    }
    // The claim on the path is this open's, held through the replacement: the predecessor's close does not end it.
    tree::close_holding_claim(p, t).with_context(|| {
        format!(
            "{} is the loop's tree from an earlier start, and it would not close: it is left as it is",
            t.dir.display()
        )
    })
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

/// The commit `refs/heads/<branch>` names in `[self] repo` now.
pub fn at(p: &Place, branch: &str) -> Result<String> {
    p.git_ok(
        &p.repo,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )
}

/// Whether `a` is `b` or an ancestor of it.
pub fn is_ancestor(p: &Place, a: &str, b: &str) -> Result<bool> {
    let o = p.git(&p.repo, &["merge-base", "--is-ancestor", a, b])?;
    match o.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "git merge-base --is-ancestor failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
    }
}

/// The commits of `range` (`a..b`).
pub fn commits(p: &Place, range: &str) -> Result<Vec<String>> {
    Ok(p.git_ok(&p.repo, &["rev-list", range])?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// Where `head` left `main`: the commit both hold.
pub fn merge_base(p: &Place, main: &str, head: &str) -> Result<String> {
    p.git_ok(&p.repo, &["merge-base", main, head])
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
    let mut c = p.git_command(&p.repo, &args)?;
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

/// The key `user.signingkey` names in the repo's config, if any. It reads
/// [`Place::signing_key`], which is where the key lives now that the self
/// loop's content commit ([`tree::commit`]) is signed with the same one.
pub fn signing_key(p: &Place) -> Option<String> {
    p.signing_key()
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

/// Where a push of main goes, read from git's configuration in one go: the URL to push to as the remote names it,
/// the URL git turns that into (the destination), and a digest of every setting that can change where a push goes,
/// what it sends or what it runs. A credential's value is never read into it: credential helpers count by name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Dest {
    /// The push URL as configured (`remote.<name>.pushurl`, else `.url`); git rewrites it as the remote's would be.
    pub raw: String,
    /// The destination: `git remote get-url --push`, the one URL a push of the remote goes to.
    pub url: String,
    /// SHA-256 of the settings that bear on a push (`push_settings`), sorted.
    pub digest: String,
}

impl Dest {
    /// The destination as the owner is shown it: any user and password part of the URL removed.
    pub fn shown(&self) -> String {
        shown_url(&self.url)
    }
}

/// `url` without its `user[:password]@` part (a `scheme://` URL; any other form is shown as it is).
pub fn shown_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (auth, tail) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    let host = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
    format!("{scheme}://{host}{tail}")
}

/// Why a remote has no single destination, in fixed words.
pub const NO_DEST: &str = "the remote names no push destination";
pub const TWO_DESTS: &str = "the remote names more than one push destination";
pub const SPLIT_DEST: &str = "the remote fetches from one place and pushes to another";

/// Whether a configuration key bears on where a push goes, what it sends or what it runs.
fn push_setting(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("remote.")
        || (k.starts_with("url.") && (k.ends_with(".insteadof") || k.ends_with(".pushinsteadof")))
        || k.starts_with("push.")
        || k == "core.sshcommand"
        || k.starts_with("include.")
        || k.starts_with("includeif.")
        || (k.starts_with("credential.") && k.ends_with(".helper"))
        || k == "credential.helper"
}

/// A setting's key with its section and its name lowercased and its subsection as written: git compares the first
/// two without case and the subsection exactly.
fn key_as_git_reads(k: &str) -> String {
    match (k.find('.'), k.rfind('.')) {
        (Some(f), Some(l)) if f < l => format!(
            "{}{}{}",
            k[..f].to_ascii_lowercase(),
            &k[f..l],
            k[l..].to_ascii_lowercase()
        ),
        _ => k.to_ascii_lowercase(),
    }
}

/// Fixed words: git rewrites addresses in this repository (`url.*.insteadOf` or `url.*.pushInsteadOf`).
pub const REWRITES: &str = "git rewrites addresses here, so the destination cannot be shown truly";
/// Fixed words: the destination carries a credential where it cannot be removed from what is shown.
pub const URL_CREDENTIAL: &str =
    "the destination carries a credential outside the user part of its address, so it cannot be shown";

/// The settings that bear on a push (`push_setting`), as git reads them in `dir` (the directory the push runs in)
/// with includes followed, `key=value` in git's own order, a repeated key's values in theirs; a credential helper's
/// value is its name. Or [`REWRITES`] when an address rewrite rule is in effect.
fn push_settings(p: &Place, dir: &Path) -> Result<std::result::Result<Vec<String>, &'static str>> {
    let o = p.git(dir, &["config", "--list", "-z"])?;
    if !o.status.success() {
        bail!(
            "git config --list failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    let mut out = Vec::new();
    for e in (o.stdout.split(|b| *b == 0)).filter(|e| !e.is_empty()) {
        let e = String::from_utf8_lossy(e);
        let (k, v) = e.split_once('\n').unwrap_or((&e, ""));
        let lower = k.to_ascii_lowercase();
        if lower.starts_with("url.")
            && (lower.ends_with(".insteadof") || lower.ends_with(".pushinsteadof"))
        {
            return Ok(Err(REWRITES));
        }
        if push_setting(k) {
            out.push(format!("{}={v}", key_as_git_reads(k)));
        }
    }
    Ok(Ok(out))
}

/// Whether `url` carries what may be a credential outside its user part: a query, a fragment, or a path segment
/// holding `@` or `:`.
fn credential_outside_userinfo(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else {
        return false;
    };
    let path = rest.find('/').map_or("", |i| &rest[i..]);
    url.contains('?')
        || url.contains('#')
        || path
            .split('/')
            .any(|seg| seg.contains('@') || seg.contains(':'))
}

/// The remote's one push destination, read in `dir` (the directory the push runs in), or why there is none, in
/// fixed words: no push URL, more than one, a fetch URL that is not the push URL (the fetch the card and the merge
/// read must read where the push goes), an address rewrite rule in effect, or a credential in the address outside
/// its user part.
pub fn dest_in(
    p: &Place,
    dir: &Path,
    remote: &str,
) -> Result<std::result::Result<Dest, &'static str>> {
    let urls = |args: &[&str]| -> Result<Vec<String>> {
        let o = p.git(dir, args)?;
        Ok(String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    };
    let settings = match push_settings(p, dir)? {
        Ok(s) => s,
        Err(why) => return Ok(Err(why)),
    };
    let push = urls(&["remote", "get-url", "--push", "--all", remote])?;
    let fetch = urls(&["remote", "get-url", "--all", remote])?;
    let url = match push.as_slice() {
        [] => return Ok(Err(NO_DEST)),
        [one] => one.clone(),
        _ => return Ok(Err(TWO_DESTS)),
    };
    if fetch != vec![url.clone()] {
        return Ok(Err(SPLIT_DEST));
    }
    if credential_outside_userinfo(&url) {
        return Ok(Err(URL_CREDENTIAL));
    }
    let configured = |key: &str| -> Result<Vec<String>> {
        urls(&["config", "--get-all", &format!("remote.{remote}.{key}")])
    };
    let raw = match configured("pushurl")?.as_slice() {
        [one] => one.clone(),
        [] => match configured("url")?.as_slice() {
            [one] => one.clone(),
            [] => return Ok(Err(NO_DEST)),
            _ => return Ok(Err(TWO_DESTS)),
        },
        _ => return Ok(Err(TWO_DESTS)),
    };
    use sha2::Digest;
    let digest = hex::encode(sha2::Sha256::digest(settings.join("\0").as_bytes()));
    Ok(Ok(Dest { raw, url, digest }))
}

/// [`dest_in`], read in the self repository.
pub fn dest(p: &Place, remote: &str) -> Result<std::result::Result<Dest, &'static str>> {
    dest_in(p, &p.repo, remote)
}

/// Push the signed `merge` to main at `dest`, never forced, after checking that main is still `base`: one ref, to
/// one destination, with no tags followed and no submodule pushed, whatever git's configuration says. The remote's
/// configured URL is pushed to rather than its name, so no `remote.<name>.push` refspec or mirror setting applies.
/// An unsigned commit never goes.
pub fn push_main(
    p: &Place,
    t: &Tree,
    remote: &str,
    dest: &Dest,
    base: &str,
    merge: &str,
) -> Result<Pushed> {
    if !signed(p, merge)? {
        bail!("refused: {merge} is not signed, and an unsigned merge never reaches main");
    }
    fetch(p, remote)?;
    let now = main_of(p, remote)?;
    if now != base {
        return Ok(Pushed::Moved(now));
    }
    let spec = format!("{merge}:refs/heads/main");
    let args = push_args(&dest.raw, &spec);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = p.git(&t.dir, &args)?;
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

/// The push's argv: one refspec to one URL, tags and submodules off by flag and by setting.
pub fn push_args(url: &str, spec: &str) -> Vec<String> {
    [
        "-c",
        "push.followTags=false",
        "-c",
        "push.recurseSubmodules=no",
        "--no-replace-objects",
        "push",
        "-q",
        "--no-verify",
        "--no-follow-tags",
        "--recurse-submodules=no",
        url,
        spec,
    ]
    .map(str::to_string)
    .to_vec()
}
