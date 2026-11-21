//! A message goes to a thread by its address only (theseus-rha2; the owner,
//! 2026-11-21): the flow `turn.submit` runs before it opens a turn, in a
//! private place with live threads. `crate::thread_route` reads the address
//! (`#slug`, a place bound to the thread, a reply to a thread's message);
//! this is the rest:
//!
//! - **Nothing changes without threads.** No live thread for the place, the
//!   session being a thread's own, a task, a job's message, a prompt, a slash
//!   command: the function returns at once and the turn is the turn it was
//!   (a plain turn stays 5 frames and starts no timer).
//! - **No address, no thread.** A message that names none runs in the place's
//!   own conversation, always, at once: nothing guesses, waits or asks.
//! - **Jev is asked once, as before.** With two or more live threads the
//!   inbound point begins here, before the session is known, and the turn
//!   takes it over (`Handoff`): `classify`'s request is the one it was, its
//!   `thread` question still asked. Nothing reads that answer to place a
//!   message; a later pack version drops the question.
//! - **Replies go back to the place** the message came from
//!   (`Outbox::show_at`): a thread has no place of its own.

use std::time::Instant;

use theseus_judge::builders::ThreadInput;
use theseus_protocol::thread_route::{RouteHow, ThreadTag};
use theseus_protocol::threads::ThreadInfo;
use theseus_protocol::{error_code, Event, ThreadPlaced, TurnSubmitParams, TurnSubmitResult};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::Surface;
use crate::bus::EventSink;
use crate::fact::thread_route::RoutedRow;
use crate::judge::inbound::{
    author_of, place_kind, slash_command, Early, Inbound, CLASSIFY_PACK, ROUTE_PACK,
};
use crate::session::SessionRecord;
use crate::thread_flow::{Handoff, Routed};
use crate::thread_route::{address, top_entities, Address, Addressing};

/// What the router decided for a turn, which `turn.submit` keeps.
pub(super) struct Routing {
    pub tag: ThreadTag,
    /// The place's own session, where the message arrived.
    pub home: String,
    /// The session that runs the turn.
    pub session: String,
    /// The thread it runs in; none: at home.
    pub slug: Option<String>,
    /// The bench's clock the message arrived at, and when it was read
    /// (theseus-98bz.3): its reply is recorded at that clock plus the turn's
    /// real duration.
    pub bench: Option<(u64, Instant)>,
}

/// The threads as Jev reads them: slug, title, goal and top entities.
fn inputs(live: &[&ThreadInfo]) -> Vec<ThreadInput> {
    live.iter()
        .map(|t| ThreadInput {
            id: t.slug.clone(),
            title: t.title.clone(),
            goal: t.goal.clone(),
            entities: top_entities(&t.working_set, theseus_judge::builders::THREAD_ENTITIES),
        })
        .collect()
}

/// A routed message's row before anything is known: it ran at home.
fn fresh_row(home: &str) -> RoutedRow {
    RoutedRow {
        home: home.to_string(),
        to: None,
        session: home.to_string(),
        how: RouteHow::Home,
        confidence: 1.0,
        early: false,
        movable: false,
        tail: None,
    }
}

/// What an address names: the thread, certain; an explicit word is taken off
/// the message.
fn explicit_tag(a: &Address, live: &[&ThreadInfo], p: &mut TurnSubmitParams) -> ThreadTag {
    if let Address::Explicit { text, .. } = a {
        if !text.is_empty() {
            p.input.clone_from(text);
        }
    }
    let slug = a.slug();
    ThreadTag {
        slug: Some(slug.to_string()),
        title: live
            .iter()
            .find(|t| t.slug == slug)
            .map_or_else(|| slug.to_string(), |t| t.title.clone()),
        how: a.how(),
        confidence: 1.0,
        movable: false,
    }
}

/// The place's own conversation: where a message with no address runs.
fn home_tag(confidence: f64, movable: bool) -> ThreadTag {
    ThreadTag {
        slug: None,
        title: "home".into(),
        how: RouteHow::Home,
        confidence,
        movable,
    }
}

impl Core {
    /// A message in a place with threads runs where its address says, else
    /// at home: the session the turn runs in is put in `session`, the message
    /// is put in `p.input` (an explicit address taken off), and the tag comes
    /// back for the reply. `None`: routing does not apply, and nothing was
    /// touched.
    pub(super) fn route_to_thread(
        &self,
        p: &mut TurnSubmitParams,
        conn: Conn<'_>,
        session: &mut SessionRecord,
    ) -> Result<Option<Routing>, RpcFailure> {
        if !self.may_route(p, conn, session) {
            return Ok(None);
        }
        let board = self.threads();
        let home = session.session_id.clone();
        if board.is_empty() || board.of_session(&home).is_some() {
            return Ok(None);
        }
        let class = self.runner.class_of(&home);
        let live = board.live(class);
        if live.is_empty() {
            return Ok(None);
        }
        let state = &self.runner.threads.route;
        let bench = self.bench_clock(p);
        let now = bench.unwrap_or_else(theseus_protocol::now_unix_ms);
        let place = self.outbox.target(&home);
        let turn_id = crate::new_id("turn");
        let mut row = fresh_row(&home);
        let mut early: Option<Early> = None;
        let addr = self.addressed(p, place.as_deref(), &live);
        let tag = match addr {
            Some(a) => explicit_tag(&a, &live, p),
            None => {
                let at = (&*session, class, place.as_deref(), turn_id.as_str());
                early = self.begin_inbound(p, conn, at, &live);
                row.early = early.is_some();
                home_tag(1.0, false)
            }
        };
        let (tag, run_session) = self.session_for(tag, session);
        self.show_thread_at(&tag, &run_session, place.as_deref());
        let tail = self.place_tail(&home, &run_session.session_id, &live, &mut row);
        state.arrived(&home, tag.slug.as_deref(), now);
        (row.to, row.session) = (tag.slug.clone(), run_session.session_id.clone());
        (row.how, row.confidence, row.movable) = (tag.how, tag.confidence, tag.movable);
        state.routed(Routed {
            turn: turn_id.clone(),
            home: home.clone(),
            to: tag.slug.clone(),
            session: run_session.session_id.clone(),
            text: p.input.clone(),
            author: p.author.clone().unwrap_or_else(|| conn.client.to_string()),
            how: tag.how,
            at_ms: now,
        });
        self.tell_placed(conn, &home, &run_session.session_id, &turn_id, &tag);
        state.hand_off(
            &run_session.session_id,
            Handoff {
                turn_id,
                text: p.input.clone(),
                early,
                row: Some(row),
                tail,
                at: Instant::now(),
            },
        );
        let routing = Routing {
            slug: tag.slug.clone(),
            tag,
            home,
            session: run_session.session_id.clone(),
            bench: bench.map(|at| (at, Instant::now())),
        };
        *session = run_session;
        Ok(Some(routing))
    }

    /// What was just said in the place, for a turn that runs in `run`, when the
    /// place's last exchange ran in another session: the place's own or an
    /// unsealed thread's (`compiler::place_tail`). The row says where from and
    /// what it costs.
    fn place_tail(
        &self,
        home: &str,
        run: &str,
        live: &[&ThreadInfo],
        row: &mut RoutedRow,
    ) -> Option<String> {
        let last = self.runner.threads.route.last(home)?;
        let (sid, label) = match &last.slug {
            None => (home.to_string(), "the place's own conversation".to_string()),
            Some(slug) => {
                let t = live.iter().find(|t| &t.slug == slug)?;
                let open = !crate::compiler::place_digest::held(t);
                (
                    t.session_id.clone().filter(|_| open)?,
                    format!("the thread \u{ab}{}\u{bb}", t.title),
                )
            }
        };
        let half = self.cfg.threads.sticky_half_life_secs.saturating_mul(1000);
        let since = last.at_ms.saturating_sub(half);
        let tail = (sid != run)
            .then(|| crate::compiler::place_tail::block(&self.store, &sid, &label, since))?
            .unwrap_or_else(|e| {
                tracing::warn!(error = %format!("{e:#}"), "the place's last words were not read");
                None
            })?;
        let from = if last.slug.is_some() {
            "thread"
        } else {
            "home"
        };
        row.tail = Some((from.to_string(), tail.tokens));
        Some(tail.text)
    }

    /// What a message addresses: a thread in its own words, or the thread of
    /// the turn it replies to.
    fn addressed(
        &self,
        p: &TurnSubmitParams,
        place: Option<&str>,
        live: &[&ThreadInfo],
    ) -> Option<Address> {
        let reply_thread = p
            .in_reply_to
            .as_deref()
            .and_then(|t| self.runner.threads.route.thread_of_turn(t));
        address(
            &Addressing {
                text: &p.input,
                place,
                reply_thread: reply_thread.as_deref(),
            },
            live,
        )
    }

    /// A message the router put in a thread: the thread's replies and cards
    /// go back to the place the message came from, or nowhere for the CLI.
    fn show_thread_at(&self, tag: &ThreadTag, run_session: &SessionRecord, place: Option<&str>) {
        if tag.slug.is_some() {
            if let Err(e) = self.outbox.show_at(&run_session.session_id, place) {
                tracing::warn!(error = %format!("{e:#}"), "a thread's reply place was not written");
            }
        }
    }

    /// The sender hears where its message went now, not at the turn's end
    /// (theseus-7kjb7): a notification to its connection; `thread.routed` is
    /// the row. Only a turn the router put in a thread has anything to tell.
    fn tell_placed(
        &self,
        conn: Conn<'_>,
        home: &str,
        placed: &str,
        turn_id: &str,
        tag: &ThreadTag,
    ) {
        if tag.slug.is_none() {
            return;
        }
        let direct = Some((conn.client.to_string(), conn.tx.clone()));
        EventSink::new(self.bus.clone(), home, direct).send(Event::ThreadPlaced(ThreadPlaced {
            session_id: home.to_string(),
            placed: placed.to_string(),
            turn_id: turn_id.to_string(),
            tag: tag.clone(),
        }));
    }

    /// The session that runs it. A thread that cannot take it (it was paused
    /// as the message arrived) sends it home.
    fn session_for(&self, tag: ThreadTag, home: &SessionRecord) -> (ThreadTag, SessionRecord) {
        let Some(slug) = tag.slug.clone() else {
            return (tag, home.clone());
        };
        match self.thread_session_open(&slug) {
            Ok(s) => (tag, s),
            Err(e) => {
                tracing::warn!(thread = %slug, error = %e.message, "a routed thread cannot take the message: it runs at home");
                (home_tag(0.0, true), home.clone())
            }
        }
    }

    /// Whether this message is one the router places at all. A session the
    /// owner named with `theseus ask -s` is their choice, never re-routed
    /// (theseus-0rfjx): `ask` says so with its `via`. The chat's own session
    /// is the place's home, which the router may leave, and says so.
    fn may_route(&self, p: &TurnSubmitParams, conn: Conn<'_>, session: &SessionRecord) -> bool {
        self.cfg.threads.enabled
            && (p.via.as_deref() != Some("ask") || self.bench_clock(p).is_some())
            && p.thread.is_none()
            && p.prompt.is_none()
            && p.opened_from.is_none()
            && p.session_id.is_some()
            && session.task.is_none()
            && !p.input.trim().is_empty()
            && !matches!(conn.surface, Surface::Mcp | Surface::Unnamed)
            && !(slash_command(&p.input) && !p.input.trim_start().starts_with("/t "))
    }

    /// The bench's clock a submit carries, under `[threads] bench_clock`
    /// alone (theseus-98bz.3): the message stands for one typed in its
    /// session's place at that time.
    fn bench_clock(&self, p: &TurnSubmitParams) -> Option<u64> {
        p.route_at_ms.filter(|_| self.cfg.threads.bench_clock)
    }

    /// A submit carrying the bench's clock to a daemon that does not take it.
    pub(super) fn refuse_bench_clock(&self, p: &TurnSubmitParams) -> Result<(), RpcFailure> {
        match p.route_at_ms.is_some() && !self.cfg.threads.bench_clock {
            true => Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "route_at_ms is the threadbench's clock: this daemon takes it only with \
                 [threads] bench_clock = true",
            )),
            false => Ok(()),
        }
    }

    /// The inbound point for a message that runs at home, begun now as it was
    /// while the router waited on it, so `classify` is asked the request it
    /// was (its `thread` question among it) and the turn takes it over: none
    /// with fewer than two live threads, or a judge that does not act on
    /// `classify`. Nothing waits on the `thread` answer, and nothing reads it.
    fn begin_inbound(
        &self,
        p: &TurnSubmitParams,
        conn: Conn<'_>,
        (session, class, place, turn_id): (
            &SessionRecord,
            crate::places::PlaceClass,
            Option<&str>,
            &str,
        ),
        live: &[&ThreadInfo],
    ) -> Option<Early> {
        let judge = &self.runner.judge;
        let home = &session.session_id;
        let acting = live.len() >= 2
            && judge.config().enabled
            && judge.reachable()
            && judge.mode_for(CLASSIFY_PACK, home).mode >= crate::config::PackMode::Canary;
        if !acting {
            return None;
        }
        let route = self
            .cfg
            .routing
            .pack_mode(judge.mode_for(ROUTE_PACK, home).mode);
        let route = match crate::routing::chosen(p) {
            Some(_) => route.min(crate::config::PackMode::Shadow),
            None => route,
        };
        judge.inbound_early(Inbound {
            session_id: home.clone(),
            execution_id: session.execution_id.clone().unwrap_or_default(),
            turn_id: turn_id.to_string(),
            node_id: String::new(),
            text: p.input.clone(),
            place_kind: place_kind(place, conn.client, class),
            author: author_of(class).into(),
            kernel: self.kernel.clone(),
            threads: inputs(live),
            in_thread: false,
            route,
            chosen: None,
            task: false,
            own: false,
            persona: self.runner.persona_ctx(home, class),
            // The owner's own words: `directive.v2` rides this call, and
            // the turn says where the message was written (`Early::said`).
            capture: self.capture_wanted(p, conn) && crate::capture::owner_alone(class, place),
            data_names: self.cfg.privacy.data_names(),
            attachments: Vec::new(),
        })
    }

    /// After the routed turn ran: its handoff is gone, the thread's turn is
    /// known by its id (a reply to its reply finds the thread), and the place
    /// has heard a reply.
    pub(super) fn thread_routed_turn(&self, r: &Routing, res: Option<&TurnSubmitResult>) {
        let state = &self.runner.threads.route;
        state.clear_handoff(&r.session);
        let Some(res) = res else { return };
        if let Some(slug) = &r.slug {
            state.turn_ran(&res.turn_id, slug);
        }
        let replied = r
            .bench
            .map_or_else(theseus_protocol::now_unix_ms, |(at, t0)| {
                at + t0.elapsed().as_millis() as u64
            });
        state.replied(&r.home, replied);
    }
}
