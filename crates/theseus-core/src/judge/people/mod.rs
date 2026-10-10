//! `people.v1` (theseus-wy7y): people proposed from a session's text, by the
//! documented Jev pattern, "a model writes the text, Jev selects, code
//! verifies". Jev answers no names, so:
//!
//! 1. **Extract** (`extract.rs`): a cheap profile (`[people]
//!    extract_profile`, default `haiku`) reads the session's human-facing
//!    lines ([`lines`]) and returns its candidate people through a tool
//!    schema, never prose: name, handles, a role line, and the lines that
//!    name them. One `people.extracted` row a call (cost, tokens, model).
//! 2. **Exclude** ([`NotPeople`]), before any judgment: the owner (the
//!    `[places] owner` handles, and the names the owner writes under), the
//!    configured personas, the agents a session names (`agent:<name>`), the
//!    assistant, `[people] not_people`, and for that session a name already
//!    proposed, rejected, or held.
//! 3. **Judge** (`run.rs`): one `people.v1` call a candidate: is it a real
//!    person, is it involved in the session's work, which held person is it
//!    (the 50 nearest by name and handle, [`nearest`]) or a new one, and does
//!    its role line judge the person. The judgments are the sink's
//!    `judge.call` rows, scoped `judge:people`; their context holds the
//!    candidate.
//! 4. **Code decides** ([`decide`]), when the proposals are read
//!    (`rpc/proposals.rs`): bands as `categorize.v1`'s, the thresholds
//!    `[people] act` and `confirm` over the least of the three
//!    probabilities; an evaluative role line is dropped and the name kept. A
//!    proposal, never a membership: the owner accepts or rejects it as a
//!    topic's.
//!
//! Where it runs: live at a private conversation's exchange end (`live.rs`,
//! beside `categorize.v1`, on a mark of its own), and over an import's tag,
//! the owner's backfill (`backfill.rs`, `theseus import people TAG
//! --propose`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use theseus_judge::builders::{HeldPerson, PersonCandidate};
use theseus_judge::{Answer, Judgment, Outcome};
use theseus_ontology::{handles_of, Category, Ontology};

use crate::config::Config;
use crate::node::{Body, Node, Origin};

pub mod backfill;
pub mod extract;
pub mod live;
pub mod run;
#[cfg(test)]
mod tests;

/// The pack.
pub const PACK: &str = "people.v1";
/// Its judgments, their labels, and the extractions' rows.
pub const SCOPE: &str = "judge:people";
/// The lines an extraction reads at most: the newest.
pub const LINES: usize = 80;
/// A line's characters at most.
pub const LINE_CHARS: usize = 600;
/// The candidates one extraction keeps at most.
pub const CANDIDATES: usize = 12;

/// One line of a session's human-facing text: what a person wrote, or what
/// an agent wrote to people. Tool results and outside text are not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub node: String,
    /// `owner`, `assistant`, or the import's author (`person:<name>`,
    /// `agent:<name>`, the owner's bare name).
    pub author: String,
    pub text: String,
}

/// The human-facing lines of `nodes`, oldest first, the newest [`LINES`].
pub fn lines(nodes: &[(u64, Node)]) -> Vec<Line> {
    let mut out: Vec<Line> = nodes
        .iter()
        .filter_map(|(_, n)| {
            let (author, text) = match &n.body {
                Body::UserMessage { text, .. } if n.origin == Origin::Operator => (
                    n.author.clone().unwrap_or_else(|| "owner".into()),
                    text.clone(),
                ),
                Body::AssistantMessage { blocks, .. } => {
                    ("assistant".into(), crate::provider::text_of(blocks))
                }
                Body::Imported {
                    text, integrity, ..
                } if *integrity != crate::import::Integrity::Outside => {
                    let author = n.author.clone().unwrap_or_default();
                    if author == "tool" || author == "outside" {
                        return None;
                    }
                    (author, text.clone())
                }
                _ => return None,
            };
            let text = text.trim();
            (!text.is_empty()).then(|| Line {
                node: n.id.clone(),
                author,
                text: text.chars().take(LINE_CHARS).collect(),
            })
        })
        .collect();
    let cut = out.len().saturating_sub(LINES);
    out.drain(..cut);
    out
}

/// A name folded for comparison: lowercase, its words single-spaced.
pub fn fold(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Who is never proposed, from the config and a session's lines.
#[derive(Debug, Clone, Default)]
pub struct NotPeople {
    names: HashSet<String>,
    handles: HashSet<String>,
}

impl NotPeople {
    /// The config's: `[places] owner` (handles), the personas' names,
    /// `[people] not_people`; and the session's: the owner's names (the
    /// import's bare authors, a live owner's message's author), the agents
    /// it names (`agent:<name>`), and the assistant.
    pub fn of(cfg: &Config, lines: &[Line]) -> NotPeople {
        let mut n = NotPeople::default();
        for h in cfg.places.owner.iter().flatten() {
            if let Ok(h) = theseus_ontology::handle(h) {
                n.handles.insert(h);
            }
        }
        n.names.extend(cfg.personas.keys().map(|k| fold(k)));
        n.names
            .extend(cfg.people.not_people.iter().map(|k| fold(k)));
        n.names.insert("assistant".into());
        for l in lines {
            let a = l.author.trim();
            if let Some(agent) = a.strip_prefix("agent:") {
                n.names.insert(fold(agent));
            } else if a.starts_with("person:") || a == "assistant" || a.is_empty() {
                continue;
            } else if a.contains(':') {
                // A transport's author id (`discord:<id>`) is the owner's in
                // a private place.
                if let Ok(h) = theseus_ontology::handle(a) {
                    n.handles.insert(h);
                }
            } else if a != "owner" {
                n.names.insert(fold(a));
            }
        }
        n.names.remove("");
        n
    }

    /// Whether a candidate is one never proposed: by its folded name, any
    /// word-for-word part of it one of the names (an agent's "Gull" in "Gull
    /// bot"), or any of its handles.
    pub fn excludes(&self, c: &PersonCandidate) -> bool {
        let name = fold(&c.name);
        self.names.contains(&name)
            || name
                .split(' ')
                .any(|w| self.names.contains(w) && name.ends_with(" bot"))
            || c.handles
                .iter()
                .filter_map(|h| theseus_ontology::handle(h).ok())
                .any(|h| self.handles.contains(&h))
    }
}

/// The held people a candidate may be, nearest first, at most
/// [`theseus_judge::builders::PEOPLE`]: one holding a handle of the
/// candidate's, or its name; then those sharing a word of the name; then the
/// rest, each part by name.
pub fn nearest(o: &Ontology, c: &PersonCandidate) -> Vec<HeldPerson> {
    let name = fold(&c.name);
    let words: HashSet<&str> = name.split(' ').filter(|w| w.len() > 1).collect();
    let wanted: HashSet<String> = c
        .handles
        .iter()
        .filter_map(|h| theseus_ontology::handle(h).ok())
        .chain(theseus_ontology::handle(&format!("name:{}", c.name)).ok())
        .map(|h| h.to_lowercase())
        .collect();
    let mut people: Vec<(u8, &Category)> = o
        .categories()
        .filter(|p| p.kind() == theseus_ontology::person::KIND)
        .map(|p| {
            let hs: Vec<String> = handles_of(p).iter().map(|h| h.to_lowercase()).collect();
            let rank = if fold(&p.name) == name || hs.iter().any(|h| wanted.contains(h)) {
                0
            } else if fold(&p.name).split(' ').any(|w| words.contains(w)) {
                1
            } else {
                2
            };
            (rank, p)
        })
        .collect();
    people.sort_by(|a, b| (a.0, &a.1.name).cmp(&(b.0, &b.1.name)));
    people
        .into_iter()
        .take(theseus_judge::builders::PEOPLE)
        .map(|(_, p)| HeldPerson {
            id: p.id.local().to_string(),
            description: describe(p),
        })
        .collect()
}

/// A held person as an option reads: name, handles, and description.
fn describe(p: &Category) -> String {
    let handles: Vec<String> = handles_of(p)
        .into_iter()
        .filter(|h| !theseus_ontology::person::is_name(h) || h[5..] != p.name)
        .collect();
    let mut s = p.name.clone();
    if !handles.is_empty() {
        s.push_str(&format!(" ({})", handles.join(", ")));
    }
    if !p.description.trim().is_empty() {
        s.push_str(&format!(": {}", p.description.trim()));
    }
    s
}

/// Whom a kept candidate is.
#[derive(Debug, Clone, PartialEq)]
pub enum Whom {
    /// A held person, by local id.
    Held(String),
    New,
}

/// What code decided of one `people.v1` judgment.
#[derive(Debug, Clone, PartialEq)]
pub struct Decided {
    pub candidate: PersonCandidate,
    pub whom: Whom,
    /// The least of the probabilities that kept it.
    pub confidence: f64,
    /// `act` or `confirm`.
    pub band: &'static str,
    /// The role line, unless Jev found it judged the person.
    pub role_line: Option<String>,
}

/// A judgment's candidate, as its context holds it.
pub fn candidate_of(j: &Judgment) -> Option<PersonCandidate> {
    serde_json::from_value(j.context["candidate"].clone()).ok()
}

fn noul(j: &Judgment, q: &str) -> Option<f64> {
    j.answers
        .iter()
        .find(|a| a.question == q)
        .and_then(|a| match a.answer {
            Answer::Noul { noul } => Some(noul),
            _ => None,
        })
}

/// The decision on an answered `people.v1` judgment, by the bands: each of
/// `real`, `involved` and `match` at `confirm` or above keeps it, in `act`
/// when the least reaches `act`; `unsure`, or any under `confirm`, drops it.
/// A role line Jev finds evaluative (`evaluative` at 0.5 or above), or did
/// not answer of, is dropped, the name kept.
pub fn decide(j: &Judgment, act: f64, confirm: f64) -> Option<Decided> {
    if j.pack != PACK || j.outcome != Outcome::Answered {
        return None;
    }
    let candidate = candidate_of(j)?;
    let real = noul(j, "real")?;
    let involved = noul(j, "involved")?;
    let (choice, matched) =
        j.answers
            .iter()
            .find(|a| a.question == "match")
            .and_then(|a| match &a.answer {
                Answer::Choice {
                    choice, confidence, ..
                } => Some((choice.clone(), *confidence)),
                _ => None,
            })?;
    let least = real.min(involved).min(matched);
    if least < confirm || choice == "unsure" {
        return None;
    }
    let whom = match choice.as_str() {
        "new_person" => Whom::New,
        id => Whom::Held(id.to_string()),
    };
    let role = candidate.role_line.trim();
    // No line is better than one Jev did not clear.
    let role_line = match noul(j, "evaluative") {
        Some(p) if p < 0.5 && !role.is_empty() => Some(role.to_string()),
        _ => None,
    };
    Some(Decided {
        whom,
        confidence: least,
        band: if least >= act { "act" } else { "confirm" },
        role_line,
        candidate,
    })
}
