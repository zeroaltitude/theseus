//! `fact.retract` (theseus-2ced, the killer-features plan's R13): the owner
//! marks one node wrong, and every place it spread is found and told.
//!
//! - **The act** (`Core::fact_retract`): the `wrong` label through the
//!   label path recall already reads (`memory.label`'s row, scoped `memory`),
//!   so recall never serves the node again; then `node.reach`'s walk, to its
//!   caps, over the copies; each exposure is sorted into one of
//!   `exposure_kind`'s five, and what each kind gets is written in the same
//!   frame as the `fact.retracted` row that holds the table:
//!   - a **session** that goes on gets a correction note, queued here and
//!     written into its tail at its next turn (`Notes`, the turn's
//!     `turn/retract_step.rs`): the harness's own user-role node, as a wake's
//!     or a report's is, once;
//!   - a **synthesis** that cites it is labeled `stale` (recall leaves it
//!     out), and consolidation leaves a wrong node out of its clusters, so the
//!     cluster is proposed again without it at its next run;
//!   - a **task** that used it is named on one card to the owner's DM
//!     ("re-run?"): one per retraction, never one per task;
//!   - a **published copy** is listed with its place (and labeled `wrong`: it is
//!     the node's text); it cannot be unpublished;
//!   - **seen, not used** is listed.
//! - **Idempotent**: a node retracted before answers from the stored table
//!   and writes nothing.
//! - **The place rule**: the answer is shaped for the place the request is
//!   run for. A shared place's never shows a private exposure's text (its
//!   title, or the node's own words when the node's session is private).
//! - **FAST**: an operator's act, off every turn. The walk is reach's: a scan
//!   per session reached. A turn costs nothing unless a note is pending
//!   (`Notes::waiting` is two atomic loads).
//!
//! The note's queue is one META record (`retract.notes`: session to notes), a
//! new key, so no format change. A note leaves it in the frame that writes it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_memory::recall::Place;
use theseus_protocol::retract::{
    exposure_kind as kind, FactRetractParams, FactRetractResult, RetractExposure,
};
use theseus_protocol::{now_unix_ms, short_id, LedgerKind};
use theseus_store::{kinds, NewRecord};

use crate::fact;
use crate::graph;
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::reach::{self, Named};
use crate::recall::labels;
use crate::rpc::{Act, Core};
use crate::session::SessionRecord;
use crate::store::Store;

/// The scope every `fact.retracted` row is kept under.
pub const SCOPE: &str = "retract";
/// The META key of the notes waiting for a turn.
pub const NOTES_KEY: &str = "retract.notes";
/// The author of a correction note's node.
pub const NOTE_AUTHOR: &str = "harness:retraction";
/// The longest gist of a node a note or a card quotes.
const GIST_CHARS: usize = 160;
/// The longest card.
const CARD_CHARS: usize = 1_800;

/// A correction note waiting for its session's next turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// The retracted node.
    pub node_id: String,
    /// The note's words.
    pub text: String,
    pub at_ms: u64,
}

type Pending = BTreeMap<String, Vec<Note>>;

/// The notes waiting, by session: read from the META record once, kept in
/// memory, and changed only together with a frame that writes the record.
#[derive(Default)]
pub struct Notes {
    held: Mutex<Option<Pending>>,
    /// The record has been read.
    loaded: AtomicBool,
    /// Some session has a note waiting (a lower bound is never kept: it is
    /// set before a note is written and cleared after the last leaves).
    any: AtomicBool,
}

impl Notes {
    /// Whether `session` has a note waiting. Two atomic loads once the record
    /// is read, and none waiting; the first call reads it.
    pub fn waiting(&self, store: &Store, session: &str) -> bool {
        if self.loaded.load(Ordering::Acquire) && !self.any.load(Ordering::Acquire) {
            return false;
        }
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        match self.pending(&mut held, store) {
            Ok(p) => p.contains_key(session),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "retraction: the waiting notes cannot be read");
                false
            }
        }
    }

    /// The map, read on first use.
    fn pending<'a>(&self, held: &'a mut Option<Pending>, store: &Store) -> Result<&'a mut Pending> {
        if held.is_none() {
            let read: Pending = store.get_meta(NOTES_KEY)?.unwrap_or_default();
            self.any.store(!read.is_empty(), Ordering::Release);
            *held = Some(read);
            self.loaded.store(true, Ordering::Release);
        }
        Ok(held.as_mut().expect("read above"))
    }

    /// Queue `notes` (a session each) and write `frame` with the record that
    /// holds them, in one frame. A note for a node the session already waits
    /// on is not queued twice.
    pub fn add(
        &self,
        store: &Store,
        mut frame: Vec<NewRecord>,
        notes: Vec<(String, Note)>,
    ) -> Result<Vec<u64>> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = self.pending(&mut held, store)?.clone();
        for (session, note) in notes {
            let waiting = next.entry(session).or_default();
            if !waiting.iter().any(|n| n.node_id == note.node_id) {
                waiting.push(note);
            }
        }
        frame.push(NewRecord::json(kinds::META, Some(NOTES_KEY), &next)?);
        // Marked before the write: a turn that looks while it lands reads the
        // lock, and so waits for the map.
        self.any.store(true, Ordering::Release);
        let positions = store.append(&frame)?;
        *held = Some(next);
        Ok(positions)
    }

    /// Take `session`'s notes: `build` makes the records that write them (their
    /// nodes and rows), and the frame that writes those also removes them from
    /// the record. None when it has none waiting.
    pub fn deliver(
        &self,
        store: &Store,
        session: &str,
        build: impl FnOnce(&[Note]) -> Result<Vec<NewRecord>>,
    ) -> Result<Option<Vec<Note>>> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = self.pending(&mut held, store)?.clone();
        let Some(notes) = next.remove(session) else {
            return Ok(None);
        };
        let mut frame = build(&notes)?;
        frame.push(NewRecord::json(kinds::META, Some(NOTES_KEY), &next)?);
        store.append(&frame)?;
        self.any.store(!next.is_empty(), Ordering::Release);
        *held = Some(next);
        Ok(Some(notes))
    }
}

/// An exposure as the row keeps it: the wire's, and whether its place is
/// private (a shared place's answer leaves such a one's words out).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    #[serde(flatten)]
    pub e: RetractExposure,
    #[serde(default)]
    pub private: bool,
}

/// What a `fact.retracted` row holds, read back.
#[derive(Debug, Clone, Deserialize)]
struct Stored {
    node_id: String,
    session_id: String,
    gist: String,
    why: String,
    by: String,
    at_ms: u64,
    exposures: Vec<Found>,
    #[serde(default)]
    partial: bool,
    #[serde(default)]
    card: bool,
    #[serde(default)]
    private: bool,
}

/// The first words of a node, on one line.
fn gist_of(n: &Node) -> String {
    let text = crate::recall::text_of(n);
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= GIST_CHARS {
        return one;
    }
    let cut: String = one.chars().take(GIST_CHARS - 1).collect();
    format!("{cut}…")
}

/// `[m:a1b2c3]` or `m:a1b2c3` as the ending it carries; anything else as it
/// is. A handle is read as an id's ending until handles are the recall's own
/// (theseus-ejdk).
fn handle_ending(name: &str) -> &str {
    let n = name.trim();
    let inner = n
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(n);
    inner.strip_prefix("m:").unwrap_or(inner)
}

/// What the walk knows of one session.
#[derive(Clone)]
struct Sess {
    task: bool,
    retired: bool,
    imported: bool,
    private: bool,
    /// Where, in words, for a reader who may read it.
    place: String,
}

impl Core {
    /// `fact.retract`: see the module's words.
    pub fn fact_retract(
        &self,
        p: &FactRetractParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> Result<FactRetractResult> {
        let who = who.into();
        let why = p.why.trim();
        if why.is_empty() {
            bail!("a retraction says why: give the reason in a sentence");
        }
        let may_read = match p.session_id.as_deref() {
            Some(sid) => {
                if self.store.get_session::<SessionRecord>(sid)?.is_none() {
                    bail!("no session {sid}");
                }
                matches!(self.runner.place_of(sid), Place::Private)
            }
            None => true,
        };
        let node_id = self.retract_target(&p.node)?;
        let what = format!("the retraction of {node_id}");
        self.judge_act(&who, Act::Retract { what: &what })?;
        if let Some(done) = self.retraction_of(&node_id)? {
            return Ok(shown(done, true, may_read));
        }
        let (_, node) = self
            .store
            .get_node(&node_id)?
            .with_context(|| format!("no node is named {node_id}"))?;
        let walk = reach::reach(&self.store, &node_id, Some(reach::MAX_GENERATIONS))?
            .with_context(|| format!("no node is named {node_id}"))?;
        let now = now_unix_ms();
        let gist = gist_of(&node);
        let found = self.exposures_of(&walk)?;
        let private_origin = self.sess(&node.session_id)?.is_none_or(|s| s.private);
        let (who_s, via) = (who.who(), who.via());

        // The labels: the node wrong, each synthesis that cites it stale, each
        // published copy of it wrong. All through `memory.label`'s row.
        let mut labeled: Vec<(String, &'static str)> = vec![(node_id.clone(), "wrong")];
        for f in &found {
            match f.e.kind.as_str() {
                kind::SYNTHESIS => labeled.push((f.e.node_id.clone(), "stale")),
                kind::PUBLISHED => labeled.push((f.e.node_id.clone(), "wrong")),
                _ => {}
            }
        }
        let notes = notes_for(&found, &node_id, &gist, why, now);
        let tasks: Vec<&Found> = found.iter().filter(|f| f.e.kind == kind::TASK).collect();
        let card_text = card_text(&gist, why, &tasks);
        let dm = !tasks.is_empty() && self.runner.place_rule.owner_dm_bound();

        let mut frame = Vec::new();
        let memory = &self.runner.memory;
        let label_note = format!("retracted: {why}");
        for (id, label) in &labeled {
            frame.push(label_row(id, label, &label_note, &who_s, &via)?);
        }
        let post = if dm {
            let body = json!({"kind": "retraction", "text": card_text});
            let (a, records) = self.outbox.stage_to_operator(None, body)?;
            frame.extend(records);
            Some(a)
        } else {
            None
        };
        let card = post.is_some();
        let f = fact::retract::Retracted {
            node_id: &node_id,
            session_id: &node.session_id,
            gist: &gist,
            why,
            who: &who_s,
            via: &via,
            at_ms: now,
            exposures: &found,
            partial: walk.partial,
            card,
            private: private_origin,
            labeled: &labeled,
        };
        frame.push(fact::row(&f, Some(&node.session_id), None)?.scoped(SCOPE));
        let records = frame.clone();
        let positions = memory.corrections.add(&self.store, frame, notes)?;
        for (id, label) in &labeled {
            memory.labeled_now(id, label);
        }
        memory.retention_written(&records, &positions);
        if let Some(a) = post {
            self.outbox.posted(&a);
        }
        self.rec(None).announce(&f);
        Ok(shown(
            Stored {
                node_id,
                session_id: node.session_id,
                gist,
                why: why.to_string(),
                by: who_s,
                at_ms: now,
                exposures: found,
                partial: walk.partial,
                card,
                private: private_origin,
            },
            false,
            may_read,
        ))
    }

    /// The node a name means: a whole id, an id's ending, or a handle.
    fn retract_target(&self, name: &str) -> Result<String> {
        let n = handle_ending(name);
        if n.is_empty() {
            bail!("name the node to retract: its id, the last six or more characters of it, or a handle");
        }
        if self.store.get_node(n)?.is_some() {
            return Ok(n.to_string());
        }
        match reach::resolve(&self.store, n)? {
            Named::One(id) => Ok(id),
            Named::Unknown(w) | Named::Refused(w) => bail!("{w}"),
        }
    }

    /// The table a node's first retraction stored, if it has been retracted.
    fn retraction_of(&self, node_id: &str) -> Result<Option<Stored>> {
        for r in self.store.scope_after(SCOPE, 0)? {
            if r.kind != kinds::LEDGER {
                continue;
            }
            let row: LedgerRow = r.decode()?;
            if row.kind == LedgerKind::FactRetracted.as_str() && row.data["node_id"] == node_id {
                return Ok(Some(serde_json::from_value(row.data)?));
            }
        }
        Ok(None)
    }

    /// What is known of a session; None for one with no record.
    fn sess(&self, id: &str) -> Result<Option<Sess>> {
        let Some(rec) = self.store.get_session::<SessionRecord>(id)? else {
            return Ok(None);
        };
        let place = self.runner.place_of(id);
        let private = !matches!(place, Place::Shared(_));
        let retired = rec
            .state_at(self.cfg.sessions.rule(), now_unix_ms(), false)
            .0
            == theseus_protocol::SessionState::Retired;
        let words = if private {
            let title = rec.title.as_deref().or(rec.label.as_deref());
            match title {
                Some(t) => format!("{} · {t}", short_id(id)),
                None => short_id(id),
            }
        } else {
            self.runner.place_name(id)
        };
        Ok(Some(Sess {
            task: rec.task.is_some(),
            retired,
            imported: rec.imported.is_some(),
            private,
            place: words,
        }))
    }

    /// `sess`, read once a walk.
    fn cached(&self, cache: &mut BTreeMap<String, Option<Sess>>, id: &str) -> Result<Option<Sess>> {
        if !cache.contains_key(id) {
            cache.insert(id.to_string(), self.sess(id)?);
        }
        Ok(cache[id].clone())
    }

    /// Whether the turns of `session` used what they recalled of `copied`:
    /// `memory.used` rows say, any one that says so; none, unknown.
    fn used_of(&self, session: &str, copied: &str) -> Result<Option<bool>> {
        let mut seen = None;
        for r in self.store.scope_after(&fact::recall::scope(session), 0)? {
            if r.kind != kinds::LEDGER {
                continue;
            }
            let row: LedgerRow = r.decode()?;
            if row.kind != LedgerKind::MemoryUsed.as_str() || row.data["node_id"] != copied {
                continue;
            }
            match row.data["used"].as_bool() {
                Some(true) => return Ok(Some(true)),
                Some(false) => seen = Some(false),
                None => {}
            }
        }
        Ok(seen)
    }

    /// Every exposure the walk found, sorted: by kind in table order, and in
    /// the walk's order within one.
    fn exposures_of(&self, walk: &theseus_protocol::NodeReachResult) -> Result<Vec<Found>> {
        let memory_session = self.memory_session()?;
        let mut out: Vec<Found> = Vec::new();
        let mut sessions: BTreeMap<String, Option<Sess>> = BTreeMap::new();
        // The node's own session, if its context held it.
        let direct = &walk.direct;
        if (!direct.compilations.is_empty() || direct.loops > 0)
            && memory_session.as_deref() != Some(walk.session_id.as_str())
        {
            if let Some(s) = self.cached(&mut sessions, &walk.session_id)? {
                if !s.imported {
                    let k = if s.task { kind::TASK } else { kind::SESSION };
                    out.push(Found {
                        e: RetractExposure {
                            kind: k.into(),
                            session_id: walk.session_id.clone(),
                            node_id: walk.node_id.clone(),
                            generation: 0,
                            place: s.place.clone(),
                            at_ms: direct.last_ms,
                            used: None,
                            what: what_of(k, s.retired, ""),
                            hidden: false,
                        },
                        private: s.private,
                    });
                }
            }
        }
        for d in &walk.descendants {
            let Some(s) = self.cached(&mut sessions, &d.session_id)? else {
                continue;
            };
            let (k, used) = match d.route.as_str() {
                graph::VIA_SYNTHESIS => (kind::SYNTHESIS, None),
                graph::VIA_PUBLISH => (kind::PUBLISHED, None),
                graph::VIA_RECALL => {
                    let used = self.used_of(&d.session_id, &d.from)?;
                    match used {
                        Some(false) => (kind::SEEN, used),
                        _ if s.task => (kind::TASK, used),
                        _ if s.retired || s.imported => (kind::SEEN, used),
                        _ => (kind::SESSION, used),
                    }
                }
                graph::VIA_ARRANGEMENT | graph::VIA_BRIEF | graph::VIA_CLAIM => {
                    (kind::TASK, Some(true))
                }
                graph::VIA_REPORT if !s.task && !s.retired => (kind::SESSION, None),
                _ => (kind::SEEN, None),
            };
            let detail = match (k, used) {
                (kind::SEEN, None) => format!("a copy by {}", d.route),
                (kind::SESSION | kind::TASK, None) if d.route == graph::VIA_RECALL => {
                    "its use is not yet recorded: treated as used".to_string()
                }
                _ => String::new(),
            };
            out.push(Found {
                e: RetractExposure {
                    kind: k.into(),
                    session_id: d.session_id.clone(),
                    node_id: d.node_id.clone(),
                    generation: d.generation,
                    place: if k == kind::SYNTHESIS {
                        format!("synthesis {}", short_id(&d.node_id))
                    } else {
                        s.place.clone()
                    },
                    at_ms: d.exposure.last_ms,
                    used,
                    what: what_of(k, s.retired, &detail),
                    hidden: false,
                },
                private: s.private,
            });
        }
        Ok(merge(out))
    }
}

/// The correction note for each session that goes on: one text, a note each.
fn notes_for(
    found: &[Found],
    node_id: &str,
    gist: &str,
    why: &str,
    now: u64,
) -> Vec<(String, Note)> {
    let note_text = format!(
        "Correction: {} was marked wrong on {}: {why}",
        if gist.is_empty() {
            format!("a recalled item ({node_id})")
        } else {
            format!("\u{201c}{gist}\u{201d}")
        },
        crate::judge::spend::local_day(now),
    );
    found
        .iter()
        .filter(|f| f.e.kind == kind::SESSION)
        .map(|f| {
            (
                f.e.session_id.clone(),
                Note {
                    node_id: node_id.to_string(),
                    text: note_text.clone(),
                    at_ms: now,
                },
            )
        })
        .collect()
}

/// One `memory.label` row, as the label path writes it.
fn label_row(id: &str, label: &str, note: &str, who: &str, via: &str) -> Result<NewRecord> {
    let l = fact::recall::Labeled {
        node_id: id,
        label,
        recall_id: None,
        note: Some(note),
        who,
        via,
        excluded: true,
    };
    Ok(fact::row(&l, None, None)?.scoped(labels::SCOPE))
}

/// What happens to an exposure of `k`, in a sentence.
fn what_of(k: &str, retired: bool, detail: &str) -> String {
    let base = match k {
        kind::SESSION => "a correction note goes into its tail at its next turn".to_string(),
        kind::SYNTHESIS => "labeled stale: recall leaves it out, and consolidation proposes its \
             cluster again without the node"
            .to_string(),
        kind::TASK => "named on the card to the owner: re-run?".to_string(),
        kind::PUBLISHED => "posted to a place: it cannot be unpublished".to_string(),
        _ if retired => "in a retired session: nothing to do".to_string(),
        _ => "seen, not used: nothing to do".to_string(),
    };
    if detail.is_empty() {
        base
    } else {
        format!("{base} ({detail})")
    }
}

/// One exposure per session and kind (per node, for a synthesis or a
/// published copy), the stronger kind of a session keeping it over its
/// `seen`, in table order.
fn merge(all: Vec<Found>) -> Vec<Found> {
    let acting: BTreeSet<String> = all
        .iter()
        .filter(|f| matches!(f.e.kind.as_str(), kind::SESSION | kind::TASK))
        .map(|f| f.e.session_id.clone())
        .collect();
    let mut seen_keys = BTreeSet::new();
    let mut kept: Vec<Found> = Vec::new();
    for f in all {
        let key = match f.e.kind.as_str() {
            kind::SYNTHESIS | kind::PUBLISHED => (f.e.kind.clone(), f.e.node_id.clone()),
            _ => (f.e.kind.clone(), f.e.session_id.clone()),
        };
        if f.e.kind == kind::SEEN && acting.contains(&f.e.session_id) {
            continue;
        }
        if seen_keys.insert(key) {
            kept.push(f);
        }
    }
    let order = |k: &str| {
        kind::ALL
            .iter()
            .position(|a| *a == k)
            .unwrap_or(kind::ALL.len())
    };
    kept.sort_by_key(|f| order(&f.e.kind));
    kept
}

/// The card to the owner: the tasks that used the node, to re-run.
fn card_text(gist: &str, why: &str, tasks: &[&Found]) -> String {
    let mut t = format!(
        "\u{26a0}\u{fe0f} Retracted: \u{201c}{gist}\u{201d} was marked wrong ({why}). {} task{} used it \
         and may need a re-run:",
        tasks.len(),
        if tasks.len() == 1 { "" } else { "s" }
    );
    for f in tasks {
        t.push_str(&format!("\n\u{2022} {}", f.e.place));
    }
    t.push_str("\nRe-run?");
    if t.chars().count() > CARD_CHARS {
        let cut: String = t.chars().take(CARD_CHARS - 1).collect();
        return format!("{cut}…");
    }
    t
}

/// A stored table as the place the request is run for may read it.
fn shown(s: Stored, already: bool, may_read: bool) -> FactRetractResult {
    let exposures = s
        .exposures
        .into_iter()
        .map(|f| {
            let mut e = f.e;
            if f.private && !may_read {
                e.place = "a private place".into();
                e.hidden = true;
            }
            e
        })
        .collect();
    FactRetractResult {
        node_id: s.node_id,
        session_id: s.session_id,
        gist: (!s.gist.is_empty() && (may_read || !s.private)).then_some(s.gist),
        why: s.why,
        by: s.by,
        at_ms: s.at_ms,
        already,
        exposures,
        partial: s.partial,
        card: s.card,
    }
}

/// The user-role node a note becomes in `session`.
pub fn note_node(session: &str, turn: &str, note: &Note) -> Node {
    let mut n = Node::relayed(
        session,
        Some(turn),
        crate::node::Origin::Harness,
        NOTE_AUTHOR,
        &note.text,
    );
    debug_assert!(matches!(n.body, Body::UserMessage { .. }));
    n.author = Some(NOTE_AUTHOR.into());
    n
}
