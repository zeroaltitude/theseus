//! The import's writes (theseus-0lrr.6): a batch of an episode file's
//! lines into imported sessions, a tag's erase, and the tags' counts.
//!
//! - **One frame per batch**, never one per message: a batch's sessions,
//!   their nodes, the tags' counts and the batch's row together, cut into a
//!   second frame only past `FRAME_RECORDS` or `FRAME_BYTES`. Between two
//!   frames the writer waits while the machine is busy
//!   (`theseus_store::pressure`), and it holds no store lock but the
//!   writer's own, a frame at a time, so the daemon answers throughout.
//! - **Idempotent.** An episode is found by its session's key: one imported
//!   with the same hash is skipped, one with another hash is rejected and
//!   named, never overwritten; an erased one is not imported again.
//! - **One import or erase at a time** (`ONE`): a tag's counts are read and
//!   written by one writer.
//!
//! These run on a blocking thread (the RPC's `spawn_blocking`), never on a
//! runtime worker.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::Result;
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEpisodesResult, ImportEraseResult, ImportListResult,
    ImportRejected, ImportTagInfo,
};
use theseus_protocol::SessionKind;
use theseus_store::{kinds, NewRecord, Store as _};

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

/// One import or erase at a time.
static ONE: Mutex<()> = Mutex::new(());

/// A frame being built: its records, their bytes, and the tags' counts it
/// moves.
#[derive(Default)]
struct Frame {
    records: Vec<NewRecord>,
    bytes: usize,
    imported: BTreeMap<String, (u64, u64)>,
}

impl Frame {
    fn push(&mut self, r: NewRecord) {
        self.bytes += r.payload.len();
        self.records.push(r);
    }

    fn full(&self) -> bool {
        self.records.len() >= FRAME_RECORDS || self.bytes >= FRAME_BYTES
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

/// `import.episodes`: one batch of a file's lines, as `by` sent them.
pub fn import_batch(
    store: &Store,
    p: &ImportEpisodesParams,
    by: &str,
) -> Result<ImportEpisodesResult> {
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
    let mut frame = Frame::default();
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
        let reject = |why: String| ImportRejected {
            line: l.line,
            episode_id: Some(e.episode_id.clone()),
            why,
        };
        let sid = session_id_of(&e.episode_id);
        let before = match seen.get(&e.episode_id) {
            Some(h) => Some((h.clone(), None)),
            None => store
                .get_session::<SessionRecord>(&sid)?
                .and_then(|r| r.imported)
                .map(|i| (i.hash.clone(), i.erased.clone())),
        };
        match before {
            Some((h, None)) if h == e.hash => {
                out.skipped += 1;
                continue;
            }
            Some((h, None)) => {
                out.rejected.push(reject(format!(
                    "imported before with another hash ({}…, this line's {}…): not overwritten",
                    &h[..12],
                    &e.hash[..12]
                )));
                continue;
            }
            Some((_, Some(erased))) => {
                out.rejected.push(reject(format!(
                    "erased by import.erase (by {}): an erased episode is not imported again",
                    erased.by
                )));
                continue;
            }
            None => {}
        }
        seen.insert(e.episode_id.clone(), e.hash.clone());
        let n = records_of(&e, &p.file, l.line, &mut frame)?;
        let c = counts.get(&e.import_tag)?;
        c.sessions += 1;
        c.nodes += n;
        *c.sources.entry(e.source.clone()).or_default() += 1;
        c.first_ms = Some(
            c.first_ms
                .map_or(e.as_of.start_ms, |f| f.min(e.as_of.start_ms)),
        );
        c.last_ms = Some(c.last_ms.map_or(e.as_of.end_ms, |f| f.max(e.as_of.end_ms)));
        let t = frame.imported.entry(e.import_tag.clone()).or_default();
        t.0 += 1;
        t.1 += n;
        out.imported += 1;
        out.nodes += n;
        if frame.full() {
            write_frame(store, &mut frame, &mut counts, &p.file, by)?;
            out.frames += 1;
            theseus_store::pressure::quiet_blocking(theseus_store::pressure::BOUND);
        }
    }
    if !frame.records.is_empty() {
        write_frame(store, &mut frame, &mut counts, &p.file, by)?;
        out.frames += 1;
    }
    out.ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok(out)
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

/// Write `frame` with the tags' counts it moved and each tag's batch row,
/// and empty it.
fn write_frame(
    store: &Store,
    frame: &mut Frame,
    counts: &mut Counts<'_>,
    file: &str,
    by: &str,
) -> Result<()> {
    let now = theseus_protocol::now_unix_ms();
    for (tag, (imported, nodes)) in std::mem::take(&mut frame.imported) {
        let c = counts.get(&tag)?;
        c.updated_ms = now;
        frame.push(NewRecord::json(kinds::META, Some(&tag_key(&tag)), &*c)?);
        let row = fact::import::ImportBatch {
            tag: &tag,
            file,
            by,
            imported,
            nodes,
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
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let t0 = Instant::now();
    let now = theseus_protocol::now_unix_ms();
    let mut sessions: BTreeMap<String, SessionRecord> = BTreeMap::new();
    for r in store.scope_after(&tag_scope(tag), 0)? {
        if r.kind != kinds::SESSION {
            continue;
        }
        let rec: SessionRecord = r.decode()?;
        sessions.insert(rec.session_id.clone(), rec);
    }
    let receipt = format!("import.erase of {tag}");
    let mut frame = Frame::default();
    let (mut n_sessions, mut n_nodes, mut frames) = (0u64, 0u64, 0u64);
    let mut forget = Vec::new();
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
        imp.erased = Some(Erased {
            at_ms: now,
            by: by.to_string(),
            why: why.map(str::to_string),
        });
        rec.title = None;
        frame.push(NewRecord::json(kinds::SESSION, Some(&sid), &rec)?.scoped(&tag_scope(tag)));
        n_sessions += 1;
        if frame.full() {
            store.append(&std::mem::take(&mut frame.records))?;
            frame.bytes = 0;
            frames += 1;
            theseus_store::pressure::quiet_blocking(theseus_store::pressure::BOUND);
        }
    }
    let mut c = store
        .get_meta::<TagCounts>(&tag_key(tag))?
        .unwrap_or_else(|| TagCounts {
            tag: tag.to_string(),
            ..TagCounts::default()
        });
    c.erased += n_sessions;
    c.updated_ms = now;
    frame.push(NewRecord::json(kinds::META, Some(&tag_key(tag)), &c)?);
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
        },
        nodes: forget,
    })
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
