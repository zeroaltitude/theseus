//! What the observer noted of the owner's own words today, in the turn's
//! request (theseus-6f8t0, the owner's sitting of 2026-11-07, stop 5).
//!
//! The exchange-end observer (`observe.rs`) keeps a self-fact the owner
//! stated in passing as an unconfirmed `card.obs.<id>` record, and the
//! nightly live pass turns it into a question. Nothing read those records on
//! the turn's path, so asked four minutes later what it had learned about him
//! that day, the model could only list what it had done: the fact had been
//! heard and written, and was in no prompt to name.
//!
//! So the day's open observations ride in the request's tail, beside the
//! commitments' block and the surface's standing records
//! (the harness's block beside the message, `turn::now_step`): after the cached prefix, so they are
//! never a cache write, and built again every turn, so one noted mid-session
//! shows at the next turn.
//!
//! They are still not records, and the rest of their discipline is unchanged:
//!
//! - **Only where the owner is alone** (`capture::owner_alone`, the observer's
//!   own rule): the CLI, the web UI and a DM with an owner. A guild channel
//!   bound private is not one — anyone it lets in writes there — and a task's
//!   session is not one either.
//! - **Only today's**, by the local day the spend counts (`judge::spend`), and
//!   only those still `Open`: one the owner has answered is a record or a
//!   declined question, and the block is not a second copy of either.
//! - **The head says what they are**: notes, not records, not to be restated
//!   to the owner unasked and not to be acted on as settled. A standing record
//!   read as a message just received is its own finding (B4); the head is
//!   written so this block cannot become another.

use theseus_protocol::{CardObservation, ObservationState};

use crate::judge::spend;

/// How the block begins, as the commitments' block does
/// (`commitments::HEAD`): a reader of a request can tell it from the
/// conversation's.
pub const HEAD: &str = "[Noted about the owner today, from the owner's own words";
/// The most lines it shows, and its bound in tokens (estimated as the task
/// view's are, `task_graph::view::BYTES_PER_TOKEN` to a token).
pub const MAX_LINES: usize = 8;
pub const MAX_TOKENS: u64 = 300;

fn tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN)
}

/// The block's text: the open observations of `today`, newest first (as
/// `state::observations` returns them), at most [`MAX_LINES`] and
/// [`MAX_TOKENS`]. `None` when none was noted today, so a day with nothing
/// heard adds nothing at all.
pub fn render(all: &[CardObservation], now_ms: u64) -> Option<String> {
    rendered(all, now_ms).map(|(text, _)| text)
}

/// [`render`], with the ids of the observations it shows: what the turn's block is rendered from, so a
/// removal before a later call of the turn is seen (`turn::now_step`).
pub fn rendered(all: &[CardObservation], now_ms: u64) -> Option<(String, Vec<String>)> {
    let today = spend::local_day(now_ms);
    let mine: Vec<&CardObservation> = all
        .iter()
        .filter(|o| o.state == ObservationState::Open && spend::local_day(o.at_ms) == today)
        .collect();
    if mine.is_empty() {
        return None;
    }
    let head = format!(
        "{HEAD}, {}: notes, not records on the owner's card. Say nothing of them unless the \
         owner asks what you have learned; never act on one as settled, and never thank the \
         owner for one.]",
        mine.len()
    );
    let mut text = head;
    let mut used = tokens(&text);
    let mut ids = Vec::new();
    for o in &mine {
        let line = format!("- {}", one_line(&o.text));
        if ids.len() >= MAX_LINES || used + tokens(&line) + 1 > MAX_TOKENS {
            break;
        }
        used += tokens(&line) + 1;
        ids.push(o.id.clone());
        text.push('\n');
        text.push_str(&line);
    }
    (!ids.is_empty()).then_some((text, ids))
}

/// A text on one line: an observation is a line of the block, so a newline in
/// its text could pass for another.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The day's observations as the turn shows them, with their ids; `None` where
/// the owner is not alone, or when nothing was noted today. The read's error is
/// the caller's to decide on.
pub fn shown(
    store: &crate::store::Store,
    class: theseus_protocol::PlaceClass,
    place: Option<&str>,
    now_ms: u64,
) -> anyhow::Result<Option<(String, Vec<String>)>> {
    if !crate::capture::owner_alone(class, place) {
        return Ok(None);
    }
    let all = crate::card::state::observations(store, Some(ObservationState::Open))?;
    Ok(rendered(&all, now_ms))
}
