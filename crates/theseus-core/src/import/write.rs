//! The import's writes (theseus-0lrr.6): a batch of an episode file's
//! lines into imported sessions, a tag's erase, and the tags' counts.
//!
//! - **One frame per batch**, never one per message: a batch's sessions,
//!   their nodes, the tags' counts and the batch's row together, cut into a
//!   second frame only past `FRAME_RECORDS` or `FRAME_BYTES`. Between two
//!   frames the writer waits while the machine is busy
//!   (`theseus_store::pressure`), and it holds no store lock but the
//!   writer's own, a frame at a time, so the daemon answers throughout. A
//!   stop ends the writer at the first boundary that finds it
//!   (theseus-autz), with what it wrote adding up, so the stop's drop of
//!   the runtime waits a frame, never the whole batch or erase.
//! - **The counts ride with what they count.** Every frame of a batch or an
//!   erase carries its tags' counts as they stand after it, so a reader
//!   lags a frame at most, and a kill or a crash between two frames leaves
//!   them whole. An erase's count of the tag's erased sessions is absolute,
//!   counted as it reads the tag's sessions, never added to the stored one:
//!   a run cut between frames before this rule left a count short of its
//!   tombstones, and the next erase of the tag counts it whole
//!   (theseus-mce3).
//! - **Idempotent.** An episode is found by its session's key: one imported
//!   with the same hash is skipped, one with another hash is rejected and
//!   named, never overwritten; an erased one is not imported again.
//! - **One import or erase at a time** (`ONE`): a tag's counts are read and
//!   written by one writer.
//!
//! These run on a blocking thread (the RPC's `spawn_blocking`), never on a
//! runtime worker.

use sha2::Digest as _;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::Result;
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEpisodesResult, ImportEraseResult, ImportListResult,
    ImportRejected, ImportTagInfo,
};
use theseus_protocol::import_sync::ImportTouched;
use theseus_protocol::SessionKind;
use theseus_store::{kinds, NewRecord, Store as _};

use super::append;
use super::episode::{self, Episode};
use super::{
    is_imported, node_id_of, session_id_of, summary_id_of, tag_key, tag_scope, Erased,
    ImportedFrom, TagCounts,
};
use crate::fact;
use crate::node::{Body, Node};
use crate::session::SessionRecord;
use crate::store::Store;

/// A frame's records past which a batch is cut into another frame.
pub const FRAME_RECORDS: usize = 4_000;
/// A frame's bytes past which a batch is cut into another frame.
pub const FRAME_BYTES: usize = 8 << 20;

/// One import, erase, or topics run (`topics`) at a time.
pub(super) static ONE: Mutex<()> = Mutex::new(());

/// A frame being built: its records, their bytes, the tags' counts it
/// moves, and the records past which it is cut (`FRAME_RECORDS`).
#[derive(Default)]
pub(super) struct Frame {
    records: Vec<NewRecord>,
    bytes: usize,
    /// Per tag: episodes imported, nodes written, episodes appended to.
    imported: BTreeMap<String, (u64, u64, u64)>,
    cap: usize,
}

impl Frame {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            ..Self::default()
        }
    }

    pub(super) fn push(&mut self, r: NewRecord) {
        self.bytes += r.payload.len();
        self.records.push(r);
    }

    fn full(&self) -> bool {
        self.records.len() >= self.cap || self.bytes >= FRAME_BYTES
    }
}

/// The tags' counts as this writer moves them: read once, written with
/// each frame.
struct Counts<'a> {
    store: &'a Store,
    tags: BTreeMap<String, TagCounts>,
}

impl Counts<'_> {
    fn get(&mut self, tag: &str) -> Result<&mut TagCounts> {
        if !self.tags.contains_key(tag) {
            let c = self
                .store
                .get_meta::<TagCounts>(&tag_key(tag))?
                .unwrap_or_else(|| TagCounts {
                    tag: tag.to_string(),
                    ..TagCounts::default()
                });
            self.tags.insert(tag.to_string(), c);
        }
        Ok(self.tags.get_mut(tag).expect("inserted"))
    }
}

/// How often an erase's read of its tag's sessions looks for a stop.
const LOOK_EVERY: usize = 256;

/// `import.episodes`: one batch of a file's lines, as `by` sent them.
pub fn import_batch(
    store: &Store,
    p: &ImportEpisodesParams,
    by: &str,
) -> Result<ImportEpisodesResult> {
    Ok(import_batch_unless(store, p, by, || false)?.result)
}

/// What a batch did, and whether a stop ended it early.
pub struct Batch {
    pub result: ImportEpisodesResult,
    /// The daemon's stop ended the batch between two frames (theseus-autz):
    /// `result` counts what was written, and the lines after it were not
    /// read. The batch sent again finishes it, skipping what was written.
    pub stopped: bool,
}

/// [`import_batch`], ending at the first frame boundary that finds
/// `stopping` (the daemon's stop): every frame written carries the tags'
/// counts it moved, so a stopped batch adds up.
pub fn import_batch_unless(
    store: &Store,
    p: &ImportEpisodesParams,
    by: &str,
    stopping: impl Fn() -> bool,
) -> Result<Batch> {
    import_batch_cut(store, p, by, stopping, FRAME_RECORDS)
}

/// [`import_batch_unless`], its frames cut past `cap` records: a test's
/// three frames' worth is then small enough to run under load.
#[cfg(test)]
pub(super) fn import_batch_in(
    store: &Store,
    p: &ImportEpisodesParams,
    by: &str,
    stopping: impl Fn() -> bool,
    cap: usize,
) -> Result<Batch> {
    import_batch_cut(store, p, by, stopping, cap)
}

fn import_batch_cut(
    store: &Store,
    p: &ImportEpisodesParams,
    by: &str,
    stopping: impl Fn() -> bool,
    cap: usize,
) -> Result<Batch> {
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let t0 = Instant::now();
    let mut out = ImportEpisodesResult::default();
    let mut counts = Counts {
        store,
        tags: BTreeMap::new(),
    };
    // Episodes this batch took, by id, with their hash.
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut frame = Frame::new(cap);
    let mut stopped = false;
    let sent = Sent {
        file: &p.file,
        by,
        numbered: (p.batch, p.batches),
    };
    for l in &p.lines {
        if l.text.trim().is_empty() {
            continue;
        }
        out.read += 1;
        let e = match episode::parse(&l.text) {
            Ok(e) => e,
            Err(b) => {
                out.rejected.push(ImportRejected {
                    line: l.line,
                    episode_id: b.episode_id,
                    why: b.why,
                });
                continue;
            }
        };
        let sid = session_id_of(&e.episode_id);
        let eid = e.episode_id.clone();
        let at = Place {
            file: &p.file,
            line: l.line,
        };
        match take(store, e, &at, (&mut frame, &mut counts, &mut seen))? {
            Took::Skipped => {
                out.skipped += 1;
                continue;
            }
            Took::Refused(why) => {
                out.rejected.push(ImportRejected {
                    line: l.line,
                    episode_id: Some(eid),
                    why,
                });
                continue;
            }
            Took::Wrote { tag, nodes, grew } => {
                out.touched.push(ImportTouched {
                    session_id: sid,
                    tag: tag.clone(),
                });
                let t = frame.imported.entry(tag).or_default();
                t.1 += nodes;
                out.nodes += nodes;
                if grew {
                    t.2 += 1;
                    out.appended += 1;
                } else {
                    t.0 += 1;
                    out.imported += 1;
                }
            }
        }
        if frame.full() {
            write_frame(store, &mut frame, &mut counts, &sent)?;
            out.frames += 1;
            theseus_store::pressure::quiet_blocking_unless(
                theseus_store::pressure::BOUND,
                &stopping,
            );
            // The rest waits for the batch sent again.
            if stopping() {
                stopped = true;
                break;
            }
        }
    }
    if !frame.records.is_empty() {
        write_frame(store, &mut frame, &mut counts, &sent)?;
        out.frames += 1;
    }
    out.ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok(Batch {
        result: out,
        stopped,
    })
}

/// Where an episode's line is: its file and its number in it.
struct Place<'a> {
    file: &'a str,
    line: u64,
}

/// What one episode came to in a batch.
enum Took {
    /// Held already with this hash, or taken earlier in the batch.
    Skipped,
    /// Not written, and why (named in the result).
    Refused(String),
    /// New, or appended to: the tag that holds its session, and its nodes.
    Wrote { tag: String, nodes: u64, grew: bool },
}

/// One episode of a batch: skipped when held with the same hash, refused when
/// erased or when its stored messages are not a prefix of its own (`append`),
/// else its records added to `frame` and its tag's counts moved. `seen` is
/// the batch's episodes by id and hash: their records are not written yet.
fn take(
    store: &Store,
    e: Episode,
    at: &Place<'_>,
    (frame, counts, seen): (&mut Frame, &mut Counts<'_>, &mut BTreeMap<String, String>),
) -> Result<Took> {
    let sid = session_id_of(&e.episode_id);
    let this = e.hash.clone();
    let another = |h: &str, why: &str| {
        Took::Refused(format!(
            "imported before with another hash ({}…, this line's {}…): {why}",
            &h[..12],
            &this[..12]
        ))
    };
    if let Some(h) = seen.get(&e.episode_id) {
        return Ok(match *h == e.hash {
            true => Took::Skipped,
            false => another(h, "not overwritten"),
        });
    }
    let stored = store
        .get_session::<SessionRecord>(&sid)?
        .filter(|r| r.imported.is_some());
    if let Some(i) = stored.as_ref().and_then(|r| r.imported.as_ref()) {
        if let Some(erased) = &i.erased {
            return Ok(Took::Refused(format!(
                "erased by import.erase (by {}): an erased episode is not imported again",
                erased.by
            )));
        }
        if i.hash == e.hash {
            return Ok(Took::Skipped);
        }
    }
    let e = digested(e);
    let Some(mut rec) = stored else {
        seen.insert(e.episode_id.clone(), e.hash.clone());
        let tag = e.import_tag.clone();
        let nodes = records_of(&e, at.file, at.line, frame)?;
        count(counts.get(&tag)?, &e, nodes);
        return Ok(Took::Wrote {
            tag,
            nodes,
            grew: false,
        });
    };
    let held = rec.imported.as_ref().expect("filtered");
    let (tag, was) = (held.tag.clone(), held.hash.clone());
    let plan = match append::plan(store, &rec, &e)? {
        Ok(plan) => plan,
        Err(why) => return Ok(another(&was, &why)),
    };
    seen.insert(e.episode_id.clone(), e.hash.clone());
    let nodes = append::records(&e, &mut rec, &plan, frame)?;
    let c = counts.get(&tag)?;
    c.nodes += nodes;
    c.last_ms = Some(c.last_ms.map_or(e.as_of.end_ms, |f| f.max(e.as_of.end_ms)));
    Ok(Took::Wrote {
        tag,
        nodes,
        grew: true,
    })
}

/// A harness, sync or Vestige episode's message digests, computed over the
/// text as it is stored (the record is raw, so this is the text the CLI read).
/// The episode's `hash`, the idempotency key, stays as the check took it.
fn digested(mut e: Episode) -> Episode {
    if episode::DIGESTED_SOURCES.contains(&e.source.as_str()) {
        for m in &mut e.messages {
            m.sha256 = hex::encode(sha2::Sha256::digest(m.text.as_bytes()));
        }
    }
    e
}

/// An episode's session and its `n` nodes, in its tag's counts.
fn count(c: &mut TagCounts, e: &Episode, n: u64) {
    c.sessions += 1;
    c.nodes += n;
    *c.sources.entry(e.source.clone()).or_default() += 1;
    c.first_ms = Some(
        c.first_ms
            .map_or(e.as_of.start_ms, |f| f.min(e.as_of.start_ms)),
    );
    c.last_ms = Some(c.last_ms.map_or(e.as_of.end_ms, |f| f.max(e.as_of.end_ms)));
}

/// An episode's records, its nodes then its session, added to `frame`:
/// how many nodes.
fn records_of(e: &Episode, file: &str, line: u64, frame: &mut Frame) -> Result<u64> {
    let sid = session_id_of(&e.episode_id);
    let mut nodes = 0;
    let mut ids: BTreeMap<u32, String> = BTreeMap::new();
    for m in &e.messages {
        let id = node_id_of(&sid, m.idx);
        ids.insert(m.idx, id.clone());
        let n = Node::imported(
            id,
            &sid,
            &m.author,
            m.at_ms,
            Body::Imported {
                text: m.text.clone(),
                integrity: m.integrity,
                source: e.source.clone(),
                unit: m.unit.clone(),
                sha256: m.sha256.clone(),
                idx: m.idx,
            },
        );
        frame.push(n.record()?);
        nodes += 1;
    }
    let summary = e.summary.as_ref().filter(|s| !s.text.trim().is_empty());
    if let Some(s) = summary {
        let n = Node::imported(
            summary_id_of(&sid),
            &sid,
            &format!("summary:{}", s.model),
            e.as_of.end_ms,
            Body::ImportedSummary {
                text: s.text.clone(),
                cites: s.cites.iter().filter_map(|c| ids.get(c).cloned()).collect(),
                model: s.model.clone(),
            },
        );
        frame.push(n.record()?);
        nodes += 1;
    }
    let mut r = SessionRecord::with_id(sid, SessionKind::Conversation, Some("imported".into()));
    r.created_at_unix_ms = e.as_of.start_ms;
    r.last_active_ms = e.as_of.end_ms;
    r.title = Some(crate::session::title_from(summary.map_or_else(
        || e.messages.first().map_or("", |m| m.text.as_str()),
        |s| s.text.as_str(),
    )));
    r.imported = Some(Box::new(ImportedFrom {
        tag: e.import_tag.clone(),
        episode_id: e.episode_id.clone(),
        hash: e.hash.clone(),
        source: e.source.clone(),
        agent: e.agent.clone(),
        place: e.place.clone(),
        labels: e.labels.clone(),
        triage: e.triage.clone(),
        as_of: e.as_of,
        messages: e.messages.len() as u32,
        summary: summary.is_some(),
        file: file.to_string(),
        line,
        imported_at_ms: theseus_protocol::now_unix_ms(),
        erased: None,
    }));
    frame.push(
        NewRecord::json(kinds::SESSION, Some(&r.session_id), &r)?.scoped(&tag_scope(&e.import_tag)),
    );
    Ok(nodes)
}

/// Whose batch a frame is part of: its file, who sent it, and the number the
/// client gave it and its run's total (theseus-jn17m).
#[derive(Clone, Copy)]
struct Sent<'a> {
    file: &'a str,
    by: &'a str,
    numbered: (Option<u32>, Option<u32>),
}

/// Write `frame` with the tags' counts it moved and each tag's batch row,
/// and empty it.
fn write_frame(
    store: &Store,
    frame: &mut Frame,
    counts: &mut Counts<'_>,
    sent: &Sent<'_>,
) -> Result<()> {
    let Sent { file, by, numbered } = *sent;
    let now = theseus_protocol::now_unix_ms();
    for (tag, (imported, nodes, appended)) in std::mem::take(&mut frame.imported) {
        let c = counts.get(&tag)?;
        c.updated_ms = now;
        frame.push(NewRecord::json(kinds::META, Some(&tag_key(&tag)), &*c)?);
        // The tag's source: the one most of its sessions came from.
        let source = c
            .sources
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(s, _)| s.clone());
        let row = fact::import::ImportBatch {
            tag: &tag,
            file,
            by,
            imported,
            nodes,
            appended,
            batch: numbered.0,
            batches: numbered.1,
            source: source.as_deref(),
        };
        frame.push(fact::row(&row, None, None)?.scoped(&fact::import::scope(&tag)));
    }
    store.append(&std::mem::take(&mut frame.records))?;
    frame.bytes = 0;
    Ok(())
}

/// What an erase did, and the nodes the index is to forget.
pub struct Erasure {
    pub result: ImportEraseResult,
    pub nodes: Vec<String>,
    /// The daemon's stop ended the erase early (theseus-autz): `result`
    /// counts what was erased, every frame carries the tag's counts and the
    /// last its `import.erased` row for that much, and the erase run again
    /// finishes the tag, skipping what was erased.
    pub stopped: bool,
}

/// An erase's frame before the last carries its progress for the work board
/// (theseus-jn17m): the sessions erased so far, of this run's.
fn erasing(frame: &mut Frame, who: (&str, &str), done: u64, of: u64) -> Result<()> {
    let row = fact::import::ImportErasing {
        tag: who.0,
        by: who.1,
        progress: fact::import::Progress { done, of },
    };
    frame.push(fact::row(&row, None, None)?.scoped(&fact::import::scope(who.0)));
    Ok(())
}

/// The imported sessions of a tag not erased yet: an erase run's total.
fn unerased(sessions: &BTreeMap<String, SessionRecord>) -> u64 {
    sessions
        .values()
        .filter(|r| {
            r.imported
                .as_ref()
                .is_some_and(|i| i.erased.is_none() && is_imported(&r.session_id))
        })
        .count() as u64
}

/// The tag's counts as they stand after an erase's frame, added to it
/// (theseus-mce3): `erased` is the tag's sessions erased before the run and
/// in it, so a reader lags a frame at most, and a kill between two frames
/// leaves the count whole.
fn counted(
    frame: &mut Frame,
    tag: &str,
    counts: &mut TagCounts,
    erased: u64,
    now: u64,
) -> Result<()> {
    counts.erased = erased;
    counts.updated_ms = now;
    frame.push(NewRecord::json(kinds::META, Some(&tag_key(tag)), &*counts)?);
    Ok(())
}

/// `import.erase`: tombstone every session of `tag` and each of its nodes
/// (§5.6's payload erasure with preserved structure: each node is written
/// again under its id, origin and time with an `Erased` body, and the
/// session's record gets its receipt). The WAL's earlier frames still hold
/// the payloads: the store has no rewrite of a frame, and the spec's
/// erasure of a payload in place is not built. Every reader that takes a
/// node's newest record (`get_node`), the index's follower (a tombstone has
/// nothing to index, so it drops the node, at a rebuild too), and recall
/// see the tombstone.
pub fn erase(store: &Store, tag: &str, why: Option<&str>, by: &str) -> Result<Erasure> {
    erase_unless(store, tag, why, by, || false)
}

/// [`erase`], ending at the first frame boundary that finds `stopping`
/// (the daemon's stop), and in the read of the tag's sessions before the
/// first frame, every [`LOOK_EVERY`] records. The tag's counts ride in
/// every frame, its sessions erased counted whole (theseus-mce3), and its
/// row in the last, for what this run erased.
pub fn erase_unless(
    store: &Store,
    tag: &str,
    why: Option<&str>,
    by: &str,
    stopping: impl Fn() -> bool,
) -> Result<Erasure> {
    erase_in(store, tag, why, by, stopping, FRAME_RECORDS)
}

/// [`erase_unless`], its frames cut past `cap` records, as
/// [`import_batch_in`]'s.
pub(super) fn erase_in(
    store: &Store,
    tag: &str,
    why: Option<&str>,
    by: &str,
    stopping: impl Fn() -> bool,
    cap: usize,
) -> Result<Erasure> {
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let t0 = Instant::now();
    let now = theseus_protocol::now_unix_ms();
    // Stopped before its first frame, an erase has written nothing.
    let Some(sessions) = sessions_of_tag(store, tag, &stopping)? else {
        let result = ImportEraseResult {
            tag: tag.to_string(),
            ms: t0.elapsed().as_secs_f64() * 1e3,
            ..ImportEraseResult::default()
        };
        let (nodes, stopped) = (Vec::new(), true);
        return Ok(Erasure {
            result,
            nodes,
            stopped,
        });
    };
    // The tag's sessions erased before this run, a cut run's among them:
    // each frame's count starts from them, never from the stored count,
    // which such a run left short (theseus-mce3).
    let before = sessions
        .values()
        .filter(|r| r.imported.as_ref().is_some_and(|i| i.erased.is_some()))
        .count() as u64;
    let mut counts = store
        .get_meta::<TagCounts>(&tag_key(tag))?
        .unwrap_or_else(|| TagCounts {
            tag: tag.to_string(),
            ..TagCounts::default()
        });
    let receipt = format!("import.erase of {tag}");
    let of_sessions = unerased(&sessions);
    let mut frame = Frame::new(cap);
    let (mut n_sessions, mut n_nodes, mut frames) = (0u64, 0u64, 0u64);
    let mut forget = Vec::new();
    let mut stopped = false;
    for (sid, mut rec) in sessions {
        let Some(imp) = rec.imported.as_mut() else {
            continue;
        };
        if imp.erased.is_some() || !is_imported(&sid) {
            continue;
        }
        for (_, n) in store.session_nodes(&sid)? {
            if matches!(n.body, Body::Erased { .. }) {
                continue;
            }
            frame.push(n.erased(now, &receipt).record()?);
            forget.push(n.id.clone());
            n_nodes += 1;
        }
        // An erased session's pre-images go with it (theseus-23rk).
        if let Err(e) = crate::rewind::forget(store, &sid) {
            tracing::warn!(session = %sid, error = %e, "import.erase: its pre-images were not removed");
        }
        imp.erased = Some(Erased {
            at_ms: now,
            by: by.to_string(),
            why: why.map(str::to_string),
        });
        rec.title = None;
        frame.push(NewRecord::json(kinds::SESSION, Some(&sid), &rec)?.scoped(&tag_scope(tag)));
        n_sessions += 1;
        if frame.full() {
            counted(&mut frame, tag, &mut counts, before + n_sessions, now)?;
            erasing(&mut frame, (tag, by), n_sessions, of_sessions)?;
            store.append(&std::mem::take(&mut frame.records))?;
            frame.bytes = 0;
            frames += 1;
            theseus_store::pressure::quiet_blocking_unless(
                theseus_store::pressure::BOUND,
                &stopping,
            );
            // The rest waits for a rerun: this much is counted, and its row
            // is written below.
            if stopping() {
                stopped = true;
                break;
            }
        }
    }
    counted(&mut frame, tag, &mut counts, before + n_sessions, now)?;
    let row = fact::import::ImportErased {
        tag,
        by,
        why,
        sessions: n_sessions,
        nodes: n_nodes,
    };
    frame.push(fact::row(&row, None, None)?.scoped(&fact::import::scope(tag)));
    store.append(&frame.records)?;
    frames += 1;
    Ok(Erasure {
        result: ImportEraseResult {
            tag: tag.to_string(),
            sessions: n_sessions,
            nodes: n_nodes,
            frames,
            index: String::new(),
            ms: t0.elapsed().as_secs_f64() * 1e3,
            ..ImportEraseResult::default()
        },
        nodes: forget,
        stopped,
    })
}

/// The sessions of `tag`, by id, or `None` once `stopping` turns true: it
/// is looked at every [`LOOK_EVERY`] records.
pub(crate) fn sessions_of_tag(
    store: &Store,
    tag: &str,
    stopping: &impl Fn() -> bool,
) -> Result<Option<BTreeMap<String, SessionRecord>>> {
    let mut sessions = BTreeMap::new();
    for (i, r) in store
        .scope_after(&tag_scope(tag), 0)?
        .into_iter()
        .enumerate()
    {
        if i % LOOK_EVERY == LOOK_EVERY - 1 && stopping() {
            return Ok(None);
        }
        if r.kind != kinds::SESSION {
            continue;
        }
        let rec: SessionRecord = r.decode()?;
        sessions.insert(rec.session_id.clone(), rec);
    }
    Ok(Some(sessions))
}

/// `import.list`: each tag with its counts, from their META records.
pub fn list(store: &Store) -> Result<ImportListResult> {
    let mut tags = Vec::new();
    for r in store
        .inner()
        .latest_with_prefix(kinds::META, &tag_key(""))?
    {
        let c: TagCounts = r.decode()?;
        tags.push(ImportTagInfo {
            tag: c.tag,
            sessions: c.sessions,
            nodes: c.nodes,
            erased: c.erased,
            sources: c.sources,
            first_ms: c.first_ms,
            last_ms: c.last_ms,
            updated_ms: c.updated_ms,
        });
    }
    tags.sort_by(|a, b| a.tag.cmp(&b.tag));
    Ok(ImportListResult { tags })
}
