//! `ontology.person.candidates` (theseus-jrffh): the pairs of people that may
//! be one person, from `judge::people::cands`, which reads no model. The
//! ontology gives the people and the sessions that hold them; a pair that
//! shares sessions is checked against those sessions' own messages, and one
//! message that names both people takes the pair out (two names in one message
//! are two people). Nothing is written.

use std::collections::BTreeSet;

use anyhow::Result;
use theseus_ontology::person;
use theseus_protocol::{
    OntologyPersonCandidate, OntologyPersonCandidatesParams, OntologyPersonCandidatesResult,
};

use super::Core;
use crate::judge::people::cands::{candidates, named_together, Who};
use crate::node::Body;

/// The shared sessions read for one pair, and the nodes of each.
const SESSIONS_PER_PAIR: usize = 8;

impl Core {
    /// `ontology.person.candidates`: a read.
    pub fn ontology_person_candidates(
        &self,
        p: &OntologyPersonCandidatesParams,
    ) -> Result<OntologyPersonCandidatesResult> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let mut people: Vec<Who> = o
            .categories()
            .filter(|c| {
                c.kind() == person::KIND && c.retired_ms.is_none() && c.merged_into.is_none()
            })
            .map(|c| Who {
                id: c.id.to_string(),
                name: c.name.clone(),
                handles: person::handles_of(c)
                    .into_iter()
                    .filter(|h| !person::is_name(h))
                    .collect(),
                role: c.description.clone(),
                sessions: BTreeSet::new(),
            })
            .collect();
        for l in o.lists().filter(|l| l.kind == person::KIND) {
            for m in &l.members {
                if let Some(w) = people.iter_mut().find(|w| w.id == m.category.to_string()) {
                    w.sessions.insert(l.session.clone());
                }
            }
        }
        people.sort_by(|a, b| a.id.cmp(&b.id));
        let apart = |a: &Who, b: &Who| {
            a.sessions
                .intersection(&b.sessions)
                .take(SESSIONS_PER_PAIR)
                .any(|sid| {
                    self.store.session_nodes(sid).is_ok_and(|nodes| {
                        nodes.iter().any(|(_, n)| match &n.body {
                            Body::UserMessage { text, .. } => {
                                named_together(text, &a.name, &b.name)
                            }
                            _ => false,
                        })
                    })
                })
        };
        let (found, left_out) = candidates(&people, &self.not_same_pairs(), &apart);
        let name_of = |id: &str| {
            people
                .iter()
                .find(|w| w.id == id)
                .map(|w| w.name.clone())
                .unwrap_or_default()
        };
        let limit = p.limit.map_or(50, |n| n as usize);
        Ok(OntologyPersonCandidatesResult {
            candidates: found
                .into_iter()
                .take(limit)
                .map(|c| OntologyPersonCandidate {
                    a_name: name_of(&c.a),
                    b_name: name_of(&c.b),
                    a: c.a,
                    b: c.b,
                    kind: c.kind.into(),
                    score: c.score,
                    why: c.why,
                    could_be: c.could_be,
                })
                .collect(),
            left_out: left_out as u64,
        })
    }
}
