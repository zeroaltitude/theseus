//! What a push sends, as the outgoing check reads it (`super`): every commit of the exact graph `base..tip`, a
//! merge's second-parent commits among them, each read against its first parent, and the branch it goes to.
//!
//! - **Per commit**: its message; its author's and committer's names and addresses; every path it changes (an
//!   added, a removed, a renamed path alike: renames are not followed, so a rename is its old path removed and its
//!   new one added); and the whole new text of every file it changes, read as the blob is stored (`git cat-file
//!   blob`: no text conversion, no external diff, no filter), so a run that new words complete across words the
//!   file already held is seen. Nothing here parses a diff for words.
//! - **What cannot be inspected**: a changed file git reads as binary (its own rule, a `binary` or `-diff`
//!   attribute among them), one holding a NUL, or one larger than [`super::FILE_BYTES`], is refused
//!   ([`super::Refusal::Uninspectable`]). A submodule's entry is its path alone: its commit id is no words.
//! - **For the scrub** ([`Leaving::scrub_text`]): the messages, names and paths, and of each file the lines its new
//!   text holds that its old one did not, as a set of lines, not a diff.
//! - **The base** ([`base_runs`]): the runs the base's tree holds, one file at a time and one path at a time, so
//!   no run is made across two files.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{runs, words, Part, Refusal, Why};
use crate::rsi::tree::Place;

/// The empty blob and tree's id, all zeros: a side of a change that is not there.
const NONE: &str = "0000000000000000000000000000000000000000";
/// A submodule's mode.
const GITLINK: &str = "160000";

/// One commit as it leaves.
#[derive(Debug, Clone, Default)]
pub struct Out {
    pub id: String,
    pub message: String,
    pub names: String,
    pub paths: Vec<String>,
    /// The whole new text of each file it changes.
    pub files: Vec<String>,
    /// Each file's lines its old text did not hold, for the scrub.
    pub added: Vec<String>,
}

/// What a push sends: its commits, and the name of the branch it goes to.
#[derive(Debug, Clone, Default)]
pub struct Leaving {
    pub branch: String,
    pub commits: Vec<Out>,
}

impl Leaving {
    /// Every piece the check reads, each on its own: (its commit, if it is one's; its kind; its text).
    pub fn pieces(&self) -> impl Iterator<Item = (Option<&str>, Part, &str)> {
        let branch = std::iter::once((None, Part::Branch, self.branch.as_str()));
        branch.chain(self.commits.iter().flat_map(|c| {
            let id = Some(c.id.as_str());
            std::iter::once((id, Part::Message, c.message.as_str()))
                .chain(std::iter::once((id, Part::Names, c.names.as_str())))
                .chain(c.paths.iter().map(move |p| (id, Part::Path, p.as_str())))
                .chain(c.files.iter().map(move |f| (id, Part::File, f.as_str())))
        }))
    }

    /// What the daemon's scrubber reads: every message, name and path, and every line a commit adds to a file.
    pub fn scrub_text(&self) -> String {
        let mut out = vec![self.branch.clone()];
        for c in &self.commits {
            out.push(c.message.clone());
            out.push(c.names.clone());
            out.extend(c.paths.iter().cloned());
            out.extend(c.added.iter().cloned());
        }
        out.join("\n")
    }
}

/// `args` in `dir`, its stdout as bytes, or none when git failed.
fn bytes(p: &Place, dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let o = p.git(dir, args).ok()?;
    o.status.success().then_some(o.stdout)
}

/// [`bytes`], as text.
fn text(p: &Place, dir: &Path, args: &[&str]) -> Option<String> {
    bytes(p, dir, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// One changed entry: its old and new modes and blobs, and its path.
struct Entry {
    old_mode: String,
    new_mode: String,
    old: String,
    new: String,
    path: String,
}

/// The entries `id` changes against `parent` (none: a root commit), renames not followed.
fn entries(p: &Place, dir: &Path, parent: Option<&str>, id: &str) -> Option<Vec<Entry>> {
    let raw = match parent {
        Some(parent) => bytes(p, dir, &["diff-tree", "-r", "-z", "--no-renames", "--raw", parent, id])?,
        None => bytes(p, dir, &["diff-tree", "-r", "-z", "--no-renames", "--raw", "--root", id])?,
    };
    let mut out = Vec::new();
    let mut fields = raw.split(|b| *b == 0).filter(|f| !f.is_empty());
    while let Some(meta) = fields.next() {
        let meta = String::from_utf8_lossy(meta);
        // With `--root` the commit's own id comes first.
        if !meta.starts_with(':') {
            continue;
        }
        let path = String::from_utf8_lossy(fields.next()?).into_owned();
        let m: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        if m.len() < 5 {
            return None;
        }
        out.push(Entry {
            old_mode: m[0].into(),
            new_mode: m[1].into(),
            old: m[2].into(),
            new: m[3].into(),
            path,
        });
    }
    Some(out)
}

/// The paths git reads as binary in `id` against `parent`, by its own rule and the attributes (`--numstat`
/// shows each as `-`).
fn binary(p: &Place, dir: &Path, parent: Option<&str>, id: &str) -> Option<HashSet<String>> {
    let base = ["diff-tree", "-r", "-z", "--no-renames", "--numstat", "--no-textconv", "--no-ext-diff"];
    let mut args = base.to_vec();
    match parent {
        Some(parent) => args.extend([parent, id]),
        None => args.extend(["--root", id]),
    }
    let raw = bytes(p, dir, &args)?;
    let mut out = HashSet::new();
    for f in raw.split(|b| *b == 0).filter(|f| !f.is_empty()) {
        let line = String::from_utf8_lossy(f);
        // A commit id line comes first with --root.
        let mut cols = line.splitn(3, '\t');
        let (Some(a), Some(_), Some(path)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        if a == "-" {
            out.insert(path.trim_start_matches('\n').to_string());
        }
    }
    Some(out)
}

/// A blob's text, or why it is not inspected: none when it is too large, holds a NUL or cannot be read.
fn blob(p: &Place, dir: &Path, id: &str) -> Option<String> {
    let size: u64 = text(p, dir, &["cat-file", "-s", id])?.trim().parse().ok()?;
    if size > super::FILE_BYTES {
        return None;
    }
    let b = bytes(p, dir, &["cat-file", "blob", id])?;
    if b.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&b).into_owned())
}

/// The lines of `new` that `old` did not hold, counted: a set difference of lines, not a diff.
fn added(old: &str, new: &str) -> String {
    let mut had: HashMap<&str, usize> = HashMap::new();
    for l in old.lines() {
        *had.entry(l).or_default() += 1;
    }
    let mut out = Vec::new();
    for l in new.lines() {
        match had.get_mut(l) {
            Some(n) if *n > 0 => *n -= 1,
            _ => out.push(l),
        }
    }
    out.join("\n")
}

/// Every commit in `base..tip`, read in `dir`, and `branch`: what a push of `tip` to `branch` sends.
pub fn leaving(p: &Place, dir: &Path, base: &str, tip: &str, branch: &str) -> Result<Leaving, Refusal> {
    let gone = || Refusal::Unresolved(Why::Commits);
    let list = text(p, dir, &["rev-list", &format!("{base}..{tip}")]).ok_or_else(gone)?;
    let mut commits = Vec::new();
    for id in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let message = text(p, dir, &["log", "-1", "--format=%B", id]).ok_or_else(gone)?;
        let names = text(p, dir, &["log", "-1", "--format=%an%n%ae%n%cn%n%ce", id]).ok_or_else(gone)?;
        let parent = format!("{id}^1");
        let parent = match p.git(dir, &["rev-parse", "-q", "--verify", &parent]) {
            Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).trim().to_string()),
            Ok(_) => None,
            Err(_) => return Err(gone()),
        };
        let entries = entries(p, dir, parent.as_deref(), id).ok_or_else(gone)?;
        let binary = binary(p, dir, parent.as_deref(), id).ok_or_else(gone)?;
        let uninspectable = || Refusal::Uninspectable { commit: id.to_string() };
        let mut out = Out {
            id: id.to_string(),
            message,
            names,
            ..Out::default()
        };
        for e in entries {
            out.paths.push(e.path.clone());
            if e.new == NONE || e.new_mode == GITLINK {
                continue;
            }
            if binary.contains(&e.path) {
                return Err(uninspectable());
            }
            let new = blob(p, dir, &e.new).ok_or_else(uninspectable)?;
            let old = match e.old == NONE || e.old_mode == GITLINK {
                true => String::new(),
                // An old side that cannot be read as text adds every new line for the scrub.
                false => blob(p, dir, &e.old).unwrap_or_default(),
            };
            out.added.push(added(&old, &new));
            out.files.push(new);
        }
        commits.push(out);
    }
    Ok(Leaving {
        branch: branch.to_string(),
        commits,
    })
}

/// Of the runs `wanted`, those the tree at `base` holds: in one of its text files, or in one of its paths, each
/// on its own.
pub fn base_runs(p: &Place, dir: &Path, base: &str, wanted: &HashSet<u64>) -> Result<HashSet<u64>, Why> {
    let mut found = HashSet::new();
    let mut keep = |text: &str| {
        for h in runs(&words(text)) {
            if wanted.contains(&h) {
                found.insert(h);
            }
        }
    };
    let paths = bytes(p, dir, &["ls-tree", "-r", "-z", "--name-only", base]).ok_or(Why::Base)?;
    for path in paths.split(|b| *b == 0).filter(|f| !f.is_empty()) {
        keep(&String::from_utf8_lossy(path));
    }
    // `<base>:<path>\0<line>` for every line of every text file: one file's lines are kept together.
    let grep = p
        .git(dir, &["grep", "-I", "--null", "--no-color", "-e", "", base, "--"])
        .map_err(|_| Why::Base)?;
    // git grep exits 1 when nothing matched (a tree with no text file); anything else is a failed read.
    let grep = match grep.status.code() {
        Some(0) => grep.stdout,
        Some(1) => Vec::new(),
        _ => return Err(Why::Base),
    };
    let (mut at, mut file) = (String::new(), String::new());
    for line in grep.split(|b| *b == b'\n') {
        let Some(nul) = line.iter().position(|b| *b == 0) else {
            continue;
        };
        let path = String::from_utf8_lossy(&line[..nul]);
        if path != at {
            keep(&file);
            file.clear();
            at = path.into_owned();
        }
        file.push_str(&String::from_utf8_lossy(&line[nul + 1..]));
        file.push('\n');
    }
    keep(&file);
    Ok(found)
}
