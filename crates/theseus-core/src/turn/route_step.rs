//! The turn's routing step (M5 25e; `crate::routing`): `route.v1`'s verdict,
//! asked at `inbound` beside `classify.v1` and `role.v1`, decides which
//! profile the turn runs on.
//!
//! - **Beside the first compile, never before it** ([`beside`]): the call
//!   started at `inbound`, so the turn compiles on the session's profile,
//!   then waits at most `[routing] max_wait_ms` more. A verdict later than
//!   that applies from the next message, and this turn says `late`. The
//!   judge off, Jev's breaker open, a slash command, a continuation, a pinned
//!   turn, or `route.v1` in shadow: no wait.
//! - **A switch** rebuilds the spec and compiles again on the routed profile;
//!   the first compile's compilation is persisted only when the call uses it.
//! - **A detour** (`trivial`) compiles the persona and the last
//!   `trivial_context_turns` exchanges outside the session's compilation,
//!   which it never writes; the session's profile, `last_target`, and
//!   compilation stay as they were.
//! - **Recorded** as one `route.decided` row, in the turn's next frame, and
//!   on the turn's result (`TurnSubmitResult.route`).

use std::future::Future;
use std::sync::OnceLock;

use theseus_protocol::route::TurnRoute;

use super::*;
use crate::config::PackMode;
use crate::judge::inbound::{RouteWait, ROUTE_PACK};
use crate::routing::{self, Decision, HoldNext, Reason, Verdict};

/// What a turn holds of routing, from its inbound point to its end.
#[derive(Default)]
pub(super) struct RouteState {
    /// The verdict's channel, until the first compile takes it.
    wait: Option<RouteWait>,
    /// `route.v1`'s mode for the turn (live, or shadow for a pin).
    mode: Option<PackMode>,
    /// The turn's input carries an image.
    images: bool,
    /// Routing is deciding: the first compile does not persist its
    /// compilation (only the one the call uses is).
    pub(super) defer_persist: bool,
    /// A detour: the session's own target, which its record keeps.
    pub(super) keeps: Option<TargetRef>,
    /// What the turn's result says.
    pub(super) result: Option<TurnRoute>,
}

/// What came of the wait.
#[derive(Debug, Clone, PartialEq)]
pub enum Got {
    /// The verdict, or none (Jev gave none, or nothing was sent).
    Verdict(Option<Verdict>),
    /// The wait ended first.
    Late,
}

/// Run `compile`, then wait for the verdict at most `max_wait` after it
/// ends (not waiting at all when `wait` is false): the verdict's call
/// started before the compile, so its time runs beside it. Also returns how
/// long it waited after the compile.
pub async fn beside<F: Future>(
    compile: F,
    rx: &mut RouteWait,
    max_wait: Duration,
    wait: bool,
) -> (F::Output, Got, Duration) {
    let out = compile.await;
    let t0 = tokio::time::Instant::now();
    let got = match wait {
        true => match tokio::time::timeout(max_wait, &mut *rx).await {
            Ok(Ok(v)) => Got::Verdict(v),
            Ok(Err(_)) => Got::Verdict(None),
            Err(_) => Got::Late,
        },
        false => match rx.try_recv() {
            Ok(v) => Got::Verdict(v),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => Got::Verdict(None),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => Got::Late,
        },
    };
    (out, got, t0.elapsed())
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

    /// A person's message's turn runs on the session's routed profile, unless
    /// the owner chose one.
    pub(super) fn route_base(
        &self,
        session: &SessionRecord,
        target: Target,
        input: bool,
    ) -> Target {
        let routed = session.routed.as_ref().and_then(|r| r.profile.as_deref());
        match routed {
            Some(p) if input && target.chosen.is_none() && p != target.profile => self
                .resolve_target(&target.profile, Some(p), None, None)
                .unwrap_or(target),
            _ => target,
        }
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
    /// `route.v1`, the compile, the wait beside it, and the decision; on a
    /// detour's later loops, the detour's compile; else the compile.
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
        let Some(mut rx) = t.route.wait.take().filter(|_| i == 0) else {
            return self
                .compile_step(t, session, spec, force, strip, overflow, i)
                .await;
        };
        let mode = t.route.mode.unwrap_or(PackMode::Shadow);
        let live = mode >= PackMode::Canary && !self.judge.breaker_open();
        t.route.defer_persist = live;
        let max_wait = Duration::from_millis(self.cfg.routing.max_wait_ms);
        let compile = self.compile_step(t, session, spec, force, strip, overflow, i);
        let (compiled, got, waited) = beside(compile, &mut rx, max_wait, live).await;
        t.route.defer_persist = false;
        let compiled = match compiled? {
            Ok(c) => c,
            Err(f) => return Ok(Err(f)),
        };
        let base = t.target.profile.clone();
        let (verdict, reason) =
            self.read_verdict(t, got, live, rx, session.last_turn_id.as_deref());
        let decision = match (&verdict, reason) {
            (Some(v), None) => Some(self.decide_route(t, session, v, compiled.est_tokens)),
            _ => None,
        };
        Self::record_route(
            t,
            verdict.as_ref(),
            decision.as_ref(),
            reason,
            &compiled,
            waited,
        );
        if live {
            // A routed turn: the owner's pin of another profile within 10
            // minutes after it counts on route.v1's ladder (26a).
            let ran = decision
                .as_ref()
                .map_or(base.as_str(), |d| d.profile.as_str());
            self.judge.set_routed(t.tc.session_id, ran);
        }
        let Some(d) = decision else {
            return Self::keep_first(t, session, compiled);
        };
        let routed = session.routed.get_or_insert_with(Default::default);
        match &d.hold {
            HoldNext::Keep => {}
            HoldNext::Clear => routed.hold = None,
            HoldNext::Set(h) => routed.hold = Some(h.clone()),
        }
        if session.routed.as_ref() == Some(&Default::default()) {
            session.routed = None;
        }
        if d.profile == base {
            return Self::keep_first(t, session, compiled);
        }
        let target = match self.resolve_target(&base, Some(&d.profile), None, None) {
            Ok(tg) => tg,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), profile = %d.profile, "routing: the profile did not resolve; the session's own runs");
                return Self::keep_first(t, session, compiled);
            }
        };
        let Some(p) = self.providers.get(&target.provider).cloned() else {
            return Self::keep_first(t, session, compiled);
        };
        if d.detour {
            t.route.keeps = Some(TargetRef::from(t.target));
        }
        self.route_to(t, session, spec, slot, target, &d)?;
        *provider = Some(p);
        if d.detour {
            return self.compile_detour(t, session, spec, i);
        }
        self.compile_step(t, session, spec, force, strip, overflow, i)
            .await
    }

    /// The verdict the turn reads, or why it has none: its own, else a late
    /// one of the session's last message; a pin's or shadow's is recorded
    /// only. A wait that ended first leaves the verdict to the next message.
    /// Every message takes the session's late verdict, and keeps it only when
    /// it was asked for `last_turn`, the session's turn just before this one:
    /// a late verdict applies to the next message alone, never to a later one.
    fn read_verdict(
        &self,
        t: &Turn<'_>,
        got: Got,
        live: bool,
        rx: RouteWait,
        last_turn: Option<&str>,
    ) -> (Option<Verdict>, Option<Reason>) {
        let sid = t.tc.session_id;
        let stale = self.judge.take_late(sid);
        let late = || stale.filter(|v| Some(v.turn.as_str()) == last_turn);
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
                    if let Ok(Some(v)) = rx.await {
                        judge.set_late(&s, v);
                    }
                });
                match late() {
                    Some(v) => (Some(v), None),
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
        waited: Duration,
    ) {
        let base = t.target.profile.clone();
        let reason = decision.map_or(reason.unwrap_or(Reason::NoVerdict), |d| d.reason);
        let profile = decision.map_or(base.clone(), |d| d.profile.clone());
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
            wait_ms: waited.as_millis() as u64,
        });
        t.route.result = Some(TurnRoute {
            mode: verdict.map(|v| v.mode.clone()),
            reason: reason.as_str().into(),
            from: base,
        });
    }

    /// Where the session's record says it runs: its own target through a
    /// detour, else the turn's.
    pub(super) fn ran_on(t: &Turn<'_>) -> TargetRef {
        t.route
            .keeps
            .clone()
            .unwrap_or_else(|| TargetRef::from(t.target))
    }

    /// The first compile is the one the call uses: persist it, as the
    /// compile step would have.
    fn keep_first(
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        compiled: Compiled,
    ) -> Result<Result<Compiled, Failure>> {
        if compiled.new_compilation
            && session.compilation_id.as_deref() != Some(&compiled.compilation.id)
        {
            Self::persist_compilation(t.tc.store, &compiled, session, t.tc.turn_id)?;
        }
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
        let target = slot.get().expect("set");
        t.target = target;
        t.tc.target = Some(target);
        let (mut s, _) = self.request_spec(target, session.kind, t.tc.place());
        s.walk = self.walk(t.tc.session_id, t.tc.class);
        *spec = s;
        if d.switch {
            session.last_target = Some(TargetRef::from(target));
            session.routed.get_or_insert_with(Default::default).profile =
                Some(target.profile.clone());
        }
        Ok(())
    }

    /// A detour's request: the persona and the profile's own header, and the
    /// last `trivial_context_turns` exchanges with the message, compiled
    /// outside the session's compilation, which it never writes.
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
        let nodes: Vec<_> = nodes[from..].to_vec();
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
        });
        t.record(&fact::turn::LoopStarted {
            turn_id: t.tc.turn_id,
            index: i,
            model: &t.target.model,
            tools_offered: detour.tools.len() as u32,
        });
        Ok(Ok(compiled))
    }
}

/// Where a detour's nodes begin: at the person's message `turns` messages
/// before the newest one (the turn's own), or the session's start.
pub fn detour_start(nodes: &[(u64, Arc<Node>)], turns: u32) -> usize {
    let asks: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, (_, n))| {
            n.origin == crate::node::Origin::Operator && matches!(n.body, Body::UserMessage { .. })
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
                    let _ = tx.send(Some(v("chat")));
                }
                None => {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    drop(tx);
                }
            }
        });
        let compile = tokio::time::sleep(Duration::from_millis(100));
        let (_, got, _) = beside(compile, &mut rx, Duration::from_millis(200), true).await;
        (t0.elapsed().as_millis() as u64, got)
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
        let (tx, mut rx) = tokio::sync::oneshot::channel::<Option<Verdict>>();
        drop(tx);
        let (_, got, _) = beside(async {}, &mut rx, Duration::from_millis(200), true).await;
        assert_eq!((t0.elapsed().as_millis(), got), (0, Got::Verdict(None)));
        let (_tx, mut rx) = tokio::sync::oneshot::channel::<Option<Verdict>>();
        let (_, got, _) = beside(async {}, &mut rx, Duration::from_millis(200), false).await;
        assert_eq!((t0.elapsed().as_millis(), got), (0, Got::Late));
    }

    #[test]
    fn a_detour_takes_the_last_exchanges_and_the_message() {
        let n = |text: &str, op: bool| {
            let mut node = Node::user_with("ses_1", None, "cli", text, vec![]);
            if !op {
                node.origin = crate::node::Origin::Agent;
            }
            (0u64, Arc::new(node))
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
