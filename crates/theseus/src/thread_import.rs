//! `theseus thread import` (theseus-71ib, T11): candidate threads from the
//! owner's earlier harness, then the ones the owner ticks.
//!
//! - **`--from openclaw`** (with no `--apply`, or `--dry-run`): the dry run.
//!   One `thread.import` call that writes nothing, a numbered table, and a
//!   plan file (`--plan FILE`, `thread-import-plan.txt` by default): one
//!   `[ ] N slug title` line a candidate, to tick with an `x`.
//! - **`--apply 1,3,5`** makes those numbers of the table a fresh dry run
//!   gives; **`--apply all-ticked`** makes the ticked lines of the plan file,
//!   which is read by slug, so a plan stays good while the history grows.
//!   Each thread is made with a card written from its episodes.
//! - **`--vestige PATH`** imports a Vestige dump (a file, or the
//!   `vestige*.json` files of a directory) as imported notes, once.
//!
//! The owner's act: `thread.import` and `import.episodes` are in
//! `client::OPERATORS`, so a job's shell is refused before anything is sent.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::import::{ImportEpisodesParams, ImportEpisodesResult, ImportLine};
use theseus_protocol::method;
use theseus_protocol::threads::WorkingSet;
use theseus_protocol::threads_import::{ThreadCandidate, ThreadImportParams, ThreadImportResult};

use crate::cmd::output;
use crate::vestige;

#[cfg(test)]
mod tests;

/// The plan file's name when none is given.
pub const PLAN: &str = "thread-import-plan.txt";
/// How many of a dump's episodes go in one batch.
const BATCH: usize = 200;

/// What the command was asked.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Args {
    /// The source of the streams: `openclaw`.
    #[arg(long)]
    pub from: Option<String>,
    /// Owner messages in the last N days count (30).
    #[arg(long)]
    pub since_days: Option<u32>,
    /// A DM topic cluster needs this many owner messages (8).
    #[arg(long)]
    pub min_cluster: Option<u32>,
    /// The default: read and write nothing but the plan file.
    #[arg(long)]
    pub dry_run: bool,
    /// Table numbers (`1,3,5`) or `all-ticked` (the plan file's ticked lines).
    #[arg(long)]
    pub apply: Option<String>,
    /// The plan file (thread-import-plan.txt).
    #[arg(long)]
    pub plan: Option<PathBuf>,
    /// A Vestige dump file, or a directory holding `vestige*.json`.
    #[arg(long)]
    pub vestige: Option<PathBuf>,
}

pub async fn run(conn: &mut Conn, json: bool, a: Args) -> Result<()> {
    if a.from.is_none() && a.vestige.is_none() {
        bail!("name what to import: `--from openclaw` for candidate threads, `--vestige PATH` for a dump");
    }
    if a.dry_run && a.apply.is_some() {
        bail!("--dry-run writes nothing and --apply writes: give one");
    }
    if let Some(path) = &a.vestige {
        vestige_run(conn, json, path).await?;
    }
    let Some(from) = a.from.clone() else {
        return Ok(());
    };
    let plan = a.plan.clone().unwrap_or_else(|| PathBuf::from(PLAN));
    let ask = |apply: Option<Vec<String>>| ThreadImportParams {
        from: from.clone(),
        since_days: a.since_days,
        min_cluster: a.min_cluster,
        apply,
        ..Default::default()
    };
    match a.apply.as_deref() {
        None => {
            let r = call(conn, &ask(None)).await?;
            let text = plan_file(&r);
            std::fs::write(&plan, &text)
                .with_context(|| format!("writing the plan file {}", plan.display()))?;
            if json {
                return print_json(&r);
            }
            for l in table_lines(&r) {
                println!("{l}");
            }
            println!(
                "\nPlan written to {}: tick the threads to make with an x, then\n  theseus thread import --from {} --apply all-ticked --plan {}\nor make some now: --apply 1,3,5",
                plan.display(),
                r.from,
                plan.display()
            );
            Ok(())
        }
        Some("all-ticked") => {
            let text = std::fs::read_to_string(&plan)
                .with_context(|| format!("reading the plan file {}", plan.display()))?;
            let slugs = ticked(&text);
            if slugs.is_empty() {
                bail!(
                    "no thread is ticked in {}: put an x in the brackets",
                    plan.display()
                );
            }
            applied(conn, json, &ask(Some(slugs))).await
        }
        Some(list) => {
            let numbers = numbers(list)?;
            let table = call(conn, &ask(None)).await?;
            let slugs = slugs_of(&table.candidates, &numbers)?;
            applied(conn, json, &ask(Some(slugs))).await
        }
    }
}

async fn call(conn: &mut Conn, p: &ThreadImportParams) -> Result<ThreadImportResult> {
    let v = conn
        .request(method::THREAD_IMPORT, serde_json::to_value(p)?)
        .await?;
    Ok(serde_json::from_value(v)?)
}

fn print_json(r: &ThreadImportResult) -> Result<()> {
    output(
        true,
        serde_json::to_value(r)?,
        |_: serde_json::Value| Ok(()),
    )
}

async fn applied(conn: &mut Conn, json: bool, p: &ThreadImportParams) -> Result<()> {
    let r = call(conn, p).await?;
    if json {
        return print_json(&r);
    }
    for l in applied_lines(&r) {
        println!("{l}");
    }
    Ok(())
}

/// `1,3,5` as numbers.
pub fn numbers(list: &str) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let n: u32 = part.parse().with_context(|| {
            format!("{part:?} is no table number: write --apply 1,3,5 or all-ticked")
        })?;
        if !out.contains(&n) {
            out.push(n);
        }
    }
    if out.is_empty() {
        bail!("--apply names no table number");
    }
    Ok(out)
}

/// The slugs of the table's numbers.
pub fn slugs_of(cands: &[ThreadCandidate], numbers: &[u32]) -> Result<Vec<String>> {
    numbers
        .iter()
        .map(|n| {
            cands
                .iter()
                .find(|c| c.n == *n)
                .map(|c| c.slug.clone())
                .with_context(|| format!("no candidate {n}: the table has {}", cands.len()))
        })
        .collect()
}

/// The plan file: a header, and a line for each candidate.
pub fn plan_file(r: &ThreadImportResult) -> String {
    let mut out = format!(
        "# theseus thread import plan: {} streams of {}, {} days, DM clusters of {}+ messages\n\
         # Put an x in the brackets of each thread to make: [x]. Lines may be reordered or deleted.\n\
         # Then: theseus thread import --from {} --apply all-ticked --plan THIS-FILE\n",
        r.candidates.len(),
        r.from,
        r.since_days,
        r.min_cluster,
        r.from
    );
    for c in &r.candidates {
        let exists = if c.exists {
            "  (exists: will be skipped)"
        } else {
            ""
        };
        out.push_str(&format!(
            "[ ] {} {} {}  ({}, {} owner messages){exists}\n",
            c.n, c.slug, c.title, c.kind, c.owner_messages
        ));
    }
    out
}

/// The slugs of the ticked lines of a plan file, in order.
pub fn ticked(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line
            .strip_prefix("[x]")
            .or_else(|| line.strip_prefix("[X]"))
        else {
            continue;
        };
        let mut words = rest.split_whitespace();
        if let (Some(n), Some(slug)) = (words.next(), words.next()) {
            if n.bytes().all(|b| b.is_ascii_digit()) && !out.iter().any(|s| s == slug) {
                out.push(slug.to_string());
            }
        }
    }
    out
}

fn set_line(w: &WorkingSet) -> String {
    let mut parts = Vec::new();
    for (k, v) in [
        ("repos", &w.repos),
        ("refs", &w.refs),
        ("branches", &w.branches),
        ("files", &w.files),
        ("hosts", &w.hosts),
    ] {
        if !v.is_empty() {
            parts.push(format!("{k} {}", v.join(", ")));
        }
    }
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join("; ")
    }
}

fn counts(c: &ThreadCandidate) -> String {
    c.sensitivity
        .iter()
        .map(|(k, n)| format!("{k} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The dry run's table: a header, a row for each candidate, and under it
/// the working set, the sensitivity counts and any note.
pub fn table_lines(r: &ThreadImportResult) -> Vec<String> {
    if r.candidates.is_empty() {
        let mut out = vec![format!(
            "No candidates from {} in the last {} days.",
            r.from, r.since_days
        )];
        out.extend(r.notes.iter().map(|n| format!("  {n}")));
        return out;
    }
    let w = r.candidates.iter().map(|c| c.slug.len()).max().unwrap_or(4);
    let tw = r
        .candidates
        .iter()
        .map(|c| c.title.len())
        .max()
        .unwrap_or(5)
        .min(32);
    let mut out = vec![format!(
        "{:>3}  {:<w$}  {:<tw$}  {:<8}  {:>5}  {:<16}  {:<12}  topics",
        "#", "slug", "title", "persona", "msgs", "last active", "kind"
    )];
    for c in &r.candidates {
        let title: String = c.title.chars().take(tw).collect();
        out.push(format!(
            "{:>3}  {:<w$}  {:<tw$}  {:<8}  {:>5}  {:<16}  {:<12}  {}",
            c.n,
            c.slug,
            title,
            c.persona.as_deref().unwrap_or("-"),
            c.owner_messages,
            fmt_stamp(c.last_active_ms),
            if c.exists { "exists" } else { c.kind.as_str() },
            if c.topics.is_empty() {
                "-".into()
            } else {
                c.topics.join(", ")
            },
        ));
        out.push(format!("       working set: {}", set_line(&c.working_set)));
        out.push(format!(
            "       sensitivity: {} ({} episodes)",
            counts(c),
            c.episodes
        ));
        out.extend(c.notes.iter().map(|n| format!("       {n}")));
    }
    out.extend(r.notes.iter().map(|n| format!("left out: {n}")));
    out
}

/// What an apply made, and did not.
pub fn applied_lines(r: &ThreadImportResult) -> Vec<String> {
    let mut out = Vec::new();
    for a in &r.applied {
        out.push(format!(
            "Made the thread {}: {} card lines citing {} episodes (${:.4}).",
            a.slug,
            a.card.len(),
            a.cited.len(),
            a.cost_usd
        ));
        out.extend(a.card.iter().map(|l| format!("  {l}")));
        if let Some(why) = &a.card_failed {
            out.push(format!("  no card: {why}"));
        }
    }
    for s in &r.skipped {
        out.push(format!("Skipped {}: {}", s.slug, s.why));
    }
    out.push(format!(
        "{} made, {} skipped; cards cost ${:.4} (${:.4} of ${:.2} today).",
        r.applied.len(),
        r.skipped.len(),
        r.spent_usd,
        r.spent_today_usd,
        r.cap_usd
    ));
    out
}

/// Import a Vestige dump (or a directory's), once.
async fn vestige_run(conn: &mut Conn, json: bool, path: &Path) -> Result<()> {
    theseus_client::client::refuse_in_a_job(
        method::IMPORT_EPISODES,
        theseus_client::client::job_session().as_deref(),
    )?;
    let files = vestige::dumps_at(path)?;
    if files.is_empty() {
        bail!("{} holds no vestige*.json dump", path.display());
    }
    let (mut read, mut imported, mut skipped) = (0u64, 0u64, 0u64);
    for f in files {
        let name = f
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("vestige.json")
            .to_string();
        let mtime = std::fs::metadata(&f)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64);
        let text =
            std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))?;
        let memories = vestige::parse(&text, mtime).with_context(|| f.display().to_string())?;
        let mut seen = std::collections::BTreeSet::new();
        let mut lines = Vec::new();
        for (i, m) in memories.iter().enumerate() {
            let (id, line) = vestige::episode(m, &name, i);
            if seen.insert(id) {
                lines.push(ImportLine {
                    line: i as u64 + 1,
                    text: line,
                });
            }
        }
        read += memories.len() as u64;
        for batch in lines.chunks(BATCH) {
            let p = ImportEpisodesParams {
                file: f.display().to_string(),
                lines: batch.to_vec(),
            };
            let v = conn
                .request(method::IMPORT_EPISODES, serde_json::to_value(p)?)
                .await?;
            let r: ImportEpisodesResult = serde_json::from_value(v)?;
            imported += r.imported;
            // The same note again, even under another time, is the same note.
            for x in &r.rejected {
                if x.why.starts_with("imported before with another hash") {
                    skipped += 1;
                } else {
                    eprintln!("{}: {}", f.display(), x.why);
                }
            }
            skipped += r.skipped;
        }
    }
    if !json {
        println!(
            "Vestige: {read} notes read, {imported} imported, {skipped} already imported (once by content)."
        );
    }
    Ok(())
}
