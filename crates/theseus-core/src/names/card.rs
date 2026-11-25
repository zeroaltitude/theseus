//! A thing's card, as one read: its title, and at most [`LINES`] lines under
//! it of what it is, its current facts, its open tasks, the owner's guidance
//! for it and the newest exchanges of its timeline. A turn in a place set to
//! `view` lays it beside the message (`crate::view::sources_names`).
//!
//! - **Facts** are the owner's standing records whose scope is the thing's
//!   own session: a task's session. A record that ended (revoked, replaced, lapsed) is
//!   dropped, as `standing::live` drops it, and none is read that the
//!   caller's [`Admits`] refuses. A category has no facts of its own yet: no
//!   stored record names a fact's subject.
//! - **Open tasks**: a task's children, and a topic's (tasks whose session is filed under it).
//! - **Guidance**: a category's, as the ontology holds it.
//! - **Exchanges**: the newest [`EXCHANGES`] the caller admits, from the
//!   touch index, each with the words that opened it, cut by code.
//!
//! A cut says what it left out: past [`FACTS`] facts or [`TASKS`] tasks the
//! last line counts the rest, of what the gate admitted only.
//!
//! The caller's gate ([`Admits`]) is asked first of the thing's own record (a
//! refused thing has no card at all), then of every task, guidance, standing
//! record and exchange before it is read.

use theseus_ontology::{Category, CategoryId, Guidance, Ontology};
use theseus_protocol::tasks::TaskRecord;
use theseus_protocol::{StandingRecord, StandingScope};

use super::table::{Entry, Kind, Table};
use crate::exchange::{self, Exchange, Root};

/// The lines under the title, at most.
pub const LINES: usize = 12;
/// Facts, open tasks and exchanges a card shows, at most.
pub const FACTS: usize = 5;
pub const TASKS: usize = 2;
pub const EXCHANGES: usize = 3;
/// A page of a thing's timeline, read until [`EXCHANGES`] are admitted.
const PAGE: usize = 24;
/// A line's text, at most, in characters.
const LINE_CHARS: usize = 200;
const ASK_CHARS: usize = 120;

/// What the caller's point of view admits, asked of every record the card
/// or the name table would read, before it is read. A record it is not
/// asked about is never shown; the methods a gate does not give deny.
pub trait Admits {
    fn fact(&self, r: &StandingRecord) -> bool;
    fn exchange(&self, e: &Exchange) -> bool;
    /// The thing itself, by the record that holds it: refused, the thing has
    /// no card and is no name (a sealed thread's exception is the gate's).
    fn thing(&self, _s: Source<'_>) -> bool {
        false
    }
    /// An open task shown on a card.
    fn task(&self, _t: &TaskRecord) -> bool {
        false
    }
    /// The owner's guidance for a category.
    fn guidance(&self, _g: &Guidance) -> bool {
        false
    }
    /// Who wrote the words that opened an admitted exchange, when they are not the owner's: the card keeps
    /// them as outside text, under this name. None: shown as his words.
    fn by(&self, _e: &Exchange) -> Option<&'static str> {
        None
    }
}

/// The record a thing of the table is held by.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    Task(&'a TaskRecord),
    Topic(&'a Category),
}

/// The record that holds `entry` now, if any.
fn source_of<'a>(entry: &Entry, tasks: &'a [TaskRecord], o: &'a Ontology) -> Option<Source<'a>> {
    let thing = entry.thing.as_str();
    match entry.kind {
        Kind::Task => {
            let id = thing.strip_prefix("task:")?;
            tasks.iter().find(|t| t.id == id).map(Source::Task)
        }
        Kind::Topic => o
            .category(&CategoryId::parse(thing).ok()?)
            .map(Source::Topic),
        Kind::Item => None,
    }
}

/// One card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub thing: String,
    pub title: String,
    /// At most [`LINES`].
    pub lines: Vec<String>,
    /// What the lines came from, by id: standing records, tasks, exchanges'
    /// turns.
    pub facts: Vec<String>,
    pub tasks: Vec<String>,
    pub exchanges: Vec<String>,
}

/// An exchange as the card shows it: the record and its opening words.
pub struct Said {
    pub exchange: Exchange,
    pub ask: Option<String>,
    /// Its author, when not the owner ([`Admits::by`]).
    pub by: Option<&'static str>,
}

/// What a card is made from, read once.
pub struct Sources<'a> {
    pub entry: &'a Entry,
    /// What it is about: a thread's goal, a task's objective, a category's
    /// description.
    pub about: &'a str,
    /// The live standing records of the thing's scope, oldest first.
    pub facts: Vec<&'a StandingRecord>,
    /// Its open tasks.
    pub tasks: Vec<&'a TaskRecord>,
    pub guidance: Option<&'a str>,
    /// Its newest exchanges, newest first.
    pub said: Vec<Said>,
}

fn cut(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::scrub::safe::head_marked(&one, n)
}

/// Up to `most` of `items` as lines, the last one counting the rest when
/// there are more.
fn some<T>(items: &[T], most: usize, line: impl Fn(&T) -> String, what: &str) -> Vec<String> {
    if items.len() <= most {
        return items.iter().map(line).collect();
    }
    let mut out: Vec<String> = items[..most - 1].iter().map(line).collect();
    out.push(format!("… and {} more {what}", items.len() - (most - 1)));
    out
}

/// The card `src` makes, the caller's gate already applied.
pub fn compose(src: Sources<'_>) -> Card {
    let e = src.entry;
    let kind = match e.kind {
        Kind::Task => "Task",
        Kind::Topic => "Topic",
        Kind::Item => "Item",
    };
    let mut lines = Vec::new();
    if !src.about.trim().is_empty() {
        lines.push(cut(src.about, LINE_CHARS));
    }
    lines.extend(some(
        &src.facts,
        FACTS,
        |r| format!("Fact: {}", cut(&r.text, LINE_CHARS)),
        "facts",
    ));
    lines.extend(some(
        &src.tasks,
        TASKS,
        |t| {
            format!(
                "Open task: {} ({}, {})",
                cut(&t.title, LINE_CHARS),
                t.id,
                t.state.as_str()
            )
        },
        "open tasks",
    ));
    if let Some(g) = src.guidance.filter(|g| !g.trim().is_empty()) {
        lines.push(format!("Guidance: {}", cut(g, LINE_CHARS)));
    }
    for s in src.said.iter().take(EXCHANGES) {
        let ex = &s.exchange;
        let words = match (s.ask.as_deref(), s.by) {
            (None, _) => "(no words)".into(),
            (Some(a), None) => format!("\"{}\"", cut(a, ASK_CHARS)),
            (Some(a), Some(by)) => {
                crate::view::text::labels::wrap_outside(Some(by), &cut(a, ASK_CHARS))
            }
        };
        lines.push(format!(
            "{} in {}: {words}",
            crate::recall::stamp::date(ex.start_ms),
            ex.place
        ));
    }
    lines.truncate(LINES);
    Card {
        thing: e.thing.clone(),
        title: format!(
            "{kind}: {}{} ({})",
            e.title,
            if e.live { "" } else { " (closed)" },
            e.thing
        ),
        lines,
        facts: src.facts.iter().map(|r| r.id.clone()).collect(),
        tasks: src.tasks.iter().map(|t| t.id.clone()).collect(),
        exchanges: src
            .said
            .iter()
            .take(EXCHANGES)
            .map(|s| s.exchange.turn_id.clone())
            .collect(),
    }
}

/// The newest exchanges of `thing` that `gate` admits, newest first, with
/// their opening words: its timeline paged back until [`EXCHANGES`] are
/// admitted or it ends, so what the gate refuses never crowds out what it
/// admits.
fn said(store: &crate::store::Store, thing: &str, gate: &dyn Admits) -> anyhow::Result<Vec<Said>> {
    use theseus_store::Store as _;
    let prefix = format!(
        "{}{}\u{1}",
        exchange::TOUCH_PREFIX,
        Root::Thing(thing).key()
    );
    let mut from = prefix.clone();
    let mut out = Vec::new();
    loop {
        let page = store.inner().first_with_prefix_from(
            theseus_store::kinds::META,
            &prefix,
            &from,
            PAGE,
        )?;
        for r in &page {
            let t: exchange::Touch = r.decode()?;
            let Some(ex) = exchange::exchange(store, &t.exchange)? else {
                continue;
            };
            if !gate.exchange(&ex) {
                continue;
            }
            let ask = match &ex.ask {
                Some(id) => store.get_node(id)?.map(|(_, n)| crate::recall::text_of(&n)),
                None => None,
            };
            let by = gate.by(&ex);
            out.push(Said {
                exchange: ex,
                ask,
                by,
            });
            if out.len() == EXCHANGES {
                return Ok(out);
            }
        }
        // The next page starts just past this one's last key.
        match page.last().and_then(|r| r.key.as_deref()) {
            Some(last) if page.len() == PAGE => from = format!("{last}\u{0}"),
            _ => return Ok(out),
        }
    }
}

/// The card of `thing` (a key of `table`), as the stores hold it now and as
/// `gate` admits it; none for a thing the table does not hold, or whose own
/// record the gate refuses. Every part is admitted before it is read: its
/// open tasks, its guidance, its facts (what is current is decided inside
/// the admitted history), and its exchanges.
pub fn card(
    core: &crate::rpc::Core,
    table: &Table,
    thing: &str,
    gate: &dyn Admits,
) -> anyhow::Result<Option<Card>> {
    let Some(entry) = table.get(thing) else {
        return Ok(None);
    };
    let tasks = crate::task_graph::all_shown(&core.store, &core.kernel)?;
    let o = core.runner.ontology.snapshot(&core.store)?;
    // The thing itself first: refused, there is no card, not even a title.
    match source_of(entry, &tasks, &o) {
        Some(src) if gate.thing(src) => {}
        _ => return Ok(None),
    }
    let (about, scope, mut mine) = what(entry, &tasks, &o);
    mine.retain(|t| gate.task(t));
    let guidance = CategoryId::parse(thing)
        .ok()
        .filter(|_| entry.kind == Kind::Topic)
        .and_then(|id| {
            o.guidance(&id)
                .filter(|g| gate.guidance(g))
                .map(|g| g.text.clone())
        });
    // What is current is decided inside the history the gate admits: a
    // record it refuses ends nothing the card shows.
    let all = core.runner.standing.all(&core.store)?;
    let admitted: Vec<StandingRecord> = all.iter().filter(|r| gate.fact(r)).cloned().collect();
    let live = crate::standing::live(&admitted, None, theseus_protocol::now_unix_ms());
    let facts = live
        .into_iter()
        .filter(|r| scope.contains(&r.scope))
        .collect();
    Ok(Some(compose(Sources {
        entry,
        about: &about,
        facts,
        tasks: mine,
        guidance: guidance.as_deref(),
        said: said(&core.store, thing, gate)?,
    })))
}

/// A numbered item's card (`super::items`): its key, and the newest
/// exchanges of its timeline that `gate` admits, with the newest of them; none
/// when the gate admits none of them (an item is known only by where it was
/// said).
pub fn item_card(
    store: &crate::store::Store,
    thing: &str,
    gate: &dyn Admits,
) -> anyhow::Result<Option<(Card, Exchange)>> {
    let said = said(store, thing, gate)?;
    let Some(newest) = said.first().map(|s| s.exchange.clone()) else {
        return Ok(None);
    };
    let entry = Entry {
        thing: thing.to_string(),
        kind: Kind::Item,
        title: thing
            .strip_prefix(super::items::ITEM)
            .unwrap_or(thing)
            .into(),
        live: true,
    };
    let c = compose(Sources {
        entry: &entry,
        about: "",
        facts: Vec::new(),
        tasks: Vec::new(),
        guidance: None,
        said,
    });
    Ok(Some((c, newest)))
}

/// What `entry` is about, the scopes its facts are in, and its open tasks.
fn what<'a>(
    entry: &Entry,
    tasks: &'a [TaskRecord],
    o: &Ontology,
) -> (String, Vec<StandingScope>, Vec<&'a TaskRecord>) {
    let thing = entry.thing.as_str();
    let open = tasks.iter().filter(|t| !t.state.is_closed());
    match entry.kind {
        Kind::Task => {
            let id = thing.strip_prefix("task:").unwrap_or(thing);
            let task = tasks.iter().find(|t| t.id == id);
            let scope = task
                .and_then(|t| t.session.clone())
                .map(StandingScope::Session)
                .into_iter()
                .collect();
            let mine = open.filter(|t| t.parent.as_deref() == Some(id)).collect();
            let about = task.map(|t| t.objective.clone()).unwrap_or_default();
            (about, scope, mine)
        }
        Kind::Topic => {
            let id = CategoryId::parse(thing).ok();
            let mine = open
                .filter(|t| filed(o, t.session.as_deref(), id.as_ref()))
                .collect();
            let about = id
                .as_ref()
                .and_then(|id| o.category(id))
                .map(|c| c.description.clone())
                .unwrap_or_default();
            (about, Vec::new(), mine)
        }
        Kind::Item => (String::new(), Vec::new(), Vec::new()),
    }
}

/// Whether `session` is filed under category `id`.
fn filed(o: &Ontology, session: Option<&str>, id: Option<&CategoryId>) -> bool {
    match (session, id) {
        (Some(s), Some(id)) => o.memberships(s).iter().any(|m| &m.category == id),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Opener;

    fn entry() -> Entry {
        Entry {
            thing: "topic:ledger".into(),
            kind: Kind::Topic,
            title: "Ledger".into(),
            live: true,
        }
    }

    fn fact(n: u32) -> StandingRecord {
        StandingRecord {
            id: format!("std_{n}"),
            kind: theseus_protocol::StandingKind::Fact,
            text: format!("fact number {n}"),
            scope: StandingScope::Everywhere,
            since_ms: u64::from(n),
            until_ms: None,
            source: "cli".into(),
            ends: None,
        }
    }

    fn task(n: u32) -> TaskRecord {
        TaskRecord {
            id: format!("tsk_{n}"),
            title: format!("task {n}"),
            ..Default::default()
        }
    }

    fn said(n: u64) -> Said {
        Said {
            exchange: Exchange {
                turn_id: format!("turn_{n}"),
                session_id: "ses_1".into(),
                place: "discord:dm:42".into(),
                private: true,
                opened_by: Opener::Message,
                person: Some("owner".into()),
                start_ms: n,
                end_ms: Some(n + 1),
                ask: None,
                reply: None,
                messages: Vec::new(),
                stop: None,
                rebuilt: false,
            },
            ask: Some(format!("words of exchange {n}")),
            by: None,
        }
    }

    /// KEY: a card holds its title, and under it at most 12 lines: what it
    /// is, facts and tasks cut with a count of the rest, the guidance, and
    /// the newest three exchanges, newest first.
    #[test]
    fn a_card_is_its_title_and_at_most_twelve_lines_and_says_what_it_cut() {
        let e = entry();
        let facts: Vec<StandingRecord> = (1..=7).map(fact).collect();
        let tasks: Vec<TaskRecord> = (1..=3).map(task).collect();
        let c = compose(Sources {
            entry: &e,
            about: "the books of the lantern shop",
            facts: facts.iter().collect(),
            tasks: tasks.iter().collect(),
            guidance: Some("Prefer the ledger's own words."),
            said: (1..=5).rev().map(said).collect(),
        });
        assert_eq!(c.title, "Topic: Ledger (topic:ledger)");
        assert!(c.lines.len() <= LINES, "{:#?}", c.lines);
        assert_eq!(c.lines[0], "the books of the lantern shop");
        assert_eq!(c.lines[4], "Fact: fact number 4");
        assert_eq!(c.lines[5], "… and 3 more facts");
        assert_eq!(c.lines[7], "… and 2 more open tasks");
        assert_eq!(c.lines[8], "Guidance: Prefer the ledger's own words.");
        assert!(
            c.lines[9].ends_with(": \"words of exchange 5\""),
            "{}",
            c.lines[9]
        );
        assert_eq!(c.exchanges, ["turn_5", "turn_4", "turn_3"]);
        assert_eq!(c.lines.len(), 12);
    }

    /// A card of a thing with nothing yet is its title alone, and a closed
    /// thing says so.
    #[test]
    fn an_empty_card_is_its_title_and_a_closed_thing_says_so() {
        let mut e = entry();
        e.live = false;
        let c = compose(Sources {
            entry: &e,
            about: " ",
            facts: Vec::new(),
            tasks: Vec::new(),
            guidance: None,
            said: Vec::new(),
        });
        assert_eq!(c.title, "Topic: Ledger (closed) (topic:ledger)");
        assert!(c.lines.is_empty());
    }
}
