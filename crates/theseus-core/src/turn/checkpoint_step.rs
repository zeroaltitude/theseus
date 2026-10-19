//! A running task's checkpoints (theseus-ypnxt.9), and a routed turn's
//! (theseus-a1wvq): the turn's side. The task a person started with no
//! profile, or a conversation's turn the route placed, asks the route pack
//! about its recent state at a loop's end when its work calls for it
//! (`routing::checkpoint::Tally`: it keeps failing, stalls, changes phase, or
//! is about to report), and the verdict, applied at the first later loop
//! boundary that finds it come (never waited for), moves its model up, or
//! back down to its base, by the rule in `routing::checkpoint`.
//!
//! - **Which turns**: a task's started with no profile (the brief's mark,
//!   `brief_step::marked`), or a conversation's turn the route placed (not
//!   pinned, not a detour), while routing acts live and the judge is on. A
//!   "set up X" message reads routine and turns out deep: its turn's first
//!   guess was the only one until v1 patch1. A task started with a profile,
//!   or a turn on the owner's pin, asks nothing; a turn of one loop pays one
//!   `match`.
//! - **The switch** is a refusal's fallback's shape (`fallback_step`): a
//!   slot of its own per allowed move ([`Slots`], bounded by
//!   `[routing.task_checkpoints] max_switches`, validated at load), the spec
//!   rebuilt as `route_to` builds it, the provider resolved as the first
//!   compile's switch resolves it, and the next compile made on the new
//!   model (a model change strips the thinking). "A turn routes once" stays
//!   true of the inbound route: the checkpoint has its own slots.
//! - **Never waited on**: one checkpoint in flight at a time, the answer read
//!   with `try_recv`; a flight a window old is dropped, so a late answer is
//!   never applied after the next ask.
//! - **Its cost** is the task's (`Kernel::charge_execution`, from the judge
//!   when the call settles); a task whose budget cannot cover it skips the
//!   ask, recorded as reason `budget`.

use super::*;
use crate::config::PackMode;
use crate::judge::inbound::{author_of, place_kind, Inbound, RouteWait};
use crate::routing::checkpoint::{self as cp, CheckpointAsk, LoopLine, Seen, Tally, Trigger};
use crate::routing::effort::{self, EffortDecision};
use crate::routing::{self, Decision, Hold, HoldNext, Reason, Verdict};
use theseus_protocol::modes::PlanState;
use theseus_protocol::route::TurnRoute;

/// The `source` of a task's checkpoint's `route.decided` row.
pub const SOURCE: &str = "task checkpoint";
/// The `source` of a routed turn's checkpoint's row (theseus-a1wvq).
pub const TURN_SOURCE: &str = "turn checkpoint";

/// The slots a turn holds for its checkpoints' moves: at most
/// `max_switches` moves and the step back down past the cap, which
/// `TaskCheckpoints::validate` keeps within.
pub(super) const SLOTS: usize = 8;
pub(super) type Slots = [OnceLock<Target>; SLOTS];

/// A turn's slots.
pub(super) fn slots() -> Slots {
    Default::default()
}

/// What a checkpoint's judgment may need of its task's budget, in
/// micro-dollars: a route pack request's worst case, with room. A task with less
/// left asks nothing.
const RESERVE_MICROS: u64 = 30_000;

/// What a turn holds of its task's checkpoints.
#[derive(Default)]
pub(super) struct Checkpoints {
    /// A tool result of the loop being run was an error as the runner marks
    /// one (`ResultStatus::is_error`), set as its calls answer.
    pub(super) failed: bool,
    state: State,
}

impl Checkpoints {
    /// The profile a task runs on when a checkpoint has moved it off its
    /// base: what its `loop.ended` rows say, so the board's line does.
    pub(super) fn moved_to(&self, on: &str) -> Option<String> {
        match &self.state {
            State::On(a) if a.base != on => Some(on.to_string()),
            _ => None,
        }
    }
}

#[derive(Default)]
enum State {
    /// Not read yet: the first loop's end decides.
    #[default]
    Unread,
    /// Not a task's, pinned, or off.
    Off,
    On(Box<Active>),
}

/// One task's, or one routed turn's, checkpoints in a turn.
struct Active {
    /// A task's (else a conversation's turn's).
    task: bool,
    /// The task's base: its brief's pick or its parent's; a turn's, where
    /// its route put it.
    base: String,
    tally: Tally,
    /// Moves made, up or down.
    moves: u32,
    /// Slots used.
    used: usize,
    /// What the last checkpoint's verdict left held.
    hold: Option<Hold>,
    flight: Option<Flight>,
    /// The last loops' lines, for the text.
    loops: Vec<LoopLine>,
    said: Option<String>,
    /// The plan as last read: each step's title and state.
    plan: Vec<(String, PlanState)>,
    brief: String,
    /// The last compile's estimate, and whether its request names a sensitive name.
    est: u64,
    sensitive: bool,
}

impl Active {
    /// Its rows' `source`.
    fn source(&self) -> &'static str {
        if self.task {
            SOURCE
        } else {
            TURN_SOURCE
        }
    }
}

/// A checkpoint asked and not yet answered.
struct Flight {
    rx: RouteWait,
    trigger: Trigger,
    asked: u32,
}

/// What the loop that just ended showed the step.
pub(super) struct Ended<'r> {
    pub(super) compiled: &'r Compiled,
    pub(super) resp: &'r ModelResponse,
    pub(super) uses: &'r [ToolUse],
    pub(super) index: u32,
    /// The turn goes on to another loop.
    pub(super) more: bool,
}

/// A plan step's place: the first one doing, else the first todo.
fn current(plan: &[(String, PlanState)]) -> Option<&str> {
    let open = |s: &&(String, PlanState)| matches!(s.1, PlanState::Todo | PlanState::Doing);
    plan.iter()
        .filter(open)
        .find(|s| s.1 == PlanState::Doing)
        .or_else(|| plan.iter().find(open))
        .map(|s| s.0.as_str())
}

fn done_in(plan: &[(String, PlanState)]) -> usize {
    plan.iter().filter(|s| s.1 == PlanState::Done).count()
}

/// The plan's signals between two reads: a step done, the current step moved,
/// the last open step done.
fn plan_seen(before: &[(String, PlanState)], now: &[(String, PlanState)]) -> Seen {
    let (was, is) = (current(before), current(now));
    Seen {
        advanced: done_in(now) > done_in(before),
        phase: match (was, is) {
            (Some(a), Some(b)) if a != b => Some((a.to_string(), b.to_string())),
            _ => None,
        },
        report: was.is_some() && is.is_none() && done_in(now) > 0,
        ..Seen::default()
    }
}

/// A call that may move the plan: the plan and task tools.
fn touches_plan(uses: &[ToolUse]) -> bool {
    uses.iter()
        .any(|u| u.name.starts_with("plan_") || u.name.starts_with("task_"))
}

impl TurnRunner {
    /// The turn's state, read once a turn: none for a turn the route did not
    /// place (pinned, a detour, unrouted), a pinned task, routing or the judge
    /// off, or checkpoints off.
    fn checkpoints_for(&self, t: &Turn<'_>, session: &SessionRecord) -> Option<Box<Active>> {
        let cfg = &self.cfg.routing;
        let sid = t.tc.session_id;
        let task = t.tc.task.is_some();
        if !cfg.task_checkpoints.enabled
            || !self.judge.config().enabled
            || self.route_mode(t.target, sid) < PackMode::Canary
        {
            return None;
        }
        let (brief, base) = if task {
            if !super::brief_step::marked(t.tc.store, sid) {
                return None;
            }
            let brief = match crate::task::brief(t.tc.store, sid) {
                Ok(Some(n)) => match n.body {
                    Body::UserMessage { text, .. } => text,
                    _ => String::new(),
                },
                _ => String::new(),
            };
            // A task a checkpoint moved before keeps its base in `routed.from`.
            let base = session
                .routed
                .as_ref()
                .and_then(|r| r.from.clone())
                .unwrap_or_else(|| t.target.profile.clone());
            (brief, base)
        } else {
            if !Self::placed_by_route(t) {
                return None;
            }
            (t.route.ask.clone(), t.target.profile.clone())
        };
        Some(Box::new(Active {
            task,
            base,
            tally: Tally::default(),
            moves: 0,
            used: 0,
            hold: None,
            flight: None,
            loops: Vec::new(),
            said: None,
            plan: Self::plan_of(t),
            brief,
            est: 0,
            sensitive: false,
        }))
    }

    /// A conversation's turn the route placed: the route decided it, it is
    /// no detour, and the owner chose no profile for it.
    fn placed_by_route(t: &Turn<'_>) -> bool {
        t.route.result.is_some() && t.route.keeps.is_none() && t.target.chosen.is_none()
    }

    fn plan_of(t: &Turn<'_>) -> Vec<(String, PlanState)> {
        crate::task_graph::plan::view(t.tc.store, t.tc.session_id)
            .into_iter()
            .map(|p| (p.title, p.state))
            .collect()
    }

    /// The first line of the loop's first failed result, from the tail of the
    /// transcript (read only after a loop that failed).
    fn failed_line(t: &Turn<'_>, index: u32) -> Option<String> {
        let nodes = t.tc.store.transcript(t.tc.session_id).ok()?;
        nodes.iter().rev().take(64).find_map(|(_, n)| {
            let n: &crate::node::Node = n;
            match &n.body {
                Body::ToolResult {
                    is_error: true,
                    content,
                    ..
                } if n.loop_index == Some(index) && n.turn_id.as_deref() == Some(t.tc.turn_id) => {
                    content
                        .lines()
                        .map(str::trim)
                        .find(|l| !l.is_empty())
                        .map(str::to_string)
                }
                _ => None,
            }
        })
    }

    /// A loop ended: take what it showed, and ask a checkpoint when the task's
    /// work calls for one. Quiet for a turn that is not a task's.
    pub(super) fn checkpoint_ended(&self, t: &mut Turn<'_>, session: &SessionRecord, e: Ended<'_>) {
        let mut a = match std::mem::take(&mut t.checkpoint.state) {
            State::Off => {
                t.checkpoint.state = State::Off;
                return;
            }
            State::Unread => match self.checkpoints_for(t, session) {
                Some(a) => {
                    // The brief's verdict was the first loop's: later
                    // switches are the checkpoints' own.
                    t.route.brief = false;
                    a
                }
                None => {
                    t.checkpoint.state = State::Off;
                    return;
                }
            },
            State::On(a) => a,
        };
        let failed = std::mem::take(&mut t.checkpoint.failed);
        let mut seen = Seen {
            failed,
            ..Seen::default()
        };
        if e.index > 0 && touches_plan(e.uses) {
            let now = Self::plan_of(t);
            let plan = plan_seen(&a.plan, &now);
            (seen.advanced, seen.phase, seen.report) = (plan.advanced, plan.phase, plan.report);
            a.plan = now;
        }
        let say = (!e.uses.is_empty())
            .then(|| super::advance_step::say_of(&e.resp.text, &self.tools.scrubber))
            .flatten();
        if say.is_some() && say != a.said {
            seen.advanced = true;
            a.said = say.clone();
        }
        a.loops.push(LoopLine {
            index: e.index,
            say,
            failed: failed.then(|| Self::failed_line(t, e.index)).flatten(),
        });
        a.loops
            .drain(..a.loops.len().saturating_sub(cp::LOOP_LINES));
        a.est = e.compiled.est_tokens;
        a.sensitive |= self.sensitive_request(&e.compiled.request);
        let window = self.cfg.routing.task_checkpoints.min_loops_between;
        // A flight a window old is given up: its late answer is dropped.
        if a.flight
            .as_ref()
            .is_some_and(|f| e.index.saturating_sub(f.asked) >= window)
        {
            a.flight = None;
        }
        let trigger = a.tally.observe(
            &self.cfg.routing.task_checkpoints,
            &seen,
            a.flight.is_some(),
        );
        if let (Some(trigger), true) = (trigger, e.more) {
            self.checkpoint_ask(t, &mut a, trigger, e.index);
        }
        t.checkpoint.state = State::On(a);
    }

    /// Ask the route pack about the turn's recent state, unless the judge cannot
    /// answer or the task's budget cannot cover it.
    fn checkpoint_ask(&self, t: &mut Turn<'_>, a: &mut Active, trigger: Trigger, index: u32) {
        if self.judge.breaker_open() || !self.judge.reachable() || self.judge.jev_unreachable() {
            return;
        }
        let sid = t.tc.session_id;
        let left = self
            .kernel
            .execution(t.tc.execution_id)
            .ok()
            .flatten()
            .map_or(u64::MAX, |e| e.budget.available());
        if left < RESERVE_MICROS {
            Self::record_checkpoint(t, a, Skipped(&trigger), index);
            return;
        }
        let plan: Vec<(String, &str)> = a.plan.iter().map(|(n, s)| (n.clone(), s.name())).collect();
        let text = cp::text(&a.brief, &plan, &a.loops, &trigger);
        let class = t.tc.class;
        let place = self.outbox.try_target(sid).ok().flatten();
        let route = self.route_mode(t.target, sid);
        let rx = self.judge.at_checkpoint(
            &mut t.trace,
            Inbound {
                session_id: sid.into(),
                execution_id: t.tc.execution_id.into(),
                turn_id: t.tc.turn_id.into(),
                node_id: String::new(),
                text,
                place_kind: place_kind(place.as_deref(), "harness", class),
                author: author_of(class).into(),
                kernel: self.kernel.clone(),
                route,
                chosen: None,
                task: a.task,
                threads: Vec::new(),
                persona: self.persona_ctx(sid, class),
                capture: false,
                data_names: self.cfg.routing.data.names(&self.cfg.privacy),
            },
        );
        a.flight = rx.map(|rx| Flight {
            rx,
            trigger,
            asked: index,
        });
    }

    /// At a loop boundary: the answer of the checkpoint in flight, if it has
    /// come. A verdict is recorded, and a move is made now, so the loop about
    /// to run is on the new model. A `Failure` is the data floor's: no
    /// profile of US origin is left to run the task on.
    pub(super) fn checkpoint_apply<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        provider: &mut Option<Arc<dyn Provider>>,
        slots: &'a Slots,
    ) -> Result<Option<Failure>> {
        let State::On(mut a) = std::mem::take(&mut t.checkpoint.state) else {
            return Ok(None);
        };
        let came = a.flight.as_mut().map(|f| f.rx.try_recv());
        let out = match came {
            Some(Ok(answered)) => {
                let f = a.flight.take().expect("a flight came");
                match answered.verdict {
                    Some(v) => self.checkpoint_verdict(
                        t,
                        session,
                        (spec, provider),
                        slots,
                        &mut a,
                        (&v, &f, answered.cost_micros),
                    ),
                    None => Ok(None),
                }
            }
            Some(Err(tokio::sync::oneshot::error::TryRecvError::Closed)) => {
                a.flight = None;
                Ok(None)
            }
            _ => Ok(None),
        };
        t.checkpoint.state = State::On(a);
        out
    }

    /// The verdict's decision, its row, and the move it makes.
    fn checkpoint_verdict<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        (spec, provider): (&mut RequestSpec, &mut Option<Arc<dyn Provider>>),
        slots: &'a Slots,
        a: &mut Active,
        (v, f, cost): (&Verdict, &Flight, u64),
    ) -> Result<Option<Failure>> {
        t.route.sensitive |= a.sensitive;
        let d = self.checkpoint_decision(t, session, a, v);
        let d = match self.data_floor(t, Some(d)) {
            Ok(d) => d.expect("a decision"),
            Err(failure) => return Ok(Some(failure)),
        };
        match &d.hold {
            HoldNext::Keep => {}
            HoldNext::Clear => a.hold = None,
            HoldNext::Set(h) => a.hold = Some(h.clone()),
        }
        let from = t.target.profile.clone();
        let target = (d.profile != from && a.used < SLOTS)
            .then(|| {
                let to = self
                    .resolve_target(&from, Some(&d.profile), None, None)
                    .ok()?;
                let to = to.speaking_as(t.target);
                let p = self.providers.get(&to.provider).cloned()?;
                Some((to, p))
            })
            .flatten();
        let effort = target
            .as_ref()
            .and_then(|_| self.route_effort(t, v, Some(&d), (&*spec, a.est), true));
        let cold = target
            .as_ref()
            .map(|(to, _)| self.cold_of(t, session, a.est, to));
        let at = f.asked + 1;
        Self::record_checkpoint(
            t,
            a,
            Verdicted {
                v,
                d: &d,
                trigger: &f.trigger,
                effort: effort.as_ref(),
                cold,
                cost,
                moved: target.is_some(),
            },
            at,
        );
        let Some((to, p)) = target else {
            return Ok(None);
        };
        let slot = &slots[a.used];
        a.used += 1;
        self.route_to(t, session, spec, slot, to, &d)?;
        *provider = Some(p);
        if let Some(e) = effort.filter(EffortDecision::applied) {
            let entry = self.catalog.get(&spec.model);
            if crate::compiler::effort::takes(spec, entry, &spec.model) {
                spec.turn_effort = e.ran;
            } else {
                spec.effort = e.sent.or(e.ran);
            }
        }
        a.moves += u32::from(d.switch);
        t.route.result = Some(TurnRoute {
            mode: Some(v.mode.clone()),
            reason: d.reason.as_str().into(),
            from,
            source: Some(format!("{}: {}", a.source(), f.trigger.key())),
            effort: v.effort.as_ref().map(|e| e.level.clone()),
            effort_reason: effort.as_ref().map(|e| e.reason.as_str().into()),
            effort_applied: effort
                .filter(EffortDecision::applied)
                .and_then(|e| e.sent.or(e.ran))
                .map(|e| effort::name(e).into()),
            persona: t.route.result.take().and_then(|r| r.persona),
        });
        Ok(None)
    }

    /// The rule's answer, the warm prefix on the held target taken off the
    /// estimate as `decide_route` takes it (theseus-rr15).
    fn checkpoint_decision(
        &self,
        t: &Turn<'_>,
        session: &SessionRecord,
        a: &Active,
        v: &Verdict,
    ) -> Decision {
        let profiles = self.route_profiles(t.route.sensitive);
        let cap = Self::place_cap(t, &profiles);
        let ask = |est_tokens| {
            cp::decide(
                &self.cfg.routing,
                &profiles,
                &CheckpointAsk {
                    verdict: v,
                    base: &a.base,
                    current: &t.target.profile,
                    est_tokens,
                    hold: a.hold.as_ref(),
                    images: t.route.images,
                    cap,
                    moves: a.moves,
                },
            )
        };
        let d = ask(a.est);
        let (Reason::CacheHold, HoldNext::Set(h)) = (d.reason, &d.hold) else {
            return d;
        };
        if h.mode == routing::STEP_DOWN {
            return d;
        }
        let warm = self
            .resolve_target(&t.target.profile, Some(&h.profile), None, None)
            .map_or(0, |to| self.warm_of(t, session, &to));
        match warm {
            0 => d,
            w => ask(a.est.saturating_sub(w)),
        }
    }

    fn warm_of(&self, t: &Turn<'_>, session: &SessionRecord, to: &Target) -> u64 {
        self.warm_prefix_tokens(to, session.kind, t.tc.session_id, t.tc.place())
    }

    /// What a move to `to` writes cold: the estimate less the warm prefix
    /// there, and its price at `to`'s catalog cache-write rate.
    fn cold_of(&self, t: &Turn<'_>, session: &SessionRecord, est: u64, to: &Target) -> (u64, f64) {
        let tokens = est.saturating_sub(self.warm_of(t, session, to));
        let per_mtok = self
            .catalog
            .get(&to.model)
            .map_or(0.0, |e| match to.cache_ttl {
                crate::config::CacheTtl::OneHour => e.cache_write_1h_per_mtok,
                crate::config::CacheTtl::FiveMinutes => e.cache_write_per_mtok,
            });
        (tokens, tokens as f64 * per_mtok / 1e6)
    }

    /// The checkpoint's one `route.decided` row.
    fn record_checkpoint(t: &mut Turn<'_>, a: &Active, what: impl Row, loop_index: u32) {
        let (v, d, trigger, ef, cold, cost, moved) = what.parts();
        let cur = t.target.profile.clone();
        let profile = d.map_or(cur.as_str(), |d| d.profile.as_str());
        let profile = if moved { profile } else { cur.as_str() };
        let answer = v.and_then(|v| v.effort.as_ref());
        let applied = ef.filter(|e| e.applied()).and_then(|e| e.sent.or(e.ran));
        t.record(&fact::route::RouteDecided {
            mode: v.map(|v| v.mode.as_str()),
            confidence: v.map(|v| v.confidence),
            judgment: v.map(|v| v.judgment.as_str()),
            from: &cur,
            profile,
            reason: d.map_or("budget", |d| d.reason.as_str()),
            detour: false,
            switch: moved && d.is_some_and(|d| d.switch),
            est_tokens: a.est,
            wait_ms: 0,
            late: false,
            answered_ms: None,
            source: Some(a.source()),
            follows: None,
            effort: answer.map(|x| x.level.as_str()),
            effort_confidence: answer.map(|x| x.confidence),
            effort_reason: ef.map(|e| e.reason.as_str()),
            effort_applied: applied.is_some(),
            effort_ran: ef.and_then(|e| e.ran).map(effort::name),
            effort_sent: ef.and_then(|e| e.sent).map(effort::name),
            checkpoint: Some(fact::route::Checkpointed {
                trigger: trigger.key(),
                loop_index,
                cold,
                cost_usd: cost as f64 / 1e6,
            }),
        });
    }
}

/// A checkpoint skipped for the task's budget.
struct Skipped<'a>(&'a Trigger);

/// A checkpoint's verdict and what became of it.
struct Verdicted<'a> {
    v: &'a Verdict,
    d: &'a Decision,
    trigger: &'a Trigger,
    effort: Option<&'a EffortDecision>,
    cold: Option<(u64, f64)>,
    cost: u64,
    moved: bool,
}

type Parts<'a> = (
    Option<&'a Verdict>,
    Option<&'a Decision>,
    &'a Trigger,
    Option<&'a EffortDecision>,
    Option<(u64, f64)>,
    u64,
    bool,
);

/// What a checkpoint's row is made from.
trait Row {
    fn parts(&self) -> Parts<'_>;
}

impl Row for Skipped<'_> {
    fn parts(&self) -> Parts<'_> {
        (None, None, self.0, None, None, 0, false)
    }
}

impl Row for Verdicted<'_> {
    fn parts(&self) -> Parts<'_> {
        (
            Some(self.v),
            Some(self.d),
            self.trigger,
            self.effort,
            self.cold,
            self.cost,
            self.moved,
        )
    }
}
