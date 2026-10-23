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
//! - **Names that differ but mean one kind** (theseus-63tfk.1): before the
//!   plan, a pair of mined names no session shares is asked `same_kind.v1`
//!   (`renamed.rs`); a yes at the act band joins the two names' sessions into
//!   one cluster ([`Hints::same`]), each name an example of the kind.
//! - **The kind's name** is a Choice among the cluster's names
//!   (`name_it.v1`): Jev's pick at the act band names the kind
//!   ([`Hints::named`]); `none`, a pick below the band, a single name or no
//!   answer fall back to the most shared name, as before.
//! - **Undo** retires the kind with its category and drops the memberships
//!   (`Ontology::undo`). A name a kind of the table already lists in
//!   `examples` is not clustered again, an undone kind's included, so a
//!   repeat run writes nothing and an undo stays undone.
//! - **No card.** The sweep runs at night: its write is in the morning note
//!   (`digest.rs`), with the id `theseus ontology undo` takes.

use std::collections::{BTreeMap, BTreeSet};

use std::cmp::Reverse;

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

/// The sentence a kind's description ends with when Jev's pick named it: the
/// morning note and the cockpit read it (`lib/ontology.ts` has the same words).
pub const NAMED_NOTE: &str = "Jev picked its name.";

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
    /// Named by Jev's pick (else by the most shared name).
    pub named: bool,
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

/// What Jev's answers add to the plan: names it called one kind, and the
/// name it picked for a cluster.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hints {
    /// Pairs of folded names called one kind at the act band.
    pub same: Vec<(String, String)>,
    /// A cluster's key (its smallest folded name) and the mined name picked.
    pub named: BTreeMap<String, String>,
    /// What the questions cost.
    pub spent: theseus_judge::price::Micros,
}

/// Every folded name the `other` answers mined: the name as first spelled,
/// and each session that said it with its most sure answer.
pub type Folds<'a> = BTreeMap<String, (String, BTreeMap<&'a str, f64>)>;

pub fn folds(said: &[Said]) -> Folds<'_> {
    let mut names: Folds = BTreeMap::new();
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
    names
}

/// Folded names a kind of the table lists in `examples`.
fn listed_in(o: &Ontology) -> BTreeSet<String> {
    o.kinds()
        .iter()
        .flat_map(|k| k.examples.iter().map(|e| fold(e)))
        .collect()
}

/// The pairs of unlisted names whose sessions together add to either group's,
/// largest union first, in a fixed order: what `same_kind.v1` may be asked.
/// `kept` are the yeses of earlier nights, which already join names into
/// groups. A pair is asked when a yes could lead to a cluster, now or by a
/// chain (`a~b`, `b~c`): its two groups, with the sessions of every name no
/// kept yes has linked, reach [`CLUSTER`] sessions. A group of kept names
/// reaches on by its own pairs, so another's sessions do not count for this
/// one. Two groups that are each a cluster are asked too, so one thing does not
/// mint twice. A run asks at most `KIND_PAIRS_MAX` of them (`renamed.rs`).
pub fn kind_pairs(
    o: &Ontology,
    names: &Folds<'_>,
    kept: &BTreeSet<(String, String)>,
) -> Vec<(String, String)> {
    let listed = listed_in(o);
    let open: Folds = names
        .iter()
        .filter(|(f, _)| !listed.contains(*f))
        .map(|(f, v)| (f.clone(), v.clone()))
        .collect();
    let same: Vec<(String, String)> = kept.iter().cloned().collect();
    let groups = clusters(&open, &same);
    let at: BTreeMap<&String, usize> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, g)| g.folds.iter().map(move |f| (f, i)))
        .collect();
    let free: BTreeSet<&str> = groups
        .iter()
        .filter(|g| g.folds.len() == 1)
        .flat_map(|g| g.sessions.keys().copied())
        .collect();
    let mut pairs: Vec<(Reverse<usize>, &String, &String)> = Vec::new();
    for (i, a) in open.keys().enumerate() {
        for b in open.keys().skip(i + 1) {
            if at[a] == at[b] {
                continue;
            }
            let (ga, gb) = (&groups[at[a]], &groups[at[b]]);
            let joined = ga.sessions.keys().chain(gb.sessions.keys());
            let union = joined.clone().collect::<BTreeSet<_>>().len();
            let reach = joined.chain(&free).collect::<BTreeSet<_>>().len();
            if union > ga.sessions.len().max(gb.sessions.len()) && reach >= CLUSTER {
                pairs.push((Reverse(union), a, b));
            }
        }
    }
    pairs.sort();
    pairs
        .into_iter()
        .map(|(_, a, b)| (a.clone(), b.clone()))
        .collect()
}

/// A group of names the sessions share a kind for: the names joined by
/// `same`, with every session that said any of them.
#[derive(Debug, Clone)]
pub struct Cluster<'a> {
    /// The smallest folded name: what a pick is kept under.
    pub key: String,
    /// The folded names, most shared first.
    pub folds: Vec<String>,
    pub sessions: BTreeMap<&'a str, f64>,
}

impl Cluster<'_> {
    /// The names as first spelled, most shared first.
    pub fn spelled(&self, names: &Folds<'_>) -> Vec<String> {
        self.folds.iter().map(|f| names[f].0.clone()).collect()
    }
}

/// The clusters of `names`, `same` joining the names it pairs.
fn clusters<'a>(names: &Folds<'a>, same: &[(String, String)]) -> Vec<Cluster<'a>> {
    let mut groups: Vec<BTreeSet<&String>> = names.keys().map(|f| BTreeSet::from([f])).collect();
    for (a, b) in same {
        let at = |f: &String| groups.iter().position(|g| g.iter().any(|x| *x == f));
        if let (Some(i), Some(j)) = (at(a), at(b)) {
            if i != j {
                let moved = groups.remove(j.max(i));
                groups[j.min(i)].extend(moved);
            }
        }
    }
    groups
        .into_iter()
        .map(|g| {
            let mut sessions: BTreeMap<&str, f64> = BTreeMap::new();
            for f in &g {
                for (s, c) in &names[*f].1 {
                    let at = sessions.entry(*s).or_default();
                    *at = at.max(*c);
                }
            }
            let mut folds: Vec<String> = g.iter().map(|f| (*f).clone()).collect();
            folds.sort_by_key(|f| (Reverse(names[f].1.len()), f.clone()));
            let key = g.iter().next().map(|f| (*f).clone()).unwrap_or_default();
            Cluster {
                key,
                folds,
                sessions,
            }
        })
        .collect()
}

/// The clusters a run would mint from, largest first, at most [`PER_RUN`]:
/// what `name_it.v1` may be asked about.
pub fn leads<'a>(o: &Ontology, names: &Folds<'a>, same: &[(String, String)]) -> Vec<Cluster<'a>> {
    let listed = listed_in(o);
    let mut found: Vec<Cluster> = clusters(names, same)
        .into_iter()
        .filter(|c| c.sessions.len() >= CLUSTER && !c.folds.iter().any(|f| listed.contains(f)))
        .collect();
    found.sort_by_key(|c| (Reverse(c.sessions.len()), c.key.clone()));
    found.truncate(PER_RUN);
    found
}

/// What `said` mints against `o`: `write(n)` is the id of the nth write
/// (`sweep-<ms>-<n>-kind`), which the kind's and category's `added_by` carry.
pub fn plan(o: &Ontology, said: &[Said], now_ms: u64, write: impl Fn(usize) -> String) -> Plan {
    plan_with(o, said, now_ms, write, &Hints::default())
}

/// [`plan`] with what Jev's answers add (`hints`).
pub fn plan_with(
    o: &Ontology,
    said: &[Said],
    now_ms: u64,
    write: impl Fn(usize) -> String,
    hints: &Hints,
) -> Plan {
    let names = folds(said);
    let all = clusters(&names, &hints.same);
    let mut listed = listed_in(o);
    let mut precedence = o.kinds().iter().map(|k| k.precedence).max().unwrap_or(0);
    let mut working = o.clone();
    let mut plan = Plan::default();
    while plan.minted.len() < PER_RUN {
        let lead = all
            .iter()
            .filter(|c| c.sessions.len() >= CLUSTER && !c.folds.iter().any(|f| listed.contains(f)))
            .max_by_key(|c| (c.sessions.len(), Reverse(c.key.clone())));
        let Some(cluster) = lead else {
            break;
        };
        let spelled = cluster.spelled(&names);
        let default = spelled[0].clone();
        let picked = hints.named.get(&cluster.key).cloned();
        // The names that at least `CLUSTER` of its sessions share, then by how
        // many share them.
        let mut shared: Vec<(usize, &String)> = names
            .iter()
            .filter(|(f, _)| !listed.contains(*f) && !cluster.folds.contains(f))
            .map(|(_, (spelled, with))| {
                let n = with
                    .keys()
                    .filter(|s| cluster.sessions.contains_key(*s))
                    .count();
                (n, spelled)
            })
            .filter(|(n, _)| *n >= CLUSTER)
            .collect();
        shared.sort_by_key(|(n, spelled)| (Reverse(*n), spelled.to_ascii_lowercase()));
        // The kind's name leads its examples, then the rest of the cluster's
        // names, then the names its sessions share.
        let examples_of = |lead: &str| {
            let mut examples = vec![lead.to_string()];
            let rest = spelled.iter().chain(shared.iter().map(|(_, s)| *s));
            for e in rest {
                if !examples.iter().any(|x| fold(x) == fold(e)) {
                    examples.push(e.clone());
                }
            }
            examples.truncate(EXAMPLES_MAX);
            examples
        };
        let n = plan.minted.len() + plan.refused as usize;
        // Jev's pick first, then the most shared name when the pick makes no
        // kind.
        let mut attempts: Vec<(String, bool)> = picked.into_iter().map(|p| (p, true)).collect();
        attempts.push((default.clone(), false));
        let made = attempts.into_iter().find_map(|(name, named)| {
            mint(
                &mut working,
                &examples_of(&name),
                &cluster.sessions,
                now_ms,
                precedence,
                &write(n),
                named,
            )
        });
        listed.extend(cluster.folds.iter().cloned());
        listed.extend(examples_of(&default).iter().map(|e| fold(e)));
        match made {
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
    examples: &[String],
    sessions: &BTreeMap<&str, f64>,
    now_ms: u64,
    precedence: u32,
    write: &str,
    named: bool,
) -> Option<Minted> {
    let name = examples[0].as_str();
    let by = format!("sweep:{write}");
    let note = if named {
        format!(" {NAMED_NOTE}")
    } else {
        String::new()
    };
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
             kinds: {}.{note}",
            sessions.len(),
            examples.join(", "),
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
        named,
    })
}

impl Core {
    /// The `other` steps of the window's `onto.filed` rows; none while the
    /// index's shape is built after a start: the next run reads them.
    pub(super) fn onto_said(&self, now_ms: u64) -> anyhow::Result<Option<Vec<Said>>> {
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
        hints: &Hints,
        out: &mut Swept,
    ) -> anyhow::Result<()> {
        let Some(said) = self.onto_said(now_ms)? else {
            return Ok(());
        };
        let o = self.runner.ontology.snapshot(&self.store)?;
        let plan = plan_with(
            &o,
            &said,
            now_ms,
            |n| format!("sweep-{now_ms}-{}-kind", first + n),
            hints,
        );
        out.refused += plan.refused;
        for m in plan.minted {
            let write = m.write.clone();
            match self.ontology_machine_write(Origin::Sweep, &write, m.records.clone()) {
                Ok(()) => {
                    out.kinds += 1;
                    out.named_kinds += u64::from(m.named);
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
