//! `theseus standing …` (theseus-a7vi): the owner's standing orders,
//! preferences and facts, kept in every prompt of their scope. `standing`
//! lists them with the manifests' tally; `add` and `revoke` are the owner's
//! acts (refused inside a job, `client::OPERATORS`).

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::{render, Conn};
use theseus_protocol::method;
use theseus_protocol::standing::{
    StandingAddParams, StandingChanged, StandingKind, StandingListParams, StandingListResult,
    StandingRevokeParams, StandingScope, SURFACES,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum StandingCmd {
    /// The live records, each with its scope and where it came from, and whether every compile of
    /// their scope since they began carried them (default).
    List {
        /// The ended and expired ones too.
        #[arg(long)]
        all: bool,
    },
    /// Add a record: what every prompt of its scope carries until you revoke it. An order is
    /// followed, a preference honoured, a fact known. Preferences and facts stay private: a
    /// shared place gets the orders' text and only a line about the rest. Only you, from a
    /// private place (the CLI is one), may.
    Add {
        /// `order` (default), `preference`, or `fact`.
        #[arg(long, default_value = "order")]
        kind: String,
        /// `everywhere` (default), `place:TARGET` (a bound place's target, `discord:dm:<user id>` or
        /// `discord:channel:<id>`, as `theseus places` lists it), `surface:NAME`, or `session:ID`.
        #[arg(long, default_value = "everywhere", conflicts_with = "surface")]
        scope: String,
        /// Hold only on turns that came through this surface: `terminal` (the CLI and the TUI),
        /// `discord`, `voice`, `cockpit`, or `mcp`. It rides the turn's tail, so the system prompt
        /// stays the same on every surface. Shorthand for `--scope surface:NAME`.
        #[arg(long, value_name = "NAME")]
        surface: Option<String>,
        /// It lapses by itself after this long: `30m`, `12h`, `7d`, `2w`.
        #[arg(long, value_name = "AGE")]
        until: Option<String>,
        /// The record it replaces, which it ends.
        #[arg(long, value_name = "ID")]
        replaces: Option<String>,
        text: Vec<String>,
    },
    /// Revoke a record by its id (or the start of it): it leaves every prompt at its next turn.
    Revoke { id: String },
}

fn scope_of(s: &str) -> Result<StandingScope> {
    Ok(match s.split_once(':') {
        None if s == "everywhere" => StandingScope::Everywhere,
        Some(("place", p)) if !p.is_empty() => StandingScope::Place(p.to_string()),
        Some(("session", id)) if !id.is_empty() => StandingScope::Session(id.to_string()),
        Some(("surface", n)) if SURFACES.contains(&n) => StandingScope::Surface(n.to_string()),
        Some(("surface", n)) => bail!("`{n}` is no surface: write {}", SURFACES.join(", ")),
        _ => {
            bail!("`{s}` is no scope: write everywhere, place:TARGET, surface:NAME, or session:ID")
        }
    })
}

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

impl Default for StandingCmd {
    fn default() -> Self {
        Self::List { all: false }
    }
}

pub async fn run(conn: &mut Conn, json: bool, cmd: StandingCmd) -> Result<()> {
    match cmd {
        StandingCmd::List { all } => {
            let p = StandingListParams {
                all,
                ..Default::default()
            };
            let v = conn
                .request(method::STANDING_LIST, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: StandingListResult| {
                for l in list_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        StandingCmd::Add {
            kind,
            scope,
            surface,
            until,
            replaces,
            text,
        } => {
            let Some(kind) = StandingKind::parse(&kind) else {
                bail!("`{kind}` is no kind: write order, preference, or fact");
            };
            let p = StandingAddParams {
                kind,
                text: text.join(" "),
                scope: scope_of(&surface.map_or(scope, |n| format!("surface:{n}")))?,
                until_ms: until
                    .as_deref()
                    .map(age_ms)
                    .transpose()?
                    .map(|a| theseus_protocol::now_unix_ms() + a),
                replaces,
                ..Default::default()
            };
            let v = conn
                .request(method::STANDING_ADD, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: StandingChanged| {
                println!("{}", added_line(&r));
                Ok(())
            })
        }
        StandingCmd::Revoke { id } => {
            let p = StandingRevokeParams {
                id,
                ..Default::default()
            };
            let v = conn
                .request(method::STANDING_REVOKE, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: StandingChanged| {
                println!(
                    "Revoked {}: it leaves every prompt of its scope at the next turn.",
                    r.ended.as_deref().unwrap_or("the record")
                );
                Ok(())
            })
        }
    }
}

/// What an add answers.
pub fn added_line(r: &StandingChanged) -> String {
    let rec = &r.record;
    let ended = r
        .ended
        .as_deref()
        .map(|e| format!(", replacing {e}"))
        .unwrap_or_default();
    format!(
        "Added {} {} for {}{ended}: every prompt of that scope carries it from its next turn.",
        rec.kind.name(),
        rec.id,
        rec.scope.words()
    )
}

/// `theseus standing`'s lines: a row a record (id, kind, scope, since, source, text), then the
/// manifests' tally.
pub fn list_lines(r: &StandingListResult) -> Vec<String> {
    if r.records.is_empty() {
        return vec![
            "No standing records. `theseus standing add \"never push to main\"` makes an order."
                .into(),
        ];
    }
    let mut out = Vec::new();
    let width = r
        .records
        .iter()
        .map(|i| i.record.scope.words().len())
        .max()
        .unwrap_or(0);
    for i in &r.records {
        let rec = &i.record;
        let ended = match i.state.as_str() {
            "live" => String::new(),
            s => format!(" [{s}]"),
        };
        out.push(format!(
            "{}  {:<10} {:<width$} {}  {}  {}{ended}",
            rec.id,
            rec.kind.name(),
            rec.scope.words(),
            render::fmt_date(rec.since_ms),
            rec.source,
            rec.text
        ));
    }
    out.push(format!(
        "in every compile of their scope since they began: {} compiles, {} missing",
        r.compiles, r.missing
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::standing::{StandingInfo, StandingRecord};

    fn info(
        id: &str,
        kind: StandingKind,
        scope: StandingScope,
        text: &str,
        state: &str,
    ) -> StandingInfo {
        StandingInfo {
            record: StandingRecord {
                id: id.into(),
                kind,
                text: text.into(),
                scope,
                since_ms: 1_790_000_000_000,
                until_ms: None,
                source: "the CLI".into(),
                ends: None,
            },
            state: state.into(),
            compiles: 12,
            missing: 0,
        }
    }

    /// The list: a row a record, then the tally the manifests make.
    #[test]
    fn the_list_has_a_row_a_record_and_the_manifests_tally() {
        let r = StandingListResult {
            records: vec![
                info(
                    "std_3f9a1c2d",
                    StandingKind::Order,
                    StandingScope::Everywhere,
                    "never push to main",
                    "live",
                ),
                info(
                    "std_77aa01b2",
                    StandingKind::Fact,
                    StandingScope::Place("discord:dm:u1".into()),
                    "keeps a vegetable garden",
                    "revoked",
                ),
            ],
            compiles: 12,
            missing: 0,
        };
        let d = render::fmt_date(1_790_000_000_000);
        assert_eq!(
            list_lines(&r),
            vec![
                format!("std_3f9a1c2d  order      everywhere          {d}  the CLI  never push to main"),
                format!("std_77aa01b2  fact       place:discord:dm:u1 {d}  the CLI  keeps a vegetable garden [revoked]"),
                "in every compile of their scope since they began: 12 compiles, 0 missing".to_string(),
            ]
        );
        assert_eq!(
            list_lines(&StandingListResult { records: vec![], compiles: 0, missing: 0 }),
            vec!["No standing records. `theseus standing add \"never push to main\"` makes an order."]
        );
    }

    #[test]
    fn a_scope_and_an_age_are_read_or_refused() {
        assert_eq!(scope_of("everywhere").unwrap(), StandingScope::Everywhere);
        assert_eq!(
            scope_of("place:discord:dm:u1").unwrap(),
            StandingScope::Place("discord:dm:u1".into())
        );
        assert_eq!(
            scope_of("session:ses_a").unwrap(),
            StandingScope::Session("ses_a".into())
        );
        assert_eq!(
            scope_of("surface:terminal").unwrap(),
            StandingScope::Surface("terminal".into())
        );
        assert!(scope_of("surface:phone").is_err() && scope_of("surface:").is_err());
        assert!(scope_of("place:").is_err() && scope_of("here").is_err());
        assert_eq!(age_ms("7d").unwrap(), 7 * 86_400_000);
        assert!(age_ms("7").is_err());
    }

    #[test]
    fn an_add_says_what_it_replaced() {
        let rec = info(
            "std_0000aaaa",
            StandingKind::Preference,
            StandingScope::Everywhere,
            "short answers",
            "live",
        )
        .record;
        let r = StandingChanged {
            record: rec,
            ended: Some("std_1111bbbb".into()),
        };
        assert_eq!(
            added_line(&r),
            "Added preference std_0000aaaa for everywhere, replacing std_1111bbbb: every prompt of that scope carries it from its next turn."
        );
    }
}
