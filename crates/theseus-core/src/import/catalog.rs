//! The imported episodes as `import.sessions` lists them (theseus-7n3e):
//! a projection of every imported session's record, kept in memory, and the
//! query over it.
//!
//! - **Built once, kept until the import changes.** Its key is
//!   `import.list`'s counts (each tag's sessions, erased, and last write),
//!   all META reads; a batch or an erase moves them, and nothing else
//!   writes an imported session's record. A read whose key differs builds
//!   it again, from each tag's scope, the walk `import.erase` makes.
//! - **A query costs the rows in memory**, never the store: its filters
//!   and its facets (each counted with every other filter, its own aside),
//!   in one pass over the rows in its sort's order, made once. Every value a filter or a facet reads (a tag, a
//!   source, a place kind, a sensitivity, a book, a month, each topic path
//!   with its ancestors) is interned when the projection is built, so the
//!   pass compares and counts small numbers. Only its page's summaries are
//!   read, a node each.
//! - **Pure but for `build`**: `query` takes a catalog and params, so its
//!   tests need no store. A topic takes itself and what is under it, cut at
//!   a slash (`a/b` takes `a/b/c`, never `a/bc`): each row's paths hold its
//!   topics' ancestors by their slashes.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use theseus_protocol::import::{
    ImportFacet, ImportFacets, ImportListResult, ImportSessionsParams, ImportedEpisode,
};
use theseus_protocol::resident::CacheHealth;
use theseus_store::kinds;

use super::{tag_scope, ImportedFrom};
use crate::session::SessionRecord;
use crate::store::Store;

/// The page's size by default, and at most.
pub const PAGE: u64 = 50;
pub const MAX_PAGE: u64 = 500;

/// The values a facet counts, by dimension.
const DIMS: usize = 7;
const D_TAG: usize = 0;
const D_SOURCE: usize = 1;
const D_PLACE: usize = 2;
const D_SENS: usize = 3;
const D_BOOK: usize = 4;
const D_MONTH: usize = 5;
const D_PATH: usize = 6;

/// One episode: its row, its interned keys, and the folded words `q`
/// searches.
#[derive(Debug, Clone)]
pub struct Row {
    pub ep: ImportedEpisode,
    /// Its title, place name and topics, lower case.
    pub words: String,
    /// Its tag, source, place kind, sensitivity, book (or none) and month,
    /// each a value's number in its dimension.
    keys: [Option<u32>; 6],
    /// Its topics and their ancestors, each once.
    paths: Vec<u32>,
}

/// Each dimension's values by number, and their numbers by value.
#[derive(Debug, Default)]
struct Dict {
    names: [Vec<String>; DIMS],
    index: [HashMap<String, u32>; DIMS],
}

impl Dict {
    fn intern(&mut self, dim: usize, v: &str) -> u32 {
        if let Some(n) = self.index[dim].get(v) {
            return *n;
        }
        let n = self.names[dim].len() as u32;
        self.names[dim].push(v.to_string());
        self.index[dim].insert(v.to_string(), n);
        n
    }

    fn get(&self, dim: usize, v: &str) -> Option<u32> {
        self.index[dim].get(v).copied()
    }
}

/// The projection: its key, every imported session's row, and the values
/// its rows' keys number.
#[derive(Debug, Default)]
pub struct Catalog {
    pub version: String,
    pub rows: Vec<Row>,
    dict: Dict,
    /// The rows in each sort's order (newest, oldest, longest), made once,
    /// so a query walks one and sorts nothing.
    orders: [Vec<u32>; 3],
}

impl Catalog {
    /// The rows of `episodes`, their values interned.
    pub fn of(episodes: Vec<ImportedEpisode>, version: String) -> Self {
        let mut dict = Dict::default();
        let rows = episodes
            .into_iter()
            .map(|ep| {
                let keys = [
                    Some(dict.intern(D_TAG, &ep.tag)),
                    Some(dict.intern(D_SOURCE, &ep.source)),
                    Some(dict.intern(D_PLACE, &ep.place_kind)),
                    Some(dict.intern(D_SENS, &ep.sensitivity)),
                    ep.book.as_deref().map(|b| dict.intern(D_BOOK, b)),
                    Some(dict.intern(D_MONTH, &month_of(ep.start_ms))),
                ];
                let paths = with_ancestors(&ep.topics)
                    .into_iter()
                    .map(|t| dict.intern(D_PATH, t))
                    .collect();
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
                Row {
                    ep,
                    words,
                    keys,
                    paths,
                }
            })
            .collect::<Vec<Row>>();
        let order = |cmp: &dyn Fn(&ImportedEpisode, &ImportedEpisode) -> std::cmp::Ordering| {
            let mut v: Vec<u32> = (0..rows.len() as u32).collect();
            v.sort_by(|a, b| cmp(&rows[*a as usize].ep, &rows[*b as usize].ep));
            v
        };
        let orders = [
            order(&|a, b| (b.end_ms, &a.session_id).cmp(&(a.end_ms, &b.session_id))),
            order(&|a, b| (a.start_ms, &a.session_id).cmp(&(b.start_ms, &b.session_id))),
            order(&|a, b| {
                (b.messages, b.end_ms, &a.session_id).cmp(&(a.messages, a.end_ms, &b.session_id))
            }),
        ];
        Self {
            version,
            rows,
            dict,
            orders,
        }
    }
}

impl Catalog {
    /// What it holds, estimated (theseus-9lxe): each row and the text it
    /// owns, the values interned, and the orders.
    pub fn bytes(&self) -> u64 {
        let text = |v: &Option<String>| v.as_ref().map_or(0, String::capacity);
        let rows: usize = self
            .rows
            .iter()
            .map(|r| {
                let e = &r.ep;
                std::mem::size_of::<Row>()
                    + r.words.capacity()
                    + r.paths.capacity() * 4
                    + [
                        &e.session_id,
                        &e.episode_id,
                        &e.tag,
                        &e.source,
                        &e.place_kind,
                        &e.sensitivity,
                        &e.file,
                    ]
                    .iter()
                    .map(|s| s.capacity())
                    .sum::<usize>()
                    + [
                        &e.agent,
                        &e.place_name,
                        &e.partner,
                        &e.book,
                        &e.triage,
                        &e.title,
                    ]
                    .iter()
                    .map(|v| text(v))
                    .sum::<usize>()
                    + e.topics
                        .iter()
                        .map(|t| t.capacity() + std::mem::size_of::<String>())
                        .sum::<usize>()
            })
            .sum();
        let dict: usize = self
            .dict
            .names
            .iter()
            .flatten()
            .map(|n| 2 * (n.capacity() + std::mem::size_of::<String>()) + 8)
            .sum();
        let orders: usize = self.orders.iter().map(|o| o.capacity() * 4).sum();
        (rows + dict + orders) as u64
    }
}

/// The core's copy, built on a read that finds it stale, and dropped after
/// an idle stretch (theseus-9lxe: `crate::resident`'s tender asks
/// `drop_if_idle`), so the memory of a catalog nobody reads is given back;
/// the next read builds it again.
#[derive(Debug, Default)]
pub struct Cache {
    kept: Mutex<Kept>,
}

#[derive(Debug, Default)]
struct Kept {
    catalog: Option<Arc<Catalog>>,
    /// When it was last read: `None` while none is kept.
    read_at: Option<Instant>,
    /// Its estimated bytes, made as it was built.
    bytes: u64,
    /// The catalogs dropped after an idle stretch since the start.
    drops: u64,
}

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
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.read_at = Some(Instant::now());
        if let Some(c) = kept.catalog.as_ref().filter(|c| c.version == version) {
            return Ok((c.clone(), None));
        }
        // The stale one goes before the new one is built, so the two are
        // never held at once.
        kept.catalog = None;
        let t0 = Instant::now();
        let c = Arc::new(build(store, list, version)?);
        kept.bytes = c.bytes();
        kept.catalog = Some(c.clone());
        Ok((c, Some(t0.elapsed().as_secs_f64() * 1e3)))
    }

    /// When the kept catalog was last read: `None` while none is kept.
    pub fn read_at(&self) -> Option<Instant> {
        let kept = self
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.catalog.as_ref().and(kept.read_at)
    }

    /// Drop the kept catalog when nothing has read it for `idle`: its rows
    /// when it did. A read in progress keeps its own `Arc` to the end.
    pub fn drop_if_idle(&self, idle: Duration) -> Option<usize> {
        let mut kept = self
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let idle_now = kept.read_at.is_none_or(|r| r.elapsed() >= idle);
        if !idle_now {
            return None;
        }
        let c = kept.catalog.take()?;
        kept.read_at = None;
        kept.bytes = 0;
        kept.drops += 1;
        drop(kept);
        Some(c.rows.len())
    }

    /// Health's line for it (theseus-9lxe): its rows and estimated bytes,
    /// its idle bound, and its drops.
    pub fn health(&self, idle: Duration) -> CacheHealth {
        let kept = self
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rows = kept.catalog.as_ref().map_or(0, |c| c.rows.len() as u64);
        let state = match (&kept.catalog, kept.read_at) {
            (Some(_), Some(r)) => format!("read {} s ago", r.elapsed().as_secs()),
            _ => "not built: the next import.sessions read builds it".into(),
        };
        CacheHealth {
            name: "import catalog".into(),
            bytes: kept.bytes,
            cap_bytes: 0,
            entries: rows,
            estimated: true,
            note: format!(
                "{state}; dropped after {} min with no read ({} so far)",
                idle.as_secs() / 60,
                kept.drops
            ),
        }
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

/// The records a build reads at a time (theseus-9lxe): each page's records
/// are made rows and dropped before the next, so a build holds the rows and
/// one page, never every record and every decoded session at once.
const BUILD_PAGE: usize = 1_000;

/// Every imported session of every tag, as rows: each session's newest
/// record (an erase writes it again), read a page at a time.
pub fn build(store: &Store, list: &ImportListResult, version: String) -> Result<Catalog> {
    let mut episodes = Vec::new();
    for t in &list.tags {
        let first = episodes.len();
        // A session's place in `episodes`, so a newer record replaces it.
        let mut at: HashMap<String, usize> = HashMap::new();
        let mut after = 0;
        loop {
            let page = store.scope_page(&tag_scope(&t.tag), after, BUILD_PAGE)?;
            let Some(last) = page.last() else { break };
            after = last.position;
            let full = page.len() == BUILD_PAGE;
            for r in page {
                if r.kind != kinds::SESSION {
                    continue;
                }
                let rec: SessionRecord = r.decode()?;
                let Some(imp) = rec.imported.as_deref() else {
                    continue;
                };
                let ep = episode_of(&rec.session_id, rec.title.as_deref(), imp);
                match at.get(&rec.session_id) {
                    Some(&i) => episodes[i] = ep,
                    None => {
                        at.insert(rec.session_id.clone(), episodes.len());
                        episodes.push(ep);
                    }
                }
            }
            if !full {
                break;
            }
        }
        // Each tag's sessions by id, the tags in their order.
        episodes[first..].sort_by(|a: &ImportedEpisode, b| a.session_id.cmp(&b.session_id));
    }
    episodes.shrink_to_fit();
    Ok(Catalog::of(episodes, version))
}

/// An imported session's episode, as `import.sessions` lists it.
pub fn episode_of(session_id: &str, title: Option<&str>, i: &ImportedFrom) -> ImportedEpisode {
    ImportedEpisode {
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
    }
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

/// A filter, as the values' numbers it wants: none named (every row
/// passes), or one number, or a value no row has (none passes).
#[derive(Clone, Copy)]
enum Want {
    Any,
    One(u32),
    Nothing,
}

impl Want {
    fn of(dict: &Dict, dim: usize, v: Option<&str>) -> Self {
        match v {
            None => Self::Any,
            Some(v) => dict.get(dim, v).map_or(Self::Nothing, Self::One),
        }
    }

    fn passes(self, have: Option<u32>) -> bool {
        match self {
            Self::Any => true,
            Self::One(n) => have == Some(n),
            Self::Nothing => false,
        }
    }
}

/// The filters of `p` against a catalog's values.
struct Wants {
    keys: [Want; 5],
    topic: Want,
    from: Option<u64>,
    to: Option<u64>,
    words: Vec<String>,
}

impl Wants {
    fn of(dict: &Dict, p: &ImportSessionsParams) -> Self {
        Self {
            keys: [
                Want::of(dict, D_TAG, p.tag.as_deref()),
                Want::of(dict, D_SOURCE, p.source.as_deref()),
                Want::of(dict, D_PLACE, p.place.as_deref()),
                Want::of(dict, D_SENS, p.sensitivity.as_deref()),
                Want::of(dict, D_BOOK, p.book.as_deref()),
            ],
            topic: Want::of(
                dict,
                D_PATH,
                p.topic.as_deref().map(|t| t.trim_end_matches('/')),
            ),
            from: p.from_ms,
            to: p.to_ms,
            words: p
                .q
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_lowercase)
                .collect(),
        }
    }

    /// The filters a row passes, as bits.
    fn mask(&self, r: &Row) -> u8 {
        let mut m = 0;
        for (i, bit) in [TAG, SOURCE, PLACE, SENSITIVITY, BOOK]
            .into_iter()
            .enumerate()
        {
            if self.keys[i].passes(r.keys[i]) {
                m |= bit;
            }
        }
        let topic = match self.topic {
            Want::Any => true,
            Want::One(n) => r.paths.contains(&n),
            Want::Nothing => false,
        };
        if topic {
            m |= TOPIC;
        }
        if self.from.is_none_or(|f| r.ep.end_ms >= f) && self.to.is_none_or(|t| r.ep.start_ms <= t)
        {
            m |= SPAN;
        }
        if self.words.iter().all(|w| r.words.contains(w.as_str())) {
            m |= WORDS;
        }
        m
    }
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

/// `p` over `cat`: filter, facet, sort, page, in one pass over its rows.
pub fn query(cat: &Catalog, p: &ImportSessionsParams) -> Answer {
    let rows = &cat.rows;
    let want = Wants::of(&cat.dict, p);
    let mut counts: [Vec<u64>; DIMS] = std::array::from_fn(|d| vec![0; cat.dict.names[d].len()]);
    let mut kept = Vec::new();
    let ids: BTreeSet<&str> = p.ids.iter().map(String::as_str).collect();
    // Each facet's bit, and its dimension: a facet counts a row that passes
    // every filter but its own.
    let facets = [
        (TAG, D_TAG),
        (SOURCE, D_SOURCE),
        (PLACE, D_PLACE),
        (SENSITIVITY, D_SENS),
        (BOOK, D_BOOK),
        (SPAN, D_MONTH),
    ];
    let order = match p.sort.as_deref() {
        Some("oldest") => &cat.orders[1],
        Some("longest") => &cat.orders[2],
        _ => &cat.orders[0],
    };
    for i in order.iter().map(|i| *i as usize) {
        let r = &rows[i];
        if (r.ep.erased && !p.erased)
            || (!ids.is_empty() && !ids.contains(r.ep.session_id.as_str()))
        {
            continue;
        }
        let m = want.mask(r);
        for (k, (bit, dim)) in facets.into_iter().enumerate() {
            if m | bit == ALL {
                if let Some(n) = r.keys[k] {
                    counts[dim][n as usize] += 1;
                }
            }
        }
        if m | TOPIC == ALL {
            for n in &r.paths {
                counts[D_PATH][*n as usize] += 1;
            }
        }
        if m == ALL {
            kept.push(i);
        }
    }
    let total = kept.len() as u64;
    let offset = p.offset.unwrap_or(0).min(total) as usize;
    let limit = p.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE) as usize;
    let page = kept.into_iter().skip(offset).take(limit).collect();
    // A dimension's counted values: the most first, or, for the months and
    // the topic paths, by value.
    let listed = |dim: usize, most: bool| {
        let mut v: Vec<ImportFacet> = counts[dim]
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(n, c)| ImportFacet {
                value: cat.dict.names[dim][n].clone(),
                count: *c,
            })
            .collect();
        if most {
            v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
        } else {
            v.sort_by(|a, b| a.value.cmp(&b.value));
        }
        v
    };
    Answer {
        total,
        page,
        facets: ImportFacets {
            tags: listed(D_TAG, true),
            sources: listed(D_SOURCE, true),
            places: listed(D_PLACE, true),
            sensitivities: listed(D_SENS, true),
            books: listed(D_BOOK, true),
            topics: listed(D_PATH, false),
            months: listed(D_MONTH, false),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up episode: `i` picks its source, place, label and span.
    fn ep(i: u64, topics: &[&str], sensitivity: &str, erased: bool) -> ImportedEpisode {
        ImportedEpisode {
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
            book: i.is_multiple_of(2).then(|| "diary".to_string()),
            messages: 10 - i as u32,
            summary: true,
            title: Some(format!("The tide log of survey {i}")),
            erased,
            ..ImportedEpisode::default()
        }
    }

    fn rows() -> Catalog {
        Catalog::of(
            vec![
                ep(0, &["reef/survey"], "personal", false),
                ep(1, &["reef/survey/tides"], "public", false),
                ep(2, &["reef/surveyor"], "personal", false),
                ep(3, &["harbour", "reef"], "company-confidential", false),
                ep(4, &[], "public", false),
                ep(5, &["reef/survey"], "personal", true),
                ep(6, &["harbour/pier"], "partner-confidential", false),
            ],
            String::new(),
        )
    }

    fn ids(c: &Catalog, a: &Answer) -> Vec<String> {
        a.page
            .iter()
            .map(|i| {
                c.rows[*i].ep.session_id[6..]
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
        let topics: [String; 3] = ["a/b/c".into(), "a/bc".into(), "x/".into()];
        let paths = with_ancestors(&topics);
        assert_eq!(
            paths.into_iter().collect::<Vec<_>>(),
            ["a", "a/b", "a/b/c", "a/bc", "x"]
        );
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
    fn named_sessions_alone_and_their_facets() {
        let rs = rows();
        let p = ImportSessionsParams {
            ids: vec![
                rs.rows[3].ep.session_id.clone(),
                rs.rows[6].ep.session_id.clone(),
                "ses_epnone".into(),
            ],
            ..ImportSessionsParams::default()
        };
        let a = query(&rs, &p);
        assert_eq!(ids(&rs, &a), ["6", "3"]);
        assert_eq!(facet(&a.facets.topics, "harbour"), 2);
        assert_eq!(a.facets.months.len(), 2);
        // An erased one is named and still left out unless asked.
        let gone = ImportSessionsParams {
            ids: vec![rs.rows[5].ep.session_id.clone()],
            ..ImportSessionsParams::default()
        };
        assert_eq!(query(&rs, &gone).total, 0);
        assert_eq!(
            query(
                &rs,
                &ImportSessionsParams {
                    erased: true,
                    ..gone
                }
            )
            .total,
            1
        );
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
