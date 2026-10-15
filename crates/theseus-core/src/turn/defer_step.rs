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
use crate::compiler::{Compiled, RequestSpec};
use crate::fact::tools_defer::{ToolsMissed, ToolsSearched, ToolsUsed};
use crate::provider::{ModelResponse, ToolUse};
use crate::tools_defer::board::Miss;
use crate::tools_defer::{self, scan, Deferral, Mechanism, PlaceKind};

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
        let kind = self.place_kind(t.tc.session_id, t.tc.task.is_some());
        spec.defer = self.deferral_in(t.tc.session_id, kind, spec, &t.route.ask);
    }

    /// The fresh choice for a session's request on `spec`.
    pub(crate) fn deferral(&self, sid: &str, task: bool, spec: &RequestSpec) -> Option<Deferral> {
        self.deferral_in(sid, self.place_kind(sid, task), spec, "")
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
            Some(kind) => self.deferral_in(sid, kind, spec, ""),
            None => self.deferral(sid, task, spec),
        }
    }

    /// The fresh choice in a kind of place for a turn whose message said
    /// `ask`: where the model has no tool search, the families the message
    /// names load too (`tools_defer::want`, theseus-jpjcd).
    fn deferral_in(
        &self,
        sid: &str,
        kind: PlaceKind,
        spec: &RequestSpec,
        ask: &str,
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
        let mut extra: BTreeSet<String> = (self.tools.earned.modes.mode_of(sid)
            == SessionMode::Plan)
            .then(|| tools_defer::PLAN_FAMILY.to_string())
            .into_iter()
            .collect();
        let mut loaded = board.select(sid, kind, cfg, now, &known, &extra).loaded();
        if mechanism == Mechanism::Find {
            let wanted = tools_defer::want::extra(ask, kind, &spec.tools, &loaded);
            if !wanted.is_empty() {
                extra.extend(wanted);
                loaded = board.select(sid, kind, cfg, now, &known, &extra).loaded();
            }
        }
        Some(Deferral {
            mechanism,
            loaded,
            resolved: false,
        })
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

    /// The turn's rows: its use of its tools, and each search it made.
    pub(super) fn defer_ended(&self, t: &mut Turn<'_>) {
        if !self.cfg.tools.defer.enabled {
            return;
        }
        let sid = t.tc.session_id;
        let kind = self.place_kind(sid, t.tc.task.is_some());
        let turn = self.tools.defer.end_turn(sid, kind, self.kernel.now_ms());
        if turn.loaded.is_empty() && turn.used.is_empty() && turn.searches.is_empty() {
            return;
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
    }
}
