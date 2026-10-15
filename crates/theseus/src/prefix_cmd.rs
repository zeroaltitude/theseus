//! `theseus prefix report` (theseus-rr15): the prefix library's picks (reuse,
//! coarser, mint), the cache writes the reuses saved, the reuses below `act`
//! and the false fits among them, and the library's size against its cap.
//! A read (`prefix.report`), the owner's from a private surface.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::prefix::{PrefixReport, PrefixReportParams};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum PrefixCmd {
    /// The picks, what they saved, and the false fits with their scores.
    Report {
        /// The newest picks read, at most (2,000).
        #[arg(long)]
        limit: Option<u32>,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: PrefixCmd) -> Result<()> {
    let PrefixCmd::Report { limit } = cmd;
    let v = conn
        .request(
            method::PREFIX_REPORT,
            serde_json::to_value(PrefixReportParams { limit })?,
        )
        .await?;
    output(json, v, |r: PrefixReport| {
        for l in lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

/// The report's words.
pub fn lines(r: &PrefixReport) -> Vec<String> {
    let mut out = vec![format!(
        "prefix library: {}, {} headers, {:.1} of {:.1} MiB, {} warm",
        if r.enabled { "on" } else { "off" },
        r.headers,
        r.bytes as f64 / 1_048_576.0,
        r.max_bytes as f64 / 1_048_576.0,
        r.warm
    )];
    if r.picks == 0 {
        out.push("no picks yet: a pick is made where a request is rewritten anyway".into());
        return out;
    }
    out.push(format!(
        "{} picks: {} reuse, {} coarser, {} mint (reuse {}, mint {})",
        r.picks,
        r.reuses,
        r.coarser,
        r.mints,
        pct(r.reuse_rate),
        pct(r.mint_rate)
    ));
    out.push(format!(
        "cache writes saved: {} tokens (${:.4}) over {} reuses that read the cache",
        r.writes_saved_tokens, r.writes_saved_usd, r.cache_hits
    ));
    out.push(format!(
        "below act: {} reuses, {} false fits",
        r.below_act,
        r.false_fits.len()
    ));
    for f in &r.false_fits {
        let tail = |s: &str| s[s.len().saturating_sub(8)..].to_string();
        out.push(format!(
            "  {} fit {:.2} {} · {} turn …{} · header {}",
            f.choice,
            f.fit,
            f.why,
            tail(&f.session_id),
            tail(&f.turn_id),
            &f.header[..f.header.len().min(12)]
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::prefix::FalseFit;

    #[test]
    fn the_report_says_the_picks_the_savings_and_each_false_fit() {
        let r = PrefixReport {
            enabled: true,
            picks: 5,
            reuses: 3,
            coarser: 1,
            mints: 1,
            reuse_rate: 0.8,
            mint_rate: 0.2,
            cache_hits: 2,
            writes_saved_tokens: 24_000,
            writes_saved_usd: 0.0828,
            below_act: 2,
            false_fits: vec![FalseFit {
                session_id: "ses_00000000000000000000000000a1b2c3".into(),
                turn_id: "turn_000000000000000000000000d4e5f6".into(),
                at_unix_ms: 1_790_000_000_000,
                choice: "reuse".into(),
                fit: 0.7,
                header: "0123456789abcdef0123".into(),
                why: "correction".into(),
            }],
            headers: 4,
            bytes: 3 * 1_048_576 / 2,
            max_bytes: 10 * 1_048_576,
            warm: 2,
        };
        assert_eq!(
            lines(&r),
            [
                "prefix library: on, 4 headers, 1.5 of 10.0 MiB, 2 warm",
                "5 picks: 3 reuse, 1 coarser, 1 mint (reuse 80%, mint 20%)",
                "cache writes saved: 24000 tokens ($0.0828) over 2 reuses that read the cache",
                "below act: 2 reuses, 1 false fits",
                "  reuse fit 0.70 correction · 00a1b2c3 turn …00d4e5f6 · header 0123456789ab",
            ]
        );
        let none = PrefixReport::default();
        assert_eq!(lines(&none).len(), 2);
    }
}
