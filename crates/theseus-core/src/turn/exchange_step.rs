//! A turn's exchange (issue 44, step 1; `crate::exchange`): the owner's mark
//! as the input's node is heard, the exchange's opening half once the input
//! is written, what the first compile sent, and the end with its measuring
//! row. Every record rides a frame the turn writes anyway (`Store::defer`);
//! a naming of things, off the path, rides any writer's next one. Nothing here
//! changes what any request carries. A task's turns are at the task's own
//! place (`task_graph::place`), its first turn's ask its brief.

use std::collections::{BTreeMap, HashSet};

use theseus_protocol::{ContextCompiled, TurnSubmitResult};
use tokio::sync::oneshot;

use super::{Turn, TurnRunner};
use crate::compiler::{Compiled, RequestSpec};
use crate::exchange::ledger::{self, Given, Seen, View};
use crate::exchange::{
    exchange_record, mark_record, put_reply, thing_touches, touch, Exchange, Opener, OwnerMark,
    ReplyThings, Root,
};
use crate::node::Node;
use crate::places::PlaceClass;
use crate::store::Transcript;
use crate::turn::view_step::occasion::Occasion;

/// The most of a message the index is asked to name things in.
const THINGS_TEXT: usize = crate::exchange::THINGS_TEXT;

/// What a turn holds of its exchange until it ends.
#[derive(Default)]
pub(super) struct Held {
    ex: Option<Exchange>,
    /// The message that opened it, heard before the exchange is opened: its
    /// node, its length, and its words until the naming of its things starts.
    ask: Option<(String, u64)>,
    ask_text: Option<String>,
    mark: Option<OwnerMark>,
    /// The message's things by kind, told once the index has named them.
    things: Option<oneshot::Receiver<BTreeMap<String, u64>>>,
    /// The scope of its numbered items (`names::items::scope_for`): the
    /// project of the session the message arrived at.
    scope: Option<String>,
    /// The message's numbered items, scoped, told once the index has named
    /// them (`turn::view_step` reads them, `Held::await_items`).
    items: Option<oneshot::Receiver<Vec<String>>>,
    items_named: Option<Vec<String>>,
    /// The things of the reply before it, when they were named in time.
    reply_things: Option<BTreeMap<String, u64>>,
    seen: Option<Seen>,
    /// The turn's place, read once ([`TurnRunner::turn_place`]): none inside when it could not be read.
    read_place: Option<Option<String>>,
    /// What woke a turn no message, wake or report opened, as its catch-up told it.
    occasion: Option<Occasion>,
}

impl Held {
    /// The person of the message that opened it, and whether it carried a mark.
    pub(super) fn person(&self) -> (Option<String>, bool) {
        (
            self.mark.as_ref().and_then(|m| m.person()),
            self.mark.is_some(),
        )
    }

    /// The mark of the message that opened the turn, when one was written.
    pub(super) fn mark(&self) -> Option<&OwnerMark> {
        self.mark.as_ref()
    }

    /// The turn's place, once read and readable ([`TurnRunner::turn_place`]).
    pub(super) fn read_place(&self) -> Option<&str> {
        self.read_place.as_ref().and_then(|p| p.as_deref())
    }

    /// Whether the turn's place was read and could not be (the view fails such a turn plainly).
    pub(super) fn place_unread(&self) -> bool {
        matches!(self.read_place, Some(None))
    }

    /// What woke the turn, when no message of its own opened it.
    pub(super) fn occasion(&self) -> Option<Occasion> {
        self.occasion
    }

    /// The turn's catch-up says what woke it (`Occasion::of`).
    pub(super) fn caught_up(&mut self, occasion: Option<Occasion>) {
        self.occasion = occasion;
    }

    /// The message's numbered items, once the naming started at the
    /// exchange's opening has told them: bounded by that naming's own wait
    /// (`names::items::WAIT`), none when it gave up or never started.
    pub(super) async fn await_items(&mut self) {
        if let Some(rx) = self.items.take() {
            self.items_named = Some(rx.await.unwrap_or_default());
        }
    }

    /// The message's numbered items, as [`Held::await_items`] took them.
    pub(super) fn items(&self) -> &[String] {
        self.items_named.as_deref().unwrap_or_default()
    }
}

/// A test's seam: the sessions whose route (`speaks_at`) reads as unreadable.
#[cfg(test)]
static UNREADABLE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Whether `session`'s route reads as unreadable (a test's).
#[cfg(test)]
pub(super) fn route_is_unreadable(session: &str) -> bool {
    UNREADABLE.lock().unwrap().iter().any(|s| s == session)
}

/// Make `session`'s route unreadable to its turns (a test's).
#[cfg(test)]
pub(crate) fn route_unreadable(session: &str) {
    UNREADABLE.lock().unwrap().push(session.into());
}

/// Make `session`'s route readable again (a test's).
#[cfg(test)]
pub(crate) fn route_readable(session: &str) {
    UNREADABLE.lock().unwrap().retain(|s| s != session);
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
    /// The turn's place, read once and the same for its exchange and its gate (issue 44): where its
    /// message arrived (its mark's), else where its reply is delivered (`speaks_at`: a wake's or a
    /// report's target too). None when that cannot be read: a route that cannot be read is no place,
    /// never the terminal's.
    pub(super) fn turn_place(&self, t: &mut Turn<'_>) -> Option<String> {
        if let Some(read) = &t.exchange.read_place {
            return read.clone();
        }
        let read = t.exchange.mark.as_ref().map_or_else(
            || {
                #[cfg(test)]
                if UNREADABLE
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|s| s == t.tc.session_id)
                {
                    return None;
                }
                (self.outbox.speaks_at(t.tc.session_id))
                    .ok()
                    .map(|at| crate::exchange::place_of(at.as_deref(), t.tc.dir))
            },
            |m| Some(m.place.clone()),
        );
        // A turn no message opened answers where its session speaks now, else where its work was asked
        // (`resume_step`).
        let read = self.resumed_place(t, read);
        t.exchange.read_place = Some(read.clone());
        read
    }

    /// The index's things in `text`, named off the turn's path: the task
    /// writes their touches itself, keyed at the exchange's `start_ms`, riding
    /// any writer's next frame (`Store::defer_waiting`), however long the index
    /// takes; `counts` is told their counts by kind. None with no index, or
    /// outside a runtime.
    /// The core, weakly, for a naming task: upgraded only at the moment it
    /// writes, so a core dropped while the index is still answering lets its
    /// store go at once (a restart opens it again); the touches it would have
    /// written are then not written, and `exchange.rebuild` fills them. None
    /// outside a core.
    fn weak_core(&self) -> Option<std::sync::Weak<crate::rpc::Core>> {
        self.threads.core().map(|c| std::sync::Arc::downgrade(&c))
    }

    fn things_of(
        &self,
        text: &str,
        (turn, session, start_ms): (&str, &str, u64),
        counts: impl FnOnce(BTreeMap<String, u64>) + Send + 'static,
    ) -> Option<()> {
        let index = self.pass.index()?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let text = crate::scrub::safe::head_bytes(text, THINGS_TEXT).0;
        let (text, core) = (text.to_string(), self.weak_core()?);
        let (turn, session) = (turn.to_string(), session.to_string());
        rt.spawn(async move {
            let Ok(mut named) = index.entities(vec![text]).await else {
                return;
            };
            let terms = named.pop().unwrap_or_default();
            let Some(core) = core.upgrade() else {
                return;
            };
            for r in thing_touches(&terms, &turn, &session, start_ms) {
                if let Err(e) = core.store.defer_waiting(r) {
                    tracing::warn!(error = %format!("{e:#}"), "a thing's touch was not written");
                }
            }
            counts(ledger::by_kind(&terms));
        });
        Some(())
    }

    /// `text`'s numbered items scoped by `scope` (`names::items`), named
    /// apart from its things so a slow tender costs them nothing, given up
    /// after `names::items::WAIT`, and touched as the things are. With `tell`
    /// (a place set to the view), the receiver is told them, scoped, and is
    /// dropped untold when they are given up; without, there is no receiver,
    /// and nothing is done beyond what a transcript place always did.
    fn items_of(
        &self,
        text: &str,
        (turn, session, start_ms): (&str, &str, u64),
        (scope, tell): (Option<String>, bool),
    ) -> Option<oneshot::Receiver<Vec<String>>> {
        let (Some(index), Ok(rt), Some(core)) = (
            self.pass.index(),
            tokio::runtime::Handle::try_current(),
            self.weak_core(),
        ) else {
            return None;
        };
        let (tx, rx) = match tell {
            true => {
                let (tx, rx) = oneshot::channel();
                (Some(tx), Some(rx))
            }
            false => (None, None),
        };
        let text = crate::scrub::safe::head_bytes(text, THINGS_TEXT)
            .0
            .to_string();
        let (turn, session) = (turn.to_string(), session.to_string());
        rt.spawn(async move {
            let asked = index.items(vec![text]);
            let Ok(Ok(mut named)) = tokio::time::timeout(crate::names::items::WAIT, asked).await
            else {
                return;
            };
            let items =
                crate::names::items::scoped(named.pop().unwrap_or_default(), scope.as_deref());
            if let Some(tx) = tx {
                let _ = tx.send(items.clone());
            }
            let Some(core) = core.upgrade() else {
                return;
            };
            for r in thing_touches(&items, &turn, &session, start_ms) {
                if let Err(e) = core.store.defer_waiting(r) {
                    tracing::warn!(error = %format!("{e:#}"), "an item's touch was not written");
                }
            }
        });
        rx
    }

    /// A person's message, its node written: `arrival`, the mark its own
    /// submission carried (a steer's with it), is written beside it. The one
    /// that opens the turn is its ask; a steer joins the exchange's messages.
    pub(super) fn exchange_heard(
        t: &mut Turn<'_>,
        node: &Node,
        arrival: Option<&OwnerMark>,
        steered: bool,
    ) {
        let crate::node::Body::UserMessage { text, .. } = &node.body else {
            return;
        };
        let mark = arrival.map(|m| OwnerMark {
            node: node.id.clone(),
            ran_session: t.tc.session_id.into(),
            at_ms: node.created_at_ms,
            ..m.clone()
        });
        defer(t, mark.as_ref().and_then(|m| mark_record(m).ok()));
        if steered {
            if let Some(ex) = &mut t.exchange.ex {
                ex.messages.push(node.id.clone());
            }
            return;
        }
        t.exchange.ask = Some((node.id.clone(), text.chars().count() as u64));
        t.exchange.ask_text = Some(text.clone());
        t.exchange.mark = mark;
        t.exchange.scope =
            crate::names::items::scope_for(t.tc.store, t.exchange.mark.as_ref(), t.tc.session_id);
    }

    /// The exchange's opening half, once the turn's input is written: what
    /// opened it, its place (where a person's message arrived, else the
    /// session's own), and its person, with the place's and the person's
    /// touches; the naming of its message's things starts here, keyed at its
    /// start, and the counts of the place's last reply's are taken for its row.
    pub(super) fn exchange_open(&self, t: &mut Turn<'_>, prompted: bool) {
        let sid = t.tc.session_id;
        // A task's first turn answers its brief, written before the turn: its ask.
        let brief = (t.tc.task.is_some() && t.exchange.ask.is_none())
            .then(|| t.tc.store.transcript(sid).ok())
            .flatten()
            .and_then(|nodes| super::view_step::task::brief(&nodes, t.tc.turn_id));
        // A task's turns are at the task's own place; any other turn at its one place (`turn_place`).
        let place = match t.tc.task {
            Some(_) => crate::task_graph::place::of_session(sid),
            None => {
                (self.turn_place(t)).unwrap_or_else(|| crate::exchange::rebuild::UNKNOWN.into())
            }
        };
        let h = &t.exchange;
        let opened_by = match () {
            _ if prompted => Opener::Prompt,
            _ if h.ask.is_some() || brief.is_some() => Opener::Message,
            _ if !t.wakes.is_empty() => Opener::Wake,
            _ if !t.woke_by.is_empty() => Opener::Report,
            _ => Opener::Continuation,
        };
        let ask = (h.ask.as_ref().map(|(id, _)| id.clone())).or(brief.map(|(id, _)| id));
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
        // The place's exchange before this one, whose reply's counts these are.
        let before =
            crate::exchange::newest(t.tc.store, Root::Place(&ex.place), 2).unwrap_or_default();
        let preceding = before
            .iter()
            .find(|p| p.exchange != ex.turn_id)
            .map(|p| p.exchange.as_str());
        t.exchange.reply_things = self.exchanges.take_reply(&ex.place, preceding);
        if let Some(text) = t.exchange.ask_text.take() {
            // The view reads the message's items back (`turn::view_step`).
            let scope = (t.exchange.scope.clone(), true);
            t.exchange.items = self.items_of(&text, (&ex.turn_id, sid, at), scope);
            let (tx, rx) = oneshot::channel();
            let told = move |c| {
                let _ = tx.send(c);
            };
            if self
                .things_of(&text, (&ex.turn_id, sid, at), told)
                .is_some()
            {
                t.exchange.things = Some(rx);
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
        });
    }

    /// The exchange's end, in the turn's last frame: the same key again with
    /// its end, its final reply and how it stopped; the message's things; in
    /// a private place, its measuring row. The reply's things are named off
    /// the path, touched at this exchange's start, and counted for the
    /// place's next row.
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
                    things,
                    reply_things: h.reply_things.clone(),
                },
            );
            t.record(&row);
        }
        let replies = self.exchanges.replies();
        let place = ex.place.clone();
        let (turn, start_ms) = (ex.turn_id.clone(), ex.start_ms);
        let keep = move |counts| {
            put_reply(
                &replies,
                &place,
                ReplyThings {
                    turn,
                    start_ms,
                    counts,
                },
            )
        };
        let at = (ex.turn_id.as_str(), ex.session_id.as_str(), ex.start_ms);
        // The reply's items are touched; nothing waits for them.
        let _ = self.items_of(&result.output, at, (t.exchange.scope.take(), false));
        self.things_of(&result.output, at, keep);
    }
}
