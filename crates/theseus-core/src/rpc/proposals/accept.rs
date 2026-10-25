//! A proposal accepted (`ontology.proposal.accept`, and each of
//! `accept_all`'s). An accept is planned against an ontology snapshot
//! ([`Core::plan_accept`]: the records it writes, the rows that describe them,
//! what it announces once they are written), so one proposal writes its own
//! frame (`accept_open`) and a whole `accept_all` writes one frame for all of
//! them (`finish_bulk`): one clone of the ontology, one append, one fsync, the
//! same records, rows and announcements each accept always made (theseus-6o8q8).

use anyhow::{anyhow, bail, Result};
use theseus_ontology::{Category, MemberList, Membership, Ontology, Origin, Record};
use theseus_protocol::{
    OntologyProposal, OntologyProposalAcceptAllResult, OntologyProposalAcceptParams,
    OntologyProposalAnswered,
};
use theseus_store::NewRecord;

use super::{answered, topic_of, Proposed};
use crate::approval::Answerer;
use crate::fact;
use crate::rpc::Core;

/// What an accept writes and says, planned and not yet written.
struct Planned {
    records: Vec<Record>,
    rows: Vec<NewRecord>,
    label_id: String,
    topic: String,
    /// The accept's facts, said once its frame is written.
    said: Said,
}

/// The proposals of a bulk yes, taken in order and written together; the
/// result is what `accept_all` answers (it reads through to `accepted` and
/// `left`).
#[derive(Default)]
pub(in crate::rpc) struct Bulk {
    out: OntologyProposalAcceptAllResult,
    items: Vec<(String, Proposed, OntologyProposalAcceptParams)>,
}

impl std::ops::Deref for Bulk {
    type Target = OntologyProposalAcceptAllResult;
    fn deref(&self) -> &Self::Target {
        &self.out
    }
}

impl std::ops::DerefMut for Bulk {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.out
    }
}

impl Bulk {
    /// One proposal of a bulk yes, judged already: taken, or left with why.
    pub(in crate::rpc) fn take(
        &mut self,
        l: &OntologyProposal,
        pr: Proposed,
        as_person: Option<&str>,
    ) {
        // A new person's proposal names its person: it needs no name.
        if l.new_topic || (l.topic.is_none() && l.person.is_none()) {
            self.out.left.push(format!(
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
        self.items.push((l.judgment.clone(), pr, one));
    }
}

impl Core {
    /// Write a bulk yes: every proposal planned in order against the
    /// ontology as the ones before it leave it, their records and rows in
    /// one frame, then each one's announcements. One that cannot be planned
    /// is left with why, and the rest go on.
    pub(in crate::rpc) fn finish_bulk(
        &self,
        bulk: Bulk,
        who: &Answerer,
    ) -> Result<OntologyProposalAcceptAllResult> {
        let Bulk { mut out, items } = bulk;
        if items.is_empty() {
            return Ok(out);
        }
        let mut taken: Vec<(String, Proposed, Planned)> = Vec::new();
        let mut left: Vec<String> = Vec::new();
        let staged = self
            .runner
            .ontology
            .stage(&self.store, Origin::Operator, |now| {
                let mut working = now.clone();
                let (mut records, mut rows) = (Vec::new(), Vec::new());
                for (j, pr, p) in items {
                    let planned = self
                        .plan_accept(&working, &pr, &p, who)
                        .and_then(|pl| apply(&mut working, &pl.records).map(|()| pl));
                    match planned {
                        Ok(mut pl) => {
                            records.extend(pl.records.iter().cloned());
                            rows.append(&mut pl.rows);
                            taken.push((j, pr, pl));
                        }
                        Err(e) => left.push(format!("{j}: {e:#}")),
                    }
                }
                Ok((records, rows))
            })?;
        if let Some(s) = staged {
            self.store.append(s.frame())?;
            s.commit();
        }
        for (j, pr, pl) in taken {
            pl.said.say(self, &pr);
            out.accepted.push(j);
        }
        out.left.extend(left);
        Ok(out)
    }

    /// An unanswered proposal accepted, as `ontology.proposal.accept` takes
    /// it once judged, in a frame of its own.
    pub(in crate::rpc) fn accept_open(
        &self,
        pr: Proposed,
        p: &OntologyProposalAcceptParams,
        who: &Answerer,
    ) -> Result<OntologyProposalAnswered> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let Planned {
            records,
            rows,
            label_id,
            topic,
            said,
        } = self.plan_accept(&o, &pr, p, who)?;
        let o = match records.is_empty() {
            true => {
                self.store.append(&rows)?;
                o
            }
            false => self
                .runner
                .ontology
                .write(&self.store, records, Origin::Operator, |_| Ok(rows))?,
        };
        said.say(self, &pr);
        Ok(answered(&o, &pr, label_id, "accepted", Some(topic)))
    }

    /// The accept of `pr` against `o`: its records (a made category, the
    /// session's list with the topic joined), its rows, its announcements.
    /// Writes nothing.
    fn plan_accept(
        &self,
        o: &Ontology,
        pr: &Proposed,
        p: &OntologyProposalAcceptParams,
        who: &Answerer,
    ) -> Result<Planned> {
        let sid = pr.session.as_str();
        if self
            .store
            .get_session::<crate::session::SessionRecord>(sid)?
            .is_none()
        {
            bail!("the session {sid} is gone");
        }
        let (who_s, via) = (who.who(), who.via());
        let (id, made) = topic_of(o, pr, p, who)?;
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
        let pack = pr.judgment.pack.clone();
        let note = p.note.clone();
        let mut records: Vec<Record> = made.iter().cloned().map(Record::Category).collect();
        if joins {
            records.push(Record::Members(list.clone()));
        }
        let mut rows = Vec::new();
        if let Some(c) = &made {
            rows.push(fact::row(
                &fact::ontology::CategorySet {
                    category: c,
                    origin: Origin::Operator.name(),
                    who: &who_s,
                    via: &via,
                    undo: None,
                },
                None,
                None,
            )?);
        }
        if joins {
            rows.push(fact::row(
                &fact::ontology::MembershipSet {
                    list: &list,
                    who: &who_s,
                    via: &via,
                    undo: None,
                },
                Some(sid),
                None,
            )?);
        }
        let mut label_row = fact::row(
            &fact::judge::ProposalLabel {
                id: &label_id,
                judgment: &pr.judgment.id,
                pack: &pack,
                question: pr.question(),
                label: "accepted",
                answer: &pr.choice,
                topic: Some(&topic),
                who: &who_s,
                via: &via,
                note: note.as_deref(),
            },
            Some(sid),
            None,
        )?;
        label_row.key = Some(label_id.clone());
        rows.push(label_row.scoped(pr.scope()));
        let said = Said {
            made,
            list: joins.then_some(list),
            sid: sid.to_string(),
            who: who_s,
            via,
            label_id: label_id.clone(),
            topic: topic.clone(),
            pack,
            note,
        };
        Ok(Planned {
            records,
            rows,
            label_id,
            topic,
            said,
        })
    }
}

/// What an accept announces, owned, so it can be said once its frame is
/// written.
struct Said {
    made: Option<Category>,
    list: Option<MemberList>,
    sid: String,
    who: String,
    via: String,
    label_id: String,
    topic: String,
    pack: String,
    note: Option<String>,
}

impl Said {
    fn say(self, core: &Core, pr: &Proposed) {
        if let Some(c) = &self.made {
            core.rec(None).announce(&fact::ontology::CategorySet {
                category: c,
                origin: Origin::Operator.name(),
                who: &self.who,
                via: &self.via,
                undo: None,
            });
        }
        if let Some(list) = &self.list {
            core.rec(Some(&self.sid))
                .announce(&fact::ontology::MembershipSet {
                    list,
                    who: &self.who,
                    via: &self.via,
                    undo: None,
                });
        }
        core.rec(Some(&self.sid))
            .announce(&fact::judge::ProposalLabel {
                id: &self.label_id,
                judgment: &pr.judgment.id,
                pack: &self.pack,
                question: pr.question(),
                label: "accepted",
                answer: &pr.choice,
                topic: Some(&self.topic),
                who: &self.who,
                via: &self.via,
                note: self.note.as_deref(),
            });
    }
}

/// `records` put into `o` as the ontology's write would, none of them if one
/// does not check.
fn apply(o: &mut Ontology, records: &[Record]) -> Result<()> {
    if records.len() < 2 {
        for r in records {
            o.put(r.clone(), Origin::Operator)
                .map_err(anyhow::Error::new)?;
        }
        return Ok(());
    }
    let mut trial = o.clone();
    for r in records {
        trial
            .put(r.clone(), Origin::Operator)
            .map_err(|e| anyhow!(e))?;
    }
    *o = trial;
    Ok(())
}
