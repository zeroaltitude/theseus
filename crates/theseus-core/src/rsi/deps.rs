//! Libraries for a self build (issue 20, "The builder cannot try a library the
//! machine does not already hold"; the owner's decision of 2026-11-15 17:46:56,
//! chat message 1571687985215308000: let the builder download libraries so it
//! can try them, and have any change that adds one stop at his card before it
//! joins). Design note: `docs/design/self-dependencies.md`.
//!
//! Four parts, and the line between them is the point:
//!
//! - **The builder may fetch** ([`builder`]): its commands reach the registries
//!   in [`hosts::HOSTS`] and nothing new, and what they download goes to a package
//!   cache of their own inside the item's build folder. Nothing else reads it.
//! - **The gate never takes a package from the builder** ([`provide`]): before a
//!   gate on a tree whose lock files differ from main's, the pipeline (this code, in
//!   the daemon) fetches exactly the packages those lock files name
//!   ([`fetch::Fetcher`]), from the registries, each checked against the checksum the
//!   lock records, into `<state>/self/packages/<key>` (named by the lock files'
//!   content, so every tree with the same lock files reads it), which the gate's
//!   view binds as an overlay ([`Packages`]). The gate keeps no network.
//! - **A dependency from anywhere else is refused** ([`policy`], [`lock`]): a git
//!   URL, another registry, a path that leaves the tree, a `[patch]`, a changed
//!   `[source]`.
//! - **A change of dependencies stops at the owner's card**
//!   (`join::dependencies`): the packages a branch adds, removes or changes, with
//!   their versions and where each comes from.
//!
//! Only text of the branch is read here. No cargo, npm, build script or package
//! code runs in the daemon.

pub mod builder;
pub mod fetch;
pub mod hosts;
pub mod lock;
pub mod policy;

#[cfg(test)]
mod stand_in;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_fetch;
#[cfg(test)]
mod tests_live;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use sha2::{Digest, Sha256};

use super::tree::{Place, Tree};
use fetch::{Fail, Fetcher};
use hosts::Endpoints;

/// Where the unpacked crates are, from a package cache's directory: inside cargo's home, so a
/// build that remaps `CARGO_HOME` out of the paths it embeds (`scripts/build.sh`, the install's)
/// takes the crates' with it, and no path of the daemon's state reaches an installed binary.
const VENDOR: &str = "cargo-home/vendor";
/// The workspace's cargo lock file, from the tree's root.
pub const CARGO_LOCK: &str = "Cargo.lock";
/// The cockpit's npm lock file, from the tree's root.
pub const NPM_LOCK: &str = "cockpit/package-lock.json";

/// What `provide` made for a tree.
#[derive(Debug)]
pub enum Provided {
    /// The tree's lock files are main's: its gate reads the cache kept for main's lock files
    /// when there is one, and the machine's own otherwise.
    Nothing,
    /// A cache for the lock files that differ.
    Ready(Packages),
    /// The gate must not run: the words the builder reads. `infra` marks a
    /// registry that did not answer, which is not the change's fault.
    Refused { why: String, infra: bool },
}

impl Provided {
    #[cfg(test)]
    pub(crate) fn is_nothing(&self) -> bool {
        matches!(self, Self::Nothing)
    }
}

/// The package cache a tree's gate reads: one directory, bound as an overlay
/// (a write lands in scratch and vanishes). It holds `cargo-home` (cargo's home,
/// whose config sends crates.io to `cargo-home/vendor`, each crate unpacked) and
/// `npm` (npm's cache, holding each tarball by its integrity).
#[derive(Debug, Clone)]
pub struct Packages {
    pub dir: PathBuf,
    /// Whether cargo's packages are in it.
    pub cargo: bool,
    /// Whether npm's are.
    pub npm: bool,
    _pin: Arc<Cache>,
}

impl PartialEq for Packages {
    fn eq(&self, other: &Self) -> bool {
        self.dir == other.dir && self.cargo == other.cargo && self.npm == other.npm
    }
}
impl Eq for Packages {}

/// The same handle owns construction and pins the cache for every reader. Pruning and
/// admission share this registry, so no deletion can slip between admission and ready.json.
#[derive(Debug, Default)]
struct Cache {
    fetching: tokio::sync::Mutex<()>,
}
static CACHES: Mutex<Option<HashMap<PathBuf, Weak<Cache>>>> = Mutex::new(None);

fn cache_for(dir: &Path) -> Arc<Cache> {
    let dir = theseus_tools::paths::canonical_best_effort(dir);
    let mut caches = CACHES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let caches = caches.get_or_insert_with(HashMap::new);
    caches.retain(|_, cache| cache.strong_count() > 0);
    if let Some(cache) = caches.get(&dir).and_then(Weak::upgrade) {
        return cache;
    }
    let cache = Arc::new(Cache::default());
    caches.insert(dir, Arc::downgrade(&cache));
    cache
}

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Packages {
    /// Where the cache for the lock files with this key is, made or not.
    pub fn dir_of(p: &Place, key: &str) -> PathBuf {
        p.packages().join(key)
    }

    /// Where the cache for `tree`'s lock files is, made or not.
    #[cfg(test)]
    pub(crate) fn dir_for(p: &Place, tree: &Tree) -> Option<PathBuf> {
        Some(Self::dir_of(p, &key_of(&tree.dir)?))
    }

    /// The cache `provide` made for the lock files `tree` has now, or none. The
    /// directory is named by the lock files' own content ([`key_of`]), so every build
    /// of the same lock files (the branch's gate, the planted revert's tests, the
    /// merge's gate, the install) reads one cache, and a cache made for other lock
    /// files is never found.
    pub fn of(p: &Place, tree: &Tree) -> Option<Self> {
        let dir = Self::dir_of(p, &key_of(&tree.dir)?);
        let pin = cache_for(&dir);
        let ready: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("ready.json")).ok()?).ok()?;
        let held = |k: &str| ready.get(k).and_then(serde_json::Value::as_bool) == Some(true);
        let (cargo, npm) = (held("cargo"), held("npm"));
        (cargo || npm).then_some(Self {
            dir,
            cargo,
            npm,
            _pin: pin,
        })
    }

    /// The variables the gate's commands are given for it, and no others:
    /// cargo's home (its config sends crates.io to the unpacked crates) and
    /// npm's cache.
    pub fn env(&self) -> Vec<(String, String)> {
        let at = |s: &str| self.dir.join(s).display().to_string();
        let mut env = Vec::new();
        if self.cargo {
            env.push(("CARGO_HOME".to_string(), at("cargo-home")));
        }
        if self.npm {
            env.push(("npm_config_cache".to_string(), at("npm")));
        }
        env
    }
}

/// The name of the cache for the lock files in `dir`: a sha256 over both, as the files are
/// (a missing one counts as empty), or none when neither exists.
fn key_of(dir: &Path) -> Option<String> {
    let read = |rel: &str| std::fs::read(dir.join(rel)).ok();
    let (cargo, npm) = (read(CARGO_LOCK), read(NPM_LOCK));
    (cargo.is_some() || npm.is_some())
        .then(|| key_from(&cargo.unwrap_or_default(), &npm.unwrap_or_default()))
}

/// [`key_of`], from the two files' bytes.
fn key_from(cargo: &[u8], npm: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update((cargo.len() as u64).to_le_bytes());
    h.update(cargo);
    h.update(npm);
    hex::encode(&h.finalize()[..16])
}

/// A lock file of the tree and whether it differs from main's.
struct Locked {
    text: String,
    changed: bool,
}

/// What `read_tree` found.
struct Read {
    cargo: Option<Locked>,
    npm: Option<Locked>,
    /// The tree's tracked paths, read only when a lock file differs (for the manifest scan).
    tracked: Vec<String>,
    /// The cache key of main's own lock files, which no prune takes away.
    main_key: Option<String>,
}

/// What the tree's lock files are, and its tracked paths (for the manifest scan).
fn read_tree(p: &Place, tree: &Tree) -> anyhow::Result<Read> {
    let main_bytes = |rel: &str| -> Option<Vec<u8>> {
        let o = p
            .git(&p.repo, &["show", &format!("origin/main:{rel}")])
            .ok()?;
        o.status.success().then_some(o.stdout)
    };
    let one = |rel: &str| -> Option<Locked> {
        let text = std::fs::read_to_string(tree.dir.join(rel)).ok()?;
        let main = main_bytes(rel).map(|b| String::from_utf8_lossy(&b).into_owned());
        let changed = main.is_none_or(|m| m.trim() != text.trim());
        Some(Locked { text, changed })
    };
    let (m_cargo, m_npm) = (main_bytes(CARGO_LOCK), main_bytes(NPM_LOCK));
    let main_key = (m_cargo.is_some() || m_npm.is_some())
        .then(|| key_from(&m_cargo.unwrap_or_default(), &m_npm.unwrap_or_default()));
    let (cargo, npm) = (one(CARGO_LOCK), one(NPM_LOCK));
    let tracked =
        if cargo.as_ref().is_some_and(|l| l.changed) || npm.as_ref().is_some_and(|l| l.changed) {
            p.git_ok(&tree.dir, &["ls-files"])?
                .lines()
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };
    Ok(Read {
        cargo,
        npm,
        tracked,
        main_key,
    })
}

/// Make the package cache the gate on `tree` reads, from the registries
/// themselves, if the tree's lock files differ from main's. What may not be
/// fetched is refused whatever the place; only a place with a sandbox (a daemon's)
/// fetches, because only a sandbox's view reads the cache: a test's stand-in gate
/// runs at L0 and no test ever reaches a registry.
pub async fn provide(p: &Place, tree: &Tree) -> Provided {
    let (ends, home) = (Endpoints::registries(), crate::config::expand("~"));
    provide_with(p, tree, &ends, &home, p.jail.is_some()).await
}

/// [`provide`] for the builds that run on a blocking thread (the planted revert's tests, the
/// install's build): the same fetch, waited for there. `Err` is the words the build is
/// refused with.
pub fn provide_blocking(p: &Place, tree: &Tree) -> anyhow::Result<Option<Packages>> {
    let run = provide(p, tree);
    let done = match tokio::runtime::Handle::try_current() {
        Ok(h) => h.block_on(run),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(run),
    };
    match done {
        Provided::Refused { why, .. } => anyhow::bail!("{why}"),
        Provided::Nothing => Ok(Packages::of(p, tree)),
        Provided::Ready(pk) => Ok(Some(pk)),
    }
}

/// [`provide`], from `ends`, with `home` the operator's HOME (where cargo's
/// advisory database is, which `cargo deny` in the gate reads). Without `fetch`
/// the lock files and manifests are judged and nothing is downloaded.
pub(crate) async fn provide_with(
    p: &Place,
    tree: &Tree,
    ends: &Endpoints,
    home: &Path,
    fetch: bool,
) -> Provided {
    let (pc, t) = (p.clone(), tree.clone());
    let read = tokio::task::spawn_blocking(move || read_tree(&pc, &t)).await;
    let Read {
        cargo,
        npm,
        tracked,
        main_key,
    } = match read {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return infra(format!("the tree's lock files could not be read: {e:#}")),
        Err(e) => return infra(format!("the tree's lock files could not be read: {e}")),
    };
    let cargo = cargo.filter(|l| l.changed);
    let npm = npm.filter(|l| l.changed);
    if cargo.is_none() && npm.is_none() {
        return Provided::Nothing;
    }
    let mut problems = policy::tree(&tree.dir, &tracked);
    let cargo_lock = cargo.as_ref().map(|l| lock::cargo(&l.text));
    let npm_lock = npm.as_ref().map(|l| lock::npm(&l.text, ends));
    let mut parts = Vec::new();
    for l in [cargo_lock, npm_lock].into_iter().flatten() {
        match l {
            Ok(l) => {
                problems.extend(l.problems.clone());
                parts.push(l);
            }
            Err(e) => problems.push(e),
        }
    }
    if !problems.is_empty() {
        let lines: String = problems.iter().map(|l| format!("\n- {l}")).collect();
        return refused(format!(
            "this branch changes a dependency, and it cannot be built as it stands: a dependency may come only from crates.io or the npm registry, and each must be named by a checksum.{lines}"
        ));
    }
    if !fetch {
        return Provided::Nothing;
    }
    fetch_cache(
        p,
        tree,
        ends,
        home,
        (cargo.is_some(), npm.is_some()),
        parts,
        main_key,
    )
    .await
}

async fn fetch_cache(
    p: &Place,
    tree: &Tree,
    ends: &Endpoints,
    home: &Path,
    wanted: (bool, bool),
    parts: Vec<lock::Lock>,
    main_key: Option<String>,
) -> Provided {
    let (cargo, npm) = wanted;
    let t0 = Instant::now();
    let Some(key) = key_of(&tree.dir) else {
        return infra("the tree's lock files vanished while its packages were fetched".into());
    };
    let dir = Packages::dir_of(p, &key);
    let pin = cache_for(&dir);
    let _fetch = pin.fetching.lock().await;
    let held = Packages::of(p, tree);
    let need_cargo = cargo && !held.as_ref().is_some_and(|pk| pk.cargo);
    let need_npm = npm && !held.as_ref().is_some_and(|pk| pk.npm);
    if !need_cargo && !need_npm {
        return held.map_or_else(
            || infra("the ready package cache disappeared".into()),
            Provided::Ready,
        );
    }
    // Fetch only absent parts into a private directory. Already-published parts are immutable:
    // a reader of cargo must not lose its files while a later call adds npm (or vice versa).
    let missing: Vec<_> = parts
        .into_iter()
        .enumerate()
        .filter_map(|(i, part)| {
            let is_cargo = cargo && i == 0;
            ((is_cargo && need_cargo) || (!is_cargo && need_npm)).then_some(part)
        })
        .collect();
    let stage = Staging(
        p.packages()
            .join(format!(".fetch-{}", crate::new_id("packages"))),
    );
    let _stage_pin = cache_for(&stage.0);
    let made = make(&stage.0, &dir, ends, home, need_cargo, &missing).await;
    let stats = match made {
        Ok(s) => s,
        Err(Fail::Refused(why)) => {
            return refused(format!(
                "a package this branch's lock files name could not be fetched: {why}"
            ))
        }
        Err(Fail::Infra(why)) => {
            return infra(format!(
                "a package this branch's lock files name could not be fetched: {why}"
            ))
        }
    };
    // What an earlier call for these same lock files made stays held: a cache is the lock
    // files' own, and a call that needed one part must not forget the other.
    let ready = serde_json::json!({
        "cargo": cargo || held.as_ref().is_some_and(|k| k.cargo),
        "npm": npm || held.as_ref().is_some_and(|k| k.npm),
    });
    let published = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        for (needed, name) in [(need_cargo, "cargo-home"), (need_npm, "npm")] {
            if !needed {
                continue;
            }
            let to = dir.join(name);
            // Only an unmarked part from an interrupted publication can be here.
            if to.exists() {
                std::fs::remove_dir_all(&to)?;
            }
            std::fs::rename(stage.0.join(name), to)?;
        }
        crate::config_change::write::atomic(&dir.join("ready.json"), &ready.to_string(), None)
    })();
    if let Err(e) = published {
        return infra(format!("the package cache could not be published: {e}"));
    }
    drop(stage);
    prune(p, &key, main_key.as_deref());
    tracing::info!(
        slug = %tree.slug,
        fetched = stats.fetched,
        kept = stats.kept,
        ms = t0.elapsed().as_millis() as u64,
        "self build: the packages this branch's lock files name are in their cache"
    );
    match Packages::of(p, tree) {
        Some(pk) => Provided::Ready(pk),
        None => infra("the package cache was made, and then did not match the lock files".into()),
    }
}

/// How many caches are kept besides the one just made and the one for main's lock files.
const KEPT_CACHES: usize = 3;

/// Remove every cache but `key`'s, main's, and the [`KEPT_CACHES`] most recently made: they are
/// regenerable from the registries, and the disk they take is a crate set each. Nothing here is
/// an error: a cache that will not go is tried at the next call.
fn prune(p: &Place, key: &str, main_key: Option<&str>) {
    let caches = CACHES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Ok(entries) = std::fs::read_dir(p.packages()) else {
        return;
    };
    let mut others: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            n != key && Some(n.to_string_lossy().as_ref()) != main_key
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    others.sort_by_key(|o| std::cmp::Reverse(o.0));
    for (_, path) in others.into_iter().skip(KEPT_CACHES) {
        let key = theseus_tools::paths::canonical_best_effort(&path);
        if caches
            .as_ref()
            .and_then(|c| c.get(&key))
            .is_some_and(|pin| pin.strong_count() > 0)
        {
            continue;
        }
        let _ = std::fs::remove_dir_all(path);
    }
}

fn refused(why: String) -> Provided {
    Provided::Refused { why, infra: false }
}

fn infra(why: String) -> Provided {
    Provided::Refused { why, infra: true }
}

/// Fetch into `dir`: the crates when the cargo lock changed (`cargo` is
/// true; the first of `parts` is then its packages), the tarballs when the npm
/// lock did. The cache's directory is made, and a stale ready mark is removed
/// first so an interrupted fetch is never read as a finished one.
async fn make(
    dir: &Path,
    final_dir: &Path,
    ends: &Endpoints,
    home: &Path,
    cargo: bool,
    parts: &[lock::Lock],
) -> Result<fetch::Stats, Fail> {
    let io = |e: std::io::Error| Fail::Infra(format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(dir).map_err(io)?;
    match std::fs::remove_file(dir.join("ready.json")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
        _ => {}
    }
    let fetcher = Fetcher::new(ends.clone())?;
    let mut total = fetch::Stats::default();
    let mut rest = parts.iter();
    if cargo {
        let lock = rest
            .next()
            .ok_or_else(|| Fail::Infra("no cargo packages were read".into()))?;
        let s = fetcher.cargo(&dir.join(VENDOR), &lock.pkgs).await?;
        write_cargo_home(dir, final_dir, home).map_err(io)?;
        total.fetched += s.fetched;
        total.kept += s.kept;
    }
    if let Some(lock) = rest.next() {
        std::fs::create_dir_all(dir.join("npm")).map_err(io)?;
        let s = fetcher.npm(&dir.join("npm"), &lock.pkgs).await?;
        total.fetched += s.fetched;
        total.kept += s.kept;
    }
    Ok(total)
}

/// Cargo's home for the gate: a config that sends crates.io to the unpacked
/// crates, and a link to the operator's advisory database (what `cargo deny`
/// reads from `CARGO_HOME`; the view's `~/.cargo` overlay is where it
/// resolves).
fn write_cargo_home(dir: &Path, final_dir: &Path, home: &Path) -> std::io::Result<()> {
    let ch = dir.join("cargo-home");
    std::fs::create_dir_all(&ch)?;
    let config = format!(
        "[source.crates-io]\nreplace-with = \"theseus-packages\"\n\n[source.theseus-packages]\ndirectory = \"{}\"\n",
        final_dir.join(VENDOR).display()
    );
    std::fs::write(ch.join("config.toml"), config)?;
    let link = ch.join("advisory-dbs");
    let _ = std::fs::remove_file(&link);
    let db = home.join(".cargo/advisory-dbs");
    if db.exists() {
        std::os::unix::fs::symlink(db, link)?;
    }
    Ok(())
}
