//! `same_kind.v1` and `name_it.v1` (theseus-63tfk.1): what the ontology's
//! nightly sweep asks Jev about the kinds it mints and the names it keeps.
//! Both are off every turn's path, and Jev writes no name: every name is a
//! Choice among names code mined (titles, paths, hosts, capitalised terms).
//!
//! - **`same_kind`**: two groups of sessions that said `other` to
//!   `kind_of_thing`, each with the names mined from its messages and up to
//!   [`TITLES`] of its sessions' titles. One Noul: do they talk about one kind
//!   of thing?
//! - **`name_it`**: a group of sessions, their titles, and the mined names
//!   (at most ten). One Choice picks the name that fits them best, or `none`.
//!   With a `current` name (a rename) `none` keeps it; without one (a kind to
//!   mint) `none` leaves the naming to the sweep's rule.
//!
//! Every string is scrubbed and clipped here, as in every state.

use super::*;

/// The session titles a group shows.
pub const TITLES: usize = 12;
const NAMES: usize = 10;
const TITLE_CHARS: usize = 120;
const NAME_CHARS: usize = 60;

/// One side of a `same_kind` pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindSide {
    /// The names mined from the sessions' messages.
    pub names: Vec<String>,
    /// Titles of sessions that said them.
    #[serde(default)]
    pub titles: Vec<String>,
}

/// `same_kind.v1`'s input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SameKindInput {
    pub first: KindSide,
    pub second: KindSide,
}

/// `name_it.v1`'s input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameItInput {
    pub kind: String,
    /// The name it has now, for a rename.
    #[serde(default)]
    pub current: Option<String>,
    /// What the category says of itself, for a rename.
    #[serde(default)]
    pub description: String,
    /// Titles of the sessions that hold it, newest first.
    #[serde(default)]
    pub titles: Vec<String>,
    /// The names to choose among (`n1` is the first), best first.
    pub names: Vec<String>,
}

fn side(c: &Clipper, s: &KindSide) -> Value {
    json!({
        "names": s.names.iter().take(NAMES).map(|n| c.clip(n, NAME_CHARS)).collect::<Vec<_>>(),
        "sessions": s.titles.iter().take(TITLES).map(|t| c.clip(t, TITLE_CHARS)).collect::<Vec<_>>(),
    })
}

pub fn same_kind(i: &SameKindInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("same_kind", SAME_KIND_VERSION, cap, scrub);
    b.scalar("first", side(&c, &i.first))
        .cut_if("first", c.cut());
    b.scalar("second", side(&c, &i.second))
        .cut_if("second", c.cut());
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

pub fn name_it(i: &NameItInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("name_it", NAME_IT_VERSION, cap, scrub);
    b.scalar("kind", c.clip(&i.kind, 40));
    if let Some(current) = &i.current {
        b.scalar("current", c.clip(current, 100))
            .cut_if("current", c.cut());
        b.scalar("description", c.clip(&i.description, 300));
    }
    let lines: Vec<Value> = i
        .titles
        .iter()
        .take(TITLES)
        .map(|t| json!(c.clip(t, TITLE_CHARS)))
        .collect();
    b.list_head("sessions", 10, share(cap, 70), lines, 0)
        .cut_if("sessions", c.cut());
    let names: Vec<&String> = i.names.iter().take(NAMES).collect();
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Names,
        names
            .iter()
            .enumerate()
            .map(|(n, name)| Item {
                key: format!("n{}", n + 1),
                text: clip_with(scrub, name, NAME_CHARS),
            })
            .collect(),
    );
    if !names.is_empty() {
        let shown: Vec<Value> = names.iter().map(|n| json!(c.clip(n, NAME_CHARS))).collect();
        b.list_head("mined_names", 8, share(cap, 10), shown, 0)
            .cut_if("mined_names", c.cut());
    }
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}
