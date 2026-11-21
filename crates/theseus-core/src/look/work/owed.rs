//! What else waits on the owner, beside the questions on calls (`look.work`'s `needs_you`): the card's
//! proposals (`card.list`'s open ones), the router's questions about which thread a message belongs to
//! (`thread.ask_list`), and, as a count and not a wait, the standing records proposed for review.
//!
//! **What is not a wait.** A proposed standing record (`standing.proposals`) is asked of no one: nothing
//! posts it, no turn waits on it and it never expires, so it is a list kept for review and only counted
//! (`theseus standing proposals`). A card proposal is asked (the nightly pass posts it to the owner's DM
//! and an unanswered one becomes "Not this" at its expiry), and a thread question holds a message until it
//! is answered or ends: both wait. Each is read without the sweep `card.list` makes (`card_view` expires
//! what is due: an open proposal past its end is simply not listed here), and the router's read drops only
//! the kept copies of questions that have run out, as `thread.ask_list` does.

use theseus_protocol::card::ProposalState;

use super::reader::Reader;
use super::{clip, CARDS_MAX, THREAD_ASKS_MAX};

/// What waits, as lines, and how many of each were left out.
#[derive(Default)]
pub(super) struct Owed {
    pub cards: Vec<String>,
    pub cards_more: usize,
    pub asks: Vec<String>,
    pub asks_more: usize,
    pub standing: usize,
}

impl Owed {
    /// How many things wait (a count of standing proposals is not one).
    pub(super) fn waiting(&self) -> usize {
        self.cards.len() + self.cards_more + self.asks.len() + self.asks_more
    }
}

fn card_line(p: &theseus_protocol::card::CardProposal) -> String {
    let at = match p.posted_ms {
        Some(_) => "the owner's DM",
        None => "the card, not posted yet",
    };
    format!(
        "- {} · card proposal · waits in {at} · {}\n",
        crate::task::short(&p.id),
        clip(&p.question, 200)
    )
}

fn ask_line(a: &theseus_protocol::thread_route::ThreadAsk, r: &Reader<'_>) -> String {
    format!(
        "- {} · thread question · waits in {} · {}\n",
        crate::task::short(&a.ask_id),
        r.place(&a.session_id),
        r.words(&a.session_id, &a.question, 200)
    )
}

/// What waits besides the questions on calls. `here` is this conversation's alone: its thread questions;
/// the card's proposals and the standing count are tied to no conversation and are left out.
pub(super) fn owed(r: &Reader<'_>, here: bool) -> Result<Owed, ()> {
    let now = theseus_protocol::now_unix_ms();
    let mut out = Owed::default();
    let asks: Vec<_> = r
        .core
        .runner
        .threads
        .route
        .held(now)
        .into_iter()
        .filter(|a| !here || a.session_id == r.here)
        .collect();
    out.asks_more = asks.len().saturating_sub(THREAD_ASKS_MAX);
    out.asks = asks
        .iter()
        .take(THREAD_ASKS_MAX)
        .map(|a| ask_line(a, r))
        .collect();
    if here {
        return Ok(out);
    }
    let all = crate::card::state::proposals(&r.core.store).map_err(|_| ())?;
    let open: Vec<_> = crate::card::state::in_state(&all, ProposalState::Open)
        .filter(|p| p.expires_ms == 0 || p.expires_ms > now)
        .collect();
    out.cards_more = open.len().saturating_sub(CARDS_MAX);
    out.cards = open
        .iter()
        .take(CARDS_MAX)
        .copied()
        .map(card_line)
        .collect();
    out.standing = crate::standing_proposals::all(&r.core.store)
        .map_err(|_| ())?
        .iter()
        .filter(|x| x.state == "open")
        .count();
    Ok(out)
}

/// The words for the standing count, or none.
pub(super) fn standing_words(n: usize) -> String {
    match n {
        0 => String::new(),
        n => clip(
            &format!(
                "Not asked of the owner, open for review: {n} proposed standing record{} (`theseus standing proposals`)",
                if n == 1 { "" } else { "s" }
            ),
            200,
        ) + "\n",
    }
}
