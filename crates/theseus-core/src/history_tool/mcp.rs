//! `history.read` (issue 46): the history tool, `context.history`, read by an MCP client with no model call.
//! It runs the tool's one read (`super::read`) with no turn (`Allowance::NoTurn`): each call is one page or one
//! open, and nothing is counted across calls.
//!
//! - **Its anchor**: `place` (a place's key, as a line names it), else where `session_id`, the client's own
//!   conversation, speaks (`super::anchor`). An `open` of a handle and a `thing`'s timeline need none; a page
//!   or a words search with neither is refused in words ([`NO_ANCHOR`]). Nothing is in view with no turn, so a
//!   bare page starts at the newest exchange.
//! - **Typed lines**, not the model's page: the page's text speaks to a model in a turn (its looks, the person
//!   it talks to), which an outside agent is not. A line holds what `HistoryLine` holds, cut by the same
//!   margins, and `who` by the page's own rule; an open is the tool's spoken form, one string.

use std::sync::Arc;

use theseus_protocol::memory::{
    HistoryCursor, HistoryOpened, HistoryReadLine, HistoryReadParams, HistoryReadResult,
};

use super::page::{self, Lens};
use super::{read, Allowance, Read};
use crate::session::SessionRecord;
use crate::view::history::{HistoryArgs, HistoryLine, PageEnd};
use crate::view::timeline::Cursor;
use crate::view::Handle;
use crate::Core;

/// What a page or a words search with no anchor is told.
pub const NO_ANCHOR: &str =
    "context_history reads a place's history: pass place (a place's key, as a \
     page's lines name it) or session_id (your conversation, from conversation_open)";

fn some(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Where the call is anchored: `place`, else where `session_id` speaks; none when neither is given.
fn anchor(core: &Core, p: &HistoryReadParams) -> Result<Option<String>, String> {
    if let Some(place) = some(&p.place) {
        return Ok(Some(place.to_string()));
    }
    let Some(sid) = some(&p.session_id) else {
        return Ok(None);
    };
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .map_err(|e| format!("the session could not be read: {e:#}"))?
        .ok_or_else(|| {
            format!(
                "no session {}: pass your conversation's session_id from conversation_open, or a place",
                page::flat(sid)
            )
        })?;
    Ok(super::anchor(core, sid, None, rec.dir.as_deref()))
}

fn line(l: HistoryLine) -> HistoryReadLine {
    let HistoryLine {
        handle,
        place,
        at_ms,
        his_words,
        did,
        person,
        reply_begins,
    } = l;
    HistoryReadLine {
        handle: handle.0,
        place,
        at_ms,
        when: page::when(at_ms),
        who: page::who(person.as_deref()),
        words: his_words,
        did,
        reply_begins,
    }
}

fn cursor(c: Cursor) -> HistoryCursor {
    HistoryCursor {
        at_ms: c.at_ms,
        turn_id: c.turn_id,
    }
}

/// `history.read`: a page of `p`'s timeline, or one opened exchange.
pub(crate) async fn history(
    core: &Arc<Core>,
    p: &HistoryReadParams,
) -> Result<HistoryReadResult, String> {
    let a = HistoryArgs {
        place: p.place.clone(),
        thing: p.thing.clone(),
        words: p.words.clone(),
        before: p.before.as_ref().map(|c| Cursor {
            at_ms: c.at_ms,
            turn_id: c.turn_id.clone(),
        }),
        open: p.open.clone().map(Handle),
    };
    let needs_none = some(&p.open).is_some() || some(&p.thing).is_some();
    let here = match anchor(core, p)? {
        Some(h) => h,
        None if needs_none => String::new(),
        None => return Err(NO_ANCHOR.into()),
    };
    let place = match (some(&p.thing), some(&p.open)) {
        (None, None) => here.clone(),
        _ => String::new(),
    };
    let lens = Lens::bare(&core.store, here);
    Ok(match read(core, lens, Allowance::NoTurn, &a).await? {
        Read::Opened { handle, text, .. } => HistoryReadResult {
            open: Some(HistoryOpened { handle, text }),
            ..HistoryReadResult::default()
        },
        // A call with no turn counts no opens.
        Read::OpensUsed { .. } => return Err("no exchange can be opened now".into()),
        Read::Page { walked, resume, .. } => {
            let pg = walked.page;
            let (end, next) = match pg.end {
                PageEnd::More | PageEnd::AllowanceUsed => ("more", pg.next),
                PageEnd::ScanLimit => ("scan_limit", resume),
                PageEnd::HistoryExhausted => ("history_exhausted", None),
            };
            HistoryReadResult {
                place,
                lines: pg.lines.into_iter().map(line).collect(),
                end: end.into(),
                next: next.map(cursor),
                open: None,
            }
        }
    })
}
