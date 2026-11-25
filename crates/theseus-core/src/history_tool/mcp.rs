//! `history.read` (issue 46): the history tool, `context.history`, read by an MCP client with no model call.
//! It runs the tool's one read (`super::read`) with no turn (`Allowance::NoTurn`): each call is one page or one
//! open, and nothing is counted across calls. Every read of the tool is the model's own: a span, an exchange or
//! a session opened by id (an imported one included, each node by its newest record), a words search, a label.
//!
//! - **Its anchor**: `place` (a place's key, as a line names it), else where `session_id`, the client's own
//!   conversation, speaks (`super::anchor`). An `open`, a `label`'s timeline, and a page that narrows (a span, or
//!   words) need none: with no place named, a page that narrows reads every place; a bare page with neither is
//!   refused in words ([`NO_ANCHOR`]). Nothing is in view with no turn, so a bare page
//!   starts at the newest exchange.
//! - **A fork's cut**: a `session_id` that is a fork (an MCP client may fork its own conversation) carries
//!   that fork's cut, as the model's own call in it does: its pages, its search and its opens hold nothing an
//!   ancestor said at or after the fork's point (`super::lens_for`). With no session the cut is empty.
//! - **No self-loop scope**: an MCP client is not the self loop, so its read takes none; but a `session_id`
//!   whose scope cannot be read fails the call, as it fails the model's (`super::self_scope`).
//! - **Outside words, told**: a line or an open that holds another person's words (or what was done or
//!   replied derived from them) says `outside`, as the model's result is marked External.
//! - **Typed lines**, not the model's page: the page's text speaks to a model in a turn (its looks, the person
//!   it talks to), which an outside agent is not. A line holds what `HistoryLine` holds, cut by the same
//!   margins, and `who` by the page's own rule; an open is the tool's spoken form, one string.
//! - **The vault's values are substituted in every string** of a reply (handles, times, the page's end and
//!   cursor too) and of an error (`Exits::text`), as the MCP memory tools' are: no provider call is on this
//!   path to do it. A handle or cursor the substitution changes no longer navigates: that is the right failure.
//! - **Retraction corrects; it does not hide**: a retracted statement still appears in history as said, and
//!   its correction is the note the retraction gave the sessions that used it. An erased import is gone.

use std::sync::Arc;

use theseus_protocol::memory::{
    HistoryCursor, HistoryOpened, HistoryReadLine, HistoryReadParams, HistoryReadResult,
};

use super::{count, lens_for, page, read, self_scope, Allowance, Read};
use crate::exchange::OWNER;
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

/// The anchoring session's place, when `session_id` names one: where it speaks, or none when that cannot be
/// read. A `session_id` that names no session is refused.
fn place_of_session(core: &Core, sid: &str) -> Result<Option<String>, String> {
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

fn line(l: HistoryLine, cuts: &crate::scrub::Cuts) -> HistoryReadLine {
    let HistoryLine {
        handle,
        place,
        at_ms,
        his_words,
        did,
        person,
        reply_begins,
        outside,
        labels,
        quotes: _,
    } = l;
    let outside = outside || person.as_deref().is_some_and(|p| p != OWNER);
    HistoryReadLine {
        handle: handle.0,
        place,
        at_ms,
        when: page::when(at_ms),
        who: page::who(person.as_deref(), cuts),
        words: his_words,
        did,
        reply_begins,
        outside,
        labels,
    }
}

fn cursor(c: Cursor) -> HistoryCursor {
    HistoryCursor {
        at_ms: c.at_ms,
        turn_id: c.turn_id,
    }
}

/// `history.read`: a page of `p`'s timeline, or one opened exchange or session, with the vault's values
/// substituted in every string of the reply and of an error.
pub(crate) async fn history(
    core: &Arc<Core>,
    p: &HistoryReadParams,
) -> Result<HistoryReadResult, String> {
    read_history(core, p)
        .await
        .map(|r| substituted(core, r))
        .map_err(|e| said(core, &e))
}

/// Words with the vault's values substituted: an error's, which may echo what was asked, and the endpoint's
/// every error (`rpc/memory_read.rs`).
pub(crate) fn said(core: &Core, e: &str) -> String {
    core.exits.text(e).0.into_owned()
}

/// The reply with the vault's values substituted in EVERY string a client reads: handles, places, times, the
/// page's end and its cursor as well as what was said. Every field is named (no `..`), so a field added later
/// does not compile until it is placed here; only numbers pass as they are.
pub(crate) fn substituted(core: &Core, r: HistoryReadResult) -> HistoryReadResult {
    let sub = |s: String| said(core, &s);
    let cur = |c: HistoryCursor| {
        let HistoryCursor { at_ms, turn_id } = c;
        HistoryCursor {
            at_ms,
            turn_id: sub(turn_id),
        }
    };
    let HistoryReadResult {
        place,
        lines,
        end,
        next,
        open,
    } = r;
    let lines = lines
        .into_iter()
        .map(|l| {
            let HistoryReadLine {
                handle,
                place,
                at_ms,
                when,
                who,
                words,
                did,
                reply_begins,
                outside,
                labels,
            } = l;
            HistoryReadLine {
                handle: sub(handle),
                place: sub(place),
                at_ms,
                when: sub(when),
                who: sub(who),
                words: sub(words),
                did: did.map(sub),
                reply_begins: reply_begins.map(sub),
                outside,
                labels: labels.map(sub),
            }
        })
        .collect();
    let open = open.map(|o| {
        let HistoryOpened {
            handle,
            text,
            outside,
            next,
        } = o;
        HistoryOpened {
            handle: sub(handle),
            text: sub(text),
            outside,
            next: next.map(cur),
        }
    });
    HistoryReadResult {
        place: sub(place),
        lines,
        end: sub(end),
        next: next.map(cur),
        open,
    }
}

async fn read_history(
    core: &Arc<Core>,
    p: &HistoryReadParams,
) -> Result<HistoryReadResult, String> {
    let a = HistoryArgs {
        place: p.place.clone(),
        label: p.label.clone(),
        words: p.words.clone(),
        before: p.before.as_ref().map(|c| Cursor {
            at_ms: c.at_ms,
            turn_id: c.turn_id.clone(),
        }),
        open: p.open.clone().map(Handle),
        when: p.when.clone(),
        from: p.from.clone(),
        to: p.to.clone(),
    };
    // The session that anchors the read (its place, and its fork's cut), when one is named.
    let session = some(&p.session_id);
    let session_place = match session {
        Some(sid) => place_of_session(core, sid)?,
        None => None,
    };
    // Only a bare page needs an anchor: one that narrows (a span, words) reads every place.
    let narrows = [&p.words, &p.when, &p.from, &p.to]
        .into_iter()
        .any(|s| some(s).is_some());
    let needs_none = some(&p.open).is_some() || some(&p.label).is_some() || narrows;
    let here = match (some(&p.place), session_place) {
        (Some(place), _) => place.to_string(),
        (None, Some(place)) => place,
        (None, None) if needs_none => String::new(),
        (None, None) => return Err(NO_ANCHOR.into()),
    };
    // The place a page reads: a label's beside a named place keeps that place's exchanges.
    let place = match (some(&p.label), some(&p.open), some(&p.place)) {
        (None, None, _) => here.clone(),
        (Some(_), None, Some(named)) => named.to_string(),
        _ => String::new(),
    };
    // An MCP client is not the self loop: no scope; but an anchoring session whose scope cannot be read fails
    // the call, as it fails the model's.
    if let Some(sid) = session {
        self_scope(&core.runner, sid)?;
    }
    let lens = lens_for(
        core,
        &here,
        None,
        (session, None),
        &a,
        theseus_protocol::now_unix_ms(),
    )?;
    let in_span = lens.span.is_some();
    let used = count(core, &Allowance::NoTurn, &a);
    Ok(
        match read(core, lens, &Allowance::NoTurn, used, &a).await? {
            Read::Opened { handle, opened, .. } => HistoryReadResult {
                open: Some(HistoryOpened {
                    handle,
                    text: opened.text,
                    outside: opened.outside,
                    next: opened.next.map(cursor),
                }),
                ..HistoryReadResult::default()
            },
            // A call with no turn counts no opens.
            Read::OpensUsed { .. } => return Err("no exchange can be opened now".into()),
            Read::Page { walked, resume, .. } => {
                // Every place's page names no one place.
                let place = match walked.every {
                    true => String::new(),
                    false => place,
                };
                let pg = walked.page;
                let (end, next) = match pg.end {
                    PageEnd::More | PageEnd::AllowanceUsed => ("more", pg.next),
                    PageEnd::ScanLimit => ("scan_limit", resume),
                    PageEnd::HistoryExhausted if in_span => ("span_start", None),
                    PageEnd::HistoryExhausted => ("history_exhausted", None),
                    PageEnd::SearchCut | PageEnd::SearchStale => ("search_cut", None),
                };
                let cuts = crate::scrub::Cuts::live();
                HistoryReadResult {
                    place,
                    lines: pg.lines.into_iter().map(|l| line(l, &cuts)).collect(),
                    end: end.into(),
                    next: next.map(cursor),
                    open: None,
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Another person's name over 80 characters, a value the live board knows
    /// across that cut: the line an MCP client reads holds it whole or not at
    /// all.
    #[test]
    fn an_outside_persons_name_stops_before_a_known_value() {
        use crate::tests_cut_sites::{whole_or_absent, V};
        let board = crate::secrets::SecretBoard::empty();
        board.hold("cut_test", crate::secrets::Secret::new(V.into()));
        let l = HistoryLine {
            handle: crate::view::Handle("h1".into()),
            place: "dm".into(),
            at_ms: 1,
            his_words: "hello".into(),
            did: None,
            person: Some(format!("{}{V} after", "p".repeat(70))),
            reply_begins: None,
            outside: false,
            labels: None,
            quotes: Default::default(),
        };
        let read = line(l, &crate::scrub::Cuts::live());
        assert!(read.who.starts_with("outside text, ppp"), "{}", read.who);
        whole_or_absent("the line's who", &read.who, V);
    }
}
