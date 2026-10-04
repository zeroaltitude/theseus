//! `categorize.v1`'s proposals (M5 step 28b; design §2.12): the read,
//! `ontology.proposals`, and the operator's two answers. Jev writes no
//! membership in M5. An accept makes the `operator`-origin membership
//! through the ontology's write path, with the judgment's `judge.label` row
//! in the same frame (and, for `new_topic`, the topic the operator names); a
//! reject writes the label alone. Both are judged as the ontology's writes
//! are (`judge_act(Act::Ontology)`: the owner, from a private place), before
//! anything is read, so a refused answer writes neither.
//!
//! The judgments and their labels share one scope, `judge:categorize`, so
//! the read is one scan of it; a label is a row keyed by its own id
//! (`lbl_…`), and a judgment with an operator's label is answered.

use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, bail, Result};
use theseus_judge::{Answer, Judgment, Outcome};
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Ontology, Origin, Record};
use theseus_protocol::{
    method, OntologyProposal, OntologyProposalAcceptParams, OntologyProposalAnswered,
    OntologyProposalRejectParams, OntologyProposalsParams, OntologyProposalsResult,
};
use theseus_store::{kinds, NewRecord, Store as _};

use super::confirms::Act;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::judge::categorize::PACK;
use crate::ledger::LedgerRow;

/// The pack's scope: its judgments and their labels.
pub const SCOPE: &str = "judge:categorize";
/// The read's default limit.
const LIMIT: u32 = 50;

/// A judgment's `topic` answer, as a proposal.
struct Proposed {
    judgment: Judgment,
    session: String,
    /// `new_topic`, or the option's id (a topic's local id).
    choice: String,
    confidence: f64,
    band: String,
    at_ms: u64,
}

impl Proposed {
    /// The answered `topic` of a `judge.call` row; none for `none`, an
    /// unanswered judgment, or another pack's.
    fn of(row: &LedgerRow) -> Option<Proposed> {
        let judgment: Judgment = serde_json::from_value(row.data.clone()).ok()?;
        if judgment.pack != PACK || judgment.outcome != Outcome::Answered {
            return None;
        }
        let a = judgment.answers.iter().find(|a| a.question == "topic")?;
        let Answer::Choice {
            choice, confidence, ..
        } = &a.answer
        else {
            return None;
        };
        if choice == "none" {
            return None;
        }
        let band = serde_json::to_value(a.band.band)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        Some(Proposed {
            session: row.session_id.clone()?,
            choice: choice.clone(),
            confidence: *confidence,
            band,
            at_ms: row.at_unix_ms,
            judgment,
        })
    }

    fn new_topic(&self) -> bool {
        self.choice == "new_topic"
    }

    /// The category the choice names, among the kinds the operator assigns
    /// (the candidates' kinds); `topic:<choice>` when none holds it now.
    fn category(&self, o: &Ontology) -> Option<CategoryId> {
        if self.new_topic() {
            return None;
        }
        o.categories()
            .find(|c| {
                c.id.local() == self.choice
                    && o.kind(c.kind())
                        .is_some_and(|k| !k.is_given() && k.assigned_by.contains(&Origin::Operator))
            })
            .map(|c| c.id.clone())
            .or_else(|| CategoryId::new("topic", &self.choice).ok())
    }
}

impl Core {
    /// The scope's rows: the proposals, oldest first, and the judgments an
    /// operator has labelled.
    fn proposals_scan(&self) -> Result<(Vec<Proposed>, HashSet<String>)> {
        let mut proposed = Vec::new();
        let mut labelled = HashSet::new();
        for r in self.store.scope_after(SCOPE, 0)? {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            match row.kind.as_str() {
                "judge.label" => {
                    if row.data["source"] == "operator" {
                        if let Some(j) = row.data["judgment"].as_str() {
                            labelled.insert(j.to_string());
                        }
                    }
                }
                "judge.call" => proposed.extend(Proposed::of(&row)),
                _ => {}
            }
        }
        Ok((proposed, labelled))
    }

    /// `ontology.proposals`: each answered `categorize.v1` judgment whose
    /// topic names a topic its session is not in now, or `new_topic`, that
    /// no operator has labelled, newest first.
    pub fn ontology_proposals(
        &self,
        p: &OntologyProposalsParams,
    ) -> Result<OntologyProposalsResult> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let (proposed, labelled) = self.proposals_scan()?;
        let mut titles: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut out = Vec::new();
        for pr in proposed.into_iter().rev() {
            if labelled.contains(&pr.judgment.id)
                || p.session_id.as_ref().is_some_and(|s| *s != pr.session)
            {
                continue;
            }
            let topic = pr.category(&o);
            if let Some(t) = &topic {
                if o.memberships(&pr.session).iter().any(|m| m.category == *t) {
                    continue;
                }
            }
            let title = titles
                .entry(pr.session.clone())
                .or_insert_with(|| self.session_title(&pr.session))
                .clone();
            out.push(OntologyProposal {
                judgment: pr.judgment.id.clone(),
                session_id: pr.session.clone(),
                session_title: title,
                topic_name: topic
                    .as_ref()
                    .and_then(|t| o.category(t))
                    .map(|c| c.name.clone()),
                topic: topic.map(|t| t.to_string()),
                new_topic: pr.new_topic(),
                confidence: pr.confidence,
                band: pr.band,
                at_ms: pr.at_ms,
            });
        }
        let limit = p.limit.unwrap_or(LIMIT) as usize;
        let more = out.len().saturating_sub(limit) as u32;
        out.truncate(limit);
        Ok(OntologyProposalsResult {
            proposals: out,
            more,
        })
    }

    fn session_title(&self, sid: &str) -> Option<String> {
        self.store
            .get_session::<crate::session::SessionRecord>(sid)
            .ok()
            .flatten()
            .and_then(|s| s.title)
    }

    /// The proposal a judgment id names, unanswered.
    fn open_proposal(&self, id: &str) -> Result<Proposed> {
        let id = id.trim();
        let rec = self
            .store
            .inner()
            .latest_by_key(kinds::LEDGER, id)?
            .ok_or_else(|| {
                anyhow!("no judgment is named {id:?}: `theseus ontology proposals` lists them")
            })?;
        let row: LedgerRow = rec.decode()?;
        let pr = Proposed::of(&row)
            .ok_or_else(|| anyhow!("{id} is not a categorize.v1 judgment that proposes a topic"))?;
        if self.proposals_scan()?.1.contains(id) {
            bail!("{id} is answered already: its label is in the ledger (`theseus ledger -k judge.label`)");
        }
        Ok(pr)
    }

    /// `ontology.proposal.accept`: the session joins the proposed topic (for
    /// `new_topic`, the one `topic` names, made now when no topic has that
    /// name), with the judgment's label, in one frame.
    pub fn ontology_proposal_accept(
        &self,
        p: &OntologyProposalAcceptParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyProposalAnswered> {
        let who = who.into();
        let what = format!("Jev's proposal {}", p.judgment.trim());
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PROPOSAL_ACCEPT,
                what: &what,
            },
        )?;
        let pr = self.open_proposal(&p.judgment)?;
        let sid = pr.session.as_str();
        if self
            .store
            .get_session::<crate::session::SessionRecord>(sid)?
            .is_none()
        {
            bail!("the session {sid} is gone");
        }
        let o = self.runner.ontology.snapshot(&self.store)?;
        let (who_s, via) = (who.who(), who.via());
        let (id, made) = topic_of(&o, &pr, p, &who)?;
        let mut members = o
            .member_list(sid, id.kind())
            .map(|l| l.members.clone())
            .unwrap_or_default();
        let joins = !members.iter().any(|m| m.category == id);
        if joins {
            members.push(Membership::operator(
                id.clone(),
                theseus_protocol::now_unix_ms(),
            ));
        }
        let list = MemberList {
            session: sid.to_string(),
            kind: id.kind().to_string(),
            members,
        };
        let topic = id.to_string();
        let label_id = format!("lbl_{}", uuid::Uuid::now_v7().simple());
        let label = fact::judge::JudgeLabel {
            id: &label_id,
            judgment: &pr.judgment.id,
            pack: PACK,
            question: "topic",
            label: "accepted",
            answer: &pr.choice,
            topic: Some(&topic),
            who: &who_s,
            via: &via,
            note: p.note.as_deref(),
        };
        let set = made.as_ref().map(|category| fact::ontology::CategorySet {
            category,
            origin: Origin::Operator.name(),
            who: &who_s,
            via: &via,
        });
        let member = fact::ontology::MembershipSet {
            list: &list,
            who: &who_s,
            via: &via,
        };
        let label_row = || -> Result<NewRecord> {
            let mut r = fact::row(&label, Some(sid), None)?;
            r.key = Some(label_id.clone());
            Ok(r.scoped(SCOPE))
        };
        let mut records: Vec<Record> = made.iter().cloned().map(Record::Category).collect();
        if joins {
            records.push(Record::Members(list.clone()));
        }
        let o = match records.is_empty() {
            true => {
                self.store.append(&[label_row()?])?;
                o
            }
            false => self
                .runner
                .ontology
                .write(&self.store, records, Origin::Operator, |_| {
                    let mut rows = Vec::new();
                    if let Some(s) = &set {
                        rows.push(fact::row(s, None, None)?);
                    }
                    if joins {
                        rows.push(fact::row(&member, Some(sid), None)?);
                    }
                    rows.push(label_row()?);
                    Ok(rows)
                })?,
        };
        if let Some(s) = &set {
            self.rec(None).announce(s);
        }
        if joins {
            self.rec(Some(sid)).announce(&member);
        }
        self.rec(Some(sid)).announce(&label);
        Ok(answered(&o, &pr, label_id, "accepted", Some(topic)))
    }

    /// `ontology.proposal.reject`: the judgment's label, and nothing else.
    pub fn ontology_proposal_reject(
        &self,
        p: &OntologyProposalRejectParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyProposalAnswered> {
        let who = who.into();
        let what = format!("Jev's proposal {}", p.judgment.trim());
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PROPOSAL_REJECT,
                what: &what,
            },
        )?;
        let pr = self.open_proposal(&p.judgment)?;
        let sid = pr.session.as_str();
        let (who_s, via) = (who.who(), who.via());
        let label_id = format!("lbl_{}", uuid::Uuid::now_v7().simple());
        let label = fact::judge::JudgeLabel {
            id: &label_id,
            judgment: &pr.judgment.id,
            pack: PACK,
            question: "topic",
            label: "rejected",
            answer: &pr.choice,
            topic: None,
            who: &who_s,
            via: &via,
            note: p.note.as_deref(),
        };
        let mut row = fact::row(&label, Some(sid), None)?;
        row.key = Some(label_id.clone());
        self.store.append(&[row.scoped(SCOPE)])?;
        self.rec(Some(sid)).announce(&label);
        let o = self.runner.ontology.snapshot(&self.store)?;
        Ok(answered(&o, &pr, label_id, "rejected", None))
    }
}

/// The topic an accept joins, and the topic it makes: the proposal's own;
/// or, for `new_topic`, the one `topic` names, made now (with
/// `description`) when no topic has that name.
fn topic_of(
    o: &Ontology,
    pr: &Proposed,
    p: &OntologyProposalAcceptParams,
    who: &Answerer,
) -> Result<(CategoryId, Option<Category>)> {
    Ok(match (pr.new_topic(), p.topic.as_deref().map(str::trim)) {
        (false, None) => (
            pr.category(o)
                .filter(|t| o.category(t).is_some())
                .ok_or_else(|| anyhow!("the topic {} it proposed is gone", pr.choice))?,
            None,
        ),
        (false, Some(_)) => bail!(
            "{} proposes {} already; --topic names the topic of a new_topic proposal",
            pr.judgment.id,
            pr.choice
        ),
        (true, None | Some("")) => bail!(
            "{} proposes a new topic: name it with --topic (an existing topic, or a new one \
             with --desc)",
            pr.judgment.id
        ),
        (true, Some(name)) => match super::ontology::resolve(o, "topic", name) {
            Ok(id) => (id, None),
            Err(_) => {
                let c = Category {
                    id: o.mint_id("topic", name)?,
                    name: name.to_string(),
                    parent: None,
                    description: p.description.clone().unwrap_or_default().trim().to_string(),
                    added_by: super::ontology::added_by(who),
                };
                (c.id.clone(), Some(c))
            }
        },
    })
}

fn answered(
    o: &Ontology,
    pr: &Proposed,
    label_id: String,
    label: &str,
    topic: Option<String>,
) -> OntologyProposalAnswered {
    OntologyProposalAnswered {
        judgment: pr.judgment.id.clone(),
        session_id: pr.session.clone(),
        label_id,
        label: label.into(),
        topic,
        memberships: o
            .memberships(&pr.session)
            .iter()
            .map(|m| super::ontology::membership_info(&pr.session, m))
            .collect(),
    }
}
