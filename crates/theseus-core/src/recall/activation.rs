//! Spreading activation as recall's ranked source (M6 step 32b's wire-in;
//! design §2.4, §2.7): the `+activation` arm, between the index's answer and
//! the pipeline.
//!
//! - **The projection** (`adjacency`) is built after serving on the blocking
//!   pool, when `[memory] arm = "+activation"` puts it in front of the model
//!   (`Core::warm_activation`), or by the first search that names the arm.
//!   A turn that finds it unbuilt starts the build and goes on without
//!   activation (`building`); it never waits for it. A search builds it
//!   itself, unpaced, unless the warm build is running: then it answers
//!   `building` at once too, never queued behind the warm build's paces.
//! - **Seeds**: the turn's new node at 1.0, its edges the projection's (its
//!   neighbour; its own entities once the memory pass labels it, after the
//!   turn) and the entities of the query that the index's hits matched (the
//!   tender is asked nothing more), and the top fused hits at their scores
//!   over the best one's. A search has no new node: its query seeds with
//!   those entities alone.
//! - **One more ranked source.** The reached nodes, strongest first, are a
//!   list in the fusion: each hit of a reached node gains the science's term
//!   (`Activated::term`, the tender's weighted reciprocal rank), and the
//!   strongest the index did not return (at most `Activated::adds`, none the
//!   turn already holds, none written after the recall's `as_of`) join the
//!   candidates, read from the store, before every filter: the place rule
//!   reads their places as it reads any candidate's.
//! - **Inside recall's deadline.** The refresh and the spread run on the
//!   blocking pool, waited on for what is left of the index's deadline; past
//!   it the pipeline goes on with the index's answer alone (`deadline`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use theseus_memory::{Activated, MemoryScience};
use theseus_protocol::index::{IndexHit, IndexSourceRank};
use theseus_protocol::memory::RecallActivation;

use super::adjacency::{Projection, Stats};
use super::{ms, text_of, Answer, Begun, Memory};
use crate::config::memory::MemoryArm;
use crate::node::{Body, Node, Origin};
use crate::store::Store;

/// The source's name in a hit's and an item's `sources`.
pub const SOURCE: &str = "activation";
/// A search's seed: its query, which no node is.
pub const QUERY: &str = "?query";

/// The projection, and whether it is built.
#[derive(Default)]
pub struct Adjacent {
    projection: Mutex<Option<Projection>>,
    built: AtomicBool,
    building: AtomicBool,
    /// The paces its builds have taken, every build's added: a search's own
    /// build takes none (theseus-e21m).
    paces: AtomicU64,
    /// Its size after its last fold, for health without the lock.
    stats: Mutex<Option<Stats>>,
    /// Why its last build or refresh failed.
    error: Mutex<Option<String>>,
}

impl Adjacent {
    /// Its size after its last fold; `None` until it is built.
    pub fn stats(&self) -> Option<Stats> {
        *self.stats.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn built(&self) -> bool {
        self.built.load(Ordering::Acquire)
    }

    pub fn building(&self) -> bool {
        self.building.load(Ordering::Acquire)
    }

    /// The paces its builds have taken: the warm build's, between its pages.
    pub fn paces(&self) -> u64 {
        self.paces.load(Ordering::Acquire)
    }

    pub fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Build it now, unless it is built: for the blocking pool. A `paced`
    /// build (the warm one, after serving) waits between its pages while the
    /// machine is busy; a search's own build, which a person waits on inside
    /// recall's deadline, never does.
    pub fn build(&self, store: &Store, paced: bool) -> anyhow::Result<()> {
        let mut p = self
            .projection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if p.is_none() {
            let t0 = Instant::now();
            let (mut waited, mut paces) = (Duration::ZERO, 0u64);
            let built = theseus_store::blocking(|| {
                Projection::build_paced(store, &mut || {
                    if paced {
                        // Counted before it waits, so a pace still waiting
                        // is seen.
                        paces += 1;
                        self.paces.fetch_add(1, Ordering::AcqRel);
                        waited += super::adjacency::pace();
                    }
                })
            });
            let built = match built {
                Ok(b) => b,
                Err(e) => {
                    *self.error.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(format!("{e:#}"));
                    return Err(e);
                }
            };
            let st = built.stats();
            tracing::info!(
                nodes = st.nodes,
                edges = st.edges,
                entities = st.entities,
                bytes = st.bytes,
                took_ms = t0.elapsed().as_millis() as u64,
                waited_ms = waited.as_millis() as u64,
                paces,
                "memory: the adjacency projection is built"
            );
            *self.stats.lock().unwrap_or_else(PoisonError::into_inner) = Some(st);
            *p = Some(built);
            self.built.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Start its build on the blocking pool, unless it is built or being
    /// built.
    pub fn warm(self: &Arc<Self>, store: &Store) {
        if self.built() || self.building.swap(true, Ordering::AcqRel) {
            return;
        }
        let (me, store) = (self.clone(), store.clone());
        tokio::task::spawn_blocking(move || {
            if let Err(e) = me.build(&store, true) {
                tracing::warn!(error = %format!("{e:#}"),
                    "memory: the adjacency projection cannot be built; +activation spreads nothing");
            }
            me.building.store(false, Ordering::Release);
        });
    }
}

/// Why a search answers `building`: the warm build holds the projection.
const WARM: &str = "the adjacency projection's warm build is running, paced by the machine's \
pressure; a search does not wait for it";

/// What a spread found, on the blocking pool.
struct Spread {
    reached: Vec<(String, f32)>,
    added: Vec<IndexHit>,
    stats: Stats,
}

/// A spread's question, owned for the blocking pool.
struct Ask {
    adjacent: Arc<Adjacent>,
    science: Arc<Activated>,
    store: Store,
    seed: String,
    entities: Vec<String>,
    seeds: Vec<(String, f32)>,
    /// The index's hits and the turn's own nodes: never added.
    exclude: BTreeSet<String>,
    as_of: Option<u64>,
    /// Build the projection here if it is not (a search).
    build: bool,
}

impl Ask {
    fn run(self) -> Result<Spread, String> {
        if !self.adjacent.built() {
            if !self.build {
                return Err("building".into());
            }
            // The warm build holds the projection's lock for its whole walk,
            // its paces included, so a search that took the lock after it
            // would wait out a busy machine past its deadline (theseus-6fn.14):
            // it answers at once instead, as a turn does. `building` is set
            // before the warm build takes the lock, and only the warm build
            // sets it: a turn's refresh, or another search's own build, holds
            // the lock only as long as an unpaced fold takes, and is waited on.
            if self.adjacent.building() {
                return Err(WARM.into());
            }
            self.adjacent
                .build(&self.store, false)
                .map_err(|e| format!("{e:#}"))?;
        }
        let mut guard = self
            .adjacent
            .projection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(p) = guard.as_mut() else {
            return Err("building".into());
        };
        theseus_store::blocking(|| p.refresh(&self.store)).map_err(|e| format!("{e:#}"))?;
        let stats = p.stats();
        *self
            .adjacent
            .stats
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(stats);
        let sci = &self.science;
        let view = p.view(&sci.spread, Some(&self.seed), &self.entities);
        let reached = sci.activate(&view, &self.seeds, sci.spread.budget);
        let mut added = Vec::new();
        for (rank, (id, a)) in reached.iter().enumerate() {
            if added.len() >= sci.adds {
                break;
            }
            if self.exclude.contains(id) {
                continue;
            }
            let Some(position) = p.position(id) else {
                continue;
            };
            if self.as_of.is_some_and(|as_of| position >= as_of) {
                continue;
            }
            let node = match theseus_store::blocking(|| self.store.get_node(id)) {
                Ok(Some((_, n))) => n,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(node_id = %id, error = %format!("{e:#}"),
                        "recall: a node activation reached cannot be read; it is left out");
                    continue;
                }
            };
            if let Some(hit) = hit_of(&node, position, rank + 1, *a, sci) {
                added.push(hit);
            }
        }
        Ok(Spread {
            reached,
            added,
            stats,
        })
    }
}

/// A reached node as a candidate: what the index would give of it, its one
/// source activation. A node the index never indexes (a tool call, an
/// arrangement, a recall), a synthesis (31b: only `+synthesis` admits one),
/// or one with no text, is none.
fn hit_of(n: &Node, position: u64, rank: usize, a: f32, sci: &Activated) -> Option<IndexHit> {
    let (tool, external) = match &n.body {
        Body::UserMessage { .. } | Body::AssistantMessage { .. } | Body::Summary { .. } => {
            (None, false)
        }
        Body::ToolResult { tool, external, .. } => (Some(tool.clone()), external.is_some()),
        Body::Imported { integrity, .. } => (None, *integrity == crate::import::Integrity::Outside),
        Body::ImportedSummary { .. } => (None, false),
        Body::ToolCall { .. }
        | Body::Recall { .. }
        | Body::Arrangement { .. }
        | Body::Synthesis { .. }
        | Body::Erased { .. } => return None,
    };
    let text = text_of(n);
    if text.trim().is_empty() {
        return None;
    }
    let origin = match n.origin {
        Origin::Operator => "operator",
        Origin::Agent => "agent",
        Origin::Tool => "tool",
        Origin::Harness => "harness",
        Origin::Mcp => "mcp",
        Origin::Import => "import",
    };
    Some(IndexHit {
        node_id: n.id.clone(),
        chunk: 0,
        session_id: n.session_id.clone(),
        position,
        kind: n.kind_str().into(),
        origin: origin.into(),
        author: n.author.clone(),
        place: None,
        tool,
        time_ms: n.created_at_ms,
        external,
        text,
        entities_matched: Vec::new(),
        sources: [(
            SOURCE.to_string(),
            IndexSourceRank {
                rank,
                score: f64::from(a),
            },
        )]
        .into(),
        fused: sci.term(rank),
    })
}

/// The top `n` hits' nodes at their fused scores over the best one's, each
/// node once at its best chunk's.
fn seeds_of(hits: &[IndexHit], n: usize) -> Vec<(String, f32)> {
    let best = hits.iter().map(|h| h.fused).fold(0.0f64, f64::max);
    if best <= 0.0 || !best.is_finite() {
        return Vec::new();
    }
    let mut by_node: BTreeMap<&str, f64> = BTreeMap::new();
    for h in hits {
        let s = by_node.entry(&h.node_id).or_insert(h.fused);
        *s = s.max(h.fused);
    }
    let mut v: Vec<(&str, f64)> = by_node.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    v.truncate(n);
    v.into_iter()
        .map(|(k, s)| (k.to_string(), (s / best) as f32))
        .collect()
}

/// The spread into the index's hits: each reached hit's term and rank, and
/// the nodes it added after them.
fn apply(hits: &mut Vec<IndexHit>, spread: Spread, sci: &Activated, report: &mut RecallActivation) {
    let rank_of: HashMap<&str, (usize, f32)> = spread
        .reached
        .iter()
        .enumerate()
        .map(|(i, (k, a))| (k.as_str(), (i + 1, *a)))
        .collect();
    let mut boosted = BTreeSet::new();
    for h in hits.iter_mut() {
        if let Some(&(rank, a)) = rank_of.get(h.node_id.as_str()) {
            h.fused += sci.term(rank);
            h.sources.insert(
                SOURCE.to_string(),
                IndexSourceRank {
                    rank,
                    score: f64::from(a),
                },
            );
            boosted.insert(h.node_id.clone());
        }
    }
    report.reached = spread.reached.len() as u64;
    report.boosted = boosted.len() as u64;
    report.added = spread.added.len() as u64;
    report.nodes = spread.stats.nodes;
    report.edges = spread.stats.edges;
    hits.extend(spread.added);
}

impl Memory {
    /// Spreading activation over the index's answer, for an arm that reads
    /// it (`+activation`): each reached hit's term, and the nodes it adds;
    /// any other arm's answer comes back as it went in, with no report.
    /// `exclude` is what the turn already holds. A search (`build`) builds
    /// the projection if it must, within the deadline; a turn never waits
    /// for a build.
    pub async fn activated(
        &self,
        store: &Store,
        arm: MemoryArm,
        begun: &Begun,
        (answer, took): (Answer, Duration),
        exclude: BTreeSet<String>,
        build: bool,
    ) -> ((Answer, Duration), Option<RecallActivation>) {
        match arm {
            MemoryArm::Activation => {}
            MemoryArm::None
            | MemoryArm::Bm25
            | MemoryArm::Baseline
            | MemoryArm::Retention
            | MemoryArm::Synthesis => return ((answer, took), None),
        }
        let t0 = Instant::now();
        let mut report = RecallActivation {
            outcome: "ran".into(),
            ..RecallActivation::default()
        };
        let Answer::Hits(mut r) = answer else {
            report.outcome = "no_hits".into();
            return ((answer, took), Some(report));
        };
        if !build && !self.adjacency.built() {
            self.adjacency.warm(store);
            report.outcome = "building".into();
            report.why = Some("the adjacency projection is built after serving".into());
            return ((Answer::Hits(r), took), Some(report));
        }
        let sci = self.activated.clone();
        let seed = begun.new_node.clone().unwrap_or_else(|| QUERY.to_string());
        let mut seeds = vec![(seed.clone(), 1.0)];
        seeds.extend(seeds_of(&r.hits, sci.seeds));
        let entities: Vec<String> = r
            .hits
            .iter()
            .flat_map(|h| h.entities_matched.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        report.seeds = seeds.len() as u64;
        report.entities = entities.clone();
        let mut exclude = exclude;
        exclude.extend(r.hits.iter().map(|h| h.node_id.clone()));
        let ask = Ask {
            adjacent: self.adjacency.clone(),
            science: sci.clone(),
            store: store.clone(),
            seed,
            entities,
            seeds,
            exclude,
            as_of: begun.as_of,
            build,
        };
        let left = begun.deadline.saturating_sub(begun.started.elapsed());
        if left.is_zero() {
            report.outcome = "deadline".into();
            report.why = Some("the index's answer took all of recall's deadline".into());
            return ((Answer::Hits(r), took), Some(report));
        }
        let job = tokio::task::spawn_blocking(move || ask.run());
        let spread = match tokio::time::timeout(left, job).await {
            Ok(Ok(Ok(s))) => s,
            Ok(Ok(Err(why))) => {
                report.outcome = if why == "building" || why == WARM {
                    "building".into()
                } else {
                    "unavailable".into()
                };
                report.why = Some(why);
                report.took_ms = ms(t0.elapsed());
                return ((Answer::Hits(r), took), Some(report));
            }
            Ok(Err(e)) => {
                report.outcome = "unavailable".into();
                report.why = Some(format!("the spread's task ended: {e}"));
                report.took_ms = ms(t0.elapsed());
                return ((Answer::Hits(r), took), Some(report));
            }
            Err(_) => {
                report.outcome = "deadline".into();
                report.why = Some(format!(
                    "the spread did not finish in the {} ms left of recall's deadline",
                    left.as_millis()
                ));
                report.took_ms = ms(t0.elapsed());
                return ((Answer::Hits(r), took), Some(report));
            }
        };
        apply(&mut r.hits, spread, &sci, &mut report);
        report.took_ms = ms(t0.elapsed());
        ((Answer::Hits(r), took), Some(report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(node: &str, fused: f64) -> IndexHit {
        IndexHit {
            node_id: node.into(),
            chunk: 0,
            session_id: "ses_a".into(),
            position: 1,
            kind: "user_message".into(),
            origin: "operator".into(),
            author: None,
            place: None,
            tool: None,
            time_ms: 0,
            external: false,
            text: "t".into(),
            entities_matched: vec![],
            sources: BTreeMap::new(),
            fused,
        }
    }

    /// The top hits' nodes, each once at its best chunk, over the best.
    #[test]
    fn the_seeds_are_the_top_nodes_normalized() {
        let hits = [
            hit("a", 0.04),
            hit("b", 0.02),
            hit("a", 0.01),
            hit("c", 0.01),
        ];
        let got = seeds_of(&hits, 2);
        assert_eq!(got, [("a".to_string(), 1.0), ("b".to_string(), 0.5)]);
        assert!(seeds_of(&[], 10).is_empty());
        assert!(seeds_of(&[hit("a", 0.0)], 10).is_empty());
    }
}
