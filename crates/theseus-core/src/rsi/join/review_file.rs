//! The review file of a publication (issue 53): everything a join of `head` onto the remote's `main` would send,
//! made from the objects themselves, written under the self reports folder before the owner's card is asked. For
//! every commit of `main..head` (every parent's history, not the first parent's only): the raw commit object, every
//! header as git stores it, and its change against every parent, read with attributes, text conversion, external
//! diff drivers, color and rename detection off, file modes, symlink targets and gitlinks shown as git records them.
//! A range holding what the file cannot show as text is not offered (`Unshowable`): a file git calls binary or that
//! holds a NUL byte, a file over [`FILE_BYTES`], or an annotated tag or a note on a commit of the range.

use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::rsi::tree::Place;

/// The largest file the review file shows.
pub const FILE_BYTES: u64 = 4 * 1024 * 1024;

/// What the review file cannot show, in fixed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unshowable {
    /// A file git's own rule calls binary, or that holds a NUL byte.
    Binary,
    /// A file over [`FILE_BYTES`].
    Large,
    /// An annotated tag or a note on a commit of the range.
    TagOrNote,
}

impl Unshowable {
    pub fn words(self) -> &'static str {
        match self {
            Unshowable::Binary => {
                "it holds a file that cannot be shown as text (binary by git's rule, or holding a NUL byte)"
            }
            Unshowable::Large => "it holds a file larger than 4 MiB",
            Unshowable::TagOrNote => "an annotated tag or a note names one of its commits",
        }
    }
}

/// A review file written.
#[derive(Debug, Clone)]
pub struct Review {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
    /// `(commit, subject)` of every commit of the range, oldest first.
    pub commits: Vec<(String, String)>,
    /// The files `main...head` changes.
    pub files: usize,
    /// `git diff --shortstat main...head`.
    pub size: String,
}

/// git with every presentation setting off: no attributes file, no text conversion, no external diff, no color, no
/// pager, no renames, paths unquoted.
fn plain<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec![
        "-c",
        "core.attributesFile=/dev/null",
        "-c",
        "core.quotePath=false",
        "-c",
        "diff.noprefix=false",
        "-c",
        "color.ui=false",
        "--no-pager",
    ];
    v.extend_from_slice(args);
    v
}

fn git_ok(p: &Place, args: &[&str]) -> Result<String> {
    let o = p.git(&p.repo, &plain(args))?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// What of the range the file cannot show, if anything.
fn unshowable(p: &Place, commits: &[String]) -> Result<Option<Unshowable>> {
    for c in commits {
        let parents = parents(p, c)?;
        let against: Vec<String> = match parents.is_empty() {
            true => vec!["--root".into()],
            false => parents,
        };
        for parent in &against {
            let mut args = vec![
                "diff-tree",
                "-r",
                "--no-renames",
                "--no-textconv",
                "--no-ext-diff",
                "--numstat",
            ];
            let range: Vec<&str> = match parent.as_str() {
                "--root" => vec!["--root", c.as_str()],
                p => vec![p, c.as_str()],
            };
            args.extend(range.iter().copied());
            let numstat = git_ok(p, &args)?;
            if numstat.lines().any(|l| l.starts_with("-\t-\t")) {
                return Ok(Some(Unshowable::Binary));
            }
            let mut raw = vec!["diff-tree", "-r", "--no-renames", "--raw", "--no-abbrev"];
            raw.extend(range.iter().copied());
            for l in git_ok(p, &raw)?.lines().filter(|l| l.starts_with(':')) {
                let f: Vec<&str> = l.split_whitespace().collect();
                let (mode, blob) = (
                    f.get(1).copied().unwrap_or(""),
                    f.get(3).copied().unwrap_or(""),
                );
                if !(mode == "100644" || mode == "100755") || blob.chars().all(|ch| ch == '0') {
                    continue;
                }
                let bytes: u64 = git_ok(p, &["cat-file", "-s", blob])?
                    .trim()
                    .parse()
                    .unwrap_or(u64::MAX);
                if bytes > FILE_BYTES {
                    return Ok(Some(Unshowable::Large));
                }
                let o = p.git(&p.repo, &["cat-file", "blob", blob])?;
                if o.stdout.contains(&0) {
                    return Ok(Some(Unshowable::Binary));
                }
            }
        }
    }
    let tags = git_ok(
        p,
        &[
            "for-each-ref",
            "refs/tags",
            "--format=%(objecttype) %(*objectname)",
        ],
    )?;
    if (tags.lines())
        .filter_map(|l| l.strip_prefix("tag "))
        .any(|target| commits.iter().any(|c| c == target.trim()))
    {
        return Ok(Some(Unshowable::TagOrNote));
    }
    let notes = git_ok(p, &["for-each-ref", "refs/notes", "--format=%(refname)"])?;
    for r in notes.lines().map(str::trim).filter(|r| !r.is_empty()) {
        let listed = git_ok(p, &["notes", "--ref", r, "list"])?;
        if (listed.lines())
            .filter_map(|l| l.split_whitespace().nth(1))
            .any(|annotated| commits.iter().any(|c| c == annotated))
        {
            return Ok(Some(Unshowable::TagOrNote));
        }
    }
    Ok(None)
}

fn parents(p: &Place, c: &str) -> Result<Vec<String>> {
    let line = git_ok(p, &["rev-list", "--parents", "-n", "1", c])?;
    Ok(line
        .split_whitespace()
        .skip(1)
        .map(str::to_string)
        .collect())
}

/// Write the review file of `head` onto `main` in `p`'s reports folder, its header naming the two shas and
/// `destination`; or what of the range it cannot show.
pub fn write(
    p: &Place,
    main: &str,
    head: &str,
    destination: &str,
) -> Result<std::result::Result<Review, Unshowable>> {
    let range = format!("{main}..{head}");
    let commits: Vec<String> = (git_ok(p, &["rev-list", "--reverse", "--topo-order", &range])?
        .lines())
    .map(str::trim)
    .filter(|l| !l.is_empty())
    .map(str::to_string)
    .collect();
    if let Some(u) = unshowable(p, &commits)? {
        return Ok(Err(u));
    }
    let mut text = format!(
        "THESEUS SELF-PUBLISH REVIEW FILE\nremote main: {main}\nhead: {head}\ndestination: {destination}\n\
         commits: {}\n\nEvery commit the join of head onto remote main would send, oldest first: its raw object, then \
         its change against each of its parents. The join adds its merge commit and main's count to these.\n",
        commits.len()
    );
    let mut listed = Vec::new();
    for (i, c) in commits.iter().enumerate() {
        let raw = git_ok(p, &["cat-file", "commit", c])?;
        let subject = git_ok(p, &["log", "-1", "--format=%s", c])?
            .trim()
            .to_string();
        text.push_str(&format!(
            "\n=== commit {} of {}: {c}\n--- raw object (git cat-file commit {c})\n{raw}",
            i + 1,
            commits.len()
        ));
        if !raw.ends_with('\n') {
            text.push('\n');
        }
        let ps = parents(p, c)?;
        let against: Vec<&str> = match ps.is_empty() {
            true => vec!["--root"],
            false => ps.iter().map(String::as_str).collect(),
        };
        for parent in against {
            let mut args = vec![
                "diff-tree",
                "-r",
                "-p",
                "--raw",
                "--no-abbrev",
                "--full-index",
                "--no-renames",
                "--no-textconv",
                "--no-ext-diff",
                "--no-color",
                "--submodule=short",
            ];
            match parent {
                "--root" => args.extend(["--root", c.as_str()]),
                _ => args.extend([parent, c.as_str()]),
            }
            let diff = git_ok(p, &args)?;
            text.push_str(&format!("--- change against parent {parent}\n{diff}"));
            if !diff.ends_with('\n') {
                text.push('\n');
            }
        }
        listed.push((c.clone(), subject));
    }
    text.push_str("\n=== end of the review file\n");
    let three = format!("{main}...{head}");
    let names = git_ok(p, &["diff", "--name-only", "--no-renames", &three])?;
    let size = git_ok(p, &["diff", "--shortstat", "--no-renames", &three])?;
    use sha2::Digest;
    let sha256 = hex::encode(sha2::Sha256::digest(text.as_bytes()));
    let dir = p.reports();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "publish-{}-onto-{}.txt",
        &head[..head.len().min(12)],
        &main[..main.len().min(12)]
    ));
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(Ok(Review {
        path,
        bytes: text.len() as u64,
        sha256,
        commits: listed,
        files: names.lines().filter(|l| !l.trim().is_empty()).count(),
        size: size.trim().to_string(),
    }))
}

/// The SHA-256 of the file at `path` now, if it can be read.
pub fn sha256_of(path: &std::path::Path) -> Option<String> {
    use sha2::Digest;
    std::fs::read(path)
        .ok()
        .map(|b| hex::encode(sha2::Sha256::digest(b)))
}
