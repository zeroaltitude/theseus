//! People's methods (theseus-wy7y): a person's merge and its undo
//! (`ontology.person.merge`), the automatic merge an exact handle makes,
//! and the operator's bulk answers to Jev's proposals
//! (`ontology.proposal.accept_all`, and `ontology.proposal.reject_all` for
//! one person's row, theseus-fvyx). Each is the owner's act, from a private
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

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use theseus_ontology::{Category, CategoryId, Ontology, Origin, Record};
use theseus_protocol::{
    method, OntologyPersonMergeParams, OntologyPersonMerged, OntologyProposal,
    OntologyProposalAcceptAllParams, OntologyProposalAcceptAllResult, OntologyProposalAcceptParams,
    OntologyProposalRejectAllParams, OntologyProposalRejectAllResult,
};

use super::confirms::Act;
use super::ontology::{category_info_of, resolve};
use super::proposals::Proposed;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::ledger::LedgerRow;

/// The methods `rpc_prefixed` routes here, to the ontology's arm: `dispatch`
/// stays within clippy's length that way.
pub(super) const ROUTE: [&str; 3] = [
    method::ONTOLOGY_PERSON_MERGE,
    method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
    method::ONTOLOGY_PROPOSAL_REJECT_ALL,
];

/// The listed proposals by judgment: what a bulk answer may take.
type Listed = HashMap<String, (OntologyProposal, Proposed)>;

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

    /// The unanswered proposals listed now (the exclusions' aside), by
    /// judgment, and the ontology they were read over.
    fn listed_now(
        &self,
    ) -> Result<(
        Listed,
        std::sync::Arc<Ontology>,
        crate::judge::people::NotPeople,
    )> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let not = self.not_people(&[], &o);
        let (listed, _) = self.proposals_listed(&o, &not, None)?;
        let by = listed
            .into_iter()
            .map(|(l, pr)| (l.judgment.clone(), (l, pr)))
            .collect();
        Ok((by, o, not))
    }

    /// `ontology.proposal.accept_all`: each unanswered proposal of `kind` at
    /// or above `min_confidence`, people a row at a time (theseus-fvyx: each
    /// person whose best proposal reaches it, all of the row, a first name
    /// as its person), or exactly the judgments named (as `as_person`, when
    /// given), each accepted as `ontology.proposal.accept` takes it, judged
    /// once; a proposal that needs a name (a new topic) or a first name
    /// several people's names hold is left, with why.
    pub fn ontology_proposal_accept_all(
        &self,
        p: &OntologyProposalAcceptAllParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyProposalAcceptAllResult> {
        let who = who.into();
        let what = match p.judgments.is_empty() {
            true => format!(
                "every {} proposal at {:.2} or more",
                p.kind.as_deref().unwrap_or("topic and person"),
                p.min_confidence
            ),
            false => format!("{} proposals", p.judgments.len()),
        };
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
                what: &what,
            },
        )?;
        let (mut by, o, not) = self.listed_now()?;
        let mut out = OntologyProposalAcceptAllResult::default();
        if !p.judgments.is_empty() {
            let mut picked = Vec::new();
            for j in &p.judgments {
                match by.remove(j) {
                    Some(x) => picked.push(x),
                    None => out.left.push(not_listed(j)),
                }
            }
            // The person `as_person` names first, so a declared one takes
            // its own proposal's handles and role line.
            if let Some(name) = p.as_person.as_deref() {
                let name = crate::judge::people::fold(name);
                picked.sort_by_key(|(l, _)| {
                    l.person
                        .as_ref()
                        .is_none_or(|w| crate::judge::people::fold(&w.name) != name)
                });
            }
            for (l, pr) in picked {
                self.accept_bulk(&l, pr, p.as_person.as_deref(), &who, &mut out);
            }
            return Ok(out);
        }
        let listed: Vec<OntologyProposal> = by.values().map(|(l, _)| l.clone()).collect();
        let mut listed = listed;
        listed.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(a.judgment.cmp(&b.judgment)));
        let (topics, rows) = super::proposal_groups::group(&o, &not, listed, p.min_confidence);
        for t in topics {
            let kind = t
                .topic
                .as_deref()
                .and_then(|t| t.split_once(':'))
                .map_or("topic", |(k, _)| k);
            if p.kind.as_deref().is_none_or(|k| kind == k) {
                if let Some((l, pr)) = by.remove(&t.judgment) {
                    self.accept_bulk(&l, pr, None, &who, &mut out);
                }
            }
        }
        if p.kind.as_deref().is_some_and(|k| k != PERSON) {
            return Ok(out);
        }
        for row in rows {
            if !row.ambiguous.is_empty() {
                out.left.extend(row.judgments.iter().map(|j| {
                    format!(
                        "{j}: \"{}\" is a word of {} people's names ({}): accept it alone with --as",
                        row.name,
                        row.ambiguous.len(),
                        row.ambiguous.join(", ")
                    )
                }));
                continue;
            }
            // The row's own name's proposals first, then its first names'.
            let mut js: Vec<&String> = row.judgments.iter().collect();
            js.sort_by_key(|j| {
                by.get(*j)
                    .and_then(|(l, _)| l.person.as_ref())
                    .is_some_and(|w| row.first_names.contains(&w.name.trim().to_string()))
            });
            for j in js {
                if let Some((l, pr)) = by.remove(j) {
                    self.accept_bulk(&l, pr, Some(&row.as_person), &who, &mut out);
                }
            }
        }
        Ok(out)
    }

    /// One proposal of a bulk yes, judged already.
    fn accept_bulk(
        &self,
        l: &OntologyProposal,
        pr: Proposed,
        as_person: Option<&str>,
        who: &Answerer,
        out: &mut OntologyProposalAcceptAllResult,
    ) {
        // A new person's proposal names its person: it needs no name.
        if l.new_topic || (l.topic.is_none() && l.person.is_none()) {
            out.left.push(format!(
                "{}: it proposes a new category, which needs a name: accept it alone",
                l.judgment
            ));
            return;
        }
        let one = OntologyProposalAcceptParams {
            judgment: l.judgment.clone(),
            as_person: as_person.filter(|_| l.person.is_some()).map(str::to_string),
            note: Some("accepted in bulk".into()),
            ..Default::default()
        };
        match self.accept_open(pr, &one, who) {
            Ok(_) => out.accepted.push(l.judgment.clone()),
            Err(e) => out.left.push(format!("{}: {e:#}", l.judgment)),
        }
    }

    /// `ontology.proposal.reject_all` (theseus-fvyx): each named proposal
    /// listed now rejected, as `ontology.proposal.reject` takes it, judged
    /// once; one not listed is left, with why.
    pub fn ontology_proposal_reject_all(
        &self,
        p: &OntologyProposalRejectAllParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyProposalRejectAllResult> {
        let who = who.into();
        let what = format!("{} proposals", p.judgments.len());
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PROPOSAL_REJECT_ALL,
                what: &what,
            },
        )?;
        if p.judgments.is_empty() {
            bail!("name the proposals to reject: a person's row's `judgments`");
        }
        let (mut by, _, _) = self.listed_now()?;
        let note = p.note.as_deref().unwrap_or("rejected in bulk");
        let mut out = OntologyProposalRejectAllResult::default();
        for j in &p.judgments {
            let Some((_, pr)) = by.remove(j) else {
                out.left.push(not_listed(j));
                continue;
            };
            match self.reject_open(pr, Some(note), &who) {
                Ok(_) => out.rejected.push(j.clone()),
                Err(e) => out.left.push(format!("{j}: {e:#}")),
            }
        }
        Ok(out)
    }
}

/// Why a named proposal was not taken.
fn not_listed(j: &str) -> String {
    format!(
        "{j}: not listed now: answered already, hidden by the exclusions, or no such proposal \
         (`theseus ontology proposals` lists them)"
    )
}
