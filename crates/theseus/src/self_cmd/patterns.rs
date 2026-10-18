//! `theseus self patterns` (theseus-pw1q.21.3): the pattern miner's top
//! motifs, run on demand over the window. A read: the daemon writes nothing,
//! not even the miner's cursor, and it works with `[self] mode = "off"`. It
//! is the owner's: inside a job the CLI refuses it (`OPERATORS`).

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::patterns::{SelfPatternsParams, SelfPatternsResult};

use crate::cmd::output;

pub async fn run(conn: &mut Conn, json: bool, top: u32) -> Result<()> {
    let p = SelfPatternsParams {
        top: Some(top),
        from_job: theseus_client::client::job_session(),
        ..Default::default()
    };
    let v = conn
        .request(method::SELF_PATTERNS, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: SelfPatternsResult| {
        for l in lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

/// The answer's lines: the window, then a block a motif.
pub fn lines(r: &SelfPatternsResult) -> Vec<String> {
    let mut out = vec![format!(
        "{} recurring sequences in {} runs and {} calls over {} days.",
        r.found, r.runs, r.calls, r.window_days
    )];
    for (i, m) in r.rows.iter().enumerate() {
        out.push(format!(
            "{}. {}  {} in {} sessions on {} days  score {}{}",
            i + 1,
            m.scope,
            m.occurrences,
            m.sessions,
            m.days,
            m.score,
            if m.bench_born { "  (bench-born)" } else { "" }
        ));
        for s in &m.steps {
            out.push(format!("     {s}"));
        }
        for a in &m.arms {
            out.push(format!("     arm: {a}"));
        }
        out.push(format!(
            "     per occurrence: {} tokens, ${:.2}, {:.0} s, {:.1} decisions; saves ${:.2} and {:.0} s a day",
            m.tokens_per_occ, m.usd_per_occ, m.secs_per_occ, m.decisions_per_occ,
            m.saving_usd_per_day, m.saving_secs_per_day
        ));
    }
    out
}
