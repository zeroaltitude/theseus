//! The sweep mints a kind (theseus-81zx7): when three sessions say `other`
//! to `kind_of_thing` about one mined name, none of the table's kinds
//! describes what they talk about, and the sweep writes a kind for it, one
//! category of it, and each session's membership.
//!
//! - **What is read**: the `other` steps of `onto.filed` rows from the last
//!   [`WINDOW_MS`] (seven days; the design says "the day's", and three
//!   sessions in a day is rare). A name is its fold (`sweep::fold`): `Release
//!   Notes` and `release-notes` are one.
//! - **A cluster** is the sessions that share a name: [`CLUSTER`] distinct
//!   sessions or more. Two do not mint. The most shared name leads; the names
//!   at least as many of its sessions share are its examples.
//! - **Undo** retires the kind with its category and drops the memberships
//!   (`Ontology::undo`). A name a kind of the table already lists in
//!   `examples` is not clustered again, an undone kind's included, so a
//!   repeat run writes nothing and an undo stays undone.
//! - **No card.** The sweep runs at night: its write is in the morning note
//!   (`digest.rs`), with the id `theseus ontology undo` takes.

use std::collections::{BTreeMap, BTreeSet};

use theseus_ontology::sweep::fold;
use theseus_ontology::{
    Basis, Category, Kind, MemberList, Membership, Ontology, Origin, PerSession, Record, Rule,
    EXAMPLES_MAX,
};

use theseus_protocol::LedgerKind;

use super::sweep::Swept;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// How far back the `other` answers are read.
pub const WINDOW_MS: u64 = 7 * 24 * 3_600_000;
/// The sessions that make a cluster.
pub const CLUSTER: usize = 3;
/// The kinds one run mints at most.
pub const PER_RUN: usize = 3;

/// What the `other` steps of the window said: a session, a name, and the
/// most sure answer that named it.
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub session: String,
    pub names: Vec<String>,
    pub confidence: f64,
}

/// The `other` steps of `onto.filed` rows.
pub fn said_of(rows: &[LedgerRow]) -> Vec<Said> {
    let mut out = Vec::new();
    for row in rows.iter().filter(|r| r.kind == "onto.filed") {
        let Some(session) = row.session_id.clone() else {
            continue;
        };
        for step in row.data["steps"].as_array().into_iter().flatten() {
            if step["outcome"] != "other" {
                continue;
            }
            out.push(Said {
                session: session.clone(),
                names: step["names"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n.as_str().map(str::to_string))
                    .collect(),
                confidence: step["confidence"].as_f64().unwrap_or(0.0),
            });
        }
    }
    out
}

/// One kind to mint: its records in the order they write, and its sessions.
#[derive(Debug, Clone, PartialEq)]
pub struct Minted {
    /// The id of its write.
    pub write: String,
    pub kind: String,
    pub precedence: u32,
    pub records: Vec<Record>,
    pub sessions: usize,
}

/// What a run mints, and the clusters it could not (a name that is no kind's
/// name, or one the table holds).
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub minted: Vec<Minted>,
    pub refused: u64,
}

/// A name as a kind's name: lowercase, other runs `-`, at most 32 characters.
fn kind_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(32);
    out.trim_end_matches('-').to_string()
}

/// What `said` mints against `o`: `write(n)` is the id of the nth write
/// (`sweep-<ms>-<n>-kind`), which the kind's and category's `added_by` carry.
pub fn plan(o: &Ontology, said: &[Said], now_ms: u64, write: impl Fn(usize) -> String) -> Plan {
    // folded name -> (the name as first spelled, session -> most sure answer)
    let mut names: BTreeMap<String, (String, BTreeMap<&str, f64>)> = BTreeMap::new();
    for s in said {
        for n in &s.names {
            let f = fold(n);
            if f.is_empty() {
                continue;
            }
            let seen = names
                .entry(f)
                .or_insert_with(|| (n.clone(), BTreeMap::new()));
            let at = seen.1.entry(&s.session).or_default();
            *at = at.max(s.confidence);
        }
    }
    let mut listed: BTreeSet<String> = o
        .kinds()
        .iter()
        .flat_map(|k| k.examples.iter().map(|e| fold(e)))
        .collect();
    let mut precedence = o.kinds().iter().map(|k| k.precedence).max().unwrap_or(0);
    let mut working = o.clone();
    let mut plan = Plan::default();
    while plan.minted.len() < PER_RUN {
        let lead = names
            .iter()
            .filter(|(f, (_, sessions))| sessions.len() >= CLUSTER && !listed.contains(*f))
            .max_by_key(|(f, (_, sessions))| (sessions.len(), std::cmp::Reverse((*f).clone())));
        let Some((name, sessions)) = lead.map(|(_, v)| v.clone()) else {
            break;
        };
        // The names that at least `CLUSTER` of its sessions share, the lead
        // first, then by how many share them.
        let mut shared: Vec<(usize, &String)> = names
            .iter()
            .filter(|(f, _)| !listed.contains(*f))
            .map(|(_, (spelled, with))| {
                let n = with.keys().filter(|s| sessions.contains_key(*s)).count();
                (n, spelled)
            })
            .filter(|(n, spelled)| *n >= CLUSTER && fold(spelled) != fold(&name))
            .collect();
        shared.sort_by_key(|(n, spelled)| (std::cmp::Reverse(*n), spelled.to_ascii_lowercase()));
        let mut examples = vec![name.clone()];
        examples.extend(shared.into_iter().map(|(_, s)| s.clone()));
        examples.truncate(EXAMPLES_MAX);
        listed.extend(examples.iter().map(|e| fold(e)));
        let n = plan.minted.len() + plan.refused as usize;
        match mint(
            &mut working,
            &name,
            &examples,
            &sessions,
            now_ms,
            precedence,
            &write(n),
        ) {
            Some(m) => {
                precedence = m.precedence;
                plan.minted.push(m);
            }
            None => plan.refused += 1,
        }
    }
    plan
}

/// The kind, its one category and each session's membership, applied to
/// `working` when every record checks as the sweep's.
fn mint(
    working: &mut Ontology,
    name: &str,
    examples: &[String],
    sessions: &BTreeMap<&str, f64>,
    now_ms: u64,
    precedence: u32,
    write: &str,
) -> Option<Minted> {
    let by = format!("sweep:{write}");
    let kind = Kind {
        name: kind_name(name),
        basis: Basis::Interpreted,
        assigned_by: vec![Origin::Operator, Origin::Jev, Origin::Sweep],
        per_session: PerSession::AtMost(3),
        parent: None,
        precedence: (precedence / 10 + 1) * 10,
        rule: Rule::Chain,
        description: format!(
            "A kind the sweep made from {} sessions whose messages were about none of the listed \
             kinds: {}.",
            sessions.len(),
            examples.join(", ")
        ),
        version: 1,
        added_by: by.clone(),
        retired_ms: None,
        examples: examples.to_vec(),
        mined_from: Vec::new(),
    };
    if kind.check_row().is_err() || working.kind(&kind.name).is_some() {
        return None;
    }
    let mut trial = working.clone();
    trial.put(Record::Kind(kind.clone()), Origin::Sweep).ok()?;
    let id = trial.mint_id(&kind.name, name).ok()?;
    let category = Category::new(id.clone(), name, by);
    let mut records = vec![Record::Kind(kind.clone()), Record::Category(category)];
    records.extend(sessions.iter().map(|(session, confidence)| {
        Record::Members(MemberList {
            session: (*session).to_string(),
            kind: kind.name.clone(),
            members: vec![Membership {
                category: id.clone(),
                origin: Origin::Sweep,
                confidence: Some(*confidence as f32),
                as_of_ms: now_ms,
            }],
        })
    }));
    for r in records.iter().skip(1) {
        trial.put(r.clone(), Origin::Sweep).ok()?;
    }
    *working = trial;
    Some(Minted {
        write: write.to_string(),
        precedence: kind.precedence,
        kind: kind.name,
        records,
        sessions: sessions.len(),
    })
}

impl Core {
    /// The `other` steps of the window's `onto.filed` rows; none while the
    /// index's shape is built after a start: the next run reads them.
    fn onto_said(&self, now_ms: u64) -> anyhow::Result<Option<Vec<Said>>> {
        let since = now_ms.saturating_sub(WINDOW_MS);
        Ok(self
            .onto_rows(LedgerKind::OntoFiled, since)?
            .map(|rows| said_of(&rows)))
    }

    /// The kinds the window's `other` answers mint, each one machine write of
    /// the sweep's (`sweep-<ms>-<n>-kind`), `n` counting from `first`.
    pub(super) fn onto_mint(
        &self,
        now_ms: u64,
        first: usize,
        out: &mut Swept,
    ) -> anyhow::Result<()> {
        let Some(said) = self.onto_said(now_ms)? else {
            return Ok(());
        };
        let o = self.runner.ontology.snapshot(&self.store)?;
        let plan = plan(&o, &said, now_ms, |n| {
            format!("sweep-{now_ms}-{}-kind", first + n)
        });
        out.refused += plan.refused;
        for m in plan.minted {
            let write = m.write;
            match self.ontology_machine_write(Origin::Sweep, &write, m.records) {
                Ok(()) => {
                    out.kinds += 1;
                    out.writes.push(write);
                }
                Err(e) => {
                    out.failed += 1;
                    tracing::warn!(write, error = %format!("{e:#}"), "ontology: a kind was not written");
                }
            }
        }
        Ok(())
    }
}
