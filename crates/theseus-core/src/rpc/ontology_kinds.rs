//! The ontology's three new writes (theseus-xzxqv): `ontology.undo`, which
//! takes back what a machine origin wrote, and `ontology.kind.add` and
//! `.retire`, the operator's two writes to the kinds table. Each is the
//! operator's act (`judge_act(Act::Ontology)`: the owner, from a private
//! place), checked against the snapshot, written in one frame with its row.

use anyhow::{anyhow, bail, Result};
use serde::de::DeserializeOwned;
use serde_json::json;
use theseus_ontology::{Basis, Kind, MinedFrom, Origin, PerSession, Record, Rule};
use theseus_protocol::{
    method, now_unix_ms, OntologyKind, OntologyKindAddParams, OntologyKindResult,
    OntologyKindRetireParams, OntologyUndoParams, OntologyUndoResult,
};

use super::confirms::Act;
use super::ontology::{added_by, kind_info};
use super::Core;
use crate::approval::Answerer;
use crate::fact;

/// The methods `rpc_prefixed` routes to the ontology's arm.
pub(super) const ROUTE: [&str; 9] = [
    method::ONTOLOGY_UNDO,
    method::ONTOLOGY_KIND_ADD,
    method::ONTOLOGY_KIND_RETIRE,
    // The nevers (theseus-648f8, `never.rs`).
    method::ONTOLOGY_PROPOSAL_NEVER,
    method::ONTOLOGY_NEVER_LIST,
    method::ONTOLOGY_NEVER_UNDO,
    // Two people that are not one (theseus-jrffh, `never.rs`).
    method::ONTOLOGY_PERSON_NOT_SAME,
    method::ONTOLOGY_PERSON_NOT_SAME_LIST,
    method::ONTOLOGY_PERSON_CANDIDATES,
];

/// A closed word of the wire (`operator`, `chain`, `titles`) as its type.
fn word<T: DeserializeOwned>(what: &str, w: &str) -> Result<T> {
    serde_json::from_value(json!(w.trim()))
        .map_err(|_| anyhow!("{w:?} is not a {what}: the CLI's help lists them"))
}

impl Core {
    /// `ontology.undo`: the machine write `p.write` taken back, as the
    /// operator's act: its records restored, a category it minted retired,
    /// and one `ontology.undo` row labelled `wrong` for the pack's ledger.
    pub fn ontology_undo_rpc(
        &self,
        p: &OntologyUndoParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyUndoResult> {
        let who = who.into();
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_UNDO,
                what: &format!("undo of {}", p.write),
            },
        )?;
        let keys = self.ontology_undo(p.write.trim(), &who.who(), &who.via())?;
        Ok(OntologyUndoResult {
            write: p.write.trim().to_string(),
            keys,
        })
    }

    /// `ontology.kind.add`: a new interpreted kind, the operator's row.
    pub fn ontology_kind_add(
        &self,
        p: &OntologyKindAddParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyKindResult> {
        let who = who.into();
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_KIND_ADD,
                what: &format!("kind {}", p.name),
            },
        )?;
        let o = self.runner.ontology.snapshot(&self.store)?;
        let name = p.name.trim().to_string();
        if o.kind(&name).is_some() {
            bail!("the kind `{name}` is in the table already: `theseus ontology kinds` lists them");
        }
        let assigned_by: Vec<Origin> = match p.assigned_by.is_empty() {
            true => vec![Origin::Operator, Origin::Jev],
            false => p
                .assigned_by
                .iter()
                .map(|w| word("origin", w))
                .collect::<Result<_>>()?,
        };
        let per_session = match p.per_session.as_deref().map(str::trim) {
            None => PerSession::AtMost(3),
            Some("many") => PerSession::Many,
            Some(n) => PerSession::AtMost(
                n.parse()
                    .map_err(|_| anyhow!("per_session is a number or \"many\", not {n:?}"))?,
            ),
        };
        let precedence = p
            .precedence
            .unwrap_or_else(|| o.kinds().iter().map(|k| k.precedence).max().unwrap_or(0) + 10);
        let kind = Kind {
            name,
            basis: Basis::Interpreted,
            assigned_by,
            per_session,
            parent: p.parent.clone().filter(|s| !s.trim().is_empty()),
            precedence,
            rule: match p.rule.as_deref() {
                None => Rule::Chain,
                Some(r) => word("rule", r)?,
            },
            description: p.description.trim().to_string(),
            version: 1,
            added_by: added_by(&who),
            retired_ms: None,
            examples: p.examples.iter().map(|e| e.trim().to_string()).collect(),
            mined_from: p
                .mined_from
                .iter()
                .map(|m| word::<MinedFrom>("source of names", m))
                .collect::<Result<_>>()?,
        };
        self.kind_write(kind, &who, false)
    }

    /// `ontology.kind.retire`: a kind nothing holds, taken away.
    pub fn ontology_kind_retire(
        &self,
        p: &OntologyKindRetireParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyKindResult> {
        let who = who.into();
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_KIND_RETIRE,
                what: &format!("retire of kind {}", p.name),
            },
        )?;
        let o = self.runner.ontology.snapshot(&self.store)?;
        let name = p.name.trim();
        let Some(old) = o.kind(name) else {
            bail!("no kind is named {name:?}: `theseus ontology kinds` lists them");
        };
        let kind = Kind {
            version: old.version + 1,
            added_by: added_by(&who),
            retired_ms: Some(now_unix_ms()),
            ..old.clone()
        };
        self.kind_write(kind, &who, true)
    }

    fn kind_write(&self, kind: Kind, who: &Answerer, retired: bool) -> Result<OntologyKindResult> {
        let (who_s, via) = (who.who(), who.via());
        let set = fact::ontology::KindSet {
            kind: &kind,
            retired,
            who: &who_s,
            via: &via,
            undo: None,
        };
        self.runner.ontology.write(
            &self.store,
            vec![Record::Kind(kind.clone())],
            Origin::Operator,
            |_| Ok(vec![fact::row(&set, None, None)?]),
        )?;
        self.rec(None).announce(&set);
        let kind: OntologyKind = kind_info(&kind);
        Ok(OntologyKindResult { kind, retired })
    }
}
