//! The daemon's two calls on the sealed sets, and health's view of them.

use std::sync::Arc;

use anyhow::Result;
use theseus_protocol::rsi::SealedState;

use super::{ItemId, Sealed};
use crate::fact::rsi_sealed::SelfExamCounted;
use crate::rpc::Core;

impl Core {
    /// The sealed store, when `[self] sealed_dir` is set.
    pub fn sealed(&self) -> Option<&Arc<Sealed>> {
        self.sealed.as_ref()
    }

    /// Write an item into a sealed set, counted by `reviewer`'s independent
    /// review: its `self.exam.counted` row carries the item's opaque id and
    /// family, never its text. Returns the id and no bytes. self-exams feeds
    /// its sealed half through here.
    pub fn sealed_put(
        &self,
        set: &str,
        family: &str,
        item: &[u8],
        reviewer: &str,
    ) -> Result<ItemId> {
        let sealed = self.sealed.as_ref().ok_or_else(|| {
            anyhow::anyhow!("[self] sealed_dir is not set: there is no sealed set")
        })?;
        let id = sealed.put(set, family, item)?;
        let fact = SelfExamCounted {
            item: id.as_str(),
            family,
            sealed: true,
            reviewer,
        };
        self.store.append(&[crate::fact::row(&fact, None, None)?])?;
        self.rec(None).announce(&fact);
        Ok(id)
    }

    /// Health's counts: present when a sealed dir is set.
    pub fn sealed_state(&self) -> Option<SealedState> {
        self.sealed
            .as_ref()
            .map(|s| SealedState { sets: s.counts() })
    }
}
