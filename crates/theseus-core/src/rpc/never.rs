//! "Never propose this" (theseus-648f8). The config is the owner's file and
//! nothing in the daemon writes it, so the cockpit's Never button keeps its
//! choice in the store: one META record a name, under
//! `ontology.never.<kind>.<name folded>`, with who and when, and no store
//! format of its own (a META key is a keyed record). Three kinds:
//!
//! - `topic`: a topic's name (a proposal of it is hidden, and Jev is not
//!   offered it); the config's `[ontology] not_topics` is the same set,
//!   kept in the file;
//! - `person`: a person's name, which joins `Core::not_people` as
//!   `[people] not_people` does;
//! - `pair`: two people who are not one (theseus-jrffh), its name the two
//!   ids as `same::pair_key` joins them; `ontology.person.not_same` writes
//!   it and the automatic merge and the proposals read it;
//! - `session`: for a new topic that has no name yet, the session itself:
//!   never propose a new topic for it.
//!
//! Undo is a new record, the same one marked `undone_ms`: the record only
//! grows, and a name never'd again is a fresh record. A hidden proposal is
//! counted in `ontology.proposals`' `hidden`.

use std::collections::HashSet;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use theseus_protocol::{
    method, OntologyNever, OntologyNeverListParams, OntologyNeverListResult,
    OntologyNeverUndoParams, OntologyNeverUndoResult, OntologyPersonNotSame,
    OntologyPersonNotSameListParams, OntologyPersonNotSameListResult, OntologyPersonNotSameParams,
    OntologyProposalNeverParams, OntologyProposalNeverResult,
};
use theseus_store::{kinds, NewRecord, Store as _};

use super::confirms::Act;
use super::Core;
use crate::approval::Answerer;
use crate::judge::people::fold;
use crate::judge::people::same::pair_key;

/// The META keys' prefix.
const PREFIX: &str = "ontology.never.";

/// One never, as stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Never {
    kind: String,
    /// As the owner saw it.
    name: String,
    who: String,
    via: String,
    at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    /// Set by the undo: the record stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    undone_ms: Option<u64>,
}

/// The wire's key for a never: `<kind>:<name folded>`.
fn wire_key(kind: &str, name: &str) -> String {
    match kind {
        "session" => format!("session:{}", name.trim()),
        "pair" => format!("pair:{}", name.trim()),
        _ => format!("{kind}:{}", fold(name)),
    }
}

fn meta_key(wire: &str) -> String {
    format!("{PREFIX}{}", wire.replacen(':', ".", 1))
}

fn info(n: &Never) -> OntologyNever {
    OntologyNever {
        key: wire_key(&n.kind, &n.name),
        kind: n.kind.clone(),
        name: n.name.clone(),
        who: n.who.clone(),
        at_ms: n.at_ms,
        note: n.note.clone(),
    }
}

fn pair_info(n: &Never, undone: bool) -> OntologyPersonNotSame {
    let (a, b) = n.name.split_once('|').unwrap_or((&n.name, ""));
    OntologyPersonNotSame {
        key: wire_key(&n.kind, &n.name),
        a: a.into(),
        b: b.into(),
        who: n.who.clone(),
        at_ms: n.at_ms,
        note: n.note.clone(),
        undone,
    }
}

/// What is never proposed: the config's `not_topics` and the stored topic
/// and session nevers (the people's join `Core::not_people`).
#[derive(Debug, Clone, Default)]
pub struct NeverSet {
    topics: HashSet<String>,
    sessions: HashSet<String>,
}

impl NeverSet {
    /// Whether a topic is never proposed, by its name or its local id.
    pub fn topic(&self, name: &str, local: &str) -> bool {
        self.topics.contains(&fold(name)) || self.topics.contains(&fold(local))
    }

    /// Whether a new topic is never proposed for the session.
    pub fn new_topic_in(&self, session: &str) -> bool {
        self.sessions.contains(session)
    }

    pub fn is_empty(&self) -> bool {
        self.topics.is_empty() && self.sessions.is_empty()
    }
}

impl Core {
    /// The stored nevers in force, newest first.
    fn nevers_stored(&self) -> Result<Vec<Never>> {
        let mut out = Vec::new();
        for r in self.store.inner().latest_with_prefix(kinds::META, PREFIX)? {
            if let Ok(n) = r.decode::<Never>() {
                if n.undone_ms.is_none() {
                    out.push(n);
                }
            }
        }
        out.sort_by_key(|n| std::cmp::Reverse(n.at_ms));
        Ok(out)
    }

    /// The effective never-set for topics: the config's plus the stored.
    pub(crate) fn never_set(&self) -> NeverSet {
        let mut set = NeverSet::default();
        set.topics
            .extend(self.runner.cfg.ontology.not_topics.iter().map(|t| fold(t)));
        for n in self.nevers_stored().unwrap_or_default() {
            match n.kind.as_str() {
                "topic" => {
                    set.topics.insert(fold(&n.name));
                }
                "session" => {
                    set.sessions.insert(n.name.trim().to_string());
                }
                _ => {}
            }
        }
        set.topics.remove("");
        set
    }

    /// The stored person nevers' names, for `Core::not_people`.
    pub(crate) fn never_people(&self) -> Vec<String> {
        self.nevers_stored()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.kind == "person")
            .map(|n| n.name)
            .collect()
    }

    /// The people pairs recorded as not the same, as `pair_key` writes them.
    pub(crate) fn not_same_pairs(&self) -> HashSet<String> {
        self.nevers_stored()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.kind == "pair")
            .map(|n| n.name)
            .collect()
    }

    /// `ontology.person.not_same`: the pair of people recorded as not one
    /// (or, with `undo`, the record taken back), judged as the other
    /// ontology writes are. Both must be held people.
    pub fn ontology_person_not_same(
        &self,
        p: &OntologyPersonNotSameParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyPersonNotSame> {
        let who = who.into();
        let what = format!("{} and {} are not the same person", p.a.trim(), p.b.trim());
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PERSON_NOT_SAME,
                what: &what,
            },
        )?;
        let o = self.runner.ontology.snapshot(&self.store)?;
        let a = super::ontology::resolve(&o, theseus_ontology::person::KIND, &p.a)?;
        let b = super::ontology::resolve(&o, theseus_ontology::person::KIND, &p.b)?;
        for id in [&a, &b] {
            if o.category(id)
                .is_none_or(|c| c.kind() != theseus_ontology::person::KIND)
            {
                bail!("{id} is no held person: `theseus ontology categories` lists them");
            }
        }
        if a == b {
            bail!("{a} is one person: a \"not the same\" takes two");
        }
        let name = pair_key(&a.to_string(), &b.to_string());
        let wire = wire_key("pair", &name);
        let meta = meta_key(&wire);
        if p.undo {
            let mut never = self
                .store
                .get_meta::<Never>(&meta)?
                .filter(|n| n.undone_ms.is_none())
                .ok_or_else(|| anyhow!("{a} and {b} are not recorded as different people"))?;
            never.undone_ms = Some(theseus_protocol::now_unix_ms());
            self.store.put_meta(&meta, &never)?;
            return Ok(pair_info(&never, true));
        }
        let never = Never {
            kind: "pair".into(),
            name,
            who: who.who(),
            via: who.via(),
            at_ms: theseus_protocol::now_unix_ms(),
            note: p.note.clone(),
            undone_ms: None,
        };
        self.store.put_meta(&meta, &never)?;
        Ok(pair_info(&never, false))
    }

    /// `ontology.person.not_same_list`: a read.
    pub fn ontology_person_not_same_list(
        &self,
        _: &OntologyPersonNotSameListParams,
    ) -> Result<OntologyPersonNotSameListResult> {
        Ok(OntologyPersonNotSameListResult {
            pairs: self
                .nevers_stored()?
                .iter()
                .filter(|n| n.kind == "pair")
                .map(|n| pair_info(n, false))
                .collect(),
        })
    }

    /// What a listed proposal asks never to be proposed: a person's name, a
    /// new topic's session, else the topic's name.
    fn never_of(l: &theseus_protocol::OntologyProposal) -> (&'static str, String) {
        if let Some(p) = &l.person {
            return ("person", p.name.clone());
        }
        if l.new_topic {
            return ("session", l.session_id.clone());
        }
        let name = l.topic_name.clone().unwrap_or_else(|| {
            let t = l.topic.clone().unwrap_or_default();
            t.split_once(':').map_or(t.clone(), |(_, n)| n.to_string())
        });
        ("topic", name)
    }

    /// `ontology.proposal.never`: each named proposal listed now rejected,
    /// and what it proposes kept as a never, in one frame each, judged once.
    pub fn ontology_proposal_never(
        &self,
        p: &OntologyProposalNeverParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyProposalNeverResult> {
        let who = who.into();
        let what = format!("never: {} proposals", p.judgments.len());
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_PROPOSAL_NEVER,
                what: &what,
            },
        )?;
        if p.judgments.is_empty() {
            bail!("name the proposals to never propose again: a row's `judgments`");
        }
        let (mut by, _, _) = self.listed_now()?;
        let note = p.note.as_deref().unwrap_or("never propose this");
        let mut out = OntologyProposalNeverResult::default();
        let mut kept: HashSet<String> = HashSet::new();
        for j in &p.judgments {
            let Some((l, pr)) = by.remove(j) else {
                out.left.push(super::people::not_listed(j));
                continue;
            };
            let (kind, name) = Self::never_of(&l);
            let never = Never {
                kind: kind.into(),
                name,
                who: who.who(),
                via: who.via(),
                at_ms: theseus_protocol::now_unix_ms(),
                note: p.note.clone(),
                undone_ms: None,
            };
            let wire = wire_key(kind, &never.name);
            let beside = match kept.insert(wire.clone()) {
                true => Some(NewRecord::json(
                    kinds::META,
                    Some(&meta_key(&wire)),
                    &never,
                )?),
                false => None,
            };
            match self.reject_with(pr, Some(note), &who, beside) {
                Ok(_) => {
                    out.rejected.push(j.clone());
                    if kept.contains(&wire) && !out.nevers.iter().any(|n| n.key == wire) {
                        out.nevers.push(info(&never));
                    }
                }
                Err(e) => {
                    kept.remove(&wire);
                    out.left.push(format!("{j}: {e:#}"));
                }
            }
        }
        Ok(out)
    }

    /// `ontology.never.list`: a read.
    pub fn ontology_never_list(
        &self,
        _: &OntologyNeverListParams,
    ) -> Result<OntologyNeverListResult> {
        Ok(OntologyNeverListResult {
            nevers: self.nevers_stored()?.iter().map(info).collect(),
            config_topics: self.runner.cfg.ontology.not_topics.clone(),
            config_people: self.runner.cfg.people.not_people.clone(),
        })
    }

    /// `ontology.never.undo`: the never taken back, judged as the other
    /// ontology writes are.
    pub fn ontology_never_undo(
        &self,
        p: &OntologyNeverUndoParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyNeverUndoResult> {
        let who = who.into();
        let key = p.key.trim();
        let what = format!("undo never {key}");
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_NEVER_UNDO,
                what: &what,
            },
        )?;
        let (kind, name) = key
            .split_once(':')
            .filter(|(k, _)| matches!(*k, "topic" | "person" | "session" | "pair"))
            .ok_or_else(|| {
                anyhow!("{key:?} is not a never's key: `theseus ontology never` lists them")
            })?;
        let wire = wire_key(kind, name);
        let meta = meta_key(&wire);
        let mut never = self
            .store
            .get_meta::<Never>(&meta)?
            .filter(|n| n.undone_ms.is_none())
            .ok_or_else(|| {
                anyhow!("no never {key:?} is in force: `theseus ontology never` lists them")
            })?;
        never.undone_ms = Some(theseus_protocol::now_unix_ms());
        self.store.put_meta(&meta, &never)?;
        Ok(OntologyNeverUndoResult {
            undone: info(&never),
        })
    }
}
