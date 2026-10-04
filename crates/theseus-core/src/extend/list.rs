//! `extend.list` (`theseus extend list`) and health's count of proposals
//! (M7 43a): each manifest as it is now, read by one META prefix scan.

use theseus_protocol::extend::{ExtendHealth, ExtendInfo, ExtendListResult};

use super::Manifest;
use crate::rpc::Core;

impl From<&Manifest> for ExtendInfo {
    fn from(m: &Manifest) -> Self {
        Self {
            name: m.name.clone(),
            digest: m.digest.clone(),
            state: m.state.clone(),
            description: m.description.clone(),
            command: m.command.clone(),
            source: m.source.clone(),
            frozen: m.frozen.clone(),
            tools: m.tools.iter().map(|t| t.name.clone()).collect(),
            passed: m.passed() as u64,
            tests: m.tests.len() as u64,
            network: m.capabilities.network.clone(),
            error: m.error.clone(),
            session_id: m.proposed_by.session_id.clone(),
            question: m.question.clone(),
            proposed_at_ms: m.proposed_at_ms,
            answered_by: m.answered_by.clone(),
        }
    }
}

/// Proposals by state; None when there are none.
pub fn health_of(ms: &[Manifest]) -> Option<ExtendHealth> {
    if ms.is_empty() {
        return None;
    }
    let n = |s: &str| ms.iter().filter(|m| m.state == s).count() as u64;
    Some(ExtendHealth {
        proposals: ms.len() as u64,
        waiting: n("proposed"),
        acked: n("acked"),
        declined: n("declined"),
        failed: n("failed"),
    })
}

impl Core {
    /// `extend.list`: every proposal, newest first.
    pub fn extend_list(&self) -> anyhow::Result<ExtendListResult> {
        Ok(ExtendListResult {
            extensions: super::manifests(&self.store)?
                .iter()
                .map(ExtendInfo::from)
                .collect(),
        })
    }

    /// Health's `extensions`: a store that cannot be read says nothing.
    pub fn extend_health(&self) -> Option<ExtendHealth> {
        super::manifests(&self.store)
            .ok()
            .and_then(|ms| health_of(&ms))
    }
}
