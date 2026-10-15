//! `theseus digest` (theseus-qkoi3): what finished, failed, was answered or
//! woke since you last looked, in a few lines. One `work.digest` read.
//!
//! "Since you last looked" is the machine's seen file (`seen::looked`), which
//! the terminal clients write when they show a board; `--since 2h` names a time
//! instead. `theseus status` prints the digest's summary as its last line when
//! there is something to say (`status_line`).
//!
//! The words are the protocol's (`digest_words`, `digest_summary`): this file
//! only lays them out, so the terminal UI, the cockpit and Discord say the same
//! thing.

use anyhow::Result;
use clap::Args;
use theseus_client::{seen, Conn};
use theseus_protocol::work::Question;
use theseus_protocol::{digest_summary, method, now_unix_ms, WorkDigestParams, WorkDigestResult};

use crate::{postmortem, status::local_hm};
use theseus_client::client;

/// With nothing seen yet, the digest reaches back this far.
const FIRST: u64 = 24 * 3_600_000;

#[derive(Args, Debug)]
pub struct DigestArgs {
    /// From this age or time on: `2h`, `30m`, `22:00`, `yesterday 22:00`, or an RFC 3339 time.
    #[arg(long, conflicts_with = "since_seen", value_name = "WHEN")]
    since: Option<String>,
    /// Since the last time a terminal client (this command, the TUI) showed you the board: the
    /// default. With nothing seen yet, the last 24 hours.
    #[arg(long)]
    since_seen: bool,
    /// Only this conversation or task (a session id): its own timeline.
    #[arg(long, value_name = "SESSION")]
    session: Option<String>,
}

/// Run `theseus digest`.
pub async fn run(conn: &mut Conn, json: bool, a: DigestArgs) -> Result<()> {
    let now = now_unix_ms();
    let path = seen::Seen::default_path();
    let looked = path.as_deref().map(seen::looked).unwrap_or_default();
    let mut params = WorkDigestParams {
        roots: a.session.map(|s| vec![s]),
        ..Default::default()
    };
    let since_ms = match a.since.as_deref() {
        Some(t) => {
            let ms = postmortem::when(t, now)?;
            params.since_ms = Some(ms);
            ms
        }
        None if looked.position > 0 => {
            params.since = Some(looked.position);
            looked.at_ms
        }
        None => {
            params.since_ms = Some(now.saturating_sub(FIRST));
            now.saturating_sub(FIRST)
        }
    };
    let v = conn
        .call(
            method::WORK_DIGEST,
            serde_json::to_value(&params)?,
            |_, _| {},
        )
        .await?;
    if json {
        println!("{}", serde_json::json!({"result": v}));
        return Ok(());
    }
    let res: WorkDigestResult = serde_json::from_value(v)?;
    for l in lines(&res, since_ms, &local_hm) {
        println!("{l}");
    }
    // The whole board was shown; a session's own timeline is not the board,
    // and a command inside a job is the agent's reading, not the operator's.
    if params.roots.is_none() && client::job_session().is_none() {
        if let Some(p) = path {
            if let Err(e) = seen::record_looked(&p, res.to, now) {
                eprintln!("could not record that you looked: {e}");
            }
        }
    }
    Ok(())
}

/// The status line: `◆ 2 reported, 1 answered by you since 18:14 · theseus digest`,
/// printed when there is something since the machine's last look. Best
/// effort: a daemon that does not answer, or has no `work.digest`, prints
/// nothing. `quiet` (`--short`, `--json`) prints nothing at all.
pub async fn status_line(conn: &mut Conn, quiet: bool) {
    if quiet {
        return;
    }
    let looked = seen::Seen::default_path()
        .as_deref()
        .map(seen::looked)
        .unwrap_or_default();
    // Never looked: there is no "since", and status is not the place to start one.
    if looked.position == 0 {
        return;
    }
    let params = WorkDigestParams {
        since: Some(looked.position),
        ..Default::default()
    };
    let Ok(v) = conn
        .call(
            method::WORK_DIGEST,
            serde_json::to_value(&params).unwrap_or_default(),
            |_, _| {},
        )
        .await
    else {
        return;
    };
    let Ok(res) = serde_json::from_value::<WorkDigestResult>(v) else {
        return;
    };
    if let Some(l) = status_text(&res, looked.at_ms, &local_hm) {
        println!("{l}");
    }
}

/// The status line's text; none when nothing happened.
pub fn status_text(
    res: &WorkDigestResult,
    since_ms: u64,
    hm: &dyn Fn(u64) -> String,
) -> Option<String> {
    digest_summary(&res.lines, since_ms, hm).map(|s| format!("{s} · theseus digest"))
}

/// `theseus digest`'s lines: the summary, each line with its time, what the
/// board holds now, and the questions waiting.
pub fn lines(res: &WorkDigestResult, since_ms: u64, hm: &dyn Fn(u64) -> String) -> Vec<String> {
    let mut out = Vec::new();
    match digest_summary(&res.lines, since_ms, hm) {
        Some(s) => out.push(s),
        None if since_ms > 0 => out.push(format!("◆ nothing since {}", hm(since_ms))),
        None => out.push("◆ nothing".to_string()),
    }
    for l in &res.lines {
        out.push(format!("  {}  {}", hm(l.at_ms), l.words));
    }
    let n = &res.now;
    let mut now = Vec::new();
    for (count, word) in [
        (n.needs_you, "need you"),
        (n.working, "work"),
        (n.failed, "failed"),
    ] {
        if count > 0 {
            now.push(format!("{count} {word}"));
        }
    }
    if !now.is_empty() {
        out.push(format!(
            "now: {} of {} on the board",
            now.join(", "),
            n.total
        ));
    }
    for q in &res.questions {
        out.push(question_line(q));
    }
    if res.from_ledger {
        out.push("(read from the ledger: the live record does not reach back that far)".into());
    }
    out
}

/// `● fs.write create notes.txt? · theseus answer a1b2c3`.
fn question_line(q: &Question) -> String {
    let n = q.id.chars().count();
    let tail: String = q.id.chars().skip(n.saturating_sub(6)).collect();
    format!(
        "● {}? · theseus answer {tail}",
        q.prompt.trim_end_matches('?')
    )
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
