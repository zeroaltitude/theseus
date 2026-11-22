//! The view makes its own room (issue 44): a call whose request passes the budget, or that the provider
//! refused for its size, is never handed to another way of building it. In this order, each step tried only
//! when the one before did not make the request fit:
//!
//! 1. The turn's view assembled again, under the gate of now, with its old tool results cleared: the
//!    clearing pass a chain's first call runs, whatever its line, keeping the newest
//!    [`KEEP_TOKENS`](crate::compiler::view::KEEP_TOKENS) of results whole, then only those after the
//!    turn's last answer. A cleared result is its one-line stand-in, as clearing makes it.
//! 2. Slot G's allowance (the window and the ladder) lowered, halving from what it held, down to none: the
//!    floor (the turn's message and the fixed blocks) is never cut.
//! 3. A provider that refused a request of the turn for its size: its stated maximum, read at Theseus's
//!    estimate (scaled by how far its count ran over the estimate), is the window for the rest of the turn,
//!    and 1 and 2 run under it until the request is also smaller than the refused one.
//! 4. Only then, and for a floor that cannot fit at all, the call goes back over its budget, and the compile
//!    step fails the turn before anything is sent with the core overage's words and class
//!    (`turn::overage`), which nothing retries.
//!
//! Every step is a new chain: the bytes under the turn's signed answers are never changed inside one (their
//! thinking goes, as at any new chain, `view_reset`). What one room-making costs: at most six assemblies
//! and compiles in memory, no model call, and the cache written again from the front on the call that goes
//! out. The view's record says what was done (`view::record::Room`): counts of the turn's own results and
//! the numbers, nothing of what a result held.

use super::{recalled, Asked, Built, Turn, TurnRunner};
use crate::compiler::view::{Frozen, KEEP_TOKENS};
use crate::compiler::{CompileInput, Compiled, Overflowed, RequestSpec};
use crate::view::budget::{Budget, Overflow, HERE_ALLOWANCE};
use crate::view::record::Room;
use crate::view::Slot;

#[cfg(test)]
mod tests;

/// How many times slot G's allowance is halved before it is none.
const HALVINGS: u32 = 3;

impl TurnRunner {
    /// One call of a view turn compiled from its frozen view, with room made when it is over the budget or
    /// was refused for its size (the module comment's order). A request no step fits, or a floor that
    /// cannot fit, comes back over its budget, and the compile step fails the turn on it.
    pub(super) fn view_roomed(
        &self,
        t: &mut Turn<'_>,
        input: CompileInput<'_>,
    ) -> anyhow::Result<Compiled> {
        let refused = input.overflowed.and_then(stated_window);
        if let (Some(w), Some(f)) = (refused, t.view.as_mut()) {
            f.room.window = Some(f.room.window.map_or(w, |r| r.min(w)));
        }
        // After a refusal the retry must also be smaller than the refused request, by the estimate: a
        // provider that stated no count, or one under the estimate, says the estimate itself is short.
        let line = input.overflowed.map(|o| o.estimated);
        let fits =
            |c: &Compiled| c.budget.overage.is_none() && line.is_none_or(|l| c.estimate.tokens < l);
        let mut compiled = rendered(t, input)?;
        if floored(t) || fits(&compiled) {
            return Ok(compiled);
        }
        let now = t.view.as_ref().map(|f| f.room).unwrap_or_default();
        for room in steps(now, compiled.budget.limit_tokens) {
            self.view_refrozen(t, input.spec, Some(room))?;
            Self::view_reset(t, input.nodes);
            compiled = rendered(t, input)?;
            if floored(t) || fits(&compiled) {
                break;
            }
        }
        Ok(compiled)
    }

    /// The turn's view assembled (again) under `room`, or the room its frozen view made (`None`): the
    /// answers whose thinking a reset retired, and the recall it laid, are carried.
    pub(super) fn view_refrozen(
        &self,
        t: &mut Turn<'_>,
        spec: &RequestSpec,
        room: Option<Room>,
    ) -> anyhow::Result<()> {
        let (sid, turn, store) = (t.tc.session_id, t.tc.turn_id, t.tc.store);
        let was = t.view.take().filter(|f| f.turn_id == turn);
        let room = room.or(was.as_ref().map(|f| f.room)).unwrap_or_default();
        let (stripped, mut laid) = was.map(|f| (f.stripped, f.recalled)).unwrap_or_default();
        if laid.is_empty() {
            laid = recalled(t);
        }
        // Admitted again now: a source sealed since is not laid (issue 44, `view_step::recall`).
        let (laid, found) = self.view_recall(sid, laid);
        let asked = Asked {
            store,
            sid,
            turn,
            person: t.exchange.person().0,
            verified: None,
            recalled: found,
            message: None,
            items: t.exchange.items().to_vec(),
        };
        #[cfg(test)]
        super::failed::injected(sid)?;
        let built = self.view_built(asked, spec, &self.room_budget(spec, &room))?;
        let mut f = frozen_of(built, room, laid);
        f.stripped.extend(stripped);
        // A turn no message opened: its empty "now", and beside it one line saying what woke it (issue 44).
        if let Some(o) = t.exchange.occasion() {
            f.beside = super::occasion::beside_with(f.beside, o, (sid, turn));
        }
        t.view = Some(f);
        Ok(())
    }

    /// The view's budget under `room`: slot G's allowance as lowered, the window as the provider stated it.
    /// What slot G gives up goes to the ceiling, so the whole (rule 5) is what it was: only slot G is held
    /// to less, and the floor keeps its room.
    fn room_budget(&self, spec: &RequestSpec, room: &Room) -> Budget {
        let b = self.view_budget(spec);
        let here = room
            .here
            .map_or(b.here_allowance, |h| h.min(b.here_allowance));
        let window = room
            .window
            .map_or(b.provider_window, |w| w.min(b.provider_window));
        Budget {
            ceiling: b.ceiling + (b.here_allowance - here),
            here_allowance: here,
            provider_window: window,
            ..b
        }
    }
}

/// The call compiled from the turn's frozen view, under the window the provider stated, and over by the
/// floor's numbers when the floor did not fit.
fn rendered(t: &Turn<'_>, input: CompileInput<'_>) -> anyhow::Result<Compiled> {
    let Some(f) = t.view.as_ref() else {
        anyhow::bail!("the turn's view was not assembled");
    };
    let mut input = input;
    if let Some(w) = f.room.window {
        let entry = input.catalog.get(&input.spec.model);
        let window = input.window_override.or(entry.map(|e| e.context_window));
        input.window_override = Some(window.map_or(w, |at| at.min(w)));
    }
    let mut compiled = crate::compiler::view::compile(input, f);
    if let Some(o) = &f.floor {
        floor_over(&mut compiled, o);
    }
    Ok(compiled)
}

/// Whether the turn's view lays nothing because its floor did not fit.
fn floored(t: &Turn<'_>) -> bool {
    t.view.as_ref().is_some_and(|f| f.floor.is_some())
}

/// The frozen view of `built` under `room`: what slot G held, or the floor's overflow.
fn frozen_of(built: Built, room: Room, recalled: Vec<String>) -> Frozen {
    let Built { frozen, assembled } = built;
    let (held, floor) = match assembled {
        Ok(a) => {
            let here = (a.view.shown.iter()).filter(|s| s.piece.slot == Slot::Here);
            (here.map(|s| s.tokens).sum(), None)
        }
        Err(o) => (0, Some(o)),
    };
    Frozen {
        room: Room { held, ..room },
        floor,
        recalled,
        ..frozen
    }
}

/// The rooms to try after `now`, under a request's `limit` (0: none known), in order: the clearing with the
/// newest [`KEEP_TOKENS`] of results kept whole (at most a quarter of the limit, so the next room is a while
/// off), then only the newest exchange's; then slot G's allowance halved [`HALVINGS`] times from what it
/// held (or the allowance the turn lowered it to), then none. Every room-making clears afresh: the results
/// that were the newest at the last one are old now.
pub(super) fn steps(now: Room, limit: u64) -> Vec<Room> {
    let made = now.made + 1;
    let keep = match limit {
        0 => KEEP_TOKENS,
        l => KEEP_TOKENS.min(l / 4),
    };
    let mut keeps = vec![keep];
    if keep > 0 {
        keeps.push(0);
    }
    let mut out: Vec<Room> = (keeps.into_iter())
        .map(|k| Room {
            made,
            keep: Some(k),
            ..now
        })
        .collect();
    let mut here = now.here.unwrap_or(now.held.min(HERE_ALLOWANCE));
    for _ in 0..HALVINGS {
        here /= 2;
        if here == 0 {
            break;
        }
        out.push(Room {
            made,
            keep: Some(0),
            here: Some(here),
            ..now
        });
    }
    if now.here.unwrap_or(now.held) > 0 {
        out.push(Room {
            made,
            keep: Some(0),
            here: Some(0),
            ..now
        });
    }
    out
}

/// The window a provider's refusal states, read at Theseus's estimate: its maximum, scaled down by how far
/// its count of the refused request ran over the estimate. None when it stated no maximum.
pub(super) fn stated_window(o: &Overflowed) -> Option<u64> {
    let max = o.maximum?;
    Some(match o.counted {
        Some(n) if o.estimated > 0 && n > o.estimated => {
            (u128::from(max) * u128::from(o.estimated) / u128::from(n)) as u64
        }
        _ => max,
    })
}

/// A call whose view's floor did not fit (rule 9): over its budget by the floor's numbers, so the compile
/// step fails it with the core overage's words before anything is sent.
fn floor_over(c: &mut Compiled, o: &Overflow) {
    let Overflow::Floor { tokens, room, .. } = o;
    let over = tokens.saturating_sub(*room).max(1);
    let at = c
        .estimate
        .tokens
        .max(c.budget.limit_tokens.saturating_add(over));
    (c.estimate.tokens, c.est_tokens, c.budget.used_tokens) = (at, at, at);
    c.estimate.upper = c.estimate.upper.max(at);
    c.budget.overage = Some(theseus_protocol::memory::BudgetOverage {
        tokens: over,
        why: "the turn's message and the fixed blocks do not fit the window".into(),
    });
}
