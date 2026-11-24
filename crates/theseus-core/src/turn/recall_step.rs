//! The recall step on a turn's first loop (M6 steps 30a, 30b;
//! `crate::recall`).
//!
//! - **The arm's sources** (34b): a live arm asks the index for its own
//!   (`MemoryArm::sources`: `bm25` BM25 and entities, `baseline` all three);
//!   arm `none` asks nothing. There is no shadow recall path.
//! - **Live** (30b): read before the compile, never past
//!   `[memory] recall_deadline_ms`. What it admits is a `Recall` node, built
//!   here and held in the turn (`Recalled`) until it rides the provider
//!   call's plan frame with its `derived_from` edges; the compile renders it
//!   after the new message as though it were written (`recall_view`), and
//!   the new compilation leaves it out of its prefix, since its position
//!   comes after the compilation's `as_of`. The `recall.ran` row rides in
//!   the turn's next frame.
//! - **While routing decides** (theseus-n7nc): the row waits for the route
//!   (`recall_routed`), and is recorded once it is known.
//! - **Every turn carries its node** (issue 44, issue 46): a view turn's too,
//!   so its count, citations, reach and retraction see what it recalled; the
//!   view lays the items beside the message and keeps the node out of the
//!   window and the turn's tail. The withholding that once kept it out (the
//!   `recall.withheld` row, theseus-ectzg) is gone; old rows still read. (Two earlier gates, the small-talk bar and a model
//!   of restricted origin, are gone; their old rows still read.)

use std::collections::BTreeSet;

use theseus_protocol::memory::{BudgetDrop, RecallManifest};
use theseus_store::NewRecord;

use super::{Turn, TurnRunner};
use crate::compiler::Compiled;
use crate::config::memory::{Assigned, MemoryArm};
use crate::fact::recall::{scope, RecallRan};
use crate::graph::{Edge, EdgeKind, VIA_RECALL};
use crate::node::{Body, Node, RecalledRef};
use crate::recall::render::{self, Sources};
use crate::recall::{query_of, text_of, Begun, Scene};
use crate::session::SessionRecord;
use crate::store::Transcript;

/// What recall put in front of the model this turn.
#[derive(Default)]
pub(crate) struct Recalled {
    /// The `Recall` node, until the plan frame writes it.
    pub pending: Option<Node>,
    /// Its record and its edges, for the plan frame.
    pub rides: Vec<NewRecord>,
    /// What its pack left out for the budget: the compilation's report
    /// carries them.
    pub drops: Vec<BudgetDrop>,
    /// The notes it admitted: the reply's footer says them.
    pub count: u32,
    /// What the notes cite, for the reply's citations (theseus-ejdk): the
    /// admitted items as the node holds them, handles in their headers.
    /// Read only while `count` is above zero.
    pub items: Vec<RecalledRef>,
    /// The node's id, for `memory.cited`'s rows.
    pub recall_id: String,
    /// A task's first compile (30c): its pack takes `assembled_budget_tokens`.
    /// (The view lays recall as slot H's pieces; nothing renders the node first.)
    pub assembled: bool,
    /// The `recall.ran` rows of recalls made while routing had the turn's
    /// route to decide, with their times: recorded once it is known
    /// (`recall_routed`, theseus-n7nc).
    pub held: Vec<(RecallManifest, u64, u64)>,
}

/// The position a pending `Recall` node renders at: after every node, as
/// the plan frame will write it.
const PENDING: u64 = u64::MAX;

impl TurnRunner {
    /// The first loop's recall: read now, under its deadline, and its node
    /// held for the compile. Nothing here fails the turn.
    pub(super) async fn recall_first(&self, t: &mut Turn<'_>, session: &SessionRecord) {
        if !self.memory.on() {
            return;
        }
        // A task's first compile is assembled (30c): its recall section
        // goes first in the prefix, at the assembled budget.
        t.recall.assembled = session.task.is_some() && session.compilation_id.is_none();
        let Some(a) = self.memory.cfg().assign(t.tc.session_id) else {
            return;
        };
        // `live` with arm `none` (the memory exam's `none` daemon, 34b): the
        // index is asked nothing.
        if !a.live {
            return;
        }
        let Some(begun) = self.recall_begin(t, a.arm) else {
            return;
        };
        // On the heap: the turn's own future stays the size it was before
        // the arms' work (32b) joined this one.
        Box::pin(self.recall_live(t, begun, a)).await;
    }

    /// Ask the index for `arm`'s sources, when the turn brings something
    /// new; every arm leaves the memory's harness session out (31b).
    fn recall_begin(&self, t: &Turn<'_>, arm: MemoryArm) -> Option<Begun> {
        self.memory.syntheses(&self.store);
        let nodes = match t.tc.store.transcript(t.tc.session_id) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "recall: the transcript cannot be read; no recall this turn");
                return None;
            }
        };
        // The message is searched as written: recall reads no English from it.
        // A time is the model's to ask, through the history tool (issue 46).
        let (query, as_of) = query_of(&nodes, t.tc.turn_id)?;
        let deadline = std::time::Duration::from_millis(self.memory.cfg().recall_deadline_ms);
        let mut begun = self.memory.begin_turn(query, Some(as_of), arm, deadline);
        // The turn's new node seeds a spread (32b).
        begun.new_node = nodes
            .iter()
            .find(|(p, _)| *p == as_of)
            .map(|(_, n)| n.id.clone());
        Some(begun)
    }

    /// The scene of the turn's recall under `arm`'s science: its place, the
    /// nodes its request carries (and the sources of the recalls among
    /// them), and the labels.
    pub(super) fn scene<'a>(&self, t: &'a Turn<'_>, mode: &'a str, arm: MemoryArm) -> Scene<'a> {
        // A turn counts only what its view lays (issue 44).
        let in_context = self.view_in_context(t);
        let labeled = self
            .memory
            .labeled(&self.store)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %format!("{e:#}"), "recall: the labels cannot be read; none is applied");
                BTreeSet::new()
            });
        // Every turn draws on every place (anything goes); the manifest
        // names the place it asked from.
        let place = self.place_of(t.tc.session_id);
        let category = self.category_scene(Some(t.tc.session_id), true);
        // The prompt's tier sizes the pack (theseus-d9je7); an assembled
        // section keeps its own budget.
        let (budget_tokens, max_items) = match t.recall.assembled {
            true => (Some(self.memory.cfg().assembled_budget_tokens), None),
            false => t
                .route
                .injection
                .as_ref()
                .map(|b| (b.recall_tokens, b.recall_items))
                .unzip(),
        };
        Scene {
            mode,
            session_id: Some(t.tc.session_id),
            turn_id: Some(t.tc.turn_id),
            place,
            in_context,
            labeled,
            budget_tokens,
            max_items,
            science: self.memory.science_for(arm),
            persona: self.persona_scene(t.target.persona.as_deref()),
            category,
        }
    }

    /// Recall in front of the model: the answer now, the pack, and its
    /// `Recall` node with its edges, held for the compile and the plan
    /// frame.
    async fn recall_live(&self, t: &mut Turn<'_>, mut begun: Begun, a: Assigned) {
        let t0 = t.trace.at(begun.started);
        let answer = begun.answer().await;
        // The tier, if the verdict has landed; ordinary if not: never a wait.
        self.injection(t, true);
        let mode = self.memory.cfg().mode.as_str();
        let mut m = self.recall_reranked(t, mode, a.arm, &begun, answer).await;
        self.memory.noted(&m);
        m.arm = Some(a.arm.as_str().into());
        if !m.admitted.is_empty() {
            self.hold_node(t, &m, a);
        }
        // The row keeps references, never copies.
        for item in &mut m.admitted {
            item.text = None;
        }
        let t1 = t.trace.now_us();
        // The row waits for the route (theseus-n7nc).
        if t.route.deciding() {
            t.recall.held.push((m, t0, t1));
            return;
        }
        Self::record_ran(t, &m, t0, t1);
    }

    /// The route is known (theseus-n7nc): the rows held while routing decided
    /// are recorded.
    pub(super) fn recall_routed(t: &mut Turn<'_>) {
        for (m, t0, t1) in std::mem::take(&mut t.recall.held) {
            Self::record_ran(t, &m, t0, t1);
        }
    }

    /// A recall's `recall.ran` row, for the turn's next frame, with its span
    /// and its line.
    fn record_ran(t: &mut Turn<'_>, m: &RecallManifest, t0: u64, t1: u64) {
        let f = RecallRan {
            manifest: m,
            t0,
            t1,
        };
        defer_row(t, &f);
        t.announce_fact(&f);
    }

    /// The `Recall` node of `m`'s admitted items, each read from its source
    /// by position for its frozen range and header, with an edge to each
    /// source; held in the turn for the compile and the plan frame.
    fn hold_node(&self, t: &mut Turn<'_>, m: &RecallManifest, a: Assigned) {
        let mut items = Vec::new();
        let ids: Vec<&str> = m.admitted.iter().map(|i| i.node_id.as_str()).collect();
        let handles = theseus_protocol::cite::handles_for(&ids);
        let persona = self.persona_scene(t.target.persona.as_deref());
        // Where an old thread's session said a thing: its thread's place, since such a session speaks
        // nowhere else (issue 46: its provenance, which nothing ranks by).
        let board = self.threads.all(&self.store).ok();
        let memo = self.memory.thread_memo();
        let said_at = |session: &str| -> String {
            let place = self.place_name(session);
            if place != render::unplaced(session) {
                return place;
            }
            board
                .as_deref()
                .and_then(|b| memo.slug_of(&self.store, b, session))
                .map_or(place, |slug| theseus_protocol::threads::place_of(&slug))
        };
        for (item, handle) in m.admitted.iter().zip(&handles) {
            let probe = RecalledRef {
                node_id: item.node_id.clone(),
                session_id: item.session_id.clone(),
                position: item.position,
                chunk: (0, 0),
                header: String::new(),
                tokens: item.tokens,
            };
            let Some(source) = self.memory.read_source(&self.store, &probe) else {
                tracing::warn!(node_id = %item.node_id, "recall: an admitted source cannot be read; it is left out");
                continue;
            };
            let excerpt = item.text.as_deref().unwrap_or_default();
            let chunk = render::frozen_range(&text_of(&source), excerpt);
            let place = said_at(&item.session_id);
            // Where and by whom it was said: its place, never a thread's ranking (issue 46).
            let mut header = render::item_header(&source, item.position, &place, chunk);
            header = crate::recall::neighbour::with_note(header, item);
            if let Some(p) = persona.as_ref().filter(|p| p.is_own(&item.session_id)) {
                header = format!(
                    "{} {header}",
                    render::persona_mark(&p.name, &source, item.position)
                );
            }
            items.push(RecalledRef {
                chunk,
                header: theseus_protocol::cite::header_with(handle, &header),
                ..probe
            });
        }
        if items.is_empty() {
            return;
        }
        let node = Node::recall(
            t.tc.session_id,
            t.tc.turn_id,
            &m.recall_id,
            a.arm.as_str(),
            items,
        );
        let rides = match recall_records(&node) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "recall: its node cannot be encoded; no recall this turn");
                return;
            }
        };
        let Body::Recall { items, .. } = &node.body else {
            return;
        };
        t.recall.count = items.len() as u32;
        t.recall.items = items.clone();
        t.recall.recall_id = node.id.clone();
        t.recall.drops = m
            .budget
            .as_ref()
            .map(|b| b.dropped.clone())
            .unwrap_or_default();
        t.recall.rides = rides;
        t.recall.pending = Some(node);
    }

    /// [`Self::recall_view`] for the turn's own compile, which also records
    /// whether this request carries the pending node (theseus-fpm2). A later
    /// compile of the same loop, which a route's switch makes, overwrites it,
    /// so the call reads the compile it used.
    pub(super) fn recall_carried(
        &self,
        t: &mut Turn<'_>,
        nodes: Transcript,
    ) -> (Transcript, Sources) {
        // Every turn carries its pending node, a view turn's included (issue 44, issue 46): the node,
        // its edges and its rows ride the plan frame, so the reply's count, its citations, reach and
        // retraction see what the turn recalled. The view lays the items beside the message and keeps
        // the node itself out of the window and out of the turn's own tail.
        self.recall_view(t, nodes)
    }

    /// The transcript a loop compiles, with the turn's pending `Recall` node
    /// after every node, and the sources its recalls render.
    pub(super) fn recall_view(&self, t: &Turn<'_>, nodes: Transcript) -> (Transcript, Sources) {
        let mut nodes = nodes;
        if let Some(n) = t.recall.pending.as_ref() {
            // Once the plan frame wrote it, the transcript holds it.
            if !nodes.iter().any(|(_, m)| m.id == n.id) {
                nodes.push((PENDING, n.clone().into()));
            }
        }
        let recalls = nodes
            .iter()
            .filter(|(_, n)| n.kind == crate::stub::Kind::Recall);
        let sources = match recalls.clone().next() {
            Some(_) => self
                .memory
                .read_sources(&self.store, recalls.map(|(_, n)| &**n)),
            None => Sources::new(),
        };
        (nodes, sources)
    }

    /// After the compile: a new compilation's prefix leaves out the pending
    /// node (its position will come after the compilation's `as_of`, so it
    /// renders in the tail, as it did here), and the report carries what the
    /// recall's pack left out.
    pub(super) fn recall_compiled(t: &mut Turn<'_>, compiled: &mut Compiled) {
        let Some(n) = &t.recall.pending else {
            return;
        };
        compiled.compilation.includes.retain(|id| *id != n.id);
        // While routing decides, this compile may be discarded for a switch's:
        // the drops stay for the compile the call uses (theseus-3urn), and
        // `keep_first` lets them go once it is this one.
        let drops = match t.route.defer_persist {
            true => t.recall.drops.clone(),
            false => std::mem::take(&mut t.recall.drops),
        };
        compiled.budget.dropped.extend(drops);
        if compiled.new_compilation {
            compiled.compilation.budget = Some(compiled.budget.clone());
        }
    }
}

/// A `Recall` node's records: the node, and a `derived_from` edge to each source it renders.
fn recall_records(node: &Node) -> anyhow::Result<Vec<NewRecord>> {
    let mut rides = vec![node.record()?];
    if let Body::Recall { items, .. } = &node.body {
        for r in items {
            let e = Edge::new(EdgeKind::DerivedFrom, &node.id, &r.node_id, VIA_RECALL);
            rides.push(e.record()?);
        }
    }
    Ok(rides)
}

/// A recall's row, scoped to the session's recalls, for the turn's next
/// frame.
pub(super) fn defer_row<F: crate::fact::Fact>(t: &mut Turn<'_>, f: &F) {
    match t.tc.rec().row(f) {
        Ok(r) => {
            if let Err(e) = t.tc.store.defer(r.scoped(&scope(t.tc.session_id))) {
                tracing::warn!(error = %e, "ledger append failed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "recall: its row cannot be encoded"),
    }
}
