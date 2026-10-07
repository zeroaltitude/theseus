//! The books, first cut (theseus-civ0), over a store with a few hundred
//! imported sessions from made-up episodes: every book and the unsorted
//! ones counted with their spans; a book's pages newest first, each
//! episode once; the filters by topic, source, and place, alone and
//! together; an erased tag in no book; the answers from the index's terms
//! equal to those made by reading every record; a page reading a record per
//! episode it shows, never the rest; and a shared place shown no episode's
//! text. The oracle is every imported session's record, decoded.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::books::{
    BookEpisode, BooksListResult, BooksPageParams, BooksPageResult, BOOKS, SCAN_MAX, UNSORTED,
};
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};
use theseus_protocol::method;
use theseus_store::{kinds, records_read_here, Store as _};

use super::{list_from, page_from, Indexed, Query, Walked};
use crate::approval::{Client, Surface};
use crate::config::MemoryMode;
use crate::import::{episode, write, ImportedFrom, SESSION_PREFIX};
use crate::session::SessionRecord;
use crate::tests_recall::{rig, session, Rig, PIER};
use crate::Core;

const TAG: &str = "marsh-2026-01";
const LATER: &str = "marsh-2026-02";

/// Made-up topics, places, and words.
const TOPICS: &[&str] = &[
    "heron/count",
    "sluice/gate",
    "reed/cut",
    "tide/table",
    "eel/run",
];
const NAMES: &[&str] = &["wren", "teal", "rail"];

/// A time `i` hours past 2025-01-01, as the episode format writes it: the
/// day of a 28-day month, so the date is always real.
fn at(i: usize, minutes: usize) -> String {
    let (h, d) = (i % 24, i / 24);
    let (day, m) = (d % 28 + 1, d / 28);
    let (month, year) = (m % 12 + 1, 2025 + m / 12);
    format!("{year}-{month:02}-{day:02}T{h:02}:{minutes:02}:00Z")
}

/// Episode `i` of `tag`: its book the hints in turn and every eighth none,
/// one or two topics, a source and a place kind in turn, a summary for most.
pub(crate) fn episode_value(tag: &str, i: usize) -> Value {
    let book = if i % 8 == 7 {
        Value::Null
    } else {
        json!(episode::BOOKS[i % 8 % 7])
    };
    let mut topics = vec![TOPICS[i % TOPICS.len()]];
    if i.is_multiple_of(3) {
        topics.push(TOPICS[(i / 3 + 1) % TOPICS.len()]);
    }
    let topics: Vec<&str> = {
        let mut t = topics;
        t.dedup();
        t
    };
    let curated = i.is_multiple_of(11);
    let source = if curated {
        "wiki"
    } else {
        episode::SOURCES[i % 4]
    };
    let kind = episode::PLACE_KINDS[i % 3];
    let name = (!i.is_multiple_of(5)).then(|| NAMES[i % NAMES.len()]);
    let mut v = json!({
        "format": 1, "import_tag": tag,
        "episode_id": format!("ep_{:064x}", 0x5a17_0000 + i + if tag == LATER { 1 << 20 } else { 0 }),
        "source": source, "agent": null,
        "place": {"kind": kind, "name": name},
        "as_of": {"start": at(i, 0), "end": at(i, 40)},
        "labels": {"sensitivity": episode::SENSITIVITIES[i / 8 % 4],
                   "partner": if i / 8 % 4 == 2 { json!("partner-candidate:osprey") } else { Value::Null },
                   "topic": topics, "book_hint": book, "credential_redacted": false},
        "summary": if curated { Value::Null } else {
            json!({"text": format!("Marsh note {i}: the sluice log is kept in the reed shed."),
                   "cites": [0], "model": "claude-opus-5-5"})
        },
        "messages": [{"idx": 0, "time": at(i, 0), "author": "wren", "integrity": "operator",
            "text": format!("Marsh entry {i}: counted herons at the sluice."),
            "unit": format!("unit-{i}"), "sha256": "ab".repeat(32)}],
    });
    if !curated {
        v["triage"] = json!({"category": "decision_or_preference", "keep": 0.8, "model": "jev-1"});
    }
    v["hash"] = json!(episode::hash_of(&v));
    v
}

/// Import `n` episodes of `tag` from `from`, in batches of 500.
pub(crate) fn import(core: &Core, tag: &str, from: usize, n: usize) {
    let lines: Vec<ImportLine> = (from..from + n)
        .map(|i| ImportLine {
            line: i as u64 + 1,
            text: serde_json::to_string(&episode_value(tag, i)).unwrap(),
        })
        .collect();
    for chunk in lines.chunks(500) {
        let p = ImportEpisodesParams {
            file: "marsh.jsonl".into(),
            lines: chunk.to_vec(),
        };
        let r = write::import_batch(&core.store, &p, "test").unwrap();
        assert!(r.rejected.is_empty(), "{:?}", r.rejected);
    }
}

/// Every imported session held, decoded: the oracle.
fn held(core: &Core) -> Vec<(String, ImportedFrom)> {
    core.store
        .inner()
        .latest_with_prefix(kinds::SESSION, SESSION_PREFIX)
        .unwrap()
        .into_iter()
        .filter_map(|r| {
            let s: SessionRecord = r.decode().unwrap();
            let i = *s.imported?;
            i.erased.is_none().then_some((s.session_id, i))
        })
        .collect()
}

fn book_of(i: &ImportedFrom) -> &'static str {
    super::book_of(i.labels.book_hint.as_deref())
}

/// The oracle's page order: `book`'s episodes passing `keep`, newest start
/// first, then by session id, as the index's terms sort.
fn expected(core: &Core, book: &str, keep: impl Fn(&ImportedFrom) -> bool) -> Vec<String> {
    let mut v: Vec<(u64, String)> = held(core)
        .into_iter()
        .filter(|(_, i)| book_of(i) == book && keep(i))
        .map(|(s, i)| (i.as_of.start_ms, s))
        .collect();
    v.sort();
    v.reverse();
    v.into_iter().map(|(_, s)| s).collect()
}

fn params(book: &str) -> BooksPageParams {
    BooksPageParams {
        book: book.into(),
        ..BooksPageParams::default()
    }
}

/// Every page of `p`, `limit` at a time, from the index: the sessions in
/// order, and how many pages.
fn all_pages(core: &Core, mut p: BooksPageParams, limit: u32) -> (Vec<String>, usize) {
    p.limit = Some(limit);
    let (mut out, mut pages) = (Vec::new(), 0);
    loop {
        let r = super::page(&core.store, &Query::of(&p, true).unwrap()).unwrap();
        assert!(r.indexed, "a fresh store's terms are whole");
        assert!(r.episodes.len() <= limit as usize);
        pages += 1;
        out.extend(r.episodes.into_iter().map(|e| e.session_id));
        match r.next {
            Some(n) => p.cursor = Some(n),
            None => break,
        }
        assert!(pages < 1_000, "the pages end");
    }
    (out, pages)
}

fn rigged(n: usize) -> Rig {
    let r = rig(MemoryMode::Off);
    import(&r.core, TAG, 0, n);
    r
}

/// Every book and the unsorted ones: their counts, their spans, and the
/// sum, as the records say.
#[test]
fn the_list_counts_every_book_and_the_unsorted_with_their_spans() {
    assert_eq!(
        episode::BOOKS,
        BOOKS,
        "the import's books are the protocol's"
    );
    let r = rigged(320);
    let c = &r.core;
    let got = super::list(&c.store).unwrap();
    assert!(got.indexed);
    let names: Vec<&str> = got.books.iter().map(|b| b.book.as_str()).collect();
    let mut want: Vec<&str> = BOOKS.to_vec();
    want.push(UNSORTED);
    assert_eq!(names, want, "the seven books, then the unsorted");
    let all = held(c);
    for b in &got.books {
        let mine: Vec<&ImportedFrom> = all
            .iter()
            .map(|(_, i)| i)
            .filter(|i| book_of(i) == b.book)
            .collect();
        assert_eq!(b.episodes, mine.len() as u64, "{}", b.book);
        assert!(b.episodes > 0, "{} has episodes", b.book);
        assert_eq!(
            b.first_ms,
            mine.iter().map(|i| i.as_of.start_ms).min(),
            "{}",
            b.book
        );
        assert_eq!(
            b.last_ms,
            mine.iter().map(|i| i.as_of.start_ms).max(),
            "{}",
            b.book
        );
    }
    assert_eq!(got.episodes, 320);
    assert_eq!(
        got.books
            .iter()
            .find(|b| b.book == UNSORTED)
            .unwrap()
            .episodes,
        40,
        "every eighth episode has no hint"
    );
}

/// A book's pages, newest first: every episode once, in the oracle's
/// order, at page sizes that do and do not divide the book; the total and
/// the facets on the first page.
#[test]
fn a_books_pages_are_newest_first_and_hold_each_episode_once() {
    let r = rigged(320);
    let c = &r.core;
    for book in ["diary", "sop", UNSORTED] {
        let want = expected(c, book, |_| true);
        assert!(want.len() >= 40, "{book}: {}", want.len());
        for limit in [1, 7, 40, 200] {
            let (got, pages) = all_pages(c, params(book), limit);
            assert_eq!(got, want, "{book} by {limit}");
            assert_eq!(
                pages,
                want.len().div_ceil(limit as usize).max(1),
                "{book} by {limit}"
            );
        }
        let first = super::page(&c.store, &Query::of(&params(book), true).unwrap()).unwrap();
        assert_eq!(first.total, want.len() as u64);
        let f = first.facets.expect("a first page's facets");
        let topics: u64 = f.topics.iter().map(|t| t.episodes).sum();
        assert!(topics >= want.len() as u64, "every episode has a topic");
        let sources: u64 = f.sources.iter().map(|t| t.episodes).sum();
        assert_eq!(sources, want.len() as u64, "every episode has one source");
        assert!(
            f.places.iter().any(|p| p.value.contains(':')),
            "{:?}",
            f.places
        );
    }
    let e = &super::page(&c.store, &Query::of(&params("diary"), true).unwrap())
        .unwrap()
        .episodes[0];
    let (sid, i) = held(c)
        .into_iter()
        .find(|(s, _)| *s == e.session_id)
        .unwrap();
    assert_eq!(e.session_id, sid);
    assert_eq!((e.start_ms, e.end_ms), (i.as_of.start_ms, i.as_of.end_ms));
    assert_eq!(e.topics, i.labels.topic);
    assert!(e.withheld.is_none());
    let text = e.summary.as_deref().unwrap();
    assert!(text.starts_with("Marsh "), "its summary or title: {text}");
}

/// The filters, alone and together, page by page, as the oracle filters.
#[test]
fn the_filters_by_topic_source_and_place_answer_as_the_records_say() {
    let r = rigged(320);
    let c = &r.core;
    let book = "casebook";
    let topic = "sluice/gate";
    let has_topic = |i: &ImportedFrom| i.labels.topic.iter().any(|t| t == topic);
    let source = episode::SOURCES[1];
    type Keep<'a> = Box<dyn Fn(&ImportedFrom) -> bool + 'a>;
    let cases: Vec<(BooksPageParams, Keep<'_>)> = vec![
        (
            BooksPageParams {
                topic: Some(topic.into()),
                ..params(book)
            },
            Box::new(has_topic),
        ),
        (
            BooksPageParams {
                source: Some(source.into()),
                ..params(book)
            },
            Box::new(move |i| i.source == source),
        ),
        (
            BooksPageParams {
                place: Some("dm".into()),
                ..params(book)
            },
            Box::new(|i| i.place.kind == "dm"),
        ),
        (
            BooksPageParams {
                place: Some("slack-channel:teal".into()),
                ..params(book)
            },
            Box::new(|i| {
                i.place.kind == "slack-channel" && i.place.name.as_deref() == Some("teal")
            }),
        ),
        (
            BooksPageParams {
                topic: Some(topic.into()),
                place: Some("dm".into()),
                ..params(book)
            },
            Box::new(move |i| has_topic(i) && i.place.kind == "dm"),
        ),
        (
            BooksPageParams {
                topic: Some("no/such".into()),
                ..params(book)
            },
            Box::new(|_| false),
        ),
    ];
    for (p, keep) in cases {
        let want = expected(c, book, keep);
        for limit in [3, 50] {
            let (got, _) = all_pages(c, p.clone(), limit);
            assert_eq!(got, want, "{p:?} by {limit}");
        }
    }
    assert!(!expected(c, book, |i| i.place.kind == "dm").is_empty());
}

/// The answers from the index's terms are those made by reading every
/// record: the list, and each book's first pages with and without filters.
#[test]
fn the_index_answers_as_every_record_read_does() {
    let r = rigged(240);
    let c = &r.core;
    import(c, LATER, 240, 60);
    let walked = Walked::read(&c.store).unwrap();
    let indexed = Indexed(c.store.inner());
    let t0 = std::time::Instant::now();
    let strip = |mut l: BooksListResult| {
        l.ms = 0.0;
        l.indexed = false;
        l
    };
    assert_eq!(
        strip(list_from(&indexed, t0).unwrap()),
        strip(list_from(&walked, t0).unwrap())
    );
    let strip = |mut p: BooksPageResult| {
        p.ms = 0.0;
        p.indexed = false;
        p
    };
    for book in BOOKS.iter().copied().chain([UNSORTED]) {
        for p in [
            params(book),
            BooksPageParams {
                topic: Some("reed/cut".into()),
                limit: Some(4),
                ..params(book)
            },
            BooksPageParams {
                place: Some("cli".into()),
                source: Some("wiki".into()),
                ..params(book)
            },
        ] {
            let q = Query::of(&p, true).unwrap();
            let a = strip(page_from(&c.store, &indexed, &q, t0).unwrap());
            let b = strip(page_from(&c.store, &walked, &q, t0).unwrap());
            assert_eq!(a, b, "{p:?}");
        }
    }
}

/// An erased tag's episodes leave every book; the other tag's stay.
#[test]
fn an_erased_tags_episodes_are_in_no_book() {
    let r = rigged(160);
    let c = &r.core;
    import(c, LATER, 160, 80);
    assert_eq!(super::list(&c.store).unwrap().episodes, 240);
    write::erase(&c.store, TAG, Some("a test"), "test").unwrap();
    let got = super::list(&c.store).unwrap();
    assert_eq!(got.episodes, 80, "{got:?}");
    let left = held(c);
    assert_eq!(left.len(), 80);
    assert!(left.iter().all(|(_, i)| i.tag == LATER));
    let (pages, _) = all_pages(c, params("diary"), 50);
    assert_eq!(pages, expected(c, "diary", |_| true));
}

/// A page reads the records of the episodes it shows (each one's session
/// and summary), never the rest of the import's, and the list reads none.
#[test]
fn a_page_reads_only_the_episodes_it_shows() {
    let r = rigged(400);
    let c = &r.core;
    let reads = |f: &dyn Fn()| {
        let before = records_read_here();
        f();
        records_read_here() - before
    };
    let list = reads(&|| drop(super::list(&c.store).unwrap()));
    assert_eq!(list, 0, "the list reads the index's terms alone");
    let p = BooksPageParams {
        limit: Some(10),
        ..params("diary")
    };
    let page = reads(&|| {
        let r = super::page(&c.store, &Query::of(&p, true).unwrap()).unwrap();
        assert_eq!(r.episodes.len(), 10);
    });
    assert!(
        page <= 20,
        "a session and a summary an episode: {page} records read"
    );
    let filtered = BooksPageParams {
        topic: Some("eel/run".into()),
        place: Some("dm".into()),
        limit: Some(5),
        ..params("diary")
    };
    let page =
        reads(&|| drop(super::page(&c.store, &Query::of(&filtered, true).unwrap()).unwrap()));
    assert!(page <= 10, "{page} records read");
}

/// A filtered page whose filters match nothing walks its first filter's
/// episodes to the end, under `SCAN_MAX`, and answers empty with no cursor.
#[test]
fn a_filtered_page_with_no_match_ends_with_no_cursor() {
    let r = rigged(80);
    let c = &r.core;
    let p = BooksPageParams {
        topic: Some("heron/count".into()),
        source: Some("no-such-source".into()),
        ..params("diary")
    };
    let got = super::page(&c.store, &Query::of(&p, true).unwrap()).unwrap();
    assert!(got.episodes.is_empty());
    assert!(got.next.is_none(), "a walk that ends before its cap ends");
    assert!(got.scanned > 0 && got.scanned < SCAN_MAX, "{}", got.scanned);
}

/// One request over a connection the core serves as the CLI.
async fn call(core: &Arc<Core>, m: &str, params: Value) -> Result<Value, (i64, String)> {
    call_on(core, Surface::Cli, m, params).await
}

/// One request over a connection the core serves as `surface`.
async fn call_on(
    core: &Arc<Core>,
    surface: Surface,
    m: &str,
    params: Value,
) -> Result<Value, (i64, String)> {
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    let who = Client::new("conn", surface);
    let serving = tokio::spawn(core.clone().serve_connection(sr, sw, who));
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), m, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let answer = loop {
        let l = lines.next_line().await.unwrap().expect("an answer");
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = serving.await;
    match answer.error {
        Some(e) => Err((e.code, e.message)),
        None => Ok(answer.result.unwrap_or(Value::Null)),
    }
}

/// Over the protocol: a private place (no session, or a private one's) is
/// shown each episode's summary, topics and place; a shared place's session
/// is shown its book, times and sensitivity, and none of its text, whatever
/// its sensitivity, and no filter or facet; a book that is not one, and a
/// cursor no page gave, are refused.
#[tokio::test]
async fn a_shared_place_is_shown_no_episodes_text() {
    let r = rigged(96);
    let c = &r.core;
    let list: BooksListResult =
        serde_json::from_value(call(c, method::BOOKS_LIST, Value::Null).await.unwrap()).unwrap();
    assert_eq!(list.episodes, 96);

    let page = |sid: Option<&str>| json!({"book": "register", "limit": 200, "session_id": sid});
    let mine = session(c, None, &[]);
    for sid in [None, Some(mine.as_str())] {
        let got: BooksPageResult =
            serde_json::from_value(call(c, method::BOOKS_PAGE, page(sid)).await.unwrap()).unwrap();
        assert!(!got.episodes.is_empty());
        assert!(got.facets.is_some());
        for e in &got.episodes {
            assert!(e.withheld.is_none());
            assert!(
                e.summary.is_some() && e.place.is_some() && !e.topics.is_empty(),
                "{e:?}"
            );
        }
        let sens: std::collections::BTreeSet<&str> = got
            .episodes
            .iter()
            .map(|e| e.sensitivity.as_str())
            .collect();
        assert!(
            sens.contains("personal") && sens.contains("partner-confidential"),
            "{sens:?}"
        );
    }

    let pier = session(c, Some(&format!("channel:{PIER}")), &[]);
    let raw = call(c, method::BOOKS_PAGE, page(Some(&pier)))
        .await
        .unwrap();
    let text = raw.to_string();
    for word in ["Marsh", "heron", "sluice", "wren", "teal", "osprey"] {
        assert!(
            !text.contains(word),
            "{word:?} reached a shared place: {text}"
        );
    }
    let got: BooksPageResult = serde_json::from_value(raw).unwrap();
    assert_eq!(got.episodes.len(), expected(c, "register", |_| true).len());
    assert!(got.facets.is_none());
    for e in &got.episodes {
        let BookEpisode {
            withheld,
            summary,
            place,
            topics,
            partner,
            ..
        } = e;
        assert!(withheld.is_some(), "{e:?}");
        assert!(summary.is_none() && place.is_none() && topics.is_empty() && partner.is_none());
    }
    let list: BooksListResult = serde_json::from_value(
        call(c, method::BOOKS_LIST, json!({"session_id": pier}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(list.episodes, 96, "a shared place is told the counts");

    let refused = call(
        c,
        method::BOOKS_PAGE,
        json!({"book": "register", "topic": "heron/count", "session_id": pier}),
    )
    .await
    .unwrap_err();
    assert_eq!(
        refused.0,
        theseus_protocol::error_code::INVALID_PARAMS,
        "{refused:?}"
    );
    let bad = call(c, method::BOOKS_PAGE, json!({"book": "almanac"}))
        .await
        .unwrap_err();
    assert!(bad.1.contains("not a book"), "{bad:?}");
    let bad = call(
        c,
        method::BOOKS_PAGE,
        json!({"book": "diary", "cursor": "nope"}),
    )
    .await
    .unwrap_err();
    assert!(bad.1.contains("not a cursor"), "{bad:?}");
}

/// A surface that reads no private text (the Discord binding, a
/// connection no listener named; the MCP server may not call the books) is
/// a shared place whatever
/// session it names, or none: each episode's text withheld, no facets, a
/// filter refused. The web UI reads them as the CLI does (the surface rule
/// of `import.sessions`, theseus-7n3e).
#[tokio::test]
async fn a_surface_that_reads_no_private_text_is_shown_none() {
    let r = rigged(48);
    let c = &r.core;
    let mine = session(c, None, &[]);
    let page = |sid: Option<&str>| json!({"book": "diary", "limit": 200, "session_id": sid});
    let web: BooksPageResult = serde_json::from_value(
        call_on(c, Surface::Web, method::BOOKS_PAGE, page(None))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(!web.episodes.is_empty() && web.facets.is_some());
    assert!(web
        .episodes
        .iter()
        .all(|e| e.withheld.is_none() && e.summary.is_some()));
    for surface in [Surface::Discord, Surface::Unnamed] {
        for sid in [None, Some(mine.as_str())] {
            let raw = call_on(c, surface, method::BOOKS_PAGE, page(sid))
                .await
                .unwrap();
            let text = raw.to_string();
            for word in ["Marsh", "heron", "sluice", "wren", "teal", "osprey"] {
                assert!(!text.contains(word), "{word:?} reached {surface:?}: {text}");
            }
            let got: BooksPageResult = serde_json::from_value(raw).unwrap();
            assert_eq!(got.episodes.len(), web.episodes.len(), "{surface:?}");
            assert!(got.facets.is_none(), "{surface:?}");
            assert!(
                got.episodes.iter().all(|e| e.withheld.is_some()),
                "{surface:?}"
            );
        }
        let refused = call_on(
            c,
            surface,
            method::BOOKS_PAGE,
            json!({"book": "diary", "topic": "heron/count"}),
        )
        .await
        .unwrap_err();
        assert_eq!(
            refused.0,
            theseus_protocol::error_code::INVALID_PARAMS,
            "{surface:?}"
        );
    }
}

/// What the books cost over 20,000 imported sessions: `books.list` and a
/// book's first page of 50, unfiltered and with two filters, each timed 20
/// times, with the records each reads. A measure, not a check: run it with
/// `--ignored --nocapture`, in a debug and a release build.
#[test]
#[ignore = "a measure: 20,000 imported sessions, printed"]
fn the_books_over_twenty_thousand_episodes_timed() {
    let r = rig(MemoryMode::Off);
    let c = &r.core;
    let t = std::time::Instant::now();
    import(c, TAG, 0, 20_000);
    println!("import of 20,000: {:.1} s", t.elapsed().as_secs_f64());
    let time = |what: &str, f: &dyn Fn()| {
        f();
        let read = records_read_here();
        f();
        let read = records_read_here() - read;
        let mut ms: Vec<f64> = (0..20)
            .map(|_| {
                let t = std::time::Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        println!(
            "{what}: p50 {:.2} ms, max {:.2} ms, {read} records",
            ms[10], ms[19]
        );
    };
    time("books.list", &|| drop(super::list(&c.store).unwrap()));
    let first = params("diary");
    time("books.page diary, first 50", &|| {
        drop(super::page(&c.store, &Query::of(&first, true).unwrap()).unwrap())
    });
    let deep = super::page(
        &c.store,
        &Query::of(
            &BooksPageParams {
                limit: Some(200),
                ..params("diary")
            },
            true,
        )
        .unwrap(),
    )
    .unwrap();
    let next = BooksPageParams {
        cursor: deep.next,
        ..params("diary")
    };
    time("books.page diary, 50 after 200", &|| {
        drop(super::page(&c.store, &Query::of(&next, true).unwrap()).unwrap())
    });
    let filtered = BooksPageParams {
        topic: Some("eel/run".into()),
        place: Some("dm:wren".into()),
        ..params("diary")
    };
    time("books.page diary, topic and place", &|| {
        drop(super::page(&c.store, &Query::of(&filtered, true).unwrap()).unwrap())
    });
    let walked = std::time::Instant::now();
    let w = Walked::read(&c.store).unwrap();
    println!(
        "every record read instead (the fallback while terms build): {:.1} ms",
        walked.elapsed().as_secs_f64() * 1e3
    );
    drop(w);
}

/// A store an older build wrote last has no book terms: the books read
/// every imported session's record, say so (`indexed: false`), and answer
/// as the index did; once the terms are built after serving, they read the
/// index again, with the same answers.
#[test]
fn before_the_terms_are_built_the_books_read_every_record_and_answer_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let core_on = |store: crate::store::Store| {
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![]));
        Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap()
    };
    let answers = |c: &Core| {
        let mut l = super::list(&c.store).unwrap();
        let mut p = super::page(
            &c.store,
            &Query::of(
                &BooksPageParams {
                    topic: Some("tide/table".into()),
                    limit: Some(9),
                    ..params("cookbook")
                },
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let indexed = (l.indexed, p.indexed);
        (l.ms, l.indexed, p.ms, p.indexed) = (0.0, false, 0.0, false);
        (l, p, indexed)
    };
    let core = core_on(crate::store::Store::open(&store_dir).unwrap());
    import(&core, TAG, 0, 150);
    let (list, page, indexed) = answers(&core);
    assert_eq!(indexed, (true, true));
    assert_eq!(list.episodes, 150);
    assert!(!page.episodes.is_empty());
    drop(core);

    let plain =
        theseus_store::WalStore::open(&store_dir, theseus_store::WalConfig::default()).unwrap();
    let row = theseus_store::NewRecord::json(kinds::LEDGER, None, &json!({"at": 1})).unwrap();
    theseus_store::Store::append(&plain, &[row]).unwrap();
    plain.checkpoint().unwrap();
    drop(plain);
    let core = core_on(crate::store::Store::open(&store_dir).unwrap());
    assert!(!core.store.inner().terms_whole(), "the terms are not built");
    let (l, p, indexed) = answers(&core);
    assert_eq!(indexed, (false, false), "read from the records");
    assert_eq!((&l, &p), (&list, &page), "the fallback's answers");

    let mut at = None;
    while let Some(next) = core.store.inner().build_terms(at, 64).unwrap() {
        at = Some(next);
    }
    assert!(core.store.inner().terms_whole());
    let (l, p, indexed) = answers(&core);
    assert_eq!(indexed, (true, true), "the index again");
    assert_eq!((&l, &p), (&list, &page));
}
