//! The history tool's search of imported conversations by words ([`imported`]): a search of words through
//! recall's own pipeline (the index, the filters, the pack), kept to a span of time and to a book's sessions;
//! each item with its as-of date, its session, its source and its sensitivity. A label veils nothing (anything
//! goes, 2026-11-22).
//!
//! The tools that once read through it are gone: the model's `memory.lookup` (D6 step 6; an old
//! `[policy.tools] "memory.lookup"` is a retired key), and the MCP server's `memory_search` and `memory_get`
//! (issue 46). The history tool, `context.history` (an MCP client's `context_history`), is the one way to look
//! back.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use theseus_memory::recall::Place;
use theseus_protocol::import::{ImportSessionsParams, ImportedEpisode};

use crate::config::memory::MemoryArm;
use crate::recall::when::When;
use crate::Core;

mod imported;

pub(crate) use imported::{imported, ImportHit};

/// The answer's token budget at most.
const BUDGET_MAX: u64 = 6_000;
/// The longest an index's answer is waited for.
const DEADLINE: Duration = Duration::from_secs(2);
/// The most sessions a book or a topic narrows a search of words to.
const SESSIONS_MAX: u64 = 500;

/// What a search is kept to: a book or a topic, or one session; and, for the history tool's search, imported
/// sessions alone (`imports`).
#[derive(Debug, Clone, Default)]
struct Args {
    imports: bool,
    book: Option<String>,
    topic: Option<String>,
    session: Option<String>,
}

fn some(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// One item of a page, as the answer says it.
#[derive(Default)]
struct Item {
    when: String,
    session_id: String,
    book: Option<String>,
    text: String,
    /// A hit's node and its own time (unix ms); a catalog's episode has none.
    node_id: String,
    at_ms: u64,
}

/// Whether `session` is an imported episode whose tag was erased.
fn erased(core: &Core, session: &str) -> bool {
    crate::import::is_imported(session)
        && core
            .store
            .get_session::<crate::session::SessionRecord>(session)
            .ok()
            .flatten()
            .and_then(|r| r.imported)
            .is_some_and(|i| i.erased.is_some())
}

/// A date in the daemon's time zone, `2026-03-14`.
fn day(ms: u64) -> String {
    crate::judge::spend::local_day(ms)
}

fn span_words(start_ms: u64, end_ms: u64) -> String {
    let (a, b) = (day(start_ms), day(end_ms));
    if a == b {
        a
    } else {
        format!("{a} to {b}")
    }
}

fn what(a: &Args, when: Option<&When>) -> String {
    let mut w = Vec::new();
    if let Some(s) = when {
        let edge = |ms: Option<u64>, open: &str| ms.map_or(open.to_string(), day);
        // The end is exclusive: say its last day.
        let last = s.to_ms.map(|t| t.saturating_sub(1));
        w.push(format!(
            "in {} ({} to {}, the daemon's time zone, UTC{})",
            s.said,
            edge(s.from_ms, "the start"),
            edge(last, "today"),
            offset_words()
        ));
    }
    if let Some(b) = some(&a.book) {
        w.push(format!("in the {b} book"));
    }
    if let Some(t) = some(&a.topic) {
        w.push(format!("on topic {t}"));
    }
    w.join(", ")
}

fn offset_words() -> String {
    let off = crate::wake::local(theseus_protocol::now_unix_ms()).offset_secs;
    let sign = if off < 0 { '-' } else { '+' };
    let off = off.unsigned_abs();
    format!("{sign}{:02}:{:02}", off / 3600, off % 3600 / 60)
}

fn episode_item(e: &ImportedEpisode, text: Option<String>) -> Item {
    Item {
        when: span_words(e.start_ms, e.end_ms),
        session_id: e.session_id.clone(),
        book: e.book.clone(),
        text: text
            .or_else(|| e.summary_text.clone())
            .or_else(|| e.title.clone())
            .unwrap_or_default(),
        ..Item::default()
    }
}

/// Recall's pipeline over `words`, as a turn of this session's would run
/// it, kept to the span and to a book's or a topic's sessions: the first `k`
/// it admits.
async fn by_words(
    core: &Arc<Core>,
    session: Option<&str>,
    words: String,
    a: &Args,
    when: Option<&When>,
    k: u64,
) -> Result<(String, Vec<Item>, u64, Option<String>), String> {
    // A book or a topic: its sessions, from the catalog.
    let (sessions, narrowed) = match narrowed_to(core, a, when).await? {
        Some(n) => n,
        None => {
            return Ok((
                format!("memory.lookup: no imported episode is {}.", what(a, None)),
                Vec::new(),
                0,
                None,
            ))
        }
    };
    let memory = &core.runner.memory;
    let in_context = match session {
        Some(session) => core
            .store
            .session_nodes(session)
            .map_err(|e| format!("this session could not be read: {e:#}"))?
            .into_iter()
            .map(|(_, n)| n.id)
            .collect(),
        None => Default::default(),
    };
    let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(DEADLINE);
    let k = (k as usize).clamp(1, CANDIDATES_MAX);
    let labeled = memory.labeled(&core.store).map_err(|e| format!("{e:#}"))?;
    // A stale hit (erased, gone) must not cost a live one (review G-R1 r2): the liveness check runs before
    // the page's limit, and while stale hits fill what was fetched, more is fetched, bounded.
    let mut want = k;
    loop {
        let m = asked(
            core,
            (session, words.as_str(), when, sessions.as_slice()),
            (want, deadline),
            &in_context,
            &labeled,
        )
        .await?;
        let (items, stale, unread) = items_of(core, &m.admitted).await?;
        // The index or the pack held more than this fetch took.
        let beyond = m.candidates as usize >= want || m.drops.get("budget").is_some_and(|n| *n > 0);
        if items.len() >= k || !beyond || want >= CANDIDATES_MAX {
            let incomplete = match (unread, beyond && items.len() < k) {
                (0, false) => None,
                (0, true) => Some(format!(
                    "{stale} stale hits of erased or gone notes filled what was read"
                )),
                (n, _) => Some(format!("{n} hits could not be read")),
            };
            // An import search that the catalog narrowed (a book's or a span's newest episodes) says so: what
            // lay past the narrowing was never ranked, so its end is never "that is all".
            let incomplete = incomplete.or_else(|| {
                (a.imports && !narrowed.is_empty())
                    .then(|| format!("the catalog narrowed it{narrowed}"))
            });
            let total = items.len() as u64;
            let head = format!(
                "memory.lookup: recall's items for {words:?}{}{narrowed}, best first.",
                match what(a, when) {
                    w if w.is_empty() => String::new(),
                    w => format!(" {w}"),
                }
            );
            return Ok((head, items, total, incomplete));
        }
        want = (want + stale + unread).max(want * 2).min(CANDIDATES_MAX);
    }
}

/// The most candidates a search of words fetches, refills included.
const CANDIDATES_MAX: usize = 100;

/// One fetch of a search of words: recall's pipeline over the index's `want` best, as a turn of `session`
/// would run it.
async fn asked(
    core: &Arc<Core>,
    (session, words, when, sessions): (Option<&str>, &str, Option<&When>, &[String]),
    (want, deadline): (usize, Duration),
    in_context: &std::collections::BTreeSet<String>,
    labeled: &std::collections::BTreeSet<String>,
) -> Result<theseus_protocol::memory::RecallManifest, String> {
    let memory = &core.runner.memory;
    let mut begun = memory.begin_within(
        words.to_string(),
        None,
        want,
        MemoryArm::Baseline,
        deadline,
        when.cloned(),
        sessions.to_vec(),
    );
    begun.max_items = Some(want);
    let answer = begun.answer().await;
    let scene = crate::recall::Scene {
        mode: "lookup",
        session_id: session,
        turn_id: None,
        place: session.map_or(Place::Private, |s| core.runner.place_of(s)),
        in_context: in_context.clone(),
        labeled: labeled.clone(),
        budget_tokens: Some(BUDGET_MAX * 4),
        max_items: None,
        science: memory.science_for(MemoryArm::Baseline),
        persona: None,
        category: None,
    };
    let m = memory.manifest(
        &scene,
        &begun,
        answer,
        |s| core.runner.place_of(s),
        |ids| crate::recall::links(&core.store, ids),
        true,
    );
    // `words_only`: the vector search was late or failed, and the word
    // sources answered (recall-fallback); their hits are an answer.
    if m.outcome != "ran" && m.outcome != crate::recall::WORDS_ONLY {
        return Err(format!(
            "the index did not answer ({}{}): try again, or look up by when, book or topic, which reads no index",
            m.outcome,
            m.why.as_deref().map(|w| format!(": {w}")).unwrap_or_default()
        ));
    }
    Ok(m)
}

/// A book's or a topic's sessions, from the catalog, and what the answer
/// says of a cut; none when nothing is in it. No book or topic: no
/// narrowing, unless the search keeps to imported sessions (`imports`), when it is the catalog's in `when`.
async fn narrowed_to(
    core: &Arc<Core>,
    a: &Args,
    when: Option<&When>,
) -> Result<Option<(Vec<String>, String)>, String> {
    if let Some(s) = some(&a.session) {
        return Ok(Some((vec![s.to_string()], format!(" in session {s}"))));
    }
    // An import search with no book is narrowed only by a span: unqualified, it ranks over every session
    // (imported ones kept by its caller), as the unrestricted search does.
    let span = when.filter(|_| a.imports);
    if some(&a.book).is_none() && some(&a.topic).is_none() && span.is_none() {
        return Ok(Some((Vec::new(), String::new())));
    }
    let p = ImportSessionsParams {
        book: some(&a.book).map(str::to_string),
        topic: some(&a.topic).map(str::to_string),
        from_ms: span.and_then(|w| w.from_ms),
        to_ms: span.and_then(|w| w.to_ms).map(|t| t.saturating_sub(1)),
        limit: Some(SESSIONS_MAX),
        ..Default::default()
    };
    let r = catalog(core, p).await?;
    if r.episodes.is_empty() {
        return Ok(None);
    }
    let narrowed = match r.total > SESSIONS_MAX {
        true => format!(
            " (searched the newest {SESSIONS_MAX} of its {} episodes)",
            r.total
        ),
        false => String::new(),
    };
    Ok(Some((
        r.episodes.into_iter().map(|e| e.session_id).collect(),
        narrowed,
    )))
}

/// `import.sessions`, on the blocking pool, as a private place asks it. A catalog that passed over tag records
/// it could not read is an error that counts them: the episodes it lists are not all there are, so a search
/// read through it would call the others absent.
async fn catalog(
    core: &Arc<Core>,
    p: ImportSessionsParams,
) -> Result<theseus_protocol::import::ImportSessionsResult, String> {
    let c = core.clone();
    let r = tokio::task::spawn_blocking(move || c.import_sessions(&p, true))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("the imported episodes could not be read: {e:#}"))?;
    match r.tags_unread {
        Some(n) if n > 0 => Err(format!(
            "{n} of the import's tag records could not be read, so the imported conversations are not \
             known in full: nothing was searched, and none is said to be absent"
        )),
        _ => Ok(r),
    }
}

/// Recall's admitted items as the answer says them: an imported one with
/// its episode's labels (one read of the catalog for all) and its message's
/// own date; a native one with its node's.
/// With them, how many were stale (an erased or gone node, an erased episode) and how many could not be
/// read: neither is an item, and an unread one is not called absent.
async fn items_of(
    core: &Arc<Core>,
    admitted: &[theseus_protocol::memory::RecallItem],
) -> Result<(Vec<Item>, usize, usize), String> {
    // The imported sessions' labels, in one read.
    let ids: Vec<String> = admitted
        .iter()
        .map(|i| i.session_id.clone())
        .filter(|s| crate::import::is_imported(s))
        .collect();
    let mut episodes: BTreeMap<String, ImportedEpisode> = BTreeMap::new();
    if !ids.is_empty() {
        let p = ImportSessionsParams {
            ids,
            erased: true,
            limit: Some(500),
            ..Default::default()
        };
        let r = catalog(core, p).await?;
        episodes = r
            .episodes
            .into_iter()
            .map(|e| (e.session_id.clone(), e))
            .collect();
    }
    let (mut items, mut stale, mut unread) = (Vec::new(), 0, 0);
    for i in admitted {
        // An index hit never outlives a tombstone (review G-R1): forgetting the index is best-effort, so
        // the node and its session are read as they are now, and an erased one is no item.
        let live = match core.store.get_node(&i.node_id) {
            Ok(n) => n,
            Err(_) => {
                unread += 1;
                continue;
            }
        };
        if live.is_none_or(|(_, n)| matches!(n.body, crate::node::Body::Erased { .. }))
            || erased(core, &i.session_id)
        {
            stale += 1;
            continue;
        }
        let text = i.text.clone().unwrap_or_default();
        let item = match episodes.get(&i.session_id) {
            Some(e) => {
                let mut it = episode_item(e, Some(text));
                // The message's own date, inside its episode's span.
                if let Ok(Some((_, n))) = core.store.get_node(&i.node_id) {
                    it.at_ms = n.created_at_ms;
                    it.when = format!(
                        "{} (episode {})",
                        day(n.created_at_ms),
                        span_words(e.start_ms, e.end_ms)
                    );
                }
                it
            }
            // An imported session the catalog did not name: its labels
            // are unknown.
            None if crate::import::is_imported(&i.session_id) => Item {
                when: String::new(),
                session_id: i.session_id.clone(),
                book: None,
                text,
                ..Item::default()
            },
            None => {
                let at = core
                    .store
                    .get_node(&i.node_id)
                    .ok()
                    .flatten()
                    .map_or(String::new(), |(_, n)| day(n.created_at_ms));
                Item {
                    when: at,
                    session_id: i.session_id.clone(),
                    book: None,
                    text,
                    ..Item::default()
                }
            }
        };
        items.push(Item {
            node_id: i.node_id.clone(),
            at_ms: match item.at_ms {
                0 => core
                    .store
                    .get_node(&i.node_id)
                    .ok()
                    .flatten()
                    .map_or(0, |(_, n)| n.created_at_ms),
                at => at,
            },
            ..item
        });
    }
    Ok((items, stale, unread))
}
