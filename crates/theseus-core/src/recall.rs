//! Recall (M6 step 30a; design §2.4), in shadow: on a turn's first loop, when
//! the turn brings something new (its input, a wake, a task's report), ask the
//! index what past nodes bear on it, filter and pack what it answers
//! (`theseus_memory::recall`, the place rule first), and record what would
//! have been admitted as a `recall.shadow` row (`fact::recall`). Nothing of it
//! reaches the model, and it writes no frame: the row rides in the turn's
//! next one.
//!
//! - **Never in the turn's way.** The index is asked in a task of its own as
//!   the loop's model call begins, under `[memory] recall_deadline_ms`, and the
//!   turn reads the answer once its call has answered: a stalled or absent
//!   index costs the turn at most the deadline past its call, and the row says
//!   `deadline` or `unavailable`.
//! - **The place rule.** A candidate's place is its session's, read as the
//!   turn's own is (`TurnRunner::place_of`, `class_of`'s rule): its target,
//!   or where its wakes answer; a place that cannot be read is no place's.
//! - `memory.search` runs the same pipeline for a query, writing nothing, and
//!   `memory.recalls` reads a session's rows (`rpc/memory.rs`).
//!
//! **In front of the model (step 30b).** In `canary` (a sticky share of
//! sessions, `[memory] canary_fraction`) and `live`, the read finishes before
//! the first loop's compile, under the same deadline, and what it admits is
//! a `Recall` node (`node::Body::Recall`, references and never copies) with
//! a `derived_from` edge to each source (`via = "recall"`), which ride the
//! provider call's plan frame with the `recall.ran` row: no frame of their
//! own. It renders after the new message, read from each source by position
//! (`render`). A session's recall notes in its tail are capped
//! (`session_recall_cap_tokens`): past it recall pauses until the next
//! recompile. A control session runs `none` live with `baseline` in shadow.
//! The operator's labels (`labels`) keep a node out as `labeled_wrong`.

pub mod activation;
pub mod adjacency;
pub mod labels;
pub mod render;
pub mod retention;

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use theseus_memory::recall::{self as pipeline, Asker, Candidate, Link, LinkKind, Pack, Place};
use theseus_memory::{Activated, Baseline, MemoryScience, Retention, RetentionRank};
use theseus_protocol::index::{IndexQueryParams, IndexQueryResult};
use theseus_protocol::memory::{
    BudgetDrop, BudgetReport, RecallDrop, RecallItem, RecallManifest, RecallRetention,
    RecallTimings,
};

use crate::config::memory::MemoryArm;
use crate::config::MemoryConfig;
use crate::node::{AttachmentContent, Body, Node};
use crate::store::Transcript;
use crate::tender::IndexTender;
use crate::turn::TurnRunner;

/// The index's hits a recall asks for (§2.4).
pub const K: usize = 40;
/// How much of the previous reply joins the query, so that "yes, do that"
/// still has a subject.
pub const REPLY_CHARS: usize = 500;

/// The index's answer to one query, or why there is none.
pub type AskFuture = Pin<Box<dyn Future<Output = Result<IndexQueryResult, String>> + Send>>;
/// Who answers a recall's query: the index tender, or a test's stand-in.
pub type Ask = Arc<dyn Fn(IndexQueryParams) -> AskFuture + Send + Sync>;

/// Recall's settings, its science, and the index it asks.
pub struct Memory {
    cfg: MemoryConfig,
    science: Baseline,
    /// The `+activation` arm's science (32b).
    activated: Arc<Activated>,
    /// The adjacency projection it spreads over, built after serving.
    pub(crate) adjacency: Arc<activation::Adjacent>,
    ask: RwLock<Option<Ask>>,
    /// The nodes the operator labeled wrong or stale, once read (`labels`).
    labels: RwLock<Option<BTreeSet<String>>>,
    /// The sessions whose `memory.arm` row this daemon has seen or written.
    armed: Mutex<BTreeSet<String>>,
    /// Sources a `Recall` node rendered, by id: they never change.
    sources: Mutex<render::Sources>,
    /// Each node's FSRS-6 retention (32a), once something reads it.
    retention: retention::Projection,
    /// Where the projection's size is measured, once telemetry is built.
    telemetry: std::sync::OnceLock<crate::telemetry::Telemetry>,
    /// The memory's harness session and its checked syntheses (31b), once
    /// read (`syntheses`).
    syntheses: RwLock<Option<Syntheses>>,
}

/// The memory's harness session (31b), and the syntheses in it whose
/// citations were checked and supported: the `+synthesis` arm's candidates.
#[derive(Clone, Debug, Default)]
pub struct Syntheses {
    pub session: Option<String>,
    pub checked: Arc<BTreeSet<String>>,
}

impl Memory {
    /// Recall over `tender`'s index, as `cfg` says.
    pub fn new(cfg: MemoryConfig, tender: Option<Arc<IndexTender>>) -> Self {
        let ask = tender.map(|t| -> Ask {
            Arc::new(move |p: IndexQueryParams| -> AskFuture {
                let t = t.clone();
                Box::pin(async move { of_tender(&t, &p).await })
            })
        });
        Self {
            cfg,
            science: Baseline::default(),
            activated: Arc::new(Activated::default()),
            adjacency: Arc::new(activation::Adjacent::default()),
            ask: RwLock::new(ask),
            labels: RwLock::new(None),
            armed: Mutex::new(BTreeSet::new()),
            sources: Mutex::new(render::Sources::new()),
            retention: retention::Projection::default(),
            telemetry: std::sync::OnceLock::new(),
            syntheses: RwLock::new(None),
        }
    }

    /// Ask `ask` instead of the tender (tests: a stand-in index).
    pub fn set_ask(&self, ask: Ask) {
        *self.ask.write().unwrap_or_else(PoisonError::into_inner) = Some(ask);
    }

    pub fn cfg(&self) -> &MemoryConfig {
        &self.cfg
    }

    /// The sources of the `Recall` nodes in `nodes`, read by position
    /// (`render::read_sources`).
    pub fn read_sources<'a>(
        &self,
        store: &crate::store::Store,
        nodes: impl Iterator<Item = &'a Node>,
    ) -> render::Sources {
        render::read_sources(store, nodes, &self.sources)
    }

    /// One recalled item's source, read by position.
    pub fn read_source(
        &self,
        store: &crate::store::Store,
        r: &crate::node::RecalledRef,
    ) -> Option<Arc<Node>> {
        render::source(store, r, &self.sources)
    }

    /// Whether `session_id`'s `memory.arm` row is written: known to this
    /// daemon, or found among the session's recall rows.
    pub fn arm_recorded(
        &self,
        store: &crate::store::Store,
        session_id: &str,
    ) -> anyhow::Result<bool> {
        if self
            .armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(session_id)
        {
            return Ok(true);
        }
        let kind = theseus_protocol::LedgerKind::MemoryArm.as_str();
        for r in store.scope_after(&crate::fact::recall::scope(session_id), 0)? {
            if r.kind == theseus_store::kinds::LEDGER
                && r.decode::<crate::ledger::LedgerRow>()?.kind == kind
            {
                self.armed(session_id);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `session_id`'s `memory.arm` row is written.
    pub fn armed(&self, session_id: &str) {
        self.armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session_id.to_string());
    }

    /// Whether a turn recalls: `[memory] mode` is not `off`.
    pub fn on(&self) -> bool {
        self.cfg.on()
    }

    pub fn science(&self) -> &dyn MemoryScience {
        &self.science
    }

    /// The science, owned: for work after the turn's pipeline (the judge's
    /// rerank, step 32c).
    pub fn science_owned(&self) -> Arc<dyn MemoryScience> {
        Arc::new(self.science.clone())
    }

    /// An arm's science: `+retention`'s (32a), `+activation`'s (32b) and
    /// `+synthesis`'s (31b: `baseline`'s, admitting the checked syntheses) for
    /// theirs, `baseline`'s for every other. Shadow, a canary's control, and a
    /// search that names no arm run `baseline`. The arms' seam: each arm that
    /// ranks its own way adds its line here.
    pub fn science_for(&self, arm: MemoryArm) -> Arc<dyn MemoryScience> {
        match arm {
            MemoryArm::Retention => Arc::new(RetentionRank {
                base: self.science.clone(),
                fsrs: self.retention.fsrs().clone(),
                ..RetentionRank::default()
            }),
            MemoryArm::Activation => self.activated.clone(),
            MemoryArm::Synthesis => Arc::new(theseus_memory::WithSyntheses {
                base: self.science.clone(),
                checked: self.syntheses_known().checked,
            }),
            MemoryArm::None | MemoryArm::Bm25 | MemoryArm::Baseline => self.science_owned(),
        }
    }

    /// The retention of `candidates`' nodes for a science that reads it,
    /// and the projection's state in a word (`ready`, or why the rank went
    /// without: `building`, `unbuilt`, `failed`); none for one that does
    /// not.
    fn retention_for(
        &self,
        science: &dyn MemoryScience,
        candidates: &[Candidate],
    ) -> (BTreeMap<String, Retention>, Option<String>) {
        if !science.reads_retention() {
            return (BTreeMap::new(), None);
        }
        let phase = self.retention.phase();
        let map = match phase {
            retention::Phase::Ready => self
                .retention
                .of(candidates.iter().map(|c| c.node_id.as_str())),
            _ => BTreeMap::new(),
        };
        (map, Some(phase.word().to_string()))
    }

    /// The harness session and its checked syntheses, read once from the
    /// store (the session by its META key, then its nodes).
    pub fn syntheses(&self, store: &crate::store::Store) -> Syntheses {
        if let Some(s) = self
            .syntheses
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return s;
        }
        let read = || -> anyhow::Result<Syntheses> {
            let Some(session) =
                store.get_meta::<String>(crate::consolidate::run::MEMORY_SESSION)?
            else {
                return Ok(Syntheses::default());
            };
            let checked = store
                .session_nodes(&session)?
                .into_iter()
                .filter(
                    |(_, n)| matches!(&n.body, Body::Synthesis { check, .. } if check.checked()),
                )
                .map(|(_, n)| n.id)
                .collect();
            Ok(Syntheses {
                session: Some(session),
                checked: Arc::new(checked),
            })
        };
        match read() {
            Ok(s) => {
                *self
                    .syntheses
                    .write()
                    .unwrap_or_else(PoisonError::into_inner) = Some(s.clone());
                s
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "recall: the syntheses cannot be read; none is a candidate");
                Syntheses::default()
            }
        }
    }

    /// What is known of the syntheses without a read.
    fn syntheses_known(&self) -> Syntheses {
        self.syntheses
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_default()
    }

    /// Consolidation kept a synthesis in `session`; `checked`, when its
    /// citations were supported.
    pub fn kept_synthesis(&self, session: &str, id: &str, checked: bool) {
        let mut w = self
            .syntheses
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(s) = w.as_mut() else {
            // Not read yet: the first read finds it.
            return;
        };
        s.session = Some(session.to_string());
        if checked {
            Arc::make_mut(&mut s.checked).insert(id.to_string());
        }
    }

    /// Ask the index for `query`'s hits as of `as_of` from `arm`'s sources
    /// (`MemoryArm::sources`), in a task of its own, bounded by `deadline`.
    /// Every arm but `+synthesis` leaves the memory's harness session out
    /// before the index's top k (`exclude_sessions`).
    pub fn begin(
        &self,
        query: String,
        as_of: Option<u64>,
        k: usize,
        arm: MemoryArm,
        deadline: Duration,
    ) -> Begun {
        let sources = arm.sources();
        let exclude: Vec<String> = match arm {
            MemoryArm::Synthesis => Vec::new(),
            MemoryArm::None
            | MemoryArm::Bm25
            | MemoryArm::Baseline
            | MemoryArm::Retention
            | MemoryArm::Activation => self.syntheses_known().session.into_iter().collect(),
        };
        let ask = self
            .ask
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut p = IndexQueryParams::new(&query);
        p.k = k.clamp(1, 100);
        p.as_of = as_of;
        p.sources = sources.iter().map(|s| s.to_string()).collect();
        p.exclude_sessions = exclude;
        let started = Instant::now();
        let task = tokio::spawn(async move {
            let answer = match ask {
                None => Answer::Unavailable("no index is configured".into()),
                Some(ask) => match tokio::time::timeout(deadline, ask(p)).await {
                    Ok(Ok(r)) => Answer::Hits(r),
                    Ok(Err(why)) => Answer::Unavailable(why),
                    Err(_) => Answer::Deadline,
                },
            };
            (answer, started.elapsed())
        });
        Begun {
            task,
            started,
            query,
            as_of,
            deadline,
            new_node: None,
        }
    }
}

/// The tender's answer to a recall: asked only while it runs, so a recall
/// never waits on a socket no tender serves.
async fn of_tender(t: &IndexTender, p: &IndexQueryParams) -> Result<IndexQueryResult, String> {
    match t.status() {
        None => Err("the index is off: [index] enabled = false".into()),
        Some(s) if s.state != "running" => Err(format!(
            "the index tender is {}{}",
            s.state,
            s.why.map(|w| format!(": {w}")).unwrap_or_default()
        )),
        Some(_) => t.query(p).await.map_err(|(_, why)| why),
    }
}

/// What the index made of a recall's query.
pub enum Answer {
    Hits(IndexQueryResult),
    /// None within the deadline.
    Deadline,
    /// No index answered, and why.
    Unavailable(String),
}

/// A recall whose query is out.
pub struct Begun {
    task: tokio::task::JoinHandle<(Answer, Duration)>,
    pub started: Instant,
    pub query: String,
    pub as_of: Option<u64>,
    pub deadline: Duration,
    /// The turn's new node, which seeds a spread at 1.0 (32b).
    pub new_node: Option<String>,
}

impl Begun {
    /// The index's answer, and how long it took; never past the deadline.
    pub async fn answer(&mut self) -> (Answer, Duration) {
        match (&mut self.task).await {
            Ok(a) => a,
            Err(e) => (
                Answer::Unavailable(format!("the recall's task ended: {e}")),
                self.started.elapsed(),
            ),
        }
    }
}

/// The turn, or the search, a recall is for.
pub struct Scene<'a> {
    pub mode: &'a str,
    pub session_id: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub place: Place,
    pub in_context: BTreeSet<String>,
    /// The nodes the operator labeled wrong or stale.
    pub labeled: BTreeSet<String>,
    /// The pack's tokens when not `[memory] recall_budget_tokens`: an
    /// assembled prefix's recall section (30c), `assembled_budget_tokens`.
    pub budget_tokens: Option<u64>,
    /// The turn's arm's science (`Memory::science_for`): `baseline` in
    /// shadow, for a canary's control, and for a search with no arm.
    pub science: Arc<dyn MemoryScience>,
    /// What spreading activation did before the pipeline (32b).
    pub activation: Option<theseus_protocol::memory::RecallActivation>,
}

impl Memory {
    /// The recall's manifest from the index's answer: the candidates' places
    /// (`place_of`, read once a session), the pipeline, and the pack. Its
    /// items carry their excerpts only when `texts` (a search's answer; a
    /// row keeps references, never copies).
    pub fn manifest(
        &self,
        scene: &Scene<'_>,
        begun: &Begun,
        answer: (Answer, Duration),
        place_of: impl Fn(&str) -> Place,
        links_of: impl Fn(&[String]) -> Vec<Link>,
        texts: bool,
    ) -> RecallManifest {
        self.manifest_with(scene, begun, answer, place_of, links_of, texts)
            .0
    }

    /// The manifest, the candidates its pipeline read (with their places),
    /// and the memory pass's links among them (31a), for the judge's rerank
    /// after it (step 32c); none when the index gave no hits.
    pub fn manifest_with(
        &self,
        scene: &Scene<'_>,
        begun: &Begun,
        answer: (Answer, Duration),
        place_of: impl Fn(&str) -> Place,
        links_of: impl Fn(&[String]) -> Vec<Link>,
        texts: bool,
    ) -> (RecallManifest, Vec<Candidate>, Vec<Link>) {
        let (m, candidates, links, _) =
            self.manifest_ranked(scene, begun, answer, place_of, links_of, texts);
        (m, candidates, links)
    }

    /// The manifest, the candidates, the memory pass's links among them
    /// (31a), and each candidate's ranks by source, for a pack again in
    /// Jev's order (`refill`, 32d).
    pub fn manifest_ranked(
        &self,
        scene: &Scene<'_>,
        begun: &Begun,
        (answer, index_took): (Answer, Duration),
        place_of: impl Fn(&str) -> Place,
        links_of: impl Fn(&[String]) -> Vec<Link>,
        texts: bool,
    ) -> (RecallManifest, Vec<Candidate>, Vec<Link>, Ranks) {
        let mut m = RecallManifest {
            recall_id: crate::new_id("rcl"),
            mode: scene.mode.into(),
            science: scene.science.id().to_string(),
            activation: scene.activation.clone(),
            outcome: "ran".into(),
            session_id: scene.session_id.map(str::to_string),
            turn_id: scene.turn_id.map(str::to_string),
            place: place_words(&scene.place),
            query_chars: begun.query.chars().count() as u64,
            query_digest: hex::encode(&Sha256::digest(begun.query.as_bytes())[..8]),
            as_of: begun.as_of,
            budget_tokens: scene.budget_tokens.unwrap_or(self.cfg.recall_budget_tokens),
            timings: RecallTimings {
                index_ms: ms(index_took),
                deadline_ms: begun.deadline.as_millis() as u64,
                ..RecallTimings::default()
            },
            ..RecallManifest::default()
        };
        let r = match answer {
            Answer::Hits(r) => r,
            Answer::Deadline => {
                m.outcome = "deadline".into();
                m.timings.total_ms = ms(begun.started.elapsed());
                return (m, Vec::new(), Vec::new(), Ranks::new());
            }
            Answer::Unavailable(why) => {
                m.outcome = "unavailable".into();
                m.why = Some(why);
                m.timings.total_ms = ms(begun.started.elapsed());
                return (m, Vec::new(), Vec::new(), Ranks::new());
            }
        };
        let t0 = Instant::now();
        m.indexed_through = Some(r.indexed_through);
        m.candidates = r.hits.len() as u64;
        m.skipped = r.skipped;
        m.timings.index = Some(r.timings);
        let mut sources: BTreeMap<String, u64> = BTreeMap::new();
        let mut places: BTreeMap<String, Place> = BTreeMap::new();
        let mut ranks = BTreeMap::new();
        let candidates: Vec<Candidate> = r
            .hits
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                for s in h.sources.keys() {
                    *sources.entry(s.clone()).or_default() += 1;
                }
                let place = places
                    .entry(h.session_id.clone())
                    .or_insert_with(|| place_of(&h.session_id))
                    .clone();
                ranks.insert(format!("{}#{}", h.node_id, h.chunk), h.sources);
                Candidate {
                    node_id: h.node_id,
                    chunk: h.chunk,
                    session_id: h.session_id,
                    position: h.position,
                    kind: h.kind,
                    origin: h.origin,
                    external: h.external,
                    text: h.text,
                    fused: h.fused,
                    index_rank: i + 1,
                    place,
                }
            })
            .collect();
        m.sources = sources;
        // The memory pass's edges among them, for a science that reads them.
        let links = if scene.science.prefers_newer() {
            let ids: BTreeSet<String> = candidates.iter().map(|c| c.node_id.clone()).collect();
            links_of(&ids.into_iter().collect::<Vec<_>>())
        } else {
            Vec::new()
        };
        let (retention, state) = self.retention_for(&*scene.science, &candidates);
        m.retention = state;
        let asker = Asker {
            session_id: scene.session_id.unwrap_or_default(),
            place: &scene.place,
            in_context: &scene.in_context,
            labeled: &scene.labeled,
            links: &links,
            now_ms: theseus_protocol::now_unix_ms(),
            retention: &retention,
        };
        let params = theseus_memory::Params {
            budget_tokens: m.budget_tokens,
            ..self.cfg.params()
        };
        let pack = pipeline::recall(&*scene.science, &asker, candidates.clone(), &params);
        let kept = ranks.clone();
        fill(&mut m, pack, &mut ranks, texts);
        self.retention_items(&*scene.science, &mut m, &retention, asker.now_ms);
        m.timings.pack_ms = ms(t0.elapsed());
        m.timings.total_ms = ms(begun.started.elapsed());
        (m, candidates, links, kept)
    }

    /// The pack again, ranked by `order` (Jev's, from a live rerank, 32d):
    /// the same filters, the place rule first, the memory pass's links, and
    /// the same budget, into `m`'s items, drops, and budget.
    pub fn refill(
        &self,
        scene: &Scene<'_>,
        m: &mut RecallManifest,
        candidates: Vec<Candidate>,
        links: &[Link],
        mut ranks: Ranks,
        order: Vec<String>,
    ) {
        let (retention, _) = self.retention_for(&*scene.science, &candidates);
        let asker = Asker {
            session_id: scene.session_id.unwrap_or_default(),
            place: &scene.place,
            in_context: &scene.in_context,
            labeled: &scene.labeled,
            links,
            now_ms: theseus_protocol::now_unix_ms(),
            retention: &retention,
        };
        let params = self.params_of(m);
        let pack =
            theseus_memory::rerank::repack(&*scene.science, &asker, candidates, &params, order);
        m.drops.clear();
        fill(m, pack, &mut ranks, true);
        self.retention_items(&*scene.science, m, &retention, asker.now_ms);
    }

    /// The retention of `candidates`' nodes as the turn's science reads it:
    /// for the judge's rerank, whose repack ranks as the recall did.
    pub fn retention_of(
        &self,
        science: &dyn MemoryScience,
        candidates: &[Candidate],
    ) -> BTreeMap<String, Retention> {
        self.retention_for(science, candidates).0
    }

    /// Under a science that reads retention, each admitted and dropped
    /// item's: its retrievability at the turn's time, stability, difficulty
    /// and last review (none for a node without one).
    fn retention_items(
        &self,
        science: &dyn MemoryScience,
        m: &mut RecallManifest,
        retention: &BTreeMap<String, Retention>,
        now_ms: u64,
    ) {
        if !science.reads_retention() {
            return;
        }
        let fsrs = self.retention.fsrs();
        let of = |node: &str| {
            retention.get(node).map(|r| RecallRetention {
                retrievability: fsrs.retrievability_at(r, now_ms),
                stability: r.stability,
                difficulty: r.difficulty,
                last_review_ms: r.last_review_ms,
            })
        };
        for item in &mut m.admitted {
            item.retention = of(&item.node_id);
        }
        for drop in &mut m.dropped {
            drop.retention = of(&drop.node_id);
        }
    }

    /// The pack's limits for a manifest's budget.
    pub fn params_of(&self, m: &RecallManifest) -> theseus_memory::Params {
        theseus_memory::Params {
            budget_tokens: m.budget_tokens,
            ..self.cfg.params()
        }
    }
}

/// Each candidate's ranks by source, by key (`<node>#<chunk>`).
pub type Ranks = BTreeMap<String, BTreeMap<String, theseus_protocol::index::IndexSourceRank>>;

/// The pack's items and drops, into the manifest.
fn fill(m: &mut RecallManifest, pack: Pack, ranks: &mut Ranks, texts: bool) {
    m.used_tokens = pack.tokens;
    m.budget = Some(BudgetReport {
        limit_tokens: m.budget_tokens,
        used_tokens: pack.tokens,
        dropped: pack
            .dropped
            .iter()
            .filter(|d| d.reason == pipeline::Reason::Budget)
            .map(|d| BudgetDrop {
                node_id: Some(d.candidate.node_id.clone()),
                range: None,
                reason: d.reason.as_str().into(),
                tokens: d.tokens,
                tier: "recall".into(),
            })
            .collect(),
        overage: None,
    });
    for d in &pack.dropped {
        *m.drops.entry(d.reason.as_str().into()).or_default() += 1;
    }
    m.admitted = pack
        .admitted
        .into_iter()
        .map(|a| {
            let key = a.candidate.key();
            RecallItem {
                sources: ranks.remove(&key).unwrap_or_default(),
                node_id: a.candidate.node_id,
                chunk: a.candidate.chunk,
                session_id: a.candidate.session_id,
                position: a.candidate.position,
                kind: a.candidate.kind,
                rank: a.rank as u64,
                fused: a.candidate.fused,
                tokens: a.tokens,
                text: texts.then_some(a.excerpt),
                retention: None,
            }
        })
        .collect();
    m.dropped = pack
        .dropped
        .into_iter()
        .map(|d| RecallDrop {
            node_id: d.candidate.node_id,
            chunk: d.candidate.chunk,
            session_id: d.candidate.session_id,
            reason: d.reason.as_str().into(),
            fused: d.candidate.fused,
            tokens: d.tokens,
            retention: None,
        })
        .collect();
    // Activation's share of what was admitted (32b).
    if let Some(a) = &mut m.activation {
        let ranked = |i: &&RecallItem| i.sources.contains_key(activation::SOURCE);
        a.admitted = m.admitted.iter().filter(ranked).count() as u64;
        a.admitted_added = m
            .admitted
            .iter()
            .filter(ranked)
            .filter(|i| i.sources.len() == 1)
            .count() as u64;
    }
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

/// The memory pass's edges into `ids` (M6 31a; the scope `in:<id>`), as
/// recall reads them: `same_entity` and `supersedes`, from the newer node to
/// the older. An edge that does not read is left out, and said.
pub fn links(store: &crate::store::Store, ids: &[String]) -> Vec<Link> {
    use crate::graph::{Edge, EdgeKind};
    let mut out = Vec::new();
    for id in ids {
        let records = match store.scope_after(&Edge::scope_into(id), 0) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(node_id = %id, error = %format!("{e:#}"), "recall: the edges into a node cannot be read; none is applied");
                continue;
            }
        };
        for r in records {
            if r.kind != theseus_store::kinds::EDGE {
                continue;
            }
            let Ok(e) = r.decode::<Edge>() else { continue };
            let kind = match EdgeKind::named(&e.kind) {
                Some(EdgeKind::SameEntity) => LinkKind::SameEntity,
                Some(EdgeKind::Supersedes) => LinkKind::Supersedes,
                Some(EdgeKind::DerivedFrom) | None => continue,
            };
            out.push(Link {
                kind,
                newer: e.from,
                older: e.to,
            });
        }
    }
    out
}

/// `private`, or `shared:<target>`.
pub fn place_words(p: &Place) -> String {
    match p {
        Place::Private => "private".into(),
        Place::Shared(t) => format!("shared:{t}"),
        Place::Unknown => "unknown".into(),
    }
}

/// The query of a turn whose new nodes (written by `turn_id`: its input, its
/// wakes, its reports) are in `nodes`: their texts and their files' names,
/// then the start of the reply before them. `None`: the turn brings nothing
/// new. With it, the position recall reads as of: the first new node's.
pub fn query_of(nodes: &Transcript, turn_id: &str) -> Option<(String, u64)> {
    let first = nodes.iter().position(|(_, n)| {
        n.turn_id.as_deref() == Some(turn_id) && matches!(n.body, Body::UserMessage { .. })
    })?;
    let mut parts = Vec::new();
    for (_, n) in &nodes[first..] {
        if n.turn_id.as_deref() != Some(turn_id) {
            continue;
        }
        if let Body::UserMessage { text, attachments } = &n.body {
            parts.push(text.clone());
            parts.extend(attachments.iter().map(|a| a.name.clone()));
        }
    }
    let reply = nodes[..first]
        .iter()
        .rev()
        .find_map(|(_, n)| match &n.body {
            Body::AssistantMessage { blocks, .. } => Some(crate::provider::text_of(blocks)),
            _ => None,
        });
    if let Some(r) = reply.filter(|r| !r.trim().is_empty()) {
        parts.push(r.chars().take(REPLY_CHARS).collect());
    }
    let query = parts.join("\n");
    (!query.trim().is_empty()).then(|| (query, nodes[first].0))
}

/// A node's text, as the index reads it: for `memory.recalls`'s excerpts.
pub fn text_of(n: &Node) -> String {
    match &n.body {
        Body::UserMessage { text, attachments } => {
            let mut t = text.clone();
            for a in attachments {
                if let AttachmentContent::Text { text, .. } = &a.content {
                    t.push('\n');
                    t.push_str(text);
                }
            }
            t
        }
        Body::AssistantMessage { blocks, .. } => crate::provider::text_of(blocks),
        Body::ToolCall { tool, input, .. } => format!("{tool} {input}"),
        Body::ToolResult { content, .. } => content.clone(),
        // A recall's text is its sources': it is never recalled again.
        Body::Recall { .. } => String::new(),
        Body::Arrangement { pieces, claim, .. } => crate::check::render(pieces, claim.as_ref()),
        // A summary's text is the model's account of its range.
        Body::Summary { text, .. } => text.clone(),
        // A synthesis's text is its cited entry (31b).
        Body::Synthesis { text, .. } => text.clone(),
        // The operator's past history (theseus-0lrr.6), and its summaries.
        Body::Imported { text, .. } | Body::ImportedSummary { text, .. } => text.clone(),
        // A tombstone has no text: its payload is gone.
        Body::Erased { .. } => String::new(),
    }
}

impl TurnRunner {
    /// Where `session_id` speaks, by name, for a recalled item's header (35a):
    /// its bound place's name (`#harbor`, `DM @eddie`), its target when no
    /// binding names it, or the session itself on the CLI or the web UI. A
    /// shared place recalls only its own sessions (the place rule), so its
    /// headers name no other place.
    pub fn place_name(&self, session_id: &str) -> String {
        if crate::import::is_imported(session_id) {
            return crate::import::place_name(&self.store, session_id);
        }
        let target = self.outbox.try_target(session_id).and_then(|t| match t {
            Some(t) => Ok(Some(t)),
            None => self.outbox.try_wake_target(session_id),
        });
        let places = crate::places::Places {
            rule: &self.place_rule,
            cfg: &self.cfg,
        };
        match target {
            Ok(Some(t)) => places.name_of(Some(&t)),
            Ok(None) => crate::recall::render::unplaced(session_id),
            Err(_) => session_id.to_string(),
        }
    }

    /// Where `session_id` speaks, as the place rule reads a turn's (`class_of`):
    /// its target, or where its wakes and reports answer; `Unknown` when that
    /// cannot be read.
    pub fn place_of(&self, session_id: &str) -> Place {
        // An imported session is the owner's own history: private, whatever
        // place its episode names (theseus-0lrr.6).
        if crate::import::is_imported(session_id) {
            return Place::Private;
        }
        let target = self.outbox.try_target(session_id).and_then(|t| match t {
            Some(t) => Ok(Some(t)),
            None => self.outbox.try_wake_target(session_id),
        });
        match target {
            Ok(t) => match self.place_rule.class(&self.cfg, t.as_deref()) {
                theseus_protocol::PlaceClass::Private => Place::Private,
                theseus_protocol::PlaceClass::Shared => t.map_or(Place::Unknown, Place::Shared),
            },
            Err(e) => {
                tracing::warn!(session_id, error = %format!("{e:#}"),
                    "where a session speaks cannot be read: recall draws on none of it");
                Place::Unknown
            }
        }
    }
}
