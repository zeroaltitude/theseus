//! `theseus policy earned` and `theseus policy revoke` (theseus-uttu): the
//! weekly loosening card from the record. `earned` prints the number
//! (interrupts per active hour over four weeks, and what the grants would
//! have saved), the grants and the card; `--propose-now` posts a card at
//! once (the owner's); `accept ID`, `pass ID` and `show ID` answer a
//! candidate or print the record that earned it. `revoke ID` ends a grant.
//! A grant and a pass are the owner's: inside a job they are refused here,
//! and the daemon judges them again.

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::{client, Conn};
use theseus_protocol::earned::{
    EarnedCard, EarnedGrant, EarnedGroup, PolicyEarnedAnswerParams, PolicyEarnedAnswerResult,
    PolicyEarnedParams, PolicyEarnedResult, PolicyRevokeParams, PolicyRevokeResult,
};
use theseus_protocol::method;

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum EarnedCmd {
    /// Yes, with a notice: ID (from the card) runs without asking from now on. It loosens, so it
    /// counts only where an approval would.
    Accept { id: String },
    /// Not this one: ID is not proposed again for 90 days.
    Pass { id: String },
    /// The record that earned ID: its count, its first and last approval, its sessions, and
    /// three of its calls.
    Show { id: String },
}

/// `theseus policy earned [--propose-now] [accept|pass|show ID]`.
pub async fn earned(
    conn: &mut Conn,
    json: bool,
    cmd: Option<EarnedCmd>,
    propose_now: bool,
) -> Result<()> {
    match cmd {
        Some(EarnedCmd::Accept { id }) => answer(conn, json, id, true).await,
        Some(EarnedCmd::Pass { id }) => answer(conn, json, id, false).await,
        Some(EarnedCmd::Show { id }) => {
            let p = PolicyEarnedParams {
                show: Some(id),
                brief: true,
                ..Default::default()
            };
            let v = conn.request(method::POLICY_EARNED, p).await?;
            output(json, v, |r: PolicyEarnedResult| {
                if let Some(g) = &r.shown {
                    print!("{}", record_text(g));
                }
                Ok(())
            })
        }
        None => {
            if propose_now {
                if let Some(session) = client::job_session() {
                    bail!(
                        "theseus policy earned --propose-now refused: it is the operator's to run, \
                         and this shell is a Theseus job's (THESEUS_SESSION={session} is set). Run \
                         it from your own shell."
                    );
                }
            }
            let p = PolicyEarnedParams {
                propose_now,
                ..Default::default()
            };
            let v = conn.request(method::POLICY_EARNED, p).await?;
            output(json, v, |r: PolicyEarnedResult| {
                print!("{}", number_text(&r));
                Ok(())
            })
        }
    }
}

async fn answer(conn: &mut Conn, json: bool, id: String, accept: bool) -> Result<()> {
    let p = PolicyEarnedAnswerParams {
        id,
        accept,
        ..Default::default()
    };
    let v = conn.request(method::POLICY_EARNED_ANSWER, p).await?;
    output(json, v, |r: PolicyEarnedAnswerResult| {
        match (&r.grant, r.accepted, r.already) {
            (Some(g), true, false) => println!(
                "{} now runs with a notice instead of asking ({}). `theseus policy revoke {}` ends it.",
                g.group.label,
                record_line(&g.group),
                g.group.id
            ),
            (Some(g), true, true) => println!("{} is a grant already.", g.group.id),
            (_, _, true) => println!("{} was passed on already.", r.id),
            _ => println!("{} is not proposed again for 90 days.", r.id),
        }
        Ok(())
    })
}

/// `theseus policy revoke ID`.
pub async fn revoke(conn: &mut Conn, json: bool, id: String) -> Result<()> {
    let p = PolicyRevokeParams {
        id,
        ..Default::default()
    };
    let v = conn.request(method::POLICY_REVOKE, p).await?;
    output(json, v, |r: PolicyRevokeResult| {
        println!(
            "{} asks first again: the grant {} is revoked, and the group is not proposed for 90 days.",
            r.grant.group.label, r.id
        );
        Ok(())
    })
}

/// A group's record in a line: `57 approvals, Oct 1 to Oct 8`.
pub fn record_line(g: &EarnedGroup) -> String {
    format!(
        "earned: {} approvals, {} to {}",
        g.record.approvals,
        day(g.record.first_ms),
        day(g.record.last_ms)
    )
}

/// A grant's line, as `theseus policy` lists it.
pub fn grant_line(g: &EarnedGrant) -> String {
    format!(
        "{}  {}{}  ({}; granted by {}, {})",
        g.group.id,
        g.group.label,
        if g.group.open_ended && g.group.root.is_none() {
            " …"
        } else {
            ""
        },
        record_line(&g.group),
        g.by,
        day(g.at_ms)
    )
}

/// `Oct 1`, on this machine's clock.
fn day(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `localtime_r` writes only the `tm` it is given.
    let t: libc::time_t = secs as libc::time_t;
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    const M: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    if ok {
        format!("{} {}", M[(tm.tm_mon as usize).min(11)], tm.tm_mday)
    } else {
        format!("{ms}")
    }
}

/// One group's record, in full.
pub fn record_text(g: &EarnedGroup) -> String {
    let r = &g.record;
    let mut t = format!(
        "{}: {}\n  {} approvals from {} to {}, {} session{}, never declined, no \"should have asked\", no floor.\n  record digest {}\n",
        g.id,
        g.label,
        r.approvals,
        day(r.first_ms),
        day(r.last_ms),
        r.sessions,
        if r.sessions == 1 { "" } else { "s" },
        r.digest
    );
    if g.open_ended && g.root.is_none() {
        t.push_str("  It covers what follows the prefix too: any arguments after it.\n");
    }
    for s in &r.samples {
        t.push_str(&format!("  e.g. {s}\n"));
    }
    t
}

fn card_text(c: &EarnedCard) -> String {
    let mut t = format!(
        "Card of {} (posted {}):\n",
        c.candidates.len(),
        day(c.proposed_ms)
    );
    for g in &c.candidates {
        let done = if c.answered.contains(&g.id) {
            " [answered]"
        } else {
            ""
        };
        t.push_str(&format!(
            "  {}  {}  ({}){done}\n",
            g.id,
            g.label,
            record_line(g)
        ));
    }
    t
}

/// The number, the grants and the card, as `theseus policy earned` prints.
pub fn number_text(r: &PolicyEarnedResult) -> String {
    let mut t = String::new();
    if !r.enabled {
        t.push_str("Earned autonomy is off ([policy.earned] enabled = false).\n");
    }
    if let Some(p) = &r.proposed {
        t.push_str(&format!("Posted a card to your DM.\n{}", card_text(p)));
    } else if let Some(n) = &r.note {
        t.push_str(&format!("{n}\n"));
    }
    t.push_str(&format!(
        "Interrupts per active hour, the last 4 weeks: {:.2} ({} questions in {} active hours)\n",
        r.per_hour, r.interrupts, r.active_hours
    ));
    for w in &r.weeks {
        t.push_str(&format!(
            "  from {}: {:.2} ({} questions, {} hours)\n",
            day(w.since_ms),
            w.per_hour,
            w.interrupts,
            w.active_hours
        ));
    }
    let cut = if r.per_hour > 0.0 {
        format!(
            " ({:.0} % fewer)",
            100.0 * (1.0 - r.per_hour_with_grants / r.per_hour)
        )
    } else {
        String::new()
    };
    t.push_str(&format!(
        "The grants now in force would have saved {} of them: {:.2} per active hour{cut}.\n",
        r.saved, r.per_hour_with_grants
    ));
    if r.grants.is_empty() {
        t.push_str("No grants.\n");
    } else {
        t.push_str("Grants:\n");
        for g in &r.grants {
            t.push_str(&format!("  {}\n", grant_line(g)));
        }
    }
    if let Some(c) = &r.card {
        t.push_str(&card_text(c));
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    if r.enabled {
        if r.next_card_ms > now_ms {
            t.push_str(&format!(
                "The next card may be posted after {}.\n",
                day(r.next_card_ms)
            ));
        } else {
            t.push_str("The next card may be posted now (the daily tender posts it).\n");
        }
    }
    t
}
