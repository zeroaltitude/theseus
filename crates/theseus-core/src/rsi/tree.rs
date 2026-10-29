//! A self branch's tree (theseus-pw1q.8; seam S6): where Theseus works on
//! its own source, apart from everything else. self-pr builds in it, and
//! self-join and auto-revert reuse it, so its API is four calls and the
//! paths they keep:
//!
//! - [`open`] makes a git worktree off the repo's `origin/main` at
//!   `<state>/self/trees/<slug>`, on a new branch `self/<date>-<slug>`, with
//!   its own build tree at `<state>/self/target/<slug>`. Never the owner's
//!   tree, never the spine's `target/`, never sccache's shared cache.
//! - [`gate`] runs main's copy of the gate on it, through the door
//!   (scripts/AGENTS.md, "The door"): the blob `origin/main:scripts/gate.sh`,
//!   copied out of the tree and run with `THESEUS_GATE_ROOT` naming it, so the
//!   tree's own `gate.sh` judges nothing first, however it was edited. Its
//!   log and verdict are kept under `<state>/self/gates/`. The run is at the
//!   background priority (`nice -n 19`, and `ionice -c 3` where there is
//!   one), as a lane's gate is, with `THESEUS_GATE_NO_BENCH=1`, its own
//!   `CARGO_TARGET_DIR`, no compiler wrapper, and none of the daemon's own
//!   `THESEUS_*` variables. When main has `scripts/may-build.sh`, the gate
//!   waits until it says yes (that script is main's, and runs at L0).
//!   **The gate runs the branch's code in L1** (theseus-pw1q.21.1): a place
//!   made by a daemon carries a [`Jail`], and the gate, in the view
//!   `sandbox::self_build_view` gives it, can write only the tree and its
//!   build tree, reaches no network, and sees nothing of Theseus's floor or
//!   the sealed sets. A place with no jail (a test's stand-in gate) runs at L0.
//! - [`push`] pushes only to `refs/heads/self/*` on the named remote, never
//!   main and never forced, after the scrub: every line the branch adds and
//!   every commit message is read by the daemon's scrubber (each secret it
//!   holds, and the shapes of secrets), and by main's `scripts/scrub.sh`
//!   when main has one; a hit stops the push. It uses the credential git
//!   already has for that remote; this code reads none.
//! - [`close`] removes the worktree and its build tree (build caches are
//!   regenerable); the branch stays in the repo.
//!
//! Every git call is a child through `children::spawn`, blocking: callers on
//! the runtime run them on the blocking pool. The gate is a tokio child.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use theseus_kernel::children::{self, Kind};
use theseus_kernel::patience::{Cut, Patience};

use super::jail::{Jail, Job};

/// Where self branches are built: the repo they are cut from, the state dir
/// their trees live under, and any environment every git call and the gate
/// add (a test's hermetic git; none in a daemon).
#[derive(Debug, Clone)]
pub struct Place {
    pub repo: PathBuf,
    pub state: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    /// L1, where the branch's code runs: always in a daemon, none in a test
    /// that stands in for the gate.
    pub jail: Option<Jail>,
    /// Where the gate's inform notices go (a wait for may-build that has
    /// lasted a window). None in a test's place: no notice, never an error.
    pub notify: Option<Notify>,
    /// Whether the owner's stop has landed, asked each minute of a wait for
    /// may-build (and before the first ask). None in a test's place and for
    /// every caller that has no stop to read: the wait is then as it always
    /// was, for as long as the rule says no.
    pub stop: Option<Stop>,
}

/// A stop check: why the build must stop now (the kill switch's halt, the
/// session's execution stopped), or none. Read each minute of a wait for
/// may-build; it never bounds the wait by a clock.
#[derive(Clone)]
pub struct Stop(Arc<dyn Fn() -> Option<String> + Send + Sync>);

impl Stop {
    pub fn new(f: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    pub(super) fn check(&self) -> Option<String> {
        (self.0)()
    }
}

impl std::fmt::Debug for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stop")
    }
}

/// How main's gate on a tree ended: it ran, or the owner's stop landed while
/// it waited for may-build and it never started. A value of its own, not an
/// error: an error's type is recovered by a downcast, which any layer that
/// re-renders the error (`anyhow!("{e:#}")`, a `format!`) loses, and a stopped
/// build then reads as a failed one (theseus-tsfkm). Nothing reads a stop as a
/// verdict, and every caller has to say what a stop means to it.
#[derive(Debug)]
pub enum Gated {
    /// The gate ran, and this is what it said.
    Ran(GateRun),
    /// It did not run: the stop's reason.
    Stopped(String),
}

impl Gated {
    /// The run, for a caller with no halt of its own to make: a stop is its
    /// error, saying why.
    pub fn ran(self) -> Result<GateRun> {
        match self {
            Self::Ran(run) => Ok(run),
            Self::Stopped(why) => Err(anyhow!("stopped while waiting for may-build: {why}")),
        }
    }
}

/// How a wait for may-build ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Waited {
    /// The rule said yes.
    Yes,
    /// The stop check fired, with its reason: the build must not go on.
    Stopped(String),
}

/// A sink for one inform notice: its text in, a post to the operator out.
/// The daemon's is built from the core's outbox (`pr.rs`, `Board::place`).
#[derive(Clone)]
pub struct Notify(
    Arc<dyn Fn(String) + Send + Sync>,
    Option<Arc<dyn Fn(String) + Send + Sync>>,
);

impl Notify {
    pub fn new(f: impl Fn(String) + Send + Sync + 'static) -> Self {
        Self(Arc::new(f), None)
    }

    /// The sink for the halt notice alone: it pings and carries a Resume button
    /// (theseus-6s7ll). A sink with none sends the halt as it sends any notice.
    pub fn with_halt(mut self, f: impl Fn(String) + Send + Sync + 'static) -> Self {
        self.1 = Some(Arc::new(f));
        self
    }

    /// Send the halt notice.
    pub(crate) fn send_halt(&self, text: String) {
        (self.1.as_ref().unwrap_or(&self.0))(text);
    }
}

impl std::fmt::Debug for Notify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Notify")
    }
}

/// An open tree: its slug, its branch, where it is, and its build tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub slug: String,
    pub branch: String,
    pub dir: PathBuf,
    pub target: PathBuf,
}

/// The gate's verdict on a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Red,
    /// The keel guard failed it: it waits on the owner, never retried.
    Keel,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Green => "green",
            Verdict::Red => "red",
            Verdict::Keel => "keel",
        }
    }
}

/// One run of the gate: its verdict, its exit, the blob of main's gate that
/// judged, the log kept, its seconds, the log's last lines, and whether the
/// gate stopped at a phase it could not run (a tool it needs is not there), so
/// that it judged nothing about the code.
#[derive(Debug, Clone)]
pub struct GateRun {
    pub verdict: Verdict,
    pub exit: Option<i32>,
    pub blob: String,
    pub log: PathBuf,
    pub secs: u64,
    pub tail: String,
    pub unrunnable: bool,
}

/// How long a gate run may print nothing before it is stopped (theseus-x1zz8):
/// a cold build of the whole workspace at nice 19 beside busy neighbours took
/// about 22 minutes, and one that still prints is let run on, to 24 times
/// this (`theseus_kernel::patience`).
pub const GATE_SILENCE: Duration = Duration::from_secs(2 * 3600);
/// How often the operator is told that the gate is still waiting for main's
/// may-build rule to say yes. It is a notice interval, not a deadline: the
/// wait ends when the rule says yes, never on a clock (the owner, 2026-10-25
/// 13:58: "i want there to be precious few hard gates /anywhere/ in
/// theseus").
pub const MAY_BUILD_WAIT: Duration = Duration::from_secs(30 * 60);
const MAY_BUILD_EVERY: Duration = Duration::from_secs(60);
/// How long one evaluation of the may-build rule may take. A hard clock, and
/// deliberately so: the rule is a predicate polled in a loop, and the stop
/// check and the operator's notice both live in that loop, outside the run.
/// Bounding it on silence instead would let a rule script that prints - and
/// one that runs `git fetch` does print - hold the loop open for 24 times
/// this, and delay a stop aimed at the build by the same factor.
const RULE_LIMIT: Duration = Duration::from_secs(120);
/// How the gate says the keel stopped it (gate.sh's `phase` names).
const KEEL_FAILED: &[&str] = &["gate: FAILED in keel", "gate: FAILED in door"];
/// How the gate says a tool it needs is missing, so that a phase could not run
/// (gate.sh's `cockpit_modules`: no npm to install `cockpit/node_modules`). The
/// words are the gate's own and are matched whole; this is not a list of every
/// phase, and a phase that fails for another reason is the code's to answer for.
const TOOL_MISSING: &[&str] = &["and there is no npm to install it"];

/// Whether the gate said, in a line of its own (they begin `gate: `), that a
/// tool it needs is missing. A line of a test's output that quotes the words
/// does not count.
pub(super) fn could_not_run(text: &str) -> bool {
    text.lines()
        .filter(|l| l.starts_with("gate: "))
        .any(|l| TOOL_MISSING.iter().any(|m| l.contains(m)))
}

impl Place {
    /// L1 for the branch's code. A place with none runs a stand-in at L0
    /// in this crate's own tests; anywhere else it is an error, so a self
    /// build never runs outside L1.
    pub fn jail(&self) -> Result<Option<&Jail>> {
        match &self.jail {
            Some(j) => Ok(Some(j)),
            None if cfg!(test) => Ok(None),
            None => bail!("a self build runs in L1, and this place has no sandbox to run it in"),
        }
    }

    pub fn root(&self) -> PathBuf {
        self.state.join("self")
    }
    pub fn trees(&self) -> PathBuf {
        self.root().join("trees")
    }
    pub fn targets(&self) -> PathBuf {
        self.root().join("target")
    }
    pub fn gates(&self) -> PathBuf {
        self.root().join("gates")
    }
    pub fn reports(&self) -> PathBuf {
        self.root().join("reports")
    }

    /// `git -C <dir> <args>` ready to spawn, never prompting.
    pub(super) fn git_command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut c = Command::new("git");
        c.arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.env {
            c.env(k, v);
        }
        c
    }

    /// `git -C <dir> <args>`, as a child of the daemon's, never prompting.
    pub fn git(&self, dir: &Path, args: &[&str]) -> Result<Output> {
        let mut c = self.git_command(dir, args);
        let child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
            .context("git could not start")?;
        Ok(child.wait_with_output()?)
    }

    /// Who a self commit is by: the repo's `user.name` and `user.email`, which
    /// the switch-on sets so GitHub can show a self-join Verified. Each falls
    /// back to Theseus's own when the repo names none.
    pub fn identity(&self) -> (String, String) {
        let read = |key: &str, fallback: &str| {
            self.git_ok(&self.repo, &["config", "--get", key])
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| fallback.to_string())
        };
        (
            read("user.name", "Theseus"),
            read("user.email", "theseus@localhost"),
        )
    }

    /// [`Place::identity`] as the `-c` pairs that put it on a git call; they
    /// beat the repo's config, which is why they carry the repo's own.
    pub fn ident_args(&self) -> Vec<String> {
        let (name, email) = self.identity();
        vec![
            "-c".into(),
            format!("user.name={name}"),
            "-c".into(),
            format!("user.email={email}"),
        ]
    }

    /// `git` that must succeed: its stdout, trimmed.
    pub fn git_ok(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let o = self.git(dir, args)?;
        if !o.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    /// Main's copy of `path` (a blob of `origin/main`), if main has one.
    fn main_blob(&self, path: &str) -> Option<String> {
        self.git_ok(
            &self.repo,
            &[
                "rev-parse",
                "-q",
                "--verify",
                &format!("origin/main:{path}"),
            ],
        )
        .ok()
        .filter(|b| !b.is_empty())
    }

    /// Main's copy of `path` written out of every tree, under
    /// `<state>/self/gates/<name>-<blob>.sh`; none when main has none.
    fn main_script(&self, path: &str, name: &str) -> Result<Option<PathBuf>> {
        let Some(blob) = self.main_blob(path) else {
            return Ok(None);
        };
        let out = self.gates().join(format!("{name}-{blob}.sh"));
        if !out.exists() {
            std::fs::create_dir_all(self.gates())?;
            let o = self.git(&self.repo, &["cat-file", "blob", &blob])?;
            if !o.status.success() {
                bail!("main's {path} could not be read");
            }
            let tmp = out.with_extension("tmp");
            std::fs::write(&tmp, &o.stdout)?;
            std::fs::rename(&tmp, &out)?;
        }
        Ok(Some(out))
    }
}

/// An item's slug: lower case letters, digits and `-`, at most 40.
pub fn slug(item: &str) -> String {
    let mut s = String::new();
    for c in item.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s: String = s.trim_end_matches('-').chars().take(40).collect();
    match s.trim_end_matches('-') {
        "" => "item".into(),
        t => t.into(),
    }
}

/// `YYYYMMDD`, UTC, of `ms`.
fn date(ms: u64) -> String {
    let (y, m, d) = crate::wake::civil_from_days((ms / 86_400_000) as i64);
    format!("{y:04}{m:02}{d:02}")
}

/// The tree open for `item`, if there is one.
pub fn find(p: &Place, item: &str) -> Option<Tree> {
    let slug = slug(item);
    let trees = std::fs::canonicalize(p.trees()).unwrap_or_else(|_| p.trees());
    let dir = trees.join(&slug);
    if !dir.join(".git").exists() {
        return None;
    }
    let branch = p
        .git_ok(&dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()?;
    Some(Tree {
        target: p.targets().join(&slug),
        slug,
        branch,
        dir,
    })
}

/// One open or close of a tree at a time in this process: two `git worktree
/// add`s at once race for the repo's locks, and the loser leaves a tree half
/// made (the self pipeline's builds start together, theseus-pw1q.16). The
/// branch has no upstream (`--no-track`), so an open writes no config.
///
/// It guards the worktree list (`.git/worktrees/`) and the branch names an
/// open picks, and nothing the fetch writes (`refs/remotes/origin/*`,
/// `FETCH_HEAD`, the object store, which git locks itself). So the fetch is
/// taken **outside** it ([`refresh_main`] runs before [`open`] locks it): a
/// slow remote never holds up a [`close`], and an open reads `origin/main`
/// once, after the fetch, whichever commit that is (theseus-6vlr4).
static REPO: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The remote the door reads main from: the `origin/main` its refs name.
const DOOR_REMOTE: &str = "origin";

/// How long one fetch serves the cuts that follow it: a pass that opens
/// several trees (the backlog's night) fetches once, not once an item.
const FETCH_FRESH: Duration = Duration::from_secs(60);

/// When each repo's door last fetched, so a pass fetches once.
///
/// This lock guards only that list, and it is held across the fetch on
/// purpose: it makes the fetch single-flight, so the cuts of a pass that
/// start together wait for the one fetch and then all see it done, rather
/// than each fetching. The wait is bounded by [`super::join::git::FETCH_LIMIT`]. The
/// fetch cannot move out of it without a second marker (a fetch in flight)
/// that does the same job; a bounded wait on one fetch is the cheaper answer.
static FETCHED: std::sync::Mutex<Vec<(PathBuf, Instant)>> = std::sync::Mutex::new(Vec::new());

/// The bound on the door's fetch: [`super::join::git::FETCH_LIMIT`]; a test's place
/// shortens it with `THESEUS_SELF_FETCH_LIMIT_MS` in its own environment.
fn fetch_limit(p: &Place) -> Duration {
    let own = p
        .env
        .iter()
        .find(|(k, _)| k == "THESEUS_SELF_FETCH_LIMIT_MS")
        .and_then(|(_, v)| v.to_str()?.parse::<u64>().ok());
    match own {
        Some(ms) if cfg!(test) => Duration::from_millis(ms),
        _ => super::join::git::FETCH_LIMIT,
    }
}

/// Bring `origin/main` up to date before a tree is cut from it, so the
/// branch's base and the gate blob [`Place::main_script`] resolves are main's
/// now, not main as of the last green join (theseus-g5wxc). A failed fetch
/// (no network, a remote that is down) is an infrastructure fault and never
/// a reason to stop the build: the door goes on with the base it has, says
/// so (to the log and, through [`Place::notify`], to the owner), and tries
/// again at the next cut. The fetch is bounded ([`super::join::git::FETCH_LIMIT`]): a
/// remote that does not answer fails it like any other fault, with its
/// child killed and reaped, and the door goes on with the base it has. Called
/// without [`REPO`] held.
fn refresh_main(p: &Place, repo: &Path) {
    let mut seen = FETCHED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    seen.retain(|(_, at)| at.elapsed() < FETCH_FRESH);
    if seen.iter().any(|(r, _)| r == repo) {
        return;
    }
    match super::join::git::fetch_within(p, DOOR_REMOTE, fetch_limit(p)) {
        Ok(()) => seen.push((repo.to_path_buf(), Instant::now())),
        Err(e) => {
            tracing::warn!(
                repo = %repo.display(),
                "self door: fetch of {DOOR_REMOTE} failed, cutting from the base it has: {e:#}"
            );
            if let Some(n) = &p.notify {
                (n.0)(format!(
                    "The self door could not reach {DOOR_REMOTE}: this tree is cut from the base the \
                     repo already had, which may be behind main, and it will be judged by that base's \
                     gate script. The next cut tries again. ({e:#})"
                ));
            }
        }
    }
}

/// A new tree for `item` off the repo's `origin/main` (fetched first, once a
/// pass), on a new `self/<date>-<slug>` branch (with `-2`, `-3`… when the
/// name is taken).
pub fn open(p: &Place, item: &str, now_ms: u64) -> Result<Tree> {
    let repo = std::fs::canonicalize(&p.repo)
        .with_context(|| format!("[self] repo {} is not there", p.repo.display()))?;
    // Before [`REPO`], so a slow remote holds no close of another tree.
    refresh_main(p, &repo);
    let _one = REPO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let slug = slug(item);
    let dir = p.trees().join(&slug);
    if dir.exists() {
        bail!("a tree for {item} is open already at {}", dir.display());
    }
    p.git_ok(&repo, &["rev-parse", "--verify", "origin/main^{commit}"])
        .context("[self] repo has no origin/main to branch from")?;
    std::fs::create_dir_all(p.trees())?;
    let trees = std::fs::canonicalize(p.trees())?;
    if trees.starts_with(&repo) {
        bail!("the trees ({}) are inside the repo itself", trees.display());
    }
    let base = format!("self/{}-{slug}", date(now_ms));
    let mut branch = base.clone();
    let mut n = 1;
    while p
        .git(
            &repo,
            &[
                "rev-parse",
                "-q",
                "--verify",
                &format!("refs/heads/{branch}"),
            ],
        )?
        .status
        .success()
    {
        n += 1;
        branch = format!("{base}-{n}");
    }
    let dir = trees.join(&slug);
    let d = dir.to_string_lossy().to_string();
    p.git_ok(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--no-track",
            "-b",
            &branch,
            &d,
            "origin/main",
        ],
    )?;
    let target = p.targets().join(&slug);
    std::fs::create_dir_all(&target)?;
    Ok(Tree {
        slug,
        branch,
        dir,
        target,
    })
}

/// Move the tree to a fresh branch for another attempt, from where it is.
pub fn next_branch(p: &Place, tree: &mut Tree) -> Result<()> {
    let base = tree.branch.clone();
    let stem = match base.rsplit_once('-') {
        Some((s, n)) if n.chars().all(|c| c.is_ascii_digit()) && s.contains('-') => s.to_string(),
        _ => base.clone(),
    };
    let mut n = 2;
    loop {
        let b = format!("{stem}-{n}");
        let taken = p
            .git(
                &tree.dir,
                &["rev-parse", "-q", "--verify", &format!("refs/heads/{b}")],
            )?
            .status
            .success();
        if !taken {
            p.git_ok(&tree.dir, &["checkout", "-q", "-b", &b])?;
            tree.branch = b;
            return Ok(());
        }
        n += 1;
    }
}

/// Commit everything the tree holds that is not committed, as the repo's
/// identity ([`Place::identity`]), with
/// `subject` and `body`. Its head, or none when there was nothing to commit.
pub fn commit(p: &Place, tree: &Tree, subject: &str, body: &str) -> Result<Option<String>> {
    p.git_ok(&tree.dir, &["add", "-A"])?;
    let clean = p
        .git(&tree.dir, &["diff", "--cached", "--quiet"])?
        .status
        .success();
    if clean {
        return Ok(None);
    }
    let msg = format!("{subject}\n\n{body}\n");
    let ident = p.ident_args();
    let mut args: Vec<&str> = ident.iter().map(String::as_str).collect();
    args.extend([
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "--no-verify",
        "-m",
        &msg,
    ]);
    p.git_ok(&tree.dir, &args)?;
    Ok(Some(p.git_ok(&tree.dir, &["rev-parse", "HEAD"])?))
}

/// The commits the tree's branch holds past main, with their subjects.
pub fn commits(p: &Place, tree: &Tree) -> Result<Vec<(String, String)>> {
    let out = p.git_ok(&tree.dir, &["log", "--format=%H %s", "origin/main..HEAD"])?;
    Ok(out
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(h, s)| (h.to_string(), s.to_string()))
        .collect())
}

/// The branch's change against main: files, lines added and removed.
pub fn diffstat(p: &Place, tree: &Tree) -> Result<(u64, u64, u64)> {
    let out = p.git_ok(&tree.dir, &["diff", "--numstat", "origin/main...HEAD"])?;
    let (mut files, mut add, mut del) = (0, 0, 0);
    for l in out.lines() {
        let mut f = l.split_whitespace();
        files += 1;
        add += f.next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
        del += f.next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
    }
    Ok((files, add, del))
}

/// Run main's gate on the tree (the door), at the background priority.
pub async fn gate(p: &Place, tree: &Tree) -> Result<Gated> {
    gate_with(p, tree, false).await
}

/// Main's gate on the tree with the benches too (`THESEUS_GATE_NO_BENCH`
/// unset): the self-join's gate on the merge (theseus-pw1q.9).
pub async fn gate_with_benches(p: &Place, tree: &Tree) -> Result<Gated> {
    gate_with(p, tree, true).await
}

async fn gate_with(p: &Place, tree: &Tree, benches: bool) -> Result<Gated> {
    let (pc, t) = (p.clone(), tree.clone());
    let (script, may) = tokio::task::spawn_blocking(move || -> Result<_> {
        let script = pc
            .main_script("scripts/gate.sh", "gate")?
            .context("main has no scripts/gate.sh")?;
        let may = pc.main_script("scripts/may-build.sh", "may-build")?;
        // The gate's benches and lanes find their binaries at target/.
        let link = t.dir.join("target");
        if std::fs::symlink_metadata(&link).is_err() {
            std::os::unix::fs::symlink(&t.target, &link)?;
        }
        Ok((script, may))
    })
    .await??;
    let blob = script
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("gate-"))
        .unwrap_or_default()
        .to_string();
    if let Some(may) = may {
        if let Waited::Stopped(why) = may_build(p, tree, &may).await? {
            return Ok(Gated::Stopped(why));
        }
    }
    std::fs::create_dir_all(p.gates())?;
    let log = p
        .gates()
        .join(format!("{}.log", tree.branch.replace('/', "-")));
    let t0 = Instant::now();
    let status = match p.jail()? {
        Some(j) => jailed(j, p, tree, &script, &log, benches).await?,
        None => {
            run_niced(
                p,
                tree,
                &script,
                &log,
                Bound::Silence(GATE_SILENCE),
                benches,
            )
            .await?
        }
    };
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let verdict = if status == Some(0) {
        Verdict::Green
    } else if KEEL_FAILED.iter().any(|k| text.contains(k)) {
        Verdict::Keel
    } else {
        Verdict::Red
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut tail = lines[lines.len().saturating_sub(30)..].join("\n");
    if let Some(why) = crate::sandbox::supply_chain(&text).filter(|_| verdict != Verdict::Green) {
        tail = format!("{why}\n{tail}");
    }
    Ok(Gated::Ran(GateRun {
        verdict,
        exit: status,
        blob,
        log,
        secs: t0.elapsed().as_secs(),
        tail,
        unrunnable: verdict != Verdict::Green && could_not_run(&text),
    }))
}

/// Main's gate in L1: the same command line as `run_niced`'s, over the
/// self-build view, with the variables the gate reads and no others.
async fn jailed(
    jail: &Jail,
    p: &Place,
    tree: &Tree,
    script: &Path,
    log: &Path,
    benches: bool,
) -> Result<Option<i32>> {
    let mut argv: Vec<OsString> = vec!["nice".into(), "-n".into(), "19".into()];
    if on_path("ionice") {
        argv.extend(["ionice".into(), "-c".into(), "3".into()]);
    }
    argv.extend(["bash".into(), script.as_os_str().to_owned()]);
    let mut env = vec![(
        "THESEUS_GATE_ROOT".to_string(),
        tree.dir.to_string_lossy().into_owned(),
    )];
    if !benches {
        env.push(("THESEUS_GATE_NO_BENCH".into(), "1".into()));
    }
    let job = Job {
        tree,
        ro: vec![script.to_path_buf()],
        argv,
        env,
        log,
        limit: GATE_SILENCE,
    };
    jail.run(p, job)
        .await
        .map_err(|e| e.context("the gate did not run in L1, and a self build never runs at L0"))
}

/// What the wait for may-build measures time with: the real clock, or a
/// simulated one a test advances by its sleeps, so 90 minutes take no time.
#[derive(Clone)]
pub(super) enum Clock {
    Real(Instant),
    #[cfg(test)]
    Simulated(Arc<AtomicU64>),
}

impl Clock {
    /// A clock only its sleeps advance (a test's 90 minutes take no time).
    #[cfg(test)]
    pub(super) fn simulated() -> Self {
        Clock::Simulated(Arc::new(AtomicU64::new(0)))
    }

    fn elapsed(&self) -> Duration {
        match self {
            Clock::Real(t0) => t0.elapsed(),
            #[cfg(test)]
            Clock::Simulated(ms) => Duration::from_millis(ms.load(Ordering::SeqCst)),
        }
    }

    async fn sleep(&self, every: Duration) {
        match self {
            Clock::Real(_) => tokio::time::sleep(every).await,
            #[cfg(test)]
            Clock::Simulated(ms) => {
                ms.fetch_add(every.as_millis() as u64, Ordering::SeqCst);
                tokio::task::yield_now().await;
            }
        }
    }
}

/// Wait until main's may-build rule says yes, asking every minute and for as
/// long as it says no: its no is the machine busy (the owner gaming, trees
/// compiling, a disk low), and the right answer to that is to wait, never to
/// drop the item. Each [`MAY_BUILD_WAIT`] waited, one inform notice carries
/// the rule's last words. Nothing but the rule ends the wait except the
/// place's stop check, read each minute: it ends it as [`Waited::Stopped`].
async fn may_build(p: &Place, tree: &Tree, script: &Path) -> Result<Waited> {
    may_build_paced(
        p,
        tree,
        script,
        MAY_BUILD_EVERY,
        MAY_BUILD_WAIT,
        Clock::Real(Instant::now()),
    )
    .await
}

pub(super) async fn may_build_paced(
    p: &Place,
    tree: &Tree,
    script: &Path,
    every: Duration,
    notice_every: Duration,
    clock: Clock,
) -> Result<Waited> {
    std::fs::create_dir_all(p.gates())?;
    let log = p.gates().join("may-build.log");
    let mut told = 0u32;
    loop {
        if let Some(why) = p.stop.as_ref().and_then(Stop::check) {
            return Ok(Waited::Stopped(why));
        }
        // A hard clock, not silence: the stop check and the operator notice
        // both live outside this call (theseus-x1zz8 join review).
        if run_niced(p, tree, script, &log, Bound::Clock(RULE_LIMIT), false).await? == Some(0) {
            return Ok(Waited::Yes);
        }
        let waited = clock.elapsed();
        if waited >= notice_every * (told + 1) {
            told += 1;
            if let Some(n) = &p.notify {
                let reason = std::fs::read_to_string(&log).unwrap_or_default();
                let (n, text) = (n.clone(), wait_notice(&tree.slug, waited, &reason));
                // The sink writes a frame: the disk, not a runtime worker.
                let _ = tokio::task::spawn_blocking(move || (n.0)(text)).await;
            }
        }
        clock.sleep(every).await;
    }
}

/// The inform notice for a wait: the item, how long, and the rule's own last
/// line (cut to 200 characters), or a plain statement that it gave none.
fn wait_notice(slug: &str, waited: Duration, rule_log: &str) -> String {
    let reason = rule_log
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(200).collect::<String>());
    let said = match reason {
        Some(r) => format!("main's may-build rule says: {r}"),
        None => "main's may-build rule gave no reason".to_string(),
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        "A self build is waiting for the machine",
        &format!("{slug} has waited {} minutes", waited.as_secs() / 60),
        &format!("{said}. The item stays alive and builds when the rule says yes."),
    )
}

#[cfg(test)]
pub(super) fn wait_notice_for_test(slug: &str, log: &str) -> String {
    wait_notice(slug, Duration::from_secs(1800), log)
}

/// The phase a gate log was last in: its last `gate: ` line.
fn last_phase(log: &Path) -> Option<String> {
    let text = std::fs::read_to_string(log).ok()?;
    text.lines()
        .rev()
        .find(|l| l.starts_with("gate: "))
        .map(|l| l.chars().take(200).collect())
}

/// How a niced run is bounded (theseus-x1zz8, and its join review).
///
/// A build or a gate is bounded by `Silence`: it may take as long as it is
/// making noise, because a long quiet spell is the failure and the wall clock
/// says nothing about it. A **predicate** is bounded by `Clock`: the may-build
/// rule is polled in a loop whose stop check and operator notice both sit
/// outside this call, so a rule script that prints must not be able to hold
/// the loop open past its own interval. Silence there would turn a 120 s
/// window into a 48 minute one and delay a stop by the same factor.
pub(super) enum Bound {
    Silence(Duration),
    Clock(Duration),
}

impl Bound {
    fn limit(&self) -> Duration {
        match self {
            Self::Silence(d) | Self::Clock(d) => *d,
        }
    }
}

/// `bash <script>` in the tree at nice 19 (and the idle IO class where
/// `ionice` is), its output to `log`, stopped at its [`Bound`]. Its exit code.
pub(super) async fn run_niced(
    p: &Place,
    tree: &Tree,
    script: &Path,
    log: &Path,
    bound: Bound,
    benches: bool,
) -> Result<Option<i32>> {
    let limit = bound.limit();
    let out = std::fs::File::create(log)?;
    let err = out.try_clone()?;
    let mut argv: Vec<OsString> = vec!["nice".into(), "-n".into(), "19".into()];
    if on_path("ionice") {
        argv.extend(["ionice".into(), "-c".into(), "3".into()]);
    }
    argv.extend(["bash".into(), script.as_os_str().to_owned()]);
    let mut c = tokio::process::Command::new(&argv[0]);
    c.args(&argv[1..])
        .current_dir(&tree.dir)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .process_group(0)
        .kill_on_drop(true);
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("THESEUS_") {
            c.env_remove(&k);
        }
    }
    if !benches {
        c.env("THESEUS_GATE_NO_BENCH", "1");
    }
    c.env("THESEUS_GATE_ROOT", &tree.dir)
        .env("CARGO_TARGET_DIR", &tree.target)
        .env("RUSTC_WRAPPER", "")
        .env("CARGO_BUILD_RUSTC_WRAPPER", "")
        .env("GIT_TERMINAL_PROMPT", "0");
    for (k, v) in &p.env {
        c.env(k, v);
    }
    let mut child = children::spawn(Kind::Owned, || c.spawn(), |c| c.id())
        .context("the gate could not start")?;
    let pid = child.id();
    // The log's length is the gate's sign of life: the same bound as a
    // jailed run's, `theseus_kernel::patience` (theseus-x1zz8). A predicate
    // keeps the hard clock instead, so its caller's loop stays responsive.
    let ms = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
    let t0 = std::time::Instant::now();
    let mut patience = match bound {
        Bound::Clock(_) => Patience::clock(ms, t0),
        Bound::Silence(_) => {
            let sized = std::fs::File::open(log)?;
            Patience::silence(ms, t0, move || sized.metadata().map_or(0, |m| m.len()))
        }
    };
    loop {
        match tokio::time::timeout(patience.sleep(), child.wait()).await {
            Ok(s) => return Ok(s?.code()),
            Err(_) => {
                let Some(cut) = patience.check() else {
                    continue;
                };
                // Its whole group: the build's children too.
                if let Some(pid) = pid {
                    // SAFETY: a plain signal to the group this child leads.
                    unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
                }
                let _ = child.wait().await;
                let phase = last_phase(log).map_or_else(
                    || "no `gate: ` line in its log yet".to_string(),
                    |l| format!("`{l}`"),
                );
                let why = match cut {
                    Cut::Ceiling(_) => format!(
                        "was still printing at its ceiling, {} minutes,",
                        patience.ceiling_ms() / 60_000
                    ),
                    // A predicate's bound is the clock, so say the clock: it may
                    // have printed the whole way and still be past its window.
                    Cut::Clock => format!("ran for its whole {} s", limit.as_secs()),
                    Cut::Silent(_) => {
                        format!("printed nothing for {} minutes", limit.as_secs() / 60)
                    }
                };
                bail!("the gate {why} and was stopped; its log's last phase line: {phase}")
            }
        }
    }
}

fn on_path(prog: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|ps| std::env::split_paths(&ps).any(|d| d.join(prog).is_file()))
}

/// Why a push did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The branch is not under `self/`, or not a valid name.
    NotSelf(String),
    /// The scrub found what must not leave: how many, and by what check.
    Scrub { hits: u32, by: String },
    /// git refused, with its words.
    Git(String),
    /// The branch changes an exam item or the exam crate's data
    /// (`rsi::exams::PROTECTED`, the proposer rule of theseus-pw1q.11).
    Exams(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NotSelf(b) => write!(
                f,
                "{b} is not a self branch: a push goes only to refs/heads/self/*"
            ),
            Refused::Scrub { hits, by } => {
                write!(
                    f,
                    "the scrub found {hits} thing(s) that must not leave ({by})"
                )
            }
            Refused::Git(e) => write!(f, "git refused the push: {e}"),
            Refused::Exams(p) => write!(
                f,
                "{p} is an exam's: a self branch never changes the exams that judge it"
            ),
        }
    }
}

/// Whether `branch` may be pushed: `self/<name>`, a valid ref, no force.
pub fn is_self_branch(branch: &str) -> bool {
    let Some(rest) = branch.strip_prefix("self/") else {
        return false;
    };
    !rest.is_empty()
        && !branch.contains(['+', ':', ' ', '\\', '~', '^', '?', '*', '['])
        && !branch.contains("..")
        && !branch.ends_with('/')
        && !branch.ends_with(".lock")
}

/// Push the tree's `branch` to `remote`, at `refs/heads/<branch>` only,
/// after the scrub; `scrub` counts what must not leave in a text.
pub fn push(
    p: &Place,
    tree: &Tree,
    remote: &str,
    branch: &str,
    scrub: &dyn Fn(&str) -> u32,
) -> Result<String, Refused> {
    let git = |e: anyhow::Error| Refused::Git(format!("{e:#}"));
    if !is_self_branch(branch) {
        return Err(Refused::NotSelf(branch.into()));
    }
    let r = format!("refs/heads/{branch}");
    if !p
        .git(&tree.dir, &["check-ref-format", &r])
        .map_err(git)?
        .status
        .success()
    {
        return Err(Refused::NotSelf(branch.into()));
    }
    let changed = p
        .git_ok(
            &tree.dir,
            &["diff", "--name-only", &format!("origin/main...{r}")],
        )
        .map_err(git)?;
    if let Some(f) = changed
        .lines()
        .find(|f| crate::rsi::exams::touches_exams(f))
    {
        return Err(Refused::Exams(f.into()));
    }
    let hits = scrub(&outgoing(p, tree, branch).map_err(git)?);
    if hits > 0 {
        return Err(Refused::Scrub {
            hits,
            by: "the daemon's scrubber".into(),
        });
    }
    if let Some(script) = p.main_script("scripts/scrub.sh", "scrub").map_err(git)? {
        let mut c = Command::new("bash");
        c.arg(&script)
            .arg(&tree.dir)
            .current_dir(&tree.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let o = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
            .and_then(std::process::Child::wait_with_output)
            .map_err(|e| Refused::Git(format!("main's scrub.sh could not start: {e}")))?;
        if !o.status.success() {
            return Err(Refused::Scrub {
                hits: 1,
                by: "main's scripts/scrub.sh".into(),
            });
        }
    }
    let spec = format!("{r}:{r}");
    p.git_ok(&tree.dir, &["push", "-q", "--no-verify", remote, &spec])
        .map_err(git)?;
    Ok(spec)
}

/// What a push of `branch` would send: every commit message, and every line
/// the branch adds against main.
fn outgoing(p: &Place, tree: &Tree, branch: &str) -> Result<String> {
    let b = format!("refs/heads/{branch}");
    let mut out = p.git_ok(
        &tree.dir,
        &["log", "--format=%B", &format!("origin/main..{b}")],
    )?;
    let diff = p.git_ok(
        &tree.dir,
        &["diff", "--no-color", "-U0", &format!("origin/main...{b}")],
    )?;
    for l in diff.lines() {
        if let Some(added) = l.strip_prefix('+').filter(|_| !l.starts_with("+++")) {
            out.push('\n');
            out.push_str(added);
        }
    }
    Ok(out)
}

/// Remove the tree and its build tree; the branch stays in the repo.
pub fn close(p: &Place, tree: &Tree) -> Result<()> {
    let _one = REPO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let d = tree.dir.to_string_lossy().to_string();
    let link = tree.dir.join("target");
    if std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink()) {
        let _ = std::fs::remove_file(&link);
    }
    p.git_ok(&p.repo, &["worktree", "remove", "--force", &d])?;
    let _ = p.git(&p.repo, &["worktree", "prune"]);
    if tree.target.starts_with(p.targets()) && tree.target.exists() {
        std::fs::remove_dir_all(&tree.target)?;
    }
    Ok(())
}

/// How many commits `branch` holds beyond `origin/main`, the base a tree is
/// cut from.
pub fn commits_beyond_base(p: &Place, branch: &str) -> Result<u64> {
    let range = format!("origin/main..refs/heads/{branch}");
    let n = p.git_ok(&p.repo, &["rev-list", "--count", &range])?;
    n.trim()
        .parse()
        .with_context(|| format!("git counted {n:?} commits"))
}

/// Delete a `self/*` branch that holds nothing of its own (the caller has
/// counted); never another.
pub fn delete_branch(p: &Place, branch: &str) -> Result<()> {
    if !is_self_branch(branch) {
        bail!("{branch} is no self branch");
    }
    p.git_ok(&p.repo, &["branch", "-D", branch]).map(|_| ())
}

/// The `self/*` branches the repo holds, with their heads, newest name first.
pub fn branches(p: &Place) -> Result<Vec<(String, String)>> {
    let out = p.git_ok(
        &p.repo,
        &[
            "for-each-ref",
            "--format=%(refname:short) %(objectname)",
            "refs/heads/self/",
        ],
    )?;
    let mut v: Vec<(String, String)> = out
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(b, h)| (b.to_string(), h.to_string()))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(v)
}
