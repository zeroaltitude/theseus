//! Recall (M6 step 30a; design §2.4), in shadow: on a turn's first loop, when
//! the turn brings something new (its input, a wake, a task's report), ask the
//! index what past nodes bear on it, filter and pack what it answers
//! (`theseus_memory::recall`), and record what would
//! have been admitted as a `recall.shadow` row (`fact::recall`). Nothing of it
//! reaches the model, and it writes no frame: the row rides in the turn's
//! next one.
//!
//! - **Never in the turn's way.** The index is asked in a task of its own as
//!   the loop's model call begins, under `[memory] recall_deadline_ms`, and the
//!   turn reads the answer once its call has answered: a stalled or absent
//!   index costs the turn at most the deadline past its call, and the row says
//!   `deadline` or `unavailable`.
//! - **Every place.** A turn draws on every session (anything goes,
//!   2026-11-22). A candidate's place is its session's, read as the turn's
//!   own is (`TurnRunner::place_of`): the manifest says it, nothing drops it.
//! - `memory.search` runs the same pipeline for a query, writing nothing, and
//!   `memory.recalls` reads a session's rows (`rpc/memory.rs`).
//!
//! **In front of the model (step 30b).** In `live`, the read finishes before
//! the first loop's compile, under the same deadline, and what it admits is
//! a `Recall` node (`node::Body::Recall`, references and never copies) with
//! a `derived_from` edge to each source (`via = "recall"`), which ride the
//! provider call's plan frame with the `recall.ran` row: no frame of their
//! own. It renders after the new message, read from each source by position
//! (`render`).
//! The operator's labels (`labels`) keep a node out as `labeled_wrong`.

pub mod cite;
pub mod current;
pub mod evidence;
pub mod labels;
pub mod loading;
pub mod neighbour;
pub mod outcomes;
pub mod relevance;
pub mod render;
pub mod stamp;
pub mod temporal;
pub mod when;

#[cfg(test)]
mod tests_current;
#[cfg(test)]
mod tests_time;

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use theseus_memory::recall::{self as pipeline, Asker, Candidate, Link, LinkKind, Pack, Place};
use theseus_memory::{Baseline, MemoryScience};
use theseus_protocol::index::{IndexQueryParams, IndexQueryResult};
use theseus_protocol::memory::{
    BudgetDrop, BudgetReport, RecallDrop, RecallItem, RecallManifest, RecallTimings,
};

use crate::config::memory::MemoryArm;
use crate::config::MemoryConfig;
use crate::node::{AttachmentContent, Body, Node};
use crate::store::Transcript;
use crate::tender::IndexTender;
use crate::turn::TurnRunner;

/// The index's hits a recall asks for (§2.4).
pub const K: usize = 40;
/// A recall's outcome when its vector search missed the deadline (or
/// failed) and the word sources' hits ranked alone (theseus-w9qv).
pub const WORDS_ONLY: &str = "words_only";
/// How much of the previous reply joins the query, so that "yes, do that"
/// still has a subject.
pub const REPLY_CHARS: usize = 500;
/// A turn whose new text is this many words or fewer ("yes", "do it") is
/// short: alone, its vector is near noise, so it embeds with the previous
/// reply's last lines (`SHORT_CONTEXT_CHARS` of them).
pub const SHORT_WORDS: usize = 3;
/// The previous reply's end a short turn's vector text carries, after the
/// turn's own words: about 30 word pieces, so the two fit in `[memory]
/// recall_vector_tokens`' default of 32, which keeps the start of a text.
pub const SHORT_CONTEXT_CHARS: usize = 120;

/// The index's answer to one query, or why there is none.
pub type AskFuture = Pin<Box<dyn Future<Output = Result<IndexQueryResult, String>> + Send>>;
/// What a recall's question is asked with, beyond its words.
struct Within {
    as_of: Option<u64>,
    k: usize,
    arm: MemoryArm,
    deadline: Duration,
    when: Option<when::When>,
    sessions: Vec<String>,
    /// A turn's own recall: the hot set and the nearest groups, not every record.
    fast: bool,
}

/// Who answers a recall's query: the index tender, or a test's stand-in.
pub type Ask = Arc<dyn Fn(IndexQueryParams) -> AskFuture + Send + Sync>;
/// Who answers a recall's two queries at once, its words' alone and then
/// the whole, on one of the tender's connections (theseus-zo1y).
pub type AskTwo =
    Arc<dyn Fn(IndexQueryParams, IndexQueryParams) -> (AskFuture, AskFuture) + Send + Sync>;

/// A recall's query: the words BM25 and entities read, and the text the
/// vector source embeds when not those words (theseus-zo1y: a turn's new
/// text alone, cut at `[memory] recall_vector_tokens` word pieces).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallQuery {
    pub words: String,
    pub vector: Option<String>,
    /// Whether the vector source's text is cut at `[memory]
    /// recall_vector_tokens`: a turn's recall, under its 250 ms deadline.
    /// A search's or a lookup's own words (a plain `String`, under their
    /// 2 s deadline) are embedded whole.
    pub cut: bool,
}

impl From<String> for RecallQuery {
    fn from(words: String) -> Self {
        Self {
            words,
            vector: None,
            cut: false,
        }
    }
}

/// Recall's settings, its science, and the index it asks.
pub struct Memory {
    cfg: MemoryConfig,
    science: Baseline,
    ask: RwLock<Option<Ask>>,
    /// The tender's two queries on one connection; none for a stand-in,
    /// whose two are asked apart.
    two: RwLock<Option<AskTwo>>,
    /// `index.warm`: sent at a person's message, and asked once when a
    /// recall's deadline passes (`loading`).
    warm: RwLock<Option<loading::Warm>>,
    /// A message's `index.warm` is out (`loading`).
    warming: Arc<AtomicBool>,
    /// The nodes the operator labeled wrong or stale, once read (`labels`).
    labels: RwLock<Option<BTreeSet<String>>>,
    /// The correction notes a retraction left for sessions' next turns (theseus-2ced).
    pub corrections: crate::retract::Notes,
    /// Sources a `Recall` node rendered, by id: they never change.
    sources: Mutex<render::Sources>,
    /// Where the warm-up's outcome is counted, once telemetry is built.
    telemetry: std::sync::OnceLock<crate::telemetry::Telemetry>,
    /// The memory's harness session, where the nightly writer kept its
    /// syntheses (31b), once read (`syntheses`): recall asks the index
    /// without it.
    syntheses: RwLock<Option<Syntheses>>,
    /// The last turns' recalls by outcome, for health (theseus-w9qv).
    outcomes: Mutex<outcomes::Outcomes>,
    /// Each session's thread, once read (theseus-f2g0).
    thread_memo: Arc<crate::thread_recall::Memo>,
}

/// The memory's harness session (31b): the syntheses a store may still
/// hold are kept there. The writer is gone; the session stays out of
/// recall, as it always was.
#[derive(Clone, Debug, Default)]
pub struct Syntheses {
    pub session: Option<String>,
}

impl Memory {
    /// Recall over `tender`'s index, as `cfg` says.
    pub fn new(cfg: MemoryConfig, tender: Option<Arc<IndexTender>>) -> Self {
        let two = tender.clone().map(|t| -> AskTwo {
            Arc::new(move |a: IndexQueryParams, b: IndexQueryParams| t.query_two(&a, &b))
        });
        let warm = tender.clone().map(loading::warm_of);
        let ask = tender.map(|t| -> Ask {
            Arc::new(move |p: IndexQueryParams| -> AskFuture {
                let t = t.clone();
                Box::pin(async move { of_tender(&t, &p).await })
            })
        });
        Self {
            cfg,
            science: Baseline::default(),
            ask: RwLock::new(ask),
            two: RwLock::new(two),
            warm: RwLock::new(warm),
            warming: Arc::default(),
            labels: RwLock::new(None),
            corrections: crate::retract::Notes::default(),
            sources: Mutex::new(render::Sources::new()),
            telemetry: std::sync::OnceLock::new(),
            syntheses: RwLock::new(None),
            outcomes: Mutex::new(outcomes::Outcomes::default()),
            thread_memo: Arc::default(),
        }
    }

    /// Each session's thread, once read (`thread_recall`).
    pub fn thread_memo(&self) -> Arc<crate::thread_recall::Memo> {
        self.thread_memo.clone()
    }

    /// A turn's recall, once the index's answer is read: health counts it.
    pub fn noted(&self, m: &RecallManifest) {
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .note(m, theseus_protocol::now_unix_ms());
    }

    /// The last turns' recalls by outcome; none before the first.
    pub fn outcomes(&self) -> Option<theseus_protocol::memory::RecallOutcomes> {
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .health()
    }

    /// Ask `ask` instead of the tender (tests: a stand-in index).
    pub fn set_ask(&self, ask: Ask) {
        *self.ask.write().unwrap_or_else(PoisonError::into_inner) = Some(ask);
        *self.two.write().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Ask `two` for a recall's two queries at once (tests: a stand-in
    /// that counts connections).
    pub fn set_ask_two(&self, two: AskTwo) {
        *self.two.write().unwrap_or_else(PoisonError::into_inner) = Some(two);
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

    /// An arm's science: `baseline`'s for every arm this build has. The
    /// arms' seam: an arm that ranks its own way adds its line here.
    pub fn science_for(&self, arm: MemoryArm) -> Arc<dyn MemoryScience> {
        match arm {
            MemoryArm::None | MemoryArm::Bm25 | MemoryArm::Baseline => self.science_owned(),
        }
    }

    /// The memory's harness session, read once from the store (by its META
    /// key).
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
            let session = store.get_meta::<String>(crate::consolidate::MEMORY_SESSION)?;
            Ok(Syntheses { session })
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
                tracing::warn!(error = %format!("{e:#}"), "recall: the memory's harness session cannot be read");
                Syntheses::default()
            }
        }
    }

    /// Where recall's warm-ups are counted, once telemetry is built.
    pub fn export_to(&self, t: crate::telemetry::Telemetry) {
        let _ = self.telemetry.set(t);
    }

    /// Memory's line in health: the mode and the arm. The retention and
    /// adjacency projections went with their arms; their fields say so.
    pub fn health(&self) -> theseus_protocol::memory::MemoryHealth {
        theseus_protocol::memory::MemoryHealth {
            mode: self.cfg.mode.as_str().into(),
            arm: self.cfg.arm.as_str().into(),
            retention: "unbuilt".into(),
            why: None,
            nodes: 0,
            events: 0,
            adjacency: None,
            recalls: None,
        }
    }

    /// A test planted the harness session after recall first read it (the
    /// retired writer told recall so as it opened the session).
    #[cfg(test)]
    pub(crate) fn harness_opened(&self, session: &str) {
        *self
            .syntheses
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(Syntheses {
            session: Some(session.to_string()),
        });
    }

    /// What is known of the harness session without a read.
    fn syntheses_known(&self) -> Syntheses {
        self.syntheses
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_default()
    }

    /// Ask the index for `query`'s hits as of `as_of` from `arm`'s sources
    /// (`MemoryArm::sources`), in a task of its own, bounded by `deadline`.
    /// Every arm leaves the memory's harness session out before the index's
    /// top k (`exclude_sessions`).
    pub fn begin(
        &self,
        query: impl Into<RecallQuery>,
        as_of: Option<u64>,
        k: usize,
        arm: MemoryArm,
        deadline: Duration,
    ) -> Begun {
        self.begin_within(query, as_of, k, arm, deadline, None, Vec::new())
    }

    /// `begin`, kept to the span of time `when` names (theseus-w9qv): the
    /// index's top k is drawn from inside it, and the pack drops any hit
    /// outside it (`when` drops: a tender of an older build ignores the
    /// span). `sessions`, when any, are the only sessions asked
    /// (`memory.lookup`'s book or topic).
    #[expect(clippy::too_many_arguments, reason = "begin's, and the two filters")]
    pub fn begin_within(
        &self,
        query: impl Into<RecallQuery>,
        as_of: Option<u64>,
        k: usize,
        arm: MemoryArm,
        deadline: Duration,
        when: Option<when::When>,
        sessions: Vec<String>,
    ) -> Begun {
        self.begin_with(
            query,
            Within {
                as_of,
                k,
                arm,
                deadline,
                when,
                sessions,
                fast: false,
            },
        )
    }

    /// [`Memory::begin_within`] for a turn's own recall, whose vector search may read the hot set
    /// and the nearest groups of the cold rest and not every record (issue 21 of 1.0.7). A search
    /// somebody asks for by name (`memory.search`, `memory.lookup`, the memory tools, a thread's
    /// notes) goes through `begin` and `begin_within`, and is exact.
    pub fn begin_turn(
        &self,
        query: impl Into<RecallQuery>,
        as_of: Option<u64>,
        arm: MemoryArm,
        deadline: Duration,
        when: Option<when::When>,
    ) -> Begun {
        self.begin_with(
            query,
            Within {
                as_of,
                k: K,
                arm,
                deadline,
                when,
                sessions: Vec::new(),
                fast: true,
            },
        )
    }

    fn begin_with(&self, query: impl Into<RecallQuery>, within: Within) -> Begun {
        let Within {
            as_of,
            k,
            arm,
            deadline,
            when,
            sessions,
            fast,
        } = within;
        let sources = arm.sources();
        let exclude: Vec<String> = self.syntheses_known().session.into_iter().collect();
        let ask = self
            .ask
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let two = self
            .two
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let warm = self.warm_ask();
        let RecallQuery {
            words: query,
            vector,
            cut,
        } = query.into();
        let mut p = IndexQueryParams::new(&query);
        p.vector_text = vector;
        p.vector_tokens = if cut {
            self.cfg.recall_vector_tokens
        } else {
            0
        };
        p.k = k.clamp(1, 100);
        p.as_of = as_of;
        // A turn's own recall reads the hot set and the nearest groups, not every record.
        p.fast = fast;
        p.sources = sources.iter().map(|s| s.to_string()).collect();
        // The vector source may wait for a model load (`loading`).
        if sources.contains(&"vector") {
            p.wait_ms = loading::LOAD_BOUND.as_millis() as u64;
        }
        p.exclude_sessions = exclude;
        if let Some(w) = &when {
            (p.filters.from_ms, p.filters.to_ms) = (w.from_ms, w.to_ms);
        }
        p.filters.sessions = sessions;
        let started = Instant::now();
        let task = tokio::spawn(async move {
            let (answer, words_only) = match ask {
                None => (Answer::Unavailable("no index is configured".into()), None),
                Some(ask) => race(&ask, two.as_ref(), warm.as_ref(), p, deadline).await,
            };
            (answer, started.elapsed(), words_only)
        });
        Begun {
            task,
            started,
            said: query.clone(),
            query,
            as_of,
            deadline,
            new_node: None,
            words_only: None,
            when,
            temporal: None,
            max_items: None,
        }
    }
}

/// The index's answer to `p` within `deadline` (theseus-w9qv). A query that
/// asks for vectors goes out twice at once: as asked, and for its word
/// sources alone, which answer in a few ms while the query's embedding may
/// take hundreds. The whole answer wins whenever it comes in time; else the
/// words' answer is used, with the vector source named in its `skipped` and
/// why (the second value); with neither, `Deadline`. With `two` (the
/// tender), both go on one connection, the words' first (theseus-zo1y).
/// When the deadline passes with the whole answer out and the words' in, and
/// the tender says its model is loading (`warm`), the wait goes on to
/// [`loading::LOAD_BOUND`] from the start, once (`loading`).
async fn race(
    ask: &Ask,
    two: Option<&AskTwo>,
    warm: Option<&loading::Warm>,
    p: IndexQueryParams,
    deadline: Duration,
) -> (Answer, Option<String>) {
    let words: Vec<String> = p
        .sources
        .iter()
        .filter(|s| *s != "vector")
        .cloned()
        .collect();
    if words.len() == p.sources.len() || words.is_empty() {
        let answer = match tokio::time::timeout(deadline, ask(p)).await {
            Ok(Ok(r)) => Answer::Hits(r),
            Ok(Err(why)) => Answer::Unavailable(why),
            Err(_) => Answer::Deadline,
        };
        return (answer, None);
    }
    let mut w = p.clone();
    w.sources = words;
    w.vector_text = None;
    w.wait_ms = 0;
    let (mut whole, mut word) = match two {
        Some(two) => {
            let (word, whole) = two(w, p);
            (whole, word)
        }
        None => (ask(p), ask(w)),
    };
    let started = tokio::time::Instant::now();
    let late = tokio::time::sleep(deadline);
    tokio::pin!(late);
    let mut loading_waited = false;
    let mut words_answer: Option<Result<IndexQueryResult, String>> = None;
    let mut failed: Option<String> = None;
    loop {
        tokio::select! {
            biased;
            r = &mut whole, if failed.is_none() => match r {
                Ok(r) => return (Answer::Hits(r), None),
                Err(why) if words_answer.is_some() => {
                    failed = Some(why);
                    break;
                }
                Err(why) => failed = Some(why),
            },
            r = &mut word, if words_answer.is_none() => {
                words_answer = Some(r);
                if failed.is_some() {
                    break;
                }
            }
            () = &mut late => {
                let more = loading::LOAD_BOUND.saturating_sub(deadline);
                if loading_waited
                    || more.is_zero()
                    || failed.is_some()
                    || words_answer.is_none()
                    || !loading::covered(warm).await
                {
                    break;
                }
                loading_waited = true;
                late.as_mut().reset(started + loading::LOAD_BOUND);
            }
        }
    }
    match (words_answer, failed) {
        (Some(Ok(mut r)), failed) => {
            let waited = if loading_waited {
                loading::LOAD_BOUND
            } else {
                deadline
            };
            let waited_ms = waited.as_millis();
            let why = match failed {
                Some(e) => format!("the vector search failed ({e}); the words alone ranked"),
                None => format!(
                    "the vector search had not answered within {waited_ms} ms; the words alone ranked"
                ),
            };
            r.skipped.insert("vector".into(), why.clone());
            (Answer::Hits(r), Some(why))
        }
        (_, Some(why)) => (Answer::Unavailable(why), None),
        (Some(Err(_)) | None, None) => (Answer::Deadline, None),
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
    task: tokio::task::JoinHandle<(Answer, Duration, Option<String>)>,
    pub started: Instant,
    pub query: String,
    /// What the turn's message itself says (its text and attachment names),
    /// without the previous reply `query` carries: the words a thread is named
    /// by (theseus-n6mkd). The query's own words when nothing else is set.
    pub said: String,
    pub as_of: Option<u64>,
    pub deadline: Duration,
    /// The turn's new node, which seeds a spread at 1.0 (32b).
    pub new_node: Option<String>,
    /// The span of time the question named (theseus-w9qv).
    pub when: Option<when::When>,
    /// The kind of question about time it asked, which orders the pack by
    /// the time of the events (theseus-q1tbn).
    pub temporal: Option<temporal::Kind>,
    /// The most items the pack admits, when not `[memory] recall_max_items`:
    /// a words search's page (the history tool's, the MCP server's; theseus-w9qv).
    pub max_items: Option<usize>,
    /// Once answered: the hits are the word sources' alone, and why (the
    /// vector search was late or failed; theseus-w9qv).
    pub words_only: Option<String>,
}

/// A recall nothing will read (its turn ended, or failed) asks no more: its
/// task ends, and with it the tender's connection, so the tender stops the
/// query's embedding (theseus-zo1y).
impl Drop for Begun {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Begun {
    /// The pack's parameters: the config's, under `budget_tokens`, and this
    /// recall's own page size when it has one.
    fn params(
        &self,
        p: theseus_memory::Params,
        (budget_tokens, scene_items): (u64, Option<usize>),
    ) -> theseus_memory::Params {
        theseus_memory::Params {
            budget_tokens,
            max_items: self.max_items.or(scene_items).unwrap_or(p.max_items),
            ..p
        }
    }

    /// Its outcome once its hits are in: `ran`, or `words_only` when they
    /// are the word sources' alone.
    fn outcome(&self) -> &'static str {
        self.words_only.as_ref().map_or("ran", |_| WORDS_ONLY)
    }

    /// The index's answer, and how long it took; never past the deadline.
    pub async fn answer(&mut self) -> (Answer, Duration) {
        match (&mut self.task).await {
            Ok((answer, took, words_only)) => {
                self.words_only = words_only;
                (answer, took)
            }
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
    /// The pack's items when not `[memory] recall_max_items`: a deep prompt's
    /// (theseus-d9je7). A `Begun`'s own page size wins.
    pub max_items: Option<usize>,
    /// The turn's arm's science (`Memory::science_for`): `baseline` in
    /// shadow, and for a search with no arm.
    pub science: Arc<dyn MemoryScience>,
    /// The engaged persona's part of the recall (theseus-a8n7); none when no
    /// persona is engaged.
    pub persona: Option<crate::persona_recall::PersonaScene>,
    /// The categories the asking session shares with a note's (theseus-jrf4c);
    /// none in a shared place, for a lookup, or with the term off.
    pub category: Option<crate::category_recall::CategoryScene>,
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
            outcome: begun.outcome().into(),
            why: begun.words_only.clone(),
            temporal: begun.temporal.map(|k| k.as_str().into()),
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
            other => {
                (m.outcome, m.why) = match other {
                    Answer::Unavailable(why) => ("unavailable".into(), Some(why)),
                    _ => ("deadline".into(), None),
                };
                m.timings.total_ms = ms(begun.started.elapsed());
                return (m, Vec::new(), Vec::new(), Ranks::new());
            }
        };
        let t0 = Instant::now();
        m.indexed_through = Some(r.indexed_through);
        m.candidates = r.hits.len() as u64;
        let r = when::kept(&mut m, begun.when.as_ref(), r);
        let r_weights = r.weights.clone();
        m.skipped = r.skipped;
        m.timings.index = Some(r.timings);
        let mut sources: BTreeMap<String, u64> = BTreeMap::new();
        let mut places: BTreeMap<String, Place> = BTreeMap::new();
        let mut ranks = BTreeMap::new();
        let candidates: Vec<Candidate> = r
            .hits
            .into_iter()
            .enumerate()
            .map(|(i, mut h)| {
                for s in h.sources.keys() {
                    *sources.entry(s.clone()).or_default() += 1;
                }
                let place = places
                    .entry(h.session_id.clone())
                    .or_insert_with(|| place_of(&h.session_id))
                    .clone();
                ranks.insert(
                    format!("{}#{}", h.node_id, h.chunk),
                    std::mem::take(&mut h.sources),
                );
                candidate(h, i + 1, place)
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
        let asker = Asker {
            session_id: scene.session_id.unwrap_or_default(),
            in_context: &scene.in_context,
            labeled: &scene.labeled,
            links: &links,
        };
        let mut params = begun.params(self.cfg.params(), (m.budget_tokens, scene.max_items));
        params.persona = scene.persona.as_ref().and_then(|p| p.boost(&candidates));
        params.category = scene.category.as_ref().and_then(|c| c.boost(&candidates));
        (params.floor, m.relevance) = relevance::of(&self.cfg, &r_weights, &m.sources, &candidates);
        let sci = &*scene.science;
        let pack = match begun.temporal {
            Some(k) => temporal::pack(k, &begun.said, sci, &asker, &candidates, &params),
            None => pipeline::recall(sci, &asker, candidates.clone(), &params),
        };
        let kept = ranks.clone();
        fill(&mut m, pack, &mut ranks, texts);
        neighbour::apply(&mut m.admitted, &neighbour::marks(&candidates));
        current::mark(&mut m.admitted, begun, &links, &candidates);
        m.timings.pack_ms = ms(t0.elapsed());
        m.timings.total_ms = ms(begun.started.elapsed());
        (m, candidates, links, kept)
    }

    /// The pack again, ranked by `order` (Jev's, from a live rerank, 32d):
    /// the same filters, the memory pass's links, and
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
        let asker = Asker {
            session_id: scene.session_id.unwrap_or_default(),
            in_context: &scene.in_context,
            labeled: &scene.labeled,
            links,
        };
        let params = self.params_of(m, scene.max_items);
        let marks = neighbour::marks(&candidates);
        // The superseded marks (a current-value question's) were set by the
        // recall on the items it admitted; the refill rebuilds the items, so
        // they are read now and set again, as the neighbour note is.
        let superseded = current::marked(&m.admitted);
        let pack =
            theseus_memory::rerank::repack(&*scene.science, &asker, candidates, &params, order);
        m.drops.clear();
        fill(m, pack, &mut ranks, true);
        neighbour::apply(&mut m.admitted, &marks);
        current::apply(&mut m.admitted, &superseded);
    }

    /// The pack's limits for a manifest's budget, at `max_items` when its
    /// scene sized them.
    pub fn params_of(
        &self,
        m: &RecallManifest,
        max_items: Option<usize>,
    ) -> theseus_memory::Params {
        let p = self.cfg.params();
        theseus_memory::Params {
            budget_tokens: m.budget_tokens,
            max_items: max_items.unwrap_or(p.max_items),
            floor: m
                .relevance
                .as_ref()
                .map(relevance::floor_of)
                .unwrap_or_default(),
            ..p
        }
    }
}

/// An index hit as recall's candidate: `index_rank` its rank from 1, `place`
/// its session's.
fn candidate(h: theseus_protocol::index::IndexHit, index_rank: usize, place: Place) -> Candidate {
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
        index_rank,
        place,
        time_ms: h.time_ms,
        entities: h.entities,
        matched: h.entities_matched,
        copy: h.tool.as_deref().is_some_and(crate::copies::copier),
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
                superseded: None,
                retention: None,
                neighbour: None,
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
            own_top: None,
            share: None,
            paid: None,
        })
        .collect();
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
                Some(EdgeKind::DerivedFrom | EdgeKind::Includes) | None => continue,
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

/// `private`, `shared:<target>`, or `unknown`.
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
///
/// The vector source embeds the new texts alone (theseus-zo1y), which the
/// tender cuts at `[memory] recall_vector_tokens` word pieces: a query's
/// embedding grows with its tokens, and the whole query (about 115 word
/// pieces with the reply's start) took longer than recall's deadline. A short
/// turn (`SHORT_WORDS` words or fewer: "yes") embeds its words, then the
/// previous reply's last lines (`short_context`): the word alone is near
/// noise. Its words' query carries the reply's start, as every turn's does.
/// With no new text (files alone), the vector source embeds the whole query,
/// cut the same way.
pub fn query_of(nodes: &Transcript, turn_id: &str) -> Option<(RecallQuery, u64)> {
    let first = nodes.iter().position(|(_, n)| {
        n.turn_id.as_deref() == Some(turn_id) && matches!(n.body, Body::UserMessage { .. })
    })?;
    let mut parts = Vec::new();
    let mut new = Vec::new();
    for (_, n) in &nodes[first..] {
        if n.turn_id.as_deref() != Some(turn_id) {
            continue;
        }
        if let Body::UserMessage { text, attachments } = &n.body {
            parts.push(text.clone());
            new.push(text.as_str());
            parts.extend(attachments.iter().map(|a| a.name.clone()));
        }
    }
    let vector = new.join("\n");
    let mut vector = (!vector.trim().is_empty()).then_some(vector);
    let reply = nodes[..first]
        .iter()
        .rev()
        .find_map(|(_, n)| match &n.body {
            Body::AssistantMessage { blocks, .. } => Some(crate::provider::text_of(blocks)),
            _ => None,
        });
    if let Some(r) = reply.filter(|r| !r.trim().is_empty()) {
        if let Some(v) = vector
            .as_mut()
            .filter(|v| v.split_whitespace().count() <= SHORT_WORDS)
        {
            v.push('\n');
            v.push_str(&short_context(&r));
        }
        parts.push(r.chars().take(REPLY_CHARS).collect());
    }
    let words = parts.join("\n");
    let q = RecallQuery {
        words,
        vector,
        cut: true,
    };
    (!q.words.trim().is_empty()).then(|| (q, nodes[first].0))
}

/// What the turn's message says: its text and its attachments' names, with
/// none of the previous reply that `query_of`'s words carry.
pub fn said_of(nodes: &Transcript, turn_id: &str) -> String {
    let mut said = Vec::new();
    for (_, n) in nodes {
        if n.turn_id.as_deref() != Some(turn_id) {
            continue;
        }
        if let Body::UserMessage { text, attachments } = &n.body {
            said.push(text.clone());
            said.extend(attachments.iter().map(|a| a.name.clone()));
        }
    }
    said.join("\n")
}

/// The end of `reply` a short turn embeds with: its last lines, whole, up to
/// `SHORT_CONTEXT_CHARS` (the question a "yes" answers is usually there);
/// when its last line alone is longer, that line's end from a word's start.
pub fn short_context(reply: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut chars = 0;
    for line in reply.lines().rev().map(str::trim).filter(|l| !l.is_empty()) {
        let n = line.chars().count();
        if chars + n > SHORT_CONTEXT_CHARS {
            if kept.is_empty() {
                let skip = n - SHORT_CONTEXT_CHARS;
                let end: String = line.chars().skip(skip).collect();
                return match end.split_once(char::is_whitespace) {
                    Some((_, rest)) if !rest.trim().is_empty() => rest.trim().to_string(),
                    _ => end,
                };
            }
            break;
        }
        chars += n + 1;
        kept.push(line);
    }
    kept.reverse();
    kept.join("\n")
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
        // A moved range's record is a note to the compiler, never recalled.
        // A tombstone has no text: its payload is gone.
        Body::Moved { .. } | Body::Erased { .. } => String::new(),
    }
}

impl TurnRunner {
    /// Where `session_id` speaks, by name, for a recalled item's header (35a):
    /// its bound place's name (`#harbor`, `DM @zeroaltitude`), its target when no
    /// binding names it, or the session itself on the CLI or the web UI.
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

    /// Whether `session_id` speaks in a private place (the CLI, the web UI,
    /// the owner's DM, a channel bound private), as `place_of` reads it. The
    /// guild calendar names an event for its title only then (theseus-4zc4d).
    pub fn speaks_privately(&self, session_id: &str) -> bool {
        self.place_of(session_id) == Place::Private
    }
}
