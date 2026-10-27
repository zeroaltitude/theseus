//! The route step's data floor (theseus-oejh3, theseus-xs5ah, theseus-a1wvq.3,
//! theseus-9ymz0).
//!
//! - **Restricted origin**: a profile whose model is of restricted origin
//!   (`cn` or `unknown`, `Config::profile_origin`) runs only for a session
//!   whose labels allow it (`RouteState.cn`): none that disqualifies such a
//!   model (`ContentFlags::disqualifies_cn`), and every item in it judged
//!   (`content_wait::unjudged`). Nothing is proved per request; the session
//!   carries a label or it does not.
//! - **Waiting** (design §4.3): a turn bound for a restricted profile whose
//!   session has items not yet judged waits for their verdicts, at most
//!   `[content] wait_ms`, on the judge's per-session notify
//!   (`TurnRunner::verdicts`), then **goes to Claude**: the turn is never
//!   blocked and never fails for want of a verdict. A turn bound for Claude
//!   never waits.
//! - **The sensitive names**: a message that names one of `[privacy]
//!   sensitive_names`, or a session that read private content, runs only on a
//!   profile of US origin (`ModelOrigin::cleared_for_sensitive`).
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
use crate::content::ContentFlags;
use crate::provider::Provider;
use crate::routing::{Decision, HoldNext, Reason};

/// The profile the `[model]` section builds.
pub(crate) const DEFAULT_PROFILE: &str = "default";

pub(super) const REFUSED: &str =
    "this message names something in `[privacy] sensitive_names`, and \
                                  no profile of US origin is configured for it";

/// How many times a wait for verdicts **sweeps**: reads the record and the
/// whole transcript again and offers what nothing has asked for. A wake-up
/// that judged only some of the items leaves the rest to the next one, and a
/// notify that answers at once (the stand-in's) must not spin.
///
/// It bounds the sweeps and **not the wait** (theseus-8qgwo): the wait's own
/// bound is `[content] wait_ms`. Bounding both meant a session whose landings
/// were failing — each of which wakes the wait — spent its eight reads at once
/// and was refused with nearly all of a 30-second `wait_ms` unspent.
const SETTLE_READS: u32 = 8;

/// Where a turn stands on its session's labels, as the floor reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Cn {
    /// Not read yet: fail closed.
    #[default]
    Unchecked,
    /// No label keeps the session off a restricted model and every item was
    /// judged.
    Allowed,
    /// Items are not yet judged; a turn bound for a restricted profile waits.
    Pending,
    /// Refused, and the word for it: a flag's (`confidential`), `unjudged` when
    /// the wait ran out, `no_record` when the session did not read.
    No(&'static str),
}

impl Cn {
    /// The word for the route row and the result (`TurnData.cn`).
    pub(crate) fn word(self) -> &'static str {
        match self {
            Cn::Allowed => "allowed",
            Cn::Unchecked => "unchecked",
            Cn::Pending => "pending",
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
    /// The names' floor: only a profile of US origin.
    pub(super) sensitive: bool,
    /// The session's labels allow a profile of restricted origin.
    pub(super) cn_ok: bool,
}

impl super::route_step::RouteState {
    /// The turn's floor: its names and its labels.
    pub(super) fn floor(&self) -> Floor {
        Floor {
            sensitive: self.sensitive,
            cn_ok: self.cn == Cn::Allowed || self.assume_cn,
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

    /// The data level of the call the turn is about to make: its session's own
    /// marks, a recall that admitted a marked session's note, and the task
    /// lineage. The floors read it as private or above; the chokepoint
    /// carries the level itself.
    pub(super) fn session_level(&self, t: &Turn<'_>) -> crate::google::private::Level {
        use crate::google::private::{self, Level};
        let own = private::level(t.tc.store, &self.cfg, t.tc.session_id);
        own.max(Level::from(self.private_session(t)))
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
        if !floor.cn_ok && self.restricted_profile(profile) {
            return Some(format!(
                "`{profile}` is a model of restricted origin, and this session may not use one"
            ));
        }
        (floor.sensitive && self.uncleared_profile(profile)).then(|| REFUSED.to_string())
    }

    /// The session's labels as the store holds them now, and whether its items
    /// are judged: read from the record, never the turn's copy of it, which a
    /// label raised while the turn ran is not in. No wait.
    pub(super) fn cn_read(t: &Turn<'_>) -> Cn {
        let sid = t.tc.session_id;
        let Ok(Some(rec)) = t.tc.store.get_session::<SessionRecord>(sid) else {
            return Cn::No("no_record");
        };
        let flags: Option<&ContentFlags> = rec.data.as_deref();
        if let Some(flag) = flags.and_then(ContentFlags::disqualifies_cn) {
            return Cn::No(flag.as_str());
        }
        match t.tc.store.transcript(sid) {
            Ok(nodes) if crate::content_wait::unjudged(&nodes, flags) => Cn::Pending,
            Ok(_) => Cn::Allowed,
            Err(_) => Cn::No("no_record"),
        }
    }

    /// `cn_read`, and when items are not yet judged, a wait for their verdicts
    /// of at most `[content] wait_ms`; what is still pending then is refused
    /// (`unjudged`). Only a request bound for a restricted profile asks.
    ///
    /// **The wait runs to `wait_ms`, and [`SETTLE_READS`] bounds the sweeps
    /// inside it, not the wait** (theseus-8qgwo). It used to bound both, and
    /// each landing wakes this loop, so a session whose verdicts were failing
    /// one after another used its eight reads in milliseconds and was refused
    /// with almost all of its `wait_ms` unspent — a 30-second bound that in
    /// practice gave up in three. The reads are the expensive part (a record,
    /// a whole transcript, up to sixteen item builds); waiting is free.
    pub(super) async fn cn_wait(&self, t: &Turn<'_>) -> Cn {
        let sid = t.tc.session_id;
        let deadline = Instant::now() + Duration::from_millis(self.cfg.content.wait_ms);
        let mut cn = Self::cn_read(t);
        let mut swept = false;
        let mut sweeps = 0;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if cn != Cn::Pending || left.is_zero() {
                break;
            }
            // Interest first, so a verdict that lands between the read and the
            // wait is not slept through — and **before the sweep**, which
            // reads the record and the whole transcript: a landing while that
            // ran would be deferred to after this turn, and the wait below
            // would then be for nothing. It is also what marks the session
            // awaited, which is what lets the sweep re-ask a stuck mark.
            let woken = self.verdicts.settled(sid);
            // An item that nothing queued would never land, however long this
            // waited, and its session would be off every model of restricted
            // origin for the rest of its life (theseus-sh1n3). The floor's own
            // reading of the transcript queues what a write site did not, and
            // re-asks what a failed batch left stuck (theseus-8qgwo).
            if sweeps < SETTLE_READS {
                self.judge.ask_unsubmitted(sid);
                swept = true;
                sweeps += 1;
                cn = Self::cn_read(t);
                if cn != Cn::Pending {
                    break;
                }
            }
            // Out of sweeps with nothing in flight: no verdict is coming, and
            // `settled` on an idle session answers at once, so waiting on it
            // would spin to the deadline rather than wait.
            if sweeps >= SETTLE_READS && !self.judge.content_pending(sid) {
                break;
            }
            let _ = tokio::time::timeout(left, woken).await;
            cn = Self::cn_read(t);
        }
        // The loop may not have run its body at all: `wait_ms = 0` is a
        // supported setting meaning "a cn request never waits", and the
        // loop's first act is to break on it. Sweeping only inside would put
        // the one path that makes the blocked set and the submitted set equal
        // by construction behind a config the operator is entitled to choose,
        // and the whole defect class would be back on it (the review of
        // 2026-10-27, B1). This request goes to Claude either way; what the
        // sweep buys is that the next one need not.
        if cn == Cn::Pending && !swept {
            self.judge.ask_unsubmitted(sid);
        }
        match cn {
            Cn::Pending => Cn::No("unjudged"),
            cn => cn,
        }
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
        let cn_moved = !floor.cn_ok && self.restricted_profile(ran);
        t.route.gated = Some(match cn_moved {
            true => t.route.cn.word(),
            false => "sensitive",
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

    /// A later loop on a restricted-origin model reads the session again
    /// before it compiles: a tool's result is an item, so its verdict is
    /// waited for (at most `[content] wait_ms`), and a label raised since
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
        // Nowhere unrestricted to move to: end the turn here with the floor's
        // own words, rather than route to a model the chokepoint will refuse.
        if let Some(no) = self.floor_refuses(t.route.floor(), DEFAULT_PROFILE) {
            t.route.gated = Some(t.route.cn.word());
            return Ok(Some(refusal(&format!(
                "no model cleared for this session's content is usable: {no}"
            ))));
        }
        let d = Decision {
            profile: DEFAULT_PROFILE.into(),
            reason: Reason::DataGate,
            detour: false,
            switch: false,
            hold: HoldNext::Keep,
        };
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
