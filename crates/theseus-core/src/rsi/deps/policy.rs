//! Where a dependency may come from, read from the branch's own files as text.
//!
//! The lock files say what a build will use ([`super::lock`] refuses an entry
//! from anywhere but the registries). The manifests and cargo's own config say
//! where cargo would look for it, and a source cargo is told to use instead of
//! crates.io would make the lock file's checksum mean nothing, so those are read
//! too: a git URL, another registry, a path that leaves the tree, a `[patch]` or
//! `[replace]`, and a `[source]` or `[registries]` table in a config file are
//! each refused at the build with the reason (the owner's decision of
//! 2026-11-15, "A dependency from anywhere but those registries ... is refused
//! at the build with the reason, not fetched").
//!
//! No cargo runs here. `cargo metadata` reads the branch's config, which can
//! name a compiler to run, so the branch's code would run in the daemon.

use std::path::{Component, Path, PathBuf};

use toml::Value;

/// The tables whose entries are dependencies, wherever they sit (a package's,
/// a target's, the workspace's).
const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// A dependency's keys that name a source other than crates.io.
const OTHER_SOURCE_KEYS: [&str; 5] = ["git", "registry", "registry-index", "branch", "rev"];

/// The top-level tables of a cargo config file that change where packages
/// come from.
const CONFIG_TABLES: [&str; 4] = ["source", "registries", "patch", "paths"];

/// What is wrong with a `Cargo.toml` (`rel` is its path from the tree's root,
/// `text` its content), one sentence each, or none.
pub fn manifest(rel: &Path, text: &str) -> Vec<String> {
    let doc = match toml::from_str::<toml::Table>(text) {
        Ok(d) => d,
        Err(e) => return vec![format!("{}: it could not be read: {e}", rel.display())],
    };
    let mut out = Vec::new();
    // `[patch]` and `[replace]` stand beside the package's tables, and only there.
    let file = rel.display();
    if doc.contains_key("replace") {
        out.push(format!("{file}: a [replace] table is refused"));
    }
    if let Some(Value::Table(regs)) = doc.get("patch") {
        for (reg, v) in regs {
            if reg != "crates-io" {
                out.push(format!(
                    "{file}: [patch.\"{reg}\"] patches a source other than crates.io"
                ));
            }
            if let Value::Table(deps) = v {
                judge(rel, deps, &mut out);
            }
        }
    }
    walk(rel, &doc, &mut out);
    out
}

fn walk(rel: &Path, t: &toml::Table, out: &mut Vec<String>) {
    for (k, v) in t {
        let Value::Table(inner) = v else { continue };
        if DEP_TABLES.contains(&k.as_str()) {
            judge(rel, inner, out);
        }
        walk(rel, inner, out);
    }
}

/// Each entry of a dependencies table, judged.
fn judge(rel: &Path, deps: &toml::Table, out: &mut Vec<String>) {
    for (name, v) in deps {
        if let Some(why) = bad_dependency(rel, name, v) {
            out.push(format!("{}: {why}", rel.display()));
        }
    }
}

/// Why a dependency's entry names a source that is not crates.io, or `None`.
fn bad_dependency(rel: &Path, name: &str, v: &Value) -> Option<String> {
    let Value::Table(t) = v else { return None };
    if let Some(k) = OTHER_SOURCE_KEYS.iter().find(|k| t.contains_key(**k)) {
        return Some(format!(
            "`{name}` names a `{k}`: a dependency may come only from crates.io"
        ));
    }
    let path = t.get("path")?.as_str()?;
    let from = rel.parent().unwrap_or(Path::new(""));
    (!stays_inside(from, path)).then(|| {
        format!("`{name}` has the path `{path}`, which leaves the tree: a path dependency must stay inside it")
    })
}

/// Whether `path`, taken from the directory `from` (relative to the tree's
/// root), stays inside the tree. Lexical: a link in the tree is the review's.
fn stays_inside(from: &Path, path: &str) -> bool {
    let mut depth = 0usize;
    for c in from.join(path).components() {
        match c {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => match depth.checked_sub(1) {
                Some(d) => depth = d,
                None => return false,
            },
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

/// What is wrong with a cargo config file (`.cargo/config.toml` or
/// `.cargo/config`), one sentence each, or none.
pub fn config(rel: &Path, text: &str) -> Vec<String> {
    let doc = match toml::from_str::<toml::Table>(text) {
        Ok(d) => d,
        Err(e) => return vec![format!("{}: it could not be read: {e}", rel.display())],
    };
    CONFIG_TABLES
        .iter()
        .filter(|t| doc.contains_key(**t))
        .map(|t| {
            format!(
                "{}: a [{t}] table changes where cargo finds packages, which a branch may not do",
                rel.display()
            )
        })
        .collect()
}

/// Whether `path` (from the tree's root) is a manifest the scan reads.
pub fn is_manifest(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

/// Whether `path` is a cargo config file the scan reads.
pub fn is_config(path: &str) -> bool {
    [".cargo/config", ".cargo/config.toml"]
        .iter()
        .any(|c| path == *c || path.ends_with(&format!("/{c}")))
}

/// Every manifest and config file named by `tracked` (the tree's tracked
/// paths), read under `dir`, and what is wrong with them: the whole of the
/// scan. A file that cannot be read is a problem, never a pass.
pub fn tree(dir: &Path, tracked: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for rel in tracked {
        let read = |out: &mut Vec<String>| match std::fs::read_to_string(dir.join(rel)) {
            Ok(t) => Some(t),
            Err(e) => {
                out.push(format!("{rel}: it could not be read: {e}"));
                None
            }
        };
        let rel = PathBuf::from(rel);
        let name = rel.to_string_lossy().into_owned();
        if is_manifest(&name) {
            if let Some(t) = read(&mut out) {
                out.extend(manifest(&rel, &t));
            }
        } else if is_config(&name) {
            if let Some(t) = read(&mut out) {
                out.extend(config(&rel, &t));
            }
        }
    }
    out
}
