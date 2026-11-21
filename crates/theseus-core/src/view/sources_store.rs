//! The selectors' sources over the store (`super::select::sources`), for a turn in a place set to `view`:
//! an exchange's facts from the ledger's record and its nodes, a fold node's sizes, a place's `/new` mark
//! and the places a person has spoken in. The sources whose records belong to other work (what waits, the
//! things named, the name matcher, a task's card, recall, facts about the person) answer nothing here until
//! that work is wired in; the selectors then lay nothing for their bands.

use std::collections::BTreeSet;

use super::fold::FoldNode;
use super::select::sources::{
    ExchangeFacts, Exchanges, Folds, Kind, Names, People, Places, Recall, Ref, TaskCards, Things,
    Waiting,
};
use super::text::tokens;
use super::{AtHand, Audience, Author, Handle, Marks, PointOfView, Pos, Sizes};
use crate::exchange::{self, Opener, Root, OWNER};
use crate::ladder::did;
use crate::node::Body;
use crate::store::Store;

/// The most of a person's timeline [`Places::places_of`] reads.
const PLACES_READ: usize = 500;
/// A tool call's stub, in tokens: one line naming the call.
const STUB_TOKENS: u64 = 30;

/// The sources over `store`. `mark` is the turn's place and its `/new` mark, and `recalled` the nodes
/// today's recall found for the turn's message, both read by the turn.
pub struct StoreSources<'a> {
    pub store: &'a Store,
    pub mark: Option<(&'a str, Pos)>,
    pub recalled: Vec<String>,
}

fn json_tokens(v: &serde_json::Value) -> u64 {
    tokens(&v.to_string())
}

impl Exchanges for StoreSources<'_> {
    fn facts(&self, turn_id: &str) -> anyhow::Result<Option<ExchangeFacts>> {
        let Some(ex) = exchange::exchange(self.store, turn_id)? else {
            return Ok(None);
        };
        let (mut his, mut reply, mut tools, mut stubs, mut outside) = (0, 0, 0, 0, false);
        for (_, n) in self.store.transcript(&ex.session_id)? {
            if n.turn_id.as_deref() != Some(turn_id) {
                continue;
            }
            match &n.body {
                Body::UserMessage { text, .. } => his += tokens(text),
                Body::AssistantMessage { blocks, .. } => {
                    for b in blocks {
                        match b["type"].as_str() {
                            Some("text") => reply += tokens(b["text"].as_str().unwrap_or_default()),
                            Some("tool_use") => {
                                tools += json_tokens(&b["input"]);
                                stubs += STUB_TOKENS;
                            }
                            _ => {}
                        }
                    }
                }
                Body::ToolResult {
                    content, external, ..
                } => {
                    tools += tokens(content);
                    outside |= external.is_some();
                }
                _ => {}
            }
        }
        let author = match (ex.opened_by, ex.person.as_deref()) {
            (Opener::Message | Opener::Prompt, Some(OWNER)) => Author::His {
                may_authorise: false,
            },
            (Opener::Message | Opener::Prompt, Some(_)) => Author::Other,
            (Opener::Message | Opener::Prompt | Opener::Unknown, None) => Author::Unknown,
            _ => Author::Theseus,
        };
        Ok(Some(ExchangeFacts {
            marks: Marks {
                place: Some(ex.place.clone()),
                audience: match ex.private {
                    true => Audience::Private,
                    false => Audience::Shared,
                },
                author,
                outside,
                sealed: false,
                veiled: false,
            },
            place: ex.place,
            person: ex.person,
            kind: Kind::Unknown,
            standing: false,
            waiting: false,
            his,
            reply,
            tools,
            stubs,
            did: did::did(self.store, turn_id)?.map(|d| tokens(&d.line)),
            handle: Handle(turn_id.to_string()),
        }))
    }
}

impl Folds for StoreSources<'_> {
    fn sizes(&self, node: &FoldNode) -> anyhow::Result<Sizes> {
        let mut words = 0;
        for id in &node.his_words {
            let said = self.store.get_node(id)?.and_then(|(_, n)| did::words(&n));
            words += said.map_or(0, |w| tokens(&w));
        }
        let whole = tokens(&node.title) + tokens(&node.account) + words;
        Ok(Sizes {
            whole,
            spoken: whole,
            line: tokens(&node.title) + tokens(&node.account),
        })
    }
}

impl Places for StoreSources<'_> {
    fn new_mark(&self, place: &str) -> anyhow::Result<Option<Pos>> {
        Ok(self
            .mark
            .as_ref()
            .filter(|(p, _)| *p == place)
            .map(|(_, m)| m.clone()))
    }

    fn places_of(&self, person: &str) -> anyhow::Result<Vec<String>> {
        let mut places = BTreeSet::new();
        for t in exchange::newest(self.store, Root::Person(person), PLACES_READ)? {
            if let Some(ex) = exchange::exchange(self.store, &t.exchange)? {
                places.insert(ex.place);
            }
        }
        Ok(places.into_iter().collect())
    }
}

impl Waiting for StoreSources<'_> {
    fn waiting(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Things for StoreSources<'_> {
    fn named_in(&self, _: &str) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
    fn card(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
    fn at_hand(&self, _: &AtHand) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Names for StoreSources<'_> {
    fn named(&self, _: &str) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

impl TaskCards for StoreSources<'_> {
    fn joined(&self, _: &PointOfView, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Recall for StoreSources<'_> {
    /// Each recalled node as its exchange, in the line form (his words and the `did` line), once. A node
    /// of history the ledger holds no exchange for has no marks, and is not offered (the gate would deny it).
    fn recall(&self, _: &PointOfView, _: &str) -> anyhow::Result<Vec<Ref>> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for id in &self.recalled {
            let Some(turn) = self.store.get_node(id)?.and_then(|(_, n)| n.turn_id) else {
                continue;
            };
            if !seen.insert(turn.clone()) {
                continue;
            }
            let (Some(ex), Some(f)) = (exchange::exchange(self.store, &turn)?, self.facts(&turn)?)
            else {
                continue;
            };
            let pos = Pos {
                at_ms: ex.start_ms,
                id: turn,
            };
            let line = f.his + f.did.unwrap_or(0);
            out.push(Ref {
                range: super::SourceRange {
                    space: super::Space::Exchanges { place: f.place },
                    first: pos.clone(),
                    last: pos,
                },
                marks: f.marks,
                sizes: Sizes {
                    whole: f.his + f.reply,
                    spoken: f.his + f.reply,
                    line,
                },
                handle: f.handle,
                form: super::Form::Line,
                unit: None,
            });
        }
        Ok(out)
    }
}

impl People for StoreSources<'_> {
    fn about(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}
