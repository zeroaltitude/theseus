//! `theseus ledger verify` and `theseus ledger head` (theseus-bmkxk): the
//! ledger's hash chain, asked of the daemon (`ledger.verify`, a read: the
//! daemon recomputes every link, a page of rows at a time). `verify` exits 0
//! when the chain is whole and 1 at the first broken link, which it names:
//! its position, time and kind, and whether it was edited, removed or
//! reordered, as far as the walk can tell. The words are [`lines`] and
//! [`head_lines`], functions of the answer alone.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::{method, LedgerVerifyParams, LedgerVerifyResult};

#[derive(Subcommand, Debug)]
pub enum LedgerCmd {
    /// Walk the ledger's hash chain, recomputing every link: exit 0 when it is whole, and 1 at
    /// the first broken link, named. An edit, a removed row and a reordered pair are found; so is
    /// a truncated tail or a rewritten chain, given a head recorded earlier (`--anchor`).
    Verify {
        /// Start at the first row at or after this WAL position, trusting that row's own link.
        #[arg(long, value_name = "POS")]
        since: Option<u64>,
        /// A head recorded earlier (`theseus ledger head`), as `SEQ:DIGEST` (8 hex characters
        /// of the digest or more): fail if the chain ends before it or differs from it.
        #[arg(long, value_name = "SEQ:DIGEST")]
        anchor: Option<String>,
    },
    /// The chain's current head: its digest, seq and WAL position, as the daemon's writer last
    /// committed it. Record it somewhere the ledger's editor can't reach.
    Head,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: LedgerCmd) -> Result<()> {
    let (params, head_only) = match cmd {
        LedgerCmd::Verify { since, anchor } => (
            LedgerVerifyParams {
                since,
                anchor,
                head_only: false,
            },
            false,
        ),
        LedgerCmd::Head => (
            LedgerVerifyParams {
                head_only: true,
                ..Default::default()
            },
            true,
        ),
    };
    let v = conn
        .request(method::LEDGER_VERIFY, serde_json::to_value(&params)?)
        .await?;
    let r: LedgerVerifyResult = serde_json::from_value(v.clone())?;
    if json {
        println!("{}", serde_json::to_string(&v)?);
    } else {
        let text = if head_only { head_lines(&r) } else { lines(&r) };
        crate::print::lines(&mut std::io::stdout().lock(), &text)?;
    }
    if !r.ok {
        std::process::exit(1);
    }
    Ok(())
}

/// `1204331` as `1,204,331`.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn short(d: &str) -> &str {
    &d[..d.len().min(8)]
}

/// `theseus ledger head`.
pub fn head_lines(r: &LedgerVerifyResult) -> Vec<theseus_client::render::Line> {
    use theseus_client::render::{Line, Tag};
    match &r.head {
        Some(h) => vec![Line::new(
            Tag::Plain,
            format!(
                "head {} seq {} position {} (the writer's own word: `theseus ledger verify` proves it)",
                h.digest,
                thousands(h.seq),
                thousands(h.position)
            ),
        )],
        None => vec![Line::new(
            Tag::Dim,
            "no chain yet: it starts with the first ledger row this build writes",
        )],
    }
}

/// `theseus ledger verify`'s words.
pub fn lines(r: &LedgerVerifyResult) -> Vec<theseus_client::render::Line> {
    use theseus_client::render::{Line, Tag};
    let mut out = Vec::new();
    if let Some(b) = &r.broken {
        let what = b.kind.as_deref().unwrap_or("a row");
        let seq = b
            .seq
            .map_or(String::new(), |s| format!(" (seq {})", thousands(s)));
        let at = if b.at_unix_ms > 0 {
            format!(", {}", fmt_stamp(b.at_unix_ms))
        } else {
            String::new()
        };
        out.push(Line::new(
            Tag::Bad,
            format!(
                "ledger BROKEN at position {}: {what}{seq}{at}: {}",
                thousands(b.position),
                b.fault.replace('_', " ")
            ),
        ));
        out.push(Line::new(Tag::Plain, format!("  {}", b.detail)));
        out.push(Line::new(
            Tag::Dim,
            format!("  {} rows before it verified", thousands(r.rows)),
        ));
        return out;
    }
    let Some(h) = &r.head else {
        out.push(Line::new(
            Tag::Dim,
            format!(
                "ledger: no chain yet; {} rows from before it are unchained (it starts with the first row this build writes)",
                thousands(r.unchained)
            ),
        ));
        return out;
    };
    let since = r
        .since_ms
        .map_or(String::new(), |t| format!(" since {}", fmt_stamp(t)));
    out.push(Line::new(
        Tag::Ok,
        format!(
            "ledger ok: {} rows chained{since}, head {}… (seq {}, position {}), in {} ms",
            thousands(r.rows),
            short(&h.digest),
            thousands(h.seq),
            thousands(h.position),
            r.took_ms
        ),
    ));
    if r.unchained > 0 {
        let pinned = if r.history_checked {
            "their digest is pinned and matches"
        } else {
            "counted in the chain's first row"
        };
        out.push(Line::new(
            Tag::Dim,
            format!(
                "  {} earlier rows from before the chain: {pinned}",
                thousands(r.unchained)
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::{LedgerBreak, LedgerHead};

    fn text(ls: Vec<theseus_client::render::Line>) -> String {
        ls.iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn head() -> LedgerHead {
        LedgerHead {
            seq: 1_204_330,
            digest: "3fa1c09e".to_string() + &"0".repeat(56),
            position: 9_812_004,
        }
    }

    #[test]
    fn thousands_groups_by_three() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_204_331), "1,204,331");
    }

    #[test]
    fn a_whole_chain_says_its_rows_and_head() {
        theseus_client::render::time::pin_for_tests();
        let r = LedgerVerifyResult {
            ok: true,
            rows: 1_204_331,
            since_ms: Some(1_760_000_000_000),
            head: Some(head()),
            took_ms: 2100,
            ..Default::default()
        };
        let t = text(lines(&r));
        assert!(
            t.starts_with("ledger ok: 1,204,331 rows chained since 2025-"),
            "{t}"
        );
        assert!(t.contains("head 3fa1c09e…"), "{t}");
    }

    #[test]
    fn a_broken_link_is_named_with_its_position_time_and_kind() {
        theseus_client::render::time::pin_for_tests();
        let r = LedgerVerifyResult {
            rows: 7,
            broken: Some(LedgerBreak {
                position: 8,
                at_unix_ms: 1_760_000_000_000,
                kind: Some("turn.started".into()),
                seq: Some(7),
                fault: "edited".into(),
                missing: None,
                detail: "the row does not match its own digest".into(),
            }),
            ..Default::default()
        };
        let t = text(lines(&r));
        assert!(
            t.contains("ledger BROKEN at position 8: turn.started (seq 7), 2025-"),
            "{t}"
        );
        assert!(t.contains(": edited"), "{t}");
        assert!(t.contains("7 rows before it verified"), "{t}");
    }

    #[test]
    fn a_store_before_its_chain_says_so() {
        let r = LedgerVerifyResult {
            ok: true,
            unchained: 12,
            ..Default::default()
        };
        assert!(text(lines(&r)).contains("no chain yet; 12 rows"));
        assert!(text(head_lines(&r)).contains("no chain yet"));
    }
}
