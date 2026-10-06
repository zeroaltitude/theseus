//! `theseus import` (theseus-0lrr.6): the operator's past history, from an
//! outside pipeline's episode files, into imported sessions.
//!
//! - `import openclaw <file>...` streams each file a batch at a time
//!   (`BATCH_LINES` lines or `BATCH_BYTES`, whichever comes first) through
//!   `import.episodes`, one batch in flight, with a short pause between
//!   batches, so a file of hundreds of megabytes is never read whole and the
//!   daemon answers others meanwhile. Each file's counts go to stdout, and
//!   each rejected line with its number.
//! - `import erase --tag <tag>` tombstones a tag's every session and node.
//! - `import list` shows each tag with its counts.
//!
//! The import and the erase are the owner's acts, refused inside a job
//! (`client::OPERATORS`).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::Subcommand;
use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEpisodesResult, ImportEraseParams, ImportEraseResult, ImportLine,
    ImportListResult, ImportRejected,
};
use theseus_protocol::method;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::cmd::output;

/// A batch's lines at most.
pub const BATCH_LINES: usize = 256;
/// A batch's bytes past which it is sent (a longer line goes alone).
pub const BATCH_BYTES: usize = 4 << 20;
/// The pause between two batches.
const PAUSE: Duration = Duration::from_millis(20);

#[derive(Subcommand, Debug)]
pub enum ImportCmd {
    /// Import episode files (JSON Lines, episode format 1): each episode an imported session,
    /// closed and private. An episode imported before is skipped; one whose hash changed is
    /// rejected and named, never overwritten.
    Openclaw {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Erase every session and node of a tag: each tombstoned, and the index told to forget it.
    Erase {
        #[arg(long)]
        tag: String,
        /// Why, in a few words, for the receipt.
        #[arg(long)]
        why: Option<String>,
    },
    /// Each tag with its sessions, nodes, erased sessions, and sources (default).
    List,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ImportCmd) -> Result<()> {
    match cmd {
        ImportCmd::Openclaw { files } => {
            for f in files {
                let r = file(conn, &f).await?;
                let v = serde_json::to_value(&r)?;
                let name = f.display().to_string();
                output(json, v, |r: ImportEpisodesResult| {
                    print!("{}", lines(&name, &r));
                    Ok(())
                })?;
            }
            Ok(())
        }
        ImportCmd::Erase { tag, why } => {
            let v = conn
                .request(
                    method::IMPORT_ERASE,
                    serde_json::to_value(ImportEraseParams { tag, why })?,
                )
                .await?;
            output(json, v, |r: ImportEraseResult| {
                println!(
                    "erased {}: {} and {} tombstoned in {} ({:.0} ms); index: {}",
                    r.tag,
                    count(r.sessions, "session"),
                    count(r.nodes, "node"),
                    count(r.frames, "frame"),
                    r.ms,
                    r.index
                );
                Ok(())
            })
        }
        ImportCmd::List => {
            let v = conn.request(method::IMPORT_LIST, Value::Null).await?;
            output(json, v, |r: ImportListResult| {
                print!("{}", list(&r));
                Ok(())
            })
        }
    }
}

/// One file, streamed a batch at a time: the batches' counts summed.
async fn file(conn: &mut Conn, path: &PathBuf) -> Result<ImportEpisodesResult> {
    let name = path.display().to_string();
    let f = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("opening {name}"))?;
    let mut reader = BufReader::new(f);
    let mut total = ImportEpisodesResult::default();
    let mut batch: Vec<ImportLine> = Vec::new();
    let mut bytes = 0;
    let mut number = 0u64;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .await
            .with_context(|| format!("reading {name}"))?;
        if n == 0 {
            break;
        }
        number += 1;
        match String::from_utf8(std::mem::take(&mut buf)) {
            Ok(text) => {
                bytes += text.len();
                batch.push(ImportLine { line: number, text });
            }
            Err(_) => {
                total.read += 1;
                total.rejected.push(ImportRejected {
                    line: number,
                    episode_id: None,
                    why: "not UTF-8".into(),
                });
            }
        }
        if batch.len() >= BATCH_LINES || bytes >= BATCH_BYTES {
            send(conn, &name, std::mem::take(&mut batch), &mut total).await?;
            bytes = 0;
            tokio::time::sleep(PAUSE).await;
        }
    }
    if !batch.is_empty() {
        send(conn, &name, batch, &mut total).await?;
    }
    Ok(total)
}

async fn send(
    conn: &mut Conn,
    file: &str,
    lines: Vec<ImportLine>,
    total: &mut ImportEpisodesResult,
) -> Result<()> {
    let p = ImportEpisodesParams {
        file: file.to_string(),
        lines,
    };
    let v = conn
        .request(method::IMPORT_EPISODES, serde_json::to_value(p)?)
        .await?;
    let r: ImportEpisodesResult = serde_json::from_value(v)?;
    total.read += r.read;
    total.imported += r.imported;
    total.skipped += r.skipped;
    total.nodes += r.nodes;
    total.frames += r.frames;
    total.ms += r.ms;
    total.rejected.extend(r.rejected);
    Ok(())
}

fn count(n: u64, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// A file's counts, then each rejected line.
pub fn lines(name: &str, r: &ImportEpisodesResult) -> String {
    let mut out = format!(
        "{name}: read {}, imported {}, skipped {}, rejected {} ({} in {}, {:.0} ms)\n",
        r.read,
        r.imported,
        r.skipped,
        r.rejected.len(),
        count(r.nodes, "node"),
        count(r.frames, "frame"),
        r.ms
    );
    let mut rejected = r.rejected.clone();
    rejected.sort_by_key(|x| x.line);
    for x in &rejected {
        match &x.episode_id {
            Some(id) => out.push_str(&format!("  line {} ({id}): {}\n", x.line, x.why)),
            None => out.push_str(&format!("  line {}: {}\n", x.line, x.why)),
        }
    }
    out
}

/// The tags, one a line.
pub fn list(r: &ImportListResult) -> String {
    if r.tags.is_empty() {
        return "no imports\n".into();
    }
    let mut out = String::new();
    for t in &r.tags {
        let sources: Vec<String> = t.sources.iter().map(|(s, n)| format!("{s} {n}")).collect();
        out.push_str(&format!(
            "{}: {}, {}, {} erased; {}\n",
            t.tag,
            count(t.sessions, "session"),
            count(t.nodes, "node"),
            t.erased,
            sources.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_counts_and_its_rejected_lines_by_number() {
        let r = ImportEpisodesResult {
            read: 4,
            imported: 2,
            skipped: 0,
            rejected: vec![
                ImportRejected {
                    line: 4,
                    episode_id: Some("ep_ab".into()),
                    why: "imported before with another hash".into(),
                },
                ImportRejected {
                    line: 2,
                    episode_id: None,
                    why: "not JSON: EOF".into(),
                },
            ],
            nodes: 5,
            frames: 1,
            ms: 3.2,
        };
        assert_eq!(
            lines("reef.jsonl", &r),
            "reef.jsonl: read 4, imported 2, skipped 0, rejected 2 (5 nodes in 1 frame, 3 ms)\n  \
             line 2: not JSON: EOF\n  line 4 (ep_ab): imported before with another hash\n"
        );
    }
}
