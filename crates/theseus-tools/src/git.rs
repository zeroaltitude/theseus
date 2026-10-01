//! `git.diff` and `git.log` (spec §3.24), native through gitoxide: no git
//! binary, no PATH. `git.diff` compares a revision's tree with the working
//! tree (or two revisions) and produces unified diffs; there is no staged vs
//! unstaged split (that is `proc.run git diff --cached` until the fallback
//! ratio says otherwise). `git.log` walks history, optionally for one path.
//!
//! Neither reads above the root that holds its path (theseus-bsc). The gate
//! checks only that path, and discovery climbs from it to the nearest
//! repository, which may sit above the root: a root inside a larger
//! repository, or a stray one above a root that has none (a `~/.git`). Then
//! both tools are limited to the root's part of that repository, as a
//! pathspec, and the result's first line says so. Whatever the case, a path
//! the diff reads must be under the root and not on the floor; any other is
//! skipped and counted.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    parse, paths, Access, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

/// A repository as a call may read it (theseus-bsc).
struct Repo {
    repo: gix::Repository,
    /// Its working tree, canonical.
    wd: PathBuf,
    /// The root that holds the call's path.
    root: PathBuf,
    /// The root's path in the repository, when the working tree is above the
    /// root: both tools see only what is under it.
    prefix: Option<String>,
    /// The floor's paths (`ToolCtx::floor`).
    floor: Vec<PathBuf>,
}

/// Why the diff leaves a path out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Out {
    /// Not under the root: a tree path that is not plain names (`..`, `.`,
    /// or absolute), which git refuses to check out but a fetched tree can
    /// hold.
    Roots,
    /// On the floor: Theseus's own state, or the 1Password CLI's.
    Floor,
}

fn open(ctx: &ToolCtx, path: Option<&str>) -> Result<Repo, ToolFailure> {
    let dir = path
        .map(|p| ctx.resolve(p))
        .unwrap_or_else(|| ctx.cwd.clone());
    // The root that holds the path, the outermost where roots nest. A path
    // outside every root runs only once the operator approves it (the gate
    // asks for any such path), so it stands as its own root: what was
    // approved is what is read.
    let root = ctx
        .roots
        .iter()
        .filter(|r| paths::within(&dir, r))
        .min_by_key(|r| r.components().count())
        .cloned()
        .unwrap_or_else(|| dir.clone());
    let repo = gix::discover(&dir).map_err(|e| {
        ToolFailure::new(format!(
            "{} is not inside a git repository: {e}",
            dir.display()
        ))
    })?;
    let wd = repo
        .workdir()
        .map(paths::canonical_best_effort)
        .ok_or_else(|| ToolFailure::new("the repository has no working tree (bare)"))?;
    let prefix = if paths::within(&wd, &root) {
        None
    } else if let Ok(rel) = root.strip_prefix(&wd) {
        Some(rel.to_string_lossy().into_owned())
    } else {
        // Neither above the root nor in it: a `core.worktree` that names
        // another place.
        return Err(ToolFailure::new(format!(
            "the repository found from {} has its working tree at {}, outside {}: git.diff and \
             git.log read only under the roots",
            dir.display(),
            wd.display(),
            root.display()
        )));
    };
    Ok(Repo {
        repo,
        wd,
        root,
        prefix,
        floor: ctx.floor.clone(),
    })
}

impl Repo {
    /// The result's first line, when the tools are limited to the root.
    fn note(&self) -> Option<String> {
        self.prefix.as_ref().map(|_| {
            format!(
                "limited to {}, inside the repository at {}",
                self.root.display(),
                self.wd.display()
            )
        })
    }

    /// Whether the diff may read the tree path `p`: in the working tree,
    /// under the root, and not on the floor. Checked before anything at `p`
    /// is read, in the working tree or in history.
    fn check(&self, p: &str) -> Result<(), Out> {
        let rel = Path::new(p);
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(Out::Roots);
        }
        let at = self.wd.join(rel);
        if !paths::within(&at, &self.root) {
            return Err(Out::Roots);
        }
        if self.on_floor(&at) {
            return Err(Out::Floor);
        }
        Ok(())
    }

    fn on_floor(&self, at: &Path) -> bool {
        self.floor.iter().any(|f| paths::within(at, f))
    }

    /// The meta's account of what the call could not see.
    fn annotate(&self, meta: &mut Value, out: &NotShown) {
        if self.prefix.is_some() {
            meta["limited_to"] = json!(self.root);
        }
        if !out.roots.is_empty() || !out.floor.is_empty() {
            meta["not_shown"] = json!({"outside_roots": out.roots.len(), "floor": out.floor.len()});
        }
    }
}

/// The paths the diff left out, by why.
#[derive(Default)]
struct NotShown {
    roots: BTreeSet<String>,
    floor: BTreeSet<String>,
}

impl NotShown {
    fn add(&mut self, p: &str, why: Out) {
        match why {
            Out::Roots => self.roots.insert(p.to_string()),
            Out::Floor => self.floor.insert(p.to_string()),
        };
    }

    /// `2 paths outside the roots not shown`, and the floor's count.
    fn line(&self) -> Option<String> {
        let n = |k: usize| format!("{k} path{}", if k == 1 { "" } else { "s" });
        let mut parts = Vec::new();
        if !self.roots.is_empty() {
            parts.push(format!(
                "{} outside the roots not shown",
                n(self.roots.len())
            ));
        }
        if !self.floor.is_empty() {
            parts.push(format!(
                "{} on the floor (Theseus's own state) not shown",
                n(self.floor.len())
            ));
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

/// path → blob id, for every blob (and symlink) in a tree, or only in its
/// subtree at `prefix` (the root's part of a larger repository): nothing
/// beside that subtree is read.
fn tree_blobs(
    repo: &gix::Repository,
    rev: &str,
    prefix: Option<&str>,
) -> Result<BTreeMap<String, gix::ObjectId>, ToolFailure> {
    let id = repo
        .rev_parse_single(rev)
        .map_err(|e| ToolFailure::new(format!("unknown revision {rev:?}: {e}")))?;
    let commit = id
        .object()
        .map_err(|e| ToolFailure::new(e.to_string()))?
        .peel_to_commit()
        .map_err(|e| ToolFailure::new(format!("{rev} is not a commit: {e}")))?;
    let tree = commit.tree().map_err(|e| ToolFailure::new(e.to_string()))?;
    let tree = match prefix {
        None => tree,
        Some(pre) => match tree
            .lookup_entry_by_path(pre)
            .map_err(|e| ToolFailure::new(e.to_string()))?
        {
            Some(e) if e.mode().is_tree() => repo
                .find_tree(e.object_id())
                .map_err(|e| ToolFailure::new(e.to_string()))?,
            // The root's part is not a directory at this revision.
            _ => return Ok(BTreeMap::new()),
        },
    };
    let mut rec = gix::traverse::tree::Recorder::default();
    tree.traverse()
        .breadthfirst(&mut rec)
        .map_err(|e| ToolFailure::new(e.to_string()))?;
    let mut out = BTreeMap::new();
    for e in rec.records {
        if e.mode.is_blob_or_symlink() {
            let p = e.filepath.to_string();
            out.insert(
                match prefix {
                    Some(pre) => format!("{pre}/{p}"),
                    None => p,
                },
                e.oid,
            );
        }
    }
    Ok(out)
}

fn blob_text(repo: &gix::Repository, id: gix::ObjectId) -> Result<Option<String>, ToolFailure> {
    let obj = repo
        .find_object(id)
        .map_err(|e| ToolFailure::new(e.to_string()))?;
    let data = obj.data.clone();
    if data.iter().take(8192).any(|b| *b == 0) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&data).into_owned()))
}

fn worktree_blob_id(data: &[u8]) -> Option<gix::ObjectId> {
    gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, data).ok()
}

/// What the working tree holds at the tree path `p`, read as git reads it
/// (theseus-skc): a file's bytes, or a symbolic link's target as its bytes,
/// never what the link points at. A link's text is its blob in git, so an
/// unchanged link compares equal and a retargeted one diffs as text. A path
/// under a directory that is itself a link is not in the working tree, as
/// git sees it ("beyond a symbolic link"). The roots and the floor are
/// checked on the repository's path only, so a link committed in a cloned
/// repository (`notes -> ~/.ssh/id_ed25519`) or one into the state dir would
/// otherwise put its target's text in the diff, the model's context, and
/// the store. `None`: nothing there that git would read. `real_dirs`
/// remembers each leading directory checked, across one diff's paths.
fn worktree_bytes(wd: &Path, p: &str, real_dirs: &mut BTreeMap<PathBuf, bool>) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let rel = Path::new(p);
    // Plain names only: a crafted tree's `..` or absolute entry, which git
    // refuses to check out, would take `wd.join` out of the working tree.
    if !rel
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    let mut dir = wd.to_path_buf();
    for part in rel.parent().into_iter().flat_map(Path::components) {
        dir.push(part);
        let real = *real_dirs
            .entry(dir.clone())
            .or_insert_with(|| fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()));
        if !real {
            return None;
        }
    }
    let path = wd.join(rel);
    let meta = fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_symlink() {
        return fs::read_link(&path)
            .ok()
            .map(|t| t.as_os_str().as_bytes().to_vec());
    }
    if !meta.is_file() {
        return None;
    }
    fs::read(&path).ok()
}

fn file_diff(
    path: &str,
    old: Option<&str>,
    new: Option<&str>,
    context: usize,
) -> (String, usize, usize) {
    let (a, b) = (old.unwrap_or(""), new.unwrap_or(""));
    let d = similar::TextDiff::from_lines(a, b);
    let (mut add, mut del) = (0usize, 0usize);
    for c in d.iter_all_changes() {
        match c.tag() {
            similar::ChangeTag::Insert => add += 1,
            similar::ChangeTag::Delete => del += 1,
            _ => {}
        }
    }
    let an = if old.is_some() {
        format!("a/{path}")
    } else {
        "/dev/null".into()
    };
    let bn = if new.is_some() {
        format!("b/{path}")
    } else {
        "/dev/null".into()
    };
    let body = d
        .unified_diff()
        .context_radius(context)
        .header(&an, &bn)
        .to_string();
    (format!("diff --git a/{path} b/{path}\n{body}"), add, del)
}

// ---------------------------------------------------------------- git.diff

pub struct Diff;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    rev: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    context: Option<usize>,
    #[serde(default)]
    stat: bool,
    #[serde(default)]
    untracked: bool,
}

impl Tool for Diff {
    fn name(&self) -> &'static str {
        "git.diff"
    }
    fn description(&self) -> &'static str {
        "Show changes in a git repository as unified diffs: the working tree against a revision (default HEAD), or `A..B` between two revisions. Filter with paths (prefixes); stat=true gives per-file +/- counts only; untracked=true includes new untracked files."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "A directory inside the repository. Default: the working directory."},
                "rev": {"type": "string", "description": "Revision to compare the working tree against (default HEAD), or A..B for two revisions."},
                "paths": {"type": "array", "items": {"type": "string"}, "description": "Only files under these repository-relative prefixes."},
                "context": {"type": "integer", "minimum": 0, "maximum": 20, "description": "Context lines. Default 3."},
                "stat": {"type": "boolean", "description": "Per-file +/- counts instead of diffs."},
                "untracked": {"type": "boolean", "description": "Include untracked files (working-tree mode)."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: DiffArgs = parse(input)?;
        let dir = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        Ok(Plan {
            summary: format!(
                "git diff {} in {}",
                a.rev.as_deref().unwrap_or("HEAD"),
                dir.display()
            ),
            resources: vec![Resource {
                path: dir,
                access: Access::Read,
            }],
            argv: None,
            url: None,
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: DiffArgs = parse(input).map_err(ToolFailure::new)?;
        let r = open(ctx, a.path.as_deref())?;
        let (repo, wd) = (&r.repo, &r.wd);
        let context = a.context.unwrap_or(3);
        let rev = a.rev.clone().unwrap_or_else(|| "HEAD".into());
        let keep = |p: &str| {
            a.paths.is_empty()
                || a.paths
                    .iter()
                    .any(|pre| p.starts_with(pre.trim_start_matches("./")))
        };
        let mut not_shown = NotShown::default();
        // (path, old, new) for every changed file.
        let mut changes: Vec<(String, Option<String>, Option<String>, bool)> = Vec::new();
        if let Some((l, rt)) = rev.split_once("..") {
            let prefix = r.prefix.as_deref();
            let (left, right) = (tree_blobs(repo, l, prefix)?, tree_blobs(repo, rt, prefix)?);
            let paths: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
            for p in paths {
                if !keep(p) {
                    continue;
                }
                let (lo, ro) = (left.get(p), right.get(p));
                if lo == ro {
                    continue;
                }
                if let Err(why) = r.check(p) {
                    not_shown.add(p, why);
                    continue;
                }
                let old = match lo {
                    Some(id) => blob_text(repo, *id)?,
                    None => None,
                };
                let new = match ro {
                    Some(id) => blob_text(repo, *id)?,
                    None => None,
                };
                let binary = (lo.is_some() && old.is_none()) || (ro.is_some() && new.is_none());
                changes.push((
                    p.clone(),
                    if lo.is_some() {
                        old.or(Some(String::new()))
                    } else {
                        None
                    },
                    if ro.is_some() {
                        new.or(Some(String::new()))
                    } else {
                        None
                    },
                    binary,
                ));
            }
        } else {
            let tree = tree_blobs(repo, &rev, r.prefix.as_deref())?;
            let mut real_dirs = BTreeMap::new();
            for (p, id) in &tree {
                if !keep(p) {
                    continue;
                }
                // Before anything at the path is read: a floor file's bytes
                // would otherwise be hashed, and diffed when they changed.
                if let Err(why) = r.check(p) {
                    not_shown.add(p, why);
                    continue;
                }
                match worktree_bytes(wd, p, &mut real_dirs) {
                    Some(data) => {
                        if worktree_blob_id(&data) == Some(*id) {
                            continue;
                        }
                        let old = blob_text(repo, *id)?;
                        let binary = old.is_none() || data.iter().take(8192).any(|b| *b == 0);
                        changes.push((
                            p.clone(),
                            Some(old.unwrap_or_default()),
                            Some(String::from_utf8_lossy(&data).into_owned()),
                            binary,
                        ));
                    }
                    None => {
                        let old = blob_text(repo, *id)?;
                        changes.push((
                            p.clone(),
                            Some(old.clone().unwrap_or_default()),
                            None,
                            old.is_none(),
                        ));
                    }
                }
            }
            if a.untracked {
                // Only the root's part of a larger working tree is walked,
                // and the walk never enters the floor: each entry it leaves
                // out there is kept, to be counted.
                let top = match &r.prefix {
                    Some(pre) => wd.join(pre),
                    None => wd.clone(),
                };
                let pruned = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
                let (floor, kept) = (r.floor.clone(), pruned.clone());
                let mut w = ignore::WalkBuilder::new(&top);
                w.hidden(false)
                    .git_ignore(true)
                    .require_git(false)
                    .filter_entry(move |e| {
                        if e.file_name() == ".git" {
                            return false;
                        }
                        if floor.iter().any(|f| paths::within(e.path(), f)) {
                            kept.lock().unwrap().push(e.path().to_path_buf());
                            return false;
                        }
                        true
                    });
                for e in w.build().flatten() {
                    if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        continue;
                    }
                    let rel = e
                        .path()
                        .strip_prefix(wd)
                        .unwrap_or(e.path())
                        .to_string_lossy()
                        .replace('\\', "/");
                    if tree.contains_key(&rel) || !keep(&rel) {
                        continue;
                    }
                    let data = fs::read(e.path()).unwrap_or_default();
                    let binary = data.iter().take(8192).any(|b| *b == 0);
                    changes.push((
                        rel,
                        None,
                        Some(String::from_utf8_lossy(&data).into_owned()),
                        binary,
                    ));
                }
                // A floor entry counts once, unless a tracked path under it
                // already did.
                for p in pruned.lock().unwrap().iter() {
                    let rel = p.strip_prefix(wd).unwrap_or(p).to_string_lossy();
                    let under = format!("{rel}/");
                    if keep(&rel)
                        && !not_shown
                            .floor
                            .iter()
                            .any(|c| *c == rel || c.starts_with(&under))
                    {
                        not_shown.add(&rel, Out::Floor);
                    }
                }
            }
        }
        changes.sort_by(|x, y| x.0.cmp(&y.0));
        let mut out = String::new();
        let (mut tadd, mut tdel) = (0usize, 0usize);
        let mut stat_rows = Vec::new();
        for (p, old, new, binary) in &changes {
            if *binary {
                stat_rows.push(format!("{p} | binary"));
                if !a.stat {
                    out.push_str(&format!("diff --git a/{p} b/{p}\nBinary files differ\n"));
                }
                continue;
            }
            let (text, add, del) = file_diff(p, old.as_deref(), new.as_deref(), context);
            tadd += add;
            tdel += del;
            stat_rows.push(format!(
                "{p} | +{add} -{del}{}",
                if old.is_none() {
                    " (new)"
                } else if new.is_none() {
                    " (deleted)"
                } else {
                    ""
                }
            ));
            if !a.stat {
                out.push_str(&text);
            }
        }
        let summary = format!(
            "{} file{} changed, +{tadd} -{tdel}",
            changes.len(),
            if changes.len() == 1 { "" } else { "s" }
        );
        let text = if changes.is_empty() {
            format!(
                "No changes ({} vs {}).",
                rev,
                if rev.contains("..") {
                    ""
                } else {
                    "working tree"
                }
            )
        } else if a.stat {
            format!("{}\n{summary}", stat_rows.join("\n"))
        } else {
            format!("{summary}\n{out}")
        };
        // First what the call could not see: the limit, then the count.
        let text = r
            .note()
            .into_iter()
            .chain(not_shown.line())
            .chain([text])
            .collect::<Vec<_>>()
            .join("\n");
        let mut meta = json!({"repo": wd, "rev": rev, "files": changes.len(), "insertions": tadd, "deletions": tdel});
        r.annotate(&mut meta, &not_shown);
        Ok(ToolOutput { text, meta })
    }
    fn rest(&self, _left_out: &str) -> String {
        "git_diff narrowed by paths returns them; stat lists every file first".into()
    }
}

// ---------------------------------------------------------------- git.log

pub struct Log;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    rev: Option<String>,
    #[serde(default)]
    max: Option<usize>,
    #[serde(default)]
    file: Option<String>,
}

fn fmt_date(secs: i64) -> String {
    // Civil date from Unix seconds (UTC), no date crate.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m) = (rem / 3600, (rem % 3600) / 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}Z")
}

impl Tool for Log {
    fn name(&self) -> &'static str {
        "git.log"
    }
    fn description(&self) -> &'static str {
        "Recent commits (newest first): short hash, date, author, subject. Optionally from a revision and only commits that changed one file."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "A directory inside the repository. Default: the working directory."},
                "rev": {"type": "string", "description": "Start from this revision. Default HEAD."},
                "max": {"type": "integer", "minimum": 1, "maximum": 500, "description": "Commits to return. Default 20."},
                "file": {"type": "string", "description": "Only commits that changed this repository-relative path."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: LogArgs = parse(input)?;
        let dir = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        Ok(Plan {
            summary: format!("git log in {}", dir.display()),
            resources: vec![Resource {
                path: dir,
                access: Access::Read,
            }],
            argv: None,
            url: None,
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: LogArgs = parse(input).map_err(ToolFailure::new)?;
        let r = open(ctx, a.path.as_deref())?;
        let (repo, wd) = (&r.repo, &r.wd);
        // Limited to the root's part (theseus-bsc): only the commits that
        // changed something under it, and a file must be under it too.
        let only = match (a.file.as_deref(), r.prefix.as_deref()) {
            (Some(f), Some(pre)) => {
                let f = f.trim_start_matches("./");
                if !Path::new(f).starts_with(pre) {
                    return Err(ToolFailure::new(format!(
                        "{f} is outside {}, the part of the repository at {} that git.log reads",
                        r.root.display(),
                        wd.display()
                    )));
                }
                Some(f.to_string())
            }
            (Some(f), None) => Some(f.to_string()),
            (None, pre) => pre.map(str::to_string),
        };
        let rev = a.rev.clone().unwrap_or_else(|| "HEAD".into());
        let max = a.max.unwrap_or(20).min(500);
        let start = repo
            .rev_parse_single(rev.as_str())
            .map_err(|e| ToolFailure::new(format!("unknown revision {rev:?}: {e}")))?
            .detach();
        let walk = repo
            .rev_walk([start])
            .all()
            .map_err(|e| ToolFailure::new(e.to_string()))?;
        let file_blob = |commit: &gix::Commit<'_>, path: &str| -> Option<gix::ObjectId> {
            let tree = commit.tree().ok()?;
            let entry = tree.lookup_entry_by_path(path).ok()??;
            Some(entry.object_id())
        };
        let mut rows = Vec::new();
        let mut scanned = 0usize;
        for info in walk {
            let info = info.map_err(|e| ToolFailure::new(e.to_string()))?;
            scanned += 1;
            if scanned > 20_000 {
                break;
            }
            let commit = info.object().map_err(|e| ToolFailure::new(e.to_string()))?;
            if let Some(f) = &only {
                let here = file_blob(&commit, f);
                let parent = commit
                    .parent_ids()
                    .next()
                    .and_then(|pid| pid.object().ok())
                    .and_then(|o| o.try_into_commit().ok())
                    .and_then(|pc| file_blob(&pc, f));
                if here == parent {
                    continue;
                }
            }
            let subject = commit
                .message()
                .map(|m| m.summary().to_string())
                .unwrap_or_default();
            let (who, secs) = match commit.author() {
                Ok(sig) => (
                    sig.name.to_string(),
                    sig.time().map(|t| t.seconds).unwrap_or(0),
                ),
                Err(_) => ("?".into(), 0),
            };
            let hash = commit.id().to_hex_with_len(8).to_string();
            rows.push(format!("{hash} {} {who}: {subject}", fmt_date(secs)));
            if rows.len() >= max {
                break;
            }
        }
        let text = if rows.is_empty() {
            "No commits found.".into()
        } else {
            rows.join("\n")
        };
        let mut meta = json!({"repo": wd, "rev": rev, "commits": rows.len(), "scanned": scanned});
        r.annotate(&mut meta, &NotShown::default());
        Ok(ToolOutput {
            text: r
                .note()
                .into_iter()
                .chain([text])
                .collect::<Vec<_>>()
                .join("\n"),
            meta,
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "git_log with a lower max, a later rev, or one file returns them".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(args)
            .current_dir(dir)
            // The fixture must not read the operator's git config. A global
            // `commit.gpgsign = true` made this test wait 60 s for a locked
            // gpg-agent after a reboot, then fail.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn diff_and_log_against_a_real_repository() {
        if Command::new("git").arg("--version").output().is_err() {
            return; // the fixture needs git to build the repository; the tools do not
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        git(root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        fs::write(root.join("b.txt"), "keep\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "first commit"]);
        fs::write(root.join("a.txt"), "one\nTWO\n").unwrap();
        git(root, &["commit", "-q", "-am", "second: change a"]);
        fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        fs::remove_file(root.join("b.txt")).unwrap();
        fs::write(root.join("new.txt"), "fresh\n").unwrap();
        let c = ToolCtx::for_tests(root);

        let wt = Diff.run(&json!({}), &c).unwrap();
        assert!(
            wt.text.contains("+three") && wt.text.contains("diff --git a/b.txt"),
            "{}",
            wt.text
        );
        assert!(!wt.text.contains("new.txt"), "untracked is off by default");
        // A root that is its own repository: no limit, nothing left out
        // (theseus-bsc).
        assert!(wt.text.starts_with("2 files changed"), "{}", wt.text);
        assert!(wt.meta.get("limited_to").is_none() && wt.meta.get("not_shown").is_none());
        let wt2 = Diff
            .run(&json!({"untracked": true, "stat": true}), &c)
            .unwrap();
        assert!(
            wt2.text.contains("new.txt | +1 -0 (new)")
                && wt2.text.contains("b.txt | +0 -1 (deleted)"),
            "{}",
            wt2.text
        );
        let revs = Diff.run(&json!({"rev": "HEAD~1..HEAD"}), &c).unwrap();
        assert!(
            revs.text.contains("-two")
                && revs.text.contains("+TWO")
                && !revs.text.contains("three"),
            "{}",
            revs.text
        );
        let only = Diff.run(&json!({"paths": ["b.txt"]}), &c).unwrap();
        assert!(!only.text.contains("a.txt"));

        let log = Log.run(&json!({}), &c).unwrap();
        let lines: Vec<&str> = log.text.lines().collect();
        assert_eq!(lines.len(), 2, "{}", log.text);
        assert!(lines[0].contains("Tester: second: change a"));
        assert!(log.meta.get("limited_to").is_none());
        let flog = Log.run(&json!({"file": "b.txt"}), &c).unwrap();
        assert_eq!(flog.text.lines().count(), 1, "{}", flog.text);
        assert!(Diff.run(&json!({"rev": "nope"}), &c).is_err());
        assert!(fmt_date(0).starts_with("1970-01-01"));
        assert!(fmt_date(1_790_000_000).starts_with("2026-"));
    }

    /// Review 2's H4 (theseus-skc): symbolic links in the working tree are
    /// read as git reads them, as their target's path, never through. One
    /// link points outside the roots, one at a floor path (a state dir's
    /// store, as `~/.theseus/store` is on the floor), and one directory is
    /// replaced by a link: no target's text reaches any diff.
    #[test]
    fn diff_never_reads_through_a_symlink_in_the_working_tree() {
        use std::os::unix::fs::symlink;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("repo");
        let outside = d.path().join("outside");
        let floor = d.path().join("state/store");
        for dir in [&root, &outside, &floor] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(outside.join("secret.txt"), "OUTSIDE-SECRET-7f3a\n").unwrap();
        fs::write(outside.join("other.txt"), "OUTSIDE-OTHER-2b8d\n").unwrap();
        fs::write(outside.join("x.txt"), "OUTSIDE-DIR-SECRET-5e0c\n").unwrap();
        fs::write(floor.join("index.txt"), "FLOOR-SECRET-9c1e\n").unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/x.txt"), "committed\n").unwrap();
        symlink(outside.join("secret.txt"), root.join("notes")).unwrap();
        symlink(floor.join("index.txt"), root.join("key")).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "links"]);
        let c = ToolCtx::for_tests(&root);
        let leaks = |text: &str| {
            ["OUTSIDE-", "FLOOR-SECRET"]
                .iter()
                .any(|s| text.contains(s))
        };

        // Committed links, unchanged: the same as their blobs, so no change.
        let same = Diff.run(&json!({}), &c).unwrap();
        assert!(same.text.starts_with("No changes"), "{}", same.text);

        // A link retargeted in the working tree diffs as its path's text.
        fs::remove_file(root.join("notes")).unwrap();
        symlink(outside.join("other.txt"), root.join("notes")).unwrap();
        let moved = Diff.run(&json!({}), &c).unwrap();
        let (old, new) = (
            outside.join("secret.txt").display().to_string(),
            outside.join("other.txt").display().to_string(),
        );
        assert!(
            moved.text.contains(&format!("-{old}")) && moved.text.contains(&format!("+{new}")),
            "{}",
            moved.text
        );
        assert!(!leaks(&moved.text), "{}", moved.text);

        // A tracked file whose directory became a link is not in the working
        // tree: it shows as deleted, with its committed text only.
        fs::remove_dir_all(root.join("sub")).unwrap();
        symlink(&outside, root.join("sub")).unwrap();
        // And an untracked link to the floor, which the walk never follows.
        symlink(floor.join("index.txt"), root.join("untracked-key")).unwrap();
        for args in [
            json!({}),
            json!({"untracked": true}),
            json!({"stat": true, "untracked": true}),
            json!({"paths": ["sub", "key", "notes"]}),
        ] {
            let out = Diff.run(&args, &c).unwrap();
            assert!(!leaks(&out.text), "{args}: {}", out.text);
        }
        let gone = Diff.run(&json!({"paths": ["sub"]}), &c).unwrap();
        assert!(
            gone.text.contains("-committed") && gone.text.contains("+++ /dev/null"),
            "{}",
            gone.text
        );
    }

    /// A crafted tree can name `..` or an absolute path, which git refuses
    /// to check out but a fetched tree can hold: such a path is not in the
    /// working tree, so nothing is read from where it points (theseus-skc).
    #[test]
    fn a_tree_path_that_climbs_out_is_not_in_the_working_tree() {
        let d = tempfile::tempdir().unwrap();
        let wd = d.path().join("repo");
        fs::create_dir_all(wd.join("sub")).unwrap();
        fs::write(d.path().join("outside.txt"), "OUTSIDE").unwrap();
        fs::write(wd.join("sub/in.txt"), "in").unwrap();
        let abs = d.path().join("outside.txt");
        let mut dirs = BTreeMap::new();
        for p in [
            "../outside.txt",
            "sub/../../outside.txt",
            "./../outside.txt",
            abs.to_str().unwrap(),
        ] {
            assert_eq!(worktree_bytes(&wd, p, &mut dirs), None, "{p}");
        }
        assert_eq!(
            worktree_bytes(&wd, "sub/in.txt", &mut dirs).as_deref(),
            Some(&b"in"[..])
        );
    }

    fn have_git() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    /// Every result's text for `calls`, joined, to look for leaks in.
    fn texts(c: &ToolCtx, calls: &[Value]) -> String {
        let mut all = String::new();
        for args in calls {
            all.push_str(&Diff.run(args, c).unwrap().text);
            all.push('\n');
        }
        all
    }

    /// theseus-bsc: a root that is a subdirectory of a larger repository
    /// sees only its own part of it, in the diff, the stat, the untracked
    /// files, two revisions, and the log, and the first line says so.
    #[test]
    fn a_root_inside_a_larger_repository_sees_only_its_own_part() {
        if !have_git() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let mono = d.path().join("mono");
        fs::create_dir_all(mono.join("svc/src")).unwrap();
        fs::create_dir_all(mono.join("other")).unwrap();
        git(&mono, &["init", "-q", "-b", "main"]);
        fs::write(mono.join("top.txt"), "TOP-1\n").unwrap();
        fs::write(mono.join("other/b.txt"), "OTHER-1\n").unwrap();
        fs::write(mono.join("svc/src/a.txt"), "svc one\n").unwrap();
        git(&mono, &["add", "."]);
        git(&mono, &["commit", "-q", "-m", "first"]);
        fs::write(mono.join("other/b.txt"), "OTHER-2\n").unwrap();
        git(&mono, &["commit", "-q", "-am", "other: change b"]);
        fs::write(mono.join("svc/src/a.txt"), "svc two\n").unwrap();
        git(&mono, &["commit", "-q", "-am", "svc: change a"]);
        fs::write(mono.join("top.txt"), "TOP-SECRET-WT\n").unwrap();
        fs::write(mono.join("other/b.txt"), "OTHER-SECRET-WT\n").unwrap();
        fs::write(mono.join("other/new.txt"), "OTHER-UNTRACKED\n").unwrap();
        fs::write(mono.join("svc/src/a.txt"), "svc three\n").unwrap();
        fs::write(mono.join("svc/new.txt"), "svc fresh\n").unwrap();
        let svc = mono.join("svc").canonicalize().unwrap();
        let note = format!(
            "limited to {}, inside the repository at {}",
            svc.display(),
            mono.canonicalize().unwrap().display()
        );
        let c = ToolCtx::for_tests(&svc);
        let leaks = |t: &str| {
            ["TOP-", "OTHER-", "other/", "top.txt"]
                .iter()
                .any(|s| t.contains(s))
        };

        let wt = Diff.run(&json!({}), &c).unwrap();
        assert_eq!(wt.text.lines().next(), Some(note.as_str()), "{}", wt.text);
        assert!(wt.text.contains("+svc three"), "{}", wt.text);
        assert_eq!(wt.meta["limited_to"], json!(svc));
        let stat = Diff
            .run(&json!({"untracked": true, "stat": true}), &c)
            .unwrap();
        assert!(
            stat.text.contains("svc/src/a.txt | +1 -1")
                && stat.text.contains("svc/new.txt | +1 -0 (new)")
                && stat.text.ends_with("2 files changed, +2 -1"),
            "{}",
            stat.text
        );
        let revs = Diff.run(&json!({"rev": "HEAD~2..HEAD"}), &c).unwrap();
        assert!(
            revs.text.contains("-svc one") && revs.text.contains("+svc two"),
            "{}",
            revs.text
        );
        let all = texts(
            &c,
            &[
                json!({}),
                json!({"untracked": true}),
                json!({"stat": true, "untracked": true}),
                json!({"rev": "HEAD~2..HEAD"}),
                json!({"rev": "HEAD~2"}),
                json!({"paths": ["other", "top.txt"]}),
            ],
        );
        assert!(!leaks(&all), "{all}");
        // Two of the three commits changed the root's part.
        let log = Log.run(&json!({}), &c).unwrap();
        let lines: Vec<&str> = log.text.lines().collect();
        assert_eq!(lines.len(), 3, "{}", log.text);
        assert_eq!(lines[0], note);
        assert!(lines[1].ends_with("svc: change a") && lines[2].ends_with("first"));
        let flog = Log.run(&json!({"file": "./svc/src/a.txt"}), &c).unwrap();
        assert_eq!(flog.text.lines().count(), 3, "{}", flog.text);
        let e = Log.run(&json!({"file": "other/b.txt"}), &c).err().unwrap();
        assert!(
            e.message.contains("other/b.txt is outside"),
            "{}",
            e.message
        );

        // Nested roots: the outermost that holds the path is the limit.
        let src = svc.join("src");
        let nested = ToolCtx {
            roots: vec![src.clone(), svc.clone()],
            cwd: src.clone(),
            ..ToolCtx::for_tests(&svc)
        };
        let out = Diff.run(&json!({"untracked": true}), &nested).unwrap();
        assert!(out.text.starts_with(&note) && out.text.contains("svc/new.txt"));
        // A path outside every root (one the operator approved) is its own
        // root: what was approved is what is read.
        let elsewhere = d.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let approved = ToolCtx::for_tests(&elsewhere);
        let out = Diff
            .run(
                &json!({"path": src.to_str().unwrap(), "untracked": true}),
                &approved,
            )
            .unwrap();
        assert!(
            out.text
                .starts_with(&format!("limited to {}, inside", src.display()))
                && out.text.contains("+svc three")
                && !out.text.contains("svc/new.txt")
                && !leaks(&out.text),
            "{}",
            out.text
        );
    }

    /// theseus-bsc: a stray repository above a root that has none (a
    /// `~/.git`) is read only under the root, and nothing outside it
    /// reaches a result: not its working tree, untracked files, or history.
    #[test]
    fn a_stray_repository_above_a_root_is_read_only_under_the_root() {
        if !have_git() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let app = home.join("projects/app");
        fs::create_dir_all(&app).unwrap();
        git(&home, &["init", "-q", "-b", "main"]);
        fs::write(home.join("notes.txt"), "HOME-SECRET-1\n").unwrap();
        fs::write(app.join("x.txt"), "app one\n").unwrap();
        git(&home, &["add", "."]);
        git(&home, &["commit", "-q", "-m", "home: everything"]);
        fs::write(home.join("notes.txt"), "HOME-SECRET-2\n").unwrap();
        git(&home, &["commit", "-q", "-am", "home: notes only"]);
        fs::write(home.join("notes.txt"), "HOME-SECRET-WT\n").unwrap();
        fs::write(home.join("untracked.txt"), "HOME-UNTRACKED\n").unwrap();
        fs::write(app.join("x.txt"), "app two\n").unwrap();
        fs::write(app.join("y.txt"), "app new\n").unwrap();
        let projects = home.join("projects").canonicalize().unwrap();
        let c = ToolCtx::for_tests(&projects);
        let note = format!(
            "limited to {}, inside the repository at {}",
            projects.display(),
            home.canonicalize().unwrap().display()
        );
        let all = texts(
            &c,
            &[
                json!({}),
                json!({"untracked": true}),
                json!({"stat": true, "untracked": true}),
                json!({"rev": "HEAD~1..HEAD"}),
                json!({"rev": "HEAD~1"}),
                json!({"path": app.to_str().unwrap()}),
            ],
        );
        assert!(
            !all.contains("HOME-") && !all.contains("notes.txt"),
            "{all}"
        );
        let wt = Diff.run(&json!({"untracked": true}), &c).unwrap();
        assert!(
            wt.text.starts_with(&note)
                && wt.text.contains("+app two")
                && wt.text.contains("projects/app/y.txt"),
            "{}",
            wt.text
        );
        assert!(Diff
            .run(&json!({"rev": "HEAD~1..HEAD"}), &c)
            .unwrap()
            .text
            .ends_with("No changes (HEAD~1..HEAD vs )."));
        let log = Log.run(&json!({}), &c).unwrap();
        assert_eq!(
            log.text.lines().collect::<Vec<_>>().len(),
            2,
            "only the commit that touched the root: {}",
            log.text
        );
        assert!(!log.text.contains("home: notes only"), "{}", log.text);
    }

    /// theseus-bsc: a root that is its own repository, nested inside a
    /// larger one, opens its own (the nearest), unchanged.
    #[test]
    fn a_repository_of_its_own_inside_a_larger_one_is_unchanged() {
        if !have_git() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let outer = d.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        git(&outer, &["init", "-q", "-b", "main"]);
        fs::write(outer.join("o.txt"), "OUTER\n").unwrap();
        git(&outer, &["add", "o.txt"]);
        git(&outer, &["commit", "-q", "-m", "outer"]);
        git(&inner, &["init", "-q", "-b", "main"]);
        fs::write(inner.join("i.txt"), "one\n").unwrap();
        git(&inner, &["add", "."]);
        git(&inner, &["commit", "-q", "-m", "inner"]);
        fs::write(inner.join("i.txt"), "two\n").unwrap();
        fs::write(outer.join("o.txt"), "OUTER-WT\n").unwrap();
        let c = ToolCtx::for_tests(&inner);
        let wt = Diff.run(&json!({}), &c).unwrap();
        assert!(
            wt.text.starts_with("1 file changed") && wt.text.contains("diff --git a/i.txt"),
            "{}",
            wt.text
        );
        assert!(!wt.text.contains("OUTER") && wt.meta.get("limited_to").is_none());
        assert_eq!(Log.run(&json!({}), &c).unwrap().text.lines().count(), 1);
    }

    /// theseus-bsc: a tracked file on the floor, inside a root, is never
    /// read, in the working tree or in history: it is skipped and counted.
    /// An untracked one there is never walked.
    #[test]
    fn a_tracked_file_on_the_floor_is_skipped_and_counted() {
        if !have_git() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("repo");
        fs::create_dir_all(root.join("state/store")).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        fs::write(root.join("state/keep.txt"), "kept\n").unwrap();
        fs::write(root.join("state/store/index.txt"), "FLOOR-1\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        fs::write(root.join("a.txt"), "two\n").unwrap();
        fs::write(root.join("state/store/index.txt"), "FLOOR-2\n").unwrap();
        git(&root, &["commit", "-q", "-am", "second"]);
        fs::write(root.join("a.txt"), "three\n").unwrap();
        fs::write(root.join("state/store/index.txt"), "FLOOR-SECRET-WT\n").unwrap();
        fs::write(root.join("state/store/new.txt"), "FLOOR-UNTRACKED\n").unwrap();
        fs::write(root.join("b.txt"), "bee\n").unwrap();
        let plain = ToolCtx::for_tests(&root);
        let c = ToolCtx {
            floor: vec![plain.roots[0].join("state/store")],
            ..plain
        };
        let one = "1 path on the floor (Theseus's own state) not shown";
        let wt = Diff.run(&json!({}), &c).unwrap();
        assert!(
            wt.text.starts_with(&format!("{one}\n1 file changed")) && wt.text.contains("+three"),
            "{}",
            wt.text
        );
        assert_eq!(
            wt.meta["not_shown"],
            json!({"outside_roots": 0, "floor": 1})
        );
        // The untracked walk leaves the floor out without counting it twice.
        let un = Diff
            .run(&json!({"untracked": true, "stat": true}), &c)
            .unwrap();
        assert!(
            un.text.starts_with(one) && un.text.contains("b.txt | +1 -0 (new)"),
            "{}",
            un.text
        );
        let revs = Diff.run(&json!({"rev": "HEAD~1..HEAD"}), &c).unwrap();
        assert!(
            revs.text.starts_with(one) && revs.text.contains("+two"),
            "{}",
            revs.text
        );
        let all = texts(
            &c,
            &[
                json!({}),
                json!({"untracked": true}),
                json!({"rev": "HEAD~1..HEAD"}),
                json!({"rev": "HEAD~1"}),
                json!({"paths": ["state"], "untracked": true}),
            ],
        );
        assert!(!all.contains("FLOOR-"), "{all}");
        // Narrowed away from the floor, nothing is left out.
        let only = Diff.run(&json!({"paths": ["a.txt"]}), &c).unwrap();
        assert!(only.text.starts_with("1 file changed"), "{}", only.text);
        // With the floor's directory untracked, the walk counts it once.
        git(&root, &["rm", "-q", "--cached", "state/store/index.txt"]);
        git(&root, &["commit", "-q", "-m", "untrack the store"]);
        let walked = Diff.run(&json!({"untracked": true}), &c).unwrap();
        assert!(walked.text.starts_with(one), "{}", walked.text);
        assert!(!walked.text.contains("FLOOR-"), "{}", walked.text);
        // A tree path that climbs out is not under the root.
        let r = open(&c, None).unwrap();
        for p in ["../x", "a/../../x", "/etc/passwd", "./a.txt"] {
            assert_eq!(r.check(p), Err(Out::Roots), "{p}");
        }
        assert_eq!(r.check("state/store/x"), Err(Out::Floor));
        assert_eq!(r.check("state/keep.txt"), Ok(()));
        let mut n = NotShown::default();
        n.add("../x", Out::Roots);
        n.add("../y", Out::Roots);
        assert_eq!(n.line().unwrap(), "2 paths outside the roots not shown");
    }

    /// theseus-bsc: a repository whose working tree is set elsewhere
    /// (`core.worktree`), outside the roots, is refused: its tracked paths
    /// would be read there.
    #[test]
    fn a_working_tree_set_outside_the_roots_is_refused() {
        if !have_git() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("repo");
        let outside = d.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        fs::write(outside.join("a.txt"), "OUTSIDE-SECRET\n").unwrap();
        git(
            &root,
            &["config", "core.worktree", outside.to_str().unwrap()],
        );
        let c = ToolCtx::for_tests(&root);
        for e in [
            Diff.run(&json!({}), &c).err().unwrap(),
            Log.run(&json!({}), &c).err().unwrap(),
        ] {
            assert!(
                e.message.contains("has its working tree at")
                    && e.message.contains("read only under the roots"),
                "{}",
                e.message
            );
        }
    }
}
