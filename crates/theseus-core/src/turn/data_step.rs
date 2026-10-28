//! The route step's data floor (theseus-oejh3, theseus-xs5ah, theseus-a1wvq.3,
//! theseus-9ymz0).
//!
//! - **Restricted origin**: a profile whose model is of restricted origin
//!   (`cn` or `unknown`, `Config::profile_origin`) runs only where
//!   `[content_based_security] cn_models` allows it: `never` skips it in the
//!   route; `always` runs it; `follow_flags` (the default) runs it for a
//!   session that carries no label that disqualifies such a model
//!   (`ContentFlags::disqualifies_cn`) (`RouteState.cn`). The session's record
//!   is read, nothing else: no transcript, no proof that an item was judged
//!   (theseus-gyjyo.6).
//! - **Waiting** (design §4.3): a turn bound for a restricted profile whose
//!   session has content asks still outstanding waits for them to land, at
//!   most `[content_based_security] wait_ms`, on the judge's counter
//!   (`TurnRunner::verdicts`), then **goes either way** on the labels it
//!   reads: the turn is never blocked and never fails for want of a verdict. A
//!   turn bound for Claude never waits.
//! - **Nothing else**: a name in `[privacy] sensitive_names` (theseus-gyjyo.4)
//!   and a Google result (theseus-gyjyo.5) are no part of the floor; each raises
//!   a removable label where the item enters, and the labels are what this reads.
//!
//! The route skips every profile the floor refuses (`route_profiles`), so the
//! next usable one in the mode's list runs; if the turn would still run on
//! one (its base, a pin, a hold, a correction, a checkpoint's move), the
//! `[model]` default runs when the floor lets it, with `Reason::DataGate`, and
//! otherwise the turn ends before any model call. A turn that asked no route
//! is floored at its first loop (`floor_unrouted`), and a later loop on a
//! restricted profile is read again before it compiles (`cn_move`), so a
//! label that landed while the turn ran moves it rather than failing it at
//! the provider chokepoint (`data_gate.rs`), which stays the floor under this
//! for every other caller.

use super::*;
use crate::config::content_based_security::CnModels;
use crate::content::ContentFlags;
use crate::provider::Provider;
use crate::routing::{Decision, HoldNext, Reason};

/// The profile the `[model]` section builds.
pub(crate) const DEFAULT_PROFILE: &str = "default";

/// Where a turn stands on its session's labels, as the floor reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Cn {
    /// Not read yet: fail closed.
    #[default]
    Unchecked,
    /// Sent: `cn_models` is `always`, or it follows the flags and none keeps
    /// the session off a restricted model.
    Allowed,
    /// Refused, and the word for it: a flag's (`confidential`), `never` when
    /// `cn_models` says so, `no_record` when the session did not read.
    No(&'static str),
}

impl Cn {
    /// The word for the route row and the result (`TurnData.cn`).
    pub(crate) fn word(self) -> &'static str {
        match self {
            Cn::Allowed => "allowed",
            Cn::Unchecked => "unchecked",
            Cn::No(w) => w,
        }
    }
}

/// The targets a turn may move to, each set once: routing's, a refusal's
/// fallback, its checkpoints', and a later loop's move off a restricted
/// model (`cn_move`).
pub(super) type Slots = (
    OnceLock<Target>,
    OnceLock<Target>,
    super::checkpoint_step::Slots,
    OnceLock<Target>,
);

pub(super) fn slots() -> Slots {
    (
        OnceLock::new(),
        OnceLock::new(),
        super::checkpoint_step::slots(),
        OnceLock::new(),
    )
}

/// What the floor keeps a turn off, as `route_profiles` reads it.
#[derive(Clone, Copy)]
pub(super) struct Floor {
    /// The session's labels allow a profile of restricted origin.
    pub(super) cn_ok: bool,
}

impl super::route_step::RouteState {
    /// The turn's floor: its labels.
    pub(super) fn floor(&self) -> Floor {
        Floor {
            cn_ok: self.cn == Cn::Allowed || self.assume_cn,
        }
    }
}

impl TurnRunner {
    /// Whether `profile`'s model is of restricted origin (a profile that does
    /// not resolve is).
    pub(crate) fn restricted_profile(&self, profile: &str) -> bool {
        self.cfg.profile_origin(profile, &self.catalog).restricted()
    }

    /// Why a session marked `mark` (`ContentFlags::mark`) cannot run on
    /// `profile`, if it cannot: the profile does not resolve, or it is of
    /// restricted origin and the session's labels keep such a model off. The
    /// check `session.profile` makes and a model menu greys out by; the floor
    /// below it is the same question.
    pub(crate) fn profile_refusal(
        &self,
        mark: theseus_protocol::ops::DataMark,
        profile: &str,
    ) -> Option<String> {
        if !self.cfg.all_profiles().contains_key(profile) {
            return Some(format!("`{profile}` is not a configured profile"));
        }
        (mark != theseus_protocol::ops::DataMark::Open && self.restricted_profile(profile)).then(
            || {
                format!(
                    "this conversation carries a label that keeps a model of restricted origin off it, and `{profile}` is one"
                )
            },
        )
    }

    /// What an open session's restricted-origin profile cannot promise: the
    /// floor moves a turn once a label lands, so the profile may not hold.
    pub(crate) fn per_turn_note(
        &self,
        mark: theseus_protocol::ops::DataMark,
        profile: &str,
    ) -> Option<String> {
        (mark == theseus_protocol::ops::DataMark::Open && self.restricted_profile(profile)).then(
            || {
                format!(
                    "`{profile}` is a model of restricted origin: the floor decides each turn and may move a turn that is not cleared for it"
                )
            },
        )
    }

    /// Why the floor keeps the turn off `profile`, if it does.
    pub(super) fn floor_refuses(&self, floor: Floor, profile: &str) -> Option<String> {
        (!floor.cn_ok && self.restricted_profile(profile)).then(|| {
            format!("`{profile}` is a model of restricted origin, and this session may not use one")
        })
    }

    /// Where the session stands on a restricted model, as the store holds it
    /// now: read from the record, never the turn's copy of it, which a label
    /// raised while the turn ran is not in. One read of the record under
    /// `follow_flags`, none under `always` or `never`. No wait.
    pub(super) fn cn_read(&self, t: &Turn<'_>) -> Cn {
        match self.cfg.content_based_security.cn_models {
            CnModels::Always => return Cn::Allowed,
            CnModels::Never => return Cn::No("never"),
            CnModels::FollowFlags => {}
        }
        let Ok(Some(rec)) = t.tc.store.get_session::<SessionRecord>(t.tc.session_id) else {
            return Cn::No("no_record");
        };
        match rec.data.as_deref().and_then(ContentFlags::disqualifies_cn) {
            Some(flag) => Cn::No(flag.as_str()),
            None => Cn::Allowed,
        }
    }

    /// `cn_read`, and when the labels allow a restricted profile, a wait of at
    /// most `[content_based_security] wait_ms` for the content asks still out
    /// for the session to land, then `cn_read` again: **the turn goes either
    /// way on what the labels say then**. A label already on the session
    /// refuses it whatever lands, and `always` and `never` read no label, so
    /// neither waits. Only a request bound for a restricted profile asks.
    ///
    /// The wait is a counter and nothing else (theseus-gyjyo.7): no transcript
    /// is read and nothing is queued here. An item whose ask failed has been
    /// asked once more inside its own open ask, so by the time the count is
    /// zero the ask has either landed or been rowed and told; a session whose
    /// ask failed twice goes on the labels it has.
    pub(super) async fn cn_wait(&self, t: &Turn<'_>) -> Cn {
        let cn = self.cn_read(t);
        if cn != Cn::Allowed || self.cfg.content_based_security.cn_models != CnModels::FollowFlags {
            return cn;
        }
        let wait = Duration::from_millis(self.cfg.content_based_security.wait_ms);
        self.verdicts.drained(t.tc.session_id, wait).await;
        self.cn_read(t)
    }

    /// `cn_wait`, kept on the turn for the floor and its route row.
    pub(super) async fn cn_settle(&self, t: &mut Turn<'_>) {
        t.route.cn = self.cn_wait(t).await;
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

    /// The route's answer, kept where the floor lets the turn run: the
    /// decision stands when the floor lets its profile (else the turn's own)
    /// run, else the `[model]` default runs if the floor lets it and it is
    /// usable, else the turn ends (a draft's ends rather than leave its model).
    pub(super) fn data_floor(
        &self,
        t: &mut Turn<'_>,
        decision: Option<Decision>,
    ) -> Result<Option<Decision>, Failure> {
        let floor = t.route.floor();
        let ran = decision
            .as_ref()
            .map_or(t.target.profile.as_str(), |d| d.profile.as_str());
        let Some(why) = self.floor_refuses(floor, ran) else {
            return Ok(decision);
        };
        if let Some(f) = Self::draft_stopped(t, &why, true) {
            return Err(f);
        }
        t.route.gated = Some(t.route.cn.word());
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

    /// A later loop on a restricted-origin model reads the session again
    /// before it compiles: a tool's result is an item, so its verdict is
    /// waited for (at most `[content_based_security] wait_ms`), and a label raised since
    /// moves the turn to the `[model]` default with `data_gate`, as a plain
    /// route does, instead of failing it at the chokepoint.
    ///
    /// **The move reads the default's own origin** (theseus-8qgwo). It used to
    /// route there on nothing but "it resolves", and where the `[model]`
    /// default is itself of restricted origin — which is how every bench cell
    /// runs, and anyone who pins one model — the move landed back on the model
    /// the floor had just refused, and the chokepoint failed the whole turn
    /// with `nothing was sent`. The floor, not the chokepoint, decides: a turn
    /// with nowhere unrestricted to go ends here, saying so.
    pub(super) async fn cn_move<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        provider: &mut Option<Arc<dyn Provider>>,
        slot: &'a OnceLock<Target>,
    ) -> Result<Option<Failure>> {
        if !self.restricted_profile(&t.target.profile) {
            return Ok(None);
        }
        self.cn_settle(t).await;
        if t.route.cn == Cn::Allowed {
            return Ok(None);
        }
        let why = format!("its session's labels: {}", t.route.cn.word());
        if let Some(f) = Self::draft_stopped(t, &why, false) {
            return Ok(Some(f));
        }
        let d = Decision {
            profile: DEFAULT_PROFILE.into(),
            reason: Reason::DataGate,
            detour: false,
            switch: false,
            hold: HoldNext::Keep,
        };
        // Nowhere unrestricted to move to: end the turn here with the floor's
        // own words, rather than route to a model the chokepoint will refuse.
        //
        // The route row is written first (`record_route`), as the move below
        // does. It is the only place `t.route.gated` is consumed and the only
        // place `data_gate` is counted, so returning before it left the gate
        // that killed the turn invisible to the ledger, to `efficiency.json`
        // and so to the bench's own `cn: {gated, reasons}` accounting — the
        // counter this row is graded on. `git-multibranch__QjwxfPB`, the one
        // cell-8 trial that already took a floor refusal, has zero route rows
        // and `gated: 0` for exactly that reason.
        if let Some(no) = self.floor_refuses(t.route.floor(), DEFAULT_PROFILE) {
            t.route.gated = Some(t.route.cn.word());
            self.record_route(t, None, Some(&d), None, Default::default(), None);
            return Ok(Some(refusal(&no)));
        }
        let to = self.resolve_target(&t.target.profile, Some(&d.profile), None, None);
        let Some((p, to)) = to
            .ok()
            .and_then(|to| Some((self.providers.get(&to.provider)?, to.speaking_as(t.target))))
        else {
            return Ok(Some(refusal(&format!("`{}` does not resolve", d.profile))));
        };
        let p = p.clone();
        t.route.gated = Some(t.route.cn.word());
        self.record_route(t, None, Some(&d), None, Default::default(), None);
        self.route_to(t, session, spec, slot, to, &d)?;
        *provider = Some(p);
        Ok(None)
    }

    /// A turn that asked no route, at its first loop: a base the floor keeps
    /// it off moves to the `[model]` default (the decision, which the caller
    /// records), or the turn ends here.
    pub(super) async fn floor_unrouted<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        provider: &mut Option<Arc<dyn Provider>>,
        slot: &'a OnceLock<Target>,
    ) -> Result<Result<Option<Decision>, Failure>> {
        if self.restricted_profile(&t.target.profile) {
            self.cn_settle(t).await;
        }
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

/// The turn ends before any model call: no model its session may use is usable.
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

#[cfg(test)]
#[path = "tests_sticky.rs"]
mod tests_sticky;
