//! The turn's routing step (M5 25e; `crate::routing`): the route pack's
//! verdict (`route.v3` since theseus-qe3v), asked at `inbound` beside
//! `classify.v1` and `role.v1`, decides which profile the turn runs on, and
//! at what effort.
//!
//! - **Beside the first compile, never before it** ([`beside`]): the call
//!   started at `inbound`, so the turn compiles on the session's profile,
//!   then waits at most `[routing] max_wait_ms` more. A verdict later than
//!   that applies to the next message alone, a detour's (trivial, quick) to none
//!   (`routing::carries`, theseus-6n5j), and this turn says `late`. The
//!   judge off, Jev's breaker open, a slash command, a continuation, a pinned
//!   turn, or `route.v1` in shadow: no wait.
//! - **A switch** rebuilds the spec and compiles again on the routed profile;
//!   the first compile's compilation is persisted, and its `context.compiled`
//!   and `loop.started` recorded, only when the call uses it (theseus-d13v).
//! - **A detour** (`trivial`, `quick`) compiles the persona and the last
//!   `trivial_context_turns` exchanges outside the session's compilation,
//!   which it never writes; the session's profile, `last_target`, and
//!   compilation stay as they were. Its loops record `loop.started` and no
//!   `context.compiled`: its compilation is never stored, so no row names it.
//! - **The effort** (route.v3, `routing::effort`): decided with the route,
//!   for the profile the turn moves to, and set on the spec and the first
//!   loop's request once the compile the call uses is made (`apply_effort`),
//!   so every loop of the turn carries it; the next turn starts from its
//!   profile's own again.
//! - **Recorded** as one `route.decided` row, in the turn's next frame, and
//!   on the turn's result (`TurnSubmitResult.route`).
//! - **A detour sends no recall** (theseus-n7nc): the turn's pending
//!   `Recall` node rides nothing, the reply's footer counts none, and the
//!   recall's row, held until the route is known, says `detoured`
//!   (`TurnRunner::recall_routed`).
//! - **The trace's root follows the move** (theseus-490i): `route_to` sets
//!   its `profile`, `provider` and `model` to the routed target's, so the
//!   exported root and the turn's metrics name the model the call went to.
//! - **Only while it acts** (theseus-9yyr): a session routing moved runs
//!   there while `route.v1` acts live for it, and goes back to its own
//!   profile, its `routed` cleared, once it does not, or once the owner
//!   switches the live profile ([`TurnRunner::route_base`]).
//! - **The owner's correction first** (theseus-q31l, [`correct`]): a turn
//!   the owner's correction or the live layer steers runs there ahead of the
//!   verdict, and does not wait for it.

mod correct;

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use theseus_protocol::route::TurnRoute;

use super::*;
use crate::config::PackMode;
use crate::correction::{By, RoutedTurn, Steer, Steering};
use crate::judge::inbound::{Answered, RouteWait, ROUTE_PACK};
use crate::routing::effort::{self, EffortDecision, Runs};
use crate::routing::{self, Decision, HoldNext, Reason, Verdict};

/// What a turn holds of routing, from its inbound point to its end.
#[derive(Default)]
pub(super) struct RouteState {
    /// The verdict's channel, until the first compile takes it.
    wait: Option<RouteWait>,
    /// `route.v1`'s mode for the turn (live, or shadow for a pin).
    mode: Option<PackMode>,
    /// The mode `route_base` read at the turn's start, which the inbound
    /// point takes rather than reading it again.
    pub(super) read: Option<PackMode>,
    /// The turn's input carries an image.
    images: bool,
    /// Routing is deciding: the first compile does not persist its
    /// compilation (only the one the call uses is).
    pub(super) defer_persist: bool,
    /// The first compile's rows, recorded only when the call uses it
    /// (theseus-d13v).
    pub(super) deferred: Option<Box<super::compile_step::Deferred>>,
    /// A detour: the session's own target, which its record keeps.
    pub(super) keeps: Option<TargetRef>,
    /// What the turn's result says.
    pub(super) result: Option<TurnRoute>,
    /// Where the owner's correction, or the layer, steers the turn ahead of
    /// the verdict (theseus-q31l).
    pub(super) steer: Option<Steering>,
    /// The message's content words, for a correction to find it by.
    pub(super) words: Vec<String>,
    /// The turn is the owner's correction itself: a later correction finds
    /// the turn it corrected, not this one.
    pub(super) correcting: bool,
    /// route.v3's effort for the turn (theseus-qe3v), once decided.
    effort: Option<EffortDecision>,
}

impl RouteState {
    /// Routing has the turn's route to decide: the inbound point asked
    /// `route.v1`, and the first compile has not read its verdict yet.
    pub(super) fn deciding(&self) -> bool {
        self.wait.is_some() || self.defer_persist
    }
}

/// What came of the wait.
#[derive(Debug, Clone, PartialEq)]
pub enum Got {
    /// The verdict, or none (Jev gave none, or nothing was sent).
    Verdict(Option<Verdict>),
    /// The wait ended first.
    Late,
}

/// What came of the wait, how long it took after the compile, and when
/// `route.v1`'s request came back, if it had (theseus-ddbi).
pub struct Waited {
    pub got: Got,
    pub waited: Duration,
    pub answered: Option<tokio::time::Instant>,
}

/// The row's times (theseus-ddbi).
struct Timed {
    waited: Duration,
    late: bool,
    answered_ms: Option<u64>,
}

/// Run `compile`, then wait for the verdict at most `max_wait` after it
/// ends (not waiting at all when `wait` is false): the verdict's call
/// started before the compile, so its time runs beside it.
pub async fn beside<F: Future>(
    compile: F,
    rx: &mut RouteWait,
    max_wait: Duration,
    wait: bool,
) -> (F::Output, Waited) {
    let out = compile.await;
    let t0 = tokio::time::Instant::now();
    let came = match wait {
        true => match tokio::time::timeout(max_wait, &mut *rx).await {
            Ok(Ok(a)) => Some(Some(a)),
            Ok(Err(_)) => Some(None),
            Err(_) => None,
        },
        false => match rx.try_recv() {
            Ok(a) => Some(Some(a)),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => Some(None),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
        },
    };
    let waited = t0.elapsed();
    let (got, answered) = match came {
        Some(Some(a)) => (Got::Verdict(a.verdict), Some(a.at)),
        Some(None) => (Got::Verdict(None), None),
        None => (Got::Late, None),
    };
    (
        out,
        Waited {
            got,
            waited,
            answered,
        },
    )
}

impl TurnRunner {
    /// `route.v1`'s mode for a turn: `[routing]` over the judge's ladder, and
    /// shadow for a turn whose profile the owner chose.
    pub(super) fn route_mode(&self, target: &Target, session: &str) -> PackMode {
        let m = self
            .cfg
            .routing
            .pack_mode(self.judge.mode_for(ROUTE_PACK, session).mode);
        match target.chosen {
            Some(_) => m.min(PackMode::Shadow),
            None => m,
        }
    }

    /// Where a turn starts, routing's state read (theseus-9yyr). A person's
    /// message runs on the session's routed profile only while `route.v1`
    /// acts live for it: `[routing]` on and live, the judge on, the ladder's
    /// rung not rolled back, and Jev reachable. Once it does not, or once the
    /// owner has switched the live profile since the session's last turn
    /// began (`profile.use`, a turn of any kind), the session's `routed` is
    /// cleared, in the turn's own session write (no frame of its own), and the
    /// turn runs on its own profile, as if routing had never moved it. A
    /// message the owner pinned (`-P`, `-p`, `-m`) runs where it names and
    /// changes nothing. Returns the mode it read, which the inbound point
    /// takes rather than reading it again.
    pub(super) fn route_base(
        &self,
        session: &mut SessionRecord,
        target: Target,
        input: bool,
    ) -> (Target, Option<PackMode>) {
        if session.routed.is_none() {
            return (target, None);
        }
        if self.switched_since(session) {
            session.routed = None;
            return (target, None);
        }
        if !input || target.chosen.is_some() {
            return (target, None);
        }
        if !Self::same_base(session, &target) {
            session.routed = None;
            return (target, None);
        }
        let mode = self.route_mode(&target, &session.session_id);
        if mode < PackMode::Canary || !self.judge.reachable() {
            // Before the ladder's warm read route.v1 judges in shadow
            // (theseus-289c): this turn runs at its base, and the move is
            // kept for the read ladder to answer.
            if self.judge.ladder().is_loaded() || !self.judge.config().enabled {
                session.routed = None;
            }
            return (target, Some(mode));
        }
        let routed = session.routed.as_ref().and_then(|r| r.profile.as_deref());
        let target = match routed {
            Some(p) if p != target.profile => self
                .resolve_target(&target.profile, Some(p), None, None)
                .unwrap_or(target),
            _ => target,
        };
        (target, Some(mode))
    }

    /// The turn's base is the one routing moved the session from
    /// (theseus-0j2.17). `target` is where the message runs without routing:
    /// the place's profile or the live one, or the profile the pane carried,
    /// which `turn_submit` reads as the session's `from` when it is only the
    /// routed one. Compared by the profile's name, as a session unrouted
    /// follows its profile: a model changed under the name reaches it there.
    /// A record from before `from` takes the turn's base as it, so it reads
    /// as before.
    fn same_base(session: &mut SessionRecord, target: &Target) -> bool {
        let Some(r) = session.routed.as_mut().filter(|r| r.profile.is_some()) else {
            return true;
        };
        r.from.get_or_insert_with(|| target.profile.clone()) == &target.profile
    }

    /// The owner switched the live profile after the session's last turn
    /// began: routing's move was made before the owner's latest word.
    fn switched_since(&self, session: &SessionRecord) -> bool {
        let at = self.live_switched.at(&self.store);
        at > 0
            && session
                .last_turn_id
                .as_deref()
                .and_then(crate::id_ms)
                .is_none_or(|began| began < at)
    }

    /// The inbound point's answer: the verdict's channel, kept for the first
    /// compile.
    pub(super) fn route_asked(
        t: &mut Turn<'_>,
        wait: Option<RouteWait>,
        mode: PackMode,
        images: bool,
    ) {
        t.route.wait = wait;
        t.route.mode = Some(mode);
        t.route.images = images;
    }

    /// Every configured profile, as routing weighs it: usable when its
    /// provider is built, its key settled (or not on the board), and its
    /// model priced.
    pub(super) fn route_profiles(&self) -> routing::Profiles {
        let states = self.secrets.states();
        self.cfg
            .all_profiles()
            .iter()
            .map(|(name, p)| {
                let entry = self.catalog.get(&p.model);
                let key = self
                    .cfg
                    .all_providers()
                    .get(&p.provider)
                    .map(|pr| pr.api_key_secret.clone());
                let unusable = if !self.providers.contains_key(&p.provider) {
                    Some("provider")
                } else if key.is_some_and(|k| {
                    !matches!(
                        states.get(&k),
                        None | Some(crate::secrets::SecretState::Ready(_))
                    )
                }) {
                    Some("key")
                } else if entry.is_none() {
                    Some("unpriced")
                } else {
                    None
                };
                (
                    name.clone(),
                    routing::Profile {
                        provider: p.provider.clone(),
                        model: p.model.clone(),
                        unusable,
                        short_cost: entry.map(routing::short_cost),
                        vision: entry.is_some_and(|e| e.vision),
                    },
                )
            })
            .collect()
    }

    /// The loop's compile, routed: on the first loop of a turn that asked
    /// `route.v1`, the compile, the wait beside it, and the decision, then
    /// what the turn's recall becomes (`recall_routed`: a detour sends none,
    /// theseus-n7nc); on a detour's later loops, the detour's compile; else
    /// the compile.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn compile_routed<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        provider: &mut Option<Arc<dyn Provider>>,
        slot: &'a OnceLock<Target>,
        force: Option<Recompile>,
        strip: Option<&'static str>,
        overflow: Option<&Overflowed>,
        i: u32,
    ) -> Result<Result<Compiled, Failure>> {
        if t.route.keeps.is_some() {
            return self.compile_detour(t, session, spec, i);
        }
        // The compile step's future is boxed (situations' join fix, 35a):
        // inline, the turn's future overflowed a 2 MiB stack in debug builds.
        let Some(rx) = t.route.wait.take().filter(|_| i == 0) else {
            return Box::pin(self.compile_step(t, session, spec, force, strip, overflow, i)).await;
        };
        let mut compiled = self
            .compile_first(
                t, session, spec, provider, slot, force, strip, overflow, i, rx,
            )
            .await;
        if let Ok(Ok(c)) = &mut compiled {
            self.apply_effort(t, spec, c);
        }
        Self::recall_routed(t);
        compiled
    }

    /// Jev's effort, when it applies, on the spec (every later loop's
    /// compile) and on the request the first compile made (theseus-qe3v). A
    /// model that takes per-message effort keeps the top-level one, and the
    /// cache of the messages with it: the turn's level is a message before
    /// the turn's own (theseus-o719).
    fn apply_effort(&self, t: &Turn<'_>, spec: &mut RequestSpec, c: &mut Compiled) {
        let Some(e) = t.route.effort.filter(EffortDecision::applied) else {
            return;
        };
        if crate::compiler::effort::takes(spec, &c.request.model) {
            spec.turn_effort = e.ran;
            crate::compiler::effort::retarget(&mut c.request, spec);
            c.messages = c.request.messages.len();
        } else {
            spec.effort = e.ran;
            let entry = self.catalog.get(&c.request.model);
            c.request.output_config = crate::compiler::output_config(entry, spec.effort);
        }
        // The request changed after its compile: what the turn records of it,
        // and the overflow retry's "same request" check, read its digest.
        c.digest = c.request.digest();
    }

    /// The effort rule for the profile the turn moves to (or its own), from
    /// route.v3's answer: none when the verdict has none (route.v1 and v2
    /// ask no effort). `acts` when the turn reads a live verdict.
    fn route_effort(
        &self,
        t: &Turn<'_>,
        v: &Verdict,
        d: Option<&Decision>,
        acts: bool,
    ) -> Option<EffortDecision> {
        let answer = v.effort.as_ref()?;
        let profiles = self.cfg.all_profiles();
        let name = d.map_or(t.target.profile.as_str(), |d| d.profile.as_str());
        let (own, model) = match profiles.get(name).filter(|_| name != t.target.profile) {
            Some(p) => (p.effort, p.model.as_str()),
            None => (t.target.effort, t.target.model.as_str()),
        };
        let runs = Runs {
            own,
            fixed: profiles.get(name).is_some_and(|p| p.effort_fixed),
            takes: self.catalog.get(model).is_some_and(|e| e.effort),
        };
        let carried = v.turn != t.tc.turn_id;
        let bounds = self.cfg.routing.effort_bounds;
        Some(effort::decide(bounds, answer, runs, acts, carried))
    }

    /// The first loop's compile, the wait for the verdict beside it, and the
    /// decision: the first compile kept, a switch's compile, or a detour's.
    #[allow(clippy::too_many_arguments)]
    async fn compile_first<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        provider: &mut Option<Arc<dyn Provider>>,
        slot: &'a OnceLock<Target>,
        force: Option<Recompile>,
        strip: Option<&'static str>,
        overflow: Option<&Overflowed>,
        i: u32,
        mut rx: RouteWait,
    ) -> Result<Result<Compiled, Failure>> {
        let mode = t.route.mode.unwrap_or(PackMode::Shadow);
        let live = mode >= PackMode::Canary && !self.judge.breaker_open();
        // Jev known unreachable (its last try failed to connect, and nothing
        // has answered since): no verdict will come in time, so the turn
        // does not wait for one (theseus-otny).
        let unreachable = live && self.judge.jev_unreachable();
        // A steered turn runs where the owner said: it waits for no verdict.
        let steering = t.route.steer.take().filter(|_| live);
        t.route.defer_persist = live;
        let max_wait = Duration::from_millis(self.cfg.routing.max_wait_ms);
        let compile = Box::pin(self.compile_step(t, session, spec, force, strip, overflow, i));
        let wait = live && !unreachable && steering.is_none();
        let (compiled, w) = beside(compile, &mut rx, max_wait, wait).await;
        // Missed: the turn decides without its own verdict (theseus-ddbi).
        let late = live && steering.is_none() && w.got == Got::Late;
        let answered_ms = w.answered.map(|at| {
            at.into_std()
                .saturating_duration_since(t.started)
                .as_millis() as u64
        });
        if live && steering.is_none() {
            self.judge.record_route_wait(w.waited, late);
        }
        let (got, waited) = (w.got, w.waited);
        t.route.defer_persist = false;
        let compiled = match compiled? {
            Ok(c) => c,
            Err(f) => return Ok(Err(f)),
        };
        let base = t.target.profile.clone();
        let (verdict, reason) = self.read_verdict(
            t,
            got,
            live,
            rx,
            session.last_turn_id.as_deref(),
            unreachable,
        );
        let decision = match (&steering, &verdict, reason) {
            (Some(s), _, _) => Some(self.decide_steer(t, session, s, compiled.est_tokens)),
            (None, Some(v), None) => Some(self.decide_route(t, session, v, compiled.est_tokens)),
            _ => None,
        };
        t.route.effort = verdict
            .as_ref()
            .and_then(|v| self.route_effort(t, v, decision.as_ref(), reason.is_none()));
        Self::record_route(
            t,
            verdict.as_ref(),
            decision.as_ref(),
            reason,
            &compiled,
            Timed {
                waited,
                late,
                answered_ms,
            },
            steering.as_ref(),
        );
        self.note_routed(t, verdict.as_ref(), decision.as_ref());
        if live {
            // A routed turn: the owner's pin of another profile within 10
            // minutes after it counts on route.v1's ladder (26a).
            let ran = decision
                .as_ref()
                .map_or(base.as_str(), |d| d.profile.as_str());
            self.judge.set_routed(t.tc.session_id, ran);
        }
        let Some(d) = decision else {
            return self.keep_first(t, session, compiled, spec, i);
        };
        let routed = session.routed.get_or_insert_with(Default::default);
        match &d.hold {
            HoldNext::Keep => {}
            HoldNext::Clear => routed.hold = None,
            HoldNext::Set(h) => routed.hold = Some(h.clone()),
        }
        if session.routed.as_deref() == Some(&Default::default()) {
            session.routed = None;
        }
        if d.profile == base {
            return self.keep_first(t, session, compiled, spec, i);
        }
        let target = match self.resolve_target(&base, Some(&d.profile), None, None) {
            Ok(tg) => tg,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), profile = %d.profile, "routing: the profile did not resolve; the session's own runs");
                return self.keep_first(t, session, compiled, spec, i);
            }
        };
        let Some(p) = self.providers.get(&target.provider).cloned() else {
            return self.keep_first(t, session, compiled, spec, i);
        };
        // The first compile is not the call's: its rows go with it.
        t.route.deferred = None;
        if d.detour {
            t.route.keeps = Some(TargetRef::from(t.target));
        }
        self.route_to(t, session, spec, slot, target, &d)?;
        *provider = Some(p);
        if d.detour {
            return self.compile_detour(t, session, spec, i);
        }
        Box::pin(self.compile_step(t, session, spec, force, strip, overflow, i)).await
    }

    /// The verdict the turn reads, or why it has none: its own, else a late
    /// one of the session's last message; a pin's or shadow's is recorded
    /// only. A wait that ended first leaves the verdict to the next message.
    /// Every message takes the session's late verdict, and keeps it only when
    /// it was asked for `last_turn`, the session's turn just before this one:
    /// a late verdict applies to the next message alone, never to a later one.
    /// A late detour's verdict (trivial, quick) applies to none: it was its
    /// own message's (`routing::carries`, theseus-6n5j).
    fn read_verdict(
        &self,
        t: &Turn<'_>,
        got: Got,
        live: bool,
        rx: RouteWait,
        last_turn: Option<&str>,
        unreachable: bool,
    ) -> (Option<Verdict>, Option<Reason>) {
        let sid = t.tc.session_id;
        let stale = self.judge.take_late(sid);
        let late = || stale.filter(|v| Some(v.turn.as_str()) == last_turn && routing::carries(v));
        let recorded = match t.target.chosen {
            Some(_) => Reason::Pinned,
            None => Reason::Shadow,
        };
        match (got, live) {
            (Got::Verdict(Some(v)), true) => (Some(v), None),
            (Got::Verdict(v), false) => (v, Some(recorded)),
            (Got::Late, false) => (None, Some(recorded)),
            (Got::Verdict(None), true) => match late() {
                Some(v) => (Some(v), None),
                None => (None, Some(Reason::NoVerdict)),
            },
            (Got::Late, true) => {
                let judge = self.judge.clone();
                let s = sid.to_string();
                tokio::spawn(async move {
                    if let Ok(Answered {
                        verdict: Some(v), ..
                    }) = rx.await
                    {
                        judge.set_late(&s, v);
                    }
                });
                match late() {
                    Some(v) => (Some(v), None),
                    None if unreachable => (None, Some(Reason::Unreachable)),
                    None => (None, Some(Reason::Late)),
                }
            }
        }
    }

    /// The switch rule over the usable profiles, under the place's cap.
    fn decide_route(
        &self,
        t: &Turn<'_>,
        session: &SessionRecord,
        v: &Verdict,
        est_tokens: u64,
    ) -> Decision {
        let profiles = self.route_profiles();
        let cap =
            t.tc.ceiling
                .and_then(|c| c.profile.as_ref())
                .and_then(|p| profiles.get(p))
                .and_then(|p| p.short_cost);
        let hold = session.routed.as_ref().and_then(|r| r.hold.clone());
        routing::decide(
            &self.cfg.routing,
            &profiles,
            &routing::Ask {
                verdict: v,
                base: &t.target.profile,
                est_tokens,
                hold: hold.as_ref(),
                images: t.route.images,
                cap,
            },
        )
    }

    /// The `route.decided` row, and what the turn's result says.
    fn record_route(
        t: &mut Turn<'_>,
        verdict: Option<&Verdict>,
        decision: Option<&Decision>,
        reason: Option<Reason>,
        compiled: &Compiled,
        timed: Timed,
        steering: Option<&Steering>,
    ) {
        let base = t.target.profile.clone();
        let reason = decision.map_or(reason.unwrap_or(Reason::NoVerdict), |d| d.reason);
        let source = steering.map(|_| "correction");
        let profile = decision.map_or(base.clone(), |d| d.profile.clone());
        let answer = verdict.and_then(|v| v.effort.as_ref());
        let e = t.route.effort;
        let applied = e.filter(EffortDecision::applied).and_then(|e| e.ran);
        t.record(&fact::route::RouteDecided {
            mode: verdict.map(|v| v.mode.as_str()),
            confidence: verdict.map(|v| v.confidence),
            judgment: verdict.map(|v| v.judgment.as_str()),
            from: &base,
            profile: &profile,
            reason: reason.as_str(),
            detour: decision.is_some_and(|d| d.detour),
            switch: decision.is_some_and(|d| d.switch),
            est_tokens: compiled.est_tokens,
            wait_ms: timed.waited.as_millis() as u64,
            late: timed.late,
            answered_ms: timed.answered_ms,
            source,
            follows: steering.map(|s| s.follows.as_str()),
            effort: answer.map(|a| a.level.as_str()),
            effort_confidence: answer.map(|a| a.confidence),
            effort_reason: e.map(|e| e.reason.as_str()),
            effort_applied: applied.is_some(),
            effort_ran: e.and_then(|e| e.ran).map(effort::name),
        });
        t.route.result = Some(TurnRoute {
            mode: verdict.map(|v| v.mode.clone()),
            reason: reason.as_str().into(),
            from: base,
            source: source.map(str::to_string),
            effort: answer.map(|a| a.level.clone()),
            effort_reason: e.map(|e| e.reason.as_str().into()),
            effort_applied: applied.map(|e| effort::name(e).into()),
        });
    }

    /// Where the session's record says it runs: its own target through a
    /// detour, where it ran before a refusal's fallback (theseus-7gir.18),
    /// else the turn's.
    pub(super) fn ran_on(t: &Turn<'_>) -> TargetRef {
        let before = t.fallback.as_ref().map(|f| f.ran_on.clone());
        t.route
            .keeps
            .clone()
            .or(before)
            .unwrap_or_else(|| TargetRef::from(t.target))
    }

    /// The first compile is the one the call uses: persist it, and record
    /// its rows, as the compile step would have.
    fn keep_first(
        &self,
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        compiled: Compiled,
        spec: &RequestSpec,
        i: u32,
    ) -> Result<Result<Compiled, Failure>> {
        if compiled.new_compilation
            && session.compilation_id.as_deref() != Some(&compiled.compilation.id)
        {
            Self::persist_compilation(t.tc.store, &compiled, session, t.tc.turn_id)?;
        }
        if let Some(d) = t.route.deferred.take() {
            self.compiled_rows(t, &d.summary, &compiled, spec, (d.c0, d.c1), i);
        }
        // Its compile carries the recall's drops (theseus-3urn).
        t.recall.drops.clear();
        Ok(Ok(compiled))
    }

    /// Move the turn to `target`: its spec, and for a switch the session's
    /// routed profile and `last_target`.
    fn route_to<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        slot: &'a OnceLock<Target>,
        target: Target,
        d: &Decision,
    ) -> Result<()> {
        if slot.set(target).is_err() {
            anyhow::bail!("a turn routes once");
        }
        let base = t.target.profile.clone();
        let target = slot.get().expect("set");
        t.target = target;
        t.tc.target = Some(target);
        // The root names where the call goes, as the call's own span does:
        // the turn's metrics read its model (theseus-490i).
        t.trace.set_root(json!({
            "profile": target.profile,
            "provider": target.provider,
            "model": target.model,
        }));
        let (mut s, _) = self.request_spec(target, session.kind, t.tc.place());
        s.walk = self.walk(t.tc.session_id, t.tc.class);
        *spec = s;
        if d.switch {
            session.last_target = Some(TargetRef::from(target));
            // The base it was moved from, kept through later switches; a
            // switch back to it ends the move (theseus-0j2.17).
            let r = session.routed.get_or_insert_with(Default::default);
            if r.from.get_or_insert(base) == &target.profile {
                (r.profile, r.from) = (None, None);
            } else {
                r.profile = Some(target.profile.clone());
            }
            if session.routed.as_deref() == Some(&Default::default()) {
                session.routed = None;
            }
        }
        Ok(())
    }

    /// A detour's request: the persona and the profile's own header, and the
    /// last `trivial_context_turns` exchanges with the message, compiled
    /// outside the session's compilation, which it never writes. It reads the
    /// transcript without `recall_view`, so the turn's recall reaches no
    /// request, and `recall_routed` drops it.
    fn compile_detour(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        spec: &RequestSpec,
        i: u32,
    ) -> Result<Result<Compiled, Failure>> {
        let sid = t.tc.session_id;
        let nodes = t.tc.store.transcript(sid)?;
        let from = detour_start(&nodes, self.cfg.routing.trivial_context_turns);
        // A detour admits no recall and no summary (35a): its last exchanges
        // and the message.
        let nodes: Vec<_> = nodes[from..]
            .iter()
            .filter(|(_, n)| {
                !matches!(
                    n.kind,
                    crate::stub::Kind::Recall | crate::stub::Kind::Summary
                )
            })
            .cloned()
            .collect();
        let sources = crate::recall::render::Sources::default();
        let mut detour = spec.clone();
        detour.context_text = String::new();
        detour.context_files = Vec::new();
        detour.walk = None;
        let compiled = compile(CompileInput {
            session_id: sid,
            current: None,
            nodes: &nodes,
            last_position: self.store.last_position(),
            spec: &detour,
            catalog: &self.catalog,
            force: None,
            window_override: None,
            blobs: Some(self.store.blobs()),
            hidden: &session.not_shown,
            strip: None,
            overflowed: None,
            sources: &sources,
            signals: None,
            assembled: None,
            situation: &crate::compiler::situation::Situation::Detour,
        });
        let last = self.store.last_position();
        if let Some(f) = Self::unadmitted(t, &compiled, &nodes, last, false, i) {
            return Ok(Err(f));
        }
        t.record(&fact::turn::LoopStarted {
            turn_id: t.tc.turn_id,
            index: i,
            model: &t.target.model,
            tools_offered: detour.tools.len() as u32,
        });
        Ok(Ok(compiled))
    }
}

/// The META key of the owner's last switch of the live profile, in Unix
/// milliseconds (`profile.use`, theseus-9yyr). A new key: no store format.
pub const SWITCHED: &str = "live_profile.switched_ms";

/// The owner's last switch of the live profile, 0 for none: read from
/// [`SWITCHED`] at the first turn of a routed session after a start (never
/// on the start path), and moved by each switch once its record is written.
#[derive(Default)]
pub struct LiveSwitched(OnceLock<AtomicU64>);

impl LiveSwitched {
    pub fn at(&self, store: &crate::store::Store) -> u64 {
        self.0
            .get_or_init(|| {
                let read = store.get_meta::<u64>(SWITCHED);
                AtomicU64::new(read.ok().flatten().unwrap_or(0))
            })
            .load(Ordering::Relaxed)
    }

    pub fn moved(&self, ms: u64) {
        self.0
            .get_or_init(|| AtomicU64::new(ms))
            .store(ms, Ordering::Relaxed);
    }
}

/// Where a detour's nodes begin: at the person's message `turns` messages
/// before the newest one (the turn's own), or the session's start.
pub fn detour_start(nodes: &[(u64, crate::stub::Stub)], turns: u32) -> usize {
    let asks: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, (_, n))| {
            n.origin == crate::node::Origin::Operator && n.kind == crate::stub::Kind::UserMessage
        })
        .map(|(i, _)| i)
        .collect();
    let back = turns as usize;
    match asks.len().checked_sub(back + 1) {
        Some(k) => asks[k],
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(mode: &str) -> Verdict {
        Verdict {
            mode: mode.into(),
            confidence: 0.9,
            effort: None,
            judgment: "jdg_1".into(),
            turn: "turn_1".into(),
        }
    }

    /// The compile takes 100 ms; the verdict comes at `at` ms after the
    /// turn's start, or never. Returns when the call is released, in ms.
    async fn release(at: Option<u64>) -> (u64, Got) {
        let t0 = tokio::time::Instant::now();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            match at {
                Some(ms) => {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    let _ = tx.send(Answered {
                        verdict: Some(v("chat")),
                        at: tokio::time::Instant::now(),
                    });
                }
                None => {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    drop(tx);
                }
            }
        });
        let compile = tokio::time::sleep(Duration::from_millis(100));
        let (_, w) = beside(compile, &mut rx, Duration::from_millis(200), true).await;
        if let Some(ms) = at.filter(|_| w.got != Got::Late) {
            let came = w.answered.map(|a| (a - t0).as_millis() as u64);
            assert_eq!(came, Some(ms), "when the verdict came");
        }
        (t0.elapsed().as_millis() as u64, w.got)
    }

    /// On tokio's paused clock: a verdict that never comes releases the call
    /// exactly `max_wait_ms` after the compile ends; one 50 ms after the
    /// compile, then; one during the compile delays nothing.
    #[tokio::test(start_paused = true)]
    async fn the_wait_runs_beside_the_compile_and_never_past_its_bound() {
        assert_eq!(release(None).await, (300, Got::Late));
        assert_eq!(
            release(Some(150)).await,
            (150, Got::Verdict(Some(v("chat"))))
        );
        assert_eq!(
            release(Some(40)).await,
            (100, Got::Verdict(Some(v("chat"))))
        );
        assert_eq!(release(Some(299)).await.0, 299);
        assert_eq!(release(Some(301)).await, (300, Got::Late));
    }

    /// A dropped sender (nothing sent: the day's limit, a failed prepare)
    /// ends the wait at once; not waiting reads only what is there.
    #[tokio::test(start_paused = true)]
    async fn nothing_sent_waits_for_nothing() {
        let t0 = tokio::time::Instant::now();
        let (tx, mut rx) = tokio::sync::oneshot::channel::<Answered>();
        drop(tx);
        let (_, w) = beside(async {}, &mut rx, Duration::from_millis(200), true).await;
        assert_eq!((t0.elapsed().as_millis(), w.got), (0, Got::Verdict(None)));
        assert!(w.answered.is_none());
        let (_tx, mut rx) = tokio::sync::oneshot::channel::<Answered>();
        let (_, w) = beside(async {}, &mut rx, Duration::from_millis(200), false).await;
        assert_eq!((t0.elapsed().as_millis(), w.got), (0, Got::Late));
    }

    /// An id's time is when `new_id` minted it, which `switched_since` reads
    /// from the session's last turn's id.
    #[test]
    fn an_ids_time_is_when_new_id_minted_it() {
        let before = theseus_protocol::now_unix_ms();
        let id = crate::new_id("turn");
        let after = theseus_protocol::now_unix_ms();
        let ms = crate::id_ms(&id).unwrap();
        assert!((before..=after).contains(&ms), "{before} {ms} {after}");
        assert_eq!(crate::id_ms("turn_c3"), None);
        assert_eq!(crate::id_ms("ses_old2"), None);
    }

    #[test]
    fn a_detour_takes_the_last_exchanges_and_the_message() {
        let n = |text: &str, op: bool| {
            let mut node = Node::user_with("ses_1", None, "cli", text, vec![]);
            if !op {
                node.origin = crate::node::Origin::Agent;
            }
            (0u64, node.into())
        };
        let nodes = vec![
            n("one", true),
            n("reply", false),
            n("two", true),
            n("reply", false),
            n("three", true),
            n("reply", false),
            n("thanks", true),
        ];
        assert_eq!(detour_start(&nodes, 2), 2);
        assert_eq!(detour_start(&nodes, 0), 6);
        assert_eq!(detour_start(&nodes, 9), 0);
    }
}
