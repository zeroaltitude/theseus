//! `theseus view preview PLACE` (issue 44, `view.preview`): the view a place's next turn would get, built
//! by the turn's own assembler (`TurnRunner::view_built`: the same selectors, budget and gate) from the
//! place as it stands now, and shown as `view show` shows a kept one. No model is called and nothing is sent
//! or written: no view record, no ledger row (a turn's fallback row is not written either).
//!
//! The point of view is the gate's (`view_gate::Verified::of_preview`), never the caller's word: the owner
//! in person at that place, or an agent's turn there. Every count is of what this reader is listed, after
//! the gate: a piece the listing leaves out is in no count, and a sealed session's earlier turns are not
//! counted at all.

use std::collections::{BTreeMap, HashSet};

use theseus_protocol::view::{
    ViewBudget, ViewCount, ViewEarlier, ViewPreviewResult, ViewShown, ViewTally,
};

use super::{unledgered, Asked, Built, Fallback, RequestSpec, TurnRunner};
use crate::ladder::view::StoreLadder;
use crate::session::SessionRecord;
use crate::stub::Kind;
use crate::view::record::show::{name, PieceText, Reader};
use crate::view::record::source::GateSource;
use crate::view::record::ViewRecord;
use crate::view_gate::{place_name, seal_of, Verified};
use theseus_memory::gate::{Present, Seal};

/// The turn id a preview's pieces are made under: no turn has it.
const PREVIEW_TURN: &str = "preview";

/// What a preview is asked for.
pub struct Preview<'a> {
    pub place: &'a str,
    /// The session whose next turn it is.
    pub session: &'a str,
    /// The owner in person (else an agent's process).
    pub owner: bool,
    pub message: Option<&'a str>,
    /// Each piece's text: given only for the owner at his own terminal.
    pub text: Option<&'a dyn PieceText>,
}

impl TurnRunner {
    /// The sessions of `place`, newest first: a turn's own (no task, not imported) that ran an exchange
    /// the place's window reads (its newest `WINDOW_N`, as a turn's; so history adopted into the ledger
    /// counts with no change here), or whose place the gate reads as `place` today; and none the gate
    /// holds for the owner's look (sealed, seal pending, unreadable), which lifts no seal.
    pub fn place_sessions(&self, place: &str) -> anyhow::Result<Vec<SessionRecord>> {
        use crate::view::fold::Ladder;
        let ran: HashSet<String> =
            (StoreLadder(&self.store).read(place, crate::view::fold::WINDOW_N, None)?)
                .window
                .into_iter()
                .map(|e| e.session_id)
                .collect();
        let mut all: Vec<SessionRecord> = (self.store.list_sessions::<SessionRecord>()?)
            .into_iter()
            .filter(|r| r.imported.is_none() && r.task.is_none())
            .filter(|r| {
                ran.contains(&r.session_id)
                    || place_name(self, &r.session_id).as_deref() == Some(place)
            })
            .filter(|r| seal_of(self, None, &r.session_id) == Seal::Clear)
            .collect();
        all.sort_by_key(|r| std::cmp::Reverse(r.last_active_ms));
        Ok(all)
    }

    /// The view `p.session`'s next turn at `p.place` would get under `spec`, and its tally.
    pub fn view_preview(
        &self,
        p: &Preview<'_>,
        spec: &RequestSpec,
    ) -> anyhow::Result<ViewPreviewResult> {
        let store: &crate::store::Store = &self.store;
        let verified = Verified::of_preview(self, p.session, p.place, p.owner);
        let gate_pov = verified.pov().clone();
        let mut notes = vec![
            "no model was called and no recall was run: recall's pieces are not in it".to_string(),
            "nothing was written: no view record, no ledger row".to_string(),
        ];
        if p.message.is_none() {
            notes.push("assembled for an empty message (`--message` gives one)".into());
        }
        let transcript = store.transcript(p.session)?;
        let fallback = unledgered(store, &transcript, PREVIEW_TURN).then_some(Fallback::Unledgered);
        let built: Built = self.view_built(
            Asked {
                store,
                sid: p.session,
                turn: PREVIEW_TURN,
                person: None,
                verified,
                recalled: Vec::new(),
                message: (None, p.message.unwrap_or_default().to_string()),
                items: Vec::new(),
            },
            spec,
        )?;
        let source = GateSource {
            runner: self,
            pov: gate_pov.clone(),
            // The owner's look lifts no seal (as `view show`); an agent's turn reads its own thread.
            asker: (!p.owner).then_some(p.session),
        };
        let reader = Reader {
            source: &source,
            text: p.text.filter(|_| p.owner),
            restricted: !p.owner || gate_pov.present != Present::OwnerAlone,
        };
        let (view, overflow, levels) = match &built.assembled {
            Ok(a) => {
                let levels: BTreeMap<&str, u8> = (a.ladder.nodes.iter())
                    .map(|n| (n.handle.0.as_str(), n.level))
                    .collect();
                let record =
                    (built.frozen.record.clone()).map(|v| ViewRecord::new(PREVIEW_TURN, v));
                let shown = record.as_ref().map(|r| reader.show(r));
                let folds = shown.as_ref().map(|v| folds_of(v, &levels));
                (shown, None, folds.unwrap_or_default())
            }
            Err(o) => (None, Some(overflow_said(o)), BTreeMap::new()),
        };
        let budget = self.view_budget(spec);
        Ok(ViewPreviewResult {
            place: p.place.into(),
            session: p.session.into(),
            reader: match p.owner {
                true => "owner".into(),
                false => "agent".into(),
            },
            tally: view.as_ref().map(tally_of).unwrap_or_default(),
            window: view.as_ref().map_or(0, window_of),
            view,
            fallback: fallback.map(|f| f.as_str().to_string()),
            overflow,
            budget: ViewBudget {
                provider_window: budget.provider_window,
                reserve: budget.reserve,
                ceiling: budget.ceiling,
                here_allowance: budget.here_allowance,
            },
            folds: levels,
            earlier: self.earlier(p.place, p.session, p.owner)?,
            notes: {
                notes.push(
                    "the rendered request's own check (over_budget) is made when a turn compiles it"
                        .into(),
                );
                notes
            },
        })
    }

    /// The earlier turns of `place`'s sessions this reader may count: a session the gate holds (sealed,
    /// seal pending, or unreadable) adds nothing, not even to the count of sessions. The owner's look
    /// lifts no seal; an agent's reads its own session.
    fn earlier(&self, place: &str, session: &str, owner: bool) -> anyhow::Result<ViewEarlier> {
        let asker = (!owner).then_some(session);
        let mut e = ViewEarlier::default();
        for r in self.place_sessions(place)? {
            let sid = r.session_id.as_str();
            if seal_of(self, asker, sid) != Seal::Clear {
                continue;
            }
            e.sessions += 1;
            let mut seen = HashSet::new();
            for (_, n) in self.store.transcript(sid)?.iter() {
                let Some(turn) = n.turn_id.as_deref() else {
                    continue;
                };
                if n.kind != Kind::UserMessage || !seen.insert(turn.to_string()) {
                    continue;
                }
                e.turns += 1;
                match crate::exchange::exchange(&self.store, turn) {
                    Ok(Some(x)) if x.place == place => {}
                    Ok(Some(_)) => e.other_place += 1,
                    _ => e.no_exchange += 1,
                }
            }
        }
        Ok(e)
    }
}

/// Fold nodes listed, by level.
fn folds_of(v: &ViewShown, levels: &BTreeMap<&str, u8>) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for x in v.slots.iter().flat_map(|s| &s.pieces) {
        if x.kind != "folds" {
            continue;
        }
        if let Some(l) = levels.get(x.handle.as_str()) {
            *out.entry(l.to_string()).or_default() += 1;
        }
    }
    out
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

/// The floor's overflow, by its numbers (no text).
fn overflow_said(o: &crate::view::budget::Overflow) -> String {
    let crate::view::budget::Overflow::Floor {
        handle,
        tokens,
        room,
    } = o;
    format!(
        "the newest exchange ({}) alone is about {tokens} tokens; the window has room for {room}",
        handle.0
    )
}

#[cfg(test)]
mod tests;
