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
//!   to decide, the row waits for it (`recall_routed`). A trivial detour's
//!   request carries no recall, so its node rides nothing, the reply's footer
//!   counts none, and its row says `detoured`. A quick detour's carries it
//!   (theseus-pa1xx): its node, footer and row are a kept compile's.

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
    /// What the notes cite, for the reply's citations (theseus-ejdk): the
    /// admitted items as the node holds them, handles in their headers.
    /// Read only while `count` is above zero.
    pub items: Vec<RecalledRef>,
    /// The node's id, for `memory.cited`'s rows.
    pub recall_id: String,
    /// It is an assembled prefix's recall section (30c): a task's first
    /// compile, or a compaction. Its pack takes `assembled_budget_tokens`,
    /// and the new compilation renders it first in its prefix.
    pub assembled: bool,
    /// The `recall.ran` rows of recalls made while routing had the turn's
    /// route to decide, with their times: recorded once it is known
    /// (`recall_routed`, theseus-n7nc).
    pub held: Vec<(RecallManifest, u64, u64)>,
    /// It admitted a note of a session that holds the Google `private` mark
    /// (theseus-39dj0): this turn's calls are private before the plan frame
    /// writes the session's own mark, which rides with the node.
    pub private: bool,
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
    "the route pack judged the message trivial, and a trivial detour's request carries no recall";

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
        let (mut query, as_of) = query_of(&nodes, t.tc.turn_id)?;
        // A question about time searches on its event: the temporal words
        // match every dated note (theseus-q1tbn).
        let temporal = crate::recall::temporal::of_turn(&nodes, t.tc.turn_id);
        if temporal.is_some() {
            query.words = crate::recall::temporal::subject(&query.words);
            query.vector = query.vector.map(|v| crate::recall::temporal::subject(&v));
        }
        let deadline = std::time::Duration::from_millis(self.memory.cfg().recall_deadline_ms);
        // A span of time the turn's words name keeps recall inside it
        // (theseus-w9qv): read here, in microseconds, with no call.
        let now = theseus_protocol::now_unix_ms();
        let when = crate::recall::when::of_turn(&nodes, t.tc.turn_id, now);
        let mut begun = self.memory.begin_within(
            query,
            Some(as_of),
            crate::recall::K,
            arm,
            deadline,
            when,
            Vec::new(),
        );
        begun.temporal = temporal.map(|a| a.kind);
        begun.said = crate::recall::said_of(&nodes, t.tc.turn_id);
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
    pub(super) fn scene<'a>(
        &self,
        t: &'a Turn<'_>,
        mode: &'a str,
        arm: MemoryArm,
        message: &str,
    ) -> Scene<'a> {
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
        let place = match t.tc.draft {
            Some(_) => theseus_memory::Place::Open(None),
            None => self.place_of(t.tc.session_id),
        };
        let threads = self.thread_scene(Some(t.tc.session_id), message, true);
        let category = self.category_scene(
            Some(t.tc.session_id),
            place == theseus_memory::Place::Private,
            threads.as_ref(),
        );
        // The prompt's tier sizes the pack (theseus-d9je7); an assembled
        // section keeps its own budget.
        let (budget_tokens, max_items) = match t.recall.assembled {
            true => (Some(self.memory.cfg().assembled_budget_tokens), None),
            false => t
                .route
                .injection
                .as_ref()
                .map(|b| b.pack(&self.cfg))
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
            activation: None,
            threads,
            persona: self.persona_scene(t.target.persona.as_deref()),
            category,
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
            // A range a split moved is out of the compile, so recall may bring it back.
            let moves = crate::moved::read(&nodes);
            for (_, n) in nodes
                .iter()
                .filter(|(p, n)| *p > after && !moves.hidden.contains(&n.id))
            {
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
        let scene = self.scene(t, "shadow", MemoryArm::Baseline, &begun.said);
        let draft = t.tc.draft.is_some();
        let (mut m, candidates, links) = self.memory.manifest_with(
            &scene,
            &begun,
            answer,
            |s| self.drawn_place(draft, s),
            |ids| crate::recall::links(&self.store, ids),
            false,
        );
        self.memory.noted(&m);
        // What the rerank reads of the scene (32c), taken now: the rest of
        // the scene borrows the turn, which the row and the mark need.
        let retention = self.memory.retention_of(&*scene.science, &candidates);
        let lens = scene.lens(&candidates);
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
                threads: lens,
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
        // The tier, if the verdict has landed; ordinary if not: never a wait.
        self.injection(t, true);
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
        self.memory.noted(&m);
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
    /// no recall (`compile_detour` reads no `recall_view` for it), so its node
    /// rides nothing, the reply's footer counts none, and its row says
    /// `detoured`; a quick detour's carried it (theseus-pa1xx), which stays.
    /// Then the rows held while routing decided are recorded.
    pub(super) fn recall_routed(t: &mut Turn<'_>) {
        if t.route.keeps.is_some() && !t.route.detour_recall {
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
        if !self.memory.on() || self.restricted_profile(&t.target.profile) {
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
        let ids: Vec<&str> = m.admitted.iter().map(|i| i.node_id.as_str()).collect();
        let handles = theseus_protocol::cite::handles_for(&ids);
        let threads = self.thread_scene(Some(t.tc.session_id), "", false);
        let persona = self.persona_scene(t.target.persona.as_deref());
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
            let place = self.place_name(&item.session_id);
            let session = self
                .store
                .get_session::<crate::session::SessionRecord>(&item.session_id)
                .ok()
                .flatten();
            let mut header =
                render::item_header(&source, item.position, &place, chunk, session.as_ref());
            // A node a split moved is credited to the thread it moved to (theseus-q9b85).
            let moved = self.moved_title(&item.node_id, threads.as_ref());
            header = crate::recall::neighbour::with_note(header, item);
            if let Some(title) = moved.or_else(|| {
                threads
                    .as_ref()
                    .and_then(|th| th.other_title(&item.session_id))
            }) {
                header = format!(
                    "{} {header}",
                    render::thread_mark(&title, &source, item.position)
                );
            }
            header = crate::recall::current::with_mark(header, item);
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
        // The one point recalled notes join a request, for every recall path
        // (theseus-39dj0): a note of a private session makes the asker private.
        rides.extend(self.follow_private(t, items));
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

    /// The title of the thread a split moved `node_id` to, when it has one
    /// (theseus-q9b85).
    fn moved_title(
        &self,
        node_id: &str,
        threads: Option<&crate::thread_recall::ThreadScene>,
    ) -> Option<String> {
        self.store
            .get_meta::<Option<crate::moved::MovedTo>>(&crate::moved::moved_of_key(node_id))
            .ok()
            .flatten()
            .flatten()
            .and_then(|m| threads.and_then(|th| th.other_title_of(&m.thread)))
    }

    /// The asker's Google `private` mark when `items` hold a note of a
    /// session that holds it (`google::private::followed`); the turn's own
    /// calls are private from now on.
    fn follow_private(&self, t: &mut Turn<'_>, items: &[RecalledRef]) -> Vec<NewRecord> {
        let recalled = items
            .iter()
            .map(|r| (r.node_id.as_str(), r.session_id.as_str()));
        let mut mark = crate::google::private::followed(
            t.tc.store,
            &self.cfg.google,
            t.tc.session_id,
            recalled,
        );
        // The place digest's mark rides the same frame (theseus-yso6p): a key
        // it already writes is not written twice, and a level it lacks (a
        // recalled sensitive note under a private digest) is still added.
        mark.retain(|r| !t.digest.mark.iter().any(|d| d.key == r.key));
        t.recall.private |= !mark.is_empty();
        mark
    }

    /// The transcript a loop compiles, with the turn's pending `Recall` node
    /// after every node, and the sources its recalls render. A request bound
    /// for a model of restricted origin (`profile`) carries no `Recall` node
    /// at all: a note is drawn from other sessions, whose labels are not this
    /// one's, so what it holds is not this session's to send there
    /// (theseus-9ymz0, the review's B1). The notes stay in the transcript for
    /// the next request that may carry them.
    pub(super) fn recall_view(
        &self,
        t: &Turn<'_>,
        nodes: Transcript,
        profile: &str,
    ) -> (Transcript, Sources) {
        let mut nodes = nodes;
        if self.restricted_profile(profile) {
            nodes.retain(|(_, n)| n.kind != crate::stub::Kind::Recall);
            return (nodes, Sources::new());
        }
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
