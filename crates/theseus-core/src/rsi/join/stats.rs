//! The count in the self-join's own merge (issue 45): every landing on main holds a STATS.md counted
//! from its own tree, the loop's joins too. Between the merge and main's gate, main's counter
//! (`scripts/stats.py --count-head`) writes STATS.md in the join's tree, and the merge is folded to
//! hold it, so the merge the gate judges, the signature signs and the push sends is the counted one.
//!
//! - **The counter is main's**, pinned to the join's base: the blob `<base>:scripts/stats.py`,
//!   frozen under `<state>/self/gates/`, never the branch's copy nor the merged tree's.
//! - **It runs in L1**, as every command in a join's tree does (`Place::jail`), with its time limit,
//!   and with the tree its one lasting write: the build directory its view gives it is a throwaway
//!   one, never the gate's (`Tree::target`), and is removed after it.
//! - **Containment is the whole tree, read from the file system** ([`stats_tree`]): no git reads the
//!   tree's files for the count, so the count runs nothing the tree's attributes select (a filter, a
//!   diff driver) outside L1. Before and after the counter, every path is taken with its kind, bits
//!   and content; nothing but STATS.md may differ, and it must be a plain file.
//! - **The fold reads no attributes from the tree**: the blob is written from the bytes checked, the
//!   tree is the merge's with that one entry (`mktree`), the commit is the merge's object with that
//!   tree, and HEAD and the index move to it with git pointed at an empty directory, never at the tree.
//! - **A failed count does not hold the join**: the tree is put back as the counter found it (HEAD, the
//!   index, every path), that is checked, and the join goes on uncounted, its row saying
//!   `skipped: <why>`. The one exception: a tree that cannot be shown put back stops the join.
//!
//! A main with no counter joins as before. Before the count, the same counter's `--range` gives
//! the merge's change in lines for the notice (issue 45, line 2), checked by the same containment.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use theseus_kernel::children::{self, Kind};

use super::stats_tree::{self, Snapshot};
use crate::rsi::jail::Job;
use crate::rsi::tree::{Place, Tree};

/// The counter, main's copy of which runs.
const COUNTER: &str = "scripts/stats.py";
/// The one path the count may change.
const STATS: &str = "STATS.md";
/// What the count did to a made merge.
pub(super) struct Count {
    /// The merge the join carries on with: the folded one when it was counted, else the one made.
    pub(super) merge: String,
    /// What the join's row says: `counted`, `unchanged`, or `skipped: <why>`.
    pub(super) said: String,
    /// The merge's change in lines, as the counter's `--range` says it without its ids.
    pub(super) lines: Option<String>,
}

/// Count the made merge `merge` (on `base`) in its tree `t`, and fold the count into it. An error
/// only when the tree could not be shown put back after a count that failed.
pub(super) fn count(
    p: &Place,
    t: &Tree,
    base: &str,
    merge: &str,
    limit: Duration,
) -> Result<Count> {
    let kept = |said: String| Count {
        merge: merge.to_string(),
        said,
        lines: None,
    };
    let frozen = match frozen(p, base) {
        Ok(Some(f)) => f,
        Ok(None) => return Ok(kept(format!("skipped: main has no {COUNTER}"))),
        Err(e) => return Ok(kept(format!("skipped: {e:#}"))),
    };
    let before = match stats_tree::snapshot(&t.dir) {
        Ok(s) => s,
        Err(e) => return Ok(kept(format!("skipped: the tree could not be read: {e:#}"))),
    };
    let ran = Scratch::new(p, t).and_then(|s| {
        let range = format!("{base}..HEAD");
        let lines = run(p, &s.job, &frozen, &["--range", &range], limit)
            .ok()
            .and_then(|log| range_line(&log));
        run(p, &s.job, &frozen, &["--count-head"], limit).map(|_| lines)
    });
    let counted = ran.and_then(|lines| Ok((fold(p, t, merge, &before)?, lines)));
    match counted {
        Ok((Some(folded), lines)) => Ok(Count {
            merge: folded,
            said: "counted".into(),
            lines,
        }),
        Ok((None, lines)) => Ok(Count {
            lines,
            ..kept("unchanged".into())
        }),
        Err(why) => {
            restore(p, t, merge, &before)?;
            Ok(kept(format!("skipped: {why:#}")))
        }
    }
}

/// Main's counter as the join's base holds it, written out of every tree under
/// `<state>/self/gates/stats-<blob>.py`; none when the base has none. Read from `base`, never from
/// `origin/main` (which may have moved since) or the merged tree (which the branch wrote).
fn frozen(p: &Place, base: &str) -> Result<Option<PathBuf>> {
    let o = p.git(
        &p.repo,
        &["rev-parse", "-q", "--verify", &format!("{base}:{COUNTER}")],
    )?;
    let blob = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if !o.status.success()
        || blob.is_empty()
        || p.git_ok(&p.repo, &["cat-file", "-t", &blob])? != "blob"
    {
        return Ok(None);
    }
    let out = p.gates().join(format!("stats-{blob}.py"));
    if !out.exists() {
        std::fs::create_dir_all(p.gates())?;
        let o = p.git(&p.repo, &["cat-file", "blob", &blob])?;
        if !o.status.success() {
            bail!("main's {COUNTER} could not be read");
        }
        let tmp = out.with_extension("tmp");
        std::fs::write(&tmp, &o.stdout)?;
        std::fs::rename(&tmp, &out)?;
    }
    Ok(Some(out))
}

/// The tree as the counter's job sees it: the join's tree, with a throwaway build directory of its
/// own under the gates in place of the gate's, removed when the count is done.
struct Scratch {
    job: Tree,
}

impl Scratch {
    fn new(p: &Place, t: &Tree) -> Result<Self> {
        let target = p.gates().join(format!("stats-target-{}", t.slug));
        if std::fs::symlink_metadata(&target).is_ok() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::create_dir_all(&target)?;
        Ok(Self {
            job: Tree {
                target,
                ..t.clone()
            },
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.job.target);
    }
}

/// `python3 -I <frozen> <args> --root <tree>` in L1, its output to a log of its own under the
/// gates; the log when it exited 0.
fn run(p: &Place, job: &Tree, frozen: &Path, args: &[&str], limit: Duration) -> Result<PathBuf> {
    let what = args[0].trim_start_matches('-');
    let log = p.gates().join(format!("stats-{what}-{}.log", job.slug));
    std::fs::create_dir_all(p.gates())?;
    let mut argv: Vec<OsString> = vec!["python3".into(), "-I".into(), frozen.into()];
    argv.extend(args.iter().map(OsString::from));
    argv.extend(["--root".into(), job.dir.clone().into()]);
    let code = match p.jail()? {
        Some(j) => j.run_blocking(
            p,
            &Job {
                tree: job,
                ro: vec![frozen.to_path_buf()],
                argv,
                env: Vec::new(),
                log: &log,
                limit,
            },
        )?,
        // Only a test's place has no jail (`Place::jail`): its stand-in counter runs at L0, as its
        // stand-in gate does.
        None => at_l0(p, job, &argv, &log, limit)?,
    };
    if code != Some(0) {
        let said = std::fs::read_to_string(&log).unwrap_or_default();
        let last = said
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        bail!(
            "main's counter ({}) exited {}: {}",
            args[0],
            code.map_or_else(|| "on a signal".to_string(), |c| c.to_string()),
            last.chars().take(200).collect::<String>()
        );
    }
    Ok(log)
}

/// A test's stand-in for L1: the command in the tree, with the build directory the view would give
/// it, its whole group stopped at the limit.
fn at_l0(
    p: &Place,
    job: &Tree,
    argv: &[OsString],
    log: &Path,
    limit: Duration,
) -> Result<Option<i32>> {
    use std::os::unix::process::CommandExt;
    let out = std::fs::File::create(log)?;
    let mut c = std::process::Command::new(&argv[0]);
    c.args(&argv[1..])
        .current_dir(&job.dir)
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .process_group(0)
        .env("CARGO_TARGET_DIR", &job.target)
        .envs(p.env.iter().map(|(k, v)| (k, v)));
    let mut child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
        .context("main's counter could not start")?;
    let pid = child.id();
    let (done, finished) = mpsc::channel::<()>();
    let watcher = std::thread::spawn(move || {
        let timed_out = finished.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout);
        if timed_out {
            // SAFETY: a plain signal to the group this child leads.
            unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
        }
        timed_out
    });
    let status = child.wait();
    drop(done);
    if watcher.join().unwrap_or(false) {
        bail!(
            "main's counter ran past its {} s and was stopped",
            limit.as_secs()
        );
    }
    Ok(status?.code())
}

/// The change in lines the counter's `--range` printed, without its `lines <a>..<b>: ` ids: the
/// merge it names is not yet the one that lands.
fn range_line(log: &Path) -> Option<String> {
    let said = std::fs::read_to_string(log).ok()?;
    let line = said.lines().rev().find(|l| l.starts_with("lines "))?;
    let (_, change) = line.split_once(": ")?;
    Some(change.chars().take(300).collect())
}

/// The count in the tree, checked and folded into `merge`: the folded merge, or none when the tree
/// is the merge's still. An error when anything but STATS.md changed, or STATS.md is not a plain
/// file, or the fold is not the merge with the count; the caller puts the tree back.
fn fold(p: &Place, t: &Tree, merge: &str, before: &Snapshot) -> Result<Option<String>> {
    let after = stats_tree::snapshot(&t.dir)?;
    let changed = stats_tree::changed(before, &after);
    if changed.is_empty() {
        return Ok(None);
    }
    if changed != [STATS.as_bytes()] {
        let shown: Vec<String> = changed.iter().map(|c| stats_tree::shown(c)).collect();
        bail!(
            "the count changed more than {STATS} ({}), so it was undone",
            shown.join(", ")
        );
    }
    let text = plain(t, &after)?;
    let blob = fed(p, &["hash-object", "-w", "--no-filters", "--stdin"], &text)?;
    let tree = with_stats(p, merge, &blob)?;
    let was = p.git_ok(&p.repo, &["rev-parse", &format!("{merge}^{{tree}}")])?;
    if tree == was {
        return Ok(None);
    }
    let commit = p.git(&p.repo, &["cat-file", "commit", merge])?;
    let object = retree(&commit.stdout, &tree)?;
    let folded = fed(
        p,
        &["hash-object", "-w", "-t", "commit", "--stdin"],
        &object,
    )?;
    p.git_ok(
        &t.dir,
        &["update-ref", "--no-deref", "HEAD", &folded, merge],
    )?;
    let entry = format!("100644,{blob},{STATS}");
    apart(p, t, &["update-index", "--add", "--cacheinfo", &entry])?;
    let parents = |c: &str| p.git_ok(&p.repo, &["rev-parse", &format!("{c}^@")]);
    let head = p.git_ok(&t.dir, &["rev-parse", "HEAD"])?;
    let now = p.git_ok(&p.repo, &["rev-parse", &format!("{folded}^{{tree}}")])?;
    let index = apart(p, t, &["write-tree"])?;
    if head != folded || now != tree || index != tree || parents(&folded)? != parents(merge)? {
        bail!("the folded merge {folded} is not {merge} with the count");
    }
    if stats_tree::snapshot(&t.dir)? != after {
        bail!("the tree changed while the count was folded");
    }
    Ok(Some(folded))
}

/// STATS.md as the counter left it, when it is a plain file: not a link, a directory or an
/// executable.
fn plain(t: &Tree, after: &Snapshot) -> Result<Vec<u8>> {
    let Some(stats_tree::Entry::File(mode, sum)) = after.get(STATS.as_bytes()) else {
        let what = match after.get(STATS.as_bytes()) {
            None => "gone",
            Some(stats_tree::Entry::Dir(_)) => "a directory",
            Some(stats_tree::Entry::Link(_)) => "a link",
            _ => "not a file",
        };
        bail!("{STATS} is {what} in the count's tree, so it was undone");
    };
    if mode & 0o111 != 0 {
        bail!("{STATS} is executable in the count's tree, so it was undone");
    }
    let text = std::fs::read(t.dir.join(STATS))?;
    if <[u8; 32]>::from(<sha2::Sha256 as sha2::Digest>::digest(&text)) != *sum {
        bail!("{STATS} changed as it was read");
    }
    Ok(text)
}

/// The merge's tree with STATS.md at its root as `blob`, a plain file: written from the merge's root
/// entries (`mktree`), reading no file of any tree.
fn with_stats(p: &Place, merge: &str, blob: &str) -> Result<String> {
    let root = p.git(&p.repo, &["ls-tree", "-z", &format!("{merge}^{{tree}}")])?;
    if !root.status.success() {
        bail!("the merge's tree could not be read");
    }
    let mut entries: Vec<Vec<u8>> = root
        .stdout
        .split(|b| *b == 0)
        .filter(|e| !e.is_empty())
        .filter(|e| e.splitn(2, |b| *b == b'\t').nth(1) != Some(STATS.as_bytes()))
        .map(<[u8]>::to_vec)
        .collect();
    entries.push(format!("100644 blob {blob}\t{STATS}").into_bytes());
    let input: Vec<u8> = entries
        .iter()
        .flat_map(|e| [&e[..], b"\0"].concat())
        .collect();
    let tree = fed(p, &["mktree", "-z"], &input)?;
    let changed = p.git(
        &p.repo,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--name-only",
            &format!("{merge}^{{tree}}"),
            &tree,
        ],
    )?;
    let changed: Vec<&[u8]> = changed
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    if !changed.is_empty() && changed != [STATS.as_bytes()] {
        bail!("the count's tree differs from the merge's in more than {STATS}");
    }
    Ok(tree)
}

/// The commit object `commit` with its tree `tree` in place of its own, every other byte kept:
/// the same parents, author, committer and message. An unsigned merge only.
fn retree(commit: &[u8], tree: &str) -> Result<Vec<u8>> {
    let first = commit
        .iter()
        .position(|b| *b == b'\n')
        .unwrap_or(commit.len());
    let headers_end = commit
        .windows(2)
        .position(|w| w == b"\n\n")
        .unwrap_or(commit.len());
    if !commit[..first].starts_with(b"tree ")
        || commit[..headers_end]
            .split(|b| *b == b'\n')
            .any(|l| l.starts_with(b"gpgsig"))
    {
        bail!("the merge is not an unsigned commit");
    }
    Ok([format!("tree {tree}").as_bytes(), &commit[first..]].concat())
}

/// `git <args>` in the repository with `input` on its standard input, which must succeed: its
/// output, trimmed. It reads no tree.
fn fed(p: &Place, args: &[&str], input: &[u8]) -> Result<String> {
    let mut c = p.git_command(&p.repo, args)?;
    c.stdin(Stdio::piped());
    let mut child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
        .context("git could not start")?;
    let mut stdin = child.stdin.take().context("git's input")?;
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let o = child.wait_with_output()?;
    let wrote = writer.join().map_err(|_| anyhow!("git's input"))?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    wrote?;
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// `git <args>` on the join tree's index with git's work tree an empty directory of its own, never
/// the tree: so git reads no file of the tree, and no attribute of it selects anything. Its output,
/// trimmed.
fn apart(p: &Place, t: &Tree, args: &[&str]) -> Result<String> {
    let empty = p.gates().join(format!("stats-empty-{}", t.slug));
    if std::fs::symlink_metadata(&empty).is_ok() {
        std::fs::remove_dir_all(&empty)?;
    }
    std::fs::create_dir_all(&empty)?;
    let e = empty.to_string_lossy().into_owned();
    let work = format!("--work-tree={e}");
    let mut all = vec!["-C", e.as_str(), work.as_str()];
    all.extend(args);
    let r = p.git_ok(&t.dir, &all);
    let _ = std::fs::remove_dir_all(&empty);
    r
}

/// The tree put back as the counter found it: every path as `before` held it, HEAD at `merge`, and
/// the index the merge's tree, none of it read through git. Then checked; an error when it cannot be
/// shown to be the merge again, which stops the join.
fn restore(p: &Place, t: &Tree, merge: &str, before: &Snapshot) -> Result<()> {
    let undo = (|| -> Result<()> {
        let now = stats_tree::snapshot(&t.dir)?;
        stats_tree::put_back(&t.dir, before, &now, |path| {
            let at = format!("{merge}:{}", String::from_utf8_lossy(path));
            let o = p.git(&p.repo, &["cat-file", "blob", &at])?;
            Ok(o.status.success().then_some(o.stdout))
        })?;
        if p.git_ok(&t.dir, &["rev-parse", "HEAD"])? != merge {
            p.git_ok(&t.dir, &["update-ref", "--no-deref", "HEAD", merge])?;
        }
        let want = p.git_ok(&p.repo, &["rev-parse", &format!("{merge}^{{tree}}")])?;
        if apart(p, t, &["write-tree"])? != want {
            let entry = p.git_ok(&p.repo, &["ls-tree", merge, "--", STATS])?;
            match entry
                .split_once('\t')
                .map(|(m, _)| m.split(' ').collect::<Vec<_>>())
            {
                Some(m) if m.len() == 3 => {
                    let info = format!("{},{},{STATS}", m[0], m[2]);
                    apart(p, t, &["update-index", "--add", "--cacheinfo", &info])?
                }
                _ => apart(p, t, &["update-index", "--force-remove", "--", STATS])?,
            };
        }
        Ok(())
    })();
    undo.context("a failed count could not be undone in the join's tree")?;
    let head = p.git_ok(&t.dir, &["rev-parse", "HEAD"])?;
    let index = apart(p, t, &["write-tree"])?;
    let want = p.git_ok(&p.repo, &["rev-parse", &format!("{merge}^{{tree}}")])?;
    let left = stats_tree::changed(before, &stats_tree::snapshot(&t.dir)?);
    if head != merge || index != want || !left.is_empty() {
        let left: Vec<String> = left.iter().map(|l| stats_tree::shown(l)).collect();
        bail!(
            "a failed count was undone, but the join's tree is not the merge again (HEAD {head}, index {index}, \
             left {}), so the join stops",
            left.join(", ")
        );
    }
    Ok(())
}
