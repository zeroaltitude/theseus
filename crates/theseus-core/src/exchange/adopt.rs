//! `exchange.adopt` (issue 44): the owner adopts a place's unplaced history
//! into it, one place at a time (the design note: `docs/design/exchanges.md`,
//! "Adoption"). The rebuild proves a place only from a mark, so turns from
//! before the marks are `unknown` and no place holds them. Adoption is the
//! owner's own recorded act, "the conversation held in these sessions was
//! held in this place"; it proves nothing the rebuild could not, and it says
//! so by being an act, not a finding.
//!
//! - **Which sessions.** Those the store ties to this place and to no other
//!   ([`ties`]): the Discord binding's place records (`discord.session.<p>`,
//!   the session a place runs on now), the succession links of a session
//!   record (`supersedes`, `superseded_by`: the session a place moved from
//!   and to at a `/new`), a wake's or report's target (`wake.target.<s>`),
//!   the owner's marks of its messages (`said.<node>`, where each arrived),
//!   and, for a session tied to no chat place, its working directory
//!   (`local:<dir>`). A thread's session (`thread.shown.<s>`) serves a thread
//!   that spans places and is never adopted from; nor is a task's or an
//!   imported one. None of these proves where an old message arrived or who
//!   typed it: they say where the session spoke.
//! - **Which exchanges.** Of those sessions, every exchange whose place is
//!   `unknown` and that no adoption holds. One with a proven place is never
//!   touched. A turn with no exchange yet means the rebuild has not run for
//!   it: nothing is adopted then (the dry run counts them).
//! - **The gate.** A session whose seal is not proven clear (sealed,
//!   proposed, or unreadable) is left; a seal set later still hides its
//!   exchanges where they are read, as for any exchange.
//! - **What is written.** Nothing already written is written again. The
//!   adoption's record (`adoption.<id>`: the place, who, when, the sessions,
//!   the turns, the touches it wrote), one `adopted.<turn>` per exchange, and
//!   the place's touches. [`super::exchange`] reads `adopted.<turn>` for an
//!   `unknown` exchange, so every reader (window, ladder, history tool, the
//!   past) sees the place through the one call it already makes. No mark is
//!   made and no person is given: the author stays unknown.
//! - **Undo.** By its id: its `adopted.<turn>` records and the touches it
//!   wrote are ended, and its record written again with the time it was
//!   taken back. The exchanges read `unknown` again.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use theseus_protocol::exchange::{ExchangeAdoptParams, ExchangeAdoptResult};
use theseus_store::{kinds, NewRecord, Store as _};

use super::rebuild::UNKNOWN;
use super::{exchange, mark, touch, Root};
use crate::node::Body;
use crate::outbox::{PLACE_META_PREFIX, SHOWN_PREFIX, WAKE_TARGET_PREFIX};
use crate::session::SessionRecord;
use crate::store::Store;

/// One exchange's adoption: `adopted.<turn>`.
pub const ADOPTED_PREFIX: &str = "adopted.";
/// An adoption's record: `adoption.<id>`.
pub const ADOPTION_PREFIX: &str = "adoption.";
/// Records a frame holds, at most.
const BATCH: usize = 256;

/// The place an adoption gave an exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    pub adoption: String,
    pub place: String,
    /// The place's class was private when it was adopted.
    pub private: bool,
}

/// One adoption, as the owner made it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adoption {
    pub id: String,
    pub place: String,
    pub private: bool,
    /// Who made it: always the owner in person (the method refuses anyone
    /// else), and the surface he made it on.
    pub by: String,
    pub surface: String,
    pub at_ms: u64,
    pub sessions: Vec<String>,
    pub turns: Vec<String>,
    /// The touch keys it wrote: those an undo ends.
    pub touches: Vec<String>,
    #[serde(default)]
    pub undone_ms: Option<u64>,
}

/// The adoption that placed `turn`, if one does.
pub fn placed(store: &Store, turn: &str) -> anyhow::Result<Option<Placed>> {
    store.get_meta(&format!("{ADOPTED_PREFIX}{turn}"))
}

/// What the caller knows that the store does not: who asks, where, the
/// place's class now, and the gate's seal of a session.
pub struct Asker<'a> {
    pub surface: &'a str,
    pub private: bool,
    /// A session whose seal is not proven clear.
    pub sealed: &'a dyn Fn(&str) -> bool,
    pub now_ms: u64,
}

/// A `discord.session.<p>` place, as the ledger names it.
fn chat_place(p: &str) -> String {
    format!("discord:{p}")
}

/// The places the store ties `s` to, and whether it serves a thread. The
/// binding's records are read once for all sessions (`bound`).
fn ties(
    store: &Store,
    s: &SessionRecord,
    bound: &HashMap<String, BTreeSet<String>>,
) -> anyhow::Result<(BTreeSet<String>, bool)> {
    let sid = s.session_id.as_str();
    let mut out: BTreeSet<String> = bound.get(sid).cloned().unwrap_or_default();
    let links = [s.supersedes.as_ref(), s.superseded_by.as_ref()];
    out.extend(
        links
            .into_iter()
            .flatten()
            .filter_map(|l| l.place.as_deref())
            .map(chat_place),
    );
    out.extend(store.get_meta::<String>(&format!("{WAKE_TARGET_PREFIX}{sid}"))?);
    let thread = store
        .inner()
        .latest_by_key(kinds::META, &format!("{SHOWN_PREFIX}{sid}"))?
        .is_some();
    for (_, n) in store.session_nodes(sid)? {
        if matches!(n.body, Body::UserMessage { .. }) {
            out.extend(mark(store, &n.id)?.map(|m| m.place));
        }
    }
    if out.is_empty() {
        out.insert(super::place_of(None, s.dir.as_deref()));
    }
    Ok((out, thread))
}

/// Every binding's place, by the session it runs on.
fn bindings(store: &Store) -> anyhow::Result<HashMap<String, BTreeSet<String>>> {
    let mut out: HashMap<String, BTreeSet<String>> = HashMap::new();
    for r in store
        .inner()
        .latest_with_prefix(kinds::META, PLACE_META_PREFIX)?
    {
        let Some(p) = r
            .key
            .as_deref()
            .and_then(|k| k.strip_prefix(PLACE_META_PREFIX))
        else {
            continue;
        };
        if let Ok(sid) = r.decode::<String>() {
            out.entry(sid).or_default().insert(chat_place(p));
        }
    }
    Ok(out)
}

/// The plan: the sessions, the exchanges to place, what is left and why.
#[derive(Default)]
struct Plan {
    sessions: Vec<String>,
    turns: Vec<(String, String, u64)>,
    out: ExchangeAdoptResult,
}

fn left(out: &mut ExchangeAdoptResult, why: &str, n: u64) {
    if n > 0 {
        *out.left.entry(why.into()).or_insert(0) += n;
    }
}

/// The turns of `sid`, each once, by its first node.
fn turns_of(store: &Store, sid: &str) -> anyhow::Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (_, n) in store.session_nodes(sid)? {
        if let Some(t) = n.turn_id {
            if seen.insert(t.clone()) {
                out.push(t);
            }
        }
    }
    Ok(out)
}

fn plan(store: &Store, place: &str, a: &Asker<'_>) -> anyhow::Result<Plan> {
    let bound = bindings(store)?;
    let mut p = Plan::default();
    for s in store.list_sessions::<SessionRecord>()? {
        if s.task.is_some() || s.imported.is_some() {
            continue;
        }
        let (tied, thread) = ties(store, &s, &bound)?;
        if !tied.contains(place) {
            continue;
        }
        let sid = s.session_id.as_str();
        let why = match () {
            _ if thread => Some("sessions_of_a_thread"),
            _ if tied.len() > 1 => Some("sessions_tied_to_another_place_too"),
            _ if (a.sealed)(sid) => Some("sessions_sealed_or_unproven"),
            _ => None,
        };
        if let Some(why) = why {
            left(&mut p.out, why, 1);
            continue;
        }
        let mut took = false;
        for turn in turns_of(store, sid)? {
            let Some(ex) = exchange(store, &turn)? else {
                p.out.unledgered += 1;
                continue;
            };
            match (ex.place == UNKNOWN, ex.place == place) {
                (true, _) => {
                    took = true;
                    p.turns.push((turn, ex.session_id, ex.start_ms));
                }
                (false, true) if placed(store, &turn)?.is_some() => {
                    left(&mut p.out, "exchanges_adopted_already", 1)
                }
                (false, true) => left(&mut p.out, "exchanges_placed_already", 1),
                (false, false) => left(&mut p.out, "exchanges_of_another_place", 1),
            }
        }
        if took {
            p.sessions.push(s.session_id.clone());
        }
    }
    p.out.sessions = p.sessions.len() as u64;
    p.out.exchanges = p.turns.len() as u64;
    Ok(p)
}

/// `exchange.adopt`, an adoption or a dry run.
pub fn adopt(
    store: &Store,
    a: &Asker<'_>,
    params: &ExchangeAdoptParams,
) -> anyhow::Result<ExchangeAdoptResult> {
    let place = params.place.trim();
    if place.is_empty() || place == UNKNOWN || !crate::ladder::laddered(place) {
        anyhow::bail!("{place:?} is not a place history can be adopted into");
    }
    let mut p = plan(store, place, a)?;
    p.out.dry_run = params.dry_run;
    p.out.place = place.into();
    let mut touches = Vec::new();
    for (turn, sid, at) in &p.turns {
        if let Some(r) = touch(Root::Place(place), turn, sid, *at) {
            let key = r.key.clone().unwrap_or_default();
            if store.inner().latest_by_key(kinds::META, &key)?.is_none() {
                touches.push(r);
            }
        }
    }
    p.out.touches = touches.len() as u64;
    if params.dry_run || p.turns.is_empty() {
        return Ok(p.out);
    }
    if p.out.unledgered > 0 {
        anyhow::bail!(
            "{} turns of these sessions have no exchange yet: run `theseus ledger rebuild-exchanges` first",
            p.out.unledgered
        );
    }
    let id = uuid::Uuid::new_v4().to_string();
    let placed = Placed {
        adoption: id.clone(),
        place: place.into(),
        private: a.private,
    };
    let record = Adoption {
        id: id.clone(),
        place: place.into(),
        private: a.private,
        by: super::OWNER.into(),
        surface: a.surface.into(),
        at_ms: a.now_ms,
        sessions: p.sessions.clone(),
        turns: p.turns.iter().map(|t| t.0.clone()).collect(),
        touches: touches.iter().filter_map(|r| r.key.clone()).collect(),
        undone_ms: None,
    };
    let mut recs = Vec::new();
    for (turn, _, _) in &p.turns {
        recs.push(NewRecord::json(
            kinds::META,
            Some(&format!("{ADOPTED_PREFIX}{turn}")),
            &placed,
        )?);
    }
    recs.extend(touches);
    // The adoption's own record goes last, so an adoption that is recorded
    // was written whole.
    for batch in recs.chunks(BATCH) {
        store.append(batch)?;
    }
    store.append(&[adoption_record(&record)?])?;
    p.out.id = Some(id);
    Ok(p.out)
}

fn adoption_record(a: &Adoption) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&format!("{ADOPTION_PREFIX}{}", a.id)), a)
}

/// `exchange.adopt` with `undo`: the adoption `id` taken back.
pub fn undo(
    store: &Store,
    a: &Asker<'_>,
    params: &ExchangeAdoptParams,
    id: &str,
) -> anyhow::Result<ExchangeAdoptResult> {
    let Some(mut rec) = store.get_meta::<Adoption>(&format!("{ADOPTION_PREFIX}{id}"))? else {
        anyhow::bail!("no adoption {id:?}");
    };
    if rec.place != params.place.trim() {
        anyhow::bail!(
            "adoption {id:?} adopted into {:?}, not {:?}",
            rec.place,
            params.place
        );
    }
    if rec.undone_ms.is_some() {
        anyhow::bail!("adoption {id:?} was taken back already");
    }
    let mut out = ExchangeAdoptResult {
        dry_run: params.dry_run,
        place: rec.place.clone(),
        id: Some(id.into()),
        undone: !params.dry_run,
        sessions: rec.sessions.len() as u64,
        touches: rec.touches.len() as u64,
        ..ExchangeAdoptResult::default()
    };
    let mut ends = Vec::new();
    for turn in &rec.turns {
        // Only this adoption's own record: never one a later act wrote.
        if placed(store, turn)?.is_some_and(|p| p.adoption == id) {
            ends.push(NewRecord::delete_meta(&format!("{ADOPTED_PREFIX}{turn}")));
        }
    }
    out.exchanges = ends.len() as u64;
    if params.dry_run {
        return Ok(out);
    }
    ends.extend(rec.touches.iter().map(|k| NewRecord::delete_meta(k)));
    for batch in ends.chunks(BATCH) {
        store.append(batch)?;
    }
    rec.undone_ms = Some(a.now_ms);
    store.append(&[adoption_record(&rec)?])?;
    Ok(out)
}
