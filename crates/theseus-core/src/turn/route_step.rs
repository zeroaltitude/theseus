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
//!   `context.compiled`: its compilation is never stored, so its row is
//!   `context.detoured` (names and counts, in the same frame), which
//!   `context.explain` reads (theseus-r5ydi). The conversation's project slice
//!   rides it as it rides any request (theseus-v4u66).
//! - **The effort** (route.v3, `routing::effort`): decided with the route,
//!   for the profile the turn moves to, and set on the spec and the first
//!   loop's request once the compile the call uses is made (`apply_effort`),
//!   so every loop of the turn carries it; the next turn starts from its
//!   profile's own again.
//! - **Recorded** as one `route.decided` row, in the turn's next frame, and
//!   on the turn's result (`TurnSubmitResult.route`).
//! - **A trivial detour sends no recall** (theseus-n7nc): the turn's pending
//!   `Recall` node rides nothing, the reply's footer counts none, and the
//!   recall's row, held until the route is known, says `detoured`
//!   (`TurnRunner::recall_routed`). **A quick one sends it**
//!   (theseus-pa1xx, `config::routing::detour_recalls`): a quick question is
//!   often about what was said or decided before, so its request carries the
//!   turn's own `Recall` node after the message, as a kept compile's does.
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
mod fan_out_cue;

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use theseus_protocol::route::{TurnData, TurnRoute};

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
    pub(super) wait: Option<RouteWait>,
    /// `route.v1`'s mode for the turn (live, or shadow for a pin).
    pub(super) mode: Option<PackMode>,
    /// The mode `route_base` read at the turn's start, which the inbound
    /// point takes rather than reading it again.
    pub(super) read: Option<PackMode>,
    /// The turn's input carries an image.
    pub(super) images: bool,
    /// Routing is deciding: the first compile does not persist its
    /// compilation (only the one the call uses is).
    pub(super) defer_persist: bool,
    /// The first compile's rows, recorded only when the call uses it
    /// (theseus-d13v).
    pub(super) deferred: Option<Box<super::compile_step::Deferred>>,
    /// A detour: the session's own target, which its record keeps.
    pub(super) keeps: Option<TargetRef>,
    /// The detour's request carries the turn's recall (a quick one,
    /// theseus-pa1xx).
    pub(super) detour_recall: bool,
    /// The mode that sent the turn on a detour (`trivial`, `quick`), for the
    /// `context.detoured` row (theseus-r5ydi).
    pub(super) detour_kind: String,
    /// What the turn's result says.
    pub(super) result: Option<TurnRoute>,
    /// What the turn injects, fixed at the pack or with `result` (theseus-d9je7).
    pub(super) injection: Option<super::tier::InjectionBudget>,
    /// Where the owner's correction, or the layer, steers the turn ahead of
    /// the verdict (theseus-q31l).
    pub(super) steer: Option<Steering>,
    /// The message's content words, for a correction to find it by.
    pub(super) words: Vec<String>,
    /// The message names one of `[privacy] sensitive_names`: a profile not of
    /// US origin is unusable for the turn (`data_step.rs`).
    pub(super) sensitive: bool,
    /// Where the turn stands on its session's content labels
    /// (`data_step::Cn`): unchecked at the turn's start, which fails closed.
    pub(super) cn: super::data_step::Cn,
    /// Why the floor moved the turn off a restricted-origin profile, for its
    /// route row (`data_step`, theseus-a1wvq.11).
    pub(super) gated: Option<&'static str>,
    /// While the counterfactual decision runs (`decide_floored`), the turn's
    /// floor reads the session's labels as allowing a restricted profile.
    pub(super) assume_cn: bool,
    /// The message's text, its first characters: what a model without tool
    /// search is given the tools for (theseus-jpjcd).
    pub(super) ask: String,
    /// Who sent the message (the client's or the binding's person), for the
    /// owner's warning of a reach for their personal details (theseus-i1z13).
    pub(super) asker: String,
    /// The turn's verdict held it: a reach for the owner's personal details
    /// in a shared place, answered without a model call (`personal_step`).
    pub(super) held: bool,
    /// The turn is the owner's correction itself: a later correction finds
    /// the turn it corrected, not this one.
    pub(super) correcting: bool,
    /// route.v3's effort for the turn (theseus-qe3v), once decided.
    pub(super) effort: Option<EffortDecision>,
    /// The turn's persona choices, prior and address (theseus-c1uz), while
    /// two or more personas could answer; and the persona once decided.
    pub(super) engage: Option<super::persona_engage::Engage>,
    pub(super) persona: Option<crate::engage::Engaged>,
    /// A persona correction or the layer's entry placing the persona ahead of
    /// Jev's pick (theseus-c1uz).
    pub(super) persona_steer: Option<Steering>,
    /// The turn is a task's first, and the verdict on its brief picks the
    /// task's base (theseus-ypnxt.7, `brief_step`).
    pub(super) brief: bool,
    /// Jev's families judgment of the message, pending or taken (theseus-xlri5).
    pub(super) families: super::families_step::Judged,
    /// Why a class with `cn_draft` did not draft (theseus-a1wvq.13), for the
    /// route row; and the line of a turn that opened a draft instead of a
    /// model call (`draft_step.rs`).
    pub(super) draft_skip: Option<&'static str>,
    /// What Jev answered to `draft` (`cheap` or `strong`), when the operator
    /// lets Jev choose (theseus-kqeih).
    pub(super) draft_choice: Option<&'static str>,
    pub(super) drafted: Option<String>,
    /// The static fan-out cue fired for this turn (theseus-z9mze.4), so its tail line rides.
    pub(super) fan_out: bool,
}

/// Where an effort is decided: the session (its base profile), whether the
/// verdict acts, and whether it is a checkpoint's, whose raise the sticky rule
/// does not hold (theseus-53ujg.3).
#[derive(Clone, Copy)]
pub(super) struct EffortAt<'s> {
    session: &'s SessionRecord,
    acts: bool,
    checkpoint: bool,
}

impl<'s> EffortAt<'s> {
    /// The inbound point's: `acts` when the turn reads a live verdict.
    pub(super) fn inbound(session: &'s SessionRecord, acts: bool) -> Self {
        Self {
            session,
            acts,
            checkpoint: false,
        }
    }

    /// A checkpoint's, which acts.
    pub(super) fn checkpoint(session: &'s SessionRecord) -> Self {
        Self {
            session,
            acts: true,
            checkpoint: true,
        }
    }
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

/// The row's times (theseus-ddbi), and the compile's estimate it was made at.
#[derive(Default)]
pub(super) struct Timed {
    est_tokens: u64,
    waited: Duration,
    late: bool,
    answered_ms: Option<u64>,
}

impl Timed {
    /// The times of a turn that waited `w` for its verdict from `started`.
    fn of(w: &Waited, late: bool, started: std::time::Instant) -> Self {
        let answered = w
            .answered
            .map(|at| at.into_std().saturating_duration_since(started));
        Self {
            est_tokens: 0,
            waited: w.waited,
            late,
            answered_ms: answered.map(|d| d.as_millis() as u64),
        }
    }
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
                .map(|moved| moved.speaking_as(&target))
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
    ///
    /// With `sensitive`, a profile not of US origin is unusable (`data_gate`):
    /// routing then picks the next usable one (theseus-oejh3, theseus-xs5ah).
    pub(super) fn route_profiles(&self, floor: super::data_step::Floor) -> routing::Profiles {
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
                } else if self.floor_refuses(floor, name).is_some() {
                    Some("data_gate")
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
                        input_per_mtok: entry.map(|e| e.input_per_mtok),
                        vision: entry.is_some_and(|e| e.vision),
                        restricted: self.restricted_profile(name),
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
            // A turn that asked no route meets the data floor here.
            let floored = match i {
                0 => match Box::pin(self.floor_unrouted(t, session, spec, provider, slot)).await? {
                    Ok(d) => d,
                    Err(f) => return Ok(Err(f)),
                },
                _ => None,
            };
            let compiled =
                Box::pin(self.compile_step(t, session, spec, force, strip, overflow, i)).await;
            if let (Some(d), Ok(Ok(c))) = (&floored, &compiled) {
                let timed = Timed {
                    est_tokens: c.est_tokens,
                    ..Timed::default()
                };
                self.record_route(t, None, Some(d), None, timed, None);
            }
            return compiled;
        };
        let mut compiled = self
            .compile_first(
                t, session, spec, provider, slot, force, strip, overflow, i, rx,
            )
            .await;
        if let Ok(Ok(c)) = &mut compiled {
            self.apply_effort(t, spec, c);
            Self::persona_line(t, c);
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
        let entry = self.catalog.get(&c.request.model);
        if crate::compiler::effort::takes(spec, entry, &c.request.model) {
            spec.turn_effort = e.ran;
            crate::compiler::effort::retarget(&mut c.request, spec, entry);
            c.messages = c.request.messages.len();
        } else {
            // `sent` is the previous request's level when the sticky rule held it.
            spec.effort = e.sent.or(e.ran);
            c.request.output_config = crate::compiler::output_config(entry, spec.effort);
        }
        // The request changed after its compile: what the turn records of it,
        // and the overflow retry's "same request" check, read its digest.
        c.digest = c.request.digest();
    }

    /// The effort rule for the profile the turn moves to (or its own), from
    /// route.v3's answer: none when the verdict has none (route.v1 and v2
    /// ask no effort). A profile other than the session's base runs at no
    /// less than its own effort (theseus-53ujg.3).
    pub(super) fn route_effort(
        &self,
        t: &Turn<'_>,
        v: &Verdict,
        d: Option<&Decision>,
        (spec, tokens): (&RequestSpec, u64),
        at: EffortAt<'_>,
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
            floor: own.filter(|_| name != Self::effort_base(at.session, t)),
        };
        let carried = v.turn != t.tc.turn_id;
        let r = &self.cfg.routing;
        let bounds = r.effort_bounds;
        let bars = effort::Bars {
            effort: r.effort_confidence,
            high: r.high_effort_confidence,
        };
        let decided = effort::decide(bounds, bars, answer, runs, at.acts, carried);
        Some(self.stick(t, spec, decided, answer, (model, tokens), at.checkpoint))
    }

    /// The profile the session runs on when routing has not moved it: where
    /// a move started (`Routed.from`), else the turn's own.
    fn effort_base<'s>(session: &'s SessionRecord, t: &'s Turn<'_>) -> &'s str {
        session
            .routed
            .as_ref()
            .and_then(|r| r.from.as_deref())
            .unwrap_or(&t.target.profile)
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
        let wait = (live && !unreachable && steering.is_none()) || self.judge.pins_wait();
        let (compiled, w) = beside(compile, &mut rx, max_wait, wait).await;
        // Missed: the turn decides without its own verdict (theseus-ddbi).
        let late = live && steering.is_none() && w.got == Got::Late;
        if live && steering.is_none() {
            self.judge.record_route_wait(w.waited, late);
        }
        let timed = Timed::of(&w, late, t.started);
        let got = w.got;
        t.route.defer_persist = false;
        let mut compiled = match compiled? {
            Ok(c) => c,
            Err(f) => {
                let seen = (got, live, unreachable);
                let held = self.held(t, f, seen, rx, session, (timed, steering.as_ref()));
                return Ok(Err(held));
            }
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
        let ask = (&steering, &verdict, reason);
        let decision = match self.decide_floored(t, session, ask, &compiled).await {
            Ok(d) => d,
            Err(f) => return Ok(Err(f)),
        };
        let drafting = self.draft_check(t, session, verdict.as_ref().filter(|_| reason.is_none()));
        let drafting = drafting.await;
        let at = EffortAt::inbound(session, reason.is_none());
        t.route.effort = verdict.as_ref().and_then(|v| {
            self.route_effort(t, v, decision.as_ref(), (&*spec, compiled.est_tokens), at)
        });
        self.verdict_asides(t, verdict.as_ref());
        self.record_route(
            t,
            verdict.as_ref(),
            decision.as_ref(),
            reason,
            Timed {
                est_tokens: compiled.est_tokens,
                ..timed
            },
            steering.as_ref(),
        );
        self.note_routed(t, verdict.as_ref(), decision.as_ref());
        let class = verdict.as_ref().filter(|_| reason.is_none());
        self.fan_out_cue(t, spec, class.map(|v| v.mode.as_str()));
        Self::fan_out_line(t, &mut compiled);
        if self.draft_open(t, drafting) {
            self.drop_first(t, &compiled);
            return Ok(Ok(compiled));
        }
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
        Self::hold_next(session, &d.hold);
        if d.profile == base {
            return self.keep_first(t, session, compiled, spec, i);
        }
        let target = match self.resolve_target(&base, Some(&d.profile), None, None) {
            Ok(tg) => tg.speaking_as(t.target),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), profile = %d.profile, "routing: the profile did not resolve; the session's own runs");
                return self.keep_first(t, session, compiled, spec, i);
            }
        };
        let Some(p) = self.providers.get(&target.provider).cloned() else {
            return self.keep_first(t, session, compiled, spec, i);
        };
        // The first compile is not the call's: its rows go with it.
        self.drop_first(t, &compiled);
        if d.detour {
            Self::detouring(t, verdict.as_ref(), steering.as_ref());
        }
        self.routed_to(t, session, spec, slot, target, &d).await?;
        *provider = Some(p);
        if d.detour {
            return self.compile_detour(t, session, spec, i);
        }
        Box::pin(self.compile_step(t, session, spec, force, strip, overflow, i)).await
    }

    /// The decision's hold on the session's routed record, which goes once it
    /// holds nothing.
    fn hold_next(session: &mut SessionRecord, hold: &HoldNext) {
        let routed = session.routed.get_or_insert_with(Default::default);
        match hold {
            HoldNext::Keep => {}
            HoldNext::Clear => routed.hold = None,
            HoldNext::Set(h) => routed.hold = Some(h.clone()),
        }
        if session.routed.as_deref() == Some(&Default::default()) {
            session.routed = None;
        }
    }

    /// A compile failed (every failure comes here, held or not): the verdict
    /// is read and the privacy check run, and `f` goes on as it was; the turn
    /// loop then ends the turn held where the verdict holds (theseus-cljlz).
    /// The turn is still counted in the route's books (theseus-qiyxq): its
    /// `route.decided` row (no decision, so the profile it would have run on
    /// and a reason of the verdict's absence or kind; the wait, `late`,
    /// `answered_ms` and the steering are the turn's own, theseus-m1a51) and
    /// the correction memory's note, so route stats count these turns. Not the
    /// persona: no persona answers the fixed line.
    fn held(
        &self,
        t: &mut Turn<'_>,
        f: Failure,
        (got, live, unreachable): (Got, bool, bool),
        rx: RouteWait,
        session: &SessionRecord,
        (timed, steering): (Timed, Option<&Steering>),
    ) -> Failure {
        let last = session.last_turn_id.as_deref();
        let (verdict, reason) = self.read_verdict(t, got, live, rx, last, unreachable);
        self.personal_warn(t, verdict.as_ref());
        self.record_route(t, verdict.as_ref(), None, reason, timed, steering);
        self.note_routed(t, verdict.as_ref(), None);
        f
    }

    /// The turn detours: its record keeps the session's own target, and the
    /// verdict's mode (none for a steered turn: it names the mode, but sends no
    /// recall) says whether the detour's request carries the turn's recall
    /// (theseus-pa1xx). The mode is the row's `kind` (theseus-r5ydi).
    fn detouring(t: &mut Turn<'_>, verdict: Option<&Verdict>, steering: Option<&Steering>) {
        t.route.keeps = Some(TargetRef::from(t.target));
        t.route.detour_kind = detour_word(steering.map(|s| &s.steer), verdict);
        t.route.detour_recall = steering.is_none()
            && verdict.is_some_and(|v| crate::config::routing::detour_recalls(&v.mode));
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
                // A brief's late verdict picks nothing, now or for a later message.
                let keep = !t.route.brief;
                tokio::spawn(async move {
                    if let Ok(Answered {
                        verdict: Some(v), ..
                    }) = rx.await
                    {
                        if keep {
                            judge.set_late(&s, v);
                        }
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

    /// The decision the turn's verdict (or steer) makes, kept off restricted
    /// profiles when the request names a sensitive name (theseus-oejh3), and
    /// on one only for a session whose labels allow it (theseus-9ymz0).
    ///
    /// The labels are read first, without a wait. Only when items are not yet
    /// judged and the decision, made as if they were, would take a profile of
    /// restricted origin does the turn wait for their verdicts and decide
    /// again: a turn bound for Claude never waits.
    async fn decide_floored(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        (steering, verdict, reason): (&Option<Steering>, &Option<Verdict>, Option<Reason>),
        compiled: &Compiled,
    ) -> Result<Option<Decision>, Failure> {
        t.route.sensitive |= self.sensitive_request(&compiled.request) || self.private_session(t);
        let est_tokens = compiled.est_tokens;
        let decide = |t: &Turn<'_>| match (steering, verdict, reason) {
            (Some(s), _, _) => Some(self.decide_steer(t, session, s, est_tokens)),
            (None, Some(v), None) => Some(self.decide_route(t, session, v, est_tokens)),
            _ => None,
        };
        use super::data_step::Cn;
        t.route.cn = Self::cn_read(t);
        let mut decision = decide(t);
        if t.route.cn != Cn::Allowed {
            // What the same decision would have named had the session's
            // labels allowed it: only where that is a restricted profile does
            // the rule cost the turn a model (theseus-b80si), and only then
            // does the turn wait for a verdict that has not come.
            t.route.assume_cn = true;
            let wanted = decide(t);
            t.route.assume_cn = false;
            let base = t.target.profile.clone();
            let named = |d: &Option<Decision>| {
                self.restricted_profile(d.as_ref().map_or(base.as_str(), |d| d.profile.as_str()))
            };
            if named(&wanted) {
                if t.route.cn == Cn::Pending {
                    self.cn_settle(t).await;
                    if t.route.cn == Cn::Allowed {
                        decision = decide(t);
                    }
                }
                if t.route.cn != Cn::Allowed && !named(&decision) {
                    t.route.gated = Some(t.route.cn.word());
                }
            }
        }
        self.data_floor(t, decision)
    }

    /// The switch rule over the usable profiles, under the place's cap.
    fn decide_route(
        &self,
        t: &Turn<'_>,
        session: &SessionRecord,
        v: &Verdict,
        est_tokens: u64,
    ) -> Decision {
        if t.route.brief {
            return self.decide_brief(t, v);
        }
        let profiles = self.route_profiles(t.route.floor());
        let cap = Self::place_cap(t, &profiles);
        let hold = session.routed.as_ref().and_then(|r| r.hold.clone());
        // Jev's own move up from the base is what a step back down undoes.
        let from = session
            .routed
            .as_ref()
            .filter(|r| r.profile.is_some())
            .and_then(|r| r.from.as_deref());
        let ask = |est_tokens| {
            routing::decide(
                &self.cfg.routing,
                &profiles,
                &routing::Ask {
                    verdict: v,
                    base: &t.target.profile,
                    est_tokens,
                    hold: hold.as_ref(),
                    from,
                    images: t.route.images,
                    cap,
                    cleared: t.route.floor().cn_ok,
                },
            )
        };
        let d = ask(est_tokens);
        // A switch held back because its target would write the request cold:
        // what the prefix library holds warm on that model is read, not
        // written (theseus-rr15).
        let (routing::Reason::CacheHold, routing::HoldNext::Set(h)) = (d.reason, &d.hold) else {
            return d;
        };
        let warm = self
            .resolve_target(&t.target.profile, Some(&h.profile), None, None)
            .map_or(0, |to| {
                self.warm_prefix_tokens(&to, session.kind, t.tc.session_id, t.tc.place())
            });
        if warm == 0 {
            return d;
        }
        ask(est_tokens.saturating_sub(warm))
    }

    /// The `route.decided` row, and what the turn's result says.
    pub(super) fn record_route(
        &self,
        t: &mut Turn<'_>,
        verdict: Option<&Verdict>,
        decision: Option<&Decision>,
        reason: Option<Reason>,
        timed: Timed,
        steering: Option<&Steering>,
    ) {
        self.injection_read(t, verdict, reason);
        let base = t.target.profile.clone();
        let reason = decision.map_or(reason.unwrap_or(Reason::NoVerdict), |d| d.reason);
        let source = steering
            .map(|_| "correction")
            .or(t.route.brief.then_some("task brief"));
        let profile = decision.map_or(base.clone(), |d| d.profile.clone());
        let answer = verdict.and_then(|v| v.effort.as_ref());
        let e = t.route.effort;
        let applied = e
            .filter(EffortDecision::applied)
            .and_then(|e| e.sent.or(e.ran));
        let gate = t.route.gated.take();
        if let Some(why) = gate {
            self.judge.record_data_gate("route", why);
        }
        let cn = t.route.cn.word();
        let ran = self.ran_of(&profile);
        let origin = ran.origin;
        t.record(&fact::route::RouteDecided {
            mode: verdict.map(|v| v.mode.as_str()),
            confidence: verdict.map(|v| v.confidence),
            judgment: verdict.map(|v| v.judgment.as_str()),
            from: &base,
            profile: &profile,
            reason: reason.as_str(),
            detour: decision.is_some_and(|d| d.detour),
            switch: decision.is_some_and(|d| d.switch),
            est_tokens: timed.est_tokens,
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
            effort_sent: e.and_then(|e| e.sent).map(effort::name),
            checkpoint: None,
            injection: t
                .route
                .injection
                .as_ref()
                .map(super::tier::InjectionBudget::row),
            ran,
            data: Some(fact::route::RouteData {
                cn,
                gate,
                draft_skip: t.route.draft_skip,
                draft_choice: t.route.draft_choice,
            }),
        });
        let prior = t.route.result.take();
        let persona_shown = prior.and_then(|r| r.persona);
        t.route.result = Some(TurnRoute {
            mode: verdict.map(|v| v.mode.clone()),
            reason: reason.as_str().into(),
            from: base,
            source: source.map(str::to_string),
            effort: answer.map(|a| a.level.clone()),
            effort_reason: e.map(|e| e.reason.as_str().into()),
            effort_applied: applied.map(|e| effort::name(e).into()),
            persona: persona_shown,
            data: Some(TurnData {
                cn: cn.into(),
                gate: gate.map(Into::into),
                origin: origin.into(),
            }),
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

    /// The first compile is not the call's: its deferred rows go, and its pick
    /// in the prefix library, which `compile_step` made in memory
    /// (theseus-i4y7l).
    fn drop_first(&self, t: &mut Turn<'_>, compiled: &Compiled) {
        t.route.deferred = None;
        if let Some(p) = compiled.prefix.as_deref() {
            self.prefix.discard(p);
        }
    }

    /// The first compile is the one the call uses: persist it, and record
    /// its rows, as the compile step would have.
    fn keep_first(
        &self,
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        mut compiled: Compiled,
        spec: &RequestSpec,
        i: u32,
    ) -> Result<Result<Compiled, Failure>> {
        if compiled.new_compilation
            && session.compilation_id.as_deref() != Some(&compiled.compilation.id)
        {
            Self::keep_blocks(t, &mut compiled);
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
    pub(super) fn route_to<'a>(
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
        s.session_text = self.session_block(t.tc.dir, t.tc.place());
        s.walk = self.walk(t.tc.session_id, t.tc.place().content_class());
        self.stand(t, &mut s);
        self.thread_spec(&mut s, t.tc.session_id, session.compilation_id.as_deref());
        self.defer_tools(t, &mut s);
        if t.route.fan_out {
            self.defer_cued(t, &mut s);
        }
        *spec = s;
        if d.switch {
            session.last_target = Some(TargetRef::from(target));
            if t.route.brief {
                // The class's profile is the task's base, not a move from
                // its parent's model (theseus-ypnxt.7).
                session.routed = None;
                return Ok(());
            }
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

    /// A detour's request: the profile's own header (`detour_spec`), the last
    /// `trivial_context_turns` exchanges with the message, and the project's
    /// slice, compiled
    /// outside the session's compilation, which it never writes. A trivial
    /// detour reads the transcript without `recall_view`, so the turn's recall
    /// reaches no request, and `recall_routed` drops it; a quick one reads it
    /// through `recall_view`, so the turn's own `Recall` node renders after
    /// the message (theseus-pa1xx).
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
        // A detour admits no summary and no earlier recall (35a): its last
        // exchanges and the message, and a quick one's own recall, written by
        // the plan frame once its first loop has gone out.
        let own =
            Some(t.recall.recall_id.as_str()).filter(|r| t.route.detour_recall && !r.is_empty());
        let nodes: Vec<_> = nodes[from..]
            .iter()
            .filter(|(_, n)| match n.kind {
                crate::stub::Kind::Summary => false,
                crate::stub::Kind::Recall => own == Some(n.id.as_str()),
                _ => true,
            })
            .cloned()
            .collect();
        let (nodes, sources) = match t.route.detour_recall {
            true => self.recall_view(t, nodes, &spec.profile),
            false => (nodes, crate::recall::render::Sources::default()),
        };
        // The owner's standing records ride a detour too (theseus-a7vi): they
        // hold in every prompt of their scope, a cheap turn's included.
        // (The standing block is in the header layer already.)
        let detour = detour_step::detour_spec(spec);
        let mut compiled = compile(CompileInput {
            session_id: sid,
            current: None,
            nodes: &nodes,
            last_position: self.store.last_position(),
            spec: &detour,
            catalog: &self.catalog,
            force: None,
            window_override: self.working_window(&detour),
            blobs: Some(self.store.blobs()),
            hidden: &session.not_shown,
            strip: None,
            overflowed: None,
            sources: &sources,
            signals: None,
            assembled: None,
            situation: &crate::compiler::situation::Situation::Detour,
            kept: None,
        });
        if t.route.detour_recall {
            Self::recall_compiled(t, &mut compiled);
        }
        let mut tail = detour_step::Tail::default();
        let before = compiled.est_tokens;
        Self::standing_here(t, &mut compiled);
        tail.standing = compiled.est_tokens.saturating_sub(before);
        let before = compiled.est_tokens;
        Self::persona_line(t, &mut compiled);
        tail.persona = compiled.est_tokens.saturating_sub(before);
        // The project the conversation serves, last of all as `tail_blocks`
        // does (theseus-v4u66): a cheap turn about it is as wrong without it.
        let project = (!self.restricted_profile(&spec.profile))
            .then(|| {
                crate::project::slice::attach(&self.store, &self.kernel, session, &mut compiled)
            })
            .flatten();
        // The board notes this request's loaded set (theseus-zs6mm).
        self.defer_noted(t, &detour, &compiled);
        let last = self.store.last_position();
        if let Some(f) = Self::unadmitted(t, &compiled, &nodes, last, false, i) {
            return Ok(Err(f));
        }
        if let Some(f) = Self::standing_dropped(t, &compiled, i) {
            return Ok(Err(f));
        }
        let row = self.detoured_row(
            t,
            &detour,
            &compiled,
            (&nodes, &tail, project.map_or(0, |p| p.tokens)),
            i,
        );
        t.record(&fact::detour::Detoured { request: &row });
        t.record(&fact::turn::LoopStarted {
            turn_id: t.tc.turn_id,
            index: i,
            model: &t.target.model,
            tools_offered: detour.tools.len() as u32,
        });
        Ok(Ok(compiled))
    }
}

/// A detour's `kind`: the mode a steer names, `steered` for any other steer,
/// else the verdict's mode (theseus-nfrlj). A steer that is no mode is never
/// a detour today (`steer_to` makes none), so the word is for a later one.
fn detour_word(steer: Option<&Steer>, verdict: Option<&Verdict>) -> String {
    match steer {
        Some(Steer::Mode(m)) => m.clone(),
        Some(_) => "steered".into(),
        None => verdict.map_or_else(String::new, |v| v.mode.clone()),
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

    /// Each routing profile carries its model's origin, so routing's own gate
    /// keeps a restricted one off an uncleared ask even where the floor lets
    /// it be usable (theseus-a1wvq.4 at the floor's join).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn route_profiles_carry_each_profiles_origin() {
        let r = crate::tests_route::rig(None, 1, crate::tests_route::restricted_placements);
        let cn_ok = super::super::data_step::Floor {
            sensitive: false,
            cn_ok: true,
        };
        let ps = r.core.runner.route_profiles(cn_ok);
        assert_eq!(
            ps["glm"].unusable, None,
            "the floor lets a cleared turn use it"
        );
        assert!(ps["glm"].restricted, "GLM is of restricted origin");
        assert!(!ps["sonnet"].restricted, "Claude is not");
    }

    /// A detour's word is a mode's name, or `steered` for another steer
    /// (theseus-nfrlj), never empty when a steer made it.
    #[test]
    fn a_detours_word_names_its_mode_or_says_steered() {
        let quick = v("quick");
        assert_eq!(detour_word(None, Some(&quick)), "quick");
        let steer = Steer::Mode("trivial".into());
        assert_eq!(detour_word(Some(&steer), Some(&quick)), "trivial");
        let steer = Steer::Profile("haiku".into());
        assert_eq!(detour_word(Some(&steer), None), "steered");
    }

    fn v(mode: &str) -> Verdict {
        Verdict {
            mode: mode.into(),
            confidence: 0.9,
            effort: None,
            judgment: "jdg_1".into(),
            turn: "turn_1".into(),
            persona: None,
            seal: None,
            personal: None,
            draft: None,
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
                        cost_micros: 0,
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
