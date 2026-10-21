//! The route step's data floor (theseus-oejh3, theseus-xs5ah, theseus-a1wvq.3).
//!
//! - **Restricted origin**: a profile whose model is of restricted origin
//!   (`cn` or `unknown`, `Config::profile_origin`) runs only for a turn whose
//!   `Clearance` is cleared, and none is yet: every turn starts
//!   `Clearance::closed(DataWhy::Unchecked)` (`RouteState.clearance`).
//! - **The sensitive names**: a message that names one of `[privacy]
//!   sensitive_names`, or a session that read private content, runs only on a
//!   profile of US origin (`ModelOrigin::cleared_for_sensitive`).
//!
//! The route skips every profile the floor refuses (`route_profiles`), so the
//! next usable one in the mode's list runs; if the turn would still run on
//! one (its base, a pin, a hold, a correction, a checkpoint's move), the
//! `[model]` default runs when the floor lets it, with `Reason::DataGate`, and
//! otherwise the turn ends before any model call. A turn that asked no route
//! is floored at its first loop (`floor_unrouted`), so a restricted base moves
//! rather than failing at the provider chokepoint (`data_gate.rs`), which
//! stays the floor under this for every other caller.

use super::*;
use crate::provider::Provider;
use crate::routing::{Decision, HoldNext, Reason};

/// The profile the `[model]` section builds.
const DEFAULT_PROFILE: &str = "default";

pub(super) const REFUSED: &str =
    "this message names something in `[privacy] sensitive_names`, and \
                                  no profile of US origin is configured for it";

/// What the floor keeps a turn off, as `route_profiles` reads it.
#[derive(Clone, Copy)]
pub(super) struct Floor {
    /// The names' floor: only a profile of US origin.
    pub(super) sensitive: bool,
    /// The turn's clearance lets a profile of restricted origin run.
    pub(super) cleared: bool,
}

impl super::route_step::RouteState {
    /// The turn's floor: its names and its clearance.
    pub(super) fn floor(&self) -> Floor {
        Floor {
            sensitive: self.sensitive,
            cleared: self.clearance.is_cleared(),
        }
    }
}

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

    /// Whether `profile`'s model is of restricted origin (a profile that does
    /// not resolve is).
    fn restricted_profile(&self, profile: &str) -> bool {
        self.cfg.profile_origin(profile, &self.catalog).restricted()
    }

    /// Why the floor keeps the turn off `profile`, if it does.
    pub(super) fn floor_refuses(&self, floor: Floor, profile: &str) -> Option<String> {
        if !floor.cleared && self.restricted_profile(profile) {
            return Some(format!(
                "`{profile}` is a model of restricted origin, and this turn is not cleared for it"
            ));
        }
        (floor.sensitive && self.uncleared_profile(profile)).then(|| REFUSED.to_string())
    }

    /// The route's answer, kept where the floor lets the turn run: the
    /// decision stands when the floor lets its profile (else the turn's own)
    /// run, else the `[model]` default runs if the floor lets it and it is
    /// usable, else the turn ends.
    pub(super) fn data_floor(
        &self,
        t: &Turn<'_>,
        decision: Option<Decision>,
    ) -> Result<Option<Decision>, Failure> {
        let floor = t.route.floor();
        let ran = decision
            .as_ref()
            .map_or(t.target.profile.as_str(), |d| d.profile.as_str());
        let Some(why) = self.floor_refuses(floor, ran) else {
            return Ok(decision);
        };
        let usable = self
            .route_profiles(floor)
            .get(DEFAULT_PROFILE)
            .is_some_and(|p| p.unusable.is_none());
        if usable {
            return Ok(Some(Decision {
                profile: DEFAULT_PROFILE.into(),
                reason: Reason::DataGate,
                detour: false,
                switch: false,
                hold: HoldNext::Keep,
            }));
        }
        Err(refusal(&why))
    }

    /// A turn that asked no route, at its first loop: a base the floor keeps
    /// it off moves to the `[model]` default (the decision, which the caller
    /// records), or the turn ends here.
    pub(super) fn floor_unrouted<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        provider: &mut Option<Arc<dyn Provider>>,
        slot: &'a OnceLock<Target>,
    ) -> Result<Result<Option<Decision>, Failure>> {
        let d = match self.data_floor(t, None) {
            Ok(Some(d)) => d,
            Ok(None) => return Ok(Ok(None)),
            Err(f) => return Ok(Err(f)),
        };
        let to = self.resolve_target(&t.target.profile, Some(&d.profile), None, None);
        let Some((p, to)) = to
            .ok()
            .and_then(|to| Some((self.providers.get(&to.provider)?, to.speaking_as(t.target))))
        else {
            return Ok(Err(refusal(&format!(
                "`{DEFAULT_PROFILE}` does not resolve"
            ))));
        };
        let p = p.clone();
        self.route_to(t, session, spec, slot, to, &d)?;
        *provider = Some(p);
        Ok(Ok(Some(d)))
    }

    /// The seam where a turn's pass rides its request (theseus-a1wvq.3):
    /// a cleared turn's request carries a pass for its own digest. No turn
    /// is cleared yet (the open-request row clears one), so none does.
    pub(super) fn data_pass(t: &Turn<'_>, compiled: &mut Compiled) {
        if t.route.clearance.is_cleared() {
            let digest = compiled.request.digest();
            compiled.request.data_pass = t.route.clearance.pass(&digest);
        }
    }
}

/// The turn ends before any model call: no model cleared for its data.
fn refusal(why: &str) -> Failure {
    let said = format!("No model cleared for this request's data is usable: {why}");
    Failure {
        class: crate::data_gate::DATA_GATE_CLASS.into(),
        transient: false,
        usage_unknown: false,
        reason: format!("{}: {said}", crate::data_gate::DATA_GATE_CLASS),
        source: anyhow::anyhow!("{said}"),
    }
}
