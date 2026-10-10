//! People's methods (theseus-wy7y): a person's merge and its undo
//! (`ontology.person.merge`), the automatic merge an exact handle makes,
//! and the operator's bulk yes to Jev's proposals
//! (`ontology.proposal.accept_all`). Each is the owner's act, from a private
//! place (`judge_act(Act::Ontology)`), before it reads anything; each write
//! is the ontology's (`ontology::Board::write`: checked, written in one
//! frame with its rows, held).
//!
//! A merge's row (`ontology.merged`, scoped [`scope`] of the absorbed
//! person) holds what it moved: the absorbed record as it was, the
//! survivor's handles before, and the sessions whose lists moved. Its undo
//! reads the newest such row back. A merge never happens on a display name
//! alone: only the operator's word, or an exact handle (`discord:`, `slack:`,
//! `email:`) two people hold.

use anyhow::{anyhow, bail, Result};
use theseus_ontology::{Category, CategoryId, Ontology, Origin, Record};
use theseus_protocol::{
    method, OntologyPersonMergeParams, OntologyPersonMerged, OntologyProposalAcceptAllParams,
    OntologyProposalAcceptAllResult, OntologyProposalAcceptParams, OntologyProposalsParams,
};

use super::confirms::Act;
use super::ontology::{category_info_of, resolve};
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::ledger::LedgerRow;

/// The methods `rpc_prefixed` routes here, to the ontology's arm: `dispatch`
/// stays within clippy's length that way.
pub(super) const ROUTE: [&str; 2] = [
    method::ONTOLOGY_PERSON_MERGE,
    method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
];

/// The kind people are.
const PERSON: &str = theseus_ontology::person::KIND;

/// The scope of a person's merge rows: an undo's read is one scan of it.
pub fn scope(absorbed: &CategoryId) -> String {
    format!("ontology.merged:{absorbed}")
}

/// What a merge moved, as its row holds it.
#[derive(serde::Deserialize)]
struct Moved {
    absorbed_record: Category,
    survivor: String,
    #[serde(default)]
    handles_before: Vec<String>,
    #[serde(default)]
    sessions: Vec<String>,
    #[serde(default)]
    lists_before: Vec<theseus_ontology::MemberList>,
    #[serde(default)]
    undone: bool,
}

impl Core {
    /// `ontology.person.merge`: `absorbed` into `survivor`, by the operator;
    /// or, with `undo`, the newest merge of `absorbed` undone.
    pub fn ontology_person_merge(
        &self,
        p: &OntologyPersonMergeParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyPersonMerged> {
        let who = who.into();
        let what = match p.undo {
            true => format!("the undo of {}'s merge", p.absorbed.trim()),
            false => format!(
                "the merge of {} into {}",
                p.absorbed.trim(),
                p.survivor.trim()
            ),
        };
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PERSON_MERGE,
                what: &what,
            },
        )?;
        let (who_s, via) = (who.who(), who.via());
        if p.undo {
            return self.person_unmerge(&p.absorbed, &who_s, &via);
        }
        let o = self.runner.ontology.snapshot(&self.store)?;
        let absorbed = resolve(&o, PERSON, &p.absorbed)?;
        let survivor = resolve(&o, PERSON, &p.survivor)?;
        self.person_merge_in(&o, &absorbed, &survivor, "operator", &who_s, &via)
    }

    /// Merge `absorbed` into `survivor` over `o`, as `how` (`operator`, or
    /// `automatic` for an exact handle): one frame, its records and row.
    pub(crate) fn person_merge_in(
        &self,
        o: &Ontology,
        absorbed: &CategoryId,
        survivor: &CategoryId,
        how: &str,
        who: &str,
        via: &str,
    ) -> Result<OntologyPersonMerged> {
        let m = o.merge(absorbed, survivor)?;
        let survivor_s = survivor.to_string();
        let row = fact::ontology::PersonMerged {
            absorbed: &m.absorbed,
            survivor: &survivor_s,
            handles_before: &m.survivor_handles_before,
            sessions: &m.sessions,
            lists_before: &m.lists_before,
            guidance_moved: m.guidance.is_some(),
            undone: false,
            how,
            who,
            via,
        };
        let scope = scope(absorbed);
        let o =
            self.runner
                .ontology
                .write(&self.store, m.records.clone(), Origin::Operator, |_| {
                    Ok(vec![fact::row(&row, None, None)?.scoped(&scope)])
                })?;
        self.rec(None).announce(&row);
        Ok(OntologyPersonMerged {
            survivor: category_info_of(&o, survivor)?,
            absorbed: absorbed.to_string(),
            sessions: m.sessions.len() as u64,
            guidance_moved: m.guidance.is_some(),
            undone: false,
        })
    }

    /// The newest merge of `absorbed` undone, from its row.
    fn person_unmerge(&self, absorbed: &str, who: &str, via: &str) -> Result<OntologyPersonMerged> {
        let id = CategoryId::parse(absorbed.trim()).map_err(|_| {
            anyhow!(
                "an undo names the merged person by its id (`person:<local>`), as its row has it"
            )
        })?;
        let mut newest: Option<Moved> = None;
        for r in self.store.scope_after(&scope(&id), 0)? {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if let Ok(m) = serde_json::from_value::<Moved>(row.data) {
                newest = Some(m);
            }
        }
        let Some(m) = newest.filter(|m| !m.undone) else {
            bail!(
                "{id} has no merge to undo: its newest `ontology.merged` row is an undo, or none"
            );
        };
        let o = self.runner.ontology.snapshot(&self.store)?;
        let survivor = CategoryId::parse(&m.survivor)?;
        let records = o.unmerge(
            &m.absorbed_record,
            &survivor,
            &m.handles_before,
            &m.lists_before,
        )?;
        let row = fact::ontology::PersonMerged {
            absorbed: &m.absorbed_record,
            survivor: &m.survivor,
            handles_before: &m.handles_before,
            sessions: &m.sessions,
            lists_before: &[],
            guidance_moved: false,
            undone: true,
            how: "operator",
            who,
            via,
        };
        let scope = scope(&id);
        let o = self
            .runner
            .ontology
            .write(&self.store, records, Origin::Operator, |_| {
                Ok(vec![fact::row(&row, None, None)?.scoped(&scope)])
            })?;
        self.rec(None).announce(&row);
        Ok(OntologyPersonMerged {
            survivor: category_info_of(&o, &survivor)?,
            absorbed: id.to_string(),
            sessions: m.sessions.len() as u64,
            guidance_moved: false,
            undone: true,
        })
    }

    /// A DM's person, at its first bind, whose `discord:<id>` another person
    /// holds (the import's, or the operator's): one frame, as the operator,
    /// that takes the handle off the other, makes the DM's person, and
    /// merges the other into it. The automatic merge of an exact handle.
    pub(crate) fn bind_person_merging(&self, c: &Category) -> Result<bool> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        if o.category(&c.id).is_some() {
            return Ok(false);
        }
        let Some((h, twin)) = o.handle_twin(c) else {
            return Ok(false);
        };
        let mut work = (*o).clone();
        let mut stripped = work
            .category(&twin)
            .cloned()
            .ok_or_else(|| anyhow!("the person {twin} is gone"))?;
        stripped.handles.retain(|x| *x != h);
        let mut records = vec![Record::Category(stripped), Record::Category(c.clone())];
        for r in &records {
            work.put(r.clone(), Origin::Operator)?;
        }
        let m = work.merge(&twin, &c.id)?;
        let mut before = m.survivor_handles_before.clone();
        before.retain(|x| *x != h);
        let mut absorbed = m.absorbed.clone();
        absorbed.handles.push(h.clone());
        records.extend(m.records.iter().cloned());
        let survivor = c.id.to_string();
        let set = fact::ontology::CategorySet {
            category: c,
            origin: Origin::Transport.name(),
            who: "the binding",
            via: "discord",
        };
        let row = fact::ontology::PersonMerged {
            absorbed: &absorbed,
            survivor: &survivor,
            handles_before: &before,
            sessions: &m.sessions,
            lists_before: &m.lists_before,
            guidance_moved: m.guidance.is_some(),
            undone: false,
            how: "automatic",
            who: "the binding",
            via: "discord",
        };
        let scope = scope(&twin);
        self.runner
            .ontology
            .write(&self.store, records, Origin::Operator, |_| {
                Ok(vec![
                    fact::row(&set, None, None)?,
                    fact::row(&row, None, None)?.scoped(&scope),
                ])
            })?;
        self.rec(None).announce(&set);
        self.rec(None).announce(&row);
        Ok(true)
    }

    /// `ontology.proposal.accept_all`: each unanswered proposal of `kind` at
    /// or above `min_confidence` (or exactly the judgments named), accepted
    /// one by one as `ontology.proposal.accept` takes it; a proposal that
    /// needs a name (a new topic or person) is left, with why.
    pub fn ontology_proposal_accept_all(
        &self,
        p: &OntologyProposalAcceptAllParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyProposalAcceptAllResult> {
        let who = who.into();
        let what = format!(
            "every {} proposal at {:.2} or more",
            p.kind.as_deref().unwrap_or("topic and person"),
            p.min_confidence
        );
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
                what: &what,
            },
        )?;
        let all = self.ontology_proposals(&OntologyProposalsParams {
            session_id: None,
            limit: Some(u32::MAX),
        })?;
        let mut out = OntologyProposalAcceptAllResult::default();
        for pr in all.proposals {
            // A new topic's proposal is of the topic kind; a person's,
            // held or new, of the person kind (theseus-wy7y).
            let kind = match &pr.person {
                Some(_) => PERSON,
                None => pr
                    .topic
                    .as_deref()
                    .and_then(|t| t.split_once(':'))
                    .map_or("topic", |(k, _)| k),
            };
            let picked = match p.judgments.is_empty() {
                true => {
                    pr.confidence >= p.min_confidence && p.kind.as_deref().is_none_or(|k| kind == k)
                }
                false => p.judgments.contains(&pr.judgment),
            };
            if !picked {
                continue;
            }
            // A new person's proposal names its person: it needs no name.
            if pr.new_topic || (pr.topic.is_none() && pr.person.is_none()) {
                out.left.push(format!(
                    "{}: it proposes a new category, which needs a name: accept it alone",
                    pr.judgment
                ));
                continue;
            }
            let one = OntologyProposalAcceptParams {
                judgment: pr.judgment.clone(),
                note: Some("accepted in bulk".into()),
                ..Default::default()
            };
            match self.ontology_proposal_accept(&one, who.clone()) {
                Ok(_) => out.accepted.push(pr.judgment),
                Err(e) => out.left.push(format!("{}: {e:#}", pr.judgment)),
            }
        }
        Ok(out)
    }
}
