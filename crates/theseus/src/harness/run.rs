//! `theseus import claude-code|codex` (theseus-o73y): walk a directory of a
//! harness's session files, turn each into an episode (`convert`, in a
//! blocking thread: a file is read once and never whole into the daemon's
//! hands), and send the episodes to `import.episodes` a batch at a time, one
//! in flight, with a short pause between, as the openclaw import does, so the
//! daemon serves others meanwhile. Each file's counts go to stdout, each of
//! its rejected lines with its number.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use serde::Serialize;
use theseus_client::Conn;
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEpisodesResult, ImportLine, ImportListResult,
};
use theseus_protocol::method;

use super::{convert, Counts, Source};
use crate::cmd::output;
use crate::import::{BATCH_BYTES, BATCH_LINES, PAUSE};

/// What the daemon says of an episode whose id it holds under another hash:
/// the file grew (or changed) since it was imported.
const CHANGED: &str = "imported before with another hash";

/// One file's line in the report.
#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    pub file: String,
    #[serde(flatten)]
    pub counts: Counts,
    /// The file had no message to import.
    pub empty: bool,
}

/// A run's whole report (`--json`'s object).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub source: String,
    pub dir: String,
    pub tag: String,
    pub files: u64,
    pub empty_files: u64,
    /// Subagent files left out whole (Claude Code).
    pub sidechain_files: u64,
    pub unreadable: Vec<String>,
    pub imported: u64,
    /// Files that grew since they were imported: only their new messages were
    /// written (theseus-pjmj7).
    pub appended: u64,
    pub skipped: u64,
    /// Files that changed since they were imported in a way an append cannot
    /// take (a stored message differs): not imported again.
    pub changed: Vec<String>,
    pub rejected: Vec<String>,
    pub nodes: u64,
    pub frames: u64,
    pub ms: f64,
    pub per_file: Vec<FileReport>,
}

/// The default directory of a source's files.
fn default_dir(source: Source) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set: name the directory")?;
    Ok(PathBuf::from(home).join(match source {
        Source::ClaudeCode => ".claude/projects",
        Source::Codex => ".codex/sessions",
    }))
}

/// The session files under `dir`, sorted, and the count of sidechain files
/// left out.
pub fn files_under(source: Source, dir: &Path) -> Result<(Vec<PathBuf>, u64)> {
    let mut found = Vec::new();
    let mut sidechain = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))?;
        for e in rd {
            let e = e.with_context(|| format!("reading {}", d.display()))?;
            let path = e.path();
            let t = e.file_type()?;
            if t.is_dir() {
                stack.push(path);
            } else if t.is_file() && path.extension().is_some_and(|x| x == "jsonl") {
                let sub = source == Source::ClaudeCode
                    && path
                        .strip_prefix(dir)
                        .unwrap_or(&path)
                        .components()
                        .any(|c| c.as_os_str() == "subagents");
                if sub {
                    sidechain += 1;
                } else {
                    found.push(path);
                }
            }
        }
    }
    found.sort();
    Ok((found, sidechain))
}

/// The tag a run uses when none is named: the live tag already holding this
/// source's sessions (a second run, on any day, lands in the tag the first
/// made, so an unchanged file is skipped, never rejected for the tag its
/// hash names), else `<source>-<today>`.
pub fn default_tag(listed: &ImportListResult, source: Source, today: &str) -> String {
    listed
        .tags
        .iter()
        .filter(|t| t.sources.contains_key(source.name()) && t.sessions > t.erased)
        .max_by_key(|t| t.updated_ms)
        .map_or_else(|| format!("{}-{today}", source.name()), |t| t.tag.clone())
}

fn tag_ok(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 64
        && tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// A file's line of the report.
pub fn file_line(r: &FileReport) -> String {
    let c = &r.counts;
    let mut out = format!(
        "{}: read {} lines, {} messages ({} tool lines)",
        r.file, c.read, c.messages, c.tool_lines
    );
    if c.cut > 0 {
        out.push_str(&format!(", {} cut at their bound", c.cut));
    }
    if c.skipped_total() > 0 {
        let by: Vec<String> = c.skipped.iter().map(|(k, n)| format!("{k} {n}")).collect();
        out.push_str(&format!(
            ", skipped {} ({})",
            c.skipped_total(),
            by.join(", ")
        ));
    }
    out.push_str(&format!(", rejected {}", c.rejected.len()));
    if r.empty {
        out.push_str(", nothing to import");
    }
    out.push('\n');
    for (n, why) in &c.rejected {
        out.push_str(&format!("  line {n}: {why}\n"));
    }
    out
}

/// A batch waiting to be sent: the lines and the file each came from.
#[derive(Default)]
struct Pending {
    lines: Vec<ImportLine>,
    paths: Vec<String>,
    bytes: usize,
}

/// The import is the owner's act: refused inside a job, before a file is read.
pub(super) fn operator_only(session: Option<&str>) -> Result<()> {
    theseus_client::client::refuse_in_a_job(method::IMPORT_EPISODES, session)
}

/// The directory and the tag a run uses.
async fn resolve(
    conn: &mut Conn,
    source: Source,
    dir: Option<PathBuf>,
    tag: Option<String>,
) -> Result<(PathBuf, String)> {
    operator_only(theseus_client::client::job_session().as_deref())?;
    let dir = match dir {
        Some(d) => d,
        None => default_dir(source)?,
    };
    if !dir.is_dir() {
        bail!(
            "{} is not a directory: name where {}'s session files are",
            dir.display(),
            source.name()
        );
    }
    let tag = match tag {
        Some(t) => t,
        None => {
            let v = conn
                .request(method::IMPORT_LIST, serde_json::Value::Null)
                .await?;
            let listed: ImportListResult = serde_json::from_value(v)?;
            let today = jiff::Zoned::now().strftime("%Y-%m-%d").to_string();
            default_tag(&listed, source, &today)
        }
    };
    if !tag_ok(&tag) {
        bail!("tag {tag:?}: 1 to 64 letters, digits, `.`, `_` or `-`");
    }
    Ok((dir, tag))
}

pub async fn run(
    conn: &mut Conn,
    json: bool,
    source: Source,
    dir: Option<PathBuf>,
    tag: Option<String>,
) -> Result<()> {
    let (dir, tag) = resolve(conn, source, dir, tag).await?;
    let (files, sidechain) = {
        let d = dir.clone();
        tokio::task::spawn_blocking(move || files_under(source, &d)).await??
    };
    let mut report = Report {
        source: source.name().into(),
        dir: dir.display().to_string(),
        tag: tag.clone(),
        sidechain_files: sidechain,
        ..Report::default()
    };
    if !json {
        println!(
            "importing {} session files of {} from {} as tag {tag}",
            files.len(),
            source.name(),
            dir.display()
        );
    }
    let mut pending = Pending::default();
    for (i, path) in files.iter().enumerate() {
        report.files += 1;
        let name = path.display().to_string();
        let p = path.clone();
        let t = tag.clone();
        let read = tokio::task::spawn_blocking(move || convert_file(source, &p, &t)).await?;
        let c = match read {
            Ok(c) => c,
            Err(e) => {
                let why = format!("{name}: {e}");
                if !json {
                    eprintln!("{why}");
                }
                report.unreadable.push(why);
                continue;
            }
        };
        let fr = FileReport {
            file: name.clone(),
            counts: c.counts,
            empty: c.episode.is_none(),
        };
        if !json {
            print!("{}", file_line(&fr));
        }
        if fr.empty {
            report.empty_files += 1;
        }
        if let Some(text) = c.episode {
            pending.bytes += text.len();
            pending.paths.push(name);
            pending.lines.push(ImportLine {
                line: i as u64 + 1,
                text,
            });
        }
        report.per_file.push(fr);
        if pending.lines.len() >= BATCH_LINES || pending.bytes >= BATCH_BYTES {
            send(
                conn,
                &dir,
                std::mem::take(&mut pending),
                &files,
                &mut report,
            )
            .await?;
            tokio::time::sleep(PAUSE).await;
        }
    }
    if !pending.lines.is_empty() {
        send(conn, &dir, pending, &files, &mut report).await?;
    }
    if json {
        output(
            true,
            serde_json::to_value(&report)?,
            |_: serde_json::Value| Ok(()),
        )
    } else {
        print!("{}", summary(&report));
        Ok(())
    }
}

/// One file read, in a blocking thread.
fn convert_file(source: Source, path: &Path, tag: &str) -> Result<super::Converted> {
    let f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let file = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("session.jsonl");
    convert(source, std::io::BufReader::new(f), stem, file, tag)
        .with_context(|| format!("reading {}", path.display()))
}

/// Send a batch; its answer into the report, each rejected line named by its file.
async fn send(
    conn: &mut Conn,
    dir: &Path,
    batch: Pending,
    files: &[PathBuf],
    report: &mut Report,
) -> Result<()> {
    let p = ImportEpisodesParams {
        file: dir.display().to_string(),
        lines: batch.lines,
    };
    let v = conn
        .request(method::IMPORT_EPISODES, serde_json::to_value(p)?)
        .await?;
    let r: ImportEpisodesResult = serde_json::from_value(v)?;
    report.imported += r.imported;
    report.appended += r.appended;
    report.skipped += r.skipped;
    report.nodes += r.nodes;
    report.frames += r.frames;
    report.ms += r.ms;
    for x in r.rejected {
        let path = files
            .get((x.line as usize).wrapping_sub(1))
            .map_or_else(|| format!("line {}", x.line), |p| p.display().to_string());
        let said = format!("{path}: {}", x.why);
        if x.why.starts_with(CHANGED) {
            report.changed.push(said);
        } else {
            report.rejected.push(said);
        }
    }
    Ok(())
}

/// What the run did, in lines.
pub fn summary(r: &Report) -> String {
    let appended = if r.appended > 0 {
        format!(", {} appended", r.appended)
    } else {
        String::new()
    };
    let mut out = format!(
        "{} of {} into tag {}: {} files, {} imported{appended}, {} skipped (imported before), \
         {} nodes in {} frames ({:.0} ms)\n",
        r.dir, r.source, r.tag, r.files, r.imported, r.skipped, r.nodes, r.frames, r.ms
    );
    if r.empty_files > 0 {
        out.push_str(&format!(
            "  {} files with nothing to import\n",
            r.empty_files
        ));
    }
    if r.sidechain_files > 0 {
        out.push_str(&format!(
            "  {} subagent files left out (the session's own lines hold what a subagent returned)\n",
            r.sidechain_files
        ));
    }
    if !r.changed.is_empty() {
        out.push_str(&format!(
            "  {} files changed since they were imported in a way an append cannot take (a stored \
             message differs), and were not imported again (erase the tag and import again to \
             take their newer text):\n",
            r.changed.len()
        ));
        for c in &r.changed {
            out.push_str(&format!("    {c}\n"));
        }
    }
    for c in r.unreadable.iter().chain(&r.rejected) {
        out.push_str(&format!("  rejected {c}\n"));
    }
    out
}
