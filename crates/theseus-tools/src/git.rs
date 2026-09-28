//! `git.diff` and `git.log` (spec §3.24), native through gitoxide: no git
//! binary, no PATH. `git.diff` compares a revision's tree with the working
//! tree (or two revisions) and produces unified diffs; there is no staged vs
//! unstaged split (that is `proc.run git diff --cached` until the fallback
//! ratio says otherwise). `git.log` walks history, optionally for one path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    parse, Access, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

fn open(ctx: &ToolCtx, path: Option<&str>) -> Result<(gix::Repository, PathBuf), ToolFailure> {
    let dir = path
        .map(|p| ctx.resolve(p))
        .unwrap_or_else(|| ctx.cwd.clone());
    let repo = gix::discover(&dir).map_err(|e| {
        ToolFailure::new(format!(
            "{} is not inside a git repository: {e}",
            dir.display()
        ))
    })?;
    let wd = repo
        .workdir()
        .map(Path::to_path_buf)
        .ok_or_else(|| ToolFailure::new("the repository has no working tree (bare)"))?;
    Ok((repo, wd))
}

/// path → blob id, for every blob (and symlink) in a tree.
fn tree_blobs(
    repo: &gix::Repository,
    rev: &str,
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
    let mut rec = gix::traverse::tree::Recorder::default();
    tree.traverse()
        .breadthfirst(&mut rec)
        .map_err(|e| ToolFailure::new(e.to_string()))?;
    let mut out = BTreeMap::new();
    for e in rec.records {
        if e.mode.is_blob_or_symlink() {
            out.insert(e.filepath.to_string(), e.oid);
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
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: DiffArgs = parse(input).map_err(ToolFailure::new)?;
        let (repo, wd) = open(ctx, a.path.as_deref())?;
        let context = a.context.unwrap_or(3);
        let rev = a.rev.clone().unwrap_or_else(|| "HEAD".into());
        let keep = |p: &str| {
            a.paths.is_empty()
                || a.paths
                    .iter()
                    .any(|pre| p.starts_with(pre.trim_start_matches("./")))
        };
        // (path, old, new) for every changed file.
        let mut changes: Vec<(String, Option<String>, Option<String>, bool)> = Vec::new();
        if let Some((l, r)) = rev.split_once("..") {
            let (left, right) = (tree_blobs(&repo, l)?, tree_blobs(&repo, r)?);
            let paths: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
            for p in paths {
                if !keep(p) {
                    continue;
                }
                let (lo, ro) = (left.get(p), right.get(p));
                if lo == ro {
                    continue;
                }
                let old = match lo {
                    Some(id) => blob_text(&repo, *id)?,
                    None => None,
                };
                let new = match ro {
                    Some(id) => blob_text(&repo, *id)?,
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
            let tree = tree_blobs(&repo, &rev)?;
            for (p, id) in &tree {
                if !keep(p) {
                    continue;
                }
                let on_disk = wd.join(p);
                match fs::read(&on_disk) {
                    Ok(data) => {
                        if worktree_blob_id(&data) == Some(*id) {
                            continue;
                        }
                        let old = blob_text(&repo, *id)?;
                        let binary = old.is_none() || data.iter().take(8192).any(|b| *b == 0);
                        changes.push((
                            p.clone(),
                            Some(old.unwrap_or_default()),
                            Some(String::from_utf8_lossy(&data).into_owned()),
                            binary,
                        ));
                    }
                    Err(_) => {
                        let old = blob_text(&repo, *id)?;
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
                let mut w = ignore::WalkBuilder::new(&wd);
                w.hidden(false)
                    .git_ignore(true)
                    .require_git(false)
                    .filter_entry(|e| e.file_name() != ".git");
                for e in w.build().flatten() {
                    if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        continue;
                    }
                    let rel = e
                        .path()
                        .strip_prefix(&wd)
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
        Ok(ToolOutput {
            text,
            meta: json!({"repo": wd, "rev": rev, "files": changes.len(), "insertions": tadd, "deletions": tdel}),
        })
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
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: LogArgs = parse(input).map_err(ToolFailure::new)?;
        let (repo, wd) = open(ctx, a.path.as_deref())?;
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
            if let Some(f) = &a.file {
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
        Ok(ToolOutput {
            text: if rows.is_empty() {
                "No commits found.".into()
            } else {
                rows.join("\n")
            },
            meta: json!({"repo": wd, "rev": rev, "commits": rows.len(), "scanned": scanned}),
        })
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
        let flog = Log.run(&json!({"file": "b.txt"}), &c).unwrap();
        assert_eq!(flog.text.lines().count(), 1, "{}", flog.text);
        assert!(Diff.run(&json!({"rev": "nope"}), &c).is_err());
        assert!(fmt_date(0).starts_with("1970-01-01"));
        assert!(fmt_date(1_790_000_000).starts_with("2026-"));
    }
}
