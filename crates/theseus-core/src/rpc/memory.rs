//! `memory.search` and `memory.recalls` (M6 step 30a, §2.14): recall's
//! pipeline for a query, writing nothing, and a session's recalls as its
//! turns recorded them. `memory.label` (30b): an operator's label on a node,
//! judged as an approval is (`judge_act(Act::Label)`: the owner, from a
//! private place), so a job's process cannot grade its own memory.

use std::collections::BTreeSet;
use std::time::Duration;

use theseus_memory::recall::{excerpt, Place};
use theseus_protocol::error_code;
use theseus_protocol::memory::{
    MemoryLabelParams, MemoryLabelResult, MemoryRecallsParams, MemoryRecallsResult,
    MemorySearchParams, RecallManifest, MEMORY_LABELS,
};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::fact;
use crate::fact::recall::scope;
use crate::ledger::LedgerRow;
use crate::recall::labels;
use crate::recall::{text_of, Scene, K};
use crate::session::SessionRecord;

/// The least a search waits for the index: a question asked by hand, which
/// may meet a cold index.
const SEARCH_DEADLINE: Duration = Duration::from_secs(2);
/// The recalls `memory.recalls` gives by default, and at most.
const RECALLS: usize = 10;
const MAX_RECALLS: usize = 200;

impl Core {
    /// `memory.search`: the pipeline over `p.query` under `p.arm` (default
    /// `baseline`: its sources and science), as a turn in `p.session_id`'s
    /// place would run it (its nodes in context), or, with no session, as
    /// the CLI's: a private place. It writes nothing, whatever `[memory]
    /// mode` says. An arm that reads retention builds the projection when
    /// nothing has yet, off this answer's path: until it is built, the row
    /// says so. Under `+activation` it builds the adjacency projection if no
    /// turn has (32b), within its deadline. Every arm but `+synthesis` leaves
    /// the memory's harness session out (31b).
    pub async fn memory_search(&self, p: MemorySearchParams) -> Result<RecallManifest, RpcFailure> {
        if p.query.trim().is_empty() {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "memory.search needs a query",
            ));
        }
        let arm = match p.arm.as_deref() {
            None => MemoryArm::Baseline,
            Some(name) => match MemoryArm::named(name) {
                Some(MemoryArm::None) => {
                    return Err(RpcFailure::new(
                        error_code::INVALID_PARAMS,
                        "arm none recalls nothing: it asks the index nothing",
                    ))
                }
                Some(a) => a,
                None => {
                    return Err(RpcFailure::new(
                        error_code::INVALID_PARAMS,
                        format!(
                        "{name:?} is not an arm: one of bm25, baseline, +retention, +activation, +synthesis"
                    ),
                    ))
                }
            },
        };
        let (place, in_context) = match &p.session_id {
            Some(sid) => {
                self.session_exists(sid)?;
                let nodes = self.store.session_nodes(sid)?;
                let ids: BTreeSet<String> = nodes.into_iter().map(|(_, n)| n.id).collect();
                (self.runner.place_of(sid), ids)
            }
            None => (Place::Private, BTreeSet::new()),
        };
        let memory = &self.runner.memory;
        if arm.reads_retention() {
            crate::recall::retention::warm(memory, &self.store);
        }
        memory.syntheses(&self.store);
        let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(SEARCH_DEADLINE);
        let mut begun = memory.begin(p.query.clone(), None, p.k.unwrap_or(K), arm, deadline);
        let answer = begun.answer().await;
        let (answer, activation) = memory
            .activated(&self.store, arm, &begun, answer, in_context.clone(), true)
            .await;
        let scene = Scene {
            mode: "search",
            session_id: p.session_id.as_deref(),
            turn_id: None,
            place,
            in_context,
            labeled: memory.labeled(&self.store)?,
            budget_tokens: None,
            science: memory.science_for(arm),
            activation,
        };
        Ok(memory.manifest(
            &scene,
            &begun,
            answer,
            |s| self.runner.place_of(s),
            |ids| crate::recall::links(&self.store, ids),
            true,
        ))
    }

    /// `memory.recalls`: the session's recall rows, newest last, each
    /// admitted item with its node's text as the pack would cut it.
    pub fn memory_recalls(
        &self,
        p: MemoryRecallsParams,
    ) -> Result<MemoryRecallsResult, RpcFailure> {
        self.session_exists(&p.session_id)?;
        let limit = p.limit.unwrap_or(RECALLS).clamp(1, MAX_RECALLS);
        let records = self.store.scope_after(&scope(&p.session_id), 0)?;
        let total = records.len() as u64;
        let item_tokens = self.runner.memory.cfg().params().item_tokens;
        let mut recalls = Vec::new();
        for r in &records[records.len().saturating_sub(limit)..] {
            let row: LedgerRow = r.decode()?;
            // The session's arm row shares the scope: only recalls here.
            if row.kind == theseus_protocol::LedgerKind::MemoryArm.as_str() {
                continue;
            }
            let Ok(mut m) = serde_json::from_value::<RecallManifest>(row.data) else {
                continue;
            };
            for item in &mut m.admitted {
                if let Some((_, n)) = self.store.get_node(&item.node_id)? {
                    item.text = Some(excerpt(&text_of(&n), item_tokens));
                }
            }
            recalls.push(m);
        }
        Ok(MemoryRecallsResult {
            session_id: p.session_id,
            total,
            recalls,
        })
    }

    /// `memory.label`: the operator's label on a node, as a `memory.label`
    /// row scoped `memory`; `wrong` and `stale` keep the node out of recall
    /// from the next turn on (`labeled_wrong`), and `useful` lets it back.
    pub fn memory_label(
        &self,
        p: &MemoryLabelParams,
        who: impl Into<Answerer>,
    ) -> anyhow::Result<MemoryLabelResult> {
        let who = who.into();
        let label = p.label.trim();
        if !MEMORY_LABELS.contains(&label) {
            anyhow::bail!(
                "{label:?} is not a label: one of {}",
                MEMORY_LABELS.join(", ")
            );
        }
        if self.store.get_node(&p.node_id)?.is_none() {
            anyhow::bail!("no node is named {}", p.node_id);
        }
        let what = format!("{label} on {}", p.node_id);
        self.judge_act(&who, Act::Label { what: &what })?;
        let memory = &self.runner.memory;
        let mut set = memory.labeled(&self.store)?;
        match labels::excludes(label) {
            Some(true) => {
                set.insert(p.node_id.clone());
            }
            Some(false) => {
                set.remove(&p.node_id);
            }
            None => {}
        }
        let (who_s, via) = (who.who(), who.via());
        let f = fact::recall::Labeled {
            node_id: &p.node_id,
            label,
            recall_id: p.recall_id.as_deref(),
            note: p.note.as_deref(),
            who: &who_s,
            via: &via,
            excluded: set.contains(&p.node_id),
        };
        let records = [fact::row(&f, None, None)?.scoped(labels::SCOPE)];
        let positions = self.store.append(&records)?;
        memory.labeled_now(&p.node_id, label);
        memory.retention_written(&records, &positions);
        self.rec(None).announce(&f);
        Ok(MemoryLabelResult {
            node_id: p.node_id.clone(),
            label: label.into(),
            excluded: f.excluded,
        })
    }

    pub(super) fn rpc_memory_label(
        &self,
        p: MemoryLabelParams,
        conn: Conn<'_>,
    ) -> Result<MemoryLabelResult, RpcFailure> {
        let who = conn.answerer(None, None);
        self.memory_label(&p, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "a label from {} does not count: {}. Nothing was written.",
                        r.who, r.why
                    ),
                    data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }

    /// The label set, built now, off the start path: after serving, on the
    /// blocking pool. A recall that comes sooner builds it itself, once.
    pub fn warm_labels(self: &std::sync::Arc<Self>) {
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            let Some(core) = core.upgrade() else { return };
            if let Err(e) = core.runner.memory.labeled(&core.store) {
                tracing::warn!(error = %format!("{e:#}"), "memory: the labels cannot be read");
            }
        });
    }

    /// The retention projection (32a), built now when the config's arm
    /// reads it: after serving, on the blocking pool. Otherwise the first
    /// search that asks builds it.
    pub fn warm_retention(self: &std::sync::Arc<Self>) {
        let memory = &self.runner.memory;
        if memory.on() && memory.cfg().arm.reads_retention() {
            crate::recall::retention::warm(memory, &self.store);
        }
    }

    /// The adjacency projection (32b), built now on the blocking pool, off
    /// the start path, when `[memory] arm = "+activation"` puts it in front
    /// of the model (canary or live). A turn that comes sooner starts the
    /// build and goes on without activation; a search builds it itself.
    pub fn warm_activation(&self) {
        let cfg = self.runner.memory.cfg();
        if cfg.arm == MemoryArm::Activation
            && matches!(cfg.mode, MemoryMode::Canary | MemoryMode::Live)
        {
            self.runner.memory.adjacency.warm(&self.store);
        }
    }

    /// Health's memory block: the mode and arm, the retention projection's
    /// state (32a), and the adjacency projection once an arm reads it or a
    /// search built it (32b).
    pub fn memory_health(&self) -> theseus_protocol::memory::MemoryHealth {
        let memory = &self.runner.memory;
        let cfg = memory.cfg();
        let a = &memory.adjacency;
        let reads = cfg.arm == MemoryArm::Activation
            && matches!(cfg.mode, MemoryMode::Canary | MemoryMode::Live);
        let adjacency = match (a.stats(), a.error()) {
            (Some(st), _) => Some(theseus_protocol::memory::AdjacencyHealth {
                state: "built".into(),
                why: None,
                nodes: st.nodes,
                edges: st.edges,
                entities: st.entities,
                unmapped: st.unmapped,
                bytes: st.bytes,
                through: st.through,
            }),
            (None, Some(why)) => Some(theseus_protocol::memory::AdjacencyHealth {
                state: "failed".into(),
                why: Some(why),
                ..Default::default()
            }),
            (None, None) if a.building() => Some(theseus_protocol::memory::AdjacencyHealth {
                state: "building".into(),
                ..Default::default()
            }),
            (None, None) if reads => Some(theseus_protocol::memory::AdjacencyHealth {
                state: "waiting".into(),
                ..Default::default()
            }),
            (None, None) => None,
        };
        theseus_protocol::memory::MemoryHealth {
            adjacency,
            ..memory.health()
        }
    }

    pub(super) fn session_exists(&self, sid: &str) -> Result<(), RpcFailure> {
        match self.store.get_session::<SessionRecord>(sid)? {
            Some(_) => Ok(()),
            None => Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no session {sid}"),
            )),
        }
    }
}
