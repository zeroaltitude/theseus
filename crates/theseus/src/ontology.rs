//! `theseus ontology` (theseus-8kk.1; M4 design §2.8's surfaces): the
//! kinds, the category tree, a topic, a category's guidance, and a
//! session's memberships. The writes are the operator's: the daemon judges
//! them as it does an answer, and the client refuses them inside a job
//! (`client::refuse_in_a_job`).

use std::io::Read;

use anyhow::{bail, Result};
use theseus_client::{render, Conn};
use theseus_protocol::{
    method, OntologyCategory, OntologyCategoryAddParams, OntologyGuidance,
    OntologyGuidanceSetParams, OntologyListParams, OntologyListResult, OntologyMembershipResult,
    OntologyMembershipSetParams, OntologyPersonMergeParams, OntologyPersonMerged,
    OntologyPersonProposals, OntologyProposalAcceptAllParams, OntologyProposalAcceptAllResult,
    OntologyProposalAcceptParams, OntologyProposalAnswered, OntologyProposalRejectAllParams,
    OntologyProposalRejectAllResult, OntologyProposalRejectParams, OntologyProposalsParams,
    OntologyProposalsResult,
};

use crate::cmd::output;
use crate::{OntologyCmd, PersonCmd, TopicCmd};

/// `ontology.list` for the kinds and the tree alone: an import's tens of
/// thousands of memberships are left out (theseus-anh3).
fn tree_only() -> OntologyListParams {
    OntologyListParams {
        session_id: None,
        memberships: Some(false),
    }
}

pub async fn ontology(conn: &mut Conn, json: bool, cmd: OntologyCmd) -> Result<()> {
    match cmd {
        OntologyCmd::Kinds => {
            let v = conn.request(method::ONTOLOGY_LIST, tree_only()).await?;
            output(json, v, |r: OntologyListResult| {
                print(render::ontology_kinds_lines(&r.kinds));
                Ok(())
            })
        }
        OntologyCmd::Categories => {
            let v = conn.request(method::ONTOLOGY_LIST, tree_only()).await?;
            output(json, v, |r: OntologyListResult| {
                print(render::ontology_categories_lines(&r.categories));
                Ok(())
            })
        }
        OntologyCmd::Topic {
            cmd: TopicCmd::Add { name, parent, desc },
        } => topic_add(conn, json, name, parent, desc).await,
        OntologyCmd::Guide { category, text } => guide(conn, json, category, text).await,
        OntologyCmd::Member { session, changes } => member(conn, json, session, changes).await,
        OntologyCmd::Proposals {
            session,
            limit,
            min_confidence,
            each,
        } => {
            let p = OntologyProposalsParams {
                session_id: session,
                limit,
                by_person: !each,
                min_confidence,
            };
            proposals(conn, json, p).await
        }
        OntologyCmd::Person { cmd } => person(conn, json, cmd).await,
        OntologyCmd::Accept {
            judgment: None,
            person: Some(name),
            as_person,
            ..
        } => row_accept(conn, json, &name, as_person).await,
        OntologyCmd::Accept {
            judgment: None,
            kind,
            min_confidence,
            ..
        } => accept_all(conn, json, kind, min_confidence).await,
        OntologyCmd::Accept {
            judgment: Some(judgment),
            topic,
            desc,
            note,
            as_person,
            ..
        } => {
            let v = conn
                .request(
                    method::ONTOLOGY_PROPOSAL_ACCEPT,
                    OntologyProposalAcceptParams {
                        judgment,
                        topic,
                        description: desc,
                        as_person,
                        note,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, answered)
        }
        OntologyCmd::Reject {
            judgment: None,
            person,
            note,
        } => row_reject(conn, json, person.as_deref().unwrap_or_default(), note).await,
        OntologyCmd::Reject {
            judgment: Some(judgment),
            note,
            ..
        } => {
            let v = conn
                .request(
                    method::ONTOLOGY_PROPOSAL_REJECT,
                    OntologyProposalRejectParams {
                        judgment,
                        note,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, answered)
        }
    }
}

/// `theseus ontology proposals`: people a row per person unless `--each`
/// (theseus-fvyx), then the topics'.
async fn proposals(conn: &mut Conn, json: bool, p: OntologyProposalsParams) -> Result<()> {
    let v = conn.request(method::ONTOLOGY_PROPOSALS, p).await?;
    output(json, v, |r: OntologyProposalsResult| {
        print(render::ontology_proposals_lines(&r));
        Ok(())
    })
}

/// `theseus ontology topic add`.
async fn topic_add(
    conn: &mut Conn,
    json: bool,
    name: String,
    parent: Option<String>,
    desc: Option<String>,
) -> Result<()> {
    let v = conn
        .request(
            method::ONTOLOGY_CATEGORY_ADD,
            OntologyCategoryAddParams {
                kind: Some("topic".into()),
                name,
                parent,
                description: desc,
                handles: Vec::new(),
                author: None,
                discord: None,
            },
        )
        .await?;
    output(json, v, |c: OntologyCategory| {
        println!(
            "topic {} ({}){}",
            c.name,
            c.id,
            c.parent.map(|p| format!(", under {p}")).unwrap_or_default()
        );
        Ok(())
    })
}

/// `theseus ontology accept` with no judgment: every proposal of `kind` at
/// `min_confidence` or more (theseus-wy7y).
async fn accept_all(
    conn: &mut Conn,
    json: bool,
    kind: Option<String>,
    min_confidence: Option<f64>,
) -> Result<()> {
    if kind.is_none() && min_confidence.is_none() {
        bail!(
            "name a JUDGMENT, or accept in bulk with --kind topic|person and \
                     --min-confidence X (`theseus ontology proposals` lists them)"
        );
    }
    let v = conn
        .request(
            method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
            OntologyProposalAcceptAllParams {
                kind,
                min_confidence: min_confidence.unwrap_or(0.0),
                ..Default::default()
            },
        )
        .await?;
    output(json, v, |r: OntologyProposalAcceptAllResult| {
        println!("{} accepted.", r.accepted.len());
        for j in &r.accepted {
            println!("  {j}");
        }
        if !r.left.is_empty() {
            println!("{} left for one at a time:", r.left.len());
            for l in &r.left {
                println!("  {l}");
            }
        }
        Ok(())
    })
}

/// A name folded as the daemon folds a person's: lowercase, single-spaced,
/// a handle's "@" off each word.
fn fold(name: &str) -> String {
    name.split_whitespace()
        .map(|w| w.trim_start_matches('@'))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The person's row `name` names (theseus-fvyx): by its key, its id, or its
/// name folded.
async fn row_named(conn: &mut Conn, name: &str) -> Result<OntologyPersonProposals> {
    let v = conn
        .request(
            method::ONTOLOGY_PROPOSALS,
            OntologyProposalsParams {
                limit: Some(u32::MAX),
                by_person: true,
                ..Default::default()
            },
        )
        .await?;
    let r: OntologyProposalsResult = serde_json::from_value(v)?;
    let (want, folded) = (name.trim(), fold(name));
    let mut found: Vec<OntologyPersonProposals> = r
        .people
        .into_iter()
        .filter(|g| g.key == want || g.as_person == want || fold(&g.name) == folded)
        .collect();
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => bail!("no person's row is named {name:?}: `theseus ontology proposals` lists them"),
        n => bail!(
            "{n} rows are named {name:?}: name one by its key ({})",
            found
                .iter()
                .map(|g| g.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `theseus ontology accept --person NAME`: every proposal of the row, as
/// its person (or `--as`'s).
async fn row_accept(
    conn: &mut Conn,
    json: bool,
    name: &str,
    as_person: Option<String>,
) -> Result<()> {
    let row = row_named(conn, name).await?;
    if !row.ambiguous.is_empty() && as_person.is_none() {
        bail!(
            "\"{}\" is a word of {} people's names ({}): pick one with --as NAME, or --as \"{}\" \
             for a new person of that name",
            row.name,
            row.ambiguous.len(),
            row.ambiguous.join(", "),
            row.name
        );
    }
    let to = as_person.unwrap_or_else(|| row.as_person.clone());
    let v = conn
        .request(
            method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
            OntologyProposalAcceptAllParams {
                judgments: row.judgments.clone(),
                as_person: Some(to.clone()),
                ..Default::default()
            },
        )
        .await?;
    output(json, v, |r: OntologyProposalAcceptAllResult| {
        println!(
            "{} of {}'s {} accepted, as {to}; each session joins at its next recompile.",
            r.accepted.len(),
            row.name,
            count_proposals(row.judgments.len())
        );
        left(&r.left);
        Ok(())
    })
}

/// `theseus ontology reject --person NAME`: every proposal of the row.
async fn row_reject(conn: &mut Conn, json: bool, name: &str, note: Option<String>) -> Result<()> {
    let row = row_named(conn, name).await?;
    let v = conn
        .request(
            method::ONTOLOGY_PROPOSAL_REJECT_ALL,
            OntologyProposalRejectAllParams {
                judgments: row.judgments.clone(),
                note,
                ..Default::default()
            },
        )
        .await?;
    output(json, v, |r: OntologyProposalRejectAllResult| {
        println!(
            "{} of {}'s {} rejected.",
            r.rejected.len(),
            row.name,
            count_proposals(row.judgments.len())
        );
        left(&r.left);
        Ok(())
    })
}

fn count_proposals(n: usize) -> String {
    match n {
        1 => "1 proposal".into(),
        n => format!("{n} proposals"),
    }
}

fn left(left: &[String]) {
    if !left.is_empty() {
        println!("{} left:", left.len());
        for l in left {
            println!("  {l}");
        }
    }
}

/// `theseus ontology person`: add one, or merge two (theseus-wy7y).
async fn person(conn: &mut Conn, json: bool, cmd: PersonCmd) -> Result<()> {
    match cmd {
        PersonCmd::Add {
            name,
            handles,
            desc,
        } => {
            let v = conn
                .request(
                    method::ONTOLOGY_CATEGORY_ADD,
                    OntologyCategoryAddParams {
                        kind: Some("person".into()),
                        name,
                        parent: None,
                        description: desc,
                        handles,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, |c: OntologyCategory| {
                println!("person {} ({})", c.name, c.id);
                if !c.handles.is_empty() {
                    println!("  handles: {}", c.handles.join(", "));
                }
                Ok(())
            })
        }
        PersonCmd::Merge { a, b, undo } => {
            let v = conn
                .request(
                    method::ONTOLOGY_PERSON_MERGE,
                    OntologyPersonMergeParams {
                        absorbed: a,
                        survivor: b.unwrap_or_default(),
                        undo,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, |m: OntologyPersonMerged| {
                match m.undone {
                    true => println!(
                        "undid the merge of {} into {}: {} sessions' lists as they were.",
                        m.absorbed, m.survivor.id, m.sessions
                    ),
                    false => println!(
                        "merged {} into {} ({}): {} sessions moved{}.",
                        m.absorbed,
                        m.survivor.name,
                        m.survivor.id,
                        m.sessions,
                        if m.guidance_moved {
                            ", and its guidance"
                        } else {
                            ""
                        }
                    ),
                }
                if !m.survivor.handles.is_empty() {
                    println!("  handles: {}", m.survivor.handles.join(", "));
                }
                Ok(())
            })
        }
    }
}

/// What an accept or a reject wrote.
fn answered(r: OntologyProposalAnswered) -> Result<()> {
    match &r.topic {
        Some(t) => println!(
            "{}: accepted; {} joins {t} (yours, operator), at its next recompile. Label {}.",
            r.judgment, r.session_id, r.label_id
        ),
        None => println!("{}: rejected. Label {}.", r.judgment, r.label_id),
    }
    print(render::ontology_memberships_lines(&r.memberships));
    Ok(())
}

/// `theseus ontology guide`: a category's guidance, from TEXT or stdin.
async fn guide(conn: &mut Conn, json: bool, category: String, text: Option<String>) -> Result<()> {
    let text = match text.as_deref() {
        None | Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
        Some(t) => t.to_string(),
    };
    let v = conn
        .request(
            method::ONTOLOGY_GUIDANCE_SET,
            OntologyGuidanceSetParams {
                category,
                text,
                author: None,
                discord: None,
            },
        )
        .await?;
    output(json, v, |g: OntologyGuidance| {
        match g.text.is_empty() {
            true => println!("{}: no guidance (version {}).", g.category, g.version),
            false => println!(
                "{}: guidance version {}, {} bytes, digest {}. A session that carries it \
                 recompiles at its next turn.",
                g.category,
                g.version,
                g.text.len(),
                g.digest
            ),
        }
        Ok(())
    })
}

/// `theseus ontology member`: a session's memberships, or a change of them.
async fn member(conn: &mut Conn, json: bool, session: String, changes: Vec<String>) -> Result<()> {
    if changes.is_empty() {
        let v = conn
            .request(
                method::ONTOLOGY_LIST,
                OntologyListParams {
                    session_id: Some(session),
                    memberships: None,
                },
            )
            .await?;
        return output(json, v, |r: OntologyListResult| {
            print(render::ontology_memberships_lines(&r.memberships));
            Ok(())
        });
    }
    let (mut add, mut remove) = (Vec::new(), Vec::new());
    for c in changes {
        match (c.strip_prefix('+'), c.strip_prefix('-')) {
            (Some(a), _) if !a.is_empty() => add.push(a.to_string()),
            (_, Some(r)) if !r.is_empty() => remove.push(r.to_string()),
            _ => bail!(
                "{c:?}: each change is +CATEGORY (add) or -CATEGORY (take away), as in \
                 `theseus ontology member SESSION +theseus -lamps`"
            ),
        }
    }
    let v = conn
        .request(
            method::ONTOLOGY_MEMBERSHIP_SET,
            OntologyMembershipSetParams {
                session_id: session,
                add,
                remove,
                author: None,
                discord: None,
            },
        )
        .await?;
    output(json, v, |r: OntologyMembershipResult| {
        print(render::ontology_memberships_lines(&r.memberships));
        match r.changed.is_empty() {
            true => println!("Nothing changed."),
            false => println!(
                "They apply at the session's next recompile: `theseus sessions recompile \
                 {}` asks for one.",
                r.session_id
            ),
        }
        Ok(())
    })
}

fn print(lines: Vec<String>) {
    for l in lines {
        println!("{l}");
    }
}
