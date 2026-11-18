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
//!
//! **Inside one package, finer** (issue 20, the second self join: a terminal test failed by
//! chance after a change to three other files of the same package, and nearly every change and
//! most tests are in that one package). A failed test of a package the change edits is still
//! outside when [`apart`] says so; see there for the line and why it is drawn where it is.

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
    /// The library's root is `src/lib.rs`: its manifest names no other path for it.
    plain_lib: bool,
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
        let plain_lib = m
            .get("lib")
            .is_none_or(|l| l.get("path").is_none() && l.get("name").is_none());
        found.push((
            Member {
                name,
                dir,
                deps: BTreeSet::new(),
                plain_lib,
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

/// A module name as Rust writes one: nothing else is looked for on the disk.
fn ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The modules a test's own module goes with, by the names this workspace gives them: the module
/// itself, and, for a test module `tests_a_b`, the modules `a_b` and `a` it tests and their test
/// modules `tests_a_b` and `tests_a`. A change to any of them is a change to what the test is
/// built from, as far as a name can say.
fn family(top: &str) -> Vec<String> {
    let base = top.strip_prefix("tests_").unwrap_or(top);
    let mut names = vec![top.to_string()];
    let mut end = base.len();
    loop {
        names.push(base[..end].to_string());
        names.push(format!("tests_{}", &base[..end]));
        match base[..end].rfind('_') {
            Some(i) if i > 0 => end = i,
            _ => break,
        }
    }
    names
}

/// The first module of a path under `src/`: `term/pty.rs` and `term.rs` are both `term`.
fn top_module(rel: &str) -> &str {
    let first = rel.split('/').next().unwrap_or(rel);
    first.strip_suffix(".rs").unwrap_or(first)
}

/// Whether a failed test is outside what `changed` reaches, judged more finely than by package
/// (`root` is the tree the paths are in). True when `outside` says so, and also when ALL of this
/// holds:
///
/// - the change brings no path in no package (that reaches everything, as above);
/// - the test is a unit test of its package's library (its binary id is the package's name: a
///   test binary of its own, or of a program, uses the whole package from outside and is judged
///   by package), and that package's library is rooted at `src/lib.rs`;
/// - no other package the change edits is one the test's package depends on, through any chain;
/// - every path the change brings inside that package is a Rust file under `src/`, and none is
///   the crate root (`lib.rs`, `main.rs`), a manifest, a build script, a fixture or any file that
///   is not source: those reach the whole package;
/// - the test's top module (`term` of `term::tests::x`) is a module of the library found on the
///   disk, as a file or a directory, and no changed path is in the module's own file or
///   subtree or in that of a module it goes with by name ([`family`]);
/// - no changed file is the target of a `#[path = "..."]` attribute anywhere in the library
///   ([`path_targets`]): such a file's module is named for the declaring module, not the file, so
///   what the names say about it is not read, and it reaches the whole package.
///
/// **Where the line is, and why.** A second look is cheap insurance on top of the real judge:
/// the failed test must pass when run again, and then the whole check runs once more and decides.
/// A change that deterministically breaks a test breaks it again, so the re-run holds it. What
/// this line lets through to that re-run is therefore only a failure that comes and goes, and the
/// line exists to keep it from being waved through when the change is the likely cause: the
/// test's own module and what it is named for. It is weaker than true reach: a change to module A
/// can break module B's test, and the compiler alone knows which code a test binary uses, which
/// this does not read (no call graph is built). For such a change to be joined its break would
/// have to fail once, then pass the test's own re-run, then pass the whole check: a fault that
/// shows itself less than once in three runs is the one thing this lets by, and a fault that
/// shows on every run is held. Anything this cannot read or place is inside.
pub fn apart(graph: &Graph, changed: &[String], binary: &str, test: &str, root: &Path) -> bool {
    let package = binary.split("::").next().unwrap_or_default();
    if outside(graph, changed, package) {
        return true;
    }
    let Some(m) = graph.members.iter().find(|m| m.name == binary) else {
        return false;
    };
    let Some((top, _)) = test.split_once("::") else {
        return false;
    };
    if !m.plain_lib || !ident(top) || graph.reached(changed).is_none() {
        return false;
    }
    let (mine, others): (Vec<&String>, Vec<&String>) = changed
        .iter()
        .partition(|p| p.strip_prefix(&m.dir).is_some_and(|r| r.starts_with('/')));
    let others: Vec<String> = others.into_iter().cloned().collect();
    if !outside(graph, &others, binary) {
        return false;
    }
    let src = root.join(&m.dir).join("src");
    let on_disk = |name: &str| {
        let kind =
            |p: std::path::PathBuf| std::fs::symlink_metadata(p).map(|m| (m.is_file(), m.is_dir()));
        kind(src.join(format!("{name}.rs"))).is_ok_and(|k| k.0)
            || kind(src.join(name)).is_ok_and(|k| k.1)
    };
    if !on_disk(top) {
        return false;
    }
    let Some(moved) = path_targets(&src) else {
        return false;
    };
    let family = family(top);
    let prefix = format!("{}/src/", m.dir);
    mine.iter().all(|p| {
        p.strip_prefix(&prefix).is_some_and(|rel| {
            rel.ends_with(".rs")
                && !matches!(rel, "lib.rs" | "main.rs")
                && !family.iter().any(|f| f == top_module(rel))
                && !moved.contains(rel.rsplit('/').next().unwrap_or(rel))
        })
    })
}

/// The most files of a library read for `#[path]`, and the largest of them.
const PATH_SCAN_FILES: usize = 20_000;
const PATH_SCAN_BYTES: u64 = 1 << 20;

/// The file names a `#[path = "..."]` attribute names anywhere under `src` (by their last
/// component, which is conservative: another file of that name elsewhere counts too). None when
/// the tree cannot be read whole: too many files, a file too large or not text, or a link.
fn path_targets(src: &Path) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let (mut todo, mut seen) = (vec![src.to_path_buf()], 0);
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).ok()? {
            let entry = entry.ok()?;
            let meta = std::fs::symlink_metadata(entry.path()).ok()?;
            if meta.is_dir() {
                todo.push(entry.path());
            } else if meta.is_file() && entry.path().extension().is_some_and(|e| e == "rs") {
                seen += 1;
                if seen > PATH_SCAN_FILES || meta.len() > PATH_SCAN_BYTES {
                    return None;
                }
                let text = std::fs::read_to_string(entry.path()).ok()?;
                for (at, _) in text.match_indices("#[path") {
                    let rest = &text[at + "#[path".len()..];
                    let quoted = rest.split_once('"').and_then(|(_, r)| r.split_once('"'));
                    if let Some((target, _)) = quoted.filter(|_| rest.find(']') > rest.find('"')) {
                        names.insert(target.rsplit('/').next().unwrap_or(target).to_string());
                    }
                }
            } else if !meta.is_file() {
                return None;
            }
        }
    }
    Some(names)
}

/// The paths of the tree's change against main, from main's merge base with it.
pub fn changed(p: &super::tree::Place, tree: &super::tree::Tree) -> Result<Vec<String>> {
    names(p, tree, "origin/main...HEAD")
}

/// The paths a merge brings to `base`, the main it was made on: the join's tree is that merge, and
/// what the change touches is read against the very commit it was merged into, never against a
/// ref that may have moved since.
pub fn changed_from(
    p: &super::tree::Place,
    tree: &super::tree::Tree,
    base: &str,
) -> Result<Vec<String>> {
    names(p, tree, &format!("{base}..HEAD"))
}

fn names(p: &super::tree::Place, tree: &super::tree::Tree, range: &str) -> Result<Vec<String>> {
    let o = p.git(
        &tree.dir,
        &["diff", "--name-only", "-z", "--no-renames", range],
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
