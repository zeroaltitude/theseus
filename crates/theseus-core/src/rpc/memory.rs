//! `memory.search` and `memory.recalls` (M6 step 30a, §2.14): recall's
//! pipeline for a query, writing nothing, and a session's recalls as its
//! turns recorded them.

use std::collections::BTreeSet;
use std::time::Duration;

use theseus_memory::recall::{excerpt, Place};
use theseus_protocol::error_code;
use theseus_protocol::memory::{
    MemoryRecallsParams, MemoryRecallsResult, MemorySearchParams, RecallManifest,
};

use super::server::RpcFailure;
use super::Core;
use crate::fact::recall::scope;
use crate::ledger::LedgerRow;
use crate::recall::{text_of, Scene, K};
use crate::session::SessionRecord;

/// The least a search waits for the index: a question asked by hand, which
/// may meet a cold index.
const SEARCH_DEADLINE: Duration = Duration::from_secs(2);
/// The recalls `memory.recalls` gives by default, and at most.
const RECALLS: usize = 10;
const MAX_RECALLS: usize = 200;

impl Core {
    /// `memory.search`: the pipeline over `p.query`, as a turn in
    /// `p.session_id`'s place would run it (its nodes in context), or, with
    /// no session, as the CLI's: a private place. It writes nothing, whatever
    /// `[memory] mode` says.
    pub async fn memory_search(&self, p: MemorySearchParams) -> Result<RecallManifest, RpcFailure> {
        if p.query.trim().is_empty() {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "memory.search needs a query",
            ));
        }
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
        let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(SEARCH_DEADLINE);
        let mut begun = memory.begin(p.query.clone(), None, p.k.unwrap_or(K), deadline);
        let answer = begun.answer().await;
        let scene = Scene {
            mode: "search",
            session_id: p.session_id.as_deref(),
            turn_id: None,
            place,
            in_context,
        };
        Ok(memory.manifest(&scene, &begun, answer, |s| self.runner.place_of(s), true))
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

    fn session_exists(&self, sid: &str) -> Result<(), RpcFailure> {
        match self.store.get_session::<SessionRecord>(sid)? {
            Some(_) => Ok(()),
            None => Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no session {sid}"),
            )),
        }
    }
}
