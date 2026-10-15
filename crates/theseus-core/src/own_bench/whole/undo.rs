//! A turn's directory as it was (own-benchmark part (c)): the copy of the
//! directory as it is now, with the session's recorded file edits taken back,
//! newest first, from the turn's start on. A turn whose writes cannot all be
//! taken back is not rebuildable, and the refusal names the file.
//!
//! What is recorded: `fs.edit`'s `old_string` and `new_string` (its input),
//! `fs.write`'s `created` flag and, for an overwrite, the diff the result
//! carries when it was not cut. A `proc.run`, a terminal or any tool that is
//! not one of those cannot be taken back: a program is counted and said as a
//! caveat (the files it changed are not known), any other writing tool refuses
//! the replay, and `fs.patch` does too (its reversal is not built).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::own_bench::rebuild::NotRebuildable;

/// One recorded change to a file.
#[derive(Debug, Clone, PartialEq)]
pub enum Written {
    Edit {
        path: PathBuf,
        old: String,
        new: String,
        all: bool,
    },
    Created {
        path: PathBuf,
    },
    /// `fs.write` over a file that existed: the diff the result carried, when
    /// it was whole.
    Overwrote {
        path: PathBuf,
        diff: Option<String>,
    },
}

impl Written {
    pub fn path(&self) -> &Path {
        match self {
            Written::Edit { path, .. }
            | Written::Created { path }
            | Written::Overwrote { path, .. } => path,
        }
    }
}

/// A call as the transcript recorded it: the tool, its input, whether it
/// succeeded, and its result's meta.
pub struct Recorded<'a> {
    pub tool: &'a str,
    pub input: &'a Value,
    pub ok: bool,
    pub meta: &'a Value,
}

/// The changes a span of calls made, oldest first, and the programs it ran.
#[derive(Debug, Default, PartialEq)]
pub struct Span {
    pub written: Vec<Written>,
    pub programs: u32,
}

fn no(why: impl Into<String>) -> NotRebuildable {
    NotRebuildable {
        part: "directory",
        why: why.into(),
    }
}

/// What a span of calls changed. `writes` says whether a tool changes files
/// (the registry's class); one that does and is not recorded refuses.
pub fn collect<'a>(
    calls: impl IntoIterator<Item = Recorded<'a>>,
    writes: impl Fn(&str) -> bool,
) -> Result<Span, NotRebuildable> {
    let mut span = Span::default();
    for c in calls.into_iter().filter(|c| c.ok) {
        let s = |k: &str| c.input.get(k).and_then(Value::as_str).map(str::to_string);
        let path = c
            .meta
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| c.input.get("path").and_then(Value::as_str))
            .map(PathBuf::from);
        match c.tool {
            "fs.edit" => {
                let (Some(path), Some(old), Some(new)) = (path, s("old_string"), s("new_string"))
                else {
                    return Err(no("an fs.edit in the span records no path or text"));
                };
                span.written.push(Written::Edit {
                    path,
                    old,
                    new,
                    all: c
                        .input
                        .get("replace_all")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
            "fs.write" => {
                let Some(path) = path else {
                    return Err(no("an fs.write in the span records no path"));
                };
                if c.meta.get("created").and_then(Value::as_bool) == Some(true) {
                    span.written.push(Written::Created { path });
                } else {
                    let diff = c
                        .meta
                        .get("diff")
                        .filter(|d| d.get("truncated").and_then(Value::as_bool) != Some(true));
                    span.written.push(Written::Overwrote {
                        path,
                        diff: diff
                            .and_then(|d| d.get("unified"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                }
            }
            "fs.patch" => {
                return Err(no(
                    "the span holds an fs.patch, whose reversal is not built, so the files it \
                     changed cannot be put back",
                ))
            }
            "proc.run" | "term.open" | "term.send" => span.programs += 1,
            other if writes(other) => {
                return Err(no(format!(
                    "{other} changes files and records nothing to put them back from"
                )))
            }
            _ => {}
        }
    }
    Ok(span)
}

/// Where `p` lies in the copy, or none when it is outside the directory.
pub fn within(orig: &Path, scratch: &Path, p: &Path) -> Option<PathBuf> {
    p.strip_prefix(orig).ok().map(|r| scratch.join(r))
}

/// Takes `written` back, newest first, in the copy at `scratch` of `orig`.
/// Paths outside `orig` are not in the copy and are left; the notes say so.
pub fn undo(
    orig: &Path,
    scratch: &Path,
    written: &[Written],
) -> Result<Vec<String>, NotRebuildable> {
    let mut notes = Vec::new();
    for w in written.iter().rev() {
        let Some(at) = within(orig, scratch, w.path()) else {
            notes.push(format!(
                "{} is outside the directory, so it is not in the copy",
                w.path().display()
            ));
            continue;
        };
        let name = w.path().display();
        match w {
            Written::Created { .. } => std::fs::remove_file(&at).map_err(|e| {
                no(format!(
                    "{name} was created in the span and cannot be removed: {e}"
                ))
            })?,
            Written::Edit { old, new, all, .. } => {
                let now = std::fs::read_to_string(&at)
                    .map_err(|e| no(format!("{name} cannot be read to take an edit back: {e}")))?;
                let n = if new.is_empty() {
                    0
                } else {
                    now.matches(new.as_str()).count()
                };
                if n == 0 || (n > 1 && !all) {
                    return Err(no(format!(
                        "{name} no longer holds the edit's text exactly once, so the edit cannot \
                         be taken back ({n} places)"
                    )));
                }
                let before = if *all {
                    now.replace(new.as_str(), old)
                } else {
                    now.replacen(new.as_str(), old, 1)
                };
                std::fs::write(&at, before)
                    .map_err(|e| no(format!("{name} cannot be written back: {e}")))?;
            }
            Written::Overwrote { diff, .. } => {
                let Some(diff) = diff else {
                    return Err(no(format!(
                        "{name} was overwritten in the span and no whole diff was recorded"
                    )));
                };
                let now = std::fs::read_to_string(&at)
                    .map_err(|e| no(format!("{name} cannot be read to take a write back: {e}")))?;
                let before = theseus_tools::change::unapply(diff, &now).map_err(|e| {
                    no(format!("{name} does not match the recorded diff, so the write cannot be taken back: {e}"))
                })?;
                std::fs::write(&at, before)
                    .map_err(|e| no(format!("{name} cannot be written back: {e}")))?;
            }
        }
    }
    Ok(notes)
}
