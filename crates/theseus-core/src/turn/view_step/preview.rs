//! `theseus view preview PLACE` (issue 44, `view.preview`): the view a HYPOTHETICAL next turn of the place
//! would get, opened by a message of the owner in person there, built by the turn's own assembler
//! (`TurnRunner::view_built`: the same selectors, budget and gate) from the place as it stands now, and shown
//! as `view show` shows a kept one. No model is called, nothing is sent, and nothing is written: no view
//! record, no ledger row: no persistent write and no compiled-context change (the spec is
//! `TurnRunner::preview_spec`, which resolves no thread card and compiles nothing).
//!
//! The owner's alone (`rpc::views`): the daemon cannot tell which session an agent's process runs, so it
//! previews for no agent. The point of view is the gate's (`view_gate::Verified::of_preview`), never the
//! caller's word. Every listed piece passes the shared readers (`view::record::source::GateSource`), and
//! every count is of what the reader's gate admits, node by node (its session's place today; the exchange's
//! place): an unreadable source and another place's exchange are in no count. Anything goes (2026-11-22):
//! no seal or move holds anything out.

use std::collections::{BTreeMap, HashSet};

use theseus_memory::gate::{PointOfView, Veil};
use theseus_protocol::view::{
    ViewBudget, ViewCount, ViewEarlier, ViewPreviewResult, ViewShown, ViewTally,
};

use super::{Asked, Built, RequestSpec, TurnRunner};
use crate::ladder::view::StoreLadder;
use crate::session::SessionRecord;
use crate::store::Transcript;
use crate::stub::Kind;
use crate::view::budget::Overflow;
use crate::view::record::show::{name, PieceText, Reader, Source};
use crate::view::record::source::GateSource;
use crate::view::record::ViewRecord;
use crate::view::{Pos, SourceRange, Space};
use crate::view_gate::{place_name, session_marks, Gate, Verified};

/// The most of a place's newest touches read to find the sessions that ran exchanges there.
const PLACE_TOUCHES: usize = 400;

/// The turn id a preview's pieces are made under: no turn has it.
const PREVIEW_TURN: &str = "preview";

/// What a preview is asked for (by the owner in person).
pub struct Preview<'a> {
    pub place: &'a str,
    /// The session whose next turn it is.
    pub session: &'a str,
    pub message: Option<&'a str>,
    /// Each piece's text: given only at the owner's own terminal.
    pub text: Option<&'a dyn PieceText>,
}

/// What a preview cannot say of a real turn, said with every preview.
const LIMITS: [&str; 5] = [
    "a hypothetical turn opened by your message in person: no model was called and no recall was run, so \
     recall's pieces are not in it",
    "the system part is sized from an approximation of the next request's spec (no thread card, no \
     deferred tools)",
    "not predicted: route and continuation (decided by the turn that arrives) and the room a long turn \
     makes in its own request (decided when a turn compiles it)",
    "the pieces and totals listed are code's, exchanges' and folds': a joined task's card, facts and other \
     pieces the view laid are not listed or counted here",
    "nothing was written: no view record, no ledger row, no change to what a turn compiles",
];

impl TurnRunner {
    /// The sessions of `place` the owner's look admits, newest first: a turn's own (no task, not imported)
    /// found by the place's window (its newest `WINDOW_N`, as a turn's; so adopted history counts with no
    /// change here), by the place's touches (folded history included), or as the place's by today's
    /// reading; whose own records the gate admits for the owner in person there today (its seal, which the
    /// look does not lift; its place's class); and that ran an exchange there the gate admits whole, or
    /// ran none there at all (`ran_admitted`: decided from its own exchanges, never the ladder's coverage).
    pub fn place_sessions(&self, place: &str) -> anyhow::Result<Vec<SessionRecord>> {
        use crate::view::fold::Ladder;
        let window =
            (StoreLadder(&self.store).read(place, crate::view::fold::WINDOW_N, None)?).window;
        let root = crate::exchange::Root::Place(place);
        let touched = crate::exchange::newest_before(&self.store, root, u64::MAX, PLACE_TOUCHES)?;
        let found: HashSet<String> = (window.into_iter().map(|e| e.session_id))
            .chain(touched.into_iter().map(|t| t.session_id))
            .collect();
        let mut all = Vec::new();
        for r in self.store.list_sessions::<SessionRecord>()? {
            let sid = r.session_id.as_str();
            let here = found.contains(sid) || place_name(self, sid).as_deref() == Some(place);
            if r.imported.is_some() || r.task.is_some() || !here {
                continue;
            }
            let pov = Verified::of_preview(self, sid, place);
            if self.session_admitted(pov.pov(), sid) && self.ran_admitted(place, sid)? {
                all.push(r);
            }
        }
        all.sort_by_key(|r| std::cmp::Reverse(r.last_active_ms));
        Ok(all)
    }

    /// Whether `session` ran an exchange at `place`, or ran none there at all and is the place's by today's
    /// reading: read from the session's own exchanges, so a fold's coverage of them changes nothing.
    fn ran_admitted(&self, place: &str, session: &str) -> anyhow::Result<bool> {
        let mut seen = HashSet::new();
        for (_, n) in self.store.transcript(session)?.iter() {
            let Some(turn) = n.turn_id.as_deref() else {
                continue;
            };
            if n.session_id != session || n.kind != Kind::UserMessage || !seen.insert(turn) {
                continue;
            }
            let Ok(Some(x)) = crate::exchange::exchange(&self.store, turn) else {
                continue;
            };
            if x.place == place && x.session_id == session {
                return Ok(true);
            }
        }
        Ok(place_name(self, session).as_deref() == Some(place))
    }

    /// Whether the gate admits `session`'s records to the owner's look from `pov` today: its place's marks
    /// pass the gate's check (a place that cannot be read is held).
    fn session_admitted(&self, pov: &PointOfView, session: &str) -> bool {
        Gate::check(pov, &session_marks(self, None, session, Veil::Clear)).is_ok()
    }

    /// The nodes of `session`'s transcript the gate admits from `pov`: each node's own session admitted (an
    /// inherited fork prefix is its origin's).
    fn admitted_nodes(&self, pov: &PointOfView, session: &str) -> anyhow::Result<Transcript> {
        let mut ok: BTreeMap<String, bool> = BTreeMap::new();
        Ok((self.store.transcript(session)?.iter())
            .filter(|(_, n)| {
                *(ok.entry(n.session_id.clone()))
                    .or_insert_with(|| self.session_admitted(pov, &n.session_id))
            })
            .cloned()
            .collect())
    }

    /// The view `p.session`'s next turn at `p.place` would get under `spec`, and its tally.
    pub fn view_preview(
        &self,
        p: &Preview<'_>,
        spec: &RequestSpec,
    ) -> anyhow::Result<ViewPreviewResult> {
        let store: &crate::store::Store = &self.store;
        let verified = Verified::of_preview(self, p.session, p.place);
        let gate_pov = verified.pov().clone();
        let mut notes: Vec<String> = LIMITS.iter().map(|s| (*s).to_string()).collect();
        if p.message.is_none() {
            notes.push("assembled for an empty message (`--message` gives one)".into());
        }
        let built: Built = self.view_built(
            Asked {
                store,
                sid: p.session,
                turn: PREVIEW_TURN,
                // The owner in person, as a turn of his message names him.
                person: Some(crate::exchange::OWNER.into()),
                verified: Some(verified),
                recalled: Vec::new(),
                message: Some(p.message.unwrap_or_default().to_string()),
                items: Vec::new(),
                harness: 0,
            },
            spec,
            &self.view_budget(spec),
        )?;
        let source = GateSource::new(self, gate_pov.clone(), None);
        let reader = Reader {
            source: &source,
            text: p.text,
            restricted: gate_pov.present != theseus_memory::gate::Present::OwnerAlone,
        };
        let (view, overflow, folds) = match &built.assembled {
            Ok(a) => {
                let levels: BTreeMap<&str, u8> = (a.ladder.nodes.iter())
                    .map(|n| (n.handle.0.as_str(), n.level))
                    .collect();
                let record = built.frozen.record.clone();
                let record = record.map(|v| ViewRecord::new(PREVIEW_TURN, v));
                let shown = record.as_ref().map(|r| {
                    let v = reader.show(r);
                    let folds = folds_of(&v, &levels);
                    (v, folds)
                });
                let (v, f) = shown.map_or((None, BTreeMap::new()), |(v, f)| (Some(v), f));
                (v, None, f)
            }
            Err(o) => (
                None,
                Some(overflow_said(&source, p.place, o)),
                BTreeMap::new(),
            ),
        };
        let budget = self.view_budget(spec);
        Ok(ViewPreviewResult {
            place: p.place.into(),
            session: p.session.into(),
            reader: "owner".into(),
            assembly: name(&crate::view::record::Assembly::View),
            tally: view.as_ref().map(tally_of).unwrap_or_default(),
            window: view.as_ref().map_or(0, window_of),
            view,
            // Nothing hands a turn back: a floor that cannot fit is said by `overflow` (the turn fails on
            // it), and earlier turns with no exchange are out of the view's reach (issue 44).
            fallback: None,
            overflow,
            budget: ViewBudget {
                provider_window: budget.provider_window,
                reserve: budget.reserve,
                ceiling: budget.ceiling,
                here_allowance: budget.here_allowance,
            },
            folds,
            earlier: self.earlier(&gate_pov, p.place)?,
            notes,
        })
    }

    /// The earlier turns of `place`'s sessions the owner's look admits, counted from the nodes the gate
    /// admits alone: a turn whose exchange is another place's is not counted.
    fn earlier(&self, pov: &PointOfView, place: &str) -> anyhow::Result<ViewEarlier> {
        let mut e = ViewEarlier::default();
        for r in self.place_sessions(place)? {
            e.sessions += 1;
            let mut seen = HashSet::new();
            for (_, n) in &self.admitted_nodes(pov, &r.session_id)? {
                let Some(turn) = n.turn_id.as_deref() else {
                    continue;
                };
                if n.kind != Kind::UserMessage || !seen.insert(turn.to_string()) {
                    continue;
                }
                match crate::exchange::exchange(&self.store, turn) {
                    Ok(Some(x)) if x.place != place => {}
                    Ok(Some(_)) => e.turns += 1,
                    _ => {
                        e.turns += 1;
                        e.no_exchange += 1;
                    }
                }
            }
        }
        Ok(e)
    }
}

/// The folds the listing holds, counted by level: the shared reader lists a fold only when it proves it.
fn folds_of(v: &ViewShown, levels: &BTreeMap<&str, u8>) -> BTreeMap<String, u64> {
    let mut by_level = BTreeMap::new();
    let folds = (v.slots.iter().flat_map(|s| &s.pieces)).filter(|x| x.kind == "folds");
    for level in folds.filter_map(|x| levels.get(x.handle.as_str())) {
        *by_level.entry(level.to_string()).or_default() += 1;
    }
    by_level
}

/// The floor's overflow, by its numbers, with its exchange named only when this reader's source admits
/// it; else the floor's fit alone.
fn overflow_said(source: &GateSource<'_>, place: &str, o: &Overflow) -> String {
    let Overflow::Floor {
        handle,
        tokens,
        room,
    } = o;
    let at = Pos {
        at_ms: 0,
        id: handle.0.clone(),
    };
    let range = SourceRange {
        space: Space::Exchanges {
            place: place.into(),
        },
        first: at.clone(),
        last: at,
    };
    match source.may_show(&range, None) {
        true => format!(
            "the newest exchange ({}) alone is about {tokens} tokens; the window has room for {room}",
            handle.0
        ),
        false => "the newest exchange alone does not fit the model's window".into(),
    }
}

/// Exchanges the window lays: the listed exchange pieces of slot `here`.
fn window_of(v: &ViewShown) -> u64 {
    let here = v
        .slots
        .iter()
        .filter(|s| s.slot == name(&crate::view::Slot::Here));
    here.flat_map(|s| &s.pieces)
        .filter(|x| x.kind == "exchanges")
        .count() as u64
}

/// The counts of a listed view: of what the listing holds alone, so nothing it leaves out is counted.
fn tally_of(v: &ViewShown) -> ViewTally {
    fn add(m: &mut BTreeMap<String, ViewCount>, k: &str, tokens: u64) {
        let c = m.entry(k.to_string()).or_default();
        c.pieces += 1;
        c.tokens += tokens;
    }
    let mut t = ViewTally {
        compilation: "preview".into(),
        present: v.pov.present.clone(),
        tokens: v.totals.tokens,
        ..ViewTally::default()
    };
    for s in &v.slots {
        for x in &s.pieces {
            add(&mut t.slots, &s.slot, x.tokens);
            for r in &x.reasons {
                add(&mut t.reasons, r, x.tokens);
            }
        }
    }
    for d in &v.dropped {
        add(&mut t.dropped, &d.why, d.tokens);
        for r in &d.reasons {
            add(&mut t.dropped_reasons, r, d.tokens);
        }
    }
    t
}

mod census;
#[cfg(test)]
mod tests;
