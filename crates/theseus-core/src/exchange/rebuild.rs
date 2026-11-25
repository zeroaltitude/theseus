//! `exchange.rebuild` (issue 44, step 1): the exchanges and touches of turns,
//! written from their nodes. It reads the conversations and writes only
//! these records:
//!
//! - **A turn with no exchange** gets one. Its place and its person come from
//!   the owner's mark of its message alone (`said.<node>`, written as the
//!   message arrived): a session's binding is where it posts now, not proof
//!   of where a message arrived or who typed it (an agent's terminal submit
//!   into a bound DM is the operator's too). With no mark the place is
//!   `unknown` and there is no person, and its place's touch is `unknown`'s, so a look-back by date reaches
//!   it (and says its place was not recorded). No mark is ever made here.
//! - **A turn with an exchange** keeps it, and its mark, as written; only its
//!   missing touches are written (a restart, or an index that was late or
//!   down, can leave a thing's touch unwritten).
//! - **A label raised before labels touched** (`item_labels::touch_root`): each flag an item's record holds
//!   (`labels.<node>`), removed or not, gets its `label:<flag>` touch at its turn's time (`item_labels::at`;
//!   an id with no time, at its exchange's start), when the store lacks it. It reads the records Jev's answers
//!   left; it calls no model. A record under the prefix read as an item's labels in neither their shape nor
//!   their older one (`item_labels::kept`) is passed over, left as it is, and counted; nothing in it is read.
//! - **A touch keyed off its exchange's start** (an older build keyed a
//!   reply's things at its end) is moved, not dropped: its own record is
//!   written again at the start's key and the old key ended in the same
//!   frame, with no index asked, so one exchange holds one touch of a root and
//!   no association is lost. The index only adds touches; it never ends one.
//!   A label's place, an item's or a picture's, is its turn's time
//!   (`item_labels::keyed_at`, `names::pictures::keyed_at`), so one there is
//!   left, and one elsewhere is moved there. A record under the touches'
//!   prefix that would be moved and does not read as a touch is passed over,
//!   left where it is, and counted (`touches_passed_over`); its key alone is
//!   named in the log.
//!
//! The dry run counts and writes nothing.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use theseus_protocol::exchange::{ExchangeRebuildParams, ExchangeRebuildResult};
use theseus_protocol::index::IndexEntitiesParams;
use theseus_store::{kinds, NewRecord, Store as _};

use super::{
    exchange, exchange_record, mark, thing_touches, touch, Exchange, Opener, Root, SEP,
    TOUCH_PREFIX,
};
use crate::memory_pass::PassIndex;
use crate::node::{Body, Node, Origin};
use crate::places::{PlaceClass, PlaceRule};
use crate::session::SessionRecord;
use crate::store::Store;

/// The place of history no record proves.
pub const UNKNOWN: &str = "unknown";
/// Records a frame of the rebuild holds, at most.
const BATCH: usize = 256;
/// The most of a message or reply the index is asked to name things in: the
/// live naming's own cut, so a term the turn named is named again here.
const TEXT: usize = super::THINGS_TEXT;

/// One turn's exchange, new or kept, and the words its things are named from.
struct Planned {
    ex: Exchange,
    new: bool,
    texts: Vec<String>,
}

fn cut(text: &str) -> String {
    crate::scrub::safe::head_bytes(text, TEXT).0.to_string()
}

/// The words of the nodes `ask` and `reply`, among `nodes`.
fn texts(nodes: &[&Node], ask: Option<&str>, reply: Option<&str>) -> Vec<String> {
    let of = |id: Option<&str>| nodes.iter().find(|n| Some(n.id.as_str()) == id);
    let mut out = Vec::new();
    if let Some(Body::UserMessage { text, .. }) = of(ask).map(|n| &n.body) {
        out.push(cut(text));
    }
    if let Some(Body::AssistantMessage { blocks, .. }) = of(reply).map(|n| &n.body) {
        out.push(cut(&crate::provider::text_of(blocks)));
    }
    out
}

/// One turn's nodes, as a new exchange; none for a turn with no message and
/// no reply (a task's brief, a turn that only failed). Place and person from
/// the message's mark, when it has one.
fn of_turn(
    store: &Store,
    turn: &str,
    sid: &str,
    nodes: &[&Node],
) -> anyhow::Result<Option<Exchange>> {
    let words =
        |n: &&&Node| matches!(n.body, Body::UserMessage { .. }) && n.origin == Origin::Operator;
    let messages: Vec<String> = nodes.iter().filter(words).map(|n| n.id.clone()).collect();
    let reply = nodes
        .iter()
        .rev()
        .find(|n| matches!(n.body, Body::AssistantMessage { .. }));
    let Some(first) = nodes
        .first()
        .filter(|_| !messages.is_empty() || reply.is_some())
    else {
        return Ok(None);
    };
    let opened_by = match (&first.body, first.origin) {
        (Body::UserMessage { .. }, Origin::Operator) => Opener::Message,
        (Body::UserMessage { .. }, Origin::Mcp) => Opener::Prompt,
        _ => Opener::Unknown,
    };
    let marked = match messages.first() {
        Some(id) => mark(store, id)?,
        None => None,
    };
    Ok(Some(Exchange {
        turn_id: turn.into(),
        session_id: sid.into(),
        place: marked
            .as_ref()
            .map_or_else(|| UNKNOWN.into(), |m| m.place.clone()),
        private: false,
        opened_by,
        person: marked.as_ref().and_then(super::OwnerMark::person),
        start_ms: first.created_at_ms,
        end_ms: nodes.last().map(|n| n.created_at_ms),
        ask: messages.first().cloned(),
        reply: reply.map(|n| n.id.clone()),
        messages,
        stop: None,
        rebuilt: true,
    }))
}

/// Every conversation's turns, planned: a new exchange where none is, the
/// kept one where one is.
fn plan(store: &Store, out: &mut ExchangeRebuildResult) -> anyhow::Result<Vec<Planned>> {
    let sessions: Vec<SessionRecord> = store.list_sessions()?;
    let mut planned = Vec::new();
    for s in sessions
        .iter()
        .filter(|s| s.task.is_none() && s.imported.is_none())
    {
        out.sessions += 1;
        let nodes = store.session_nodes(&s.session_id)?;
        let mut turns: BTreeMap<(u64, String), Vec<&Node>> = BTreeMap::new();
        let mut first: HashMap<&str, u64> = HashMap::new();
        for (p, n) in &nodes {
            let Some(turn) = n.turn_id.as_deref() else {
                continue;
            };
            let at = *first.entry(turn).or_insert(*p);
            turns.entry((at, turn.to_string())).or_default().push(n);
        }
        for ((_, turn), ns) in &turns {
            out.turns += 1;
            let (ex, new) = match exchange(store, turn)? {
                Some(ex) => (ex, false),
                None => match of_turn(store, turn, &s.session_id, ns)? {
                    Some(ex) => (ex, true),
                    None => continue,
                },
            };
            out.kept += u64::from(!new);
            let texts = texts(ns, ex.ask.as_deref(), ex.reply.as_deref());
            planned.push(Planned { ex, new, texts });
        }
    }
    Ok(planned)
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f).await?
}

/// The things each plan's words name, by the index: why not, when it cannot
/// be asked.
async fn things(
    index: Option<Arc<dyn PassIndex>>,
    planned: &[Planned],
) -> Result<Vec<Vec<String>>, String> {
    let index = index.ok_or("no index is configured")?;
    let texts: Vec<String> = planned.iter().flat_map(|p| p.texts.clone()).collect();
    let mut named = Vec::with_capacity(texts.len());
    for part in texts.chunks(IndexEntitiesParams::MAX_TEXTS) {
        named.extend(
            index
                .entities(part.to_vec())
                .await
                .map_err(|e| format!("{e:?}"))?,
        );
    }
    let mut it = named.into_iter();
    Ok(planned
        .iter()
        .map(|p| {
            (0..p.texts.len())
                .flat_map(|_| it.next().unwrap_or_default())
                .collect()
        })
        .collect())
}

/// The records `planned` lacks: a new exchange's record, and every touch
/// whose key is not in the store yet.
fn records(
    store: &Store,
    planned: &[Planned],
    named: Option<&[Vec<String>]>,
) -> anyhow::Result<Vec<NewRecord>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (i, p) in planned.iter().enumerate() {
        let ex = &p.ex;
        if p.new {
            out.extend(exchange_record(ex).ok());
        }
        let (turn, sid, at) = (&ex.turn_id, &ex.session_id, ex.start_ms);
        // History whose place was never recorded is touched under `unknown`, so a look-back by date reaches it.
        let mut touches: Vec<NewRecord> = Vec::new();
        touches.extend(touch(Root::Place(&ex.place), turn, sid, at));
        if let Some(person) = &ex.person {
            touches.extend(touch(Root::Person(person), turn, sid, at));
        }
        let terms = named.and_then(|n| n.get(i)).cloned().unwrap_or_default();
        touches.extend(thing_touches(&terms, turn, sid, at));
        for r in touches {
            let key = r.key.clone().unwrap_or_default();
            if seen.insert(key.clone()) && store.inner().latest_by_key(kinds::META, &key)?.is_none()
            {
                out.push(r);
            }
        }
    }
    Ok(out)
}

/// The label touches the items' records call for and the store lacks: each flag raised on an item (removed or
/// not: it is what Jev said), at its turn's time (`item_labels::at`), as a live label is written; for an id
/// with no time, at its exchange's start, from `starts` (a planned exchange's turn, session and start, so one
/// new in this run is touched where it begins) or else the exchange on record (`item_labels::landing`).
/// A record under the prefix that reads as neither an item's labels nor their older shape
/// (`item_labels::kept`) is passed over, left as it is, and counted (the second of the pair).
fn label_touches(
    store: &Store,
    starts: &HashMap<&str, (&str, u64)>,
) -> anyhow::Result<(Vec<NewRecord>, u64)> {
    use crate::item_labels::kept::{of_record, passed_over, Kept};
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut passed = 0;
    let items = (store.inner()).latest_with_prefix(kinds::META, crate::item_labels::PREFIX)?;
    for r in items {
        let (node, raised) = match of_record(&r) {
            Kept::Whole(item) | Kept::Older(item) => (item.node, item.raised),
            Kept::Unread => {
                passed_over(r.key.as_deref().unwrap_or_default());
                passed += 1;
                continue;
            }
        };
        if raised.is_empty() {
            continue;
        }
        let turn = (store.get_node(&node)?).and_then(|(_, n)| n.turn_id);
        let at = match turn.as_deref().and_then(|t| starts.get(t).map(|s| (t, s))) {
            Some((t, &(sid, start))) => (
                t.to_string(),
                sid.to_string(),
                crate::item_labels::at(t).unwrap_or(start),
            ),
            None => match crate::item_labels::landing(store, &node) {
                Some(at) => at,
                None => continue,
            },
        };
        let flags = raised.iter().map(|l| l.flag);
        for t in crate::item_labels::touches((&at.0, &at.1, at.2), flags) {
            let key = t.key.clone().unwrap_or_default();
            if seen.insert(key.clone()) && store.inner().latest_by_key(kinds::META, &key)?.is_none()
            {
                out.push(t);
            }
        }
    }
    Ok((out, passed))
}

/// The touches keyed off their exchange's start, each moved: its record at
/// the start's key, then the old key's end, a pair that is appended in one
/// frame (`run` keeps the pairs whole), so an old key ends only with its
/// replacement written beside it. A record there that does not read as a
/// touch is not moved: it is passed over, left as it is, and counted (the
/// second of the pair).
pub(super) fn misplaced(
    store: &Store,
    starts: &HashMap<&str, u64>,
) -> anyhow::Result<(Vec<[NewRecord; 2]>, u64)> {
    let mut out = Vec::new();
    let mut passed = 0;
    for r in store
        .inner()
        .latest_with_prefix(kinds::META, TOUCH_PREFIX)?
    {
        let Some(key) = r.key.as_deref() else {
            continue;
        };
        let mut parts = key.rsplitn(3, SEP);
        let (Some(turn), Some(time), Some(root)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let at = time.parse::<u64>().ok().map(|t| u64::MAX - t);
        // A label, an item's or a picture's, belongs at its turn's time, not
        // its exchange's start.
        let Some(start) = (starts.get(turn).copied())
            .map(|s| {
                (crate::item_labels::keyed_at(root, turn))
                    .or_else(|| crate::names::pictures::keyed_at(root, turn))
                    .unwrap_or(s)
            })
            .filter(|s| Some(*s) != at)
        else {
            continue;
        };
        let Ok(mut moved) = r.decode::<super::Touch>() else {
            tracing::warn!(
                key,
                "exchanges: a record under the touches' prefix could not be read as a touch: passed over, and left as it is"
            );
            passed += 1;
            continue;
        };
        moved.at_ms = start;
        let canonical = format!("{root}{SEP}{:020}{SEP}{turn}", u64::MAX - start);
        out.push([
            NewRecord::json(kinds::META, Some(&canonical), &moved)?,
            NewRecord::delete_meta(key),
        ]);
    }
    Ok((out, passed))
}

/// `exchange.rebuild`.
pub async fn run(
    store: &Store,
    rule: &PlaceRule,
    cfg: &crate::Config,
    index: Option<Arc<dyn PassIndex>>,
    p: &ExchangeRebuildParams,
) -> anyhow::Result<ExchangeRebuildResult> {
    let reader = store.clone();
    let (mut planned, counted) = blocking(move || {
        let mut out = ExchangeRebuildResult::default();
        Ok((plan(&reader, &mut out)?, out))
    })
    .await?;
    for p in planned.iter_mut().filter(|p| p.new) {
        p.ex.private =
            p.ex.place != UNKNOWN && rule.class(cfg, Some(&p.ex.place)) == PlaceClass::Private;
    }
    let mut out = ExchangeRebuildResult {
        dry_run: p.dry_run,
        ..counted
    };
    let new: Vec<&Planned> = planned.iter().filter(|p| p.new).collect();
    out.exchanges = new.len() as u64;
    out.place_proven = new.iter().filter(|p| p.ex.place != UNKNOWN).count() as u64;
    out.person_proven = new.iter().filter(|p| p.ex.person.is_some()).count() as u64;
    let named = match planned.is_empty() {
        true => Ok(Vec::new()),
        false => things(index, &planned).await,
    };
    if let Err(why) = &named {
        out.things_skipped = Some(why.clone());
    }
    let reader = store.clone();
    let (recs, (ended, touches_passed), passed) = blocking(move || {
        let mut recs = records(&reader, &planned, named.as_deref().ok())?;
        let starts: HashMap<&str, u64> = planned
            .iter()
            .map(|p| (p.ex.turn_id.as_str(), p.ex.start_ms))
            .collect();
        let at: HashMap<&str, (&str, u64)> = (planned.iter())
            .map(|p| {
                (
                    p.ex.turn_id.as_str(),
                    (p.ex.session_id.as_str(), p.ex.start_ms),
                )
            })
            .collect();
        let (labels, passed) = label_touches(&reader, &at)?;
        recs.extend(labels);
        Ok((recs, misplaced(&reader, &starts)?, passed))
    })
    .await?;
    out.touches = (recs.len() as u64).saturating_sub(out.exchanges);
    out.repaired = ended.len() as u64;
    out.labels_passed_over = passed;
    out.touches_passed_over = touches_passed;
    if !p.dry_run {
        let writer = store.clone();
        blocking(move || {
            for batch in recs.chunks(BATCH) {
                writer.append(batch)?;
            }
            // A move's two records are never split across frames.
            for pairs in ended.chunks(BATCH / 2) {
                let batch: Vec<NewRecord> = pairs.iter().flatten().cloned().collect();
                writer.append(&batch)?;
            }
            Ok(())
        })
        .await?;
    }
    Ok(out)
}
