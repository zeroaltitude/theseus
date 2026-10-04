//! The recall step on a turn's first loop (M6 step 30a; `crate::recall`):
//! begun as the loop's model call goes out, and read once it has answered,
//! so the index's wait overlaps the model's. In shadow its row rides in the
//! turn's next frame, and the request the model got is the one compiled
//! without it.

use std::collections::BTreeSet;

use super::{Turn, TurnRunner};
use crate::fact::recall::{scope, RecallShadow};
use crate::recall::{query_of, Begun, Scene};

impl TurnRunner {
    /// Ask the index, when memory is on and the turn brings something new.
    /// Nothing here fails the turn: a transcript that cannot be read is no
    /// recall.
    pub(super) fn recall_begin(&self, t: &Turn<'_>) -> Option<Begun> {
        if !self.memory.on() {
            return None;
        }
        let nodes = match t.tc.store.transcript(t.tc.session_id) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "recall: the transcript cannot be read; no recall this turn");
                return None;
            }
        };
        let (query, as_of) = query_of(&nodes, t.tc.turn_id)?;
        let deadline = std::time::Duration::from_millis(self.memory.cfg().recall_deadline_ms);
        Some(
            self.memory
                .begin(query, Some(as_of), crate::recall::K, deadline),
        )
    }

    /// Read the index's answer (never past its deadline), run the pipeline,
    /// and record the recall: its row, scoped to the session's recalls, its
    /// span, and its line.
    pub(super) async fn recall_end(&self, t: &mut Turn<'_>, mut begun: Begun) {
        let t0 = t.trace.at(begun.started);
        let answer = begun.answer().await;
        let in_context: BTreeSet<String> = match t.tc.store.transcript(t.tc.session_id) {
            Ok(nodes) => nodes.iter().map(|(_, n)| n.id.clone()).collect(),
            Err(_) => BTreeSet::new(),
        };
        let scene = Scene {
            mode: "shadow",
            session_id: Some(t.tc.session_id),
            turn_id: Some(t.tc.turn_id),
            place: self.place_of(t.tc.session_id),
            in_context,
        };
        let (m, candidates) =
            self.memory
                .manifest_with(&scene, &begun, answer, |s| self.place_of(s), false);
        let f = RecallShadow {
            manifest: &m,
            t0,
            t1: t.trace.now_us(),
        };
        match t.tc.rec().row(&f) {
            Ok(r) => {
                if let Err(e) = t.tc.store.defer(r.scoped(&scope(t.tc.session_id))) {
                    tracing::warn!(error = %e, "ledger append failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "recall: its row cannot be encoded"),
        }
        t.announce_fact(&f);
        // The `+rerank` arm in shadow (32c): off the turn's path.
        self.judge.at_recall(
            &mut t.trace,
            crate::judge::rerank::Recalled {
                recall_id: m.recall_id.clone(),
                session_id: t.tc.session_id.to_string(),
                turn_id: t.tc.turn_id.to_string(),
                message: begun.query.clone(),
                place: scene.place,
                in_context: scene.in_context,
                candidates,
                params: self.memory.cfg().params(),
                science: self.memory.science_owned(),
                admitted: m
                    .admitted
                    .iter()
                    .map(|a| format!("{}#{}", a.node_id, a.chunk))
                    .collect(),
                now_ms: theseus_protocol::now_unix_ms(),
            },
        );
    }
}
