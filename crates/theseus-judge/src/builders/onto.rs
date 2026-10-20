//! `onto.v1`'s state and options (theseus-xzxqv): what the shared `inbound`
//! state adds so Jev can file the session into the ontology.
//!
//! - the **thread** and **project** titles, when the session has them;
//! - the session's **memberships**, at most [`MEMBERS`] a kind (what the
//!   `still_<kind>` Nouls ask about);
//! - the **mined names** ([`crate::mine`]), at most ten, which a new
//!   category may take.
//!
//! The candidates are the options of each kind's question, not state: a
//! kind's [`OntoKind::pool`] holds every live category the core knows, and
//! [`OntoInput::pick`] chooses at most [`CANDIDATES`] of them in code, in this
//! order: the session's own categories, the thread's, those whose name or
//! description shares a word with the message, then the most recently used.
//! A kind is asked when it is listed in the input (the core leaves `project`
//! out when the thread has one), and its last option, `new`, is the builder's
//! own, so a kind with no category yet can still be called for. All of it is
//! left out when the input has no `onto`, so a state without it is byte for
//! byte the one before.
//!
//! Pictures and attachments (theseus-und4g, section 5 of the design) add three
//! more optional fields:
//!
//! - `images`: each picture that was sent, `{name, image}`, where `image` is
//!   the reference of an asset sent beside the state (once: each reference is
//!   billed again), and `skipped_images`, those that were not and why
//!   ([`crate::pic`]);
//! - `attachments`: a text-like attachment's first [`EXCERPT_CHARS`]
//!   characters, `{name, excerpt}`.
//!
//! A message in a **shared place** is reduced ([`OntoInput::shared`]): the
//! session's interpreted memberships, the thread and project titles, the
//! candidates and the mined names are all left out, and no kind is asked, so
//! what Jev sees of the pictures is only `image_subject` and the privacy
//! questions, with no candidate from any private session (the place rule).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{share, Clipper};
use crate::mine::{self, Source as Mined};
use crate::pack::{Dynamic, Item, Source};
use crate::pic::Picture;
use crate::state::StateBuilder;

/// The interpreted kinds `onto.v1` asks about, as the pack's questions name
/// them.
pub const KINDS: [&str; 6] = [
    "topic",
    "project",
    "event",
    "resource",
    "capability",
    "expertise",
];
/// The candidates a kind's question offers at most.
pub const CANDIDATES: usize = 20;
/// The memberships a kind's state shows, and the `still_<kind>` Nouls it asks.
pub const MEMBERS: usize = 5;
/// A text attachment's excerpt in the state: its first characters.
pub const EXCERPT_CHARS: usize = 2000;
/// The text attachments a state shows at most.
pub const MAX_TEXTS: usize = 4;
/// A candidate's description in an option: about 50 tokens.
const DESCRIPTION_CHARS: usize = 200;

/// A category a kind offers, as the core lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntoItem {
    /// The category's id (`topic:rust-harness`).
    pub id: String,
    /// What it is called, with its parents if it nests (`theseus › ontology`).
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The session holds it now.
    #[serde(default)]
    pub session: bool,
    /// Another session of the thread holds it.
    #[serde(default)]
    pub thread: bool,
    /// When a session last held it (ms), for the most recent first.
    #[serde(default)]
    pub used_ms: u64,
}

/// One interpreted kind the message is asked about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntoKind {
    pub kind: String,
    /// What the kind is for, as its row says (`new` is worded with it).
    #[serde(default)]
    pub meaning: String,
    /// Every live category of the kind.
    #[serde(default)]
    pub pool: Vec<OntoItem>,
    /// The sources code mines this kind's names from (`titles`, `paths`,
    /// `hosts`, `capitalised`); none means all four.
    #[serde(default)]
    pub mined_from: Vec<String>,
}

/// A row of the kinds table, for `kind_of_thing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntoTable {
    pub name: String,
    pub meaning: String,
}

/// A text-like attachment (plain text, Markdown, code, PDF text, CSV): its
/// name and its text, of which the state keeps the first [`EXCERPT_CHARS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntoText {
    pub name: String,
    pub text: String,
}

/// The ontology fields of the inbound input.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OntoInput {
    #[serde(default)]
    pub thread_title: Option<String>,
    #[serde(default)]
    pub project_title: Option<String>,
    /// The kinds asked about, in the order the pack asks them.
    #[serde(default)]
    pub kinds: Vec<OntoKind>,
    /// The table's interpreted kinds, for `kind_of_thing`.
    #[serde(default)]
    pub table: Vec<OntoTable>,
    /// Names the naming call suggested for this session earlier: offered
    /// before the mined ones.
    #[serde(default)]
    pub suggested: Vec<String>,
    /// The message's pictures, each prepared ([`crate::pic::prepare_all`]).
    /// Never serialized: the bytes stay in the process.
    #[serde(skip)]
    pub pictures: Vec<Picture>,
    /// The message's text-like attachments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<OntoText>,
    /// The message came from a shared place: only the pictures' own
    /// questions are asked, with nothing of any session's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
}

/// What [`OntoInput::pick`] chose for one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub kind: String,
    /// The options in order: at most [`CANDIDATES`].
    pub candidates: Vec<OntoItem>,
    /// The session's memberships, at most [`MEMBERS`].
    pub members: Vec<OntoItem>,
}

fn words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .map(|w| w.chars().flat_map(char::to_lowercase).collect())
        .collect()
}

impl OntoKind {
    /// At most [`CANDIDATES`] of the pool for `message`: the session's own,
    /// the thread's, a word match (more shared words first), then the most
    /// recent, each group newest first and none twice.
    pub fn pick(&self, message: &str) -> Picked {
        let m = words(message);
        let hits = |i: &OntoItem| {
            words(&format!("{} {}", i.name, i.description))
                .intersection(&m)
                .count()
        };
        let mut rest: Vec<(&OntoItem, usize)> = self.pool.iter().map(|i| (i, hits(i))).collect();
        rest.sort_by(|(a, ha), (b, hb)| {
            (!a.session, !a.thread, *ha == 0, std::cmp::Reverse(*ha))
                .cmp(&(!b.session, !b.thread, *hb == 0, std::cmp::Reverse(*hb)))
                .then(b.used_ms.cmp(&a.used_ms))
                .then(a.id.cmp(&b.id))
        });
        Picked {
            kind: self.kind.clone(),
            candidates: rest
                .iter()
                .take(CANDIDATES)
                .map(|(i, _)| (*i).clone())
                .collect(),
            members: self
                .pool
                .iter()
                .filter(|i| i.session)
                .take(MEMBERS)
                .cloned()
                .collect(),
        }
    }

    fn mined(&self) -> Vec<Mined> {
        let all = [
            Mined::Titles,
            Mined::Paths,
            Mined::Hosts,
            Mined::Capitalised,
        ];
        if self.mined_from.is_empty() {
            return all.to_vec();
        }
        all.into_iter()
            .filter(|s| {
                let name = match s {
                    Mined::Titles => "titles",
                    Mined::Paths => "paths",
                    Mined::Hosts => "hosts",
                    Mined::Capitalised => "capitalised",
                };
                self.mined_from.iter().any(|m| m == name)
            })
            .collect()
    }
}

impl OntoInput {
    /// The kinds asked about that the pack has questions for, in the pack's
    /// order.
    fn asked(&self) -> Vec<&OntoKind> {
        if self.shared {
            return Vec::new();
        }
        KINDS
            .iter()
            .filter_map(|k| self.kinds.iter().find(|o| o.kind == *k))
            .collect()
    }

    /// What each asked kind offers for `message`.
    pub fn pick(&self, message: &str) -> Vec<Picked> {
        self.asked().iter().map(|k| k.pick(message)).collect()
    }

    /// The names mined from the message and its session, at most ten, from
    /// the sources the asked kinds name.
    pub fn names(&self, message: &str, previous: Option<&str>) -> Vec<String> {
        if self.shared {
            return Vec::new();
        }
        let mut sources: Vec<Mined> = Vec::new();
        for k in self.asked() {
            for s in k.mined() {
                if !sources.contains(&s) {
                    sources.push(s);
                }
            }
        }
        let mined = mine::mine(
            &mine::Text {
                message,
                previous,
                thread_title: self.thread_title.as_deref(),
                project_title: self.project_title.as_deref(),
            },
            &sources,
        );
        let mut all: Vec<String> = Vec::new();
        for n in self.suggested.iter().chain(&mined) {
            if all.len() < mine::MAX_NAMES && !all.iter().any(|a| a.eq_ignore_ascii_case(n)) {
                all.push(n.clone());
            }
        }
        all
    }
}

/// The candidates source and the memberships source of a kind.
fn sources(kind: &str) -> Option<(Source, Source)> {
    Some(match kind {
        "topic" => (Source::Topics, Source::TopicMembers),
        "project" => (Source::Projects, Source::ProjectMembers),
        "event" => (Source::Events, Source::EventMembers),
        "resource" => (Source::Resources, Source::ResourceMembers),
        "capability" => (Source::Capabilities, Source::CapabilityMembers),
        "expertise" => (Source::Expertise, Source::ExpertiseMembers),
        _ => return None,
    })
}

/// The ontology fields of the inbound state.
pub(super) fn state(b: &mut StateBuilder, c: &Clipper, cap: u64, i: &super::InboundInput) {
    let Some(o) = &i.onto else { return };
    media(b, c, o);
    if o.shared {
        return;
    }
    if let Some(t) = o.thread_title.as_deref().filter(|t| !t.trim().is_empty()) {
        b.scalar("thread", c.clip(t, 80)).cut_if("thread", c.cut());
    }
    if let Some(t) = o.project_title.as_deref().filter(|t| !t.trim().is_empty()) {
        b.scalar("project", c.clip(t, 80))
            .cut_if("project", c.cut());
    }
    let mut held = Map::new();
    for p in o.pick(&i.message) {
        if !p.members.is_empty() {
            held.insert(
                p.kind.clone(),
                Value::Array(
                    p.members
                        .iter()
                        .map(|m| json!(c.clip(&m.name, 80)))
                        .collect(),
                ),
            );
        }
    }
    if !held.is_empty() {
        b.scalar("memberships", Value::Object(held))
            .cut_if("memberships", c.cut());
    }
    let names = o.names(&i.message, i.previous_human_message.as_deref());
    if !names.is_empty() {
        let items: Vec<Value> = names.iter().map(|n| json!(c.clip(n, 60))).collect();
        b.list_head("mined_names", 8, share(cap, 8), items, 0)
            .cut_if("mined_names", c.cut());
    }
}

/// The pictures and text attachments: the state's `images`,
/// `skipped_images` and `attachments`. Each is left out when there is none.
fn media(b: &mut StateBuilder, c: &Clipper, o: &OntoInput) {
    let mut sent: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    for p in &o.pictures {
        match &p.shown {
            Ok(shown) => {
                let id = shown.id();
                if !sent.iter().any(|v| v["image"] == json!(id)) {
                    sent.push(json!({"name": c.clip(&p.name, 80), "image": id}));
                }
            }
            Err(why) => skipped.push(json!({"name": c.clip(&p.name, 80), "why": why.to_string()})),
        }
    }
    if !sent.is_empty() {
        b.scalar("images", Value::Array(sent));
    }
    if !skipped.is_empty() {
        b.scalar("skipped_images", Value::Array(skipped));
    }
    let texts: Vec<Value> = o
        .texts
        .iter()
        .take(MAX_TEXTS)
        .map(|t| json!({"name": c.clip(&t.name, 80), "excerpt": c.clip(&t.text, EXCERPT_CHARS)}))
        .collect();
    if !texts.is_empty() {
        // The lowest priority of the ontology's fields: a state over its cap
        // loses excerpts before the memberships and names.
        b.list_head("attachments", 6, 2500, texts, 0)
            .cut_if("attachments", c.cut());
    }
}

/// The options and per-item Nouls of the ontology questions.
pub(super) fn dynamic(
    dynamic: &mut Dynamic,
    i: &super::InboundInput,
    clip: &dyn Fn(&str, usize) -> String,
) {
    let Some(o) = &i.onto else { return };
    let mut shown: Vec<Item> = Vec::new();
    for p in &o.pictures {
        if let Ok(s) = &p.shown {
            let asset = s.asset();
            if !dynamic.assets.iter().any(|a| a.id == asset.id) {
                shown.push(Item {
                    key: asset.id.clone(),
                    text: clip(&p.name, 80),
                });
                dynamic.assets.push(asset);
            }
        }
    }
    if !shown.is_empty() {
        dynamic.sources.insert(Source::Images, shown);
    }
    if o.shared {
        return;
    }
    for (k, picked) in o.asked().into_iter().zip(o.pick(&i.message)) {
        let Some((candidates, members)) = sources(&k.kind) else {
            continue;
        };
        let described = |m: &OntoItem| {
            let d = m.description.trim();
            match d.is_empty() {
                true => clip(&m.name, 120),
                false => format!(
                    "{}: {}",
                    clip(&m.name, 120),
                    clip(d.trim_end_matches('.'), DESCRIPTION_CHARS)
                ),
            }
        };
        let mut options: Vec<Item> = picked
            .candidates
            .iter()
            .enumerate()
            .map(|(n, m)| Item {
                key: format!("c{}", n + 1),
                text: described(m),
            })
            .collect();
        options.push(Item {
            key: "new".into(),
            text: format!(
                "A new {} of its own that none of the listed covers: {}.",
                k.kind,
                clip(k.meaning.trim().trim_end_matches('.'), DESCRIPTION_CHARS)
            ),
        });
        dynamic.sources.insert(candidates, options);
        // A membership's Noul names its category as the options do.
        dynamic.sources.insert(
            members,
            picked
                .members
                .iter()
                .map(|m| Item {
                    key: m.id.clone(),
                    text: format!("“{}”", described(m).trim_end_matches('.')),
                })
                .collect(),
        );
    }
    let names = o.names(&i.message, i.previous_human_message.as_deref());
    dynamic.sources.insert(
        Source::Names,
        names
            .iter()
            .enumerate()
            .map(|(n, name)| Item {
                key: format!("n{}", n + 1),
                text: clip(name, 60),
            })
            .collect(),
    );
    dynamic.sources.insert(
        Source::Kinds,
        o.table
            .iter()
            .map(|t| Item {
                key: t.name.clone(),
                text: clip(&t.meaning, 160),
            })
            .collect(),
    );
}
