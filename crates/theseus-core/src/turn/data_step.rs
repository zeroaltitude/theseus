//! The route step's floor under the sensitive names (theseus-oejh3,
//! theseus-xs5ah): a message that names one of `[privacy] sensitive_names`
//! runs only on a profile of US origin (`ModelOrigin::cleared_for_sensitive`).
//! The route skips every other profile, so the next usable one in the mode's
//! list runs; if none is left, the `[model]` default runs when it is cleared,
//! and otherwise the turn ends before any model call. The provider chokepoint
//! (`data_gate.rs`) is the floor under this for every other caller and for a
//! turn that asked no route.

use super::*;
use crate::routing::{Decision, HoldNext, Reason};

/// The profile the `[model]` section builds.
const DEFAULT_PROFILE: &str = "default";

pub(super) const REFUSED: &str =
    "this message names something in `[privacy] sensitive_names`, and \
                                  no profile of US origin is configured for it";

impl TurnRunner {
    /// The message names a sensitive name.
    pub(super) fn sensitive_message(&self, text: &str) -> bool {
        let names = self.cfg.privacy.names();
        !names.is_empty() && !names.hits(text).is_empty()
    }

    /// The request names a sensitive name in any string the chokepoint reads
    /// (`data_gate::names_in`): a name said earlier in the conversation, or in
    /// a recalled memory or a tool's text, keeps the route off a model that
    /// would refuse it there, not the message's own words alone.
    pub(super) fn sensitive_request(&self, req: &crate::provider::ProviderRequest) -> bool {
        let names = self.cfg.privacy.names();
        !names.is_empty() && crate::data_gate::names_in(req, &names)
    }

    /// The session has read a Google result (the Google design, §4.1): all
    /// Workspace content is private, so the route skips every profile of
    /// any origin but the allowlist's from now on. Nothing is read without `[google]`.
    pub(super) fn private_session(&self, t: &Turn<'_>) -> bool {
        crate::google::private::session(t.tc.store, &self.cfg, t.tc.session_id)
    }

    /// Whether `profile` may not be sent the sensitive names: its origin is
    /// not on the allowlist.
    pub(super) fn uncleared_profile(&self, profile: &str) -> bool {
        !self
            .cfg
            .profile_cleared_for_sensitive(profile, &self.catalog)
    }

    /// The route's answer, kept on profiles of US origin when the message
    /// names a sensitive name: the decision stands when it runs on one, else
    /// the `[model]` default runs if it is one, else the turn ends.
    pub(super) fn data_floor(
        &self,
        t: &Turn<'_>,
        decision: Option<Decision>,
    ) -> Result<Option<Decision>, Failure> {
        if !t.route.sensitive {
            return Ok(decision);
        }
        let ran = decision
            .as_ref()
            .map_or(t.target.profile.as_str(), |d| d.profile.as_str());
        if !self.uncleared_profile(ran) {
            return Ok(decision);
        }
        let usable = self
            .route_profiles(true)
            .get(DEFAULT_PROFILE)
            .is_some_and(|p| p.unusable.is_none());
        if usable && !self.uncleared_profile(DEFAULT_PROFILE) {
            return Ok(Some(Decision {
                profile: DEFAULT_PROFILE.into(),
                reason: Reason::DataGate,
                detour: false,
                switch: false,
                hold: HoldNext::Keep,
            }));
        }
        Err(Failure {
            class: crate::data_gate::DATA_GATE_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{}: {REFUSED}", crate::data_gate::DATA_GATE_CLASS),
            source: anyhow::anyhow!("{REFUSED}"),
        })
    }
}
