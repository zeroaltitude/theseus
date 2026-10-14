//! Sealed holdout sets (theseus-pw1q.5, seam S5): exams kept out of Theseus's
//! reach, so a harness that improves itself can learn the skill and never the
//! exam. Theseus gets a score, never an item: not a prompt, not an expected
//! answer, not a diff of one.
//!
//! **Out of reach, by the sandbox and the gate, not by trust.** `[self]
//! sealed_dir` is checked at load to sit outside every workspace root, the
//! state dir, the repository and every `[sandbox] ro_paths` entry
//! (`config/rsi.rs`). Then, three ways:
//! - the gate refuses any call that names it (`Guard`, run first in
//!   `ToolRuntime::refusal`, for every tool, in every place, at every
//!   posture: a path the plan resolves into it, symlinks followed, or its
//!   path written anywhere in the call's input, a shell string or a
//!   terminal's keys included);
//! - no L1 view shows it (it is a floor path, so `Sandbox` hides it from
//!   every job, MCP server and proposed extension, whatever binds it);
//! - the daemon's own calls below give back numbers only.
//!
//! An L0 job is the owner's own uid and runs a command the gate never parses,
//! so a command that builds the path from pieces gets past the gate at L0
//! (the floor's own limit, theseus-b5cl): L1, whose view hides the path, is
//! the boundary, as for the socket.
//!
//! **The two calls** the daemon gets ([`Sealed::put`], [`Sealed::score`]):
//! `put` writes an item in and returns an opaque [`ItemId`], never bytes;
//! `score` runs a candidate ([`Runner`]) over a set and returns [`Scores`]:
//! per-family counts and rates, cost, and an opaque id per failure (a keyed
//! hash, never the item's text). Every output passes the [`Filter`], which
//! refuses any string from the set, and every error is a fixed sentence.
//!
//! **The item** is a versioned JSON object: `{"v": 1, "task": "…", "check":
//! "…", "canary": "…"}`. `task` is what the candidate is asked and `check`
//! is the exam's check language over the reply (`check.rs`: `reply has`,
//! `reply lacks`, `has word`); `canary` is optional text that must never
//! come out of the scorer. Changing a set or the scorer is the owner's call:
//! the paths in `KEEL_PATHS` are what the keel guard should hold.

mod check;
mod daemon;
mod filter;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use theseus_tools::Plan;

pub use filter::Filter;

/// An item's size, at most.
pub const MAX_ITEM_BYTES: usize = 256 * 1024;

/// The item layout this build reads.
pub const ITEM_VERSION: u32 = 1;

/// What the maintainer should hold in the keel guard's `KEEL_FILES`: a change
/// to the sealed set's code or its tests is the owner's (the hard keel).
pub const KEEL_PATHS: &[&str] = &[
    "crates/theseus-core/src/rsi/sealed.rs",
    "crates/theseus-core/src/rsi/sealed/check.rs",
    "crates/theseus-core/src/rsi/sealed/filter.rs",
    "crates/theseus-core/src/rsi/sealed/tests.rs",
    "crates/theseus-core/src/rsi/sealed/tests_reach.rs",
];

/// An item's opaque id: a keyed hash of its bytes. It names an item in the
/// ledger and nothing of what it holds.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(String);

impl ItemId {
    /// The id as the ledger writes it: sixteen hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ItemId({})", self.0)
    }
}

/// Why a call failed. Every sentence is fixed: none carries an item's text,
/// a candidate's reply, or a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealedError {
    /// A set or family name is not 1 to 40 of `a-z 0-9 _ -`.
    BadName,
    /// The item is not a version-1 item, or its check cannot be scored.
    BadItem,
    /// The item is over [`MAX_ITEM_BYTES`].
    TooBig,
    /// The set holds no item.
    EmptySet,
    /// The sealed dir could not be read or written.
    Io,
    /// The scorer's output held a string from the set, and was withheld.
    Withheld,
}

impl fmt::Display for SealedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SealedError::BadName => {
                "a sealed set or family name must be 1 to 40 of a-z, 0-9, _ and -"
            }
            SealedError::BadItem => {
                "the sealed item is not a version-1 item with a check the scorer can read"
            }
            SealedError::TooBig => "the sealed item is over the size limit",
            SealedError::EmptySet => "the sealed set holds no item",
            SealedError::Io => "the sealed dir could not be read or written",
            SealedError::Withheld => {
                "the scorer's output held a string from the set and was withheld"
            }
        })
    }
}

impl std::error::Error for SealedError {}

/// What the candidate is asked, and what it answers. The runner sees the
/// task (it must) and is the only thing that does; its answer never leaves
/// the scorer.
pub struct Task<'a>(&'a str);

impl Task<'_> {
    pub fn text(&self) -> &str {
        self.0
    }
}

/// A candidate's answer to one task.
pub struct Answer {
    pub reply: String,
    pub cost_usd: f64,
}

/// A run that failed: a fixed fact, no text.
#[derive(Debug, Clone, Copy)]
pub struct RunFailed;

/// A candidate to score: it answers a task. self-exams brings the real one
/// (a scratch daemon of the candidate's build); this is the seam.
pub trait Runner {
    fn answer(&self, task: &Task<'_>) -> Result<Answer, RunFailed>;
}

/// A family's score.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyScore {
    pub items: u64,
    pub passed: u64,
    pub failed: u64,
    /// Runs that failed to answer at all (counted among `failed`).
    pub errors: u64,
    pub rate: f64,
}

/// A set's score: numbers and opaque ids, nothing else.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub set: String,
    pub items: u64,
    pub passed: u64,
    pub failed: u64,
    pub errors: u64,
    pub rate: f64,
    pub cost_usd: f64,
    pub families: BTreeMap<String, FamilyScore>,
    /// An opaque id per failure: a keyed hash of the set and the item's id,
    /// the same on every score, and no way back to the item without the key.
    pub failures: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemFile {
    v: u32,
    task: String,
    check: String,
    #[serde(default)]
    canary: Option<String>,
}

/// The sealed store. It holds a path and a key, reads and writes only under
/// that path, and has no call that returns an item.
pub struct Sealed {
    dir: PathBuf,
    key: OnceLock<[u8; 32]>,
    /// Items per set, read once and kept by `put` (health asks often).
    counts: Mutex<Option<BTreeMap<String, u64>>>,
    /// A put's writes, one at a time.
    writes: Mutex<()>,
}

impl Sealed {
    /// The sealed store at `dir`: no file is touched until a call needs it.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            key: OnceLock::new(),
            counts: Mutex::new(None),
            writes: Mutex::new(()),
        }
    }

    /// The paths a call may never name: the dir as configured and as it
    /// resolves.
    pub fn guard(&self) -> Guard {
        Guard::new(&self.dir)
    }

    /// Items per set, as counts only. Read from the dir once, then kept.
    pub fn counts(&self) -> BTreeMap<String, u64> {
        let mut g = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        g.get_or_insert_with(|| scan(&self.dir)).clone()
    }

    /// Write an item into `set` under `family`. Write-only: the answer is the
    /// item's opaque id, never bytes. The same bytes in the same set are the
    /// same id and write nothing twice.
    pub fn put(&self, set: &str, family: &str, item: &[u8]) -> Result<ItemId, SealedError> {
        name_ok(set)?;
        name_ok(family)?;
        if item.len() > MAX_ITEM_BYTES {
            return Err(SealedError::TooBig);
        }
        parse(item)?;
        let key = self.key()?;
        let id = item_id(&key, set, item);
        tracing::debug!(set, family, item = %id, "a sealed item is stored");
        let _w = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let fam = self.dir.join(set).join(family);
        mkdirs(&fam)?;
        let file = fam.join(format!("{}.item", id.0));
        if !file.exists() {
            write_new(&fam, &file, item)?;
            let mut g = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(c) = g.as_mut() {
                *c.entry(set.to_string()).or_insert(0) += 1;
            }
        }
        Ok(id)
    }

    /// Run `runner` over every item of `set` and return the numbers.
    pub fn score(&self, set: &str, runner: &dyn Runner) -> Result<Scores, SealedError> {
        name_ok(set)?;
        let key = self.key()?;
        let items = self.load(set)?;
        if items.is_empty() {
            return Err(SealedError::EmptySet);
        }
        let filter = Filter::new(
            items.iter().flat_map(Loaded::strings),
            items.iter().filter_map(|i| i.file.canary.clone()),
        );
        let mut out = Scores {
            set: set.to_string(),
            ..Scores::default()
        };
        for it in &items {
            let fam = out.families.entry(it.family.clone()).or_default();
            fam.items += 1;
            out.items += 1;
            let passed = match runner.answer(&Task(&it.file.task)) {
                Ok(a) => {
                    out.cost_usd += a.cost_usd.max(0.0);
                    it.check.holds(&a.reply)
                }
                Err(RunFailed) => {
                    fam.errors += 1;
                    out.errors += 1;
                    false
                }
            };
            if passed {
                fam.passed += 1;
                out.passed += 1;
            } else {
                fam.failed += 1;
                out.failed += 1;
                out.failures.push(failure_id(&key, set, &it.id));
            }
        }
        out.rate = rate(out.passed, out.items);
        for f in out.families.values_mut() {
            f.rate = rate(f.passed, f.items);
        }
        out.failures.sort();
        filter.clean(&out)?;
        tracing::info!(
            set,
            items = out.items,
            passed = out.passed,
            errors = out.errors,
            "a sealed set was scored"
        );
        Ok(out)
    }

    /// The set's items, by id.
    fn load(&self, set: &str) -> Result<Vec<Loaded>, SealedError> {
        let mut out = Vec::new();
        let sdir = self.dir.join(set);
        let Ok(fams) = std::fs::read_dir(&sdir) else {
            return Ok(out);
        };
        for fam in fams.flatten() {
            let Ok(ft) = fam.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let family = fam.file_name().to_string_lossy().into_owned();
            let Ok(files) = std::fs::read_dir(fam.path()) else {
                continue;
            };
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let Some(id) = name.strip_suffix(".item") else {
                    continue;
                };
                let bytes = std::fs::read(f.path()).map_err(|_| SealedError::Io)?;
                let (file, check) = parse(&bytes)?;
                out.push(Loaded {
                    id: id.to_string(),
                    family: family.clone(),
                    file,
                    check,
                });
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// The keyed hash's key: read from the sealed dir, made on first use.
    fn key(&self) -> Result<[u8; 32], SealedError> {
        if let Some(k) = self.key.get() {
            return Ok(*k);
        }
        mkdirs(&self.dir)?;
        let path = self.dir.join(".key");
        let key = match std::fs::read(&path) {
            Ok(b) if b.len() == 32 => b.try_into().map_err(|_| SealedError::Io)?,
            Ok(_) => return Err(SealedError::Io),
            Err(_) => {
                let mut k = [0u8; 32];
                std::fs::File::open("/dev/urandom")
                    .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut k))
                    .map_err(|_| SealedError::Io)?;
                write_new(&self.dir, &path, &k).or_else(|_| {
                    // Another put made it first.
                    std::fs::read(&path)
                        .map_err(|_| SealedError::Io)
                        .map(|_| ())
                })?;
                std::fs::read(&path)
                    .map_err(|_| SealedError::Io)?
                    .try_into()
                    .map_err(|_| SealedError::Io)?
            }
        };
        Ok(*self.key.get_or_init(|| key))
    }
}

struct Loaded {
    id: String,
    family: String,
    file: ItemFile,
    check: check::Check,
}

impl Loaded {
    /// The strings that must never come out: the task and the check's patterns.
    fn strings(&self) -> Vec<String> {
        let mut v = vec![self.file.task.clone()];
        v.extend(self.check.patterns().map(str::to_string));
        v
    }
}

fn parse(bytes: &[u8]) -> Result<(ItemFile, check::Check), SealedError> {
    let file: ItemFile = serde_json::from_slice(bytes).map_err(|_| SealedError::BadItem)?;
    if file.v != ITEM_VERSION || file.task.trim().is_empty() {
        return Err(SealedError::BadItem);
    }
    let check = check::Check::parse(&file.check).map_err(|_| SealedError::BadItem)?;
    Ok((file, check))
}

fn name_ok(n: &str) -> Result<(), SealedError> {
    let ok = !n.is_empty()
        && n.len() <= 40
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    ok.then_some(()).ok_or(SealedError::BadName)
}

fn rate(passed: u64, items: u64) -> f64 {
    if items == 0 {
        0.0
    } else {
        passed as f64 / items as f64
    }
}

type HmacSha = Hmac<Sha256>;

fn mac(key: &[u8; 32], parts: &[&[u8]]) -> String {
    let mut m = HmacSha::new_from_slice(key).expect("hmac takes any key length");
    for p in parts {
        m.update(p);
        m.update(&[0]);
    }
    hex::encode(&m.finalize().into_bytes()[..8])
}

fn item_id(key: &[u8; 32], set: &str, bytes: &[u8]) -> ItemId {
    ItemId(mac(key, &[b"item", set.as_bytes(), bytes]))
}

fn failure_id(key: &[u8; 32], set: &str, item: &str) -> String {
    format!(
        "f_{}",
        mac(key, &[b"failure", set.as_bytes(), item.as_bytes()])
    )
}

fn mkdirs(p: &Path) -> Result<(), SealedError> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)
        .map_err(|_| SealedError::Io)
}

/// Write a new file 0600 beside its name and move it into place.
fn write_new(dir: &Path, to: &Path, bytes: &[u8]) -> Result<(), SealedError> {
    let tmp = dir.join(format!(".tmp-{}-{:x}", std::process::id(), rand_tag()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|_| SealedError::Io)?;
    let r = f.write_all(bytes).and_then(|()| f.sync_all());
    drop(f);
    if r.is_err() || to.exists() {
        let _ = std::fs::remove_file(&tmp);
        return if r.is_err() {
            Err(SealedError::Io)
        } else {
            Ok(())
        };
    }
    std::fs::rename(&tmp, to).map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        SealedError::Io
    })
}

fn rand_tag() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    t ^ N.fetch_add(1, Ordering::Relaxed).rotate_left(32)
}

/// Items per set: the `.item` files two levels down.
fn scan(dir: &Path) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let Ok(sets) = std::fs::read_dir(dir) else {
        return out;
    };
    for set in sets.flatten() {
        if !set.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let mut n = 0u64;
        for fam in std::fs::read_dir(set.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if !fam.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            n += std::fs::read_dir(fam.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|f| f.file_name().to_string_lossy().ends_with(".item"))
                .count() as u64;
        }
        out.insert(set.file_name().to_string_lossy().into_owned(), n);
    }
    out
}

/// The sealed store for `cfg`: none while `[self] sealed_dir` is unset.
pub fn open(cfg: &crate::Config) -> Option<std::sync::Arc<Sealed>> {
    let dir = cfg.self_improve.sealed_dir.as_deref()?;
    Some(std::sync::Arc::new(Sealed::new(
        theseus_tools::paths::canonical_best_effort(&crate::config::expand(dir)),
    )))
}

/// The gate's guard for `cfg`: none while `[self] sealed_dir` is unset.
pub fn guard_of(cfg: &crate::Config) -> Option<Guard> {
    cfg.self_improve
        .sealed_dir
        .as_deref()
        .map(|d| Guard::new(&crate::config::expand(d)))
}

/// The gate's side: what refuses a call that reaches the sealed dir.
#[derive(Debug, Clone, Default)]
pub struct Guard {
    /// The dir as configured and as it resolves (symlinks followed).
    dirs: Vec<PathBuf>,
}

impl Guard {
    pub fn new(dir: &Path) -> Self {
        let mut dirs = vec![theseus_tools::paths::normalize(dir)];
        let canon = theseus_tools::paths::canonical_best_effort(dir);
        if !dirs.contains(&canon) {
            dirs.push(canon);
        }
        Self { dirs }
    }

    /// No sealed dir: nothing to guard.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_none(&self) -> bool {
        self.dirs.is_empty()
    }

    /// The paths guarded, for the L1 view: hidden whatever binds them.
    pub fn paths(&self) -> &[PathBuf] {
        &self.dirs
    }

    fn inside(&self, p: &Path) -> bool {
        self.dirs.iter().any(|d| theseus_tools::paths::within(p, d))
    }

    /// Why the gate refuses a plan: it reaches the sealed dir, by a resource
    /// (resolved, so a link into it is caught) or by a path or the dir's own
    /// text in its argv, steps, url or summary. No path in the answer.
    pub fn refusal(&self, plan: &Plan) -> Option<String> {
        if self.is_none() {
            return None;
        }
        let cwd = plan
            .resources
            .iter()
            .find(|r| r.access == theseus_tools::Access::Exec)
            .map(|r| theseus_tools::paths::canonical_best_effort(&r.path));
        let argvs = plan.argv.iter().chain(plan.steps.iter().flatten());
        let hit = plan
            .resources
            .iter()
            .any(|r| self.inside(&theseus_tools::paths::canonical_best_effort(&r.path)))
            || argvs
                .flatten()
                .any(|a| self.names(a) || self.argument_reaches(a, cwd.as_deref()))
            || plan.url.as_deref().is_some_and(|u| self.names(u));
        hit.then(|| REFUSAL.to_string())
    }

    /// Why the gate refuses a call's input: it writes the sealed dir's path
    /// anywhere, in a shell string or a terminal's keys too.
    pub fn refusal_of_input(&self, input: &serde_json::Value) -> Option<String> {
        if self.is_none() {
            return None;
        }
        let text = input.to_string();
        // JSON escapes a `/` never, but a `\"` or a `\\` could split a
        // needle; compare against the unescaped strings too.
        let hit = self.names(&text) || strings_of(input).any(|s| self.names(s));
        hit.then(|| REFUSAL.to_string())
    }

    /// Whether a command's argument is a path (`--opt=value` by its value)
    /// that resolves into the sealed dir from `cwd`, `..` and links followed.
    fn argument_reaches(&self, arg: &str, cwd: Option<&Path>) -> bool {
        let v = match arg.split_once('=') {
            Some((k, v)) if k.starts_with('-') => v,
            _ if arg.starts_with('-') => return false,
            _ => arg,
        };
        if v.is_empty() || !(v.starts_with(['/', '~', '.']) || v.contains('/')) {
            return false;
        }
        let expanded = crate::config::expand_home(v);
        let full = if expanded.is_absolute() {
            expanded
        } else if let Some(c) = cwd {
            c.join(expanded)
        } else {
            return false;
        };
        self.inside(&theseus_tools::paths::canonical_best_effort(&full))
    }

    /// Whether `text` holds a guarded path (or a relative path that resolves
    /// into one, from the dir's parent: no).
    fn names(&self, text: &str) -> bool {
        self.dirs.iter().any(|d| {
            d.to_str()
                .is_some_and(|d| !d.is_empty() && d != "/" && text.contains(d))
        })
    }
}

fn strings_of(v: &serde_json::Value) -> Box<dyn Iterator<Item = &str> + '_> {
    match v {
        serde_json::Value::String(s) => Box::new(std::iter::once(s.as_str())),
        serde_json::Value::Array(a) => Box::new(a.iter().flat_map(strings_of)),
        serde_json::Value::Object(o) => Box::new(o.values().flat_map(strings_of)),
        _ => Box::new(std::iter::empty()),
    }
}

/// The gate's word, with no path in it.
pub const REFUSAL: &str = "sealed: the sealed holdout set is out of reach of every tool and job; \
     the daemon scores a candidate against it and returns numbers only";

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_reach;
