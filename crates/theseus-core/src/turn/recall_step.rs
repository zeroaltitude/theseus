//! The recall step on a turn's first loop (M6 steps 30a, 30b;
//! `crate::recall`).
//!
//! - **Shadow** (30a, and a canary's control): begun as the loop's model call
//!   goes out, and read once it has answered, so the index's wait overlaps
//!   the model's. Its row rides in the turn's next frame, and the request
//!   the model got is the one compiled without it.
//! - **The arm's sources** (34b): a live arm asks the index for its own
//!   (`MemoryArm::sources`: `bm25` BM25 and entities, `baseline` all three);
//!   shadow, and a canary's control, ask for `baseline`'s; and `live` with
//!   arm `none` asks nothing.
//! - **Canary and live** (30b): read before the compile, never past
//!   `[memory] recall_deadline_ms`. What it admits is a `Recall` node, built
//!   here and held in the turn (`Recalled`) until it rides the provider
//!   call's plan frame with its `derived_from` edges; the compile renders it
//!   after the new message as though it were written (`recall_view`), and
//!   the new compilation leaves it out of its prefix, since its position
//!   comes after the compilation's `as_of`. The `recall.ran` row rides in
//!   the turn's next frame.
//! - **A trivial detour** (theseus-n7nc): while routing has the turn's route
//!   to decide, the row waits for it (`recall_routed`). A detour's request
//!   carries no recall, so its node rides nothing, the reply's footer counts
//!   none, and its row says `detoured`.

use std::collections::BTreeSet;

use theseus_protocol::memory::{BudgetDrop, RecallManifest};
use theseus_store::NewRecord;

use super::{Turn, TurnRunner};
use crate::compiler::Compiled;
use crate::config::memory::{Assigned, MemoryArm};
use crate::config::MemoryMode;
use crate::fact::recall::{scope, ArmAssigned, RecallRan, RecallShadow};
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
    /// It is an assembled prefix's recall section (30c): a task's first
    /// compile, or a compaction. Its pack takes `assembled_budget_tokens`,
    /// and the new compilation renders it first in its prefix.
    pub assembled: bool,
    /// The `recall.ran` rows of recalls made while routing had the turn's
    /// route to decide, with their times: recorded once it is known
    /// (`recall_routed`, theseus-n7nc).
    pub held: Vec<(RecallManifest, u64, u64)>,
}

impl Recalled {
    /// The pending node an assembled prefix renders first (`recall_id`).
    pub fn assembled_id(&self) -> Option<&str> {
        self.pending
            .as_ref()
            .filter(|_| self.assembled)
            .map(|n| n.id.as_str())
    }
}

/// The position a pending `Recall` node renders at: after every node, as
/// the plan frame will write it.
const PENDING: u64 = u64::MAX;

/// Why a detoured recall reached no request (theseus-n7nc).
const DETOURED: &str =
    "route.v1 judged the message trivial, and the detour's request carries no recall";

impl TurnRunner {
    /// The first loop's recall. In front of the model, it is read now and
    /// its node held for the compile; in shadow, its query goes out and the
    /// caller reads it once the call has answered (`recall_end`). Nothing
    /// here fails the turn.
    pub(super) async fn recall_first(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
    ) -> Option<Begun> {
        if !self.memory.on() {
            return None;
        }
        // A task's first compile is assembled (30c): its recall section
        // goes first in the prefix, at the assembled budget.
        t.recall.assembled = session.task.is_some() && session.compilation_id.is_none();
        let assigned = self.memory.cfg().assign(t.tc.session_id);
        if let Some(a) = assigned {
            self.record_arm(t, a);
        }
        match assigned {
            Some(a) if a.live => {
                let begun = self.recall_begin(t, a.arm)?;
                // On the heap: the turn's own future stays the size it was
                // before the arms' work (32b) joined this one.
                Box::pin(self.recall_live(t, Some(session), begun, a)).await;
                None
            }
            // `live` with arm `none` (the exam's `none` daemon, 34b): today's
            // compiler, and the index is asked nothing. A canary's control
            // still runs `baseline` in shadow.
            Some(_) if self.memory.cfg().mode == MemoryMode::Live => None,
            _ => self.recall_begin(t, MemoryArm::Baseline),
        }
    }

    /// Ask the index for `arm`'s sources, when the turn brings something
    /// new; every arm but `+synthesis` leaves the memory's harness session
    /// out (31b).
    fn recall_begin(&self, t: &Turn<'_>, arm: MemoryArm) -> Option<Begun> {
        self.memory.syntheses(&self.store);
        let nodes = match t.tc.store.transcript(t.tc.session_id) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "recall: the transcript cannot be read; no recall this turn");
                return None;
            }
        };
        let (query, as_of) = query_of(&nodes, t.tc.turn_id)?;
        let deadline = std::time::Duration::from_millis(self.memory.cfg().recall_deadline_ms);
        let mut begun = self
            .memory
            .begin(query, Some(as_of), crate::recall::K, arm, deadline);
        // The turn's new node seeds a spread (32b).
        begun.new_node = nodes
            .iter()
            .find(|(p, _)| *p == as_of)
            .map(|(_, n)| n.id.clone());
        Some(begun)
    }

    /// The session's `memory.arm` row, once: its first turn under canary or
    /// live writes it, in the turn's next frame.
    fn record_arm(&self, t: &mut Turn<'_>, a: Assigned) {
        let sid = t.tc.session_id;
        match self.memory.arm_recorded(t.tc.store, sid) {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "recall: the session's arm row cannot be read");
                return;
            }
        }
        let cfg = self.memory.cfg();
        let f = ArmAssigned {
            mode: cfg.mode.as_str(),
            arm: a.arm.as_str(),
            live: a.live,
            experiment: &cfg.experiment,
            science: &self.memory.science_for(a.arm).id().to_string(),
        };
        match t.tc.rec().row(&f) {
            Ok(r) => match t.tc.store.defer(r.scoped(&scope(sid))) {
                Ok(()) => self.memory.armed(sid),
                Err(e) => tracing::warn!(error = %e, "ledger append failed"),
            },
            Err(e) => tracing::warn!(error = %e, "recall: the arm's row cannot be encoded"),
        }
        t.announce_fact(&f);
    }

    /// The scene of the turn's recall under `arm`'s science: its place, the
    /// nodes its request carries (and the sources of the recalls among
    /// them), and the labels. An arm that reads retention builds the
    /// projection when nothing has yet (off the turn's path: this recall
    /// ranks without).
    pub(super) fn scene<'a>(&self, t: &'a Turn<'_>, mode: &'a str, arm: MemoryArm) -> Scene<'a> {
        if arm.reads_retention() {
            crate::recall::retention::warm(&self.memory, &self.store);
        }
        let in_context = Self::in_context(t);
        let labeled = self
            .memory
            .labeled(&self.store)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %format!("{e:#}"), "recall: the labels cannot be read; none is applied");
                BTreeSet::new()
            });
        Scene {
            mode,
            session_id: Some(t.tc.session_id),
            turn_id: Some(t.tc.turn_id),
            place: self.place_of(t.tc.session_id),
            in_context,
            labeled,
            budget_tokens: t
                .recall
                .assembled
                .then(|| self.memory.cfg().assembled_budget_tokens),
            science: self.memory.science_for(arm),
            activation: None,
        }
    }

    /// The nodes the turn's request carries, and the sources of the recalls
    /// among them.
    fn in_context(t: &Turn<'_>) -> BTreeSet<String> {
        let mut in_context = BTreeSet::new();
        if let Ok(nodes) = t.tc.store.transcript(t.tc.session_id) {
            // An assembled section's context is what its prefix keeps: past
            // a compaction, the summarized range is out of it (30c).
            let floor = crate::compiler::compaction::floor(&nodes).map(|(_, last)| last);
            let after = floor.filter(|_| t.recall.assembled).unwrap_or(0);
            for (_, n) in nodes.iter().filter(|(p, _)| *p > after) {
                in_context.insert(n.id.clone());
                if n.kind != crate::stub::Kind::Recall {
                    continue;
                }
                if let Body::Recall { items, .. } = &n.body {
                    in_context.extend(items.iter().map(|r| r.node_id.clone()));
                }
            }
        }
        in_context
    }

    /// Read the index's answer (never past its deadline), run the pipeline,
    /// and record the shadow recall: its row, scoped to the session's
    /// recalls, its span, and its line.
    pub(super) async fn recall_end(&self, t: &mut Turn<'_>, mut begun: Begun) {
        let t0 = t.trace.at(begun.started);
        let answer = begun.answer().await;
        let scene = self.scene(t, "shadow", MemoryArm::Baseline);
        let (mut m, candidates, links) = self.memory.manifest_with(
            &scene,
            &begun,
            answer,
            |s| self.place_of(s),
            |ids| crate::recall::links(&self.store, ids),
            false,
        );
        // What the rerank reads of the scene (32c), taken now: the rest of
        // the scene borrows the turn, which the row and the mark need.
        let retention = self.memory.retention_of(&*scene.science, &candidates);
        let Scene {
            place,
            in_context,
            labeled,
            science,
            ..
        } = scene;
        m.arm = self
            .memory
            .cfg()
            .assign(t.tc.session_id)
            .map(|a| a.arm.as_str().to_string());
        let f = RecallShadow {
            manifest: &m,
            t0,
            t1: t.trace.now_us(),
        };
        defer_row(t, &f);
        t.announce_fact(&f);
        // The `+rerank` arm in shadow (32c): off the turn's path.
        self.judge.at_recall(
            &mut t.trace,
            crate::judge::rerank::Recalled {
                recall_id: m.recall_id.clone(),
                session_id: t.tc.session_id.to_string(),
                turn_id: t.tc.turn_id.to_string(),
                message: begun.query.clone(),
                place,
                in_context,
                labeled,
                candidates,
                links,
                params: self.memory.cfg().params(),
                science,
                retention,
                admitted: m
                    .admitted
                    .iter()
                    .map(|a| format!("{}#{}", a.node_id, a.chunk))
                    .collect(),
                now_ms: theseus_protocol::now_unix_ms(),
            },
        );
    }

    /// Recall in front of the model: the answer now, the pack, and its
    /// `Recall` node with its edges, held for the compile and the plan
    /// frame. Past the session's cap it pauses, and writes no node.
    async fn recall_live(
        &self,
        t: &mut Turn<'_>,
        session: Option<&SessionRecord>,
        mut begun: Begun,
        a: Assigned,
    ) {
        let t0 = t.trace.at(begun.started);
        let answer = begun.answer().await;
        let mode = self.memory.cfg().mode.as_str();
        // The arm's own source before the pipeline (32b): activation.
        let (answer, activation) = Box::pin(self.memory.activated(
            &self.store,
            a.arm,
            &begun,
            answer,
            Self::in_context(t),
            false,
        ))
        .await;
        let mut m = self
            .recall_reranked(t, mode, a.arm, activation, &begun, answer)
            .await;
        m.arm = Some(a.arm.as_str().into());
        let cap = self.memory.cfg().session_recall_cap_tokens;
        // An assembled section sits in the prefix: the tail's cap is not its.
        let in_tail = session.map_or(0, |s| self.recall_tokens_in_tail(t, s));
        if !m.admitted.is_empty() && in_tail + m.used_tokens > cap {
            m.outcome = "paused".into();
            m.why = Some(format!(
                "the session's tail holds {in_tail} tokens of recall notes, and {} more would \
                 pass session_recall_cap_tokens = {cap}",
                m.used_tokens
            ));
        } else if !m.admitted.is_empty() {
            self.hold_node(t, &m, a);
        }
        // The row keeps references, never copies.
        for item in &mut m.admitted {
            item.text = None;
        }
        let t1 = t.trace.now_us();
        // A trivial detour would send none of it: the row waits for the
        // route (theseus-n7nc).
        if t.route.deciding() {
            t.recall.held.push((m, t0, t1));
            return;
        }
        Self::record_ran(t, &m, t0, t1);
    }

    /// The route is known (theseus-n7nc). A trivial detour's request carried
    /// no recall (`compile_detour` reads no `recall_view`), so its node rides
    /// nothing, the reply's footer counts none, and its row says `detoured`.
    /// Then the rows held while routing decided are recorded.
    pub(super) fn recall_routed(t: &mut Turn<'_>) {
        if t.route.keeps.is_some() {
            if let Some(Body::Recall { recall_id, .. }) = t.recall.pending.take().map(|n| n.body) {
                let mut held = t.recall.held.iter_mut();
                if let Some((m, ..)) = held.find(|(m, ..)| m.recall_id == recall_id) {
                    m.outcome = "detoured".into();
                    m.why = Some(DETOURED.into());
                }
            }
            t.recall.rides.clear();
            t.recall.drops.clear();
            t.recall.count = 0;
        }
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

    /// A compaction's assembled recall section (30c): the first loop's
    /// pending recall, when there is one, becomes it; otherwise recall runs
    /// now at the assembled budget, under its deadline, in front of the
    /// model or in shadow as the session's arm says. Its node's id, when
    /// it admitted something in front of the model.
    pub(super) async fn recall_assembled(&self, t: &mut Turn<'_>) -> Option<String> {
        if !self.memory.on() {
            return None;
        }
        t.recall.assembled = true;
        if let Some(n) = &t.recall.pending {
            return Some(n.id.clone());
        }
        // memory-arm's rule (34b), as `recall_first`: a live arm asks for its
        // own sources, `live` with arm `none` asks nothing, shadow `baseline`'s.
        let assigned = self.memory.cfg().assign(t.tc.session_id);
        let begun = match assigned {
            Some(a) if a.live => self.recall_begin(t, a.arm)?,
            Some(_) if self.memory.cfg().mode == MemoryMode::Live => return None,
            _ => self.recall_begin(t, MemoryArm::Baseline)?,
        };
        match assigned {
            Some(a) if a.live => Box::pin(self.recall_live(t, None, begun, a)).await,
            _ => self.recall_end(t, begun).await,
        }
        t.recall.pending.as_ref().map(|n| n.id.clone())
    }

    /// Drop an assembled section the compaction did not use: its node
    /// rides nothing, and the next loop recalls nothing more.
    pub(super) fn recall_unassembled(t: &mut Turn<'_>, fresh: bool) {
        t.recall.assembled = false;
        if fresh {
            t.recall.pending = None;
            t.recall.rides.clear();
            t.recall.drops.clear();
            t.recall.count = 0;
        }
    }

    /// The tokens of the recall notes in the session's tail: those after
    /// its current compilation's `as_of`.
    fn recall_tokens_in_tail(&self, t: &Turn<'_>, session: &SessionRecord) -> u64 {
        let current = session
            .compilation_id
            .as_deref()
            .and_then(|id| self.store.get_compilation(id).ok().flatten());
        let as_of = current.as_ref().map_or(0, |c| c.as_of);
        // An assembled prefix's section is its prefix's, not the tail's.
        let section = current.and_then(|c| c.recall_id);
        let Ok(nodes) = t.tc.store.transcript(t.tc.session_id) else {
            return 0;
        };
        nodes
            .iter()
            .filter(|(p, n)| *p > as_of && section.as_deref() != Some(n.id.as_str()))
            .filter(|(_, n)| n.kind == crate::stub::Kind::Recall)
            .map(|(_, n)| match &n.body {
                Body::Recall { items, .. } => items.iter().map(|r| r.tokens).sum(),
                _ => 0,
            })
            .sum()
    }

    /// The `Recall` node of `m`'s admitted items, each read from its source
    /// by position for its frozen range and header, with an edge to each
    /// source; held in the turn for the compile and the plan frame.
    fn hold_node(&self, t: &mut Turn<'_>, m: &RecallManifest, a: Assigned) {
        let mut items = Vec::new();
        for item in &m.admitted {
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
            let place = self.place_name(&item.session_id);
            items.push(RecalledRef {
                chunk,
                header: render::item_header(&source, item.position, &place, chunk),
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
        let mut rides = Vec::new();
        let records = node.record().and_then(|r| {
            rides.push(r);
            let Body::Recall { items, .. } = &node.body else {
                return Ok(());
            };
            for r in items {
                let e = Edge::new(EdgeKind::DerivedFrom, &node.id, &r.node_id, VIA_RECALL);
                rides.push(e.record()?);
            }
            Ok(())
        });
        if let Err(e) = records {
            tracing::warn!(error = %format!("{e:#}"), "recall: its node cannot be encoded; no recall this turn");
            return;
        }
        let Body::Recall { items, .. } = &node.body else {
            return;
        };
        t.recall.count = items.len() as u32;
        t.recall.drops = m
            .budget
            .as_ref()
            .map(|b| b.dropped.clone())
            .unwrap_or_default();
        t.recall.rides = rides;
        t.recall.pending = Some(node);
    }

    /// The transcript a loop compiles, with the turn's pending `Recall` node
    /// after every node, and the sources its recalls render.
    pub(super) fn recall_view(&self, t: &Turn<'_>, nodes: Transcript) -> (Transcript, Sources) {
        let mut nodes = nodes;
        if let Some(n) = &t.recall.pending {
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
        let drops = std::mem::take(&mut t.recall.drops);
        compiled.budget.dropped.extend(drops);
        if compiled.new_compilation {
            compiled.compilation.budget = Some(compiled.budget.clone());
        }
    }
}

/// A recall's row, scoped to the session's recalls, for the turn's next
/// frame.
fn defer_row<F: crate::fact::Fact>(t: &mut Turn<'_>, f: &F) {
    match t.tc.rec().row(f) {
        Ok(r) => {
            if let Err(e) = t.tc.store.defer(r.scoped(&scope(t.tc.session_id))) {
                tracing::warn!(error = %e, "ledger append failed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "recall: its row cannot be encoded"),
    }
}
