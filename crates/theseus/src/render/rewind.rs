//! The lines that show a rewind (theseus-23rk, R20): the files a restore put
//! back or left, and the messages of the picker. Shared by the CLI and the
//! terminal UI.

use theseus_protocol::{FilesRestoreResult, RewindPoint};

use super::{Line, Tag};

/// `tides.py +1 −1`, or `no file changes`: what the edits since a message did.
pub fn changes_words(p: &RewindPoint) -> String {
    if p.files.is_empty() {
        return "no file changes".to_string();
    }
    let name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_string();
    let shown: Vec<String> = p
        .files
        .iter()
        .take(3)
        .map(|f| {
            let new = if f.created { " (new)" } else { "" };
            format!("{} +{} −{}{new}", name(&f.path), f.added, f.removed)
        })
        .collect();
    let more = p.files.len().saturating_sub(3);
    match more {
        0 => shown.join(", "),
        n => format!("{}, and {n} more", shown.join(", ")),
    }
}

/// What `files.restore` did, one line a file and the summary.
pub fn restored_lines(r: &FilesRestoreResult) -> Vec<Line> {
    let mut out = Vec::new();
    for f in &r.files {
        let (tag, tail) = match f.outcome.as_str() {
            "skipped" => (Tag::Dim, format!(" — {}", f.why)),
            _ => (Tag::Plain, String::new()),
        };
        out.push(Line::new(
            tag,
            format!("{:<14} {}{tail}", f.outcome, f.path),
        ));
    }
    out.push(Line::new(Tag::Plain, r.words.clone()));
    if !r.dry_run && r.files.iter().any(|f| f.outcome != "skipped") {
        out.push(Line::new(
            Tag::Dim,
            format!(
                "undo it with `theseus rewind --to @{} --files-only`",
                r.undo_since
            ),
        ));
    }
    out
}
