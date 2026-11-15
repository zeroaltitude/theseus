//! What a change can reach (issue 20: "a failure that is not the change's fault").
//!
//! A red gate whose only failures are tests the change cannot have affected is run once more on
//! those tests (`rerun`). "Cannot have affected" is read from the workspace's own dependency
//! graph, and **conservatively**: a test is outside only when its package neither is one the
//! diff edits nor depends on one, directly or through other workspace packages, in any of the
//! three dependency tables (normal, dev and build, under `[target.*]` too). Anything this cannot
//! read or place is inside.
//!
//! **The manifests are read, never `cargo metadata`.** Cargo's metadata reads the tree's own
//! `.cargo/config.toml`, which can name the compiler it runs, and the tree is the branch's: that
//! would run its code at L0, outside the sandbox every other line of a self build runs in. A
//! manifest is only parsed here, and only one that is itself a regular file.
//!
//! **A path in no package** (`Cargo.lock`, the root manifest, `scripts/`, `docs/`, the nextest
//! config) reaches everything: tests read the docs and the scripts, and the lock decides what
//! every package builds against. The change then reaches all of it and nothing is outside.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use anyhow::{bail, Context, Result};

/// A manifest larger than this is not read: no manifest is.
const MANIFEST_LIMIT: u64 = 1 << 20;

/// The dependency tables of a manifest, and of a `[target.*]` table.
const TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// One workspace package: its name, its directory (relative to the root, `/`-separated), and the
/// workspace packages it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    name: String,
    dir: String,
    deps: BTreeSet<String>,
}

/// The workspace's packages and their edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graph {
    members: Vec<Member>,
}

/// A dependency entry: the package it names, and its `path` when it has one.
type Entry = (String, Option<String>);

/// A manifest as a TOML table: a regular file inside the tree, of reasonable size.
fn manifest(root: &Path, dir: &str) -> Result<toml::Table> {
    let path = root.join(dir).join("Cargo.toml");
    let meta = std::fs::symlink_metadata(&path)
        .with_context(|| format!("{} is not there", path.display()))?;
    if !meta.is_file() || meta.len() > MANIFEST_LIMIT {
        bail!(
            "{} is not a regular manifest of a sane size",
            path.display()
        );
    }
    let text = std::fs::read_to_string(&path)?;
    text.parse::<toml::Table>()
        .with_context(|| format!("{} does not parse", path.display()))
}

/// The directories a `[workspace] members` entry names: a path, or `<dir>/*`. Anything else a
/// glob can say is not understood, and an unread workspace reaches everything.
fn expand(root: &Path, entry: &str) -> Result<Vec<String>> {
    let entry = entry.trim_end_matches('/');
    if entry.contains(['?', '[', '{']) || entry.split('/').any(|c| c.contains('*') && c != "*") {
        bail!("the workspace member {entry:?} is a glob this does not read");
    }
    if entry.split('/').any(|c| c == ".." || c.is_empty()) {
        bail!("the workspace member {entry:?} leaves the tree");
    }
    match entry.strip_suffix("/*") {
        Some(parent) if !parent.contains('*') => {
            let mut dirs: Vec<String> = std::fs::read_dir(root.join(parent))?
                .filter_map(Result::ok)
                .filter(|e| e.path().join("Cargo.toml").is_file())
                .filter_map(|e| Some(format!("{parent}/{}", e.file_name().to_str()?)))
                .collect();
            dirs.sort();
            Ok(dirs)
        }
        _ if entry.contains('*') => {
            bail!("the workspace member {entry:?} is a glob this does not read")
        }
        _ => Ok(vec![entry.to_string()]),
    }
}

/// `rel` joined to `dir` with its `.` and `..` resolved, as a `/` path; none when it leaves the
/// root.
fn joined(dir: &str, rel: &str) -> Option<String> {
    let mut out: Vec<String> = dir.split('/').map(str::to_string).collect();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(s) => out.push(s.to_str()?.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop()?;
            }
            _ => return None,
        }
    }
    Some(out.join("/"))
}

/// What a dependency entry is, resolved through `[workspace.dependencies]` when it says
/// `workspace = true`: the package it names (its `package` key, else its own key) and its path.
fn target(key: &str, spec: &toml::Value, workspace: &toml::Table) -> Entry {
    let table = spec.as_table();
    let inherited = table
        .and_then(|t| t.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let own = if inherited {
        workspace.get(key).and_then(toml::Value::as_table).or(table)
    } else {
        table
    };
    let name = own
        .and_then(|t| t.get("package"))
        .and_then(toml::Value::as_str)
        .unwrap_or(key);
    let path = own
        .and_then(|t| t.get("path"))
        .and_then(toml::Value::as_str);
    (name.to_string(), path.map(str::to_string))
}

/// Every dependency entry of a manifest, in every table, as `(name, path)`.
fn entries(m: &toml::Table, workspace: &toml::Table) -> Vec<Entry> {
    let mut tables: Vec<&toml::Table> = TABLES
        .iter()
        .filter_map(|t| m.get(*t).and_then(toml::Value::as_table))
        .collect();
    if let Some(targets) = m.get("target").and_then(toml::Value::as_table) {
        for cfg in targets.values().filter_map(toml::Value::as_table) {
            tables.extend(
                TABLES
                    .iter()
                    .filter_map(|t| cfg.get(*t).and_then(toml::Value::as_table)),
            );
        }
    }
    tables
        .into_iter()
        .flatten()
        .map(|(k, v)| target(k, v, workspace))
        .collect()
}

/// Read the workspace at `root` from its manifests.
pub fn read(root: &Path) -> Result<Graph> {
    let top = manifest(root, "")?;
    let ws = top
        .get("workspace")
        .and_then(toml::Value::as_table)
        .context("the root manifest has no [workspace]")?;
    let listed = ws
        .get("members")
        .and_then(toml::Value::as_array)
        .context("the workspace lists no members")?;
    let mut dirs = Vec::new();
    for m in listed {
        dirs.extend(expand(root, m.as_str().context("a member is no string")?)?);
    }
    let workspace = ws
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();
    let mut found: Vec<(Member, Vec<Entry>)> = Vec::new();
    for dir in dirs {
        let m = manifest(root, &dir)?;
        let name = m
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{dir} names no package"))?
            .to_string();
        let deps = entries(&m, &workspace);
        found.push((
            Member {
                name,
                dir,
                deps: BTreeSet::new(),
            },
            deps,
        ));
    }
    let by_dir: BTreeMap<String, String> = found
        .iter()
        .map(|(m, _)| (m.dir.clone(), m.name.clone()))
        .collect();
    let names: BTreeSet<String> = by_dir.values().cloned().collect();
    let members = found
        .into_iter()
        .map(|(mut m, deps)| {
            for (name, path) in deps {
                // By name, and by path: either one is an edge, so a rename cannot hide one.
                if names.contains(&name) {
                    m.deps.insert(name);
                }
                if let Some(p) = path.and_then(|p| joined(&m.dir, &p)) {
                    m.deps.extend(by_dir.get(&p).cloned());
                }
            }
            m
        })
        .collect();
    Ok(Graph { members })
}

impl Graph {
    /// Whether `package` is one of the workspace's.
    pub fn knows(&self, package: &str) -> bool {
        self.members.iter().any(|m| m.name == package)
    }

    /// The packages the change reaches: those a changed path is in, and every one that depends on
    /// one of them, through any chain. None when a changed path is in no package: that reaches
    /// everything.
    pub fn reached(&self, changed: &[String]) -> Option<BTreeSet<String>> {
        let mut reached = BTreeSet::new();
        for path in changed {
            let m = self.members.iter().find(|m| {
                path.strip_prefix(&m.dir)
                    .is_some_and(|r| r.starts_with('/'))
            })?;
            reached.insert(m.name.clone());
        }
        loop {
            let more: Vec<String> = self
                .members
                .iter()
                .filter(|m| {
                    !reached.contains(&m.name) && m.deps.iter().any(|d| reached.contains(d))
                })
                .map(|m| m.name.clone())
                .collect();
            if more.is_empty() {
                return Some(reached);
            }
            reached.extend(more);
        }
    }
}

/// Whether `package` is outside what `changed` reaches: a workspace package, and not one the
/// change reaches. Anything else, an unknown package included, is inside.
pub fn outside(graph: &Graph, changed: &[String], package: &str) -> bool {
    graph.knows(package)
        && graph
            .reached(changed)
            .is_some_and(|inside| !inside.contains(package))
}

/// The paths of the tree's change against main, from main's merge base with it.
pub fn changed(p: &super::tree::Place, tree: &super::tree::Tree) -> Result<Vec<String>> {
    let o = p.git(
        &tree.dir,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "origin/main...HEAD",
        ],
    )?;
    if !o.status.success() {
        bail!(
            "git could not list the change: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&o.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}
