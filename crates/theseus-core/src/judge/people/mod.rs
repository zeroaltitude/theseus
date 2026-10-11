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
//!    place rule's owner handles, the names the owner writes under, and the
//!    held person those handles hold, by its name and each of its words),
//!    the configured personas, the agents (a session's `agent:<name>`, and
//!    every agent the store knows, `house.rs`), the house's own names, the
//!    assistant, `[people] not_people`, and for that session a name already
//!    proposed, rejected, or held. The owner's person stays among `match`'s
//!    options, so Jev can say a name the store cannot know is his; a match
//!    to it is never a proposal (`rpc/proposals.rs`).
//! 3. **Judge** (`run.rs`): one `people.v1` call a candidate: is it a real
//!    person, is it involved in the session's work, which held person is it
//!    (the 50 nearest by name and handle, [`nearest`]) or a new one, and does
//!    its role line judge the person. The judgments are the sink's
//!    `judge.call` rows, scoped `judge:people`; their context holds the
//!    candidate.
//! 4. **Code decides** ([`decide`]), when the proposals are read
//!    (`rpc/proposals.rs`): bands as `categorize.v1`'s, the thresholds
//!    `[people] act` and `confirm` over the least of the three
//!    probabilities; an evaluative or a sensitive role line is dropped and
//!    the name kept; a proposal of a person the exclusions exclude is not
//!    listed (theseus-0p1r: the backfill's, made before them, too). A
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
pub mod house;
pub mod live;
pub mod run;
pub mod seen;
pub mod sweep;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_excl;
#[cfg(test)]
mod tests_groups;
#[cfg(test)]
mod tests_seen;
#[cfg(test)]
mod tests_sweep;

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

/// A name folded for comparison: lowercase, its words single-spaced, a
/// handle's sigil off each word, so a person held as "@name" (a Discord
/// DM's, as the transport makes it) and a bare "name" fold equal
/// (theseus-0p1r).
pub fn fold(name: &str) -> String {
    name.split_whitespace()
        .map(|w| w.trim_start_matches('@'))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The words of a name, split at anything not a letter or a digit.
pub fn name_words(name: &str) -> impl Iterator<Item = &str> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
}

/// The house's own names (theseus-0p1r): Theseus, its judge, the history it
/// imports, its models' maker. The config's profiles and models are added
/// by [`NotPeople::of`].
pub const HOUSE: [&str; 5] = ["theseus", "jev", "openclaw", "claude", "assistant"];

/// The words that make a name with a house's or an agent's name in it
/// software ("Gull bot", "openclaw-control-ui").
const SOFTWARE: [&str; 5] = ["bot", "ui", "app", "agent", "assistant"];

/// Who is never proposed, from the config and a session's lines.
#[derive(Debug, Clone, Default)]
pub struct NotPeople {
    names: HashSet<String>,
    handles: HashSet<String>,
    /// The held people the handles hold (the owner's), by local id.
    people: HashSet<String>,
}

impl NotPeople {
    /// The config's: `[places] owner` (handles), the personas' names,
    /// `[people] not_people`, the house's names ([`HOUSE`], and each
    /// profile's name and model); and the session's: the owner's names (the
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
        n.names.extend(HOUSE.iter().map(|h| h.to_string()));
        let models = cfg.profiles.values().map(|p| &p.model);
        n.names.extend(
            cfg.profiles
                .keys()
                .chain(models)
                .chain([&cfg.model.model])
                .map(|k| fold(k)),
        );
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

    /// The owner's handles as the place rule knows them (`PlaceRule::owners`:
    /// `[places] owner`, else each bound DM's person). theseus-0p1r: on a
    /// config with no `[places] owner` the owner's DM person was never
    /// excluded, so Jev's `match` chose it for the owner's own name.
    pub fn with_owners(mut self, owners: impl IntoIterator<Item = String>) -> NotPeople {
        self.handles.extend(
            owners
                .into_iter()
                .filter_map(|h| theseus_ontology::handle(&h).ok()),
        );
        self
    }

    /// More names never proposed, folded: the agents and the house's names
    /// the store knows (`house.rs`).
    pub fn with_names<'a>(mut self, names: impl IntoIterator<Item = &'a String>) -> NotPeople {
        self.names.extend(names.into_iter().map(|n| fold(n)));
        self.names.remove("");
        self
    }

    /// The held people the handles exclude (the owner's own person, a DM's,
    /// by whatever name it is held): never a proposal (a match to one is
    /// hidden; they stay `match`'s options), and
    /// their names (each name, `name:` handle, and each word of them) never
    /// a candidate, so the owner is never one by name either (theseus-u5n8;
    /// theseus-0p1r: his first name alone, beside a person held under his
    /// handle's "@name").
    pub fn with_held(mut self, o: &Ontology) -> NotPeople {
        let theirs: Vec<&Category> = o
            .categories()
            .filter(|c| c.kind() == theseus_ontology::person::KIND)
            .filter(|c| self.holds_handle(c))
            .collect();
        for c in theirs {
            self.people.insert(c.id.local().to_string());
            let handles = handles_of(c);
            let names = std::iter::once(c.name.as_str())
                .chain(handles.iter().filter_map(|h| h.strip_prefix("name:")));
            for n in names {
                self.names.insert(fold(n));
                self.names
                    .extend(name_words(n).filter(|w| w.chars().count() > 1).map(fold));
            }
        }
        self.names.remove("");
        self
    }

    /// Whether a held person, by local id, is one the handles exclude.
    pub fn excludes_held(&self, id: &str) -> bool {
        self.people.contains(id)
    }

    fn holds_handle(&self, c: &Category) -> bool {
        handles_of(c)
            .iter()
            .filter_map(|h| theseus_ontology::handle(h).ok())
            .any(|h| self.handles.contains(&h))
    }

    /// Whether a held person is one never listed or proposed: by a handle,
    /// or by its name as a candidate's is ([`NotPeople::excludes_name`]).
    pub fn excludes_person(&self, c: &Category) -> bool {
        self.holds_handle(c) || self.excludes_name(&c.name)
    }

    /// Whether a candidate is one never proposed: by its folded name, a
    /// name of software (a word one of the names beside a [`SOFTWARE`] word:
    /// an agent's "Gull" in "Gull bot", "openclaw-control-ui"; or a "bot"
    /// word alone: "bot-this-assistant"), or any of its handles.
    pub fn excludes(&self, c: &PersonCandidate) -> bool {
        self.excludes_name(&c.name)
            || c.handles
                .iter()
                .filter_map(|h| theseus_ontology::handle(h).ok())
                .any(|h| self.handles.contains(&h))
    }

    /// Whether a name alone is one never proposed ([`NotPeople::excludes`]).
    pub fn excludes_name(&self, name: &str) -> bool {
        let folded = fold(name);
        let words: Vec<&str> = name_words(&folded).collect();
        let software = words.iter().any(|w| SOFTWARE.contains(w));
        self.names.contains(&folded)
            || words.contains(&"bot")
            || (software && words.iter().any(|w| self.names.contains(*w)))
    }
}

/// The held people a candidate may be, nearest first, at most
/// [`theseus_judge::builders::PEOPLE`]: one holding a handle of the
/// candidate's, or its name; then those sharing a word of the name; then the
/// rest, each part by name. The people the exclusions exclude stay
/// options (theseus-0p1r): Jev's match of a name to the owner's own person
/// is how a form of his name the store cannot know is told apart, and a
/// match to an excluded person is never listed.
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
pub(super) fn describe(p: &Category) -> String {
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
    /// The role line, unless Jev found it judged the person or carried
    /// what is theirs alone (pay, money, health, leave, HR).
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
/// A role line Jev finds evaluative (`evaluative` at 0.5 or above) or
/// sensitive (`sensitive`, theseus-0p1r: compensation, money owed or paid,
/// health, leave, HR), or did not answer either of, is dropped, the name
/// kept: an accept makes it the person's description.
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
    let role_line = match (noul(j, "evaluative"), noul(j, "sensitive")) {
        (Some(e), Some(s)) if e < 0.5 && s < 0.5 && !role.is_empty() => Some(role.to_string()),
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
