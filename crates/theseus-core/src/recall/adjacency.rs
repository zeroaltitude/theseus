//! The adjacency projection (M6 step 32b; design §2.7): the graph
//! spreading activation walks, folded from the record, never stored node by
//! node.
//!
//! - **What it holds.** Each node's neighbours in its session by position
//!   (0.3), a tool call and its result by `tool_use_id` (0.8), the EDGE
//!   records by kind and route ([`mapped`]), and each node's entities (the
//!   memory pass's `memory.labeled` rows, their `about`), with each entity's
//!   nodes, so its `df` is their count. A shared entity is expanded at a
//!   spread, from the entity's list (`1 / ln(1 + df)` an edge), never stored
//!   as an edge per pair.
//! - **A common entity's bound.** An entity in more than [`View::cap`] nodes
//!   is not expanded: at the spread's numbers, one such edge cannot carry a
//!   seed of 1.0 over the threshold alone (1,095 at the defaults). What that
//!   changes: a node that two or more such entities, or such an entity and
//!   another path, would have carried over the threshold together is not
//!   reached through them.
//! - **What it leaves out.** A `Recall` node is no one's neighbour (exposure,
//!   not content): its `derived_from` edges are kept as `Recall`, which
//!   weighs zero by construction. A node the memory pass never labeled (a
//!   store from before 31a, the exam's written past) has no entities here:
//!   the tender is not asked for them.
//! - **Built after serving** on the blocking pool, and **kept current** by
//!   the same fold over what was written since ([`Projection::refresh`]):
//!   the result does not depend on where a fold stopped, so a projection
//!   kept current equals one built whole (`tests_activation`).

use std::collections::HashMap;

use theseus_memory::{Adjacency, EdgeKind, SpreadParams};
use theseus_protocol::LedgerKind;
use theseus_store::Store as _;
use theseus_store::{kinds, Page, Record, RecordKind};

use crate::graph::{self, Edge};
use crate::node::{Body, Node};
use crate::store::Store;

/// What an EDGE record is to the spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapped {
    /// One kind, both ways.
    Both(EdgeKind),
    /// `from` supersedes `to`: toward the newer node, and back.
    Supersedes,
    /// A kind or route the projection does not know: it spreads nothing.
    Unmapped,
}

/// An EDGE's kind and route, as the spread weighs it (§2.7's table).
/// `derived_from`'s copying routes weigh as a task's brief or report; a
/// recall's weighs zero; a route this build does not know (31b's
/// `synthesis`, a later one) spreads nothing.
pub fn mapped(e: &Edge) -> Mapped {
    match graph::EdgeKind::named(&e.kind) {
        Some(graph::EdgeKind::DerivedFrom) => match e.via.as_str() {
            graph::VIA_RECALL => Mapped::Both(EdgeKind::Recall),
            graph::VIA_REPORT
            | graph::VIA_BRIEF
            | graph::VIA_PUBLISH
            | graph::VIA_ARRANGEMENT
            | graph::VIA_CLAIM
            | graph::VIA_GLIDE
            | "graduate" => Mapped::Both(EdgeKind::DerivedFrom),
            _ => Mapped::Unmapped,
        },
        Some(graph::EdgeKind::SameEntity) => Mapped::Both(EdgeKind::SameEntity),
        Some(graph::EdgeKind::Supersedes) => Mapped::Supersedes,
        None => Mapped::Unmapped,
    }
}

/// One node of the projection.
#[derive(Debug, Default)]
struct Entry {
    id: Box<str>,
    /// Its WAL position, once its record is folded (an edge or a label may
    /// name it first).
    position: Option<u64>,
    edges: Vec<(u32, EdgeKind)>,
    entities: Vec<u32>,
}

/// One entity, and the nodes that mention it.
#[derive(Debug, Default)]
struct Entity {
    name: Box<str>,
    nodes: Vec<u32>,
}

/// The projection's size, for health and the manifest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub nodes: u64,
    /// Stored edges, each way counted.
    pub edges: u64,
    pub entities: u64,
    /// (node, entity) pairs.
    pub memberships: u64,
    /// EDGE records whose kind or route spreads nothing.
    pub unmapped: u64,
    /// An estimate of what it holds in memory.
    pub bytes: u64,
    /// The WAL position it holds everything through.
    pub through: u64,
}

/// The adjacency projection: see the module's docs.
#[derive(Debug, Default)]
pub struct Projection {
    nodes: Vec<Entry>,
    ids: HashMap<Box<str>, u32>,
    entities: Vec<Entity>,
    entity_ids: HashMap<Box<str>, u32>,
    /// Each session's last node in the position chain.
    last: HashMap<Box<str>, u32>,
    /// Tool calls and results waiting for their other half, by
    /// `<session>\u{1}<tool_use_id>`.
    calls: HashMap<Box<str>, u32>,
    results: HashMap<Box<str>, u32>,
    edges: u64,
    unmapped: u64,
    through: u64,
}

/// A page of a kind's records, folded at a time.
pub(crate) const PAGE: usize = 4096;

#[cfg(test)]
thread_local! {
    /// How many times this thread paced a walk (the tests' count).
    pub(crate) static PACES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// How long this thread's paces waited in all (the tests' sum).
    pub(crate) static WAITED: std::cell::Cell<std::time::Duration> =
        const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

/// One pace of the warm build's walk: wait while the machine is busy, never
/// past the start of a clean stop (the runtime's end waits for the blocking
/// pool, so a build still waiting would hold the stop up to `BOUND` a page),
/// and how long it waited. Only a build calls it; a refresh never does.
pub(crate) fn pace() -> std::time::Duration {
    #[cfg(test)]
    PACES.with(|c| c.set(c.get() + 1));
    let waited = theseus_store::pressure::quiet_blocking_unless(
        theseus_store::pressure::BOUND,
        crate::startup::stop_has_begun,
    );
    #[cfg(test)]
    WAITED.with(|c| c.set(c.get() + waited));
    waited
}

impl Projection {
    /// Built whole from `store`.
    pub fn build(store: &Store) -> anyhow::Result<Self> {
        Self::build_paced(store, &mut || {})
    }

    /// Built whole from `store`, calling `pace` before each page of the walk
    /// after the first (the warm build waits there while the machine is busy).
    pub fn build_paced(store: &Store, pace: &mut dyn FnMut()) -> anyhow::Result<Self> {
        let mut p = Self::default();
        p.refresh_paced(store, pace)?;
        Ok(p)
    }

    /// Fold what `store` holds past [`Stats::through`], up to its last
    /// position as this call begins. It never waits: a turn calls it inside
    /// recall's deadline.
    pub fn refresh(&mut self, store: &Store) -> anyhow::Result<()> {
        self.refresh_paced(store, &mut || {})
    }

    /// [`Projection::refresh`], calling `pace` before each page of its walk
    /// after the first.
    pub fn refresh_paced(&mut self, store: &Store, pace: &mut dyn FnMut()) -> anyhow::Result<()> {
        let upto = store.last_position();
        if upto <= self.through {
            return Ok(());
        }
        let after = self.through;
        let inner = store.inner();
        for_each(inner.as_ref(), kinds::NODE, after, upto, pace, |r| {
            if let Ok(n) = r.decode::<Node>() {
                self.node(r.position, &n);
            }
        })?;
        for_each(inner.as_ref(), kinds::EDGE, after, upto, pace, |r| {
            if let Ok(e) = r.decode::<Edge>() {
                self.edge(&e);
            }
        })?;
        let labeled = LedgerKind::MemoryLabeled.as_str();
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![theseus_store::pages::ledger_kind(labeled)],
            after: Some(after),
            limit: 1,
            ..Page::default()
        };
        let mut row = |r: &Record| {
            if let Ok(row) = r.decode::<crate::ledger::LedgerRow>() {
                if row.kind == labeled {
                    self.labeled(&row.data);
                }
            }
        };
        if inner.page(&page)?.is_some() {
            // The ledger's tags: this kind's rows alone.
            let mut at = after;
            loop {
                if at != after {
                    pace();
                }
                let q = Page {
                    after: Some(at),
                    limit: PAGE,
                    ..page.clone()
                };
                let Some(out) = inner.page(&q)? else { break };
                for r in out.records.iter().filter(|r| r.position <= upto) {
                    row(r);
                }
                match out.last {
                    Some(last) if out.more && last < upto => at = last,
                    _ => break,
                }
            }
        } else {
            // No tags yet (the index's shape is built after serving): every
            // ledger row, read for its kind.
            for_each(inner.as_ref(), kinds::LEDGER, after, upto, pace, |r| row(r))?;
        }
        self.through = upto;
        Ok(())
    }

    fn intern(&mut self, id: &str) -> u32 {
        if let Some(&i) = self.ids.get(id) {
            return i;
        }
        let i = self.nodes.len() as u32;
        self.nodes.push(Entry {
            id: id.into(),
            ..Entry::default()
        });
        self.ids.insert(id.into(), i);
        i
    }

    fn link(&mut self, a: u32, b: u32, kind: EdgeKind) {
        if a == b {
            return;
        }
        self.nodes[a as usize].edges.push((b, kind));
        self.nodes[b as usize].edges.push((a, kind));
        self.edges += 2;
    }

    /// A node's record: its place in its session's chain, and its tool pair.
    fn node(&mut self, position: u64, n: &Node) {
        let i = self.intern(&n.id);
        if self.nodes[i as usize].position.is_some() {
            // A node's record again (an erasure's rewrite): its first holds.
            return;
        }
        self.nodes[i as usize].position = Some(position);
        // Exposure, not content: a recall is no one's neighbour.
        if matches!(n.body, Body::Recall { .. }) {
            return;
        }
        if let Some(prev) = self.last.insert(n.session_id.as_str().into(), i) {
            self.link(prev, i, EdgeKind::Neighbour);
        }
        let pair = |id: &str| -> Box<str> { format!("{}\u{1}{id}", n.session_id).into() };
        match &n.body {
            Body::ToolCall { tool_use_id, .. } => {
                let key = pair(tool_use_id);
                match self.results.remove(&key) {
                    Some(r) => self.link(i, r, EdgeKind::ToolResult),
                    None => {
                        self.calls.insert(key, i);
                    }
                }
            }
            Body::ToolResult { tool_use_id, .. } => {
                let key = pair(tool_use_id);
                match self.calls.remove(&key) {
                    Some(c) => self.link(c, i, EdgeKind::ToolResult),
                    None => {
                        self.results.insert(key, i);
                    }
                }
            }
            _ => {}
        }
    }

    /// An EDGE record, by its kind and route.
    fn edge(&mut self, e: &Edge) {
        let m = mapped(e);
        if m == Mapped::Unmapped {
            self.unmapped += 1;
            return;
        }
        let (from, to) = (self.intern(&e.from), self.intern(&e.to));
        match m {
            Mapped::Both(kind) => self.link(from, to, kind),
            Mapped::Supersedes if from != to => {
                // From the newer node to the older: 1.0 toward the newer.
                self.nodes[to as usize]
                    .edges
                    .push((from, EdgeKind::ToNewer));
                self.nodes[from as usize]
                    .edges
                    .push((to, EdgeKind::ToOlder));
                self.edges += 2;
            }
            Mapped::Supersedes | Mapped::Unmapped => {}
        }
    }

    /// A `memory.labeled` row: the node's entities.
    fn labeled(&mut self, data: &serde_json::Value) {
        let Some(id) = data.get("node_id").and_then(|v| v.as_str()) else {
            return;
        };
        let Some(about) = data.get("about").and_then(|v| v.as_array()) else {
            return;
        };
        let i = self.intern(id);
        for term in about.iter().filter_map(|t| t.as_str()) {
            let e = match self.entity_ids.get(term) {
                Some(&e) => e,
                None => {
                    let e = self.entities.len() as u32;
                    self.entities.push(Entity {
                        name: term.into(),
                        nodes: Vec::new(),
                    });
                    self.entity_ids.insert(term.into(), e);
                    e
                }
            };
            if !self.nodes[i as usize].entities.contains(&e) {
                self.nodes[i as usize].entities.push(e);
                self.entities[e as usize].nodes.push(i);
            }
        }
    }

    /// A node's WAL position, once its record is folded.
    pub fn position(&self, id: &str) -> Option<u64> {
        self.ids
            .get(id)
            .and_then(|&i| self.nodes[i as usize].position)
    }

    /// The nodes that mention `entity`, by id.
    pub fn df(&self, entity: &str) -> u32 {
        self.entity_ids
            .get(entity)
            .map_or(0, |&e| self.entities[e as usize].nodes.len() as u32)
    }

    pub fn stats(&self) -> Stats {
        let boxed = |s: &str| s.len() as u64 + 16;
        let mut bytes = 0u64;
        let mut memberships = 0u64;
        for n in &self.nodes {
            bytes += boxed(&n.id) + 64;
            bytes += n.edges.capacity() as u64 * 8 + n.entities.capacity() as u64 * 4;
            memberships += n.entities.len() as u64;
        }
        for e in &self.entities {
            bytes += boxed(&e.name) + 40 + e.nodes.capacity() as u64 * 4;
        }
        // The maps: each key boxed again, its value, and the table's slack.
        let map = |len: usize, key: u64| len as u64 * (key + 24) * 8 / 7;
        bytes += map(self.ids.len(), 32) + map(self.entity_ids.len(), 32);
        bytes += map(self.last.len() + self.calls.len() + self.results.len(), 48);
        Stats {
            nodes: self.nodes.len() as u64,
            edges: self.edges,
            entities: self.entities.len() as u64,
            memberships,
            unmapped: self.unmapped,
            bytes,
            through: self.through,
        }
    }

    /// The projection as a spread walks it under `p`, with `seed` (a node
    /// the projection may not hold yet, or a search's query) given the
    /// entities `seed_entities` besides its own.
    pub fn view<'a>(
        &'a self,
        p: &SpreadParams,
        seed: Option<&'a str>,
        seed_entities: &'a [String],
    ) -> View<'a> {
        View {
            p: self,
            cap: cap(p),
            seed,
            seed_entities,
        }
    }
}

/// The largest `df` whose one shared-entity edge carries a seed of 1.0 over
/// the threshold under `p`: past it, an entity is not expanded.
pub fn cap(p: &SpreadParams) -> u32 {
    let ok = |df: u32| {
        p.weights.weight(EdgeKind::SharedEntity { df }) * p.decay >= p.threshold
            && p.threshold > 0.0
    };
    if !ok(2) {
        // The threshold, or a weight, leaves no entity anything to carry.
        return if p.threshold > 0.0 { 0 } else { u32::MAX };
    }
    let (mut lo, mut hi) = (2u32, 1u32 << 30);
    if ok(hi) {
        return u32::MAX;
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if ok(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The projection as one spread walks it.
pub struct View<'a> {
    p: &'a Projection,
    /// The largest `df` expanded.
    pub cap: u32,
    seed: Option<&'a str>,
    seed_entities: &'a [String],
}

impl View<'_> {
    fn entity(&self, e: u32, except: Option<u32>, out: &mut Vec<(String, EdgeKind)>) {
        let ent = &self.p.entities[e as usize];
        let df = ent.nodes.len() as u32;
        if df > self.cap {
            return;
        }
        for &j in &ent.nodes {
            if Some(j) != except {
                out.push((
                    self.p.nodes[j as usize].id.to_string(),
                    EdgeKind::SharedEntity { df },
                ));
            }
        }
    }
}

impl Adjacency<String> for View<'_> {
    fn edges(&self, node: &String, out: &mut Vec<(String, EdgeKind)>) {
        let at = self.p.ids.get(node.as_str()).copied();
        if let Some(i) = at {
            let n = &self.p.nodes[i as usize];
            for &(j, kind) in &n.edges {
                out.push((self.p.nodes[j as usize].id.to_string(), kind));
            }
            for &e in &n.entities {
                self.entity(e, Some(i), out);
            }
        }
        if self.seed == Some(node.as_str()) {
            for term in self.seed_entities {
                let Some(&e) = self.p.entity_ids.get(term.as_str()) else {
                    continue;
                };
                // An entity the node's own labels hold is expanded once.
                if at.is_some_and(|i| self.p.nodes[i as usize].entities.contains(&e)) {
                    continue;
                }
                // The seed, though named in it, is no edge of its own.
                self.entity(e, at, out);
            }
        }
    }
}

/// Every record of `kind` with `after < position <= upto`, a page at a time.
fn for_each(
    store: &dyn theseus_store::Store,
    kind: RecordKind,
    after: u64,
    upto: u64,
    pace: &mut dyn FnMut(),
    mut f: impl FnMut(&Record),
) -> anyhow::Result<()> {
    let mut at = after;
    loop {
        if at != after {
            pace();
        }
        let page = store.of_kind_after(kind, at, PAGE)?;
        let Some(last) = page.last() else {
            return Ok(());
        };
        let last = last.position;
        for r in page.iter().filter(|r| r.position <= upto) {
            f(r);
        }
        if last >= upto || page.len() < PAGE {
            return Ok(());
        }
        at = last;
    }
}
