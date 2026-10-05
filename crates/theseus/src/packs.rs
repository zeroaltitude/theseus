//! `theseus packs` (M5 26a): the ladder, from `pack.list`. Each pack this
//! build wires with its mode, share and why, its rollback rules, and its
//! last `pack.mode` rows; `promote` and `rollback` move one, the owner's
//! acts (refused inside a job, `client::OPERATORS`). A security pack's
//! promotion is a card: `theseus confirm <id> --approve` answers it.

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::packs::{
    PackInfo, PackListResult, PackModeRow, PackPromoteParams, PackPromoteResult, PackRollbackParams,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum PacksCmd {
    /// Every pack: its mode, share and why, its rules, and its last rows (default).
    List,
    /// Move a pack up: to a canary share of sessions, or live. Short of the bar (a learning
    /// report whose frozen holdout has 200 labeled per deciding question, in which it beats its
    /// baseline), the row says `forced`, with the numbers. A `security.*` pack's is a card.
    Promote {
        /// The pack version (`loop.v1`).
        pack: String,
        /// A canary of this share of sessions, above 0 and at most 1.
        #[arg(long, value_name = "SHARE", conflicts_with = "live")]
        canary: Option<f64>,
        /// Live: every session.
        #[arg(long)]
        live: bool,
        /// The learning report it cites (`rpt_<date>_<pack>`); none, the latest.
        #[arg(long, value_name = "ID")]
        report: Option<String>,
    },
    /// Roll a pack back: it records in shadow and acts on nothing until a promotion.
    Rollback {
        pack: String,
        /// Why, in a few words.
        #[arg(long)]
        why: Option<String>,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: PacksCmd) -> Result<()> {
    match cmd {
        PacksCmd::List => {
            let v = conn.request(method::PACK_LIST, Value::Null).await?;
            output(json, v, |r: PackListResult| {
                print!("{}", table(&r));
                Ok(())
            })
        }
        PacksCmd::Promote {
            pack,
            canary,
            live,
            report,
        } => {
            let to = match (canary, live) {
                (Some(_), _) => "canary",
                (None, true) => "live",
                (None, false) => bail!("say where it goes: --canary <share> or --live"),
            };
            let p = PackPromoteParams {
                pack,
                to: to.into(),
                share: canary,
                report,
            };
            let v = conn
                .request(method::PACK_PROMOTE, serde_json::to_value(p)?)
                .await?;
            output(json, v, |r: PackPromoteResult| {
                println!("{}", r.said);
                Ok(())
            })
        }
        PacksCmd::Rollback { pack, why } => {
            let p = PackRollbackParams {
                pack,
                why,
                off: false,
            };
            let v = conn
                .request(method::PACK_ROLLBACK, serde_json::to_value(p)?)
                .await?;
            output(json, v, |r: PackModeRow| {
                println!("{}", row_line(&r));
                Ok(())
            })
        }
    }
}

/// The listing: a header naming the ceiling, then each pack.
pub fn table(r: &PackListResult) -> String {
    let mut out = format!(
        "the ladder: judge {}, max_mode {}\n",
        if r.enabled { "on" } else { "off" },
        r.max_mode
    );
    for p in &r.packs {
        out.push_str(&pack_lines(p));
    }
    out
}

fn pack_lines(p: &PackInfo) -> String {
    let mut out = format!("{}: {}", p.pack, p.why);
    if p.acts != p.mode && !(p.mode == "rolled_back" && p.acts == "shadow") && p.mode != "canary" {
        out.push_str(&format!(" · acts as {} under the config", p.acts));
    }
    out.push('\n');
    if !p.rules.is_empty() {
        out.push_str(&format!("  rules: {}\n", p.rules.join(", ")));
    }
    for r in &p.rows {
        out.push_str(&format!("  {}\n", row_line(r)));
    }
    out
}

/// One `pack.mode` row: its mode, who, why, and what it cites.
pub fn row_line(r: &PackModeRow) -> String {
    let mode = match (r.mode.as_str(), r.share) {
        ("canary", Some(s)) => format!("canary {s:?}"),
        (m, _) => m.to_string(),
    };
    let mut out = format!("{} → {mode} by {} ({}): {}", r.from, r.who, r.via, r.why);
    if r.declined {
        out.push_str(" · declined, no mode written");
    }
    if r.forced {
        out.push_str(&format!(
            " · forced{}",
            r.numbers
                .as_deref()
                .map(|n| format!(": {n}"))
                .unwrap_or_default()
        ));
    }
    if let Some(rep) = &r.report {
        out.push_str(&format!(" · cites {rep}"));
    }
    if let Some(w) = &r.words {
        if r.rule.is_some() {
            out.push_str(&format!(
                " · {}: {w}",
                r.rule.as_deref().unwrap_or_default()
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_names_each_packs_mode_why_and_rows() {
        let r = PackListResult {
            enabled: true,
            max_mode: "live".into(),
            packs: vec![PackInfo {
                pack: "loop.v1".into(),
                mode: "canary".into(),
                share: Some(1.0),
                acts: "canary".into(),
                wired: "shadow".into(),
                why: "canary 1.0 (owner: forced by the owner)".into(),
                rules: vec!["nudge_loop".into()],
                rows: vec![PackModeRow {
                    pack: "loop.v1".into(),
                    mode: "canary".into(),
                    from: "shadow".into(),
                    share: Some(1.0),
                    who: "owner".into(),
                    via: "cli".into(),
                    why: "forced by the owner".into(),
                    forced: true,
                    numbers: Some("work_state: labeled 0 of 200".into()),
                    ..PackModeRow::default()
                }],
                ..PackInfo::default()
            }],
        };
        let t = table(&r);
        assert!(
            t.contains("loop.v1: canary 1.0 (owner: forced by the owner)"),
            "{t}"
        );
        assert!(t.contains("rules: nudge_loop"), "{t}");
        assert!(
            t.contains("shadow → canary 1.0 by owner (cli): forced by the owner · forced: work_state: labeled 0 of 200"),
            "{t}"
        );
    }
}
