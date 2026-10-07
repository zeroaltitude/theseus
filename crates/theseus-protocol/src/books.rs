//! The books, first cut (theseus-civ0): the imported episodes organized by
//! the book the import's pipeline hinted (`labels.book_hint`), read only.
//!
//! - **`books.list`**: each of the seven books (`diary`, `encyclopedia`,
//!   `cookbook`, `sop`, `casebook`, `register`, `dictionary`) and
//!   `unsorted` (an episode with no hint), with its count and the span of
//!   its episodes' starts.
//! - **`books.page`**: one book's episodes, newest first, a page at a time
//!   by cursor, filtered by topic, source, and place, with the book's
//!   facets (its topics, sources, and places, with their counts) on the
//!   first page. An episode's messages are `session.history` of its
//!   session.
//!
//! These are the episodes as the import labelled them, not the compiled
//! books, which come next. An imported episode is the owner's own history,
//! private under the place rule: asked for a shared place (`session_id`
//! names a session there), every episode's text is withheld, as recall
//! keeps it from a shared place.

use serde::{Deserialize, Serialize};

/// The books, in the order every surface lists them, and the name of the
/// episodes with no hint.
pub const BOOKS: &[&str] = &[
    "diary",
    "encyclopedia",
    "cookbook",
    "sop",
    "casebook",
    "register",
    "dictionary",
];
pub const UNSORTED: &str = "unsorted";

/// `books.list`: as asked for the place `session_id` speaks in; none, a
/// private place (the CLI's, the web UI's).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BooksListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

/// One book, as `books.list` shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BookInfo {
    /// One of `BOOKS`, or `unsorted`.
    pub book: String,
    /// Its episodes held (an erased one is in no book).
    #[cfg_attr(test, ts(type = "number"))]
    pub episodes: u64,
    /// The earliest and the latest of its episodes' starts (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub first_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub last_ms: Option<u64>,
}

/// `books.list`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BooksListResult {
    /// The seven books in `BOOKS`' order, then `unsorted`: each listed,
    /// an empty one with no episodes.
    pub books: Vec<BookInfo>,
    /// Every episode held, the books' sum.
    #[cfg_attr(test, ts(type = "number"))]
    pub episodes: u64,
    /// Read from the index's terms; false while the index builds them after
    /// an upgrade, when the answer read every imported session's record.
    pub indexed: bool,
    pub ms: f64,
}

/// `books.page`: one book's episodes, newest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BooksPageParams {
    /// One of `BOOKS`, or `unsorted`.
    pub book: String,
    /// Only episodes with this topic, from this source, or in this place:
    /// a place's kind (`dm`), or its kind and name (`dm:wren`), as the
    /// facets name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    /// The page after the one whose `next` this is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cursor: Option<String>,
    /// At most this many (default 50, at most 200).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
    /// As `books.list`'s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

/// Where an episode happened, as the import kept it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BookPlace {
    /// `dm`, `slack-channel`, `discord-channel`, `cli`, `cron`,
    /// `heartbeat`, or `file`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub name: Option<String>,
}

/// One episode on a book's page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BookEpisode {
    /// Its imported session (`ses_ep…`): `session.history` reads its
    /// messages.
    pub session_id: String,
    pub episode_id: String,
    pub book: String,
    /// The import's tag.
    pub tag: String,
    /// Its span of time (unix ms).
    #[cfg_attr(test, ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub end_ms: u64,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub agent: Option<String>,
    /// `personal`, `company-confidential`, `partner-confidential`, or
    /// `public`: the pipeline's label.
    pub sensitivity: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub messages: u32,
    /// The pipeline removed a credential from its text.
    pub credential_redacted: bool,
    /// The pipeline's triage category, when it triaged the episode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub triage: Option<String>,
    /// Its place, topics, partner and summary are its text: none when
    /// withheld.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<BookPlace>,
    /// Empty when withheld.
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub partner: Option<String>,
    /// Its summary, or, with none, its title (from its first message).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub summary: Option<String>,
    /// Why its text is not shown, when it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub withheld: Option<String>,
}

/// One value of a facet, with how many of the book's episodes have it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BookFacet {
    pub value: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub episodes: u64,
}

/// A book's facets: what its filters can name, the most episodes first,
/// each list cut at `FACET_MAX`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BookFacets {
    pub topics: Vec<BookFacet>,
    pub sources: Vec<BookFacet>,
    pub places: Vec<BookFacet>,
    /// Whether a list was cut.
    pub cut: bool,
}

/// The most values a facet lists.
pub const FACET_MAX: usize = 100;

/// `books.page`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BooksPageResult {
    pub book: String,
    pub episodes: Vec<BookEpisode>,
    /// The cursor of the next page, when there is more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next: Option<String>,
    /// The book's episodes, unfiltered.
    #[cfg_attr(test, ts(type = "number"))]
    pub total: u64,
    /// The book's facets, on a first page (no cursor) asked for a private
    /// place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub facets: Option<BookFacets>,
    /// The episodes this page looked at to fill itself: past `SCAN_MAX` with
    /// a filter, it stops short, and `next` goes on from there.
    #[cfg_attr(test, ts(type = "number"))]
    pub scanned: u64,
    /// As `BooksListResult`'s.
    pub indexed: bool,
    pub ms: f64,
}

/// The most episodes one filtered page looks at before it answers short.
pub const SCAN_MAX: u64 = 5_000;
