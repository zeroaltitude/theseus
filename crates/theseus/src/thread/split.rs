//! `theseus thread split` and `unsplit` (theseus-vt9k; theseus-q9b85): whole turns of a thread, or of
//! a place's own session (its home), move into a thread, and the source's compile leaves them out.

use anyhow::{bail, Result};
use clap::Args;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::threads::{
    ThreadSplitParams, ThreadSplitPlan, ThreadSplitResult, ThreadUnsplitParams, ThreadUnsplitResult,
};

use crate::cmd::output;

#[derive(Args, Debug)]
pub struct SplitArgs {
    /// The source: a thread's slug, a place's own session by its whole id (`ses_…`; `theseus
    /// places` lists them), or `home@<place>`.
    pub from: String,
    /// The new thread's name; leave it out with `--into`.
    pub new: Option<String>,
    /// Put the turns at the tail of this existing thread, in their order, instead of a new thread.
    #[arg(long)]
    pub into: Option<String>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub goal: Option<String>,
    /// What the range is about, the words that stand in its place in the source's compile
    /// (default: its first human message, cut to 80 characters).
    #[arg(long)]
    pub gist: Option<String>,
    /// The first position of the range to move (`@1234` or `1234`).
    #[arg(long)]
    pub first: Option<String>,
    /// The last position of the range.
    #[arg(long)]
    pub last: Option<String>,
    /// One more range, `@A-@B` (repeat the flag): a home's old streams interleave by date.
    #[arg(long = "range", value_name = "@A-@B")]
    pub ranges: Vec<String>,
    /// One message to move, by position; repeat the flag.
    #[arg(long = "at")]
    pub at: Vec<String>,
    /// Write nothing: print what would move and the next request's tokens before and after.
    #[arg(long)]
    pub dry_run: bool,
}

/// A position as `@1234` or `1234`.
fn position(s: &str) -> Result<u64> {
    s.trim().trim_start_matches('@').parse().map_err(|_| {
        anyhow::anyhow!("`{s}` is no position: write `@1234` as `theseus history` shows it")
    })
}

/// A range `@A-@B`.
fn span(s: &str) -> Result<[u64; 2]> {
    let Some((a, b)) = s.split_once('-') else {
        bail!("`{s}` is no range: write `@A-@B`");
    };
    Ok([position(a)?, position(b)?])
}

/// The flags as the request: one range, several, or a list of positions.
pub fn params(a: SplitArgs) -> Result<ThreadSplitParams> {
    let named = [
        a.first.is_some() || a.last.is_some(),
        !a.ranges.is_empty(),
        !a.at.is_empty(),
    ];
    if named.iter().filter(|n| **n).count() != 1 {
        bail!(
            "name the turns: `--first @A --last @B`, or `--range @A-@B` (repeat it), or `--at @N` \
             for each message"
        );
    }
    if a.new.is_some() == a.into.is_some() {
        bail!("name where the turns go: a new thread (`NEW`), or an existing one (`--into SLUG`)");
    }
    let (range, positions, ranges) = match (a.first, a.last) {
        (Some(x), Some(y)) => (Some([position(&x)?, position(&y)?]), None, vec![]),
        (None, None) if !a.ranges.is_empty() => (
            None,
            None,
            a.ranges
                .iter()
                .map(|r| span(r))
                .collect::<Result<Vec<_>>>()?,
        ),
        (None, None) => (
            None,
            Some(
                a.at.iter()
                    .map(|p| position(p))
                    .collect::<Result<Vec<_>>>()?,
            ),
            vec![],
        ),
        _ => bail!("`--first` and `--last` come together"),
    };
    Ok(ThreadSplitParams {
        from: a.from,
        slug: a.new.unwrap_or_default(),
        title: a.title,
        goal: a.goal,
        gist: a.gist,
        into: a.into,
        range,
        positions,
        ranges,
        ..Default::default()
    })
}

pub async fn split(conn: &mut Conn, json: bool, a: SplitArgs) -> Result<()> {
    let dry = a.dry_run;
    let p = params(a)?;
    if dry {
        let v = conn
            .request(method::THREAD_SPLIT_PLAN, serde_json::to_value(&p)?)
            .await?;
        return output(json, v, |r: ThreadSplitPlan| {
            for l in plan_lines(&r) {
                println!("{l}");
            }
            Ok(())
        });
    }
    let v = conn
        .request(method::THREAD_SPLIT, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: ThreadSplitResult| {
        for l in split_lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

pub async fn unsplit(conn: &mut Conn, json: bool, split: String) -> Result<()> {
    let p = ThreadUnsplitParams {
        split,
        ..Default::default()
    };
    let v = conn
        .request(method::THREAD_UNSPLIT, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: ThreadUnsplitResult| {
        for l in unsplit_lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

fn messages(n: u32) -> String {
    match n {
        1 => "1 message".to_string(),
        n => format!("{n} messages"),
    }
}

/// What a split did, in lines.
pub fn split_lines(r: &ThreadSplitResult) -> Vec<String> {
    let t = &r.thread.thread;
    let mut out = vec![if r.into {
        format!(
            "Moved {} into {} from {}, @{} to @{}, at the end of {}'s conversation (its cache \
             prefix is unchanged).",
            messages(r.quoted),
            t.slug,
            r.from,
            r.first_pos,
            r.last_pos,
            t.slug
        )
    } else {
        format!(
            "Split {} out of {}: {} quoted, @{} to @{}. `theseus ask --thread {}` goes on from them.",
            t.slug,
            r.from,
            messages(r.quoted),
            r.first_pos,
            r.last_pos,
            t.slug
        )
    }];
    if r.noted {
        out.push(format!(
            "{} nodes left {}'s compile; its place holds one line, `moved to thread «{}»: …`. The \
             nodes are all still in the store: `history` marks them moved, and recall finds them.",
            r.moved, r.from, t.slug
        ));
        out.push(format!(
            "Cache: {}'s prefix changed once, so its next turn writes its cache again; no other \
             turn's prefix changed.",
            r.from
        ));
        out.push(format!(
            "`theseus thread unsplit {}` undoes it, and nothing is deleted.",
            r.split
        ));
    } else {
        out.push(format!(
            "{} had a turn running, so its compile still holds the turns: the record that takes \
             them out was not written.",
            r.from
        ));
    }
    out
}

fn percent(before: u64, after: u64) -> String {
    match before {
        0 => String::new(),
        b => format!(" ({:+.0}%)", (after as f64 - b as f64) * 100.0 / b as f64),
    }
}

/// What a dry run found, in lines.
pub fn plan_lines(r: &ThreadSplitPlan) -> Vec<String> {
    let mut out = vec![
        format!(
            "Dry run: nothing is written. Moving @{} to @{} of {} {} {}:",
            r.first_pos,
            r.last_pos,
            r.from,
            if r.into { "into" } else { "to a new thread" },
            r.thread
        ),
        format!(
            "  moves {} turns, {} nodes, {} bytes; {} quoted ({} tokens)",
            r.turns,
            r.nodes,
            r.bytes,
            messages(r.quoted),
            r.quoted_tokens
        ),
        format!(
            "  its place in {}'s compile: moved to thread «{}»: {}",
            r.from, r.thread, r.gist
        ),
        format!(
            "  {} next request: {} -> {} tokens{}",
            r.from,
            r.source.before,
            r.source.after,
            percent(r.source.before, r.source.after)
        ),
    ];
    out.push(match &r.target {
        Some(t) => format!(
            "  {} next request: {} -> {} tokens{}",
            r.thread,
            t.before,
            t.after,
            percent(t.before, t.after)
        ),
        None => format!(
            "  {} opens with the quoted turns, about {} tokens, beside its own system and tools",
            r.thread, r.quoted_tokens
        ),
    });
    out.push(format!(
        "  cache: {}'s prefix changes once, at its next turn; no other turn's does",
        r.from
    ));
    out
}

/// What an unsplit did, in lines.
pub fn unsplit_lines(r: &ThreadUnsplitResult) -> Vec<String> {
    let t = &r.thread.thread;
    vec![
        format!(
            "Undid {}: {} nodes are in {}'s compile again, in their own place (its prefix changes \
             once, at its next turn).",
            r.split, r.restored, r.from
        ),
        if r.retired {
            format!("{} was opened by the split and is done now.", t.slug)
        } else {
            format!(
                "{} no longer holds the quoted turns in its compile.",
                t.slug
            )
        },
        "Nothing was deleted.".to_string(),
    ]
}
