//! The route step's data floor (theseus-oejh3, theseus-xs5ah, theseus-a1wvq.3).
//!
//! - **Restricted origin**: a profile whose model is of restricted origin
//!   (`cn` or `unknown`, `Config::profile_origin`) runs only for a turn
//!   admitted open (`RouteState.admitted`, `open_step.rs`) whose request will
//!   be compiled in the open view: an open session, or a detour. Elsewhere
//!   an admitted turn's data reason is `private_view`.
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
use theseus_protocol::route::{TurnData, TurnRoute};

/// The profile the `[model]` section builds.
pub(crate) const DEFAULT_PROFILE: &str = "default";

pub(super) const REFUSED: &str =
    "this message names something in `[privacy] sensitive_names`, and \
                                  no profile of US origin is configured for it";

/// What the floor keeps a turn off, as `route_profiles` reads it.
#[derive(Clone, Copy)]
pub(super) struct Floor {
    /// The names' floor: only a profile of US origin.
    pub(super) sensitive: bool,
    /// The turn is admitted open: a profile of restricted origin may run.
    pub(super) cleared: bool,
}

impl super::route_step::RouteState {
    /// The turn's floor: its names and its admission.
    pub(super) fn floor(&self) -> Floor {
        Floor {
            sensitive: self.sensitive,
            cleared: self.admitted.0.is_ok() || self.assume_cleared,
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
    /// A turn whose recall admitted a private session's note is private
    /// from then on (theseus-39dj0), before its frame writes the mark. A
    /// task's turn is private while its parent, or any task above it, is,
    /// however late it took the mark (theseus-y8k3u): reads only on a task's
    /// turn, and none without `[google]`.
    pub(super) fn private_session(&self, t: &Turn<'_>) -> bool {
        t.recall.private
            || crate::google::private::session(t.tc.store, &self.cfg, t.tc.session_id)
            || t.tc.task.is_some_and(|k| {
                crate::google::private::lineage(t.tc.store, &self.cfg, &k.parent_session)
            })
    }

    /// Whether `profile` may not be sent the sensitive names: its origin is
    /// not on the allowlist.
    pub(super) fn uncleared_profile(&self, profile: &str) -> bool {
        crate::data_mark::uncleared(&self.cfg, &self.catalog, profile)
    }

    /// Whether `profile`'s model is of restricted origin (a profile that does
    /// not resolve is).
    pub(crate) fn restricted_profile(&self, profile: &str) -> bool {
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

    /// The provider, model and origin word of `profile`, for the rows that name
    /// where a turn ran: a profile that does not resolve has no provider or
    /// model and origin `unknown`.
    pub(super) fn ran_of(&self, profile: &str) -> fact::route::Ran<'_> {
        let p = self.cfg.all_profiles().get(profile);
        fact::route::Ran {
            provider: p.map(|p| p.provider.as_str()),
            model: p.map(|p| p.model.as_str()),
            origin: crate::data_gate::word(self.cfg.profile_origin(profile, &self.catalog)),
        }
    }

    /// Why the data rule kept the turn off a restricted-origin profile, for
    /// its route row, when it did: the floor's own word where it moved the
    /// decision, else the admission's, when the decision, made again as for a
    /// cleared ask, names a restricted profile an unadmitted turn was left
    /// without. A turn that made no decision here (a brief's, one with no
    /// verdict, a checkpoint's) walks the mode's list instead.
    pub(super) fn data_gate_why(
        &self,
        t: &mut Turn<'_>,
        mode: Option<&str>,
    ) -> Option<&'static str> {
        t.route.gated.take().or_else(|| {
            let why = t.route.admitted.0.as_ref().err()?;
            let won = match t.route.cleared_pick {
                Some(won) => won,
                None => self.cn_would_win(t, mode?),
            };
            won.then(|| why.as_str())
        })
    }

    /// Whether the mode's list, walked as for a cleared ask, would have taken
    /// a restricted profile: only then did the admission cost the turn a
    /// model (theseus-4h85l). A US model that wins either way is no gate. The
    /// fallback for a turn with no decision of its own to ask again.
    fn cn_would_win(&self, t: &Turn<'_>, mode: &str) -> bool {
        let floor = Floor {
            cleared: true,
            ..t.route.floor()
        };
        let profiles = self.route_profiles(floor);
        let list = self.cfg.routing.classes.effective(mode, true);
        let cap = Self::place_cap(t, &profiles);
        let (crate::routing::Picked::Profile(p) | crate::routing::Picked::Capped(Some(p))) =
            crate::routing::pick(&list, &profiles, t.route.images, cap, true)
        else {
            return false;
        };
        profiles[&p].restricted
    }

    /// A request left with a pass: the turn's result says `open`, from then
    /// on (theseus-a1wvq.11). A turn that asked no route has no result yet,
    /// so it takes one for its base, with no reason: the surfaces word a
    /// route only from one (theseus-4h85l).
    pub(super) fn note_open(&self, t: &mut Turn<'_>, compiled: &Compiled) {
        if compiled.request.data_pass.is_none() {
            return;
        }
        let profile = t.target.profile.clone();
        open_route(&mut t.route.result, &profile, self.ran_of(&profile).origin);
    }

    /// The route's answer, kept where the floor lets the turn run: the
    /// decision stands when the floor lets its profile (else the turn's own)
    /// run, else the `[model]` default runs if the floor lets it and it is
    /// usable, else the turn ends. An admitted turn runs on a restricted
    /// profile only where its request is open: an open session, or a detour
    /// (theseus-a1wvq.8); elsewhere its data reason becomes `private_view`.
    pub(super) fn data_floor(
        &self,
        t: &mut Turn<'_>,
        decision: Option<Decision>,
    ) -> Result<Option<Decision>, Failure> {
        let mut floor = t.route.floor();
        let open = t.tc.open.is_some() || decision.as_ref().is_some_and(|d| d.detour);
        floor.cleared &= open;
        let ran = decision
            .as_ref()
            .map_or(t.target.profile.as_str(), |d| d.profile.as_str());
        let Some(why) = self.floor_refuses(floor, ran) else {
            return Ok(decision);
        };
        if let Some(f) = Self::draft_stopped(t, &why) {
            return Err(f);
        }
        if t.route.floor().cleared && !open {
            t.route.admitted = super::open_step::Admitted(Err(crate::data::DataWhy::PrivateView));
        }
        let clearance_moved = !floor.cleared && self.restricted_profile(ran);
        t.route.gated = Some(match (&t.route.admitted.0, clearance_moved) {
            (Err(why), true) => why.as_str(),
            _ => "sensitive",
        });
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
}

/// The turn ends before any model call: no model cleared for its data.
pub(super) fn refusal(why: &str) -> Failure {
    let said = format!("No model cleared for this request's data is usable: {why}");
    Failure {
        class: crate::data_gate::DATA_GATE_CLASS.into(),
        transient: false,
        usage_unknown: false,
        reason: format!("{}: {said}", crate::data_gate::DATA_GATE_CLASS),
        source: anyhow::anyhow!("{said}"),
    }
}

/// The turn's result once a request of it carried a pass: `open`, on the
/// route it has or on one for `from` that names no reason (theseus-4h85l).
pub(crate) fn open_route(result: &mut Option<TurnRoute>, from: &str, origin: &str) {
    let r = result.get_or_insert_with(|| TurnRoute {
        from: from.into(),
        ..Default::default()
    });
    let d = r.data.get_or_insert_with(|| TurnData {
        origin: origin.into(),
        ..Default::default()
    });
    d.clearance = "open".into();
}

#[cfg(test)]
#[path = "tests_sticky.rs"]
mod tests_sticky;
