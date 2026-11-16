//! The lock files, read as the packages they name, and two things asked of
//! them: what a change did to the list ([`changes`], the owner's card), and
//! whether every entry is one the pipeline may fetch (`Lock::problems`, the
//! refusal a builder reads).
//!
//! Only text is read: no cargo, no npm and no code of the branch runs here.

use std::collections::{BTreeMap, BTreeSet};

use super::hosts::{Endpoints, CARGO_SOURCES};

/// One package a lock file names, from a registry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pkg {
    pub name: String,
    pub version: String,
    /// Where it comes from, in words: `crates.io`, `the npm registry`, or what
    /// the lock says when it is neither (which is refused).
    pub from: String,
    /// The checksum the lock records: cargo's sha256 as hex, npm's `integrity`.
    pub check: String,
    /// npm only: where the tarball is, as the lock says.
    pub url: String,
}

/// A lock file as read: its packages, and each entry that may not be fetched.
#[derive(Debug, Default)]
pub struct Lock {
    pub pkgs: Vec<Pkg>,
    pub problems: Vec<String>,
}

/// What cargo calls crates.io, said once.
pub const CRATES_IO: &str = "crates.io";
/// What the cockpit's lock file calls its registry, said once.
pub const NPM_REGISTRY: &str = "the npm registry";

/// The packages of a `Cargo.lock` that come from a source. A package with no
/// source is one of the workspace's own (or a path dependency, which
/// `policy` finds in the manifests), and is not here.
pub fn cargo(text: &str) -> Result<Lock, String> {
    let doc: toml::Table = toml::from_str(text).map_err(|e| format!("Cargo.lock: {e}"))?;
    let mut lock = Lock::default();
    let list = doc.get("package").and_then(toml::Value::as_array);
    for p in list.into_iter().flatten() {
        let s = |k: &str| p.get(k).and_then(toml::Value::as_str).unwrap_or_default();
        let (name, version, source) = (s("name"), s("version"), s("source"));
        if source.is_empty() {
            continue;
        }
        if !CARGO_SOURCES.contains(&source) {
            lock.problems.push(format!(
                "Cargo.lock: {name} {version} comes from `{source}`; only crates.io is allowed (no git, no other registry)"
            ));
            continue;
        }
        let word = |w: &str, extra: &[char]| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
        };
        if !word(name, &['-', '_']) || !word(version, &['.', '-', '+']) {
            lock.problems.push(format!(
                "Cargo.lock: `{name}` `{version}` is not a crate name and version"
            ));
            continue;
        }
        let check = s("checksum");
        if check.len() != 64 || !check.bytes().all(|b| b.is_ascii_hexdigit()) {
            lock.problems.push(format!(
                "Cargo.lock: {name} {version} has no sha256 checksum to check it against"
            ));
            continue;
        }
        lock.pkgs.push(Pkg {
            name: name.into(),
            version: version.into(),
            from: CRATES_IO.into(),
            check: check.to_ascii_lowercase(),
            url: String::new(),
        });
    }
    Ok(lock)
}

/// The integrity algorithms a package may be checked by.
const NPM_ALGOS: [&str; 3] = ["sha512", "sha384", "sha256"];

/// The integrity of an `integrity` field that may be checked: its first
/// sha512, sha384 or sha256 word (a field may carry several, and an old sha1 is
/// not one that is trusted).
fn npm_integrity(field: &str) -> Option<&str> {
    field
        .split_whitespace()
        .find(|w| NPM_ALGOS.iter().any(|a| w.starts_with(&format!("{a}-"))))
}

/// The packages of a `package-lock.json` (lockfile version 2 or 3).
pub fn npm(text: &str, ends: &Endpoints) -> Result<Lock, String> {
    let doc: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("package-lock.json: {e}"))?;
    let entries = doc
        .get("packages")
        .and_then(serde_json::Value::as_object)
        .ok_or("package-lock.json has no `packages` list (lockfile version 2 or 3): run `npm install` to rewrite it")?;
    let mut lock = Lock::default();
    for (key, e) in entries.iter().filter(|(k, _)| !k.is_empty()) {
        let s = |k: &str| {
            e.get(k)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        };
        let name = e
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| key.rsplit_once("node_modules/").map_or(key, |(_, n)| n));
        let (version, resolved, integrity) = (s("version"), s("resolved"), s("integrity"));
        let bad = |why: &str| format!("package-lock.json: {name} {version}: {why}");
        if e.get("link").and_then(serde_json::Value::as_bool) == Some(true) {
            lock.problems.push(bad("it is a link to a local path"));
        } else if resolved.is_empty()
            && e.get("inBundle").and_then(serde_json::Value::as_bool) == Some(true)
        {
            // Shipped inside another package's tarball: that package is the one fetched.
        } else if !resolved.starts_with(&format!("{}/", ends.npm)) {
            lock.problems.push(bad(&format!(
                "it comes from `{resolved}`; only {} is allowed",
                ends.npm
            )));
        } else if let Some(check) = npm_integrity(integrity) {
            lock.pkgs.push(Pkg {
                name: name.into(),
                version: version.into(),
                from: NPM_REGISTRY.into(),
                check: check.into(),
                url: resolved.into(),
            });
        } else {
            lock.problems.push(bad(
                "it has no sha512 (or sha384, sha256) integrity to check it against; run `npm install` to rewrite the lock file",
            ));
        }
    }
    Ok(lock)
}

/// A lock file's packages by name, each name's distinct entries.
fn group(pkgs: &[Pkg]) -> BTreeMap<&str, BTreeSet<&Pkg>> {
    let mut m: BTreeMap<&str, BTreeSet<&Pkg>> = BTreeMap::new();
    for p in pkgs {
        m.entry(p.name.as_str()).or_default().insert(p);
    }
    m
}

/// What a change did to the packages a lock file names, one line each: a
/// package added or removed, a version changed, or the same version with a
/// different source or checksum. `eco` is `cargo` or `npm`. A reorder, or a
/// change to a workspace member's own entry, is no line.
pub fn changes(eco: &str, old: &[Pkg], new: &[Pkg]) -> Vec<String> {
    let (before, after) = (group(old), group(new));
    let names: BTreeSet<&str> = before.keys().chain(after.keys()).copied().collect();
    let versions = |ps: &[&Pkg]| {
        ps.iter()
            .map(|p| p.version.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = Vec::new();
    for name in names {
        let (o, n) = (before.get(name), after.get(name));
        let none = BTreeSet::new();
        let (o, n) = (o.unwrap_or(&none), n.unwrap_or(&none));
        let gone: Vec<&Pkg> = o.difference(n).copied().collect();
        let came: Vec<&Pkg> = n.difference(o).copied().collect();
        let line = match (gone.first(), came.first()) {
            (None, None) => continue,
            (None, Some(c)) => format!("added {name} {}, from {}", versions(&came), c.from),
            (Some(g), None) => {
                format!(
                    "removed {name} {} (it came from {})",
                    versions(&gone),
                    g.from
                )
            }
            (Some(g), Some(c)) if versions(&gone) == versions(&came) => {
                let what = if g.from == c.from {
                    "checksum changed".to_string()
                } else {
                    format!("source changed, {} -> {}", g.from, c.from)
                };
                format!("{name} {}: same version, but its {what}", versions(&came))
            }
            (Some(_), Some(c)) => {
                format!(
                    "{name} {} -> {}, from {}",
                    versions(&gone),
                    versions(&came),
                    c.from
                )
            }
        };
        lines.push(format!("{eco}: {line}"));
    }
    lines
}
