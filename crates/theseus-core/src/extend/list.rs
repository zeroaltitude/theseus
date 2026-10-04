//! `extend.list` (`theseus extend list`, Discord's `/extensions`) and
//! health's counts (M7 43a, 43b): each manifest as it is now, read by one
//! META prefix scan, and each loaded extension with its server's state.

use theseus_protocol::extend::{ExtendHealth, ExtendInfo, ExtendListResult, ExtendLoadedInfo};

use super::load::{Loaded, LoadedSet};
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
            files: m.files as u64,
            bytes: m.bytes,
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

/// A loaded extension as `extend.list` shows it, with its server now.
fn loaded_info(l: &Loaded, board: &crate::mcp::McpBoard) -> ExtendLoadedInfo {
    let server = l.server();
    let status = board.status().into_iter().find(|s| s.name == server);
    ExtendLoadedInfo {
        name: l.name.clone(),
        digest: l.digest.clone(),
        description: l.description.clone(),
        command: l.command.clone(),
        frozen: l.frozen.clone(),
        tools: l.canonical(),
        network: l.capabilities.network.clone(),
        acked_by: l.acked_by.clone(),
        acked_via: l.acked_via.clone(),
        acked_at_ms: l.acked_at_ms,
        session_id: l.proposed_by.session_id.clone(),
        replaced: l.replaced.clone(),
        state: status
            .as_ref()
            .map_or_else(|| "stopped".into(), |s| s.state.clone()),
        calls: status.as_ref().map_or(0, |s| s.calls),
        errors: status.as_ref().map_or(0, |s| s.errors),
        last_error: status.and_then(|s| s.last_error),
        server,
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
        loaded: 0,
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
            loaded: LoadedSet::read(&self.store)?
                .loaded
                .values()
                .map(|l| loaded_info(l, &self.mcp))
                .collect(),
        })
    }

    /// Health's `extensions`: a store that cannot be read says nothing.
    pub fn extend_health(&self) -> Option<ExtendHealth> {
        let mut h = super::manifests(&self.store)
            .ok()
            .and_then(|ms| health_of(&ms))?;
        h.loaded = LoadedSet::read(&self.store)
            .map(|s| s.loaded.len() as u64)
            .unwrap_or_default();
        Some(h)
    }
}
