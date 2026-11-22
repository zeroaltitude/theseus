//! `theseus import vestige PATH` (theseus-71ib): a Vestige dump (a file, or
//! the `vestige*.json` files of a directory) imported as notes, once, through
//! the session import's `import.episodes`. It is a note import, not a thread
//! import: it lived under `theseus thread import --vestige PATH` until threads
//! were retired (2026-11-22), and that spelling still runs it, saying it moved.
//!
//! The thread importer that shared this command (`--from openclaw`, candidate
//! threads from the owner's earlier harness, and `--apply`) is gone (the owner,
//! 2026-11-21).
//!
//! The owner's act: `import.episodes` is in `client::OPERATORS`, so a job's
//! shell is refused before anything is sent.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use theseus_client::Conn;
use theseus_protocol::import::{ImportEpisodesParams, ImportEpisodesResult, ImportLine};
use theseus_protocol::method;

use crate::vestige;

/// How many of a dump's episodes go in one batch.
const BATCH: usize = 200;

/// What the command was asked.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Args {
    /// A Vestige dump file, or a directory holding `vestige*.json`.
    #[arg(long)]
    pub vestige: Option<PathBuf>,
}

/// `theseus thread import --vestige PATH`: moved, and still run.
pub async fn run(conn: &mut Conn, json: bool, a: Args) -> Result<()> {
    let Some(path) = &a.vestige else {
        bail!("name what to import: `theseus import vestige PATH` (this command moved there)");
    };
    eprintln!(
        "`theseus thread import` moved: `theseus import vestige {}` does this now.",
        path.display()
    );
    vestige(conn, json, path).await
}

/// Import a Vestige dump (or a directory's), once.
pub async fn vestige(conn: &mut Conn, json: bool, path: &Path) -> Result<()> {
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
                batch: None,
                batches: None,
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
