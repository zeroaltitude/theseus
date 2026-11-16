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
//!   lock records, into `<state>/self/packages/<slug>`, which the gate's view
//!   binds as an overlay ([`Packages`]). The gate keeps no network.
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

use std::path::{Path, PathBuf};
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
    /// The tree's lock files are main's: its gate reads the machine's own cache, as always.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packages {
    pub dir: PathBuf,
    /// Whether cargo's packages are in it.
    pub cargo: bool,
    /// Whether npm's are.
    pub npm: bool,
}

impl Packages {
    /// Where the tree `slug`'s cache is, made or not.
    pub fn dir_of(p: &Place, slug: &str) -> PathBuf {
        p.packages().join(slug)
    }

    /// The cache `provide` made for the lock files `tree` has now, or none:
    /// what it holds is judged by the hash of each lock file, so a cache for
    /// another head's locks is not one.
    pub fn of(p: &Place, tree: &Tree) -> Option<Self> {
        let dir = Self::dir_of(p, &tree.slug);
        let ready: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("ready.json")).ok()?).ok()?;
        let held = |key: &str, lock: &str| {
            let now = lock_hash(&tree.dir, lock);
            now.is_some() && ready.get(key).and_then(|v| v.as_str()) == now.as_deref()
        };
        let (cargo, npm) = (held("cargo", CARGO_LOCK), held("npm", NPM_LOCK));
        (cargo || npm).then_some(Self { dir, cargo, npm })
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

/// sha256 of a lock file as the tree has it, or none when it has none.
fn lock_hash(dir: &Path, rel: &str) -> Option<String> {
    Some(hex::encode(Sha256::digest(
        std::fs::read(dir.join(rel)).ok()?,
    )))
}

/// A lock file of the tree and whether it differs from main's.
struct Locked {
    text: String,
    changed: bool,
}

/// What the tree's lock files are, and its tracked paths (for the manifest scan).
fn read_tree(
    p: &Place,
    tree: &Tree,
) -> anyhow::Result<(Option<Locked>, Option<Locked>, Vec<String>)> {
    let one = |rel: &str| -> Option<Locked> {
        let text = std::fs::read_to_string(tree.dir.join(rel)).ok()?;
        let main = p
            .git(&p.repo, &["show", &format!("origin/main:{rel}")])
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        let changed = main.is_none_or(|m| m.trim() != text.trim());
        Some(Locked { text, changed })
    };
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
    Ok((cargo, npm, tracked))
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
    let (cargo, npm, tracked) = match read {
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
    let t0 = Instant::now();
    let dir = Packages::dir_of(p, &tree.slug);
    let made = make(&dir, ends, home, cargo.is_some(), &parts).await;
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
    let held = |l: &Option<Locked>, rel: &str| l.as_ref().and_then(|_| lock_hash(&tree.dir, rel));
    let ready = serde_json::json!({
        "cargo": held(&cargo, CARGO_LOCK),
        "npm": held(&npm, NPM_LOCK),
    });
    if let Err(e) = std::fs::write(dir.join("ready.json"), ready.to_string()) {
        return infra(format!("the package cache could not be marked ready: {e}"));
    }
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
        write_cargo_home(dir, home).map_err(io)?;
        total.fetched += s.fetched;
        total.kept += s.kept;
    }
    if let Some(lock) = rest.next() {
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
fn write_cargo_home(dir: &Path, home: &Path) -> std::io::Result<()> {
    let ch = dir.join("cargo-home");
    std::fs::create_dir_all(&ch)?;
    let config = format!(
        "[source.crates-io]\nreplace-with = \"theseus-packages\"\n\n[source.theseus-packages]\ndirectory = \"{}\"\n",
        dir.join(VENDOR).display()
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
