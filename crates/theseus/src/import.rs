//! `theseus import` (theseus-0lrr.6): the operator's past history, from an
//! outside pipeline's episode files, into imported sessions.
//!
//! - `import openclaw <file>...` streams each file a batch at a time
//!   (`BATCH_LINES` lines or `BATCH_BYTES`, whichever comes first) through
//!   `import.episodes`, one batch in flight, with a short pause between
//!   batches, so a file of hundreds of megabytes is never read whole and the
//!   daemon answers others meanwhile. Each file's counts go to stdout, and
//!   each rejected line with its number.
//! - `import erase --tag <tag>` tombstones a tag's every session and node,
//!   and takes back the topics' memberships and the topics nothing uses.
//! - `import topics --tag <tag>` makes a tag's sessions' topic labels the
//!   ontology's topics and memberships (theseus-anh3): run once after an
//!   import; a second run changes nothing.
//! - `import list` shows each tag with its counts.
//!
//! The import, the erase and the topics are the owner's acts, refused inside a job
//! (`client::OPERATORS`).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use clap::Subcommand;
use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEpisodesResult, ImportEraseParams, ImportEraseResult, ImportLine,
    ImportListResult, ImportPeopleParams, ImportPeopleResult, ImportRejected, ImportTopicsParams,
    ImportTopicsResult,
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
    /// Make a tag's sessions' topic labels the ontology's topics (a tree from their slash paths)
    /// and memberships (at most the topic kind's per session; the rest stay labels). From the
    /// stored labels; a second run changes nothing; the tag's erase takes them back.
    Topics {
        #[arg(long)]
        tag: String,
    },
    /// Make a tag's sessions' people the ontology's (theseus-wy7y): each message author who is
    /// a person and each DM's other party, one person each with their handles (found by an
    /// exact handle first, never by a display name alone), and each session they spoke in or
    /// were the DM of, a membership. From the stored records, no model call; a second run
    /// changes nothing; the tag's erase takes them back. TAG may be left out when one tag is
    /// imported.
    ///
    /// With --propose: people from the sessions' text instead (theseus-wy7y). A model
    /// ([people] extract_profile) reads each session's human-facing text and names its people,
    /// Jev judges each, and each kept one is a proposal (`theseus ontology proposals`), never a
    /// membership. Paced by the machine's quiet, under --cap, resumable: a second run goes on
    /// from where the first stopped. --dry-run prints the sessions, tokens and projected cost.
    People {
        tag: Option<String>,
        /// Count what it would do, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Propose people from the sessions' text (a model and Jev; it costs money).
        #[arg(long)]
        propose: bool,
        /// --propose's spend cap for this run, in dollars.
        #[arg(long, default_value_t = 5.0, requires = "propose")]
        cap: f64,
    },
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
                    "erased {}: {} and {} tombstoned in {} ({:.0} ms); index: {}; topics: the \
                     memberships of {} emptied, {} taken away; {} taken away",
                    r.tag,
                    count(r.sessions, "session"),
                    count(r.nodes, "node"),
                    count(r.frames, "frame"),
                    r.ms,
                    r.index,
                    count(r.memberships, "session"),
                    count(r.topics, "topic"),
                    people_count(r.people),
                );
                Ok(())
            })
        }
        ImportCmd::Topics { tag } => {
            let v = conn
                .request(
                    method::IMPORT_TOPICS,
                    serde_json::to_value(ImportTopicsParams { tag })?,
                )
                .await?;
            output(json, v, |r: ImportTopicsResult| {
                print!("{}", topics(&r));
                Ok(())
            })
        }
        ImportCmd::People {
            tag,
            dry_run,
            propose,
            cap,
        } => {
            let tag = match tag {
                Some(t) => t,
                None => {
                    let v = conn.request(method::IMPORT_LIST, Value::Null).await?;
                    let l: ImportListResult = serde_json::from_value(v)?;
                    match l.tags.as_slice() {
                        [one] => one.tag.clone(),
                        [] => bail!("nothing is imported: no tag to read people from"),
                        many => bail!(
                            "name a tag: {} are imported ({})",
                            many.len(),
                            many.iter()
                                .map(|t| t.tag.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }
                }
            };
            let v = conn
                .request(
                    method::IMPORT_PEOPLE,
                    serde_json::to_value(ImportPeopleParams {
                        tag,
                        dry_run,
                        propose,
                        cap_usd: propose.then_some(cap),
                    })?,
                )
                .await?;
            output(json, v, |r: ImportPeopleResult| {
                print!("{}", people(&r));
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

fn people_count(n: u64) -> String {
    match n {
        1 => "1 person".into(),
        n => format!("{n} people"),
    }
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

/// What `import people` did, or would do, in a line.
pub fn people(r: &ImportPeopleResult) -> String {
    if let Some(p) = &r.propose {
        return propose(&r.tag, r.dry_run, p, r.ms);
    }
    format!(
        "people of {}{}: {} read, {} found ({} held already, {} {}){}; {} {}, {} memberships of \
         origin import{}; {} ({:.0} ms)\n",
        r.tag,
        if r.dry_run {
            " (dry run: nothing written)"
        } else {
            ""
        },
        count(r.sessions, "session"),
        people_count(r.people),
        r.held,
        r.made,
        if r.dry_run { "to declare" } else { "declared" },
        match r.excluded {
            0 => String::new(),
            n => format!(", {n} excluded (the owner, agents, bots)"),
        },
        count(r.joined, "session"),
        if r.dry_run { "to join" } else { "joined" },
        r.memberships,
        match r.capped {
            0 => String::new(),
            n => format!(", {} capped at {}", count(n, "session"), 12),
        },
        count(r.frames, "frame"),
        r.ms
    )
}

/// What `import people --propose` did, or would do (theseus-wy7y).
pub fn propose(
    tag: &str,
    dry_run: bool,
    p: &theseus_protocol::import::PeopleProposeReport,
    ms: f64,
) -> String {
    let mut out = match dry_run {
        true => format!(
            "people proposed from {tag} (dry run: no call): {} of {} to read ({} done before, {} \
             with no human-facing text), about {} tokens to {} ({}); projected ${:.4} with Jev's \
             (cap ${})\n",
            p.read,
            count(p.sessions, "session"),
            p.done_before,
            p.no_text,
            p.tokens,
            p.profile,
            p.model,
            p.projected_usd,
            p.cap_usd
        ),
        false => format!(
            "people proposed from {tag}: {} of {} read ({} done before, {} with no human-facing \
             text), {} candidates ({} excluded), {} judged by Jev, {} failed; spent ${:.4} of \
             ${} ({:.0} ms)\n",
            p.read,
            count(p.sessions, "session"),
            p.done_before,
            p.no_text,
            p.candidates,
            p.excluded,
            p.judged,
            p.failed,
            p.spent_usd,
            p.cap_usd,
            ms
        ),
    };
    if let Some(why) = &p.stopped {
        out.push_str(&format!("{why}\n"));
    } else if !dry_run {
        out.push_str("the proposals: theseus ontology proposals; accept: theseus ontology accept --kind person\n");
    }
    out
}

/// What `import topics` did, in a line.
pub fn topics(r: &ImportTopicsResult) -> String {
    format!(
        "topics of {}: {} read, {} labels as {} ({} declared now); {} joined now, {} held \
         from the import; {} capped, {} unplaced; {} ({:.0} ms)\n",
        r.tag,
        count(r.sessions, "session"),
        r.labels,
        count(r.topics, "topic"),
        r.made,
        count(r.joined, "session"),
        count(r.memberships, "membership"),
        r.capped,
        r.unplaced,
        count(r.frames, "frame"),
        r.ms
    )
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

    /// `import people --propose`'s lines: a dry run's price, and a run
    /// stopped at its cap saying how to go on (theseus-wy7y).
    #[test]
    fn a_propose_runs_lines_say_its_price_and_its_stop() {
        let p = theseus_protocol::import::PeopleProposeReport {
            sessions: 4,
            done_before: 1,
            no_text: 1,
            read: 2,
            tokens: 1633,
            profile: "haiku".into(),
            model: "claude-haiku-5-5".into(),
            projected_usd: 0.0014,
            cap_usd: 5.0,
            ..Default::default()
        };
        assert_eq!(
            propose("tern-2026-05", true, &p, 1.0),
            "people proposed from tern-2026-05 (dry run: no call): 2 of 4 sessions to read (1 done \
             before, 1 with no human-facing text), about 1633 tokens to haiku (claude-haiku-5-5); \
             projected $0.0014 with Jev's (cap $5)\n"
        );
        let stopped = theseus_protocol::import::PeopleProposeReport {
            candidates: 3,
            excluded: 1,
            judged: 2,
            spent_usd: 0.5,
            cap_usd: 0.4,
            left: 2,
            stopped: Some("stopped at the cap: go on from the tag's mark".into()),
            ..p
        };
        assert_eq!(
            propose("tern-2026-05", false, &stopped, 40.0),
            "people proposed from tern-2026-05: 2 of 4 sessions read (1 done before, 1 with no \
             human-facing text), 3 candidates (1 excluded), 2 judged by Jev, 0 failed; spent \
             $0.5000 of $0.4 (40 ms)\nstopped at the cap: go on from the tag's mark\n"
        );
    }
}
