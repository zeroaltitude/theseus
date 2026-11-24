//! What else waits on the owner, beside the questions on calls (`look.work`'s `needs_you`): the card's
//! proposals (`card.list`'s open ones), and, as a count and not a wait, the standing records proposed for
//! review.
//!
//! **What is not a wait.** A proposed standing record (`standing.proposals`) is asked of no one: nothing
//! posts it, no turn waits on it and it never expires, so it is a list kept for review and only counted
//! (`theseus standing proposals`). A card proposal is asked (the nightly pass posts it to the owner's DM
//! and an unanswered one becomes "Not this" at its expiry): it waits. It is read without the sweep
//! `card.list` makes (`card_view` expires what is due: an open proposal past its end is simply not listed
//! here).

use theseus_protocol::card::ProposalState;

use super::reader::Reader;
use super::{clip, CARDS_MAX};
use crate::scrub::Cuts;

/// What waits, as lines, and how many of each were left out.
#[derive(Default)]
pub(super) struct Owed {
    pub cards: Vec<String>,
    pub cards_more: usize,
    pub standing: usize,
}

impl Owed {
    /// How many things wait (a count of standing proposals is not one).
    pub(super) fn waiting(&self) -> usize {
        self.cards.len() + self.cards_more
    }
}

fn card_line(p: &theseus_protocol::card::CardProposal, cuts: &Cuts) -> String {
    let at = match p.posted_ms {
        Some(_) => "the owner's DM",
        None => "the card, not posted yet",
    };
    format!(
        "- {} · card proposal · waits in {at} · {}\n",
        crate::task::short(&p.id),
        clip(&p.question, 200, cuts)
    )
}

/// What waits besides the questions on calls. `here` is this conversation's alone: the card's proposals
/// and the standing count are tied to no conversation and are left out.
pub(super) fn owed(r: &Reader<'_>, here: bool) -> Result<Owed, ()> {
    let now = theseus_protocol::now_unix_ms();
    let mut out = Owed::default();
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
        .map(|p| card_line(p, r.cuts))
        .collect();
    out.standing = crate::standing_proposals::all(&r.core.store)
        .map_err(|_| ())?
        .iter()
        .filter(|x| x.state == "open")
        .count();
    Ok(out)
}

/// The words for the standing count, or none.
pub(super) fn standing_words(n: usize, cuts: &Cuts) -> String {
    match n {
        0 => String::new(),
        n => clip(
            &format!(
                "Not asked of the owner, open for review: {n} proposed standing record{} (`theseus standing proposals`)",
                if n == 1 { "" } else { "s" }
            ),
            200,
            cuts,
        ) + "\n",
    }
}
