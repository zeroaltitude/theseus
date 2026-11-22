//! `node.reach` (theseus-n4m, step 12a; design stage2 §2.11): where a node
//! went, derived when it is asked from records that already say so.
//!
//! - **Direct exposure**, read from the node's own session: each compilation
//!   whose prefix (`includes`) holds it, and each loop after it whose context
//!   held it. A loop's reply names its compilation, which holds the node or
//!   left it in the tail (after `as_of`), as `render_request` builds the
//!   request: an old store's transcript compilations, which admitted only
//!   their own session's nodes.
//! - **A view turn** (issue 44), in ANY session: a view turn may lay another session's exchange with no
//!   copy (the same place's window, another place's exchange, a recalled item), so its consumers are found
//!   store-wide, from the kept view records (`view::record`, the ones `view show` reads) written since the
//!   node, newest first, one walk of the META births ([`MAX_VIEWS`] at most), and from the node's own
//!   turn's views. Each view is asked what it laid: an exchange in the form it was laid in
//!   (`view::text::forms::laid`, the renderers' own selection), a recalled item it showed (a dropped one is
//!   not sent), and its turn's own nodes and the notes between turns (`compiler::view::between`) at each
//!   call. A call is its `action.dispatched` row (`provider.messages`), answered or not: what was written
//!   after it was dispatched was not in it. A turn's `Recall` node is not laid (its sources are, as
//!   recalled items). A fold lays its account, not its sources, so they are not counted. A view record
//!   that cannot be read, a view whose calls cannot be found, or a walk past its cap makes the answer
//!   `partial`: never a silent "nowhere".
//! - **An erased node** is accounted from its original position and kind (ids, positions and counts; never
//!   the erased words): the requests before its erasure still say where it went.
//! - **Descendants**: the edges into the node (the scope `in:<node>`), each a
//!   copy in another session, with its own exposure, generation by
//!   generation.
//!
//! **Why derive, and not store.** §6.1 writes the reverse `includes` edges in
//! the compilation's frame. Every `includes` today is a run of the session's
//! own nodes, and the compilation record already holds it: about 300 reverse
//! edges per recompile would repeat it in every recompile's frame. The first
//! route that admits a node from outside its session is a fork's
//! (theseus-2qzx): its frame writes one reverse entry for the whole run (an
//! `includes` edge from its first compilation into the source's session),
//! and `forks_of` reads them, generation by generation. M6's recall and M7's
//! borrowing bring their own when they admit by reference.
//!
//! **FAST.** Nothing runs until asked, and nothing at startup. One scan per
//! session reached, from its earliest node on, and one scope scan per node
//! for its copies.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::Result;
use serde::Deserialize;
use theseus_protocol::{
    NodeReachResult, ReachCompilation, ReachDescendant, ReachExposure, ReachFork, ReachTotals,
};
use theseus_store::{kinds, Store as _};

use crate::compiler::Compilation;
use crate::graph::{Edge, EdgeKind};
use crate::node::{Body, Node};
use crate::store::Store;
use crate::view::record::{self, ViewRecord, SEP, VIEW_PREFIX, VIEW_TURN_PREFIX};
use crate::view::{Form, Pos, Slot, Space};

/// Generations of copies followed when the request names none.
pub const DEFAULT_GENERATIONS: u32 = 3;
/// The most a request may ask for.
pub const MAX_GENERATIONS: u32 = 16;
/// The most copies one walk follows; past it the answer is partial.
pub const MAX_DESCENDANTS: usize = 256;

/// A node the walk reached: the root, or a copy.
struct Reached {
    node: Node,
    position: u64,
    generation: u32,
    /// The edge that reached it, from the node it copies (`to`).
    by: Option<Edge>,
}

/// Where `node_id` went; None when there is no such node.
pub fn reach(
    store: &Store,
    node_id: &str,
    max_generations: Option<u32>,
) -> Result<Option<NodeReachResult>> {
    let max = max_generations
        .unwrap_or(DEFAULT_GENERATIONS)
        .min(MAX_GENERATIONS);
    let Some((latest, node)) = store.get_node(node_id)? else {
        return Ok(None);
    };
    let position = original_position(store, latest, &node)?;
    let (reached, mut partial) = walk(
        store,
        Reached {
            node,
            position,
            generation: 0,
            by: None,
        },
        max,
    )?;

    // Each session reached, read once.
    let mut by_session: HashMap<&str, Vec<usize>> = HashMap::new();
    for (k, r) in reached.iter().enumerate() {
        by_session
            .entry(r.node.session_id.as_str())
            .or_default()
            .push(k);
    }
    let mut exposure: Vec<ReachExposure> = vec![ReachExposure::default(); reached.len()];
    for (session, ks) in &by_session {
        let nodes: Vec<(u64, &Node)> = ks
            .iter()
            .map(|k| (reached[*k].position, &reached[*k].node))
            .collect();
        let (got, unread) = exposures(store, session, &nodes)?;
        partial |= unread;
        for (k, e) in ks.iter().zip(got) {
            exposure[*k] = e;
        }
    }

    // The forks that took it by reference (theseus-2qzx).
    let (mut forks, forks_partial) = forks_of(store, &reached[0], max)?;
    partial |= forks_partial;

    let (sessions, unknown) = by_views(
        store,
        &reached,
        &by_session,
        (exposure.as_mut_slice(), forks.as_mut_slice()),
    )?;
    partial |= unknown;
    let totals = totals_of(&exposure, &forks, sessions);
    let mut exposure = exposure.into_iter();
    let direct = exposure.next().unwrap_or_default();
    let mut walked = reached.into_iter();
    let root = walked.next().expect("the root");
    let descendants = walked
        .zip(exposure)
        .filter_map(|(r, exposure)| {
            let edge = r.by?;
            Some(ReachDescendant {
                node_id: r.node.id,
                session_id: r.node.session_id,
                position: r.position,
                generation: r.generation,
                via: edge.kind,
                route: edge.via,
                from: edge.to,
                exposure,
            })
        })
        .collect();
    Ok(Some(NodeReachResult {
        node_id: root.node.id,
        session_id: root.node.session_id,
        position: root.position,
        direct,
        descendants,
        forks,
        totals,
        partial,
        labels: None,
    }))
}

/// The node and its copies, generation by generation, to `max` generations and [`MAX_DESCENDANTS`]
/// copies; true when the walk stopped short.
fn walk(store: &Store, root: Reached, max: u32) -> Result<(Vec<Reached>, bool)> {
    let mut seen: BTreeSet<String> = BTreeSet::from([root.node.id.clone()]);
    let mut reached = vec![root];
    let mut partial = false;
    let mut i = 0;
    'walk: while i < reached.len() {
        let (id, generation) = (reached[i].node.id.clone(), reached[i].generation);
        i += 1;
        let copies = copies_of(store, &id)?;
        if copies.is_empty() {
            continue;
        }
        if generation >= max {
            partial = true;
            continue;
        }
        for e in copies {
            if !seen.insert(e.from.clone()) {
                continue;
            }
            if reached.len() > MAX_DESCENDANTS {
                partial = true;
                break 'walk;
            }
            let Some((latest, node)) = store.get_node(&e.from)? else {
                continue;
            };
            let position = original_position(store, latest, &node)?;
            reached.push(Reached {
                node,
                position,
                generation: generation + 1,
                by: Some(e),
            });
        }
    }
    Ok((reached, partial))
}

/// Every view turn that laid a reached node, in whatever session it ran (issue 44): the kept view records
/// written since the earliest of them, store-wide, and each one's own turns' views, merged into each
/// node's exposure (a fork's session's into its fork's). The sessions reached in all, and whether some of
/// it is unknown.
fn by_views(
    store: &Store,
    reached: &[Reached],
    by_session: &HashMap<&str, Vec<usize>>,
    (exposure, forks): (&mut [ReachExposure], &mut [ReachFork]),
) -> Result<(usize, bool)> {
    let from = reached.iter().map(|r| r.position).min().unwrap_or(0);
    let turns: Vec<&str> = (reached.iter())
        .filter_map(|r| r.node.turn_id.as_deref())
        .collect();
    let views = Views::read(store, from, &turns)?;
    let mut partial = views.partial;
    let mut sessions: BTreeSet<String> = by_session.keys().map(|s| (*s).to_string()).collect();
    let mut acc = Accounting::new(store, &views);
    for (k, r) in reached.iter().enumerate() {
        let (by, unknown) = acc.exposure_of(r.position, &r.node)?;
        partial |= unknown;
        for (session, e) in by {
            match forks.iter_mut().find(|f| k == 0 && f.session_id == session) {
                Some(f) => merge(&mut f.exposure, e),
                None => {
                    sessions.insert(session);
                    merge(&mut exposure[k], e);
                }
            }
        }
    }
    Ok((sessions.len(), partial))
}

/// What a walk reached in all: every context of the node, its copies and its
/// forks, and the sessions they are in.
fn totals_of(exposure: &[ReachExposure], forks: &[ReachFork], sessions: usize) -> ReachTotals {
    let contexts = |e: &ReachExposure| e.compilations.len() as u64 + e.loops;
    ReachTotals {
        contexts: exposure.iter().map(contexts).sum::<u64>()
            + forks.iter().map(|f| contexts(&f.exposure)).sum::<u64>(),
        sessions: (sessions + forks.len()) as u32,
    }
}

/// The forks that took the node by reference, generation by generation: the
/// `includes` edges into its session (one per fork, for the run), each kept
/// when its run reaches the node, and then the forks of each fork. The walk
/// is `max` generations deep; true when a fork beyond it was left.
fn forks_of(store: &Store, root: &Reached, max: u32) -> Result<(Vec<ReachFork>, bool)> {
    let mut out: Vec<ReachFork> = Vec::new();
    let mut partial = false;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut level = vec![root.node.session_id.clone()];
    let mut generation = 0;
    while !level.is_empty() {
        let mut next = Vec::new();
        for session in &level {
            for r in store.scope_after(&Edge::scope_into(session), 0)? {
                if r.kind != kinds::EDGE {
                    continue;
                }
                let e: Edge = r.decode()?;
                if EdgeKind::named(&e.kind) != Some(EdgeKind::Includes) {
                    continue;
                }
                let Some(c) = store.get_compilation(&e.from)? else {
                    continue;
                };
                let reaches = c
                    .inherits
                    .as_ref()
                    .is_some_and(|i| i.through_position >= root.position);
                if !reaches || !seen.insert(c.session_id.clone()) {
                    continue;
                }
                if generation >= max {
                    partial = true;
                    continue;
                }
                let at = c.inherits.as_ref().map_or(0, |i| i.through_position);
                let nodes = [(root.position, &root.node)];
                let (got, unread) = exposures(store, &c.session_id, &nodes)?;
                partial |= unread;
                let exposure = got.into_iter().next().unwrap_or_default();
                next.push(c.session_id.clone());
                out.push(ReachFork {
                    session_id: c.session_id,
                    compilation_id: c.id,
                    generation: generation + 1,
                    from: session.clone(),
                    at_position: at,
                    exposure,
                });
            }
        }
        level = next;
        generation += 1;
    }
    Ok((out, partial))
}

/// The copies of `id`: the edges into it (the scope `in:<id>`) of a kind
/// this build follows, in WAL order.
fn copies_of(store: &Store, id: &str) -> Result<Vec<Edge>> {
    let mut out = Vec::new();
    for r in store.scope_after(&Edge::scope_into(id), 0)? {
        if r.kind != kinds::EDGE {
            continue;
        }
        let e: Edge = r.decode()?;
        match EdgeKind::named(&e.kind) {
            Some(EdgeKind::DerivedFrom) => out.push(e),
            // The memory pass's (M6 31a): a likeness, not a copy.
            Some(EdgeKind::SameEntity | EdgeKind::Supersedes) => {}
            // A fork's run into a session, which no node id names.
            Some(EdgeKind::Includes) => {}
            None => {
                // A kind a newer build wrote: no copy this build follows.
            }
        }
    }
    Ok(out)
}

/// What a reply says of the loop that wrote it, read without its blocks.
#[derive(Deserialize)]
struct Reply {
    created_at_ms: u64,
    /// The turn whose call this reply answers: a view turn's calls lay the turn's own nodes before them.
    #[serde(default)]
    turn_id: Option<String>,
    body: ReplyBody,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReplyBody {
    AssistantMessage {
        #[serde(default)]
        compilation_id: Option<String>,
    },
    #[serde(other)]
    Other,
}

/// Each node's exposure in `session` through an old store's transcript compilations, from one scan of it
/// after the earliest of `nodes`. A loop's compilation made before that is read by its id. A view
/// compilation is left to [`Accounting`]. False: nothing here is unknown (the signature is the caller's).
fn exposures(
    store: &Store,
    session: &str,
    nodes: &[(u64, &Node)],
) -> Result<(Vec<ReachExposure>, bool)> {
    let from = nodes.iter().map(|(p, _)| *p).min().unwrap_or(0);
    let Scanned {
        compilations,
        made,
        loops,
    } = scan(store, session, from)?;
    let view = |c: &Compilation| c.strategy == crate::compiler::view::STRATEGY;
    let mut out = Vec::with_capacity(nodes.len());
    for (p, n) in nodes {
        let holds = |c: &Compilation| c.includes.contains(&n.id) || crate::fork::inherited(c, *p);
        let mut e = ReachExposure::default();
        let in_tail = crate::compiler::renderable(n);
        let mut by_loop: BTreeSet<&str> = BTreeSet::new();
        for (at, ms, c, _) in &loops {
            if at <= p {
                continue;
            }
            let Some((_, c)) = compilations.get(c) else {
                continue;
            };
            if !view(c) && (holds(c) || (in_tail && *p > c.as_of)) {
                e.loops += 1;
                by_loop.insert(c.id.as_str());
                seen(&mut e, *ms);
            }
        }
        // Every compilation a loop names, made in the scan or read by its id.
        let mut named: Vec<String> = made.clone();
        for id in &by_loop {
            if !named.iter().any(|m| m == id) {
                named.push((*id).to_string());
            }
        }
        for id in &named {
            let Some((at, c)) = compilations.get(id) else {
                continue;
            };
            if view(c) {
                continue;
            }
            if (at > p && holds(c)) || by_loop.contains(id.as_str()) {
                e.compilations.push(listed_compilation(c));
                seen(&mut e, c.created_at_ms);
            }
        }
        out.push(e);
    }
    Ok((out, false))
}

/// A compilation as an answer lists it.
fn listed_compilation(c: &Compilation) -> ReachCompilation {
    ReachCompilation {
        compilation_id: c.id.clone(),
        strategy: c.strategy.clone(),
        created_at_ms: c.created_at_ms,
        session_id: c.session_id.clone(),
    }
}

/// `at` into an exposure's first and last.
fn seen(e: &mut ReachExposure, at: u64) {
    e.first_ms = Some(e.first_ms.map_or(at, |f| f.min(at)));
    e.last_ms = Some(e.last_ms.map_or(at, |l| l.max(at)));
}

/// `b` into `a`: a compilation once, the calls added, the first and last widened.
fn merge(a: &mut ReachExposure, b: ReachExposure) {
    for c in b.compilations {
        if !a
            .compilations
            .iter()
            .any(|x| x.compilation_id == c.compilation_id)
        {
            a.compilations.push(c);
        }
    }
    a.loops += b.loops;
    for at in [b.first_ms, b.last_ms].into_iter().flatten() {
        seen(a, at);
    }
}

/// The position a node was first written at: its own for a live node; an erased node's tombstone is a
/// later record of the same id, so its first record's, read by key from its session (never its body).
fn original_position(store: &Store, latest: u64, n: &Node) -> Result<u64> {
    if !matches!(n.body, Body::Erased { .. }) {
        return Ok(latest);
    }
    Ok((store.scope_after(&n.session_id, 0)?.into_iter())
        .find(|r| r.kind == kinds::NODE && r.key.as_deref() == Some(n.id.as_str()))
        .map_or(latest, |r| r.position))
}

/// A node's kind as it was written: an erased node's is the one its tombstone keeps.
fn kind_of(n: &Node) -> &str {
    match &n.body {
        Body::Erased { was, .. } => was.as_str(),
        _ => n.kind_str(),
    }
}

/// A loop: its reply's position, its time, its compilation and its turn.
type Loop = (u64, u64, String, Option<String>);

/// One scan of a session from `from`: its compilations (each with its position; a loop's made before `from`
/// read by its id, at 0), the ids of those made in the scan, in order, and its loops.
struct Scanned {
    compilations: HashMap<String, (u64, Compilation)>,
    made: Vec<String>,
    loops: Vec<Loop>,
}

fn scan(store: &Store, session: &str, from: u64) -> Result<Scanned> {
    let mut compilations: HashMap<String, (u64, Compilation)> = HashMap::new();
    let mut made: Vec<String> = Vec::new();
    let mut loops: Vec<Loop> = Vec::new();
    for r in store.scope_after(session, from)? {
        match r.kind {
            kinds::COMPILATION => {
                let c: Compilation = r.decode()?;
                let id = c.id.clone();
                // A compilation written twice counts once, as its latest
                // record.
                if compilations.insert(id.clone(), (r.position, c)).is_none() {
                    made.push(id);
                }
            }
            kinds::NODE => {
                let reply: Reply = r.decode()?;
                if let ReplyBody::AssistantMessage {
                    compilation_id: Some(c),
                } = reply.body
                {
                    loops.push((r.position, reply.created_at_ms, c, reply.turn_id));
                }
            }
            _ => {}
        }
    }
    for (_, _, c, _) in &loops {
        if !compilations.contains_key(c) {
            if let Some(x) = store.get_compilation(c)? {
                compilations.insert(c.clone(), (0, x));
            }
        }
    }
    Ok(Scanned {
        compilations,
        made,
        loops,
    })
}

/// Where a node's exchange sits in its place's timeline: the place and the position a view piece names it
/// by. None for a node of no turn, or of a turn with no exchange.
fn exchange_at(store: &Store, n: &Node) -> Result<Option<(String, Pos)>> {
    let Some(turn) = n.turn_id.as_deref() else {
        return Ok(None);
    };
    Ok(crate::exchange::exchange(store, turn)?.map(|x| {
        let at = Pos {
            at_ms: x.start_ms,
            id: x.turn_id,
        };
        (x.place, at)
    }))
}

/// The most kept view records one answer reads, store-wide (a guideline, generous: a `node.reach` or a
/// retraction is the owner's own, and rare). Past it the answer is `partial`.
pub const MAX_VIEWS: usize = 50_000;

/// Kept view keys read in one page of the births' walk.
const VIEW_PAGE: usize = 512;

/// The model call's tool: a call of a view turn is its `action.dispatched` row with it.
const MODEL_CALL: &str = "provider.messages";

/// The kept views an answer asks: each with the position it was kept at, and each view's turn.
struct Views {
    kept: Vec<(u64, ViewRecord)>,
    turns: HashMap<String, String>,
    partial: bool,
}

impl Views {
    /// The views kept after `from`, store-wide, newest first, and every view of `turns` (a turn's own
    /// node written after its view was kept is laid by that view's later calls).
    fn read(store: &Store, from: u64, turns: &[&str]) -> Result<Self> {
        let mut out = Self {
            kept: Vec::new(),
            turns: HashMap::new(),
            partial: false,
        };
        let keep = |k: &str| k.starts_with(VIEW_PREFIX) || k.starts_with(VIEW_TURN_PREFIX);
        let (mut before, mut read) = (None, 0usize);
        'walk: loop {
            let Some((page, more)) =
                store
                    .inner()
                    .newest_keys_where(kinds::META, before, VIEW_PAGE, &keep)?
            else {
                // No index of births yet: what views laid it cannot be listed.
                tracing::warn!("the store keeps no births yet: reach says partial");
                out.partial = true;
                break;
            };
            for (born, r) in &page {
                if *born <= from {
                    break 'walk;
                }
                read += 1;
                if read > MAX_VIEWS {
                    tracing::warn!(read, "reach read its cap of view records: partial");
                    out.partial = true;
                    break 'walk;
                }
                out.take(*born, r);
            }
            match (more, page.last()) {
                (true, Some((born, _))) => before = Some(*born),
                _ => break,
            }
        }
        for turn in turns {
            for v in record::of_turn(store, turn)? {
                out.turns.insert(v.compilation.clone(), (*turn).to_string());
                if out.kept.iter().any(|(_, k)| k.compilation == v.compilation) {
                    continue;
                }
                let key = format!("{VIEW_PREFIX}{}", v.compilation);
                let at =
                    (store.inner().latest_by_key(kinds::META, &key)?).map_or(0, |r| r.position);
                out.kept.push((at, v));
            }
        }
        Ok(out)
    }

    /// One kept record of the walk, born at `born`: a view (dated by its birth: a view kept again after
    /// its first calls keeps its key), or a turn's index entry naming its compilation.
    fn take(&mut self, born: u64, r: &theseus_store::Record) {
        let Some(key) = r.key.as_deref() else {
            return;
        };
        if let Some(rest) = key.strip_prefix(VIEW_TURN_PREFIX) {
            let mut parts = rest.split(SEP);
            if let (Some(turn), Some(_), Some(c)) = (parts.next(), parts.next(), parts.next()) {
                self.turns.insert(c.to_string(), turn.to_string());
            }
        } else if key.starts_with(VIEW_PREFIX) {
            match r.decode::<ViewRecord>() {
                Ok(v) => self.kept.push((born, v)),
                Err(e) => {
                    tracing::warn!(key, error = %format!("{e:#}"),
                        "a view record cannot be read: reach says partial");
                    self.partial = true;
                }
            }
        }
    }
}

/// Which of the three forms lay a node's own text: Line, Spoken, Whole.
type Forms = [bool; 3];

fn form_index(f: Form) -> usize {
    match f {
        Form::Line => 0,
        Form::Spoken => 1,
        Form::Whole => 2,
    }
}

/// The per-call accounting of the views, with what it read once: a session's nodes and its records.
struct Accounting<'a> {
    store: &'a Store,
    views: &'a Views,
    transcripts: HashMap<String, crate::store::Transcript>,
    records: HashMap<String, (u64, Vec<theseus_store::Record>)>,
}

impl<'a> Accounting<'a> {
    fn new(store: &'a Store, views: &'a Views) -> Self {
        Self {
            store,
            views,
            transcripts: HashMap::new(),
            records: HashMap::new(),
        }
    }

    fn transcript(&mut self, session: &str) -> Result<&crate::store::Transcript> {
        if !self.transcripts.contains_key(session) {
            let t = self.store.transcript(session)?;
            self.transcripts.insert(session.to_string(), t);
        }
        Ok(&self.transcripts[session])
    }

    /// `session`'s records after `from`, read once from the earliest asked.
    fn records(&mut self, session: &str, from: u64) -> Result<&[theseus_store::Record]> {
        let stale = self
            .records
            .get(session)
            .is_none_or(|(start, _)| *start > from);
        if stale {
            let all = self.store.scope_after(session, from)?;
            self.records.insert(session.to_string(), (from, all));
        }
        Ok(&self.records[session].1)
    }

    /// The calls of `compilation`'s chain, kept at `at` in `session`: each `action.dispatched` model call
    /// after it, to the next compilation of another id. Its position and time.
    fn calls(&mut self, session: &str, at: u64, compilation: &str) -> Result<Vec<(u64, u64)>> {
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        let mut out = Vec::new();
        for r in self
            .records(session, at)?
            .iter()
            .filter(|r| r.position > at)
        {
            if r.kind == kinds::COMPILATION {
                if r.decode::<Id>().is_ok_and(|c| c.id != compilation) {
                    break;
                }
            } else if r.kind == kinds::LEDGER {
                let Ok(row) = r.decode::<crate::ledger::LedgerRow>() else {
                    continue;
                };
                if row.kind == "action.dispatched" && row.data["tool"] == MODEL_CALL {
                    out.push((r.position, row.at_unix_ms));
                }
            }
        }
        Ok(out)
    }

    /// What each form lays of `n`'s own text, from its exchange's nodes (`forms::laid`). An erased node's
    /// is read from its kind; an erased reply's text is unknown (true as the second).
    fn forms_of(&mut self, n: &Node) -> Result<(Forms, bool)> {
        let Some(turn) = n.turn_id.clone() else {
            return Ok(([false; 3], false));
        };
        let nodes: Vec<Node> = (self.transcript(&n.session_id)?.iter())
            .filter(|(_, x)| x.turn_id.as_deref() == Some(turn.as_str()))
            .map(|(_, x)| (**x).clone())
            .collect();
        if !matches!(n.body, Body::Erased { .. }) {
            let has = |f: Form| crate::view::text::forms::laid(&nodes, f).contains(&n.id);
            return Ok((
                [has(Form::Line), has(Form::Spoken), has(Form::Whole)],
                false,
            ));
        }
        let first_ask = (nodes.iter())
            .find(|x| kind_of(x) == "user_message")
            .is_some_and(|x| x.id == n.id);
        Ok(match kind_of(n) {
            "user_message" => ([first_ask, true, true], false),
            "tool_call" | "tool_result" => ([false, false, true], false),
            "assistant_message" => ([false; 3], true),
            _ => ([false; 3], false),
        })
    }

    /// Where `n` (first written at `p`) went through view turns: by the session each ran in, each
    /// compilation that laid it and the calls that did; and true when some of it is unknown.
    fn exposure_of(&mut self, p: u64, n: &Node) -> Result<(BTreeMap<String, ReachExposure>, bool)> {
        let mut by: BTreeMap<String, ReachExposure> = BTreeMap::new();
        let (forms, mut unknown) = self.forms_of(n)?;
        let at = match exchange_at(self.store, n) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(node = %n.id, error = %format!("{e:#}"),
                    "an exchange cannot be read: reach says partial");
                unknown = true;
                None
            }
        };
        let kind = kind_of(n).to_string();
        let views = self.views;
        for (kept, v) in &views.kept {
            let turn = views.turns.get(&v.compilation).map(String::as_str);
            let own = turn.is_some() && n.turn_id.as_deref() == turn;
            // A call node renders into no request where real turns are laid (its reply carries the call,
            // `compiler::renderable`): not in the tail, nor in a window's exchange.
            let call = kind == "tool_call";
            let frozen = !own && lays_frozen(v, &n.id, at.as_ref(), (forms, call));
            let tail = own && kind != "recall" && !call;
            let between = turn.is_some() && n.turn_id.is_none() && kind == "user_message";
            // A node of no exchange (a task's opening, laid as the window's head) is in no piece: its
            // compilation's frozen prefix (`includes`) says whether the view laid it.
            let head = !own && at.is_none() && kind != "recall";
            if !frozen && !tail && !between && !head {
                continue;
            }
            let Some(c) = self.store.get_compilation(&v.compilation)? else {
                unknown |= frozen;
                continue;
            };
            let frozen = frozen || (head && c.includes.contains(&n.id));
            let between = between && c.session_id == n.session_id && {
                let here: HashSet<&str> = (v.shown.iter())
                    .filter(|s| s.piece.slot == Slot::Here)
                    .filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
                    .map(|s| s.piece.range.first.id.as_str())
                    .collect();
                let t = self.transcript(&c.session_id)?;
                crate::compiler::view::between::notes(t, turn.unwrap_or_default(), &here)
                    .contains(&n.id)
            };
            if !frozen && !tail && !between {
                continue;
            }
            let calls = self.calls(&c.session_id, *kept, &c.id)?;
            // A frozen piece is in every call of the chain; the tail and the notes in those sent after.
            let laid: Vec<&(u64, u64)> = calls.iter().filter(|(d, _)| frozen || *d > p).collect();
            if laid.is_empty() {
                unknown |= frozen && calls.is_empty();
                continue;
            }
            let e = by.entry(c.session_id.clone()).or_default();
            e.compilations.push(listed_compilation(&c));
            e.loops += laid.len() as u64;
            seen(e, c.created_at_ms);
            for (_, ms) in laid {
                seen(e, *ms);
            }
        }
        Ok((by, unknown))
    }
}

/// Whether a view's frozen pieces laid node `id`: a recalled item it showed, or an exchange laid in a form
/// that carries the node's text (the window, slot Here, lays real turns, where a call node is not
/// rendered). A dropped piece is not in `shown`: it was not sent.
fn lays_frozen(
    v: &ViewRecord,
    id: &str,
    at: Option<&(String, Pos)>,
    (forms, call): (Forms, bool),
) -> bool {
    v.shown.iter().any(|s| match &s.piece.range.space {
        Space::Recalled { node, .. } => node == id,
        Space::Exchanges { place } => {
            let r = &s.piece.range;
            at.is_some_and(|(p, at)| {
                p == place
                    && (r.first.id == at.id
                        || r.last.id == at.id
                        || (&r.first..=&r.last).contains(&at))
            }) && forms[form_index(s.form)]
                && !(call && s.piece.slot == Slot::Here)
        }
        _ => false,
    })
}

/// The fewest characters of an id's end that name a node (theseus-glyw): an
/// id's last six are a UUIDv7's random bits.
pub const MIN_ENDING: usize = 6;

/// How many nodes a refusal of an ending that names several lists.
const NAMED: usize = 8;

/// What a name that is no node's whole id names.
#[derive(Debug, PartialEq, Eq)]
pub enum Named {
    /// The one node whose id ends so: its whole id.
    One(String),
    /// No node's id ends so, or the name is too short to be an id's end:
    /// why, in words.
    Unknown(String),
    /// The end of more than one node's id: each, in words.
    Refused(String),
}

/// The node `name` means when it is no node's whole id (theseus-glyw): its
/// id's last 6 or more characters (`a1b2c3`, `…a1b2c3`), or the cockpit's
/// `msg·a1b2c3`, its prefix too, when exactly one node ends so. The store
/// walks its index's keys for the ending and reads no node but those that
/// match, to name each one's session when there are several.
pub fn resolve(store: &Store, name: &str) -> Result<Named> {
    let n = name.trim().trim_start_matches('…');
    let (prefix, ending) = match n.split_once('·') {
        Some((p, e)) => (Some(format!("{p}_")), e),
        None => (None, n),
    };
    if ending.chars().count() < MIN_ENDING {
        return Ok(Named::Unknown(format!(
            "no node is named `{name}`: give a node's whole id, or at least its id's last \
             {MIN_ENDING} characters"
        )));
    }
    let mut ids = store.inner().keys_ending(kinds::NODE, ending, 64)?;
    if let Some(p) = &prefix {
        ids.retain(|id| id.starts_with(p.as_str()));
    }
    match ids.as_slice() {
        [] => Ok(Named::Unknown(match &prefix {
            Some(p) => format!("no node's id starts `{p}` and ends with `{ending}`"),
            None => format!("no node's id ends with `{ending}`"),
        })),
        [one] => Ok(Named::One(one.clone())),
        many => {
            let mut named = Vec::new();
            for id in many.iter().take(NAMED) {
                let session = store.get_node(id)?.map(|(_, n)| n.session_id);
                named.push(match session {
                    Some(s) => format!("{id} in {}", theseus_protocol::short_id(&s)),
                    None => id.clone(),
                });
            }
            let more = many.len().saturating_sub(NAMED);
            let more = if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            };
            let count = match many.len() {
                64 => "at least 64".to_string(),
                n => n.to_string(),
            };
            Ok(Named::Refused(format!(
                "`{name}` names {count} nodes ({}{more}): give more of its id",
                named.join(", ")
            )))
        }
    }
}
