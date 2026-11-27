//! The review file of a publication (issue 53): everything a join of `head` onto the remote's `main` would send,
//! made from the objects themselves, written under the self reports folder before the owner's card is asked. For
//! every commit of `main..head` (every parent's history, not the first parent's only): the raw commit object, every
//! header as git stores it, and its change against every parent. Text conversion and external diff drivers are off
//! whatever attributes say (`--no-textconv`, `--no-ext-diff`), the user's attributes file is not read, color, pager
//! and rename detection are off, and replacement objects are not used (`--no-replace-objects`); file modes, symlink
//! targets and gitlinks are shown as git records them.
//!
//! **One closed rule on bytes, checked before anything is rendered** ([`exact`]): every raw commit object of the
//! range, every changed or added blob (a symlink's target included) and every path must be valid UTF-8 holding no
//! control character but newline and tab (a carriage return only before a newline) and no direction character.
//! Anything else is not offered (`Unshowable`): a file git calls binary or that holds a NUL byte, a file over
//! [`FILE_BYTES`], text that cannot be shown exactly, or an annotated tag or a note on a commit of the range. The
//! rendering is then lossless: git's output is decoded strictly, never with a lossy decoder.

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
    /// Bytes [`exact`] refuses: not valid UTF-8, a control character, or a direction character.
    Inexact,
}

/// Whether `b` can be shown exactly: valid UTF-8, no control character but newline and tab (a carriage return only
/// before a newline), and no bidirectional-override or other invisible direction character.
pub fn exact(b: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(b) else {
        return false;
    };
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' | '\t' => {}
            '\r' if chars.peek() == Some(&'\n') => {}
            '\u{200e}'
            | '\u{200f}'
            | '\u{061c}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => return false,
            c if c.is_control() => return false,
            _ => {}
        }
    }
    true
}

impl Unshowable {
    pub fn words(self) -> &'static str {
        match self {
            Unshowable::Binary => {
                "it holds a file that cannot be shown as text (binary by git's rule, or holding a NUL byte)"
            }
            Unshowable::Large => "it holds a file larger than 4 MiB",
            Unshowable::TagOrNote => "an annotated tag or a note names one of its commits",
            Unshowable::Inexact => "it holds text that cannot be shown exactly",
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

/// git with its presentation settings off: the user's attributes file unread, paths unquoted, no color, no pager, and
/// replacement objects not used. Text conversion, external diff and renames are turned off on each diff's own line.
fn plain<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec![
        "--no-replace-objects",
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

/// git's output, its bytes.
fn git_bytes(p: &Place, args: &[&str]) -> Result<Vec<u8>> {
    let o = p.git(&p.repo, &plain(args))?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(o.stdout)
}

/// git's output decoded strictly: shas, counts and names the closed rule has already passed.
fn git_ok(p: &Place, args: &[&str]) -> Result<String> {
    let b = git_bytes(p, args)?;
    String::from_utf8(b)
        .map_err(|_| anyhow::anyhow!("git {} printed bytes that are not UTF-8", args.join(" ")))
}

/// git's output for the file, decoded strictly; none when it cannot be shown exactly.
fn git_exact(p: &Place, args: &[&str]) -> Result<Option<String>> {
    let b = git_bytes(p, args)?;
    Ok(String::from_utf8(b).ok())
}

/// What of the range the file cannot show, if anything: each commit's raw object, each changed path, and each
/// changed blob of a file or a symlink, checked on bytes before anything is rendered.
fn unshowable(p: &Place, commits: &[String]) -> Result<Option<Unshowable>> {
    for c in commits {
        if !exact(&git_bytes(p, &["cat-file", "commit", c])?) {
            return Ok(Some(Unshowable::Inexact));
        }
        let parents = parents(p, c)?;
        let against: Vec<String> = match parents.is_empty() {
            true => vec!["--root".into()],
            false => parents,
        };
        for parent in &against {
            let range: Vec<&str> = match parent.as_str() {
                "--root" => vec!["--root", c.as_str()],
                p => vec![p, c.as_str()],
            };
            let mut raw = vec![
                "diff-tree",
                "-r",
                "-z",
                "--no-renames",
                "--raw",
                "--no-abbrev",
            ];
            raw.extend(range.iter().copied());
            if let Some(u) = changed_unshowable(p, &git_bytes(p, &raw)?)? {
                return Ok(Some(u));
            }
            let mut num = vec![
                "diff-tree",
                "-r",
                "-z",
                "--no-renames",
                "--no-textconv",
                "--no-ext-diff",
                "--numstat",
            ];
            num.extend(range.iter().copied());
            if git_bytes(p, &num)?
                .split(|b| *b == 0)
                .any(|e| e.starts_with(b"-\t-\t"))
            {
                return Ok(Some(Unshowable::Binary));
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

/// What of one `diff-tree -z --raw` listing the file cannot show: each path, and each new blob of a file or a
/// symlink (its size, a NUL, then the closed rule). `:<mode> <mode> <sha> <sha> <status>` NUL `<path>` NUL, an entry.
fn changed_unshowable(p: &Place, listing: &[u8]) -> Result<Option<Unshowable>> {
    let mut parts = listing.split(|b| *b == 0).filter(|e| !e.is_empty());
    while let Some(head) = parts.next() {
        if !head.starts_with(b":") {
            continue;
        }
        let path = parts.next().unwrap_or_default();
        if !exact(path) {
            return Ok(Some(Unshowable::Inexact));
        }
        let fields: Vec<&[u8]> = head.split(|b| *b == b' ').collect();
        let (mode, blob) = (
            fields.get(1).copied().unwrap_or_default(),
            fields.get(3).copied().unwrap_or_default(),
        );
        let blob = std::str::from_utf8(blob).unwrap_or_default();
        let shown = matches!(mode, b"100644" | b"100755" | b"120000");
        if !shown || blob.is_empty() || blob.chars().all(|ch| ch == '0') {
            continue;
        }
        let bytes: u64 = git_ok(p, &["cat-file", "-s", blob])?
            .trim()
            .parse()
            .unwrap_or(u64::MAX);
        if bytes > FILE_BYTES {
            return Ok(Some(Unshowable::Large));
        }
        let content = git_bytes(p, &["cat-file", "blob", blob])?;
        if content.contains(&0) {
            return Ok(Some(Unshowable::Binary));
        }
        if !exact(&content) {
            return Ok(Some(Unshowable::Inexact));
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
        let subject = (raw.split_once("\n\n").map_or("", |(_, m)| m).lines().next())
            .unwrap_or_default()
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
            // Lossless: an old side the closed rule did not read (main's own bytes) that is not UTF-8 is held too.
            let Some(diff) = git_exact(p, &args)? else {
                return Ok(Err(Unshowable::Inexact));
            };
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
    // The name carries the two full shas; the bytes are written to a temporary file of this process's own and
    // renamed over it, so a reader never sees half a file.
    let path = dir.join(format!("publish-{head}-onto-{main}.txt"));
    let tmp = dir.join(format!(
        ".publish-{head}-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
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
