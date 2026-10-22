//! The open request, per request (theseus-a1wvq.8): a message Jev judged
//! `open` at the act band may reach a model of restricted origin, carrying
//! only its open run, compiled in the open view, with a pass for that one
//! request.
//!
//! - **Admission** (`admit`, after the verdict is read): `data::admits` over
//!   what the turn holds: the input and the node before it, the verdict's
//!   `data` answer, the pack acting, the thread's seal, an attachment. Its
//!   run extends the stored one only by the chain. A turn that asked no
//!   route has no verdict: `no_verdict`, but an open session's later turn is
//!   admitted with its stored run while nothing came after it.
//! - **Where**: an admitted turn may run on a restricted profile only when
//!   its request is compiled in the open view: an open session, or a detour
//!   (`data_floor`), which compiles through `open_request` over the run.
//! - **The pass** (`data_pass`, before `call_model`): the request's
//!   `data::Carried`, its `data::clearance`, and a pass for its digest when
//!   cleared. Each request's clearance is kept on the turn.
//! - **A later loop** whose nodes leave the run (a tool's result the turn did
//!   not screen, `screen_step.rs`; anything but the loop's own answer, calls
//!   and screened results) moves to the `[model]` default with
//!   `data_gate` before it compiles (`data_move`), as a plain detour.
//! - **The run is stored** in the turn's own session write when every request
//!   went out cleared and nothing but the turn's own loop came after the run
//!   (`data_end`); an open session's first input or request not admitted is
//!   its taint, recorded once. Bookkeeping: the taint closes `admits`, and
//!   never a Claude target.

use super::*;
use crate::data::{self, Carried, Carry, Clearance, DataWhy, OpenRun, Taint, Writer};
use crate::provider::Provider;
use crate::routing::{Decision, HoldNext, Reason, Verdict};
use crate::stub::{Kind, Stub};

/// The turn's admission (`RouteState.admitted`): unchecked until read.
pub(crate) struct Admitted(pub(crate) Result<OpenRun, DataWhy>);

impl Default for Admitted {
    fn default() -> Self {
        Self(Err(DataWhy::Unchecked))
    }
}

/// What the turn holds of its open requests.
#[derive(Default)]
pub(super) struct Held {
    /// An open task's brief was asked about: the first compile waits for its
    /// verdict without `[routing] max_wait_ms`'s cut.
    pub(super) brief: bool,
    /// Each request's clearance, in order.
    pub(super) requests: Vec<Clearance>,
    /// The first input or request of an open session not admitted.
    pub(super) taint: Option<Taint>,
    /// A later loop moved off a restricted model.
    moved: bool,
    /// The nodes the last request carried (theseus-a1wvq.11).
    pub(super) run_nodes: usize,
    /// The `Summary` nodes this turn's compaction wrote over a range its run
    /// cleared: the loop's own, as `writer` reads them.
    pub(super) summaries: Vec<String>,
}

/// The targets a turn may move to, each set once: routing's, a refusal's
/// fallback, its checkpoints', and a later loop's move off a restricted
/// model (`data_move`).
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

/// Who wrote `n`, for a request of `turn`; `summaries` are its compaction's
/// summaries of a cleared range.
pub(super) fn writer(n: &Stub, turn: &str, screened: &[String], summaries: &[String]) -> Writer {
    match n.kind {
        Kind::ToolResult if screened.contains(&n.id) => Writer::Loop,
        Kind::ToolResult => Writer::ToolResult,
        Kind::Summary if summaries.contains(&n.id) => Writer::Loop,
        Kind::AssistantMessage | Kind::ToolCall | Kind::Recall
            if n.turn_id.as_deref() == Some(turn) =>
        {
            Writer::Loop
        }
        _ => Writer::Other,
    }
}

impl TurnRunner {
    /// The turn's admission, once its verdict is read (or none came): kept on
    /// the turn, and an open session's refusal kept as its taint.
    pub(super) fn data_admit(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        verdict: Option<&Verdict>,
        live: bool,
        late: bool,
    ) {
        let Ok(nodes) = t.tc.store.transcript(t.tc.session_id) else {
            return;
        };
        let turn = t.tc.turn_id;
        // The input: the turn's own message, else a task's brief, which the
        // first turn's verdict judges.
        let at = match t.continuation {
            false => nodes.iter().rposition(|(_, n)| {
                n.kind == Kind::UserMessage && n.turn_id.as_deref() == Some(turn)
            }),
            true => (t.tc.task.is_some() && !nodes.is_empty()).then_some(0),
        };
        let Some(at) = at else {
            return self.data_admit_unrouted(t, session);
        };
        let input = &nodes[at].1;
        let attachment = match &input.body {
            Body::UserMessage { attachments, .. } => !attachments.is_empty(),
            _ => true,
        };
        let v = verdict.filter(|v| v.turn == turn);
        let answer = v.and_then(|v| v.data.as_deref());
        let seal = v
            .and_then(|v| v.seal.as_ref())
            .is_some_and(|s| s.recommends());
        // The thread's board is read only for an answer that could admit.
        let open = answer.is_some_and(|a| a.choice == "open");
        let admitted = data::admits(
            session.data.as_deref(),
            &data::Check {
                input: &input.id,
                before: at.checked_sub(1).map(|b| nodes[b].1.id.as_str()),
                answer,
                late: late || (verdict.is_some() && v.is_none()),
                live,
                sealed: seal || (open && self.sealed(t.tc.session_id)),
                attachment,
            },
        );
        let first = nodes.first().map(|(_, n)| n.id.as_str());
        self.data_admitted(t, session, admitted, first, Some(input.id.clone()));
    }

    /// A turn that asked no route: its input has no verdict, but an open
    /// session's turn with no new input is admitted with its stored run while
    /// nothing came after it.
    pub(super) fn data_admit_unrouted(&self, t: &mut Turn<'_>, session: &SessionRecord) {
        let nodes = match (self.open_of(session), t.continuation) {
            (Some(_), true) => t.tc.store.transcript(t.tc.session_id).ok(),
            _ => None,
        };
        let Some(nodes) = nodes else {
            return self.data_admitted(t, session, Err(DataWhy::NoVerdict), None, None);
        };
        let last = nodes.last().map(|(_, n)| n.id.as_str());
        let admitted = data::admits_again(session.data.as_deref(), last);
        let first = nodes.first().map(|(_, n)| n.id.as_str());
        self.data_admitted(t, session, admitted, first, None);
    }

    /// The admission kept. An open session's request is its own compile,
    /// which carries its history from its start, so its run must begin at
    /// its first node (else `not_in_run`, before the route decides). Its
    /// refusal for what was judged or carried (`DataWhy::taints`) is its
    /// taint.
    fn data_admitted(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        admitted: Result<OpenRun, DataWhy>,
        first: Option<&str>,
        node: Option<String>,
    ) {
        let open = self.open_of(session).is_some();
        let admitted = admitted.and_then(|run| match open && first != Some(run.from.as_str()) {
            true => Err(DataWhy::NotInRun),
            false => Ok(run),
        });
        let why = admitted
            .as_ref()
            .err()
            .copied()
            .filter(|w| open && w.taints());
        if let Some(why) = why {
            t.data.taint.get_or_insert_with(|| Taint {
                why,
                node,
                at_ms: theseus_protocol::now_unix_ms(),
            });
        }
        t.route.admitted = Admitted(admitted);
    }

    /// How long the first compile waits for the verdict: an open task's
    /// brief as long as Jev's own call runs, since a task's first turn is not
    /// latency-bound; any other, `[routing] max_wait_ms`.
    pub(super) fn route_wait(&self, t: &Turn<'_>) -> Duration {
        match t.data.brief {
            true => Duration::from_secs(self.cfg.judge.total_secs.saturating_add(1)),
            false => Duration::from_millis(self.cfg.routing.max_wait_ms),
        }
    }

    /// The session's thread is sealed or a seal is pending; a board that
    /// does not read is sealed.
    fn sealed(&self, sid: &str) -> bool {
        self.threads.all(&self.store).map_or(true, |b| {
            b.of_session(sid)
                .is_some_and(|t| t.sealed.is_some() || t.seal_pending.is_some())
        })
    }

    /// What `compiled` carries of the transcript: its prefix's nodes and its
    /// tail's, each with its writer, and the run's first's position.
    fn carried(t: &Turn<'_>, run: &OpenRun, compiled: &Compiled) -> Result<Carried> {
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let c = &compiled.compilation;
        let includes: std::collections::HashSet<&str> =
            c.includes.iter().map(String::as_str).collect();
        Ok(Carried {
            open_view: t.tc.open.is_some(),
            from: nodes
                .iter()
                .find(|(_, n)| n.id == run.from)
                .map(|(p, _)| *p),
            nodes: nodes
                .iter()
                .filter(|(p, n)| includes.contains(n.id.as_str()) || *p > c.as_of)
                .map(|(p, n)| Carry {
                    id: n.id.clone(),
                    at: *p,
                    writer: writer(n, t.tc.turn_id, &t.route.screened, &t.data.summaries),
                })
                .collect(),
        })
    }

    /// The pass for the request about to be sent (theseus-a1wvq.3's seam):
    /// its own clearance, kept on the turn, and a pass for its digest when
    /// cleared. A request that fails gets none, so the chokepoint refuses it
    /// if anything still sends it to a restricted model.
    pub(super) fn data_pass(t: &mut Turn<'_>, compiled: &mut Compiled) {
        let mut run_nodes = 0;
        let cleared = match (&t.route.admitted.0, t.tc.open) {
            (Err(why), _) => Clearance::closed(*why),
            (Ok(_), None) => Clearance::closed(DataWhy::PrivateView),
            (Ok(run), Some(_)) => Self::carried(t, run, compiled).map_or_else(
                |_| Clearance::closed(DataWhy::Unchecked),
                |c| {
                    run_nodes = c.nodes.len();
                    data::clearance(run, &c)
                },
            ),
        };
        t.data.run_nodes = run_nodes;
        if cleared.is_cleared() {
            let digest = compiled.request.digest();
            compiled.request.data_pass = cleared.pass(&digest);
        }
        t.data.requests.push(cleared);
    }

    /// Whether the open detour compiles in the open view: its target is of
    /// restricted origin and the turn is admitted.
    pub(super) fn open_detour(&self, t: &Turn<'_>) -> Option<OpenRun> {
        let run = t.route.admitted.0.as_ref().ok()?;
        self.restricted_profile(&t.target.profile)
            .then(|| run.clone())
    }

    /// An open detour's request (theseus-a1wvq.8): compiled through
    /// `open_request` over the turn's run, its calls gated in the open view,
    /// with the detour's tail. It carries no recall: `open_request` draws
    /// none, and the turn's own is dropped.
    pub(super) fn compile_open_detour(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        run: &OpenRun,
        i: u32,
    ) -> Result<Result<Compiled, Failure>> {
        let open = self.open_request(session, t.target, run)?;
        t.tc.open = open.view.open;
        t.route.detour_recall = false;
        // The standing records the open view keeps, which the check holds
        // its request to.
        self.stand(t, &mut open.spec.clone());
        let mut compiled = open.compiled;
        Self::standing_here(t, &mut compiled);
        Self::persona_line(t, &mut compiled);
        self.defer_noted(t, &open.spec, &compiled);
        let last = self.store.last_position();
        if let Some(f) = Self::unadmitted(t, &compiled, &open.nodes, last, false, i) {
            return Ok(Err(f));
        }
        if let Some(f) = Self::standing_dropped(t, &compiled, i) {
            return Ok(Err(f));
        }
        t.record(&fact::turn::LoopStarted {
            turn_id: t.tc.turn_id,
            index: i,
            model: &t.target.model,
            tools_offered: open.spec.tools.len() as u32,
        });
        Ok(Ok(compiled))
    }

    /// A later loop on a restricted model whose request would carry a node
    /// past the run that the loop did not write (a tool's result that failed
    /// the screen) moves to the `[model]` default with `data_gate` before
    /// it compiles, once a turn; a detour then compiles as a plain one, in
    /// the session's own view. The turn stores no run.
    pub(super) fn data_move<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut SessionRecord,
        spec: &mut RequestSpec,
        provider: &mut Option<Arc<dyn Provider>>,
        slot: &'a OnceLock<Target>,
    ) -> Result<Option<Failure>> {
        if t.data.moved || !self.restricted_profile(&t.target.profile) {
            return Ok(None);
        }
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let since = match &t.route.admitted.0 {
            Ok(run) => nodes.iter().position(|(_, n)| n.id == run.through),
            Err(_) => None,
        };
        let left = since.is_none_or(|at| {
            nodes[at + 1..].iter().any(|(_, n)| {
                writer(n, t.tc.turn_id, &t.route.screened, &t.data.summaries) != Writer::Loop
            })
        });
        if !left {
            return Ok(None);
        }
        if let Some(f) = Self::draft_stopped(t, "a node past its open run") {
            return Ok(Some(f));
        }
        t.data.moved = true;
        let d = Decision {
            profile: super::data_step::DEFAULT_PROFILE.into(),
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
            return Ok(Some(super::data_step::refusal(&format!(
                "`{}` does not resolve",
                d.profile
            ))));
        };
        let p = p.clone();
        t.route.gated = Some(DataWhy::ToolResult.as_str());
        self.record_route(t, None, Some(&d), None, Default::default(), None);
        if t.route.keeps.is_some() {
            // The detour goes on in the session's own view.
            t.tc.open = self.open_of(session);
        }
        self.route_to(t, session, spec, slot, to, &d)?;
        *provider = Some(p);
        Ok(None)
    }

    /// The run, in the turn's own session write: stored when every request
    /// went out cleared and only the turn's own loop wrote after the run's
    /// last node; and an open session's taint, once.
    pub(super) fn data_end(&self, t: &mut Turn<'_>, session: &mut SessionRecord) {
        let open = self.open_of(session).is_some();
        let cleared = !t.data.requests.is_empty()
            && t.data.requests.iter().all(Clearance::is_cleared)
            && t.tc.open.is_some();
        if let (true, Ok(run)) = (cleared, &t.route.admitted.0) {
            if let Ok(nodes) = t.tc.store.transcript(t.tc.session_id) {
                let at = nodes.iter().position(|(_, n)| n.id == run.through);
                let own = at.is_some_and(|at| {
                    nodes[at + 1..].iter().all(|(_, n)| {
                        writer(n, t.tc.turn_id, &t.route.screened, &t.data.summaries)
                            == Writer::Loop
                    })
                });
                if let (true, Some((_, last))) = (own, nodes.last()) {
                    let d = session.data.get_or_insert_with(Default::default);
                    d.run = Some(OpenRun {
                        from: run.from.clone(),
                        through: last.id.clone(),
                    });
                }
            }
        }
        if open && t.data.taint.is_none() {
            let why = t.data.requests.iter().find_map(Clearance::why);
            if let Some(why) = why.filter(|w| w.taints()) {
                t.data.taint = Some(Taint {
                    why,
                    node: None,
                    at_ms: theseus_protocol::now_unix_ms(),
                });
            }
        }
        let Some(taint) = t.data.taint.take().filter(|_| open) else {
            return;
        };
        let d = session.data.get_or_insert_with(Default::default);
        if d.taint.is_none() {
            t.record(&fact::data::SessionDataTainted {
                why: taint.why,
                node: taint.node.as_deref(),
            });
            d.taint = Some(taint);
        }
    }
}
