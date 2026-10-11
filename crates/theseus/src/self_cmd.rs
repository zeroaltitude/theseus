//! `theseus self …` (theseus-pw1q.2, theseus-pw1q.4): self-improvement's
//! kill switch and its ledger. `halt [why]` is anyone's; `resume` is the
//! owner's from their own shell: inside a job it sends the job's
//! `THESEUS_SESSION`, so the daemon refuses it and the refusal is ledgered.
//! `log` lists what Theseus changed about itself, newest first, with what,
//! why, its numbers and its undo; `digest` prints the week's.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::{render, Conn};
use theseus_protocol::method;
use theseus_protocol::rsi::{
    SelfDigestParams, SelfDigestResult, SelfHaltParams, SelfLogParams, SelfLogResult,
    SelfResumeParams, SelfSwitchResult,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum SelfCmd {
    /// What Theseus changed about itself, newest first: the self steps' rows and today's
    /// self-changes (pack moves, learned packs, extensions, routing corrections, syntheses),
    /// each with what, why, its numbers and its undo, and the kill switch's state.
    Log {
        /// Only changes this recent: `7d`, `12h`, `30m`, `2w`.
        #[arg(long, value_name = "AGE")]
        since: Option<String>,
        /// At most this many rows.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Halt all self-directed work at once. Anyone may; only the owner resumes.
    Halt {
        /// Why, in a few words.
        why: Vec<String>,
    },
    /// Release the kill switch: the owner's, from their own shell.
    Resume,
    /// The week's digest: what Theseus changed about itself this week.
    Digest,
}

/// `7d`, `12h`, `30m`, `2w` in milliseconds.
fn age_ms(s: &str) -> Result<u64> {
    let t = s.trim();
    let (n, unit) = t.split_at(t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len()));
    let bad = || anyhow::anyhow!("`{s}` is no age: write 30m, 12h, 7d, or 2w");
    let n: u64 = n.parse().map_err(|_| bad())?;
    let unit = match unit {
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 7 * 86_400_000,
        _ => return Err(bad()),
    };
    Ok(n * unit)
}

pub async fn run(conn: &mut Conn, json: bool, cmd: SelfCmd) -> Result<()> {
    match cmd {
        SelfCmd::Log { since, limit } => {
            let since_ms = since
                .as_deref()
                .map(age_ms)
                .transpose()?
                .map(|a| theseus_protocol::now_unix_ms().saturating_sub(a));
            let p = SelfLogParams {
                since_ms,
                limit: Some(limit),
            };
            let v = conn
                .request(method::SELF_LOG, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: SelfLogResult| {
                for l in log_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        SelfCmd::Halt { why } => {
            let why = why.join(" ");
            let p = SelfHaltParams {
                why: (!why.trim().is_empty()).then_some(why),
                ..Default::default()
            };
            let v = conn
                .request(method::SELF_HALT, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: SelfSwitchResult| {
                println!(
                    "{}",
                    match r.changed {
                        true =>
                            "Self-improvement halted: nothing self-directed runs until the owner \
                                 resumes it (`theseus self resume`)."
                                .to_string(),
                        false => format!("Already halted: {}.", render::self_state_line(&r.state)),
                    }
                );
                Ok(())
            })
        }
        SelfCmd::Resume => {
            let p = SelfResumeParams {
                from_job: theseus_client::client::job_session(),
                ..Default::default()
            };
            let v = conn
                .request(method::SELF_RESUME, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: SelfSwitchResult| {
                println!(
                    "{}",
                    match r.changed {
                        true => format!(
                            "Self-improvement resumed. {}.",
                            render::self_state_line(&r.state)
                        ),
                        false => format!("Not halted: {}.", render::self_state_line(&r.state)),
                    }
                );
                Ok(())
            })
        }
        SelfCmd::Digest => {
            let v = conn
                .request(
                    method::SELF_DIGEST,
                    serde_json::to_value(SelfDigestParams::default())?,
                )
                .await?;
            output(json, v, |r: SelfDigestResult| {
                print!("{}", r.text);
                Ok(())
            })
        }
    }
}

/// `theseus self log`'s lines: the state, then a row a change, newest first.
pub fn log_lines(r: &SelfLogResult) -> Vec<String> {
    let mut out = vec![format!(
        "Self-improvement: {}.",
        render::self_state_line(&r.state)
    )];
    if r.building {
        out.push(
            "The ledger's index is still being built after a start: ask again in a minute.".into(),
        );
        return out;
    }
    if r.rows.is_empty() {
        out.push("Theseus has changed nothing about itself in this window.".into());
    }
    for e in &r.rows {
        let mut l = format!(
            "{}  {:<20} {}",
            render::fmt_date(e.at_unix_ms),
            e.kind,
            e.what
        );
        if let Some(w) = &e.why {
            l.push_str(&format!(" ({w})"));
        }
        out.push(l);
        if !e.numbers.is_null() {
            out.push(format!("{:18}numbers: {}", "", e.numbers));
        }
        if let Some(u) = &e.undo {
            out.push(format!("{:18}undo: {u}", ""));
        }
    }
    if r.more {
        out.push("More are older: --limit or --since reads them.".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_age_reads_minutes_hours_days_and_weeks() {
        assert_eq!(age_ms("30m").unwrap(), 1_800_000);
        assert_eq!(age_ms("7d").unwrap(), 7 * 86_400_000);
        assert_eq!(age_ms("2w").unwrap(), 14 * 86_400_000);
        assert!(age_ms("7").is_err() && age_ms("d").is_err() && age_ms("7y").is_err());
    }
}
