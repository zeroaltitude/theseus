//! `exchange.rebuild` (issue 44, step 1): the exchanges and touches of turns
//! from before the ledger, written from their nodes. It reads the
//! conversations and writes only these records, and only for a turn that has
//! no exchange yet: one written as it happened holds what no node says (the
//! mark, where a message arrived), and is kept as it is.
//!
//! A place and a person are filled only where a stored record proves them:
//! the binding's record of a place's session, a `/new`'s link to the place it
//! moved, a thread's shown place; a person only in a direct conversation with
//! an owner, which the binding lets nobody else reach. Every other exchange
//! says `unknown` and names no person: a terminal's message from before the
//! mark is not proven anyone's. The dry run counts and writes nothing.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use theseus_protocol::exchange::{ExchangeRebuildParams, ExchangeRebuildResult};
use theseus_protocol::index::IndexEntitiesParams;
use theseus_store::{kinds, NewRecord, Store as _};

use super::{exchange, exchange_record, touch, Exchange, Opener, Root, OWNER};
use crate::memory_pass::PassIndex;
use crate::node::{Body, Node, Origin};
use crate::places::{PlaceClass, PlaceRule};
use crate::session::SessionRecord;
use crate::store::Store;

/// The place of history no record proves.
pub const UNKNOWN: &str = "unknown";
/// Records a frame of the rebuild holds, at most.
const BATCH: usize = 256;
/// The most of a message or reply the index is asked to name things in.
const TEXT: usize = 16 * 1024;

/// A turn's new exchange, and the words its things are named from.
struct Planned {
    ex: Exchange,
    texts: Vec<String>,
}

/// Each session's place, by what the store records of it.
fn places(store: &Store, sessions: &[SessionRecord]) -> anyhow::Result<HashMap<String, String>> {
    let inner = store.inner();
    let mut out = HashMap::new();
    for r in inner.latest_with_prefix(kinds::META, crate::outbox::PLACE_META_PREFIX)? {
        let place =
            r.key.as_deref().unwrap_or("")[crate::outbox::PLACE_META_PREFIX.len()..].to_string();
        if let (false, Ok(sid)) = (
            theseus_protocol::threads::slug_of_place(&place).is_some(),
            r.decode::<String>(),
        ) {
            out.insert(sid, format!("discord:{place}"));
        }
    }
    for r in inner.latest_with_prefix(kinds::META, crate::outbox::SHOWN_PREFIX)? {
        let sid = r.key.as_deref().unwrap_or("")[crate::outbox::SHOWN_PREFIX.len()..].to_string();
        if let Ok(target) = r.decode::<String>() {
            out.entry(sid).or_insert(target);
        }
    }
    for s in sessions {
        let moved = [&s.superseded_by, &s.supersedes]
            .into_iter()
            .flatten()
            .find_map(|l| l.place.as_ref());
        if let Some(place) = moved {
            out.entry(s.session_id.clone())
                .or_insert_with(|| format!("discord:{place}"));
        }
    }
    Ok(out)
}

fn cut(text: &str) -> String {
    let mut at = text.len().min(TEXT);
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text[..at].to_string()
}

/// One turn's nodes, as an exchange; none for a turn with no message and no
/// reply (a task's brief, a turn that only failed).
fn of_turn(turn: &str, sid: &str, place: &str, nodes: &[&Node], owner: bool) -> Option<Planned> {
    let words =
        |n: &&&Node| matches!(n.body, Body::UserMessage { .. }) && n.origin == Origin::Operator;
    let messages: Vec<String> = nodes.iter().filter(words).map(|n| n.id.clone()).collect();
    let reply = nodes
        .iter()
        .rev()
        .find(|n| matches!(n.body, Body::AssistantMessage { .. }));
    if messages.is_empty() && reply.is_none() {
        return None;
    }
    let first = nodes.first()?;
    let opened_by = match (&first.body, first.origin) {
        (Body::UserMessage { .. }, Origin::Operator) => Opener::Message,
        (Body::UserMessage { .. }, Origin::Mcp) => Opener::Prompt,
        _ => Opener::Unknown,
    };
    let mut texts = Vec::new();
    if let Some(Body::UserMessage { text, .. }) = nodes.iter().find(words).map(|n| &n.body) {
        texts.push(cut(text));
    }
    if let Some(Body::AssistantMessage { blocks, .. }) = reply.map(|n| &n.body) {
        texts.push(cut(&crate::provider::text_of(blocks)));
    }
    let ex = Exchange {
        turn_id: turn.into(),
        session_id: sid.into(),
        place: place.into(),
        private: false,
        opened_by,
        person: (owner && opened_by == Opener::Message).then(|| OWNER.into()),
        start_ms: first.created_at_ms,
        end_ms: nodes.last().map(|n| n.created_at_ms),
        ask: messages.first().cloned(),
        reply: reply.map(|n| n.id.clone()),
        messages,
        stop: None,
        rebuilt: true,
    };
    Some(Planned { ex, texts })
}

/// Every conversation's turns that have no exchange yet, planned. `owners`
/// are the owner's handles (`discord:<id>`).
fn plan(
    store: &Store,
    owners: &std::collections::BTreeSet<String>,
    out: &mut ExchangeRebuildResult,
) -> anyhow::Result<Vec<Planned>> {
    let sessions: Vec<SessionRecord> = store.list_sessions()?;
    let sessions: Vec<SessionRecord> = sessions
        .into_iter()
        .filter(|s| s.task.is_none() && s.imported.is_none())
        .collect();
    let placed = places(store, &sessions)?;
    let mut planned = Vec::new();
    for s in &sessions {
        out.sessions += 1;
        let place = placed.get(&s.session_id).map_or(UNKNOWN, String::as_str);
        let owner = place
            .strip_prefix("discord:dm:")
            .is_some_and(|u| owners.contains(&format!("discord:{u}")));
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
            if exchange(store, turn)?.is_some() {
                out.kept += 1;
                continue;
            }
            if let Some(p) = of_turn(turn, &s.session_id, place, ns, owner) {
                out.place_proven += u64::from(place != UNKNOWN);
                out.person_proven += u64::from(p.ex.person.is_some());
                planned.push(p);
            }
        }
    }
    Ok(planned)
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f).await?
}

/// The things each plan's words name, by the index: `None` and why, when it
/// cannot be asked.
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

/// The records of `planned`, `named` beside them.
fn records(planned: &[Planned], named: Option<&[Vec<String>]>) -> Vec<NewRecord> {
    let mut out = Vec::new();
    for (i, p) in planned.iter().enumerate() {
        let ex = &p.ex;
        out.extend(exchange_record(ex).ok());
        let (turn, sid, at) = (&ex.turn_id, &ex.session_id, ex.start_ms);
        if ex.place != UNKNOWN {
            out.extend(touch(Root::Place(&ex.place), turn, sid, at));
        }
        if let Some(person) = &ex.person {
            out.extend(touch(Root::Person(person), turn, sid, at));
        }
        for term in named.and_then(|n| n.get(i)).into_iter().flatten() {
            out.extend(touch(Root::Thing(term), turn, sid, at));
        }
    }
    out
}

/// `exchange.rebuild`.
pub async fn run(
    store: &Store,
    rule: &PlaceRule,
    cfg: &crate::Config,
    index: Option<Arc<dyn PassIndex>>,
    p: &ExchangeRebuildParams,
) -> anyhow::Result<ExchangeRebuildResult> {
    let owners = rule.owners(cfg);
    let reader = store.clone();
    let (mut planned, counted) = blocking(move || {
        let mut out = ExchangeRebuildResult::default();
        Ok((plan(&reader, &owners, &mut out)?, out))
    })
    .await?;
    for p in &mut planned {
        p.ex.private =
            p.ex.place != UNKNOWN && rule.class(cfg, Some(&p.ex.place)) == PlaceClass::Private;
    }
    let mut out = ExchangeRebuildResult {
        dry_run: p.dry_run,
        ..counted
    };
    out.exchanges = planned.len() as u64;
    let named = match planned.is_empty() {
        true => Ok(Vec::new()),
        false => things(index, &planned).await,
    };
    if let Err(why) = &named {
        out.things_skipped = Some(why.clone());
    }
    let recs = records(&planned, named.as_deref().ok());
    out.touches = (recs.len() as u64).saturating_sub(out.exchanges);
    if !p.dry_run {
        let writer = store.clone();
        blocking(move || {
            for batch in recs.chunks(BATCH) {
                writer.append(batch)?;
            }
            Ok(())
        })
        .await?;
    }
    Ok(out)
}
