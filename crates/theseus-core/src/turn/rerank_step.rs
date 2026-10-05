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
//!   Jev's order (`Memory::refill`: the same filters, the place rule first
//!   and the owner's labels, and the same budget); otherwise recall's own
//!   order stands. The manifest's `rerank` says which, and why.

use std::time::Duration;

use theseus_protocol::memory::RecallManifest;

use super::{Turn, TurnRunner};
use crate::config::memory::MemoryArm;
use crate::config::PackMode;
use crate::judge::rerank::Recalled;
use crate::recall::{Answer, Begun, Scene};

impl TurnRunner {
    /// The manifest of a recall in front of the model under `arm`'s
    /// science, reranked as the arms rule says; its items carry their
    /// excerpts (the node's ranges).
    pub(super) async fn recall_reranked(
        &self,
        t: &mut Turn<'_>,
        mode: &str,
        arm: MemoryArm,
        activation: Option<theseus_protocol::memory::RecallActivation>,
        begun: &Begun,
        answer: (Answer, Duration),
    ) -> RecallManifest {
        let rerank = self.judge.rerank_mode(t.tc.session_id);
        let mut scene = self.scene(t, mode, arm);
        scene.activation = activation;
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
        let retention = self.memory.retention_of(&*scene.science, &candidates);
        let Scene {
            place,
            in_context,
            labeled,
            budget_tokens,
            science,
            ..
        } = scene;
        let live = rerank == PackMode::Live;
        let recalled = |candidates| Recalled {
            recall_id: m.recall_id.clone(),
            session_id: t.tc.session_id.to_string(),
            turn_id: t.tc.turn_id.to_string(),
            message: begun.query.clone(),
            place: place.clone(),
            in_context: in_context.clone(),
            labeled: labeled.clone(),
            candidates,
            links: links.clone(),
            params: self.memory.params_of(&m),
            science: science.clone(),
            retention: retention.clone(),
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
            let scene = Scene {
                mode,
                session_id: Some(t.tc.session_id),
                turn_id: Some(t.tc.turn_id),
                place,
                in_context,
                labeled,
                budget_tokens,
                science,
                activation: None,
            };
            self.memory
                .refill(&scene, &mut m, candidates, &links, ranks, order);
        }
        m.rerank = Some(waited.manifest());
        m
    }
}
