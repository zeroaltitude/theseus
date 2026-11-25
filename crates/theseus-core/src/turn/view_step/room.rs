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
//! 4. The floor alone: every optional piece gone, rendered and measured as it will be sent. After a provider
//!    refused a request of the turn for its size, and the floor is not smaller than the refused request (or
//!    does not fit the window the refusal stated), the bare floor: the turn's message and the fixed blocks,
//!    the window's exchanges gone too, measured against the catalog's window and the refused request alone.
//!    A smaller request exists, so it is sent; the provider says whether it fits (a second refusal fails the
//!    turn, `turn::model_call::overflowed`).
//! 5. Then the newest exchange's own results, largest first, laid as their head with one line saying so,
//!    until the request fits: the last step, never the first (clearing keeps the newest results whole).
//! 6. Only then the call goes back over its budget, and the compile step fails the turn before anything is
//!    sent with the core overage's words and class (`turn::overage`), which nothing retries.
//!
//! Every step is a new chain: the bytes under the turn's signed answers are never changed inside one (their
//! thinking goes, as at any new chain, `view_reset`). What one room-making costs: at most nine assemblies
//! (two clearings, three halvings, none, the squeeze, the floor) and [`HEADS`] head cuts, each compiled in
//! memory, no model call, and the cache written again from the front on the call that goes out. A finished
//! request over its size comes back at most [`SIZINGS`] times (`view_size`). The view's record says what was done (`view::record::Room`): counts of the turn's own results and
//! the numbers, nothing of what a result held.

use super::{recalled, Asked, Built, Turn, TurnRunner};
use crate::compiler::view::{Fit, Frozen, KEEP_TOKENS};
use crate::compiler::{CompileInput, Compiled, Overflowed, RequestSpec};
use crate::view::budget::{Budget, Overflow, HERE_ALLOWANCE};
use crate::view::record::Room;
use crate::view::Slot;
use crate::view_gate::Verified;

#[cfg(test)]
mod tests;

/// How many times slot G's allowance is halved before it is none.
const HALVINGS: u32 = 3;
/// How many times a finished request over its size sends the view back to make room before the turn fails.
pub(super) const SIZINGS: u32 = 2;
/// The most head cuts one room pass makes (results of the newest exchange, largest first).
const HEADS: usize = 6;
/// The fewest characters a result cut to its head keeps.
const HEAD_MIN_CHARS: usize = 1_000;
/// Tokens added to a measured shortfall when the last step reserves it (the view's count and the request's
/// estimate are not the same measure).
const SQUEEZE_SLACK: u64 = 256;

impl TurnRunner {
    /// One call of a view turn compiled from its frozen view, with room made when it is over the budget or
    /// was refused for its size (the module comment's order). The request is measured as it will be sent:
    /// what finishing it adds to its chain (`Fit::tail_on`; a new chain's is measured when it is finished,
    /// `view_size`) counts, and after a
    /// refusal it must be strictly smaller than the refused one, estimate against estimate. A request no
    /// step fits comes back over its budget, and the compile step fails the turn on it: never sent.
    pub(super) fn view_roomed(
        &self,
        t: &mut Turn<'_>,
        input: CompileInput<'_>,
    ) -> anyhow::Result<Compiled> {
        if let Some(f) = t.view.as_mut() {
            if let Some(w) = input.overflowed.and_then(stated_window) {
                f.room.window = Some(f.room.window.map_or(w, |r| r.min(w)));
            }
            if let Some(o) = input.overflowed {
                f.fit.under = Some(f.fit.under.map_or(o.estimated, |u| u.min(o.estimated)));
            }
            f.fit.squeeze = 0;
        }
        let mut compiled = rendered(t, input)?;
        if fits(t, &compiled) {
            return Ok(compiled);
        }
        let now = t.view.as_ref().map(|f| f.room).unwrap_or_default();
        for room in steps(now, compiled.budget.limit_tokens) {
            compiled = self.view_room(t, input, room)?;
            if fits(t, &compiled) {
                return Ok(compiled);
            }
        }
        let last = t.view.as_ref().map(|f| f.room).unwrap_or_default();
        // What this pass still measures short is reserved once more, so the cut order takes what it must.
        if !floored(t) {
            let short = short_of(t, &compiled);
            if let Some(f) = t.view.as_mut() {
                f.fit.squeeze = short + short / 4 + SQUEEZE_SLACK;
            }
            let squeezed = Room {
                here: Some(0),
                keep: Some(0),
                reserve: true,
                ..last
            };
            compiled = self.view_room(t, input, squeezed)?;
            if fits(t, &compiled) {
                return Ok(compiled);
            }
        }
        // The floor alone, rendered and measured as sent: only if it does not fit is the turn short of room.
        let floor = Room {
            here: Some(0),
            keep: Some(0),
            reserve: false,
            floor_only: true,
            ..last
        };
        compiled = self.view_room(t, input, floor)?;
        for _ in 0..HEADS {
            if floored(t) || fits(t, &compiled) || !cut_newest(t, input.nodes, &compiled) {
                break;
            }
            Self::view_reset(t, input.nodes);
            compiled = rendered(t, input)?;
        }
        // After a refusal, a smaller request than the refused one is sent whenever one exists. The refusal is the
        // turn's own state (`Fit::under`, set above from the call's input), so a size repair of the retry, which
        // recompiles without that input, may still take the bare floor.
        let refused =
            input.overflowed.is_some() || t.view.as_ref().is_some_and(|f| f.fit.under.is_some());
        if refused && (floored(t) || !fits(t, &compiled)) {
            let bare = Room {
                here: Some(0),
                keep: Some(0),
                reserve: false,
                floor_only: true,
                bare: true,
                window: None,
                ..last
            };
            compiled = self.view_room(t, input, bare)?;
            if fits(t, &compiled) {
                return Ok(compiled);
            }
        }
        if !floored(t) {
            let fit = t.view.as_ref().map(|f| f.fit).unwrap_or_default();
            over_by(&mut compiled, fit);
        }
        Ok(compiled)
    }

    /// The view assembled again under `room`, on a new chain, and the call compiled from it.
    fn view_room(
        &self,
        t: &mut Turn<'_>,
        input: CompileInput<'_>,
        room: Room,
    ) -> anyhow::Result<Compiled> {
        self.view_refrozen(t, input.spec, Some(room))?;
        Self::view_reset(t, input.nodes);
        rendered(t, input)
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
        let fit = was.as_ref().map(|f| f.fit).unwrap_or_default();
        let cut = was.as_ref().map(|f| f.cut.clone()).unwrap_or_default();
        // The turn's renderings hold for the turn, but one that read a node erased since (issue 49).
        let quoted = (was.as_ref())
            .map(|f| crate::quoted::carried(store, f.quoted.clone()))
            .unwrap_or_default();
        let (stripped, mut laid) = was.map(|f| (f.stripped, f.recalled)).unwrap_or_default();
        if laid.is_empty() {
            laid = recalled(t);
        }
        // Read again now: a source that no longer reads is not laid (issue 44, `view_step::recall`).
        let (laid, found) = self.view_recall(sid, laid)?;
        // The harness's part of the block beside the message, composed at each freeze from what the turn
        // rendered once (`now_step`), with room kept for what the route still has to decide.
        let composed = (t.now.as_ref()).and_then(|r| {
            crate::turn::now_step::compose(&r.parts, crate::turn::now_step::budget())
        });
        let reserve = match t.route.defer_persist {
            true => crate::turn::now_step::routed_reserve(),
            false => 0,
        };
        if let Some(c) = &composed {
            crate::turn::now_step::traced(t, c);
        }
        // What this freeze lays of the parts: the compile's account and admission read it.
        if let Some(r) = t.now.as_mut() {
            r.laid = (composed.iter())
                .flat_map(|c| c.shown.iter().map(|(n, _)| *n))
                .collect();
        }
        let asked = Asked {
            store,
            sid,
            turn,
            person: t.exchange.person().0,
            verified: Some(Verified::of_turn(self, sid, t.exchange.read_place())),
            recalled: found,
            message: None,
            items: t.exchange.items().to_vec(),
            harness: composed.as_ref().map_or(0, |c| c.tokens) + reserve,
        };
        #[cfg(test)]
        super::failed::injected(sid)?;
        let built = self.view_built(asked, spec, &self.room_budget(spec, &room, &fit))?;
        let mut f = frozen_of(built, (room, fit), laid);
        f.stripped.extend(stripped);
        f.cut = cut;
        f.quoted = quoted;
        // A turn no message opened: its empty "now", and beside it one line saying what woke it (issue 44).
        if let Some(o) = t.exchange.occasion() {
            f.beside = super::occasion::beside_with(f.beside, o, (sid, turn));
        }
        // The harness's part first in the one block beside the message (issue 46).
        if let Some(c) = &composed {
            f.beside = crate::turn::now_step::beside_with(f.beside, &c.text, (sid, turn));
        }
        t.view = Some(f);
        Ok(())
    }

    /// The view's budget under `room`: slot G's allowance as lowered, the window as the provider stated it.
    /// What slot G gives up goes to the ceiling, so the whole (rule 5) is what it was: only slot G is held
    /// to less, and the floor keeps its room. A step that lowers slot G (the clearing steps came first, so
    /// what is measured is already cleared) also reserves what the requests carry beyond the view
    /// (`Fit::own`: the turn's own nodes, the tools, the tails) and this pass's squeeze, so the cut order
    /// takes the optional pieces (recall, reached exchanges, cards) before the floor. The floor-only room
    /// cuts every optional piece and reserves nothing more: it is measured as sent instead.
    fn room_budget(&self, spec: &RequestSpec, room: &Room, fit: &Fit) -> Budget {
        let mut b = self.view_budget(spec);
        if room.reserve {
            b.reserve += fit.own + fit.squeeze;
        }
        if room.floor_only {
            let window = room
                .window
                .map_or(b.provider_window, |w| w.min(b.provider_window));
            return Budget {
                ceiling: 0,
                here_allowance: 0,
                provider_window: window,
                floor_only: true,
                bare: room.bare,
                ..b
            };
        }
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
/// floor's numbers when the floor did not fit. Its estimate before finishing is kept (`Fit::base`), and what
/// the request carries beyond the view (`Fit::own`).
fn rendered(t: &mut Turn<'_>, input: CompileInput<'_>) -> anyhow::Result<Compiled> {
    let compiled = rendered_now(t, input)?;
    if let Some(f) = t.view.as_mut() {
        let tokens = compiled.estimate.tokens;
        f.fit.base = (compiled.estimate.upper, tokens);
        let tail = tail_of(&compiled, &f.fit);
        f.fit.own = (tokens + tail.1).saturating_sub(f.fit.charged);
    }
    Ok(compiled)
}

/// What finishing adds to `c`: its chain's last measure. A new chain is charged no earlier request's tail:
/// only what this same call's finish measured when the size check sent it back (`Fit::finish`).
fn tail_of(c: &Compiled, fit: &Fit) -> (u64, u64) {
    match c.new_compilation {
        true => fit.finish,
        false => fit.tail_on,
    }
}

/// Whether `c` fits as it will be sent, and the view laid something (a floor that did not fit lays
/// nothing).
fn fits(t: &Turn<'_>, c: &Compiled) -> bool {
    !floored(t) && short_of(t, c) == 0
}

/// The largest result of the turn's newest exchange (after its last answer) not yet cut, laid as its head
/// short enough for `c` to fit: marked on the turn's view, rendered on a new chain. False when none is left.
fn cut_newest(t: &mut Turn<'_>, nodes: &[(u64, crate::stub::Stub)], c: &Compiled) -> bool {
    let short = short_of(t, c);
    let Some(f) = t.view.as_mut() else {
        return false;
    };
    let own: Vec<&crate::node::Node> = (nodes.iter())
        .filter(|(_, n)| n.turn_id.as_deref() == Some(f.turn_id.as_str()))
        .map(|(_, n)| &**n)
        .collect();
    let after = (own.iter())
        .rposition(|n| matches!(n.body, crate::node::Body::AssistantMessage { .. }))
        .map_or(0, |i| i + 1);
    let largest = (own[after..].iter())
        .filter_map(|n| match &n.body {
            crate::node::Body::ToolResult { content, .. } => {
                let shown = f.cut.get(&n.id).copied().unwrap_or(usize::MAX);
                // A result kept by reference is as long as the turn's rendering of it (issue 49).
                let text = (f.quoted.get(&n.id)).map_or(content.as_str(), |r| r.text.as_str());
                let len = text.chars().count().min(shown);
                (len > HEAD_MIN_CHARS).then(|| (len, n.id.clone()))
            }
            _ => None,
        })
        .max();
    let Some((len, id)) = largest else {
        return false;
    };
    let per_token = c.request.json_bytes() as f64 / c.estimate.tokens.max(1) as f64;
    let less = (short as f64 * per_token * 1.25) as usize + 512;
    f.cut
        .insert(id, len.saturating_sub(less).max(HEAD_MIN_CHARS));
    true
}

/// The size check every view call passes before it is sent, on its request as finished (tails and blocks
/// attached): what finishing added is recorded (`Fit::tail_on`), and
/// the request must be within its limit and strictly smaller than one the provider refused. `last`: no
/// more room may be made, and a request that does not fit is marked over. The budget is read again from
/// the finished request; the digest stays the compile's (a replay rebuilds the request before its tails,
/// `own_bench::rebuild`).
pub(super) fn view_size(t: &mut Turn<'_>, compiled: &mut Compiled, last: bool) -> Size {
    #[cfg(test)]
    if !last && resize_forced(t.tc.session_id) {
        return Size::Again;
    }
    let turn = t.tc.turn_id;
    let Some(f) = t.view.as_mut().filter(|f| f.turn_id == turn) else {
        return Size::Fits;
    };
    let tail = (
        compiled.estimate.upper.saturating_sub(f.fit.base.0),
        compiled.estimate.tokens.saturating_sub(f.fit.base.1),
    );
    f.fit.tail_on = tail;
    let finished = Fit {
        base: (compiled.estimate.upper, compiled.estimate.tokens),
        tail_on: (0, 0),
        finish: (0, 0),
        ..f.fit
    };
    compiled.budget.used_tokens = compiled.estimate.tokens;
    if shortfall(compiled, &finished) == 0 {
        f.fit.finish = (0, 0);
        return Size::Fits;
    }
    if last || f.floor.is_some() {
        f.fit.finish = (0, 0);
        over_by(compiled, finished);
        return Size::Over;
    }
    f.fit.finish = tail;
    Size::Again
}

/// The size check's word (`view_size`).
pub(super) enum Size {
    /// Within its limit and under a refused request: it may be sent.
    Fits,
    /// Not yet: make room again and check once more.
    Again,
    /// No more room may be made: marked over, the turn fails on its overage.
    Over,
}

/// How far the request, as it will be sent (`Fit::tail` added), is from fitting: over its limit (upper
/// bound) or not strictly under a refused request (tokens); 0 when it fits.
fn short_of(t: &Turn<'_>, c: &Compiled) -> u64 {
    t.view.as_ref().map_or(0, |f| shortfall(c, &f.fit))
}

fn shortfall(c: &Compiled, fit: &Fit) -> u64 {
    let tail = tail_of(c, fit);
    let limit = c.budget.limit_tokens;
    let over = match limit {
        0 => 0,
        l => (c.estimate.upper + tail.0).saturating_sub(l),
    };
    let refused = fit
        .under
        .map_or(0, |u| (c.estimate.tokens + tail.1 + 1).saturating_sub(u));
    over.max(refused)
}

/// `c` marked over its budget by `fit`'s shortfall, when it has one: the compile step fails the turn on it
/// before anything is sent (a request that did not get smaller than a refused one included).
fn over_by(c: &mut Compiled, fit: Fit) {
    let short = shortfall(c, &fit);
    if short == 0 || c.budget.overage.is_some() {
        return;
    }
    let refused = fit
        .under
        .is_some_and(|u| c.estimate.tokens + tail_of(c, &fit).1 >= u);
    let why = match refused {
        true => {
            "the request could not be made smaller than the one the provider refused for its size"
        }
        false => {
            "the request, as it is sent, passes the window with all the room the view could make"
        }
    };
    c.budget.overage = Some(theseus_protocol::memory::BudgetOverage {
        tokens: short,
        why: why.into(),
    });
}

fn rendered_now(t: &Turn<'_>, input: CompileInput<'_>) -> anyhow::Result<Compiled> {
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
fn frozen_of(
    built: Built,
    (room, fit): (Room, Fit),
    recalled: Vec<crate::node::RecalledRef>,
) -> Frozen {
    let Built { frozen, assembled } = built;
    let (held, charged, floor) = match assembled {
        Ok(a) => {
            let here = (a.view.shown.iter()).filter(|s| s.piece.slot == Slot::Here);
            (here.map(|s| s.tokens).sum(), a.view.tokens, None)
        }
        Err(o) => (0, fit.charged, Some(o)),
    };
    Frozen {
        room: Room { held, ..room },
        fit: Fit { charged, ..fit },
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
    // The clearing steps reserve nothing: what they would reserve is measured before they clear it.
    let mut out: Vec<Room> = (keeps.into_iter())
        .map(|k| Room {
            made,
            keep: Some(k),
            reserve: false,
            floor_only: false,
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
            reserve: true,
            floor_only: false,
            ..now
        });
    }
    if now.here.unwrap_or(now.held) > 0 {
        out.push(Room {
            made,
            keep: Some(0),
            here: Some(0),
            reserve: true,
            floor_only: false,
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

/// A test's seam: the next size check of `session`'s call says "again" once, so a size repair runs.
#[cfg(test)]
static RESIZE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Make the next size check of `session` repair once.
#[cfg(test)]
pub(crate) fn force_resize(session: &str) {
    RESIZE.lock().unwrap().push(session.to_string());
}

#[cfg(test)]
fn resize_forced(session: &str) -> bool {
    let mut forced = RESIZE.lock().unwrap();
    match forced.iter().position(|s| s == session) {
        Some(i) => {
            forced.remove(i);
            true
        }
        None => false,
    }
}
