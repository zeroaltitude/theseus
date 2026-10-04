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
    OntologyMembershipSetParams, OntologyProposalAcceptParams, OntologyProposalAnswered,
    OntologyProposalRejectParams, OntologyProposalsParams, OntologyProposalsResult,
};

use crate::cmd::output;
use crate::{OntologyCmd, TopicCmd};

pub async fn ontology(conn: &mut Conn, json: bool, cmd: OntologyCmd) -> Result<()> {
    match cmd {
        OntologyCmd::Kinds => {
            let v = conn
                .request(method::ONTOLOGY_LIST, OntologyListParams::default())
                .await?;
            output(json, v, |r: OntologyListResult| {
                print(render::ontology_kinds_lines(&r.kinds));
                Ok(())
            })
        }
        OntologyCmd::Categories => {
            let v = conn
                .request(method::ONTOLOGY_LIST, OntologyListParams::default())
                .await?;
            output(json, v, |r: OntologyListResult| {
                print(render::ontology_categories_lines(&r.categories));
                Ok(())
            })
        }
        OntologyCmd::Topic {
            cmd: TopicCmd::Add { name, parent, desc },
        } => {
            let v = conn
                .request(
                    method::ONTOLOGY_CATEGORY_ADD,
                    OntologyCategoryAddParams {
                        kind: Some("topic".into()),
                        name,
                        parent,
                        description: desc,
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
        OntologyCmd::Guide { category, text } => guide(conn, json, category, text).await,
        OntologyCmd::Member { session, changes } => member(conn, json, session, changes).await,
        OntologyCmd::Proposals { session, limit } => {
            let v = conn
                .request(
                    method::ONTOLOGY_PROPOSALS,
                    OntologyProposalsParams {
                        session_id: session,
                        limit,
                    },
                )
                .await?;
            output(json, v, |r: OntologyProposalsResult| {
                print(render::ontology_proposals_lines(&r));
                Ok(())
            })
        }
        OntologyCmd::Accept {
            judgment,
            topic,
            desc,
            note,
        } => {
            let v = conn
                .request(
                    method::ONTOLOGY_PROPOSAL_ACCEPT,
                    OntologyProposalAcceptParams {
                        judgment,
                        topic,
                        description: desc,
                        note,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, answered)
        }
        OntologyCmd::Reject { judgment, note } => {
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
