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
//!   waits until it says yes.
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
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use theseus_kernel::children::{self, Kind};

/// Where self branches are built: the repo they are cut from, the state dir
/// their trees live under, and any environment every git call and the gate
/// add (a test's hermetic git; none in a daemon).
#[derive(Debug, Clone)]
pub struct Place {
    pub repo: PathBuf,
    pub state: PathBuf,
    pub env: Vec<(OsString, OsString)>,
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
/// judged, the log kept, its seconds, and the log's last lines.
#[derive(Debug, Clone)]
pub struct GateRun {
    pub verdict: Verdict,
    pub exit: Option<i32>,
    pub blob: String,
    pub log: PathBuf,
    pub secs: u64,
    pub tail: String,
}

/// The longest gate run before it is stopped: a cold build of the whole
/// workspace at nice 19 beside busy neighbours took about 22 minutes.
pub const GATE_LIMIT: Duration = Duration::from_secs(2 * 3600);
/// How long the gate waits for main's may-build rule to say yes, and how
/// often it asks.
pub const MAY_BUILD_WAIT: Duration = Duration::from_secs(30 * 60);
const MAY_BUILD_EVERY: Duration = Duration::from_secs(60);
/// How the gate says the keel stopped it (gate.sh's `phase` names).
const KEEL_FAILED: &[&str] = &["gate: FAILED in keel", "gate: FAILED in door"];

impl Place {
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

    /// `git -C <dir> <args>`, as a child of the daemon's, never prompting.
    pub fn git(&self, dir: &Path, args: &[&str]) -> Result<Output> {
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
        let child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
            .context("git could not start")?;
        Ok(child.wait_with_output()?)
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
static REPO: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A new tree for `item` off the repo's `origin/main`, on a new
/// `self/<date>-<slug>` branch (with `-2`, `-3`… when the name is taken).
pub fn open(p: &Place, item: &str, now_ms: u64) -> Result<Tree> {
    let _one = REPO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let slug = slug(item);
    let dir = p.trees().join(&slug);
    if dir.exists() {
        bail!("a tree for {item} is open already at {}", dir.display());
    }
    let repo = std::fs::canonicalize(&p.repo)
        .with_context(|| format!("[self] repo {} is not there", p.repo.display()))?;
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

/// Commit everything the tree holds that is not committed, as Theseus, with
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
    p.git_ok(
        &tree.dir,
        &[
            "-c",
            "user.name=Theseus",
            "-c",
            "user.email=theseus@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--no-verify",
            "-m",
            &msg,
        ],
    )?;
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
pub async fn gate(p: &Place, tree: &Tree) -> Result<GateRun> {
    gate_with(p, tree, false).await
}

/// Main's gate on the tree with the benches too (`THESEUS_GATE_NO_BENCH`
/// unset): the self-join's gate on the merge (theseus-pw1q.9).
pub async fn gate_with_benches(p: &Place, tree: &Tree) -> Result<GateRun> {
    gate_with(p, tree, true).await
}

async fn gate_with(p: &Place, tree: &Tree, benches: bool) -> Result<GateRun> {
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
        may_build(p, tree, &may).await?;
    }
    std::fs::create_dir_all(p.gates())?;
    let log = p
        .gates()
        .join(format!("{}.log", tree.branch.replace('/', "-")));
    let t0 = Instant::now();
    let status = run_niced(p, tree, &script, &log, GATE_LIMIT, benches).await?;
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let verdict = if status == Some(0) {
        Verdict::Green
    } else if KEEL_FAILED.iter().any(|k| text.contains(k)) {
        Verdict::Keel
    } else {
        Verdict::Red
    };
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(30)..].join("\n");
    Ok(GateRun {
        verdict,
        exit: status,
        blob,
        log,
        secs: t0.elapsed().as_secs(),
        tail,
    })
}

/// Wait until main's may-build rule says yes, asking every minute, for at
/// most [`MAY_BUILD_WAIT`].
async fn may_build(p: &Place, tree: &Tree, script: &Path) -> Result<()> {
    let until = Instant::now() + MAY_BUILD_WAIT;
    let log = p.gates().join("may-build.log");
    loop {
        if run_niced(p, tree, script, &log, Duration::from_secs(120), false).await? == Some(0) {
            return Ok(());
        }
        if Instant::now() >= until {
            bail!(
                "main's may-build rule did not say yes in {} minutes",
                MAY_BUILD_WAIT.as_secs() / 60
            );
        }
        tokio::time::sleep(MAY_BUILD_EVERY).await;
    }
}

/// `bash <script>` in the tree at nice 19 (and the idle IO class where
/// `ionice` is), its output to `log`, stopped past `limit`. Its exit code.
async fn run_niced(
    p: &Place,
    tree: &Tree,
    script: &Path,
    log: &Path,
    limit: Duration,
    benches: bool,
) -> Result<Option<i32>> {
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
    match tokio::time::timeout(limit, child.wait()).await {
        Ok(s) => Ok(s?.code()),
        Err(_) => {
            // Its whole group: the build's children too.
            if let Some(pid) = pid {
                // SAFETY: a plain signal to the group this child leads.
                unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
            }
            let _ = child.wait().await;
            bail!(
                "the gate ran past {} minutes and was stopped",
                limit.as_secs() / 60
            )
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
