//! The count in the self-join's own merge (issue 45): every landing on main holds a STATS.md counted
//! from its own tree, the loop's joins too. Between the merge and main's gate, main's counter
//! (`scripts/stats.py --count-head`) writes STATS.md in the join's tree, and the merge is amended to
//! hold it, so the merge the gate judges, the signature signs and the push sends is the counted one.
//!
//! - **The counter is main's**, pinned to the join's base: the blob `<base>:scripts/stats.py`,
//!   frozen under `<state>/self/gates/`, never the branch's copy nor the merged tree's.
//! - **It runs in L1**, as every command in a join's tree does (`Place::jail`), with its time limit.
//! - **Containment is the whole tree**: after it, nothing but STATS.md may differ, untracked and
//!   ignored files included; the merge's tree against the new one must differ in STATS.md alone, a
//!   regular file (mode 100644, never a link); the fold keeps the merge's parents and message.
//! - **A failed count never blocks the join**: the tree is put back to the merge (HEAD, index,
//!   working files, untracked and ignored files), that is checked, and the join goes on uncounted,
//!   its row saying `skipped: <why>`. Only a tree that cannot be shown put back stops the join.
//!
//! A main with no counter joins as before. Before the count, the same counter's `--range` gives
//! the merge's change in lines for the notice (issue 45, line 2), checked by the same containment.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use theseus_kernel::children::{self, Kind};

use crate::rsi::jail::Job;
use crate::rsi::tree::{Place, Tree};
use crate::rsi::tree_git;

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
    let range = format!("{base}..HEAD");
    let lines = run(p, t, &frozen, &["--range", &range], limit)
        .ok()
        .and_then(|log| range_line(&log));
    match run(p, t, &frozen, &["--count-head"], limit).and_then(|_| fold(p, t, merge)) {
        Ok(Some(folded)) => Ok(Count {
            merge: folded,
            said: "counted".into(),
            lines,
        }),
        Ok(None) => Ok(Count {
            lines,
            ..kept("unchanged".into())
        }),
        Err(why) => {
            restore(p, t, merge)?;
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

/// `python3 -I <frozen> <args> --root <tree>` in L1, its output to a log of its own under the
/// gates; the log when it exited 0.
fn run(p: &Place, t: &Tree, frozen: &Path, args: &[&str], limit: Duration) -> Result<PathBuf> {
    let what = args[0].trim_start_matches('-');
    let log = p.gates().join(format!("stats-{what}-{}.log", t.slug));
    std::fs::create_dir_all(p.gates())?;
    let mut argv: Vec<OsString> = vec!["python3".into(), "-I".into(), frozen.into()];
    argv.extend(args.iter().map(OsString::from));
    argv.extend(["--root".into(), t.dir.clone().into()]);
    let code = match p.jail()? {
        Some(j) => j.run_blocking(
            p,
            &Job {
                tree: t,
                ro: vec![frozen.to_path_buf()],
                argv,
                env: Vec::new(),
                log: &log,
                limit,
            },
        )?,
        // Only a test's place has no jail (`Place::jail`): its stand-in counter runs at L0, as its
        // stand-in gate does.
        None => at_l0(p, t, &argv, &log, limit)?,
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

/// A test's stand-in for L1: the command in the tree, its whole group stopped at the limit.
fn at_l0(
    p: &Place,
    t: &Tree,
    argv: &[OsString],
    log: &Path,
    limit: Duration,
) -> Result<Option<i32>> {
    use std::os::unix::process::CommandExt;
    let out = std::fs::File::create(log)?;
    let mut c = std::process::Command::new(&argv[0]);
    c.args(&argv[1..])
        .current_dir(&t.dir)
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .process_group(0)
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
/// is the merge's still. An error when anything but STATS.md changed, or STATS.md is not a regular
/// file, or the fold is not the merge with the count; the caller puts the tree back.
fn fold(p: &Place, t: &Tree, merge: &str) -> Result<Option<String>> {
    let status = left(p, t)?;
    let others: Vec<&str> = status
        .iter()
        .map(String::as_str)
        .filter(|s| *s != STATS)
        .collect();
    if !others.is_empty() {
        bail!(
            "the count changed more than {STATS} ({}), so it was undone",
            others.join(", ")
        );
    }
    if status.is_empty() {
        return Ok(None);
    }
    p.git_ok(&t.dir, &["add", "--", STATS])?;
    let tree = p.git_ok(&t.dir, &["write-tree"])?;
    let was = format!("{merge}^{{tree}}");
    let changed = raw(
        p,
        t,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--name-only",
            &was,
            &tree,
        ],
    )?;
    let changed: Vec<&str> = changed.split('\0').filter(|s| !s.is_empty()).collect();
    if changed.is_empty() {
        return Ok(None);
    }
    if changed != [STATS] {
        bail!(
            "the count's tree differs from the merge's in more than {STATS} ({})",
            changed.join(", ")
        );
    }
    let entry = p.git_ok(&t.dir, &["ls-tree", &tree, "--", STATS])?;
    if !entry.starts_with("100644 blob ") {
        let mode = entry
            .split(' ')
            .next()
            .filter(|m| !m.is_empty())
            .unwrap_or("none");
        bail!("{STATS} is not a regular file in the count's tree (mode {mode}), so it was undone");
    }
    let ident = p.ident_args();
    let mut args: Vec<&str> = ident.iter().map(String::as_str).collect();
    args.extend([
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "--amend",
        "--no-edit",
        "--no-verify",
    ]);
    p.git_ok(&t.dir, &args)?;
    let folded = p.git_ok(&t.dir, &["rev-parse", "HEAD"])?;
    let parents = |c: &str| p.git_ok(&t.dir, &["rev-parse", &format!("{c}^@")]);
    let message = |c: &str| p.git_ok(&t.dir, &["log", "-1", "--format=%B", c]);
    let now = p.git_ok(&t.dir, &["rev-parse", &format!("{folded}^{{tree}}")])?;
    if parents(&folded)? != parents(merge)? || message(&folded)? != message(merge)? || now != tree {
        bail!("the folded merge {folded} is not {merge} with the count");
    }
    if !left(p, t)?.is_empty() {
        bail!("the tree is not clean after the fold");
    }
    Ok(Some(folded))
}

/// What the tree has beyond its HEAD: every changed, untracked and ignored path. A repository of
/// its own inside the tree is refused before git looks (`tree_git::nested_repo`).
fn left(p: &Place, t: &Tree) -> Result<Vec<String>> {
    if let Some(at) = tree_git::nested_repo(&t.dir) {
        bail!("the count left a repository of its own at {}", at.display());
    }
    let args = [
        "status",
        "--porcelain",
        "-z",
        "--no-renames",
        "--untracked-files=all",
        "--ignored",
    ];
    let status = raw(p, t, &args)?;
    Ok(status
        .split('\0')
        .filter(|e| !e.is_empty())
        .map(|e| e.get(3..).unwrap_or(e).to_string())
        .collect())
}

/// `git <args>` in the tree, which must succeed: its whole output, untrimmed (a porcelain entry
/// begins with its status, which may be a space).
fn raw(p: &Place, t: &Tree, args: &[&str]) -> Result<String> {
    let o = p.git(&t.dir, args)?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The tree put back to `merge`: HEAD, the index, the working files, and every untracked and ignored
/// file gone (its build tree is a directory apart, `Tree::target`). Then checked; an error when it
/// cannot be shown to be the merge again, which stops the join.
fn restore(p: &Place, t: &Tree, merge: &str) -> Result<()> {
    let undo = p
        .git_ok(&t.dir, &["reset", "-q", "--hard", merge])
        .and_then(|_| p.git_ok(&t.dir, &["clean", "-q", "-f", "-f", "-d", "-x"]));
    undo.context("a failed count could not be undone in the join's tree")?;
    let head = p.git_ok(&t.dir, &["rev-parse", "HEAD"])?;
    let index = p.git_ok(&t.dir, &["write-tree"])?;
    let want = p.git_ok(&t.dir, &["rev-parse", &format!("{merge}^{{tree}}")])?;
    let rest = left(p, t)?;
    if head != merge || index != want || !rest.is_empty() {
        bail!(
            "a failed count was undone, but the join's tree is not the merge again (HEAD {head}, index {index}, \
             left {}), so the join stops",
            rest.join(", ")
        );
    }
    Ok(())
}
