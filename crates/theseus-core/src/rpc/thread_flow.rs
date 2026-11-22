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
//! - **An address that cannot be honoured is refused** (issue 44): `/t` with
//!   a word that names no thread, a word that names a paused or done thread,
//!   and an addressed thread that does not open end `turn.submit` with one
//!   plain line and start no turn; the message never runs at home. A sealed
//!   or unreadable thread is refused as no thread at all (the gate's
//!   `may_name_thread`).
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
use theseus_protocol::threads::{ThreadInfo, ThreadState};
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
use crate::thread_route::{
    address, top_entities, typed_word, unresolved, Address, Addressing, Unresolved,
};

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

/// The rows of `thread.routed` read back for a reply's turn, at most: the
/// newest first, a page at a time.
const ROUTED_READ: usize = 4096;

/// Where turn `turn` ran, by its `thread.routed` row (written in the turn's
/// own first frame): `Some(Some(slug))` in a thread, `Some(None)` at home,
/// `None` when no row of the newest `ROUTED_READ` names the turn or the rows
/// cannot be read.
fn routed_thread_of(store: &crate::store::Store, turn: &str) -> Option<Option<String>> {
    let tags = vec![theseus_store::pages::ledger_kind(
        theseus_protocol::LedgerKind::ThreadRouted.as_str(),
    )];
    let (mut before, mut read) = (None, 0);
    while read < ROUTED_READ {
        let page = theseus_store::Page {
            kind: theseus_store::kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: None,
            until_ms: None,
            limit: 512,
        };
        let got = match store.ledger_page(&page) {
            Ok(Some(got)) => got,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "a reply's turn was not read back");
                return None;
            }
        };
        for r in &got.records {
            read += 1;
            let Ok(row) = r.decode::<crate::ledger::LedgerRow>() else {
                continue;
            };
            if row.turn_id.as_deref() == Some(turn) {
                let to = row.data["to"].as_str()?;
                return Some((to != "home").then(|| to.to_string()));
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => return None,
        }
    }
    None
}

/// An address the message gives that cannot be honoured, when none was
/// found among the live threads: its own words naming no live thread
/// (`unresolved`), or a reply to a turn of a thread of `nameable` that is no
/// longer live (paused, done).
fn unhonoured(
    p: &TurnSubmitParams,
    nameable: &[&ThreadInfo],
    reply_thread: Option<String>,
) -> Option<Unresolved> {
    unresolved(&p.input, nameable).or_else(|| {
        let slug = reply_thread?;
        let t = nameable.iter().find(|t| t.slug == slug)?;
        Some(Unresolved {
            word: t.title.clone(),
            slug: Some(slug),
        })
    })
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
        if board.of_session(&home).is_some() {
            return Ok(None);
        }
        let class = self.runner.class_of(&home);
        let live = board.live(class);
        let reply_thread = self.reply_thread(p);
        let nameable = board.nameable(class);
        if live.is_empty() {
            return self
                .addressed(p, (&home, None), (&[], &nameable), reply_thread)
                .map(|_| None);
        }
        let state = &self.runner.threads.route;
        let bench = self.bench_clock(p);
        let now = bench.unwrap_or_else(theseus_protocol::now_unix_ms);
        let place = self.outbox.target(&home);
        let turn_id = crate::new_id("turn");
        let mut row = fresh_row(&home);
        let mut early: Option<Early> = None;
        let at = (home.as_str(), place.as_deref());
        let addr = self.addressed(p, at, (&live, &nameable), reply_thread)?;
        let typed = typed_word(&p.input);
        let tag = match addr {
            Some(a) => explicit_tag(&a, &live, p),
            None => {
                let at = (&*session, class, place.as_deref(), turn_id.as_str());
                early = self.begin_inbound(p, conn, at, &live);
                row.early = early.is_some();
                home_tag(1.0, false)
            }
        };
        let (tag, run_session) = self.session_for(tag, session, typed)?;
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

    /// What a message in `home` at `place` addresses among the `live`
    /// threads: a thread in its own words, its place, or the thread of the turn
    /// it replies to. An address that cannot be honoured among the `nameable`
    /// (`unhonoured`) is the refusal.
    fn addressed(
        &self,
        p: &TurnSubmitParams,
        (home, place): (&str, Option<&str>),
        (live, nameable): (&[&ThreadInfo], &[&ThreadInfo]),
        reply_thread: Option<String>,
    ) -> Result<Option<Address>, RpcFailure> {
        let addressing = Addressing {
            text: &p.input,
            place,
            reply_thread: reply_thread.as_deref(),
        };
        if let Some(a) = address(&addressing, live) {
            return Ok(Some(a));
        }
        match unhonoured(p, nameable, reply_thread) {
            Some(u) => Err(self.refuse_address(home, &u, None)),
            None => Ok(None),
        }
    }

    /// The thread of the turn a message replies to: the router's memory of
    /// this run's turns first, else the turn's `thread.routed` row, the
    /// durable record a restart keeps (`routed_thread_of`). None: no reply,
    /// a turn the records place at home, or one no record names (a turn of a
    /// place without threads, or older than the rows read), which runs at
    /// home as before.
    fn reply_thread(&self, p: &TurnSubmitParams) -> Option<String> {
        let turn = p.in_reply_to.as_deref()?;
        if let Some(slug) = self.runner.threads.route.thread_of_turn(turn) {
            return Some(slug);
        }
        let slug = routed_thread_of(&self.store, turn)??;
        self.runner.threads.route.turn_ran(turn, &slug);
        Some(slug)
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

    /// The session that runs it. A thread its address names that cannot take
    /// it (paused as the message arrived, or not opened) refuses it: the
    /// message never runs at home in its place. `typed`: the address's word
    /// as the sender typed it, for the refusal.
    fn session_for(
        &self,
        tag: ThreadTag,
        home: &SessionRecord,
        typed: Option<String>,
    ) -> Result<(ThreadTag, SessionRecord), RpcFailure> {
        let Some(slug) = tag.slug.clone() else {
            return Ok((tag, home.clone()));
        };
        match self.thread_session_open(&slug) {
            Ok(s) => Ok((tag, s)),
            Err(e) => {
                tracing::warn!(thread = %slug, error = %e.message, "a thread its address names cannot take the message: it is refused");
                let u = Unresolved {
                    word: typed.unwrap_or_else(|| tag.title.clone()),
                    slug: Some(slug),
                };
                Err(self.refuse_address(&home.session_id, &u, Some(&e.message)))
            }
        }
    }

    /// The refusal of an address that cannot be honoured (issue 44): one
    /// plain line to the sender, as `turn.submit`'s answer, and no turn
    /// anywhere. Why the thread cannot take it is said only of a thread the
    /// gate lets this place name (`view_gate::may_name_thread`); a sealed or
    /// unreadable one reads as no thread at all.
    fn refuse_address(&self, home: &str, u: &Unresolved, failed: Option<&str>) -> RpcFailure {
        let word = &u.word;
        let named = u
            .slug
            .as_deref()
            .filter(|s| crate::view_gate::may_name_thread(&self.runner, home, s))
            .and_then(|s| self.threads().get(s).cloned());
        let why = match (named.as_ref().map(|t| t.state), failed) {
            (Some(ThreadState::Paused), _) => format!(
                "is paused: `theseus thread resume {}` lets it take messages",
                u.slug.as_deref().unwrap_or_default()
            ),
            (Some(ThreadState::Done), _) => format!(
                "is done: `theseus thread reopen {}` takes it up again",
                u.slug.as_deref().unwrap_or_default()
            ),
            (Some(ThreadState::Active), Some(e)) => format!("could not be opened ({e})"),
            _ => {
                return RpcFailure::new(
                    error_code::REFUSED,
                    format!(
                        "Not run: there is no thread \u{201c}{word}\u{201d}. Send it again with a \
                         thread's name, or without one to talk here."
                    ),
                )
            }
        };
        RpcFailure::new(
            error_code::REFUSED,
            format!(
                "Not run: the thread \u{201c}{word}\u{201d} {why}. Send it again then, or \
                 without the thread's name to talk here."
            ),
        )
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
