//! Categories in recall (theseus-jrf4c; the design of
//! `docs/design/jev-ontology-growth.md`, section 7): a note from a session
//! that shares a category with the asking session ranks higher, by a
//! reciprocal-rank term (`theseus_memory::CategoryBoost`) weighted by the
//! kind: the thread and the project highest, a topic next, the rest low.
//!
//! - **What is shared** is read from memory: the asker's and the candidate's
//!   stored memberships (`Ontology::memberships`, the held snapshot, never a
//!   scan), the thread the session belongs to (`ThreadScene::of`, the same
//!   reading the thread lens makes, so a note from another thread that shares
//!   a project or topic is lifted and still named by its thread), and the
//!   project its record carries.
//! - **Nothing is admitted** by it. The place rule and every filter run
//!   before the pipeline ranks, and the term moves only the rank. It is off
//!   in a shared place, whose interpretations are never read, when
//!   `[ontology] recall_boost` is 0, and while the ontology holds no
//!   category.
//! - The asking session's own notes are not lifted: they share everything.

use std::collections::BTreeMap;
use std::sync::Arc;

use theseus_memory::{Candidate, CategoryBoost};
use theseus_ontology::Ontology;

use crate::session::SessionRecord;
use crate::store::Store;
use crate::thread_recall::ThreadScene;
use crate::turn::TurnRunner;

/// What a shared category of `kind` is worth, from 0 to 1.
pub fn weight(kind: &str) -> f64 {
    match kind {
        "project" | "thread" => 1.0,
        "topic" => 0.6,
        _ => 0.3,
    }
}

/// What the ontology does to one recall.
#[derive(Clone)]
pub struct CategoryScene {
    gain: f64,
    session: String,
    snapshot: Arc<Ontology>,
    store: Store,
    threads: Option<ThreadScene>,
    /// The asker's categories, by key, with their weight.
    mine: BTreeMap<String, f64>,
}

impl CategoryScene {
    /// The categories `session` holds, by key: its stored memberships (a
    /// category's id), its thread (`thread:<slug>`) and its project
    /// (`project:<id>`).
    pub fn keys_of(&self, session: &str) -> BTreeMap<String, f64> {
        let mut keys: BTreeMap<String, f64> = self
            .snapshot
            .memberships(session)
            .into_iter()
            .map(|m| (m.category.to_string(), weight(m.category.kind())))
            .collect();
        if let Some(t) = self.threads.as_ref().and_then(|t| t.of(session)) {
            keys.insert(format!("thread:{}", t.slug), weight("thread"));
        }
        let project = self
            .store
            .get_session::<SessionRecord>(session)
            .ok()
            .flatten()
            .and_then(|r| r.project);
        if let Some(p) = project {
            keys.insert(format!("project:{p}"), weight("project"));
        }
        keys
    }

    /// The pure pipeline's term over `candidates`: for each candidate
    /// session, the heaviest category it shares with the asker. None when
    /// nothing is shared.
    pub fn boost(&self, candidates: &[Candidate]) -> Option<CategoryBoost> {
        if self.mine.is_empty() {
            return None;
        }
        let mut shared: BTreeMap<String, f64> = BTreeMap::new();
        let mut seen = std::collections::BTreeSet::new();
        for c in candidates {
            if c.session_id == self.session || !seen.insert(c.session_id.as_str()) {
                continue;
            }
            let theirs = self.keys_of(&c.session_id);
            let best = self
                .mine
                .iter()
                .filter(|(k, _)| theirs.contains_key(*k))
                .map(|(_, w)| *w)
                .fold(0.0_f64, f64::max);
            if best > 0.0 {
                shared.insert(c.session_id.clone(), best);
            }
        }
        (!shared.is_empty()).then_some(CategoryBoost {
            gain: self.gain,
            shared,
        })
    }
}

impl TurnRunner {
    /// The categories' part of a turn's ambient recall for `session`, or
    /// none: the term is off, the place is shared, the ontology holds no
    /// category, or the session holds none.
    pub fn category_scene(
        &self,
        session: Option<&str>,
        private: bool,
        threads: Option<&ThreadScene>,
    ) -> Option<CategoryScene> {
        let gain = self.cfg.ontology.recall_boost;
        let session = session?;
        if gain <= 0.0 || !private {
            return None;
        }
        let snapshot = self.ontology.held()?;
        snapshot.categories().next()?;
        let mut scene = CategoryScene {
            gain,
            session: session.to_string(),
            snapshot,
            store: self.store.clone(),
            threads: threads.cloned(),
            mine: BTreeMap::new(),
        };
        scene.mine = scene.keys_of(session);
        Some(scene)
    }
}

impl TurnRunner {
    /// The names of the categories `session` is filed under, for route.v6's
    /// `persona` question as evidence (theseus-jrf4c): the interpreted kinds'
    /// (a person is not one), by their path, from the held snapshot. None in
    /// a shared place, whose interpretations are never read, and while the
    /// ontology is not built.
    pub(crate) fn category_names(
        &self,
        session: &str,
        class: crate::places::PlaceClass,
    ) -> Vec<String> {
        if class == crate::places::PlaceClass::Shared {
            return Vec::new();
        }
        let Some(o) = self.ontology.held() else {
            return Vec::new();
        };
        o.memberships(session)
            .into_iter()
            .filter(|m| {
                o.kind(m.category.kind())
                    .is_some_and(|k| k.basis == theseus_ontology::Basis::Interpreted)
            })
            .map(|m| {
                o.path(&m.category)
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" › ")
            })
            .filter(|n| !n.is_empty())
            .collect()
    }
}
