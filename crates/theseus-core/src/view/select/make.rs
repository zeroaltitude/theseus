//! Making pieces: every selector builds its pieces here, so a piece of an exchange is the same range, the
//! same sizes and the same marks whichever selector asks for it, and rule 3 can merge them.

use std::collections::BTreeSet;

use super::super::timeline::ExchangeRef;
use super::super::{Author, Form, Piece, Pos, Reason, Sizes, SourceRange, Space};
use super::sources::{ExchangeFacts, Ref};
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
    /// His past: his words alone, with the `did` line where the band shows Theseus's side or where his line
    /// is never bare.
    Past { did: bool },
}

/// The longest reply slot F shows in full, and what it keeps of a longer one.
const REPLY_WHOLE: u64 = 1_500;

/// The sizes of an exchange as a band shows it.
pub fn sizes(f: &ExchangeFacts, cut: Cut) -> Sizes {
    let did = f.did.unwrap_or_else(unsummarised_tokens);
    match cut {
        Cut::Window => Sizes {
            whole: f.his + f.reply + f.tools,
            spoken: f.his + f.reply + f.stubs,
            line: f.his + did,
        },
        Cut::Elsewhere => Sizes {
            whole: f.his + f.reply.min(REPLY_WHOLE),
            spoken: f.his + f.reply.min(REPLY_WHOLE),
            line: f.his + did,
        },
        Cut::Past { did: with } => {
            let line = f.his + if with || f.kind.never_bare() { did } else { 0 };
            Sizes {
                whole: line,
                spoken: line,
                line,
            }
        }
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
    let mut marks = f.marks.clone();
    // A retrieved word of his is never by itself a present instruction: only the current message may
    // authorise, and it is laid by code (band `Now`).
    if let Author::His { .. } = marks.author {
        marks.author = Author::His {
            may_authorise: false,
        };
    }
    Piece {
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
        marks,
        sizes: sizes(f, cut),
        handle: f.handle.clone(),
        unit: None,
    }
}

/// A piece of a stored thing that is not an exchange.
pub fn of_ref(r: Ref, band: Band, reason: Reason) -> Piece {
    Piece {
        range: r.range,
        band,
        slot: band.row().slot,
        reasons: BTreeSet::from([reason]),
        form: r.form,
        marks: r.marks,
        sizes: r.sizes,
        handle: r.handle,
        unit: r.unit,
    }
}
