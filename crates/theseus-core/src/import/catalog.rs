//! The imported episodes as `import.sessions` lists them (theseus-7n3e):
//! a projection of every imported session's record, kept in memory, and the
//! query over it.
//!
//! - **Built once, kept until the import changes.** Its key is
//!   `import.list`'s counts (each tag's sessions, erased, and last write),
//!   all META reads; a batch or an erase moves them, and nothing else
//!   writes an imported session's record. A read whose key differs builds
//!   it again, from each tag's scope, the walk `import.erase` makes.
//! - **A query costs the rows in memory**, never the store: its filters,
//!   its facets (each counted with every other filter, its own aside) and
//!   its sort. Only its page's summaries are read, a node each.
//! - **Pure but for `build`**: `query` takes rows and params, so its tests
//!   need no store.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use theseus_protocol::import::{
    ImportFacet, ImportFacets, ImportListResult, ImportSessionsParams, ImportedEpisode,
};
use theseus_store::kinds;

use super::{tag_scope, ImportedFrom};
use crate::session::SessionRecord;
use crate::store::Store;

/// The page's size by default, and at most.
pub const PAGE: u64 = 50;
pub const MAX_PAGE: u64 = 500;

/// One episode, with the folded words `q` searches.
#[derive(Debug, Clone)]
pub struct Row {
    pub ep: ImportedEpisode,
    /// Its title, place name and topics, lower case.
    pub words: String,
}

/// The projection: its key, and every imported session's row.
#[derive(Debug, Default)]
pub struct Catalog {
    pub version: String,
    pub rows: Vec<Row>,
}

/// The core's copy, built on a read that finds it stale.
#[derive(Debug, Default)]
pub struct Cache(Mutex<Option<Arc<Catalog>>>);

impl Cache {
    /// The projection at `list`'s counts: the kept one when its key agrees,
    /// else a new one, built now, and what building it took (ms).
    pub fn at(
        &self,
        store: &Store,
        list: &ImportListResult,
    ) -> Result<(Arc<Catalog>, Option<f64>)> {
        let version = version_of(list);
        // Held while it builds: two reads at once build it once.
        let mut kept = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(c) = kept.as_ref().filter(|c| c.version == version) {
            return Ok((c.clone(), None));
        }
        let t0 = Instant::now();
        let c = Arc::new(build(store, list, version)?);
        *kept = Some(c.clone());
        Ok((c, Some(t0.elapsed().as_secs_f64() * 1e3)))
    }
}

/// The projection's key: each tag's counts and last write.
pub fn version_of(list: &ImportListResult) -> String {
    list.tags
        .iter()
        .map(|t| format!("{}:{}:{}:{}", t.tag, t.sessions, t.erased, t.updated_ms))
        .collect::<Vec<_>>()
        .join(",")
}

/// Every imported session of every tag, as rows: each session's newest
/// record (an erase writes it again).
pub fn build(store: &Store, list: &ImportListResult, version: String) -> Result<Catalog> {
    let mut rows = Vec::new();
    for t in &list.tags {
        let mut latest: BTreeMap<String, SessionRecord> = BTreeMap::new();
        for r in store.scope_after(&tag_scope(&t.tag), 0)? {
            if r.kind != kinds::SESSION {
                continue;
            }
            let rec: SessionRecord = r.decode()?;
            latest.insert(rec.session_id.clone(), rec);
        }
        for rec in latest.into_values() {
            if let Some(imp) = rec.imported.as_deref() {
                rows.push(row_of(&rec.session_id, rec.title.as_deref(), imp));
            }
        }
    }
    Ok(Catalog { version, rows })
}

/// An imported session's row.
pub fn row_of(session_id: &str, title: Option<&str>, i: &ImportedFrom) -> Row {
    let ep = ImportedEpisode {
        session_id: session_id.to_string(),
        episode_id: i.episode_id.clone(),
        tag: i.tag.clone(),
        source: i.source.clone(),
        agent: i.agent.clone(),
        place_kind: i.place.kind.clone(),
        place_name: i.place.name.clone(),
        start_ms: i.as_of.start_ms,
        end_ms: i.as_of.end_ms,
        sensitivity: i.labels.sensitivity.clone(),
        partner: i.labels.partner.clone(),
        topics: i.labels.topic.clone(),
        book: i.labels.book_hint.clone(),
        credential_redacted: i.labels.credential_redacted,
        triage: i.triage.as_ref().map(|t| t.category.clone()),
        keep: i.triage.as_ref().map(|t| t.keep),
        messages: i.messages,
        summary: i.summary,
        title: title.map(str::to_string),
        erased: i.erased.is_some(),
        file: i.file.clone(),
        line: i.line,
        imported_at_ms: i.imported_at_ms,
        summary_text: None,
        cites: None,
    };
    let mut words = String::new();
    for w in ep
        .title
        .iter()
        .chain(ep.place_name.iter())
        .chain(ep.topics.iter())
    {
        words.push_str(&w.to_lowercase());
        words.push('\n');
    }
    Row { ep, words }
}

/// The filters a query names, by the facet each is (the bit it sets).
const TAG: u8 = 1;
const SOURCE: u8 = 1 << 1;
const PLACE: u8 = 1 << 2;
const SENSITIVITY: u8 = 1 << 3;
const BOOK: u8 = 1 << 4;
const TOPIC: u8 = 1 << 5;
const SPAN: u8 = 1 << 6;
const WORDS: u8 = 1 << 7;
const ALL: u8 = u8::MAX;

/// Whether `topics` holds `topic` or a topic under it: a path cut at a
/// slash, so `a/b` takes `a/b/c` and never `a/bc`.
pub fn under(topics: &[String], topic: &str) -> bool {
    let topic = topic.trim_end_matches('/');
    topics.iter().any(|t| {
        t == topic
            || (t.len() > topic.len() && t.starts_with(topic) && t.as_bytes()[topic.len()] == b'/')
    })
}

/// A topic path and each of its ancestors: `a/b/c` is `a`, `a/b`, `a/b/c`.
fn with_ancestors(topics: &[String]) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    for t in topics {
        let t = t.trim_end_matches('/');
        for (i, c) in t.char_indices() {
            if c == '/' && i > 0 {
                out.insert(&t[..i]);
            }
        }
        if !t.is_empty() {
            out.insert(t);
        }
    }
    out
}

/// The filters a row passes, as bits: a filter not named passes.
fn mask(r: &Row, p: &ImportSessionsParams, words: &[String]) -> u8 {
    let e = &r.ep;
    let is =
        |want: &Option<String>, have: Option<&str>| want.as_deref().is_none_or(|w| have == Some(w));
    let mut m = 0;
    if is(&p.tag, Some(&e.tag)) {
        m |= TAG;
    }
    if is(&p.source, Some(&e.source)) {
        m |= SOURCE;
    }
    if is(&p.place, Some(&e.place_kind)) {
        m |= PLACE;
    }
    if is(&p.sensitivity, Some(&e.sensitivity)) {
        m |= SENSITIVITY;
    }
    if is(&p.book, e.book.as_deref()) {
        m |= BOOK;
    }
    if p.topic.as_deref().is_none_or(|t| under(&e.topics, t)) {
        m |= TOPIC;
    }
    if p.from_ms.is_none_or(|f| e.end_ms >= f) && p.to_ms.is_none_or(|t| e.start_ms <= t) {
        m |= SPAN;
    }
    if words.iter().all(|w| r.words.contains(w.as_str())) {
        m |= WORDS;
    }
    m
}

/// A month's key, `2026-03`, from unix ms (UTC).
pub fn month_of(ms: u64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

/// A query's answer: how many every filter keeps, the page's rows (their
/// indices into the catalog's), and the facets.
#[derive(Debug, Default)]
pub struct Answer {
    pub total: u64,
    pub page: Vec<usize>,
    pub facets: ImportFacets,
}

/// `p` over `rows`: filter, facet, sort, page.
pub fn query(rows: &[Row], p: &ImportSessionsParams) -> Answer {
    let words: Vec<String> =
        p.q.as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
    let mut counts: [BTreeMap<&str, u64>; 5] = Default::default();
    let mut topics: BTreeMap<&str, u64> = BTreeMap::new();
    let mut kept = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if r.ep.erased && !p.erased {
            continue;
        }
        let m = mask(r, p, &words);
        let e = &r.ep;
        let facet = |bit: u8| m | bit == ALL;
        if facet(TAG) {
            *counts[0].entry(&e.tag).or_default() += 1;
        }
        if facet(SOURCE) {
            *counts[1].entry(&e.source).or_default() += 1;
        }
        if facet(PLACE) {
            *counts[2].entry(&e.place_kind).or_default() += 1;
        }
        if facet(SENSITIVITY) {
            *counts[3].entry(&e.sensitivity).or_default() += 1;
        }
        if facet(BOOK) {
            if let Some(b) = e.book.as_deref() {
                *counts[4].entry(b).or_default() += 1;
            }
        }
        if facet(TOPIC) {
            for t in with_ancestors(&e.topics) {
                *topics.entry(t).or_default() += 1;
            }
        }
        if m == ALL {
            kept.push(i);
        }
    }
    // The months take every filter but the span's, as a facet does; their
    // keys are owned, so they are counted apart.
    let mut months: BTreeMap<String, u64> = BTreeMap::new();
    for r in rows {
        if (r.ep.erased && !p.erased) || mask(r, p, &words) | SPAN != ALL {
            continue;
        }
        *months.entry(month_of(r.ep.start_ms)).or_default() += 1;
    }
    match p.sort.as_deref() {
        Some("oldest") => kept.sort_by(|a, b| {
            let (a, b) = (&rows[*a].ep, &rows[*b].ep);
            (a.start_ms, &a.session_id).cmp(&(b.start_ms, &b.session_id))
        }),
        Some("longest") => kept.sort_by(|a, b| {
            let (a, b) = (&rows[*a].ep, &rows[*b].ep);
            (b.messages, b.end_ms, &a.session_id).cmp(&(a.messages, a.end_ms, &b.session_id))
        }),
        _ => kept.sort_by(|a, b| {
            let (a, b) = (&rows[*a].ep, &rows[*b].ep);
            (b.end_ms, &a.session_id).cmp(&(a.end_ms, &b.session_id))
        }),
    }
    let total = kept.len() as u64;
    let offset = p.offset.unwrap_or(0).min(total) as usize;
    let limit = p.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE) as usize;
    let page = kept.into_iter().skip(offset).take(limit).collect();
    let most = |m: &BTreeMap<&str, u64>| {
        let mut v: Vec<ImportFacet> = m
            .iter()
            .map(|(k, c)| ImportFacet {
                value: (*k).to_string(),
                count: *c,
            })
            .collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
        v
    };
    Answer {
        total,
        page,
        facets: ImportFacets {
            tags: most(&counts[0]),
            sources: most(&counts[1]),
            places: most(&counts[2]),
            sensitivities: most(&counts[3]),
            books: most(&counts[4]),
            topics: topics
                .into_iter()
                .map(|(k, c)| ImportFacet {
                    value: k.to_string(),
                    count: c,
                })
                .collect(),
            months: months
                .into_iter()
                .map(|(value, count)| ImportFacet { value, count })
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up episode's row: `i` picks its source, place, label and span.
    fn row(i: u64, topics: &[&str], sensitivity: &str, erased: bool) -> Row {
        let ep = ImportedEpisode {
            session_id: format!("ses_ep{i:064x}"),
            episode_id: format!("ep_{i:064x}"),
            tag: if i < 6 {
                "reef-2026-05"
            } else {
                "tern-2026-04"
            }
            .into(),
            source: ["wiki", "skill", "openclaw-store"][(i % 3) as usize].into(),
            place_kind: ["dm", "cli"][(i % 2) as usize].into(),
            place_name: Some(format!("quay {i}")),
            // A month apart from 2026-01-01, each a day long.
            start_ms: 1_767_225_600_000 + i * 31 * 86_400_000,
            end_ms: 1_767_225_600_000 + i * 31 * 86_400_000 + 86_400_000,
            sensitivity: sensitivity.into(),
            topics: topics.iter().map(|t| (*t).to_string()).collect(),
            book: (i % 2 == 0).then(|| "diary".to_string()),
            messages: 10 - i as u32,
            summary: true,
            title: Some(format!("The tide log of survey {i}")),
            erased,
            ..ImportedEpisode::default()
        };
        let mut words = String::new();
        for w in ep
            .title
            .iter()
            .chain(ep.place_name.iter())
            .chain(ep.topics.iter())
        {
            words.push_str(&w.to_lowercase());
            words.push('\n');
        }
        Row { ep, words }
    }

    fn rows() -> Vec<Row> {
        vec![
            row(0, &["reef/survey"], "personal", false),
            row(1, &["reef/survey/tides"], "public", false),
            row(2, &["reef/surveyor"], "personal", false),
            row(3, &["harbour", "reef"], "company-confidential", false),
            row(4, &[], "public", false),
            row(5, &["reef/survey"], "personal", true),
            row(6, &["harbour/pier"], "partner-confidential", false),
        ]
    }

    fn ids(rows: &[Row], a: &Answer) -> Vec<String> {
        a.page
            .iter()
            .map(|i| {
                rows[*i].ep.session_id[6..]
                    .trim_start_matches('0')
                    .to_string()
            })
            .collect()
    }

    fn facet(f: &[ImportFacet], v: &str) -> u64 {
        f.iter().find(|x| x.value == v).map_or(0, |x| x.count)
    }

    #[test]
    fn a_topic_takes_itself_and_what_is_under_it_cut_at_a_slash() {
        let rs = rows();
        let p = |t: &str| ImportSessionsParams {
            topic: Some(t.into()),
            sort: Some("oldest".into()),
            ..ImportSessionsParams::default()
        };
        // `reef/survey` takes its child and never `reef/surveyor`.
        assert_eq!(ids(&rs, &query(&rs, &p("reef/survey"))), ["", "1"]);
        assert_eq!(ids(&rs, &query(&rs, &p("reef/survey/"))), ["", "1"]);
        assert_eq!(ids(&rs, &query(&rs, &p("reef"))), ["", "1", "2", "3"]);
        assert_eq!(query(&rs, &p("ree")).total, 0);
        assert!(under(&["a/b/c".into()], "a/b") && !under(&["a/bc".into()], "a/b"));
    }

    #[test]
    fn each_facet_counts_with_every_other_filter_and_its_own_aside() {
        let rs = rows();
        let a = query(
            &rs,
            &ImportSessionsParams {
                sensitivity: Some("personal".into()),
                place: Some("dm".into()),
                ..ImportSessionsParams::default()
            },
        );
        // Personal and in a DM: 0 and 2 (5 is erased, left out by default).
        assert_eq!(a.total, 2);
        // The sensitivities are counted over the DMs: every one, not just personal.
        assert_eq!(facet(&a.facets.sensitivities, "personal"), 2);
        assert_eq!(facet(&a.facets.sensitivities, "partner-confidential"), 1);
        assert_eq!(facet(&a.facets.sensitivities, "public"), 1);
        // The places are counted over the personal ones: both kinds.
        assert_eq!(facet(&a.facets.places, "dm"), 2);
        assert_eq!(facet(&a.facets.places, "cli"), 0);
        // A topic counts an episode once under each ancestor.
        let all = query(&rs, &ImportSessionsParams::default());
        assert_eq!(facet(&all.facets.topics, "reef"), 4);
        assert_eq!(facet(&all.facets.topics, "reef/survey"), 2);
        assert_eq!(facet(&all.facets.topics, "reef/survey/tides"), 1);
        assert_eq!(facet(&all.facets.topics, "harbour"), 2);
        assert_eq!(facet(&all.facets.books, "diary"), 4);
        // The months take every filter but the span's.
        let jan = query(
            &rs,
            &ImportSessionsParams {
                from_ms: Some(1_767_225_600_000),
                to_ms: Some(1_767_225_600_000 + 20 * 86_400_000),
                ..ImportSessionsParams::default()
            },
        );
        assert_eq!(jan.total, 1);
        assert_eq!(jan.facets.months.len(), 6, "{:?}", jan.facets.months);
        assert_eq!(jan.facets.months[0].value, "2026-01");
    }

    #[test]
    fn words_sort_pages_and_the_erased() {
        let rs = rows();
        let words = |q: &str| {
            query(
                &rs,
                &ImportSessionsParams {
                    q: Some(q.into()),
                    ..Default::default()
                },
            )
            .total
        };
        assert_eq!(words("TIDE log"), 6, "every title, case folded");
        assert_eq!(words("quay 3"), 1, "the place's name");
        assert_eq!(words("harbour pier"), 1, "a topic, every word");
        let p = |sort: &str, offset: u64, limit: u64| ImportSessionsParams {
            sort: Some(sort.into()),
            offset: Some(offset),
            limit: Some(limit),
            ..ImportSessionsParams::default()
        };
        assert_eq!(ids(&rs, &query(&rs, &p("newest", 0, 3))), ["6", "4", "3"]);
        assert_eq!(ids(&rs, &query(&rs, &p("newest", 3, 3))), ["2", "1", ""]);
        assert_eq!(ids(&rs, &query(&rs, &p("oldest", 0, 2))), ["", "1"]);
        assert_eq!(ids(&rs, &query(&rs, &p("longest", 0, 2))), ["", "1"]);
        // Past the end: an empty page, the total kept.
        let past = query(&rs, &p("newest", 99, 3));
        assert_eq!((past.total, past.page.len()), (6, 0));
        // The erased only when asked.
        let with = query(
            &rs,
            &ImportSessionsParams {
                erased: true,
                ..Default::default()
            },
        );
        assert_eq!(with.total, 7);
        // The page's size is capped.
        let big = query(&rs, &p("newest", 0, 10_000));
        assert_eq!(big.page.len(), 6);
    }

    #[test]
    fn a_month_is_its_utc_calendar_month() {
        assert_eq!(month_of(0), "1970-01");
        assert_eq!(month_of(1_767_225_600_000), "2026-01");
        assert_eq!(month_of(1_767_225_600_000 - 1), "2025-12");
        assert_eq!(month_of(1_709_164_800_000), "2024-02", "a leap day");
        assert_eq!(month_of(1_709_251_200_000 - 1), "2024-02");
        assert_eq!(month_of(1_709_251_200_000), "2024-03");
    }

    #[test]
    fn the_key_moves_with_any_tag_s_counts() {
        use theseus_protocol::import::ImportTagInfo;
        let tag = |s: u64, e: u64, at: u64| ImportListResult {
            tags: vec![ImportTagInfo {
                tag: "reef".into(),
                sessions: s,
                erased: e,
                updated_ms: at,
                ..ImportTagInfo::default()
            }],
        };
        let k = version_of(&tag(12, 0, 5));
        assert_eq!(k, version_of(&tag(12, 0, 5)));
        assert_ne!(k, version_of(&tag(13, 0, 6)), "a batch");
        assert_ne!(k, version_of(&tag(12, 12, 6)), "an erase");
        assert_ne!(
            k,
            version_of(&tag(12, 0, 6)),
            "a batch of skips still writes its counts"
        );
    }
}
