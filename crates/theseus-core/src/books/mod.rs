//! The books, first cut (theseus-civ0): the imported episodes organized by
//! the book the import's pipeline hinted (`labels.book_hint`), read only,
//! for `books.list` and `books.page`. The compiled books are the next step;
//! this is the import's labels, browsed.
//!
//! - **From the index's terms, never a re-import.** Each imported session's
//!   record gives the store's projection (`crate::store::PROJECTION`) its
//!   terms here (`session_terms`): its book's count term (`bk:`), its place
//!   in its book by time (`bt:<book>␁<start>`), the same by each topic,
//!   source, and place it has (`ft:`, `fs:`, `fp:`), and its book's facets
//!   (`ct:`, `cs:`, `cp:`), whose counts the index keeps a row each. So the
//!   list is a few rows of the index, and a page is a range of it and a
//!   record per episode shown: neither decodes the other imported sessions.
//!   An erased episode has none, and is in no book.
//! - **While the index builds its terms** after an upgrade (`build_terms`,
//!   after serving), the same terms are made by reading every imported
//!   session's record (`Walked`), and the answer says so (`indexed:
//!   false`). A test holds the two to the same answers.
//! - **A shared place reads no episode's text.** An imported session is
//!   the owner's own history, private under the place rule whatever place
//!   its episode names (`recall::place_of`), so recall never draws on it
//!   in a shared place; here, asked for a shared place, each episode keeps
//!   its book, times, source and labels' sensitivity, and its summary,
//!   topics, place and partner are withheld, and no filter or facet reads
//!   them.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Instant;

use anyhow::Result;
use serde::Deserialize;
use theseus_protocol::books::{
    BookEpisode, BookFacet, BookFacets, BookInfo, BookPlace, BooksListResult, BooksPageParams,
    BooksPageResult, BOOKS, FACET_MAX, SCAN_MAX, UNSORTED,
};
use theseus_store::{kinds, Store as _};

use crate::import::{summary_id_of, AsOf, EpisodeLabels, EpisodePlace, SESSION_PREFIX};
use crate::node::Body;
use crate::session::SessionRecord;
use crate::store::Store;

/// Why a shared place sees no episode's text.
pub const WITHHELD: &str = "an imported episode is the owner's own history, private under the \
                            place rule: its text is not shown in a shared place";

/// The book an episode's hint names: one of `BOOKS`, else `unsorted`.
pub fn book_of(hint: Option<&str>) -> &'static str {
    hint.and_then(|h| BOOKS.iter().find(|b| **b == h).copied())
        .unwrap_or(UNSORTED)
}

/// Whether `book` names a book: one of `BOOKS`, or `unsorted`.
pub fn is_book(book: &str) -> bool {
    book == UNSORTED || BOOKS.contains(&book)
}

/// A value as a term may hold it: no 0x00 (the index ends a term with one)
/// and no 0x01 (a term's own separator), and not empty.
fn clean(v: &str) -> Option<&str> {
    let v = v.trim();
    (!v.is_empty() && !v.contains(['\0', '\u{1}'])).then_some(v)
}

/// A place's facet values: its kind, and its kind and name.
fn place_values(p: &EpisodePlace) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(kind) = clean(&p.kind) {
        out.push(kind.to_string());
        if let Some(name) = p.name.as_deref().and_then(clean) {
            out.push(format!("{kind}:{name}"));
        }
    }
    out
}

/// A time as a term sorts it: 16 hex digits.
fn time_part(ms: u64) -> String {
    format!("{ms:016x}")
}

/// The count term of `book`.
fn count_term(book: &str) -> String {
    format!("bk:{book}")
}

/// The prefix of the terms that order `book`'s episodes by time; with a
/// filter, of those that order its episodes with that value.
fn walk_prefix(book: &str, filter: Option<(char, &str)>) -> String {
    match filter {
        None => format!("bt:{book}\u{1}"),
        Some((f, v)) => format!("f{f}:{v}\u{1}{book}\u{1}"),
    }
}

/// The facet term saying an episode of `book` has value `v` of facet `f`
/// (`t`opic, `s`ource, `p`lace).
fn facet_term(book: &str, f: char, v: &str) -> String {
    format!("c{f}:{book}\u{1}{v}")
}

/// The first term past every term that starts with `prefix`, which ends
/// with 0x01.
fn past(prefix: &str) -> String {
    let mut hi = prefix[..prefix.len() - 1].to_string();
    hi.push('\u{2}');
    hi
}

/// What the projection reads of an imported session's record.
#[derive(Deserialize)]
struct Imported {
    source: String,
    place: EpisodePlace,
    labels: EpisodeLabels,
    as_of: AsOf,
    #[serde(default)]
    erased: Option<serde::de::IgnoredAny>,
}

/// An imported session's book terms, from its record's `imported`; none
/// for an erased one.
fn terms_of(i: &Imported) -> Vec<String> {
    if i.erased.is_some() {
        return Vec::new();
    }
    let book = book_of(i.labels.book_hint.as_deref());
    let t = time_part(i.as_of.start_ms);
    let mut out = vec![count_term(book), format!("{}{t}", walk_prefix(book, None))];
    let mut facet = |f: char, v: &str| {
        out.push(format!("{}{t}", walk_prefix(book, Some((f, v)))));
        out.push(facet_term(book, f, v));
    };
    let topics: BTreeSet<&str> = i.labels.topic.iter().filter_map(|v| clean(v)).collect();
    for v in topics {
        facet('t', v);
    }
    if let Some(v) = clean(&i.source) {
        facet('s', v);
    }
    for v in place_values(&i.place) {
        facet('p', &v);
    }
    out
}

/// A session record's book terms, for the store's projection: an imported
/// session's (`terms_of`), none for any other.
pub fn session_terms(payload: &[u8]) -> Vec<String> {
    #[derive(Deserialize)]
    struct Rec {
        #[serde(default)]
        imported: Option<Imported>,
    }
    match serde_json::from_slice::<Rec>(payload) {
        Ok(Rec { imported: Some(i) }) => terms_of(&i),
        _ => Vec::new(),
    }
}

/// Where a read's terms come from: the index's, or every imported
/// session's record read (`Walked`).
trait Terms {
    /// (term, key) in `lo..hi`, newest first, before `past`, at most
    /// `limit`; oldest first when not `newest_first`.
    fn range(
        &self,
        lo: &str,
        hi: &str,
        past: Option<(&str, &str)>,
        newest_first: bool,
        limit: usize,
    ) -> Result<Vec<(String, String)>>;
    /// Each term in `lo..hi` with how many keys have it.
    fn counts(&self, lo: &str, hi: &str) -> Result<Vec<(String, u64)>>;
    /// A key's terms.
    fn of(&self, key: &str) -> Result<Vec<String>>;
    fn indexed(&self) -> bool;
}

struct Indexed<'a>(&'a theseus_store::WalStore);

/// The index's terms went missing mid-read: they never do once whole.
fn gone() -> anyhow::Error {
    anyhow::anyhow!("the index's terms are not whole")
}

impl Terms for Indexed<'_> {
    fn range(
        &self,
        lo: &str,
        hi: &str,
        past: Option<(&str, &str)>,
        newest_first: bool,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let rows = self
            .0
            .terms_range(kinds::SESSION, lo, hi, past, newest_first, limit)?
            .ok_or_else(gone)?;
        Ok(rows.into_iter().map(|(t, k, _)| (t, k)).collect())
    }
    fn counts(&self, lo: &str, hi: &str) -> Result<Vec<(String, u64)>> {
        self.0.term_counts(kinds::SESSION, lo, hi)?.ok_or_else(gone)
    }
    fn of(&self, key: &str) -> Result<Vec<String>> {
        self.0.terms_of(kinds::SESSION, key)?.ok_or_else(gone)
    }
    fn indexed(&self) -> bool {
        true
    }
}

/// The same terms, made by reading every imported session's record: while
/// the index builds its own, and a test's measure of them.
#[derive(Default)]
pub(crate) struct Walked {
    terms: BTreeSet<(String, String)>,
    by_key: HashMap<String, Vec<String>>,
}

impl Walked {
    pub(crate) fn read(store: &Store) -> Result<Self> {
        let mut w = Walked::default();
        for r in store
            .inner()
            .latest_with_prefix(kinds::SESSION, SESSION_PREFIX)?
        {
            let Some(key) = r.key.clone() else { continue };
            let terms = session_terms(&r.payload);
            for t in &terms {
                w.terms.insert((t.clone(), key.clone()));
            }
            w.by_key.insert(key, terms);
        }
        Ok(w)
    }
}

impl Terms for Walked {
    fn range(
        &self,
        lo: &str,
        hi: &str,
        past: Option<(&str, &str)>,
        newest_first: bool,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let inside = |(t, _): &&(String, String)| t.as_str() >= lo && t.as_str() < hi;
        let past = past.map(|(t, k)| (t.to_string(), k.to_string()));
        let rows: Vec<(String, String)> = if newest_first {
            self.terms
                .iter()
                .rev()
                .filter(inside)
                .filter(|r| past.as_ref().is_none_or(|p| *r < p))
                .take(limit)
                .cloned()
                .collect()
        } else {
            self.terms
                .iter()
                .filter(inside)
                .filter(|r| past.as_ref().is_none_or(|p| *r > p))
                .take(limit)
                .cloned()
                .collect()
        };
        Ok(rows)
    }
    fn counts(&self, lo: &str, hi: &str) -> Result<Vec<(String, u64)>> {
        let mut out: BTreeMap<String, u64> = BTreeMap::new();
        for (t, _) in &self.terms {
            if t.as_str() >= lo && t.as_str() < hi {
                *out.entry(t.clone()).or_default() += 1;
            }
        }
        Ok(out.into_iter().collect())
    }
    fn of(&self, key: &str) -> Result<Vec<String>> {
        Ok(self.by_key.get(key).cloned().unwrap_or_default())
    }
    fn indexed(&self) -> bool {
        false
    }
}

/// The index's terms when they are whole, else every record read.
fn terms(store: &Store) -> Result<Box<dyn Terms + '_>> {
    let inner = store.inner();
    if inner.terms_whole() {
        Ok(Box::new(Indexed(inner)))
    } else {
        Ok(Box::new(Walked::read(store)?))
    }
}

/// The time a term ends with (`time_part`).
fn time_of(term: &str) -> Option<u64> {
    let hex = term.rsplit('\u{1}').next()?;
    u64::from_str_radix(hex, 16).ok()
}

/// `books.list`: every book's count and span.
pub fn list(store: &Store) -> Result<BooksListResult> {
    let t0 = Instant::now();
    list_from(&*terms(store)?, t0)
}

fn list_from(src: &dyn Terms, t0: Instant) -> Result<BooksListResult> {
    let counts: BTreeMap<String, u64> = src.counts("bk:", "bk;")?.into_iter().collect();
    let mut out = BooksListResult {
        indexed: src.indexed(),
        ..BooksListResult::default()
    };
    for book in BOOKS.iter().copied().chain([UNSORTED]) {
        let episodes = counts.get(&count_term(book)).copied().unwrap_or(0);
        let (lo, hi) = (walk_prefix(book, None), past(&walk_prefix(book, None)));
        let end = |newest| -> Result<Option<u64>> {
            Ok(src
                .range(&lo, &hi, None, newest, 1)?
                .first()
                .and_then(|(t, _)| time_of(t)))
        };
        let (first_ms, last_ms) = if episodes > 0 {
            (end(false)?, end(true)?)
        } else {
            (None, None)
        };
        out.episodes += episodes;
        out.books.push(BookInfo {
            book: book.to_string(),
            episodes,
            first_ms,
            last_ms,
        });
    }
    out.ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok(out)
}

/// A page's question, checked: `Query::of` says what is wrong with one.
pub struct Query {
    book: String,
    /// Each filter: its facet's letter and its value.
    filters: Vec<(char, String)>,
    /// The (term, key) the last page ended on.
    cursor: Option<(String, String)>,
    limit: usize,
    private: bool,
}

/// A page's default size, and its largest.
pub const PAGE: usize = 50;
pub const PAGE_MAX: usize = 200;

impl Query {
    /// `p` checked, for a private place or not: the reason it is not a
    /// question, if it is not.
    pub fn of(p: &BooksPageParams, private: bool) -> std::result::Result<Self, String> {
        if !is_book(&p.book) {
            return Err(format!(
                "{:?} is not a book: one of {}, or {UNSORTED}",
                p.book,
                BOOKS.join(", ")
            ));
        }
        let mut filters = Vec::new();
        for (f, v) in [('t', &p.topic), ('p', &p.place), ('s', &p.source)] {
            let Some(v) = v else { continue };
            let Some(v) = clean(v) else {
                return Err(format!("{v:?} is not a value a filter can name"));
            };
            filters.push((f, v.to_string()));
        }
        if !private && !filters.is_empty() {
            return Err(format!(
                "no filter reads the episodes' labels here: {WITHHELD}"
            ));
        }
        let walk = walk_prefix(&p.book, filters.first().map(|(f, v)| (*f, v.as_str())));
        let cursor = match &p.cursor {
            None => None,
            Some(c) => {
                let (time, key) = c
                    .split_once('.')
                    .filter(|(t, k)| t.len() == 16 && k.starts_with(SESSION_PREFIX))
                    .ok_or_else(|| format!("{c:?} is not a cursor books.page gave"))?;
                Some((format!("{walk}{time}"), key.to_string()))
            }
        };
        let limit = p.limit.map_or(PAGE, |n| n as usize).clamp(1, PAGE_MAX);
        Ok(Self {
            book: p.book.clone(),
            filters,
            cursor,
            limit,
            private,
        })
    }
}

/// `books.page`: one book's episodes, newest first.
pub fn page(store: &Store, q: &Query) -> Result<BooksPageResult> {
    let t0 = Instant::now();
    page_from(store, &*terms(store)?, q, t0)
}

fn page_from(store: &Store, src: &dyn Terms, q: &Query, t0: Instant) -> Result<BooksPageResult> {
    let (keys, next, scanned) = walk(src, q)?;
    let book = q.book.as_str();
    let total = src
        .counts(&count_term(book), &format!("{}\u{1}", count_term(book)))?
        .first()
        .map_or(0, |(_, n)| *n);
    let facets = (q.private && q.cursor.is_none())
        .then(|| facets(src, book))
        .transpose()?;
    let mut episodes = Vec::with_capacity(keys.len());
    for key in &keys {
        if let Some(e) = episode(store, key, book, q.private)? {
            episodes.push(e);
        }
    }
    Ok(BooksPageResult {
        book: book.to_string(),
        episodes,
        next,
        total,
        facets,
        scanned,
        indexed: src.indexed(),
        ms: t0.elapsed().as_secs_f64() * 1e3,
    })
}

/// The keys of `q`'s page, newest first, its next cursor, and how many
/// episodes it looked at. The walk is the book's terms by time, or, with
/// filters, the first filter's; each other filter is its facet term among
/// the key's own. Past `SCAN_MAX` looked at, a filtered page answers short.
fn walk(src: &dyn Terms, q: &Query) -> Result<(Vec<String>, Option<String>, u64)> {
    let first = q.filters.first().map(|(f, v)| (*f, v.as_str()));
    let lo = walk_prefix(&q.book, first);
    let hi = past(&lo);
    let checks: Vec<String> = q
        .filters
        .iter()
        .skip(1)
        .map(|(f, v)| facet_term(&q.book, *f, v))
        .collect();
    let batch = if checks.is_empty() { q.limit + 1 } else { 256 };
    let mut at = q.cursor.clone();
    let mut found: Vec<(String, String)> = Vec::new();
    let (mut scanned, mut stop) = (0u64, None);
    'walk: loop {
        let rows = src.range(
            &lo,
            &hi,
            at.as_ref().map(|(t, k)| (t.as_str(), k.as_str())),
            true,
            batch,
        )?;
        let short = rows.len() < batch;
        for row in rows {
            at = Some(row.clone());
            let keep = checks.is_empty() || {
                let own = src.of(&row.1)?;
                checks.iter().all(|c| own.contains(c))
            };
            if keep {
                if found.len() == q.limit {
                    stop = found.last().cloned();
                    break 'walk;
                }
                found.push(row);
            }
            scanned += 1;
            if !checks.is_empty() && scanned >= SCAN_MAX {
                stop = at;
                break 'walk;
            }
        }
        if short {
            break;
        }
    }
    let next = stop.map(|(t, k)| format!("{}.{k}", &t[lo.len()..]));
    Ok((found.into_iter().map(|(_, k)| k).collect(), next, scanned))
}

/// A book's facets, the most episodes first.
fn facets(src: &dyn Terms, book: &str) -> Result<BookFacets> {
    let mut out = BookFacets::default();
    for (f, list) in [
        ('t', &mut out.topics),
        ('s', &mut out.sources),
        ('p', &mut out.places),
    ] {
        let lo = facet_term(book, f, "");
        let mut all: Vec<BookFacet> = src
            .counts(&lo, &past(&lo))?
            .into_iter()
            .map(|(t, n)| BookFacet {
                value: t[lo.len()..].to_string(),
                episodes: n,
            })
            .collect();
        all.sort_by(|a, b| b.episodes.cmp(&a.episodes).then(a.value.cmp(&b.value)));
        if all.len() > FACET_MAX {
            all.truncate(FACET_MAX);
            out.cut = true;
        }
        *list = all;
    }
    Ok(out)
}

/// One episode as a page shows it, from its session's record and its
/// summary's node: none when the record is gone.
fn episode(store: &Store, key: &str, book: &str, private: bool) -> Result<Option<BookEpisode>> {
    let Some(r) = store.get_session::<SessionRecord>(key)? else {
        return Ok(None);
    };
    let Some(i) = r.imported else { return Ok(None) };
    let mut e = BookEpisode {
        session_id: key.to_string(),
        episode_id: i.episode_id.clone(),
        book: book.to_string(),
        tag: i.tag.clone(),
        start_ms: i.as_of.start_ms,
        end_ms: i.as_of.end_ms,
        source: i.source.clone(),
        agent: i.agent.clone(),
        sensitivity: i.labels.sensitivity.clone(),
        messages: i.messages,
        credential_redacted: i.labels.credential_redacted,
        triage: i.triage.as_ref().map(|t| t.category.clone()),
        ..BookEpisode::default()
    };
    if !private {
        e.withheld = Some(WITHHELD.to_string());
        return Ok(Some(e));
    }
    e.place = Some(BookPlace {
        kind: i.place.kind.clone(),
        name: i.place.name.clone(),
    });
    e.topics = i.labels.topic.clone();
    e.partner = i.labels.partner.clone();
    let summary = if i.summary {
        match store.get_node(&summary_id_of(key))? {
            Some((_, n)) => match n.body {
                Body::ImportedSummary { text, .. } => Some(text),
                _ => None,
            },
            None => None,
        }
    } else {
        None
    };
    e.summary = summary.or(r.title);
    Ok(Some(e))
}
