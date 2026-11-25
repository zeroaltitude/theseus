//! Making pieces: every selector builds its pieces here, so a piece of an exchange is the same range, the
//! same sizes and the same marks whichever selector asks for it, and rule 3 can merge them.

use std::collections::BTreeSet;

use super::super::timeline::ExchangeRef;
use super::super::{Author, Form, Piece, PointOfView, Pos, Reason, Sizes, SourceRange, Space};
use super::sources::{ExchangeFacts, Ref, Sources};
use super::Band;

/// The text a line shows where the `did` line was not written; nothing is invented.
pub const UNSUMMARISED: &str = "reply not summarised";

/// Its size by today's estimate.
pub fn unsummarised_tokens() -> u64 {
    (UNSUMMARISED.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN)
}

/// How much of an exchange a band shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The window: whole is with its tool traffic, spoken with stubs, a line his ask and the `did` line.
    Window,
    /// Elsewhere lately: a line is his words and the `did` line; spoken is his words and the reply in full,
    /// or over 1,500 tokens its first 500 and last 1,000 (§5.1, slot F).
    Elsewhere,
}

/// The longest reply slot F shows in full, and what it keeps of a longer one.
const REPLY_WHOLE: u64 = 1_500;

/// The sizes of an exchange as a band shows it.
pub fn sizes(f: &ExchangeFacts, cut: Cut) -> Sizes {
    let did = f.did.unwrap_or_else(unsummarised_tokens);
    match cut {
        Cut::Window => Sizes {
            whole: f.his.saturating_add(f.reply).saturating_add(f.tools),
            spoken: f.his.saturating_add(f.reply).saturating_add(f.stubs),
            line: f.his.saturating_add(did),
        },
        Cut::Elsewhere => Sizes {
            whole: f.his.saturating_add(f.reply.min(REPLY_WHOLE)),
            spoken: f.his.saturating_add(f.reply.min(REPLY_WHOLE)),
            line: f.his.saturating_add(did),
        },
    }
}

/// A piece of one exchange of `place`.
pub fn exchange(
    e: &ExchangeRef,
    f: &ExchangeFacts,
    band: Band,
    reason: Reason,
    form: Form,
    cut: Cut,
) -> Piece {
    let pos = Pos {
        at_ms: e.at_ms,
        id: e.turn_id.clone(),
    };
    retrieved(Piece {
        range: SourceRange {
            space: Space::Exchanges {
                place: f.place.clone(),
            },
            first: pos.clone(),
            last: pos,
        },
        band,
        slot: band.row().slot,
        reasons: BTreeSet::from([reason]),
        form,
        marks: f.marks.clone(),
        sizes: sizes(f, cut),
        handle: f.handle.clone(),
        unit: None,
    })
}

/// A piece of a stored thing that is not an exchange.
pub fn of_ref(r: Ref, band: Band, reason: Reason) -> Piece {
    retrieved(Piece {
        range: r.range,
        band,
        slot: band.row().slot,
        reasons: BTreeSet::from([reason]),
        form: r.form,
        marks: r.marks,
        sizes: r.sizes,
        handle: r.handle,
        unit: r.unit,
    })
}

/// THE one place a retrieved piece loses its authority: whatever made it (an exchange, a fact, a card, a
/// fold node), a word of his read back from the store is never by itself a present instruction. Only the
/// current message, laid by code (band `Now`), may authorise. Every constructor here and
/// [`super::assemble`] (for any selector's piece) pass through it.
pub(in crate::view) fn retrieved(mut piece: Piece) -> Piece {
    if let Author::His { .. } = piece.marks.author {
        piece.marks.author = Author::His {
            may_authorise: false,
        };
    }
    piece
}

/// Whether the gate admits a stored candidate: a finite choice is made over admitted candidates only. A
/// stored reference that claims code's own space is never admitted.
pub fn admits(src: &Sources<'_>, pov: &PointOfView, piece: &Piece) -> bool {
    !matches!(piece.range.space, Space::Code { .. }) && src.gate.admit(pov, piece.clone()).is_some()
}
