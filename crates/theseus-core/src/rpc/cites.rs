//! `memory.cites` (theseus-ejdk): the sources of the handles a session's
//! replies cite, for a surface that draws a reply from its history. Each
//! handle is resolved in the session's own `Recall` nodes, newest first, as
//! the turn resolved it; a handle no recall of the session gave is left out.
//! In a shared place's session a source from a private place, or imported,
//! is left out too. A read.

use theseus_protocol::cite::{handles_in, Citation, MemoryCitesParams, MemoryCitesResult};
use theseus_protocol::error_code;

use super::server::RpcFailure;
use super::Core;
use crate::node::Body;
use crate::places::PlaceClass;
use crate::recall::cite::item_for;
use crate::stub::Kind;

/// The most handles one call resolves.
const MAX_HANDLES: usize = 64;

impl Core {
    /// `memory.recalls` and `memory.cites`: a session's recalls, and the sources its replies cite.
    pub(super) fn memory_reads(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            theseus_protocol::method::MEMORY_CITES => {
                super::server::route(params, |p| self.memory_cites(p))
            }
            _ => super::server::route(params, |p| self.memory_recalls(p)),
        }
    }

    pub fn memory_cites(&self, p: MemoryCitesParams) -> Result<MemoryCitesResult, RpcFailure> {
        if p.handles.len() > MAX_HANDLES {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                format!("memory.cites resolves at most {MAX_HANDLES} handles at once"),
            ));
        }
        self.session_exists(&p.session_id)?;
        let cites = self.cites_of(&p.session_id, &p.handles)?;
        Ok(MemoryCitesResult {
            session_id: p.session_id,
            cites,
        })
    }

    /// The sources of the handles a history page's replies hold
    /// (`session.history`'s `cites`): read only when a reply holds one.
    pub(super) fn page_cites(
        &self,
        session_id: &str,
        nodes: &[(u64, crate::node::Node)],
    ) -> Result<Vec<theseus_protocol::cite::Citation>, RpcFailure> {
        let mut handles: Vec<String> = Vec::new();
        for (_, n) in nodes {
            if let Body::AssistantMessage { blocks, .. } = &n.body {
                for h in handles_in(&crate::provider::text_of(blocks)) {
                    if !handles.contains(&h) && handles.len() < MAX_HANDLES {
                        handles.push(h);
                    }
                }
            }
        }
        if handles.is_empty() {
            return Ok(Vec::new());
        }
        self.cites_of(session_id, &handles)
    }

    /// Each handle in the session's own `Recall` nodes, newest first, as the
    /// turn resolved it; one no recall of the session gave is left out, and
    /// in a shared place's session so is a source it may not show.
    fn cites_of(&self, session_id: &str, handles: &[String]) -> Result<Vec<Citation>, RpcFailure> {
        let shared = self.runner.class_of(session_id) != PlaceClass::Private;
        let nodes = self.store.transcript(session_id)?;
        let mut cites = Vec::new();
        for handle in handles {
            let found = nodes
                .iter()
                .rev()
                .filter(|(_, n)| n.kind == Kind::Recall)
                .find_map(|(_, n)| match &n.body {
                    Body::Recall { items, .. } => item_for(items, handle),
                    _ => None,
                });
            cites.extend(found.and_then(|item| self.runner.citation(item, shared)));
        }
        Ok(cites)
    }
}
