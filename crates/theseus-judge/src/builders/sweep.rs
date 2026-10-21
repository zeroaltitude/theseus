//! `same_thing.v1` and `two_things.v1` (theseus-nzzz4): what the ontology's
//! nightly sweep asks Jev about its own categories. Both are off every
//! turn's path.
//!
//! - **`same_thing`**: two categories of one kind, each with its name, its
//!   path, its description and up to [`TITLES`] of its sessions' titles. One
//!   Noul: are they one thing, named two ways?
//! - **`two_things`**: one category, with a sample of at most [`SAMPLE`] of
//!   its sessions' titles, numbered. One Noul asks whether they fall into two
//!   things; the first thing is the one session 1 is about. Two Choices name
//!   the two over the names code mined from the titles (at most ten), and a
//!   per-item Noul says of each sampled session whether it is about the
//!   first.
//!
//! Every string is scrubbed and clipped here, as in every state.

use super::*;
use crate::mine::{self, Source as Mined};

/// The session titles a category shows in `same_thing`.
pub const TITLES: usize = 5;
/// The sessions `two_things` samples at most.
pub const SAMPLE: usize = 12;
/// The sampled sessions the first per-item Noul asks about; the rest are the
/// second's (a per-item Noul asks at most ten).
pub const FIRST_ASKED: usize = 10;
const TITLE_CHARS: usize = 120;

/// A category as the sweep shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepCategory {
    pub name: String,
    /// With its parents (`theseus › ontology`).
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub description: String,
    /// Titles of sessions that hold it, newest first.
    #[serde(default)]
    pub titles: Vec<String>,
}

/// `same_thing.v1`'s input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SameThingInput {
    pub kind: String,
    pub first: SweepCategory,
    pub second: SweepCategory,
}

/// `two_things.v1`'s input: the category, and its sampled sessions' titles
/// (session 1 is the first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwoThingsInput {
    pub kind: String,
    pub category: SweepCategory,
    /// The sampled sessions, `(session id, title)`, in the order shown.
    pub sessions: Vec<(String, String)>,
}

impl TwoThingsInput {
    /// The sessions the state shows, at most [`SAMPLE`].
    pub fn sample(&self) -> &[(String, String)] {
        &self.sessions[..self.sessions.len().min(SAMPLE)]
    }

    /// The names mined from the sampled titles, at most ten: what
    /// `first_name` and `second_name` choose among (`n1` is the first).
    pub fn names(&self) -> Vec<String> {
        let titles: Vec<&str> = self.sample().iter().map(|(_, t)| t.as_str()).collect();
        let message = titles.join(".\n");
        mine::mine(
            &mine::Text {
                message: &message,
                ..mine::Text::default()
            },
            &[Mined::Paths, Mined::Hosts, Mined::Capitalised],
        )
    }
}

fn shown(c: &Clipper, cat: &SweepCategory) -> Value {
    json!({
        "name": c.clip(&cat.name, 100),
        "path": c.clip(&cat.path, 200),
        "description": c.clip(&cat.description, 300),
        "sessions": cat.titles.iter().take(TITLES).map(|t| c.clip(t, TITLE_CHARS)).collect::<Vec<_>>(),
    })
}

pub fn same_thing(i: &SameThingInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("same_thing", SAME_THING_VERSION, cap, scrub);
    b.scalar("kind", c.clip(&i.kind, 40));
    b.scalar("first", shown(&c, &i.first))
        .cut_if("first", c.cut());
    b.scalar("second", shown(&c, &i.second))
        .cut_if("second", c.cut());
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

pub fn two_things(i: &TwoThingsInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let clip = |s: &str, n: usize| clip_with(scrub, s, n);
    let mut b = StateBuilder::new("two_things", TWO_THINGS_VERSION, cap, scrub);
    b.scalar("kind", c.clip(&i.kind, 40));
    let cat = &i.category;
    b.scalar(
        "category",
        json!({"name": c.clip(&cat.name, 100), "path": c.clip(&cat.path, 200),
               "description": c.clip(&cat.description, 300)}),
    )
    .cut_if("category", c.cut());
    let sample = i.sample();
    let lines: Vec<Value> = sample
        .iter()
        .enumerate()
        .map(|(n, (_, t))| json!(format!("{}: {}", n + 1, c.clip(t, TITLE_CHARS))))
        .collect();
    b.list_head("sessions", 10, share(cap, 70), lines, 0)
        .cut_if("sessions", c.cut());
    let names = i.names();
    let mut dynamic = Dynamic::default();
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
    let item = |(n, (id, t)): (usize, &(String, String))| Item {
        key: id.clone(),
        text: format!("{}, “{}”", n + 1, clip(t, TITLE_CHARS)),
    };
    let numbered: Vec<Item> = sample.iter().enumerate().map(item).collect();
    dynamic.sources.insert(
        Source::Sessions,
        numbered.iter().take(FIRST_ASKED).cloned().collect(),
    );
    dynamic.sources.insert(
        Source::MoreSessions,
        numbered.iter().skip(FIRST_ASKED).cloned().collect(),
    );
    if !names.is_empty() {
        let shown: Vec<Value> = names.iter().map(|n| json!(c.clip(n, 60))).collect();
        b.list_head("mined_names", 8, share(cap, 10), shown, 0)
            .cut_if("mined_names", c.cut());
    }
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}
