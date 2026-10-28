//! The turn's side of deferred tools (theseus-7m7w.2): the fresh choice of
//! the families a request loads (made once a turn, from the board's counts
//! in memory), what the compile sent, a response's searches, a call to a tool
//! the request did not load, and the turn's row at its end.
//!
//! The choice is the spec's `defer`; whether it applies is the compile's
//! (`tools_defer::forks`): an append keeps the loaded set its compilation
//! recorded, a recompile takes this one.

use std::collections::BTreeSet;

use theseus_protocol::modes::SessionMode;

use super::{Turn, TurnRunner};
use crate::catalog::TokenRates;
use crate::compiler::{Compiled, RequestSpec};
use crate::fact::tools_defer::{ToolsMissed, ToolsSearched, ToolsUsed};
use crate::provider::{ModelResponse, ToolUse};
use crate::tools_defer::board::{Miss, Offer};
use crate::tools_defer::fit::Fit;
use crate::tools_defer::{self, scan, Deferral, Mechanism, PlaceKind};

/// What a turn's message says about its tools: its text, the families Jev judged relevant to it,
/// whether the request is a trivial detour's (a bare one), and the prompt tier's admission
/// ceiling (`admit_ceiling` without one), and whether the turn has no input of a person's, so
/// the work it sees is work Theseus noticed itself (theseus-o1h3f.3).
#[derive(Default, Clone, Copy)]
struct Said<'a> {
    ask: &'a str,
    jev: &'a [(String, f64)],
    bare: bool,
    ceiling: Option<f64>,
    noticed: bool,
}

/// Whether a turn's author is Theseus's own pipeline (`self:backlog`, `self:mint`, `self:review`):
/// a prefix, so a stage added later is covered.
pub(super) fn is_self(author: &str) -> bool {
    author.starts_with("self:")
}

impl TurnRunner {
    /// The kind of place a session's turn speaks in, for the loaded set.
    pub(crate) fn place_kind(&self, session_id: &str, task: bool) -> PlaceKind {
        PlaceKind::of(
            task,
            self.outbox.target(session_id).as_deref(),
            !self.failed_turns.driver_retries(),
        )
    }

    /// Settle the spec's deferral for this turn: the families its place kind
    /// loads now, and how the model reaches the rest. Nothing when
    /// `[tools.defer]` is off.
    pub(super) fn defer_tools(&self, t: &Turn<'_>, spec: &mut RequestSpec) {
        self.flows_in_scope(t, spec);
        let kind = self.place_kind(t.tc.session_id, t.tc.task.is_some());
        // A detour sends the spec less its context, persona and files
        // (`detour_spec`, which its compile takes too): a named family is
        // counted on the request that goes out (theseus-3bs4e).
        let sent = t
            .route
            .keeps
            .is_some()
            .then(|| super::detour_step::detour_spec(spec));
        let counted = sent.as_ref().unwrap_or(&*spec);
        let jev = t.route.families.relevant.as_deref().unwrap_or_default();
        // A trivial detour carries `tools_find` and what its message names, and the prompt's
        // tier sets how far admitted families may raise it (theseus-d9je7).
        let bare = sent.is_some() && t.route.detour_kind == "trivial";
        let said = Said {
            ask: &t.route.ask,
            jev,
            bare,
            ceiling: t.route.injection.as_ref().map(|b| b.admit_ceiling),
            // The turn's own read of "no input" (`Turn::continuation`: the wake, the task
            // report, the routine's run) or its author's (the pipeline's own turns carry an
            // objective as input), never the session's kind: a task session's typed turn is a
            // person's (theseus-lmyvl).
            noticed: t.continuation || t.self_authored,
        };
        spec.defer = self.deferral_in(t.tc.session_id, kind, (&*spec, counted), said);
    }

    /// A loaded flow's tool stays in the request only where the flow is
    /// offered: its repository holds the session's directory, and its kinds
    /// of place include this one (theseus-pw1q.21.4). With no flow loaded
    /// this reads one lock and changes nothing.
    fn flows_in_scope(&self, t: &Turn<'_>, spec: &mut RequestSpec) {
        let flows = &self.tools.flows;
        if !flows.any() {
            return;
        }
        let kind = self.place_kind(t.tc.session_id, t.tc.task.is_some());
        let cwd = self.tools.cwd_for(t.tc.dir);
        let keep = |d: &serde_json::Value| {
            let Some(name) = d["name"]
                .as_str()
                .and_then(|n| n.strip_prefix(crate::flow::tool::WIRE_PREFIX))
            else {
                return true;
            };
            flows
                .get(name)
                .is_some_and(|e| crate::flow::tool::in_scope(&e.spec, &cwd, kind))
        };
        if spec.tools.iter().any(|d| !keep(d)) {
            spec.tools = spec
                .tools
                .iter()
                .filter(|d| keep(d))
                .cloned()
                .collect::<Vec<_>>()
                .into();
        }
    }

    /// The fresh choice for a session's request on `spec`.
    pub(crate) fn deferral(&self, sid: &str, task: bool, spec: &RequestSpec) -> Option<Deferral> {
        self.deferral_in(
            sid,
            self.place_kind(sid, task),
            (spec, spec),
            Said::default(),
        )
    }

    /// The same for a census: a fresh session's first request on a kind of place the daemon has no
    /// session in is counted with that kind's families, not the CLI's.
    pub(crate) fn census_deferral(
        &self,
        sid: &str,
        task: bool,
        fresh: Option<&str>,
        spec: &RequestSpec,
    ) -> Option<Deferral> {
        match fresh.and_then(PlaceKind::parse) {
            Some(kind) => self.deferral_in(sid, kind, (spec, spec), Said::default()),
            None => self.deferral(sid, task, spec),
        }
    }

    /// The fresh choice in a kind of place for a turn whose message said
    /// `ask`: where the model has no tool search, the families the message
    /// names load too (`tools_defer::want`, theseus-jpjcd), and those Jev
    /// judged relevant to it (`jev`, theseus-xlri5), up to the prompt tier's
    /// `ceiling` (`admit_ceiling` without one). `(spec, counted)` is the spec the
    /// families are chosen from and the request whose size they are held to (a
    /// detour's is smaller).
    fn deferral_in(
        &self,
        sid: &str,
        kind: PlaceKind,
        (spec, counted): (&RequestSpec, &RequestSpec),
        Said {
            ask,
            jev,
            bare,
            ceiling,
            noticed,
        }: Said<'_>,
    ) -> Option<Deferral> {
        let cfg = &self.cfg.tools.defer;
        if !cfg.enabled {
            return None;
        }
        let mechanism = match self.catalog.get(&spec.model) {
            Some(e) if e.tool_search => Mechanism::Search,
            _ => Mechanism::Find,
        };
        let known = tools_defer::families_of(&spec.tools);
        let now = self.kernel.now_ms();
        let board = &self.tools.defer;
        // A conversation in plan mode loads the family that holds the plan's
        // tools (theseus-lyxy), since the refusal in plan mode names one of them.
        let mut extra: BTreeSet<String> = (!bare
            && self.tools.modes.mode_of(sid) == SessionMode::Plan)
            .then(|| tools_defer::PLAN_FAMILY.to_string())
            .into_iter()
            .collect();
        // Learned families stay inside the place's budget (theseus-buz3h).
        let rates = self
            .catalog
            .get(&spec.model)
            .map_or_else(|| TokenRates::of(&spec.model), |e| e.bytes_per_token);
        let multiple = ceiling.unwrap_or(cfg.admit_ceiling);
        let mut fit = Fit::of(kind, counted, rates, mechanism, multiple);
        let offer = |extra: &BTreeSet<String>, fit: &Fit| {
            board
                .select(
                    sid,
                    kind,
                    cfg,
                    now,
                    &Offer {
                        known: &known,
                        extra,
                        fit: Some(fit),
                    },
                )
                .loaded()
        };
        // A bare request (a trivial detour's, on a model without tool search) loads nothing
        // the place, the board or the conversation chose: only a family the message names, or
        // Jev judged relevant, below. The provider's own search keeps the whole set (the API
        // refuses a request whose tools are all deferred).
        let bare = bare && mechanism == Mechanism::Find;
        let mut loaded = match bare {
            true => BTreeSet::new(),
            false => offer(&extra, &fit),
        };
        // A message that asks for a task loads the tasks family whichever way the model reaches
        // the rest (theseus-gg78p), held to the ceiling as `want::extra` holds it. Left out, it
        // stays deferred and searchable (theseus-s48k6).
        if mechanism == Mechanism::Search
            && !bare
            && known.contains(tools_defer::family::TASKS)
            && tools_defer::want::asks_for_task(ask)
            && !loaded.contains(tools_defer::family::TASKS)
            && tools_defer::want::fits(&fit, &loaded, tools_defer::family::TASKS)
        {
            extra.insert(tools_defer::family::TASKS.to_string());
            loaded = offer(&extra, &fit);
        }
        // A turn with no person's input loads the tasks too: no budget was promised to a person
        // there, and the model cannot start work it noticed from a tool it cannot see
        // (theseus-o1h3f.3). Not held to the place's ceiling: the ceiling is the owner's promise about
        // what a person's turn costs, and 13,290 tokens of tasks alone would break it.
        if noticed
            && !bare
            && known.contains(tools_defer::family::TASKS)
            && !loaded.contains(tools_defer::family::TASKS)
        {
            // It rides past the place's budget by exactly its own size, as an admitted family does
            // (`fit.rs`): the learned families stay held to the budget plus that raise, so admitting
            // the tasks evicts none of them.
            let whole = fit.total(&loaded) + fit.tokens(tools_defer::family::TASKS);
            fit.raised = fit.raised.max(whole.saturating_sub(fit.budget));
            extra.insert(tools_defer::family::TASKS.to_string());
            loaded = offer(&extra, &fit);
        }
        // An append keeps the families its compilation recorded, so the family joins as a find's
        // does (`Deferral::over`): the request changes once, where the wake's turn is.
        let added: BTreeSet<String> = (noticed && loaded.contains(tools_defer::family::TASKS))
            .then(|| tools_defer::family::TASKS.to_string())
            .into_iter()
            .collect();
        let mut raised = None;
        if mechanism == Mechanism::Find {
            // A family the message names, or Jev judged relevant, is held to the whole request, as
            // the learned loads are (`fit`), and may raise it past the budget to the ceiling
            // (theseus-xlri5); the learned families stay held to the budget plus that raise.
            let wanted = tools_defer::want::extra(ask, jev, &spec.tools, &loaded, &fit);
            if !wanted.is_empty() {
                let base = fit.total(&loaded);
                let whole = base + wanted.iter().map(|a| a.tokens).sum::<u64>();
                fit.raised = whole.saturating_sub(fit.budget);
                raised = (fit.raised > 0).then(|| tools_defer::raised(&fit, base, whole, &wanted));
                extra.extend(wanted.into_iter().map(|a| a.family));
                loaded = match bare {
                    true => extra,
                    false => offer(&extra, &fit),
                };
            }
        }
        // The converse of `added`: a person's turn drops a family only the loosening loaded, so the
        // append's tools differ from the compilation's and it recompiles without it. Read after the
        // message's own admissions, so a family this turn loads is never dropped.
        let dropped: BTreeSet<String> = (!noticed && !loaded.contains(tools_defer::family::TASKS))
            .then(|| tools_defer::family::TASKS.to_string())
            .into_iter()
            .collect();
        Some(Deferral {
            mechanism,
            loaded,
            resolved: false,
            added,
            dropped,
            raised,
            bare,
        })
    }

    /// The families a `tools_find` of the last loop asked for join this
    /// turn's tools now, so the model can call a found tool in the turn that
    /// found it (theseus-342qy). The next compile sees tools its compilation
    /// did not record and recompiles (`tools_changed`); tools lead the cached
    /// prefix, so that request misses the cache either way, and the turns
    /// after append cache-warm. Only where the model has no tool search: a
    /// provider's own search loads in the same turn already.
    pub(super) fn defer_found(&self, t: &Turn<'_>, spec: &mut RequestSpec) {
        let found = self.tools.defer.take_found(t.tc.session_id);
        if found.is_empty() {
            return;
        }
        let Some(d) = spec.defer.as_mut().filter(|d| !d.resolved) else {
            return;
        };
        if d.mechanism != Mechanism::Find {
            return;
        }
        let known = tools_defer::families_of(&spec.tools);
        let found: BTreeSet<String> = found.into_iter().filter(|f| known.contains(f)).collect();
        d.loaded.extend(found.iter().cloned());
        d.added.extend(found);
    }

    /// What the loop's compile sent: the families it loaded and the tools it
    /// hides, which `tools_find` and the answer to a miss read.
    pub(super) fn defer_noted(&self, t: &Turn<'_>, spec: &RequestSpec, compiled: &Compiled) {
        if !self.cfg.tools.defer.enabled {
            return;
        }
        let sid = t.tc.session_id;
        let kind = self.place_kind(sid, t.tc.task.is_some());
        let (mechanism, loaded, hidden) = match &compiled.compilation.manifest.loaded {
            Some(l) => (
                spec.defer.as_ref().map(|d| d.mechanism),
                l.iter().cloned().collect(),
                tools_defer::hidden_of(&spec.tools, &compiled.request.tools),
            ),
            None => (None, BTreeSet::new(), vec![]),
        };
        // The raise holds where the compile took the fresh set: an append over a set recorded
        // before it raised nothing this turn (theseus-xlri5).
        let raised = spec
            .defer
            .as_ref()
            .and_then(|d| d.raised.clone())
            .filter(|r| r.families.iter().all(|f| loaded.contains(&f.family)));
        self.tools.defer.note_raised(sid, raised);
        self.tools
            .defer
            .note_request(sid, kind, mechanism, loaded, hidden);
    }

    /// The searches in a response, noted before its calls are run: a call to
    /// a tool a search of this response surfaced is no miss.
    pub(super) fn defer_response(&self, t: &Turn<'_>, resp: &ModelResponse) {
        if !self.cfg.tools.defer.enabled {
            return;
        }
        for f in scan::searches_in(&resp.content) {
            self.tools
                .defer
                .searched(t.tc.session_id, &f.query, &f.tools);
        }
    }

    /// The calls of a response that can run: one to a tool the request did not
    /// load, with no search that surfaced it, is answered with words naming the
    /// search and not run (`tools.missed`). Returns the rest, and how many
    /// were answered here.
    pub(super) fn defer_unloaded(
        &self,
        t: &mut Turn<'_>,
        tc: &crate::toolrun::TurnCtx<'_>,
        uses: &[ToolUse],
    ) -> anyhow::Result<(Vec<ToolUse>, u32)> {
        if !self.cfg.tools.defer.enabled {
            return Ok((uses.to_vec(), 0));
        }
        let sid = t.tc.session_id;
        let kind = self.place_kind(sid, t.tc.task.is_some());
        let board = &self.tools.defer;
        let (mut rest, mut answered) = (Vec::with_capacity(uses.len()), 0);
        for u in uses {
            match board.is_unsurfaced(sid, &u.name) {
                Some(h) => {
                    let miss = Miss {
                        tool: u.name.clone(),
                        family: h.family.clone(),
                    };
                    let words = tools_defer::find::miss_words(&h, board.mechanism(sid));
                    self.tools.not_run(tc, u, &words)?;
                    t.record(&ToolsMissed {
                        place: kind,
                        miss: &miss,
                    });
                    board.missed(sid, miss);
                    answered += 1;
                }
                None => {
                    board.called(sid, &u.name);
                    rest.push(u.clone());
                }
            }
        }
        Ok((rest, answered))
    }

    /// The turn's rows: its use of its tools, and each search it made. Returns
    /// what its request admitted past the place's budget, for its result.
    pub(super) fn defer_ended(
        &self,
        t: &mut Turn<'_>,
    ) -> Option<theseus_protocol::route::ToolsRaised> {
        if !self.cfg.tools.defer.enabled {
            return None;
        }
        let sid = t.tc.session_id;
        let kind = self.place_kind(sid, t.tc.task.is_some());
        let turn = self.tools.defer.end_turn(sid, kind, self.kernel.now_ms());
        if turn.loaded.is_empty() && turn.used.is_empty() && turn.searches.is_empty() {
            return turn.raised;
        }
        let board = &self.tools.defer;
        board.emit("used", turn.kind, &turn.used);
        board.emit(
            "searched",
            turn.kind,
            turn.searches.iter().flat_map(|s| &s.families),
        );
        board.emit("missed", turn.kind, turn.misses.iter().map(|m| &m.family));
        board.emit("unused", turn.kind, &turn.unused);
        t.record(&ToolsUsed { turn: &turn });
        for s in &turn.searches {
            t.record(&ToolsSearched {
                place: turn.kind,
                search: s,
            });
        }
        turn.raised
    }
}
