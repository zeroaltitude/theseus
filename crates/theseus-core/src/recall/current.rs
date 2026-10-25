//! A question about how a value stands now (theseus-mo1vm): "how many days of
//! WAL segments do we keep now?", "which port is the pushgateway on now?",
//! "do we still keep 30 days?". The notes about such a subject repeat every
//! value it ever had, the stale ones most; the answer is the newest
//! statement. Read from the turn's own words, ranked and marked with no
//! model call on the path.
//!
//! - **The read** ([`read`]): a `now` word ([`temporal::CURRENT`]'s) or a
//!   `still` that asks about a number, with a question that asks for a value
//!   (how many / how much, what / which / where / who …). A question
//!   [`temporal::read`] already reads as an event (when, first, last, how long
//!   ago) is left to it, and so is "what are we doing now?" and "what day is it
//!   now?", which ask for no value of a subject.
//! - **The order** ([`order`]): among the hits, the statements of the
//!   subject's value (they share half of the question's content words and
//!   carry a number) go first, newest first: by a `supersedes` edge between
//!   two of them where there is one, else by time. The rest keep their order
//!   after them.
//! - **The mark** ([`marks`], [`with_mark`]): a statement whose values differ
//!   from the newest statement's says it is superseded, and by which day's.
//!   The newest carries none, and nothing else is marked.

use std::collections::{BTreeSet, HashMap};

use theseus_memory::recall::{Candidate, Link, LinkKind};
use theseus_protocol::memory::RecallItem;

use super::render::utc_day;
use super::temporal::{Asked, Kind};

/// The words that say a question is about how things stand.
const NOW: [&str; 4] = ["now", "currently", "nowadays", "anymore"];
/// What a question asks a value with.
const ASKS: [&str; 5] = ["what", "which", "where", "who", "whats"];
/// A question about what a person or the harness is doing is not about a
/// value ("what are we doing now?", "what am I working on now?").
const ACTIVITY: [&str; 12] = [
    "doing",
    "happening",
    "going",
    "working",
    "think",
    "should",
    "shall",
    "could",
    "would",
    "can",
    "am",
    "will",
];
const PERSON: [&str; 3] = ["we", "i", "you"];
const DAY_WORDS: [&str; 4] = ["day", "date", "time", "year"];
/// Words a subject does not turn on.
const STOP: [&str; 44] = [
    "the",
    "a",
    "an",
    "of",
    "do",
    "does",
    "did",
    "we",
    "i",
    "you",
    "is",
    "are",
    "was",
    "were",
    "our",
    "my",
    "their",
    "its",
    "it",
    "in",
    "on",
    "at",
    "to",
    "for",
    "and",
    "or",
    "have",
    "has",
    "what",
    "which",
    "where",
    "who",
    "how",
    "many",
    "much",
    "now",
    "currently",
    "nowadays",
    "anymore",
    "still",
    "there",
    "that",
    "this",
    "be",
];

fn words(text: &str) -> Vec<(String, (usize, usize))> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut push = |s: usize, e: usize| {
        let w: String = text[s..e]
            .to_lowercase()
            .chars()
            .filter(|c| *c != '\'' && *c != '’')
            .collect();
        out.push((w, (s, e)));
    };
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() || c == '\'' || c == '’' {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            push(s, i);
        }
    }
    if let Some(s) = start {
        push(s, text.len());
    }
    out
}

/// The question as a value question about now, if it is one: the byte
/// ranges of its `now` or `still` word. [`temporal::read`] calls it for a
/// question it reads no event in.
pub fn read(text: &str) -> Option<Asked> {
    let w = words(text);
    let at = |i: usize| w.get(i).map(|(x, _)| x.as_str());
    let cut: Vec<(usize, usize)> = w
        .iter()
        .filter(|(x, _)| NOW.contains(&x.as_str()) || x == "still")
        .map(|(_, r)| *r)
        .collect();
    let now = w.iter().any(|(x, _)| NOW.contains(&x.as_str()));
    let still = w.iter().any(|(x, _)| x == "still");
    if !now && !still {
        return None;
    }
    let number = w
        .iter()
        .any(|(x, _)| x.starts_with(|c: char| c.is_ascii_digit()));
    // Where the question asks: its first four words ("At what daily spend …").
    let asking = (0..w.len().min(4)).find(|&i| {
        at(i).is_some_and(|x| ASKS.contains(&x))
            || (at(i) == Some("how") && matches!(at(i + 1), Some("many" | "much")))
    });
    let it = w
        .windows(2)
        .any(|p| ["is", "was"].contains(&p[0].0.as_str()) && p[1].0 == "it");
    let value = asking.is_some_and(|i| {
        let activity = w.iter().any(|(x, _)| ACTIVITY.contains(&x.as_str()))
            || (at(i + 1).is_some_and(|x| ["is", "are", "was", "were"].contains(&x))
                && at(i + 2).is_some_and(|x| PERSON.contains(&x)));
        // The present-tense "what time does …", which `temporal` leaves alone.
        let day = DAY_WORDS.contains(&at(i + 1).unwrap_or_default())
            && matches!(at(i + 2), Some("does" | "do"));
        !activity && !day
    });
    // `still` alone asks about a value only with a number in it or an ask.
    ((value || (still && number)) && !it).then_some(Asked::current(cut))
}

/// The question's content words: what a statement of its subject holds.
pub fn subject_words(question: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (w, _) in words(question) {
        let w = stem(&w);
        if w.len() > 2 && !STOP.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

fn stem(w: &str) -> String {
    match w.strip_suffix('s') {
        Some(s) if s.len() > 2 && !s.ends_with('s') => s.to_string(),
        _ => w.to_string(),
    }
}

/// The numbers `text` states, as written (a date is not one).
fn values(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut BTreeSet<String>| {
        if !cur.is_empty() {
            out.insert(std::mem::take(cur));
        }
    };
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        let part = c.is_ascii_digit()
            || (matches!(c, ':' | '.' | ',')
                && !cur.is_empty()
                && chars.get(i + 1).is_some_and(char::is_ascii_digit));
        if part {
            cur.push(*c);
        } else if *c == '-' && !cur.is_empty() && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            // 2026-03-02: a date, not a value.
            cur.clear();
            flush(&mut cur, &mut out);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// One hit that states the subject's value.
struct Statement<'a> {
    c: &'a Candidate,
    values: BTreeSet<String>,
}

/// The hits that state a value of the question's subject: most of its
/// content words (half of them, rounded up) and a number.
fn statements<'a>(question: &str, kept: &'a [Candidate]) -> Vec<Statement<'a>> {
    let subject = subject_words(question);
    let need = subject.len().div_ceil(2).max(1);
    kept.iter()
        .filter_map(|c| {
            let have: BTreeSet<String> =
                words(&c.text).into_iter().map(|(w, _)| stem(&w)).collect();
            let shared = subject.iter().filter(|s| have.contains(*s)).count();
            let values = values(&c.text);
            (shared >= need && !values.is_empty()).then_some(Statement { c, values })
        })
        .collect()
}

/// The statements, newest first. A `supersedes` edge between two of them
/// wins over their times: a statement some other statement supersedes (by
/// any chain) goes after it, and within one depth the later time first.
fn newest_first<'a>(links: &[Link], mut s: Vec<Statement<'a>>) -> Vec<Statement<'a>> {
    let ids: BTreeSet<&str> = s.iter().map(|x| x.c.node_id.as_str()).collect();
    let edges: Vec<(&str, &str)> = links
        .iter()
        .filter(|l| l.kind == LinkKind::Supersedes)
        .filter(|l| ids.contains(l.newer.as_str()) && ids.contains(l.older.as_str()))
        .map(|l| (l.newer.as_str(), l.older.as_str()))
        .collect();
    let depth = |id: &str| {
        // The statements above `id`, through the edges (bounded: a cycle
        // adds nothing past the statement count).
        let mut above: BTreeSet<&str> = BTreeSet::new();
        let mut frontier = vec![id];
        while let Some(x) = frontier.pop() {
            for (n, o) in &edges {
                if *o == x && above.insert(n) {
                    frontier.push(n);
                }
            }
        }
        above.len()
    };
    s.sort_by_cached_key(|x| (depth(&x.c.node_id), std::cmp::Reverse(x.c.time_ms)));
    s
}

/// `kept` (most relevant first) in the order a current-value question
/// asks for: the keys, the subject's statements newest first, then the rest as
/// they came.
pub fn order(question: &str, links: &[Link], kept: &[Candidate]) -> Vec<String> {
    let stated = newest_first(links, statements(question, kept));
    let mut out: Vec<String> = stated.iter().map(|x| x.c.key()).collect();
    let first: BTreeSet<String> = out.iter().cloned().collect();
    out.extend(
        kept.iter()
            .map(Candidate::key)
            .filter(|k| !first.contains(k)),
    );
    out
}

/// `order` for any kind of question about time: a current-value question's
/// is this module's (no edges known), the others' `temporal::order`.
pub fn order_of(kind: Kind, question: &str, kept: &[Candidate]) -> Vec<String> {
    match kind {
        Kind::Current => order(question, &[], kept),
        _ => super::temporal::order(kind, kept),
    }
}

/// The statements to mark superseded, by key: each one whose values do not
/// include the newest statement's, with the day of the newest.
pub fn marks(question: &str, links: &[Link], kept: &[Candidate]) -> HashMap<String, String> {
    let stated = newest_first(links, statements(question, kept));
    let Some(newest) = stated.first() else {
        return HashMap::new();
    };
    let day = utc_day(newest.c.time_ms);
    stated
        .iter()
        .skip(1)
        .filter(|x| !newest.values.is_subset(&x.values))
        .map(|x| (x.c.key(), day.clone()))
        .collect()
}

/// An item's frozen header with its mark, when it has one.
pub fn with_mark(header: String, item: &RecallItem) -> String {
    match &item.superseded {
        Some(day) => format!("{header}, superseded: older than the {day} statement"),
        None => header,
    }
}

/// The pack's items marked as [`marks`] says, when the question is a
/// current-value one.
pub fn mark(items: &mut [RecallItem], begun: &super::Begun, links: &[Link], c: &[Candidate]) {
    mark_for(items, begun.temporal, &begun.said, links, c);
}

/// [`mark`] on the question's kind and words.
pub fn mark_for(
    items: &mut [RecallItem],
    kind: Option<Kind>,
    q: &str,
    links: &[Link],
    c: &[Candidate],
) {
    if kind == Some(Kind::Current) {
        apply(items, &marks(q, links, c));
    }
}

fn apply(items: &mut [RecallItem], marks: &HashMap<String, String>) {
    for i in items {
        i.superseded = marks.get(&format!("{}#{}", i.node_id, i.chunk)).cloned();
    }
}

impl Asked {
    fn current(cut: Vec<(usize, usize)>) -> Self {
        Asked {
            kind: Kind::Current,
            cut,
        }
    }
}
