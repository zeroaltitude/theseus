//! The ontology's three new writes and the latency reader (theseus-xzxqv):
//! `theseus ontology undo`, `theseus ontology kind add|retire`, and
//! `theseus judge latency`. The writes are the operator's: the daemon judges
//! them as it does any ontology write, and the client refuses them inside a
//! job (`client::OPERATORS`).

use anyhow::Result;
use clap::{Args, Subcommand};
use theseus_client::Conn;
use theseus_protocol::judge::{JudgeLatencyParams, JudgeLatencyResult};
use theseus_protocol::{
    method, OntologyKindAddParams, OntologyKindResult, OntologyKindRetireParams,
    OntologyNeverListParams, OntologyNeverListResult, OntologyNeverUndoParams,
    OntologyNeverUndoResult, OntologyProposalNeverParams, OntologyProposalNeverResult,
    OntologyUndoParams, OntologyUndoResult,
};

use crate::cmd::output;

/// `theseus ontology kind`.
#[derive(Subcommand, Debug)]
pub enum KindCmd {
    /// Add an interpreted kind to the table, NAME. Jev may assign it unless you leave `jev` out of
    /// `--assigned-by`; its categories are declared, filed or minted like a topic's.
    Add(KindAdd),
    /// Take a kind away: it must hold no category and no session's membership, and be neither
    /// given nor seeded.
    Retire { name: String },
}

#[derive(Args, Debug)]
pub struct KindAdd {
    pub name: String,
    /// What the kind is for, in a line.
    #[arg(long, default_value = "")]
    pub desc: String,
    /// The origins that may assign it: operator, import, jev, sweep, dream (default operator, jev).
    #[arg(long, value_delimiter = ',')]
    pub assigned_by: Vec<String>,
    /// How many a session holds: a number or `many` (default 3).
    #[arg(long)]
    pub per_session: Option<String>,
    /// The kind its categories nest under (its own name, for categories that nest in themselves).
    #[arg(long)]
    pub parent: Option<String>,
    /// Where it sits in a compile, lowest first (default after the last kind).
    #[arg(long)]
    pub precedence: Option<u32>,
    /// `chain` (default) or `intent_line`.
    #[arg(long)]
    pub rule: Option<String>,
    /// Up to five category names that show what it is for.
    #[arg(long, value_delimiter = ',')]
    pub examples: Vec<String>,
    /// Where names are mined from: titles, paths, hosts, capitalised (default all).
    #[arg(long, value_delimiter = ',')]
    pub mined_from: Vec<String>,
}

/// `theseus judge latency`.
#[derive(Args, Debug)]
pub struct LatencyArgs {
    /// The decision point whose requests are timed (`inbound`: the batch with `classify` in it).
    #[arg(long, alias = "point", default_value = "inbound")]
    pub pack: String,
    /// From this local day (`2026-10-19`) on; default the last 7 days.
    #[arg(long)]
    pub since: Option<String>,
}

/// `theseus ontology undo WRITE`.
pub async fn undo(conn: &mut Conn, json: bool, write: String) -> Result<()> {
    let v = conn
        .request(
            method::ONTOLOGY_UNDO,
            OntologyUndoParams {
                write,
                author: None,
                discord: None,
            },
        )
        .await?;
    output(json, v, |r: OntologyUndoResult| {
        println!(
            "Undone {}: {} put back. They apply at the session's next recompile.",
            r.write,
            match r.keys.len() {
                1 => "1 record".to_string(),
                n => format!("{n} records"),
            }
        );
        for k in &r.keys {
            println!("  {k}");
        }
        Ok(())
    })
}

/// `theseus ontology kind add|retire`.
pub async fn kind(conn: &mut Conn, json: bool, cmd: KindCmd) -> Result<()> {
    let v = match cmd {
        KindCmd::Add(a) => {
            conn.request(
                method::ONTOLOGY_KIND_ADD,
                OntologyKindAddParams {
                    name: a.name,
                    description: a.desc,
                    assigned_by: a.assigned_by,
                    per_session: a.per_session,
                    parent: a.parent,
                    precedence: a.precedence,
                    rule: a.rule,
                    examples: a.examples,
                    mined_from: a.mined_from,
                    author: None,
                    discord: None,
                },
            )
            .await?
        }
        KindCmd::Retire { name } => {
            conn.request(
                method::ONTOLOGY_KIND_RETIRE,
                OntologyKindRetireParams {
                    name,
                    author: None,
                    discord: None,
                },
            )
            .await?
        }
    };
    output(json, v, |r: OntologyKindResult| {
        let k = r.kind;
        println!(
            "{} the kind {} (precedence {}, {} a session, assigned by {}).",
            if r.retired { "Took away" } else { "Added" },
            k.name,
            k.precedence,
            k.per_session,
            k.assigned_by.join(", ")
        );
        Ok(())
    })
}

/// `theseus judge latency`: the batch's requests by day, p50 and p95.
pub async fn latency(conn: &mut Conn, json: bool, a: LatencyArgs) -> Result<()> {
    let v = conn
        .request(
            method::JUDGE_LATENCY,
            JudgeLatencyParams {
                point: Some(a.pack),
                since: a.since,
            },
        )
        .await?;
    output(json, v, |r: JudgeLatencyResult| {
        println!("{}'s requests by local day (ms):", r.point);
        println!("day         requests   p50   p95   with onto.v1 (p95)");
        for d in &r.days {
            println!(
                "{}  {:>8}  {:>4}  {:>4}   {} ({})",
                d.day, d.requests, d.p50_ms, d.p95_ms, d.with_onto, d.onto_p95_ms
            );
        }
        if r.days.is_empty() {
            println!("  none: no judgment of this point is recorded since then.");
        }
        Ok(())
    })
}

/// `theseus ontology never`.
#[derive(Args, Debug)]
pub struct NeverArgs {
    /// Proposals (their judgment ids, `theseus ontology proposals`) never to propose again.
    #[arg(conflicts_with_all = ["list", "undo"])]
    pub judgments: Vec<String>,
    /// The nevers in force, and the config's `not_topics` and `not_people`.
    #[arg(long, conflicts_with = "undo")]
    pub list: bool,
    /// Take a never back, by its key from --list (`topic:harbor`, `person:sable thorn`).
    #[arg(long, value_name = "KEY")]
    pub undo: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
}

/// `theseus ontology never [JUDGMENT...] [--list] [--undo KEY]`.
pub async fn never(conn: &mut Conn, json: bool, a: NeverArgs) -> Result<()> {
    if let Some(key) = a.undo {
        let v = conn
            .request(
                method::ONTOLOGY_NEVER_UNDO,
                OntologyNeverUndoParams {
                    key,
                    ..Default::default()
                },
            )
            .await?;
        return output(json, v, |r: OntologyNeverUndoResult| {
            println!(
                "Undone: {} {:?} may be proposed again.",
                r.undone.kind, r.undone.name
            );
            Ok(())
        });
    }
    if a.list || a.judgments.is_empty() {
        let v = conn
            .request(method::ONTOLOGY_NEVER_LIST, OntologyNeverListParams {})
            .await?;
        return output(json, v, |r: OntologyNeverListResult| {
            for n in &r.nevers {
                println!("{}  (by {})", n.key, n.who);
            }
            if r.nevers.is_empty() {
                println!("No nevers kept in the store.");
            }
            for t in &r.config_topics {
                println!("config not_topics: {t}");
            }
            for t in &r.config_people {
                println!("config not_people: {t}");
            }
            Ok(())
        });
    }
    let v = conn
        .request(
            method::ONTOLOGY_PROPOSAL_NEVER,
            OntologyProposalNeverParams {
                judgments: a.judgments,
                note: a.note,
                ..Default::default()
            },
        )
        .await?;
    output(json, v, |r: OntologyProposalNeverResult| {
        println!(
            "{} rejected; never proposed again: {}.",
            r.rejected.len(),
            match r.nevers.is_empty() {
                true => "nothing new".to_string(),
                false => r
                    .nevers
                    .iter()
                    .map(|n| n.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        );
        for l in &r.left {
            println!("  left: {l}");
        }
        Ok(())
    })
}
