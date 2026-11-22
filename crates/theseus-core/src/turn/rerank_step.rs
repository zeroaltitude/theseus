//! Jev's rerank of a recall in front of the model (M6 step 32d; design
//! §2.7): the recall step's one call, `recall_reranked`, runs the pipeline
//! and, by the arms rule, has `rerank.v1` order it.
//!
//! - **The arms rule.** Memory's mode decides what reaches the model,
//!   rerank's whether Jev orders it. This recall reaches the model (a
//!   canary's treatment; every session under `live`): with `rerank.v1`
//!   `live` the turn waits for Jev's order, at most `[memory]
//!   rerank_wait_ms` from the rerank's start; `shadow` reranks it off the
//!   turn's path, as 32c's shadow recalls are; `off` reranks none. The mode
//!   is read before the candidates are cloned.
//! - **Bounded.** In time and answered, the node holds the pack again in
//!   Jev's order (`Memory::refill`: the same filters, the owner's labels
//!   among them, and the same budget); otherwise recall's own order stands.
//!   The manifest's `rerank` says which, and why.

use std::collections::BTreeSet;
use std::time::Duration;

use theseus_memory::recall::Place;
use theseus_memory::MemoryScience;
use theseus_protocol::memory::RecallManifest;

use super::{Turn, TurnRunner};
use crate::config::memory::MemoryArm;
use crate::config::PackMode;
use crate::judge::rerank::Recalled;
use crate::recall::{Answer, Begun, Scene};

/// A recall's scene, owned: what a pack again needs once the turn is
/// borrowed again.
struct Parts {
    place: Place,
    in_context: BTreeSet<String>,
    labeled: BTreeSet<String>,
    budget_tokens: Option<u64>,
    max_items: Option<usize>,
    science: std::sync::Arc<dyn MemoryScience>,
}

impl Parts {
    /// A scene's owned parts.
    fn of(s: Scene<'_>) -> Self {
        Parts {
            place: s.place,
            in_context: s.in_context,
            labeled: s.labeled,
            budget_tokens: s.budget_tokens,
            max_items: s.max_items,
            science: s.science,
        }
    }

    fn scene<'a>(&self, mode: &'a str, session: &'a str, turn: &'a str) -> Scene<'a> {
        Scene {
            mode,
            session_id: Some(session),
            turn_id: Some(turn),
            place: self.place.clone(),
            in_context: self.in_context.clone(),
            labeled: self.labeled.clone(),
            budget_tokens: self.budget_tokens,
            max_items: self.max_items,
            science: self.science.clone(),
            persona: None,
            category: None,
        }
    }
}

impl TurnRunner {
    /// The manifest of a recall in front of the model under `arm`'s
    /// science, reranked as the arms rule says; its items carry their
    /// excerpts (the node's ranges).
    pub(super) async fn recall_reranked(
        &self,
        t: &mut Turn<'_>,
        mode: &str,
        arm: MemoryArm,
        begun: &Begun,
        answer: (Answer, Duration),
    ) -> RecallManifest {
        let rerank = self.judge.rerank_mode(t.tc.session_id);
        let scene = self.scene(t, mode, arm);
        if rerank == PackMode::Off {
            return self.memory.manifest(
                &scene,
                begun,
                answer,
                |s| self.place_of(s),
                |ids| crate::recall::links(&self.store, ids),
                true,
            );
        }
        let (mut m, candidates, links, ranks) = self.memory.manifest_ranked(
            &scene,
            begun,
            answer,
            |s| self.place_of(s),
            |ids| crate::recall::links(&self.store, ids),
            true,
        );
        // What the rerank reads of the scene, taken now: the rest of it
        // borrows the turn, which the mark and the span need.
        let parts = Parts::of(scene);
        let live = rerank == PackMode::Live;
        let recalled = |candidates| Recalled {
            recall_id: m.recall_id.clone(),
            session_id: t.tc.session_id.to_string(),
            turn_id: t.tc.turn_id.to_string(),
            message: begun.query.clone(),
            in_context: parts.in_context.clone(),
            labeled: parts.labeled.clone(),
            candidates,
            links: links.clone(),
            params: self.memory.params_of(&m, parts.max_items),
            science: parts.science.clone(),
            admitted: m
                .admitted
                .iter()
                .map(|a| format!("{}#{}", a.node_id, a.chunk))
                .collect(),
            now_ms: theseus_protocol::now_unix_ms(),
        };
        if !live {
            let r = recalled(candidates);
            self.judge.at_recall(&mut t.trace, r);
            return m;
        }
        let r = recalled(candidates.clone());
        let wait = Duration::from_millis(self.memory.cfg().rerank_wait_ms);
        let waited = self.judge.at_recall_live(&mut t.trace, r, wait).await;
        if let Some(order) = waited.order.clone() {
            let scene = parts.scene(mode, t.tc.session_id, t.tc.turn_id);
            self.memory
                .refill(&scene, &mut m, candidates, &links, ranks, order);
        }
        m.rerank = Some(waited.manifest());
        m
    }
}
