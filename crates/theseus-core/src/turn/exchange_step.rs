//! A turn's exchange (issue 44, step 1; `crate::exchange`): the owner's mark
//! as the input's node is heard, the exchange's opening half once the input
//! is written, what the first compile sent, and the end with its measuring
//! row. Every record rides a frame the turn writes anyway (`Store::defer`),
//! and nothing here changes what any request carries. A task's turns have no
//! place, and write none of it.

use std::collections::{BTreeMap, HashSet};

use theseus_protocol::{ContextCompiled, TurnSubmitResult};
use tokio::sync::oneshot;

use super::{Turn, TurnRunner};
use crate::compiler::{Compiled, RequestSpec};
use crate::exchange::ledger::{self, Given, Seen, View};
use crate::exchange::{
    exchange_record, mark_record, place_of, touch, Exchange, Opener, OwnerMark, Pending, Root,
};
use crate::node::Node;
use crate::places::PlaceClass;
use crate::store::Transcript;

/// The most of a message the index is asked to name things in.
const THINGS_TEXT: usize = 64 * 1024;

/// What a turn holds of its exchange until it ends.
#[derive(Default)]
pub(super) struct Held {
    ex: Option<Exchange>,
    /// The message that opened it, heard before the exchange is opened.
    ask: Option<(String, u64)>,
    mark: Option<OwnerMark>,
    /// The index naming the things in the message.
    things: Option<oneshot::Receiver<Vec<String>>>,
    /// The things of the reply before it, when they were named in time.
    reply_things: Option<BTreeMap<String, u64>>,
    seen: Option<Seen>,
}

impl Held {
    /// The exchange's place, once it is open (`turn::view_step`).
    pub(super) fn place(&self) -> Option<&str> {
        self.ex.as_ref().map(|e| e.place.as_str())
    }

    /// The person of the message that opened it, and whether it carried a mark.
    pub(super) fn person(&self) -> (Option<String>, bool) {
        (
            self.mark.as_ref().and_then(|m| m.person()),
            self.mark.is_some(),
        )
    }
}

fn defer(t: &Turn<'_>, rec: Option<theseus_store::NewRecord>) {
    if let Some(r) = rec {
        if let Err(e) = t.tc.store.defer(r) {
            tracing::warn!(error = %format!("{e:#}"), "an exchange record was not written");
        }
    }
}

/// A turn that failed after it began: its exchange ends with the failure's
/// class, and writes no row.
pub(super) fn failed(t: &mut Turn<'_>, class: &str) {
    if let Some(mut ex) = t.exchange.ex.take() {
        ex.end_ms = Some(theseus_protocol::now_unix_ms());
        ex.stop = Some(format!("failed: {class}"));
        defer(t, exchange_record(&ex).ok());
    }
}

impl TurnRunner {
    fn ledger_on(&self, t: &Turn<'_>) -> bool {
        self.cfg.context.ledger_on() && t.tc.task.is_none()
    }

    /// The index's things in `text`, named off the turn's path: none with no
    /// index, or outside a runtime.
    fn things_of(&self, text: &str) -> Option<oneshot::Receiver<Vec<String>>> {
        let index = self.pass.index()?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let mut cut = text.len().min(THINGS_TEXT);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let text = text[..cut].to_string();
        let (tx, rx) = oneshot::channel();
        rt.spawn(async move {
            if let Ok(mut e) = index.entities(vec![text]).await {
                let _ = tx.send(e.pop().unwrap_or_default());
            }
        });
        Some(rx)
    }

    /// A person's message, its node written: its mark, staged by
    /// `turn.submit` where the live proof was, is written beside it. The one
    /// that opens the turn is its ask; a steer joins the exchange's messages.
    pub(super) fn exchange_heard(&self, t: &mut Turn<'_>, node: &Node, text: &str, steered: bool) {
        if !self.ledger_on(t) {
            return;
        }
        let mark = self
            .exchanges
            .arrivals
            .take(t.tc.session_id, text)
            .map(|m| OwnerMark {
                node: node.id.clone(),
                ran_session: t.tc.session_id.into(),
                at_ms: node.created_at_ms,
                ..m
            });
        defer(t, mark.as_ref().and_then(|m| mark_record(m).ok()));
        if steered {
            if let Some(ex) = &mut t.exchange.ex {
                ex.messages.push(node.id.clone());
            }
            return;
        }
        t.exchange.ask = Some((node.id.clone(), text.chars().count() as u64));
        t.exchange.mark = mark;
        t.exchange.things = self.things_of(text);
    }

    /// The exchange's opening half, once the turn's input is written: what
    /// opened it, its place (where a person's message arrived, else the
    /// session's own), and its person, with the place's and the person's
    /// touches. The things of the place's last reply, named since, are
    /// touched now for that exchange.
    pub(super) fn exchange_open(&self, t: &mut Turn<'_>, prompted: bool) {
        if !self.ledger_on(t) {
            return;
        }
        let sid = t.tc.session_id;
        let h = &t.exchange;
        let opened_by = match () {
            _ if prompted => Opener::Prompt,
            _ if h.ask.is_some() => Opener::Message,
            _ if !t.wakes.is_empty() => Opener::Wake,
            _ if !t.woke_by.is_empty() => Opener::Report,
            _ => Opener::Continuation,
        };
        let place = h.mark.as_ref().map_or_else(
            || {
                place_of(
                    self.outbox.try_target(sid).ok().flatten().as_deref(),
                    t.tc.dir,
                )
            },
            |m| m.place.clone(),
        );
        let ask = h.ask.as_ref().map(|(id, _)| id.clone());
        let ex = Exchange {
            turn_id: t.tc.turn_id.into(),
            session_id: sid.into(),
            private: t.tc.class == PlaceClass::Private,
            opened_by,
            person: h.mark.as_ref().and_then(OwnerMark::person),
            start_ms: theseus_protocol::now_unix_ms(),
            end_ms: None,
            messages: ask.iter().cloned().collect(),
            ask,
            reply: None,
            stop: None,
            rebuilt: false,
            place,
        };
        defer(t, exchange_record(&ex).ok());
        let at = ex.start_ms;
        defer(t, touch(Root::Place(&ex.place), &ex.turn_id, sid, at));
        if let Some(p) = &ex.person {
            defer(t, touch(Root::Person(p), &ex.turn_id, sid, at));
        }
        if let Some(mut p) = self.exchanges.take_reply(&ex.place) {
            if let Ok(terms) = p.rx.try_recv() {
                for term in &terms {
                    defer(t, touch(Root::Thing(term), &p.turn, &p.session, p.at_ms));
                }
                t.exchange.reply_things = Some(ledger::by_kind(&terms));
            }
        }
        t.exchange.ex = Some(ex);
    }

    /// What the turn's first call is sent, by kind, after clearing: the last
    /// compile of loop 0 is the one the call uses (a route's switch compiles
    /// it again).
    pub(super) fn exchange_sent(
        t: &mut Turn<'_>,
        (nodes, compiled, spec): (&Transcript, &Compiled, &RequestSpec),
        cold: Option<bool>,
        summary: &ContextCompiled,
    ) {
        if t.exchange.ex.is_none() || summary.loop_index != 0 {
            return;
        }
        let cleared: HashSet<&str> = crate::compiler::clearing::drops(Some(&compiled.budget))
            .filter_map(|d| d.node_id.as_deref())
            .collect();
        let files = spec.context_files.iter().map(|f| f.bytes).sum();
        let view = View {
            includes: &compiled.compilation.includes,
            as_of: compiled.compilation.as_of,
        };
        let sent = ledger::sent(
            &compiled.request,
            nodes,
            &view,
            (
                &cleared,
                files,
                compiled.est_tokens,
                compiled.estimate.census.json,
            ),
        );
        t.exchange.seen = Some(Seen {
            compilation: compiled.compilation.id.clone(),
            sent,
            situation: summary.situation.as_ref().map(|s| s.kind().into()),
            trigger: summary.trigger.clone(),
            cold: cold.unwrap_or(false),
            place_tail: t.digest.tail.is_some(),
        });
    }

    /// The exchange's end, in the turn's last frame: the same key again with
    /// its end, its final reply and how it stopped; the message's things; in
    /// a private place, its measuring row. The reply's things are named for
    /// the place's next exchange to touch.
    pub(super) fn exchange_close(&self, t: &mut Turn<'_>, result: &TurnSubmitResult) {
        let Some(mut ex) = t.exchange.ex.take() else {
            return;
        };
        ex.end_ms = Some(theseus_protocol::now_unix_ms());
        ex.reply = t.said.last().map(|(_, id)| id.clone());
        ex.stop = Some(result.stop_reason.clone());
        defer(t, exchange_record(&ex).ok());
        let things = t
            .exchange
            .things
            .take()
            .and_then(|mut rx| rx.try_recv().ok());
        for term in things.iter().flatten() {
            defer(
                t,
                touch(Root::Thing(term), &ex.turn_id, &ex.session_id, ex.start_ms),
            );
        }
        if ex.private {
            let h = &t.exchange;
            let row = ledger::row(
                t.tc.store,
                Given {
                    ex: &ex,
                    seen: h.seen.as_ref(),
                    peer: h.mark.as_ref().map(|m| m.peer),
                    arrived_session: h.mark.as_ref().map(|m| m.arrived_session.as_str()),
                    ask_chars: h.ask.as_ref().map(|(_, n)| *n),
                    usage: &result.usage,
                    things: things.as_deref().map(ledger::by_kind),
                    reply_things: h.reply_things.clone(),
                },
            );
            t.record(&row);
        }
        if let Some(rx) = self.things_of(&result.output) {
            self.exchanges.put_reply(
                &ex.place,
                Pending {
                    turn: ex.turn_id,
                    session: ex.session_id,
                    at_ms: ex.end_ms.unwrap_or(ex.start_ms),
                    rx,
                },
            );
        }
    }
}
