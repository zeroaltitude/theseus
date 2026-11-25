//! `theseus digest` (theseus-qkoi3): what finished, failed, was answered or
//! woke since you last looked, in a few lines. One `work.digest` read.
//!
//! "Since you last looked" is the store's look (issue 46): `since_look` asks
//! from it, and `work.look` moves it when this command has shown the board.
//! Before the store has one, the time of the look an older build kept in the
//! machine's seen file is carried over (`seen::old_look_time`), never its
//! position, and the file keeps it until the store has taken a look. `--since 2h` names a time instead. `theseus status` prints the
//! digest's summary as its last line when there is something to say
//! (`status_line`).
//!
//! The words are the protocol's (`digest_words`, `digest_summary`): this file
//! only lays them out, so the terminal UI, the cockpit and Discord say the same
//! thing.

use anyhow::Result;
use clap::Args;
use theseus_client::{seen, Conn};
use theseus_protocol::work::Question;
use theseus_protocol::{
    digest_more, digest_summary, method, now_unix_ms, WorkDigestParams, WorkDigestResult,
    WorkLookParams, WorkLookResult,
};

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
    let old = path.as_deref().and_then(seen::old_look_time);
    let mut params = match a.since.as_deref() {
        Some(t) => WorkDigestParams {
            since_ms: Some(postmortem::when(t, now)?),
            ..Default::default()
        },
        None => since_look(old.unwrap_or(now.saturating_sub(FIRST))),
    };
    params.roots = a.session.map(|s| vec![s]);
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
    let since_ms = match (a.since.is_some(), res.look) {
        (false, Some(l)) => l.at_ms,
        _ => params.since_ms.unwrap_or_default(),
    };
    for l in lines(&res, since_ms, &local_hm) {
        println!("{l}");
    }
    // The whole board was shown; a session's own timeline is not the board,
    // and a command inside a job is the agent's reading, not the operator's.
    if params.roots.is_none() && client::job_session().is_none() {
        let look = WorkLookParams { position: res.to };
        match conn.request(method::WORK_LOOK, look).await {
            Err(e) => eprintln!("could not record that you looked: {e}"),
            Ok(v) => {
                // The terminal's next start knows this store by it (issue 46),
                // and the store has taken a look: the write hands an older
                // build's time over, and no write before it does.
                let r = serde_json::from_value::<WorkLookResult>(v);
                if let (Some(p), Ok(r)) = (path, r) {
                    let _ = seen::record_store_look(&p, r.look);
                }
            }
        }
    }
    Ok(())
}

/// A read since the store's look (issue 46); before the store has one, from
/// `fallback`, a time.
pub fn since_look(fallback: u64) -> WorkDigestParams {
    WorkDigestParams {
        since_look: Some(true),
        since_ms: Some(fallback),
        ..Default::default()
    }
}

/// When "since you last looked" began, for its words: the store's look, else the
/// look an older build kept on this machine; none before either.
pub fn looked_at(res: &WorkDigestResult, old: Option<u64>) -> Option<u64> {
    res.look.map(|l| l.at_ms).or(old)
}

/// The status line: `◆ 2 reported, 1 answered by you since 18:14 · theseus digest`,
/// printed when there is something since the store's look. Best effort: a
/// daemon that does not answer, or has no `work.digest`, prints nothing.
/// `quiet` (`--short`, `--json`) prints nothing at all.
pub async fn status_line(conn: &mut Conn, quiet: bool) {
    if quiet {
        return;
    }
    let old = seen::Seen::default_path()
        .as_deref()
        .and_then(seen::old_look_time);
    // Never looked, the store reads from now: nothing, and nothing is said.
    let params = since_look(old.unwrap_or_else(now_unix_ms));
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
    // Never looked: there is no "since", and status is not the place to start one.
    let Some(since) = looked_at(&res, old) else {
        return;
    };
    if let Some(l) = status_text(&res, since, &local_hm) {
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
    out.extend(digest_more(res, hm).map(|m| format!("  {m}")));
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
