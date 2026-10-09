//! The fungible ontology's first slice (spec §2 FUNGIBLE ONTOLOGY, §4.1a;
//! M4 design §2.8; theseus-8kk).
//!
//! Which kinds of context exist is data: a versioned **kinds table** whose
//! rows say where a kind's memberships come from, how many a session may
//! hold, its parent kind, its precedence, and its composition rule. M4 seeds
//! four rows: guild, channel, and person (given by the transport), and topic
//! (interpreted, declared by the operator). Instances of a kind are
//! **categories**, in a tree; a category may carry **guidance**; a session's
//! **memberships** place it in categories. A compile composes the guidance
//! of the categories a session is in ([`Ontology::compose`]), and its
//! manifest records what it used.
//!
//! The two guardrails the owner confirmed (2026-09-27) are in this crate's code:
//! - *Given versus interpreted*: a given kind's memberships are facts read
//!   from the session's place. They are never stored, and setting one is an
//!   input error; only interpreted kinds take API writes.
//! - *Interpretations route context and never grant access*: this crate
//!   depends on no labels and no policy. What it yields is guidance text for
//!   the system block, and nothing a gate reads.
//!
//! The crate is pure: types, validation, and composition. The core keeps
//! the records as META keys ([`keys`]), builds the snapshot after serving,
//! judges writes, and runs the compile walk (21b).

pub mod category;
pub mod compose;
pub mod kind;
pub mod ontology;
pub mod person;
pub mod refusal;
mod text;

pub use category::{Category, CategoryId, Guidance, MemberList, Membership};
pub use compose::{Composition, GuidanceUsed, MembershipUsed, Skipped, PATH_SEP, PREAMBLE};
pub use kind::{seeds, Basis, Kind, Origin, PerSession, Rule, GIVEN, SEED};
pub use ontology::{Ontology, MAX_DEPTH};
pub use person::{handle, handles_of, Merge};
pub use refusal::Refusal;
pub use text::{ADDED_BY_MAX, DESCRIPTION_MAX, GUIDANCE_MAX, INTENT_LINE_MAX, NAME_MAX};

/// The META keys the records live at (M4 design §2.8's data shapes). Every
/// key starts with [`keys::PREFIX`], so one scan finds them all.
pub mod keys {
    use crate::CategoryId;

    pub const PREFIX: &str = "onto:";
    pub const KIND: &str = "onto:kind:";
    pub const CATEGORY: &str = "onto:cat:";
    pub const GUIDANCE: &str = "onto:guide:";
    pub const MEMBERS: &str = "onto:member:";

    /// `onto:kind:<name>`
    pub fn kind(name: &str) -> String {
        format!("{KIND}{name}")
    }

    /// `onto:cat:<id>`
    pub fn category(id: &CategoryId) -> String {
        format!("{CATEGORY}{id}")
    }

    /// `onto:guide:<category>`
    pub fn guidance(id: &CategoryId) -> String {
        format!("{GUIDANCE}{id}")
    }

    /// `onto:member:<session>:<kind>`
    pub fn members(session: &str, kind: &str) -> String {
        format!("{MEMBERS}{session}:{kind}")
    }
}

/// One `onto:*` record, as the store holds it.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Kind(Kind),
    Category(Category),
    Guidance(Guidance),
    Members(MemberList),
}

impl Record {
    /// The META key the record lives at.
    pub fn key(&self) -> String {
        match self {
            Record::Kind(k) => keys::kind(&k.name),
            Record::Category(c) => keys::category(&c.id),
            Record::Guidance(g) => keys::guidance(&g.category),
            Record::Members(l) => keys::members(&l.session, &l.kind),
        }
    }

    /// The ledger row a change of it writes (M4 design §2.12).
    pub fn ledger_kind(&self) -> &'static str {
        match self {
            Record::Kind(_) => "ontology.kind",
            Record::Category(_) => "ontology.category",
            Record::Guidance(_) => "ontology.guidance",
            Record::Members(_) => "ontology.membership",
        }
    }

    /// The record's META value.
    pub fn to_value(&self) -> serde_json::Value {
        let v = match self {
            Record::Kind(k) => serde_json::to_value(k),
            Record::Category(c) => serde_json::to_value(c),
            Record::Guidance(g) => serde_json::to_value(g),
            Record::Members(l) => serde_json::to_value(l),
        };
        v.expect("the ontology's records serialize")
    }

    /// The record a META key and value hold. The value must be the key's:
    /// a kind row at another kind's key is refused.
    pub fn decode(key: &str, value: serde_json::Value) -> Result<Record, Refusal> {
        fn parse<T: serde::de::DeserializeOwned>(
            key: &str,
            value: serde_json::Value,
        ) -> Result<T, Refusal> {
            serde_json::from_value(value)
                .map_err(|e| Refusal::invalid(format!("`{key}`"), e.to_string()))
        }
        let record = if key.starts_with(keys::KIND) {
            Record::Kind(parse(key, value)?)
        } else if key.starts_with(keys::CATEGORY) {
            Record::Category(parse(key, value)?)
        } else if key.starts_with(keys::GUIDANCE) {
            Record::Guidance(parse(key, value)?)
        } else if key.starts_with(keys::MEMBERS) {
            Record::Members(parse(key, value)?)
        } else {
            return Err(Refusal::invalid(
                format!("`{key}`"),
                "it is not an ontology key",
            ));
        };
        if record.key() != key {
            return Err(Refusal::invalid(
                format!("`{key}`"),
                format!("it holds the record of `{}`", record.key()),
            ));
        }
        Ok(record)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_people;
