//! `theseus owner proof …` (issue 22): pairing the owner's authenticator app,
//! its status, its reset, and ending every open window. The owner's own
//! terminal alone: refused inside a Theseus job's shell (`client::OPERATORS`),
//! and the daemon refuses every other surface and any agent's process.
//!
//! `enroll` prints the address and the secret ONCE, to this terminal only, so
//! it needs a terminal on both ends (a pipe or a capture is refused before
//! anything is asked), then asks for a code from the app before the daemon
//! keeps the secret. A code is read from the terminal, never an environment
//! variable, and is not printed.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::owner_proof::{
    OwnerProofCodeParams, OwnerProofDone, OwnerProofEnrollParams, OwnerProofEnrollResult,
    OwnerProofStatus, ProofCode,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum OwnerCmd {
    /// Your own second proof: a code from an authenticator app, asked for by the changes that
    /// matter most.
    Proof {
        #[command(subcommand)]
        cmd: ProofCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProofCmd {
    /// Pair your authenticator app. Prints the address and the secret once, here, then asks for a
    /// code from the app before anything is kept. Pairing trusts nothing by itself.
    Enroll,
    /// Whether an app is paired and since when, whether codes are locked, and the places a good
    /// code still covers. Never the secret.
    Status,
    /// Take the pairing away. Needs a code from the app (lost your phone: stop the daemon and
    /// remove the authenticator's file from its state directory by hand, then enroll again).
    Reset {
        /// The six digits; `-` reads them from standard input. Without it you are asked.
        #[arg(long, value_name = "DIGITS")]
        code: Option<String>,
    },
    /// End every open window at once: the next change that needs the proof asks for a code again.
    Lock,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: OwnerCmd) -> Result<()> {
    let OwnerCmd::Proof { cmd } = cmd;
    let from_job = theseus_client::client::job_session();
    match cmd {
        ProofCmd::Enroll => enroll(conn, from_job).await,
        ProofCmd::Status => {
            let v = conn
                .call(
                    method::OWNER_PROOF_STATUS,
                    serde_json::to_value(OwnerProofEnrollParams { from_job })?,
                    |_, _| {},
                )
                .await?;
            output(json, v, |s: OwnerProofStatus| {
                println!("{}", status_words(&s, now_ms()));
                Ok(())
            })
        }
        ProofCmd::Reset { code } => {
            let code = typed(code, "Code from your authenticator app: ")?;
            let v = conn
                .call(
                    method::OWNER_PROOF_RESET,
                    serde_json::to_value(OwnerProofCodeParams {
                        code: Some(ProofCode::new(code)),
                        from_job,
                    })?,
                    |_, _| {},
                )
                .await?;
            output(json, v, |_: OwnerProofDone| {
                println!(
                    "the authenticator is no longer paired. `theseus owner proof enroll` pairs one again"
                );
                Ok(())
            })
        }
        ProofCmd::Lock => {
            let v = conn
                .call(
                    method::OWNER_PROOF_LOCK,
                    serde_json::to_value(OwnerProofCodeParams {
                        code: None,
                        from_job,
                    })?,
                    |_, _| {},
                )
                .await?;
            output(json, v, |d: OwnerProofDone| {
                match d.closed {
                    0 => println!(
                        "no window was open: the next change that needs the proof asks for a code"
                    ),
                    1 => println!(
                        "1 window closed: the next change that needs the proof asks for a code"
                    ),
                    n => println!(
                        "{n} windows closed: the next change that needs the proof asks for a code"
                    ),
                }
                Ok(())
            })
        }
    }
}

async fn enroll(conn: &mut Conn, from_job: Option<String>) -> Result<()> {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        bail!(
            "enroll prints the secret once, to the terminal you are typing in, and asks for a code \
             there: run it in your own terminal, not through a pipe or a capture"
        );
    }
    let v = conn
        .call(
            method::OWNER_PROOF_ENROLL,
            serde_json::to_value(OwnerProofEnrollParams {
                from_job: from_job.clone(),
            })?,
            |_, _| {},
        )
        .await?;
    let offer: OwnerProofEnrollResult = serde_json::from_value(v)?;
    println!("Pair your authenticator app with Theseus. This is shown once.\n");
    println!("  address: {}", offer.uri);
    println!("  secret:  {}\n", offer.secret);
    println!(
        "Scan or paste the address into your app, or add the secret by hand (time-based, six \
         digits). Nothing is kept until you type a code from the app."
    );
    drop(offer);
    for attempt in 1..=3 {
        let code = typed(None, "Code from your authenticator app: ")?;
        let sent = conn
            .call(
                method::OWNER_PROOF_CONFIRM,
                serde_json::to_value(OwnerProofCodeParams {
                    code: Some(ProofCode::new(code)),
                    from_job: from_job.clone(),
                })?,
                |_, _| {},
            )
            .await;
        match sent {
            Ok(_) => {
                println!(
                    "\nPaired. A good code covers the place it is typed in for a while, and \
                     `theseus owner proof status` says when. Pairing trusts nothing by itself."
                );
                return Ok(());
            }
            Err(e) if attempt < 3 => eprintln!("{e}"),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The six digits: `given` (`-` reads one line from standard input), else
/// asked for on the terminal, with `prompt`. Never echoed, never logged.
fn typed(given: Option<String>, prompt: &str) -> Result<String> {
    let line = match given.as_deref() {
        Some("-") => read_line()?,
        Some(c) => c.to_string(),
        None => {
            if !std::io::stdin().is_terminal() {
                bail!("give the six digits your authenticator app shows: `--code <digits>`");
            }
            eprint!("{prompt}");
            std::io::stderr().flush()?;
            read_line()?
        }
    };
    let code = line.trim().to_string();
    if code.is_empty() {
        bail!("no code given");
    }
    Ok(code)
}

fn read_line() -> Result<String> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

fn now_ms() -> u64 {
    theseus_protocol::now_unix_ms()
}

/// What `status` says, in words. Pure, so it is tested with a clock passed in.
pub fn status_words(s: &OwnerProofStatus, now: u64) -> String {
    let mut out = String::new();
    if s.unavailable {
        return "the authenticator's file is there and Theseus cannot read it, so no code can be \
                checked. Nothing is taken from it."
            .into();
    }
    if !s.enrolled {
        return "no authenticator app is paired. `theseus owner proof enroll` pairs one.".into();
    }
    out.push_str("an authenticator app is paired");
    if let Some(at) = s.enrolled_at_ms {
        out.push_str(&format!(" (since {})", ago(now, at)));
    }
    out.push('.');
    if let Some(until) = s.locked_until_ms.filter(|u| *u > now) {
        out.push_str(&format!(
            "\ncodes are locked after too many wrong ones, for {} more.",
            span(until - now)
        ));
    }
    match (s.window_hours, s.windows.as_slice()) {
        (0, _) => {
            out.push_str("\nwindows are off: every change that needs the proof asks for a code.")
        }
        (_, []) => out.push_str(&format!(
            "\nno window is open. A good code covers the place it is typed in for {} hours.",
            s.window_hours
        )),
        (_, windows) => {
            out.push_str("\nopen windows (a change that needs the proof, answered from there, asks for no code):");
            for w in windows {
                out.push_str(&format!(
                    "\n  {}: given {}, good for {} more",
                    w.place,
                    ago(now, w.opened_at_ms),
                    span(w.until_ms.saturating_sub(now))
                ));
            }
            out.push_str("\n`theseus owner proof lock` ends them all.");
        }
    }
    out
}

fn ago(now: u64, then: u64) -> String {
    format!("{} ago", span(now.saturating_sub(then)))
}

fn span(ms: u64) -> String {
    let (hours, minutes) = (ms / 3_600_000, ms / 60_000);
    let many = |n: u64, unit: &str| match n {
        1 => format!("1 {unit}"),
        n => format!("{n} {unit}s"),
    };
    match (hours, minutes) {
        (0, 0) => "under a minute".into(),
        (0, m) => many(m, "minute"),
        (h, _) => many(h, "hour"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::owner_proof::ProofWindow;

    const HOUR: u64 = 3_600_000;

    fn status(windows: Vec<ProofWindow>) -> OwnerProofStatus {
        OwnerProofStatus {
            enrolled: true,
            enrolled_at_ms: Some(0),
            locked_until_ms: None,
            unavailable: false,
            window_hours: 24,
            windows,
        }
    }

    #[test]
    fn status_says_what_is_paired_and_never_a_secret() {
        let s = OwnerProofStatus {
            enrolled: false,
            enrolled_at_ms: None,
            ..status(vec![])
        };
        assert!(status_words(&s, 0).starts_with("no authenticator app is paired"));
        let paired = status_words(&status(vec![]), 5 * HOUR);
        assert!(paired.contains("paired (since 5 hours ago)"), "{paired}");
        assert!(paired.contains("no window is open"), "{paired}");
        assert!(paired.contains("24 hours"), "{paired}");
    }

    #[test]
    fn status_lists_each_open_window_with_where_since_and_until() {
        let w = vec![ProofWindow {
            place: "discord:dm".into(),
            opened_at_ms: 2 * HOUR,
            until_ms: 26 * HOUR,
        }];
        let said = status_words(&status(w), 5 * HOUR);
        assert!(
            said.contains("discord:dm: given 3 hours ago, good for 21 hours more"),
            "{said}"
        );
        assert!(said.contains("theseus owner proof lock"), "{said}");
    }

    #[test]
    fn status_says_when_windows_are_off_or_locked_or_unreadable() {
        let mut s = status(vec![]);
        s.window_hours = 0;
        assert!(status_words(&s, 0).contains("windows are off"));
        let mut s = status(vec![]);
        s.locked_until_ms = Some(10 * 60_000);
        assert!(
            status_words(&s, 0).contains("locked after too many wrong ones, for 10 minutes more")
        );
        let mut s = status(vec![]);
        s.unavailable = true;
        assert!(status_words(&s, 0).contains("cannot read it"));
    }
}
