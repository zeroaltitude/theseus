//! Scoped names: a table of the names and aliases of tasks (titles and ids)
//! and topics (the
//! ontology's category names), built from the stores that hold them, and a
//! matcher of a person's words against it. No word list and no pattern: a
//! name is found only when its words are said.
//!
//! - **A qualified name wins**: the thing's own key (`task:<id>`, a category's id such as `topic:ledger`) or a task's bare id.
//!   It names one thing, live or not.
//! - **An unqualified alias** (a title, a slug, a category's name, matched as
//!   whole words, case ignored) is accepted only when exactly one live thing
//!   holds it. Otherwise every thing that holds it is returned, as
//!   candidates, the live ones first.
//! - A longer name said wins over a shorter one inside it ("Lantern Ledger"
//!   over "Ledger"); each word of the message belongs to one match at most.

use std::collections::{BTreeMap, BTreeSet};

use super::card::{Admits, Source};

/// The words of an alias, at most: a longer title is matched by its key or
/// by a shorter alias, never whole.
pub const ALIAS_WORDS: usize = 8;
/// An alias shorter than this, in characters, is no name (`a`, `ok`).
pub const ALIAS_CHARS: usize = 3;

/// What a thing of the table is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Task,
    /// A category of the ontology (a topic, an event, a resource, …).
    Topic,
    /// A numbered item (`names::items`): no record holds it, only its
    /// timeline; never in the table.
    Item,
}

/// One thing of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its key, as the touch index names it: `task:<id>`,
    /// or a category's id.
    pub thing: String,
    pub kind: Kind,
    pub title: String,
    /// Still in play: an open task, a category not retired
    /// or merged.
    pub live: bool,
}

/// How a name said was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The thing's own key, or a task's id.
    Qualified,
    /// An alias exactly one live thing holds.
    Unique,
    /// An alias more than one thing holds, or none live: candidates only.
    Candidates,
}

/// A name found in a person's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The words as they were said.
    pub said: String,
    pub how: How,
    /// The thing's key, or the candidates' keys, live ones first.
    pub things: Vec<String>,
}

/// The table: every thing, by key, and every alias's things.
#[derive(Debug, Default, Clone)]
pub struct Table {
    entries: BTreeMap<String, Entry>,
    /// A task's bare id, to its key.
    ids: BTreeMap<String, String>,
    /// An alias's lower-case words joined by one space, to the keys holding it.
    aliases: BTreeMap<String, BTreeSet<String>>,
}

/// A text's words: runs of letters and digits, lower-cased, with where each
/// begins and ends in bytes.
fn words(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((text[s..i].to_lowercase(), s, i));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The text's tokens between whitespace, with surrounding punctuation taken
/// off: where a qualified name may stand.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace().map(|t| {
        t.trim_matches(|c: char| "\"'`()[]{}<>,;!?".contains(c))
            .trim_end_matches(['.', ':'])
    })
}

impl Table {
    /// Add `entry`, known by each of `aliases`; a task is also known by its
    /// bare `id`.
    pub fn add(&mut self, entry: Entry, aliases: &[&str], id: Option<&str>) {
        for alias in aliases {
            let w: Vec<String> = words(alias).into_iter().map(|(w, ..)| w).collect();
            let joined = w.join(" ");
            if w.is_empty() || w.len() > ALIAS_WORDS || joined.chars().count() < ALIAS_CHARS {
                continue;
            }
            self.aliases
                .entry(joined)
                .or_default()
                .insert(entry.thing.clone());
        }
        if let Some(id) = id {
            self.ids.insert(id.to_string(), entry.thing.clone());
        }
        self.entries.insert(entry.thing.clone(), entry);
    }

    pub fn get(&self, thing: &str) -> Option<&Entry> {
        self.entries.get(thing)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The names in `text`, in the order they were said: each qualified name
    /// once, then each alias's match not inside a longer one.
    pub fn find(&self, text: &str) -> Vec<Found> {
        let mut out: Vec<(usize, Found)> = Vec::new();
        let mut sure = BTreeSet::new();
        let mut at = 0;
        for t in tokens(text) {
            let pos = text[at..].find(t).map_or(at, |p| at + p);
            at = pos + t.len();
            let key = self
                .entries
                .contains_key(t)
                .then(|| t.to_string())
                .or_else(|| self.ids.get(t).cloned());
            if let Some(key) = key.filter(|k| sure.insert(k.clone())) {
                out.push((
                    pos,
                    Found {
                        said: t.into(),
                        how: How::Qualified,
                        things: vec![key],
                    },
                ));
            }
        }
        for (start, end, keys) in self.alias_spans(text) {
            let things = self.ranked(keys);
            if things.iter().any(|k| sure.contains(k)) {
                continue;
            }
            let live: Vec<String> = things
                .iter()
                .filter(|k| self.entries[*k].live)
                .cloned()
                .collect();
            // One live holder is the one meant, whatever is gone that held it.
            let (how, things) = match live.len() {
                1 => (How::Unique, live),
                _ => (How::Candidates, things),
            };
            out.push((
                start,
                Found {
                    said: text[start..end].into(),
                    how,
                    things,
                },
            ));
        }
        out.sort_by_key(|(p, _)| *p);
        out.into_iter().map(|(_, f)| f).collect()
    }

    /// Keys, the live ones first, then by key.
    fn ranked(&self, keys: &BTreeSet<String>) -> Vec<String> {
        let mut v: Vec<String> = keys.iter().cloned().collect();
        v.sort_by_key(|k| (!self.entries[k].live, k.clone()));
        v
    }

    /// Each alias said, longest first, with no word in two: its byte span and
    /// its holders.
    fn alias_spans(&self, text: &str) -> Vec<(usize, usize, &BTreeSet<String>)> {
        let w = words(text);
        let mut hits = Vec::new();
        for i in 0..w.len() {
            for len in 1..=ALIAS_WORDS.min(w.len() - i) {
                let joined = w[i..i + len]
                    .iter()
                    .map(|(s, ..)| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Some(keys) = self.aliases.get(&joined) {
                    hits.push((len, i, keys));
                }
            }
        }
        hits.sort_by_key(|(len, i, _)| (std::cmp::Reverse(*len), *i));
        let mut used = vec![false; w.len()];
        let mut out = Vec::new();
        for (len, i, keys) in hits {
            if used[i..i + len].iter().any(|u| *u) {
                continue;
            }
            used[i..i + len].iter_mut().for_each(|u| *u = true);
            out.push((w[i].1, w[i + len - 1].2, keys));
        }
        out
    }
}

/// Admits every record: the table of everything the stores hold, for a
/// caller whose point of view sees all of it. Any other caller builds its
/// table with [`build_for`] and its own gate.
pub struct Whole;

impl Admits for Whole {
    fn fact(&self, _: &theseus_protocol::StandingRecord) -> bool {
        true
    }
    fn exchange(&self, _: &crate::exchange::Exchange) -> bool {
        true
    }
    fn thing(&self, _: Source<'_>) -> bool {
        true
    }
}

/// [`build_for`] with [`Whole`]: every thing the stores hold.
pub fn build(core: &crate::rpc::Core) -> anyhow::Result<Table> {
    build_for(core, &Whole)
}

/// The table of what the stores hold now and `gate` admits: the
/// tasks and the ontology's categories of the
/// kinds `onto.v1` files under, a project apart (a project is a task). A
/// thing the gate refuses is left out before any alias is resolved, so it is
/// no match, no candidate, and makes no alias it shares ambiguous.
pub fn build_for(core: &crate::rpc::Core, gate: &dyn Admits) -> anyhow::Result<Table> {
    let mut t = Table::default();
    let tasks = crate::task_graph::all_shown(&core.store, &core.kernel)?;
    for task in tasks.iter().filter(|t| gate.thing(Source::Task(t))) {
        t.add(
            Entry {
                thing: format!("task:{}", task.id),
                kind: Kind::Task,
                title: task.title.clone(),
                live: !task.state.is_closed(),
            },
            &[&task.title],
            Some(&task.id),
        );
    }
    let o = core.runner.ontology.snapshot(&core.store)?;
    let filed = |k: &str| k != "project" && theseus_judge::builders::ONTO_KINDS.contains(&k);
    for c in o
        .categories()
        .filter(|c| filed(c.kind()) && gate.thing(Source::Topic(c)))
    {
        t.add(
            Entry {
                thing: c.id.to_string(),
                kind: Kind::Topic,
                title: c.name.clone(),
                live: !c.is_gone(),
            },
            &[&c.name],
            None,
        );
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(thing: &str, kind: Kind, title: &str, live: bool) -> Entry {
        Entry {
            thing: thing.into(),
            kind,
            title: title.into(),
            live,
        }
    }

    fn table() -> Table {
        let mut t = Table::default();
        t.add(
            entry("topic:lantern", Kind::Topic, "Lantern Ledger", true),
            &["Lantern Ledger", "lantern"],
            None,
        );
        t.add(
            entry("topic:ledger", Kind::Topic, "Ledger", true),
            &["Ledger"],
            None,
        );
        t.add(
            entry("task:tsk_9f2", Kind::Task, "Ledger", true),
            &["Ledger"],
            Some("tsk_9f2"),
        );
        t.add(
            entry("topic:kiln", Kind::Topic, "Kiln", true),
            &["Kiln"],
            None,
        );
        t.add(
            entry("task:tsk_old", Kind::Task, "Kiln", false),
            &["Kiln"],
            Some("tsk_old"),
        );
        t.add(entry("topic:ok", Kind::Topic, "ok", true), &["ok"], None);
        t
    }

    fn found(text: &str) -> Vec<(String, How, Vec<String>)> {
        table()
            .find(text)
            .into_iter()
            .map(|f| (f.said, f.how, f.things))
            .collect()
    }

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| (*s).to_string()).collect()
    }

    /// KEY: a qualified name wins over an alias another thing shares.
    #[test]
    fn a_qualified_name_wins() {
        assert_eq!(
            found("about topic:ledger, and tsk_9f2."),
            vec![
                ("topic:ledger".into(), How::Qualified, v(&["topic:ledger"])),
                ("tsk_9f2".into(), How::Qualified, v(&["task:tsk_9f2"])),
            ]
        );
    }

    /// An alias two live things hold is returned as both, candidates; one
    /// only one live thing holds is accepted.
    #[test]
    fn an_alias_is_accepted_only_when_one_live_thing_holds_it() {
        assert_eq!(
            found("the ledger again"),
            vec![(
                "ledger".into(),
                How::Candidates,
                v(&["task:tsk_9f2", "topic:ledger"])
            )]
        );
        assert_eq!(
            found("Kiln!"),
            vec![("Kiln".into(), How::Unique, v(&["topic:kiln"]))]
        );
    }

    /// A longer name wins over the shorter one inside it, and an alias too
    /// short to be a name is none.
    #[test]
    fn the_longer_name_said_wins_and_a_short_alias_is_none() {
        assert_eq!(
            found("ok, the Lantern Ledger"),
            vec![("Lantern Ledger".into(), How::Unique, v(&["topic:lantern"]))]
        );
    }

    /// Words inside other words are no name.
    #[test]
    fn a_name_is_matched_as_whole_words() {
        assert!(found("ledgers and kilnwork").is_empty());
    }
}
