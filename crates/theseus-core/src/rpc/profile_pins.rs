//! `profile.pins` and `profile.unpin` (theseus-h66v4.2): the read side of every held profile, and the
//! let-go of them.

use theseus_protocol::{
    PinnedSession, ProfilePinsResult, ProfileUnpinParams, ProfileUnpinResult, UnpinFailure,
};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;

impl Core {
    pub fn profile_pins(&self) -> anyhow::Result<ProfilePinsResult> {
        let (live, live_source) = self.live_profile();
        Ok(ProfilePinsResult {
            live,
            live_source,
            sessions: 0,
            tasks: 0,
            pinned_sessions: 0,
            pinned_tasks: 0,
            pins: vec![],
        })
    }

    pub fn unpin_holds(&self, _who: &Answerer) -> (Vec<PinnedSession>, Vec<UnpinFailure>) {
        (vec![], vec![])
    }

    pub(super) fn profile_unpin(
        &self,
        _p: ProfileUnpinParams,
        _conn: Conn<'_>,
    ) -> Result<ProfileUnpinResult, RpcFailure> {
        Err(RpcFailure::invalid(anyhow::anyhow!("not built")))
    }
}

#[cfg(test)]
#[path = "tests_profile_pins.rs"]
mod tests;
