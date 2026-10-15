//! route.v5's persona and seal questions (theseus-c1uz): what the shared
//! `inbound` state adds for them, and the items their options come from.
//!
//! - **personas**: one per configured persona the place may speak as, each
//!   with its specialization card (never its files: those sit in the cached
//!   system block). Fewer than two leaves nothing to choose, so the state
//!   carries none and the question is not asked.
//! - **speaker**: the persona that answered last, which the question's
//!   `prior` keeps.
//! - **the thread** the message arrives in: its title, goal, and its last
//!   three turns, each with the persona that answered it.
//!
//! All of it is left out when absent, so a stored state from before is byte
//! for byte what it was.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{first_line, left_out, share, Clipper};
use crate::pack::{Dynamic, Item, Source};
use crate::state::StateBuilder;

/// The most personas the question offers.
pub const PERSONAS: usize = 12;
/// The most turns of a thread the state shows.
pub const THREAD_TURNS: usize = 3;
/// A turn's words in the state: about 120 tokens.
const TURN_CHARS: usize = 480;
/// A card's words in the state and as an option: about 60 tokens, with room.
const CARD_CHARS: usize = 400;

/// One persona the message may be answered as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaInput {
    /// The option's id: the `[personas]` key.
    pub id: String,
    /// What the person calls it.
    pub name: String,
    /// Its specialization and background, about 60 tokens.
    pub card: String,
}

/// One turn of the thread the message arrives in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnInput {
    /// What the person said.
    pub said: String,
    /// The `[personas]` key that answered, none for the built-in default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
}

/// The thread a message arrives in, for `seal` and the persona's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadNow {
    pub title: String,
    #[serde(default)]
    pub goal: String,
    /// The last turns, oldest first.
    #[serde(default)]
    pub turns: Vec<TurnInput>,
}

/// The persona and thread fields of the inbound state.
pub(super) fn state(
    b: &mut StateBuilder,
    c: &Clipper,
    cap: u64,
    i: &super::InboundInput,
) -> Vec<Item> {
    let personas = offered(&i.personas);
    if personas.len() >= 2 {
        if let Some(s) = i.speaker.as_deref() {
            b.scalar("speaker", c.clip(s, 40))
                .cut_if("speaker", c.cut());
        }
        let items: Vec<Value> = personas
            .iter()
            .map(|p| {
                json!({
                    "id": c.clip(&p.id, 40),
                    "name": c.clip(&p.name, 40),
                    "card": c.clip(&p.card, CARD_CHARS),
                })
            })
            .collect();
        b.list_head(
            "personas",
            8,
            share(cap, 20),
            items,
            left_out(&i.personas, personas.len()),
        )
        .cut_if("personas", c.cut());
    }
    let Some(t) = &i.thread_now else {
        return Vec::new();
    };
    b.scalar("thread_title", c.clip(&t.title, 80))
        .cut_if("thread_title", c.cut());
    if !t.goal.trim().is_empty() {
        b.scalar("thread_goal", c.clip(first_line(&t.goal), 160))
            .cut_if("thread_goal", c.cut());
    }
    let recent = &t.turns[t.turns.len().saturating_sub(THREAD_TURNS)..];
    if !recent.is_empty() {
        let items: Vec<Value> = recent
            .iter()
            .map(|turn| {
                let mut v = json!({"said": c.clip(&turn.said, TURN_CHARS)});
                if let Some(p) = &turn.answered_by {
                    v["answered_by"] = json!(c.clip(p, 40));
                }
                v
            })
            .collect();
        b.list_after(
            "thread_recent",
            7,
            share(cap, 25),
            items,
            left_out(&t.turns, recent.len()),
        )
        .cut_if("thread_recent", c.cut());
    }
    vec![Item {
        key: "thread".into(),
        text: c.clip(&t.title, 80),
    }]
}

/// The personas the question offers: those with something to read, at most
/// [`PERSONAS`].
fn offered(all: &[PersonaInput]) -> Vec<&PersonaInput> {
    all.iter()
        .filter(|p| !p.card.trim().is_empty())
        .take(PERSONAS)
        .collect()
}

/// The dynamic items: the personas to choose among (two or more), and the
/// thread the message is in.
pub(super) fn dynamic(
    dynamic: &mut Dynamic,
    i: &super::InboundInput,
    clip: &dyn Fn(&str, usize) -> String,
    in_thread: Vec<Item>,
) {
    let personas = offered(&i.personas);
    if personas.len() >= 2 {
        dynamic.sources.insert(
            Source::Personas,
            personas
                .iter()
                .map(|p| Item {
                    key: p.id.clone(),
                    text: format!("{}: {}", clip(&p.name, 40), clip(&p.card, CARD_CHARS)),
                })
                .collect(),
        );
    }
    if !in_thread.is_empty() {
        dynamic.sources.insert(Source::ThreadNow, in_thread);
    }
}
