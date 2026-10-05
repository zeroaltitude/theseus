//! `theseus history`'s own lines (theseus-xo0m, theseus-kym3): where the
//! page it printed ends, and the command for the next one either way.

use theseus_protocol::SessionHistoryResult;

use super::{push, Line, Tag};

/// The page's cursors as the commands that read past them: older nodes
/// before this page, and more after it.
pub fn page_lines(h: &SessionHistoryResult) -> Vec<Line> {
    let mut out = Vec::new();
    let s = &h.session.session_id;
    if let Some(p) = h.older {
        push(
            &mut out,
            Tag::Dim,
            &format!("── older nodes before position {p}: theseus history {s} --before {p}"),
        );
    }
    if let Some(p) = h.next {
        push(
            &mut out,
            Tag::Dim,
            &format!("── more nodes after position {p}: theseus history {s} --after {p}"),
        );
    }
    out
}
