//! People's proposals one row per person (theseus-fvyx). After the owner's
//! backfill a name was proposed once for every session it came up in (one
//! 76 times), and a bare first name beside the full name it is a word of as
//! a second new person, so accepting both made two people to merge. The
//! listing (`ontology.proposals` with `by_person`) groups them:
//!
//! - **One person, one row.** A held person's proposals by its id; a new
//!   person's by its name folded (`people::fold`), or by the held person an
//!   exact handle or that name finds by now (an accept would join it).
//! - **A first name beside its full name.** A new person whose whole name is
//!   one word that is a word of exactly one other person's name, among the
//!   rows and the held people (the exclusions' aside), is listed inside that
//!   person's row (`first_names`) and accepted as them (`as_person`). With
//!   two or more such people it stays its own row, `ambiguous` naming them,
//!   and is never accepted in bulk.
//!
//! A row's answer is each of its proposals answered, through the single
//! accept's or reject's own path (`rpc/people.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use theseus_ontology::{CategoryId, Ontology};
use theseus_protocol::{OntologyPersonProposals, OntologyProposal};

use crate::judge::people::{self, NotPeople};

/// A row's titles at most.
const TITLES: usize = 3;

/// One row as it is built: its proposals, newest first, and those listed
/// inside it as a first name.
struct Row {
    held: Option<CategoryId>,
    members: Vec<OntologyProposal>,
    first: Vec<OntologyProposal>,
    ambiguous: Vec<String>,
}

/// A listed person's proposal's row: the held person (its id), or a new
/// person's folded name.
fn key_of(o: &Ontology, p: &OntologyProposal) -> Option<(String, Option<CategoryId>)> {
    let who = p.person.as_ref()?;
    let held = match who.new {
        false => p.topic.as_deref().and_then(|t| CategoryId::parse(t).ok()),
        true => super::proposals::held_for(o, &who.name, &who.handles),
    };
    Some(match held {
        Some(id) => (id.to_string(), Some(id)),
        None => (format!("name:{}", people::fold(&who.name)), None),
    })
}

/// A name's words of two letters or more, folded.
fn words(name: &str) -> Vec<String> {
    people::name_words(&people::fold(name))
        .filter(|w| w.chars().count() > 1)
        .map(str::to_string)
        .collect()
}

/// The topics' proposals as listed, and the people's rows: each whose best
/// proposal reaches `min`, most sessions first.
pub(super) fn group(
    o: &Ontology,
    not: &NotPeople,
    listed: Vec<OntologyProposal>,
    min: f64,
) -> (Vec<OntologyProposal>, Vec<OntologyPersonProposals>) {
    let mut topics = Vec::new();
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for p in listed {
        let Some((key, held)) = key_of(o, &p) else {
            if p.confidence >= min {
                topics.push(p);
            }
            continue;
        };
        rows.entry(key)
            .or_insert_with(|| Row {
                held,
                members: Vec::new(),
                first: Vec::new(),
                ambiguous: Vec::new(),
            })
            .members
            .push(p);
    }
    first_names(o, not, &mut rows);
    let mut out: Vec<OntologyPersonProposals> = rows
        .into_iter()
        .filter(|(_, r)| !r.members.is_empty() || !r.first.is_empty())
        .map(|(key, r)| finish(o, key, r))
        .filter(|g| g.confidence_max >= min)
        .collect();
    out.sort_by(|a, b| {
        (b.sessions, b.judgments.len())
            .cmp(&(a.sessions, a.judgments.len()))
            .then_with(|| a.name.cmp(&b.name))
    });
    (topics, out)
}

/// A row's name: the held person's, or the name most of its proposals give
/// (the newest of equals).
fn name_of(o: &Ontology, r: &Row) -> String {
    if let Some(c) = r.held.as_ref().and_then(|id| o.category(id)) {
        return c.name.clone();
    }
    surface(&r.members).unwrap_or_default()
}

/// The name most of `ps` give, newest first among equals.
fn surface(ps: &[OntologyProposal]) -> Option<String> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for (i, p) in ps.iter().enumerate() {
        if let Some(who) = &p.person {
            let e = counts.entry(who.name.trim()).or_insert((0, i));
            e.0 += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1 .0.cmp(&b.1 .0).then(b.1 .1.cmp(&a.1 .1)))
        .map(|(n, _)| n.to_string())
}

/// The first-name pass: each new row whose name is one word is moved inside
/// the one other person whose name holds that word, or flagged when several
/// do.
fn first_names(o: &Ontology, not: &NotPeople, rows: &mut BTreeMap<String, Row>) {
    // Every person a first name may be: the rows' (by key) and the held
    // people's, with their names' words.
    let mut whole: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for (key, r) in rows.iter() {
        let name = name_of(o, r);
        whole.insert(key.clone(), (name.clone(), words(&name)));
    }
    for c in o.categories().filter(|c| {
        c.kind() == theseus_ontology::person::KIND
            && c.merged_into.is_none()
            && c.retired_ms.is_none()
            && !not.excludes_person(c)
            && !not.excludes_held(c.id.local())
    }) {
        whole
            .entry(c.id.to_string())
            .or_insert_with(|| (c.name.clone(), words(&c.name)));
    }
    let bare: Vec<String> = rows
        .iter()
        .filter(|(k, r)| k.starts_with("name:") && r.held.is_none())
        .filter(|(k, _)| words(&k["name:".len()..]).len() == 1 && !k[5..].contains(' '))
        .map(|(k, _)| k.clone())
        .collect();
    for key in bare {
        let word = &key["name:".len()..];
        let theirs: BTreeSet<&String> = whole
            .iter()
            .filter(|(k, (_, ws))| **k != key && ws.len() > 1 && ws.iter().any(|w| w == word))
            .map(|(k, _)| k)
            .collect();
        match theirs.len() {
            0 => {}
            1 => {
                let to = theirs.into_iter().next().cloned().unwrap_or_default();
                let Some(mut moved) = rows.remove(&key) else {
                    continue;
                };
                let held = CategoryId::parse(&to)
                    .ok()
                    .filter(|_| !to.starts_with("name:"));
                let row = rows.entry(to).or_insert_with(|| Row {
                    held,
                    members: Vec::new(),
                    first: Vec::new(),
                    ambiguous: Vec::new(),
                });
                row.first.append(&mut moved.members);
            }
            _ => {
                if let Some(r) = rows.get_mut(&key) {
                    r.ambiguous = theirs.iter().map(|k| whole[*k].0.clone()).collect();
                }
            }
        }
    }
}

/// A row as the protocol has it.
fn finish(o: &Ontology, key: String, r: Row) -> OntologyPersonProposals {
    let name = name_of(o, &r);
    let name = match name.is_empty() {
        true => surface(&r.first).unwrap_or_default(),
        false => name,
    };
    let mut all: Vec<&OntologyProposal> = r.members.iter().chain(&r.first).collect();
    all.sort_by_key(|p| std::cmp::Reverse(p.at_ms));
    let sessions: HashSet<&str> = all.iter().map(|p| p.session_id.as_str()).collect();
    let mut titles: Vec<String> = Vec::new();
    for p in &all {
        if let Some(t) = p.session_title.as_ref().filter(|t| !titles.contains(t)) {
            titles.push(t.clone());
        }
    }
    titles.truncate(TITLES);
    let bands: Vec<String> = ["act", "confirm", "escalate"]
        .into_iter()
        .filter(|b| all.iter().any(|p| p.band == *b))
        .map(str::to_string)
        .collect();
    let handles: BTreeSet<String> = all
        .iter()
        .filter_map(|p| p.person.as_ref())
        .flat_map(|w| w.handles.iter().cloned())
        .collect();
    let first_names: BTreeSet<String> = r
        .first
        .iter()
        .filter_map(|p| p.person.as_ref().map(|w| w.name.trim().to_string()))
        .collect();
    let conf = all.iter().map(|p| p.confidence);
    OntologyPersonProposals {
        new: r.held.is_none(),
        as_person: r
            .held
            .as_ref()
            .map_or_else(|| name.clone(), |id| id.to_string()),
        key,
        judgments: all.iter().map(|p| p.judgment.clone()).collect(),
        sessions: sessions.len() as u32,
        confidence_min: conf.clone().fold(f64::INFINITY, f64::min),
        confidence_max: conf.fold(0.0, f64::max),
        bands,
        handles: handles.into_iter().collect(),
        role_line: r
            .members
            .iter()
            .find_map(|p| p.person.as_ref().and_then(|w| w.role_line.clone())),
        titles,
        first_names: first_names.into_iter().collect(),
        ambiguous: r.ambiguous,
        at_ms: all.first().map_or(0, |p| p.at_ms),
        name,
    }
}
