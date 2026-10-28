//! A message that names no thread finds its own (theseus-rha2): the flow
//! `turn.submit` runs before it opens a turn, in a private place with live
//! threads. `crate::thread_route` is the pure part (addressing, the
//! pre-router, fusion, the decision table); this is the rest:
//!
//! - **Nothing changes without threads.** No live thread for the place, the
//!   session being a thread's own, a task, a job's message, a prompt, a slash
//!   command: the function returns at once and the turn is the turn it was
//!   (a plain turn stays 5 frames and starts no timer).
//! - **One scoring pass in memory**, then at most `[threads] route_wait_ms` of
//!   wait, and only with two or more live threads and a pre-router that does
//!   not decide. The wait is for `classify.v2`'s `thread` answer, which rides
//!   the very request `role.v1` and the rest of the inbound batch ride: the
//!   point begins here, before the session is known, and the turn takes it
//!   over (`Handoff`), so Jev is asked once.
//! - **Asking** is a `thread.asked` notification and a wait for
//!   `thread.answer`, `[threads] ask_wait_secs` at most; a question nobody
//!   answers runs the message at home. A spoken turn (a voice call's) is
//!   never asked about: it runs at home at once.
//! - **Replies go back to the place** the message came from
//!   (`Outbox::show_at`): a thread has no place of its own.

use std::time::{Duration, Instant};

use theseus_judge::builders::ThreadInput;
use theseus_protocol::thread_route::{RouteHow, ThreadAsk, ThreadTag};
use theseus_protocol::threads::{ThreadInfo, ThreadNewParams};
use theseus_protocol::{error_code, Event, ThreadPlaced, TurnSubmitParams, TurnSubmitResult};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::fact::thread_route::{JevRow, RoutedRow, ThreadAnswered, ThreadAsked};
use crate::fact::{Rec, To};
use crate::judge::inbound::{
    author_of, place_kind, slash_command, Early, Inbound, ThreadAnswered as JevAnswer,
    CLASSIFY_PACK, ROUTE_PACK,
};
use crate::session::SessionRecord;
use crate::thread_flow::{Answered, Handoff, Pending, Routed};
use crate::thread_route::{
    address, decide, decide_single, decisive, lone_offer, new_entities, pre_route, propose,
    question, top_entities, Address, Addressing, AskChoice, Choice, Decision, Jev, Pick, Pre,
    Rules, Voice,
};

/// How many characters of the waiting message a question shows.
const PREVIEW: usize = 200;

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

fn preview(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.chars().count() > PREVIEW {
        true => format!("{}\u{2026}", line.chars().take(PREVIEW).collect::<String>()),
        false => line,
    }
}

/// Jev's pick from its answer, when it names a live thread, a new one, or
/// home.
fn pick_of(a: &JevAnswer, live: &[&ThreadInfo]) -> Option<Jev> {
    let pick = match a.choice.as_str() {
        "new_thread" => Pick::New,
        "home" => Pick::Home,
        slug if live.iter().any(|t| t.slug == slug) => Pick::Thread(slug.to_string()),
        _ => return None,
    };
    Some(Jev {
        pick,
        p: a.confidence,
    })
}

/// A turn spoken in a voice call: the Discord binding's word, read from a
/// Discord connection alone (theseus-slvl.4).
fn spoken(p: &TurnSubmitParams, conn: Conn<'_>) -> bool {
    conn.surface == Surface::Discord && p.surface.as_deref() == Some("voice")
}

/// A routed message's row before anything is known: it ran at home.
fn fresh_row(home: &str) -> RoutedRow {
    RoutedRow {
        home: home.to_string(),
        to: None,
        session: home.to_string(),
        how: RouteHow::Home,
        confidence: 0.0,
        pre: Vec::new(),
        jev: None,
        timed_out: false,
        waited_ms: None,
        ask: None,
        early: false,
        movable: false,
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

/// The judgment that placed a message and the options it was offered.
type Judged = Option<(String, Vec<String>)>;

impl Core {
    /// An unaddressed message finds its thread, if its place has any: the
    /// session the turn runs in is put in `session`, the message is put in
    /// `p.input` (an explicit address taken off), and the tag comes back for
    /// the reply. `None`: routing does not apply, and nothing was touched.
    pub(super) async fn route_to_thread(
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
        let mut scored: Vec<Pre> = Vec::new();
        let mut judgment: Judged = None;
        let reply_thread = p
            .in_reply_to
            .as_deref()
            .and_then(|t| state.thread_of_turn(t));
        let addr = address(
            &Addressing {
                text: &p.input,
                place: place.as_deref(),
                reply_thread: reply_thread.as_deref(),
            },
            &live,
        );
        let tag = match addr {
            Some(a) => explicit_tag(&a, &live, p),
            None => {
                let (tag, pre) = self
                    .place_unaddressed(
                        p,
                        conn,
                        (session, class, place.as_deref(), &turn_id),
                        &live,
                        (&mut row, &mut early, &mut judgment),
                        now,
                    )
                    .await;
                scored = pre;
                tag
            }
        };
        let (mut tag, run_session) = self.session_for(tag, session);
        if tag.slug.is_none() && tag.how == RouteHow::Quiet {
            tag.how = RouteHow::Home;
        }
        // A placement below the act bar may be wrong (the misroute guard).
        self.arm_guard(&tag, &run_session, &live, &scored);
        if tag.slug.is_some() {
            // The thread's replies and cards go back to the place the
            // message came from, or nowhere for the CLI.
            if let Err(e) = self
                .outbox
                .show_at(&run_session.session_id, place.as_deref())
            {
                tracing::warn!(error = %format!("{e:#}"), "a thread's reply place was not written");
            }
        }
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
            judgment,
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
                let home_tag = ThreadTag {
                    slug: None,
                    title: "home".into(),
                    how: RouteHow::Home,
                    confidence: 0.0,
                    movable: true,
                };
                (home_tag, home.clone())
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

    /// A message that names no thread: the pre-router's scores, then the
    /// decision (the pre-router alone when it is decisive, else Jev fused in,
    /// else a question). The scores come back for the guard.
    async fn place_unaddressed(
        &self,
        p: &TurnSubmitParams,
        conn: Conn<'_>,
        at: (
            &SessionRecord,
            crate::places::PlaceClass,
            Option<&str>,
            &str,
        ),
        live: &[&ThreadInfo],
        (row, early, judgment): (&mut RoutedRow, &mut Option<Early>, &mut Judged),
        now: u64,
    ) -> (ThreadTag, Vec<Pre>) {
        let (session, _, place, _) = at;
        let home = &session.session_id;
        let state = &self.runner.threads.route;
        let half = self.cfg.threads.sticky_half_life_secs.saturating_mul(1000);
        let mut pre = pre_route(&p.input, live, state.last(home).as_ref(), now, half);
        self.learned_thread(&p.input, &mut pre);
        self.keep_apart_by_dir(p, session, live, &mut pre);
        row.pre = pre
            .iter()
            .map(|x| (x.slug.clone(), x.score, x.hits.clone()))
            .collect();
        let tag = if live.len() < 2 {
            match self.offer_beside_one(p, live, &pre) {
                None => decide_single(live, &pre),
                // As below: a spoken turn is never held for a question.
                Some(_) if spoken(p, conn) => self.answer_tag(None, live, p, ("", "")),
                Some(ask) => {
                    self.ask_thread(ask, p, conn, (home, place), live, row)
                        .await
                }
            }
        } else if decisive(&pre) {
            let top = pre
                .iter()
                .max_by(|a, b| a.score.total_cmp(&b.score))
                .expect("two live threads");
            ThreadTag {
                slug: Some(top.slug.clone()),
                title: top.title.clone(),
                how: RouteHow::Quiet,
                confidence: top.score,
                movable: false,
            }
        } else {
            match self
                .decide_with_jev(p, conn, at, (live, &pre), row, (early, judgment))
                .await
            {
                Decision::Route(tag) => tag,
                // A spoken turn is never held for a typed question: the
                // speaker hears nothing while it waits, and what they say
                // next queues behind it (theseus-sro04: 215 s in the v1
                // voice check). It runs at home, as a question nobody
                // answered does.
                _ if spoken(p, conn) => self.answer_tag(None, live, p, ("", "")),
                other => {
                    self.ask_thread(other, p, conn, (home, place), live, row)
                        .await
                }
            }
        };
        (tag, pre)
    }

    /// A place's one live thread, which the message is not clearly about,
    /// beside a new stream (theseus-98bz.7): the ask that offers the thread,
    /// a new one and home, under `[threads] single_offers_new`.
    fn offer_beside_one(
        &self,
        p: &TurnSubmitParams,
        live: &[&ThreadInfo],
        pre: &[Pre],
    ) -> Option<Decision> {
        if !self.cfg.threads.single_offers_new || pre.first()?.score >= crate::thread_route::CONFIRM
        {
            return None;
        }
        let board = self.threads();
        let taken: Vec<&str> = board.all().map(|t| t.slug.as_str()).collect();
        let (title, _) = propose(&p.input, &taken);
        let every = self.cfg.threads.ask_every_thread;
        lone_offer(live, pre, new_entities(&p.input, live), &title, every)
    }

    /// The misroute guard for a placement below the act bar: the turn's first
    /// write or program outside the thread's roots is held.
    fn arm_guard(
        &self,
        tag: &ThreadTag,
        run: &SessionRecord,
        live: &[&ThreadInfo],
        scored: &[Pre],
    ) {
        let (dir, run) = (run.dir.as_deref(), run.session_id.as_str());
        match (&tag.slug, tag.movable) {
            (Some(slug), true) => {
                let roots = live
                    .iter()
                    .find(|t| &t.slug == slug)
                    .map(|t| t.working_set.roots.clone())
                    .unwrap_or_default();
                let score = |t: &ThreadInfo| {
                    scored
                        .iter()
                        .find(|p| p.slug == t.slug)
                        .map_or(0.0, |p| p.score)
                };
                let alt = live
                    .iter()
                    .filter(|t| &t.slug != slug)
                    .max_by(|a, b| score(a).total_cmp(&score(b)))
                    .map(|t| t.title.clone());
                let base = self.tools.cwd_for(dir);
                self.tools
                    .misroute
                    .arm(run, &tag.title, &roots, base.to_str(), alt.as_deref());
            }
            (Some(_), false) => self.tools.misroute.disarm(run),
            (None, _) => {}
        }
    }

    /// Whether a message may be the owner's "that was for ..." (a move by
    /// words): the same messages the router places.
    pub(super) fn words_may_move(&self, p: &TurnSubmitParams, conn: Conn<'_>) -> bool {
        self.cfg.threads.enabled
            && p.thread.is_none()
            && p.prompt.is_none()
            && p.opened_from.is_none()
            && !matches!(conn.surface, Surface::Mcp | Surface::Unnamed)
            && p.input.len() < 200
    }

    /// The layer's entry closest to the message, when the owner moved one
    /// like it: that thread gains the evidence of the owner's word.
    fn learned_thread(&self, text: &str, pre: &mut [Pre]) {
        let cfg = &self.cfg.routing.corrections;
        let words = crate::correction::layer::words(text);
        let Some(slug) = self.runner.judge.corrections.nearest_thread(&words, cfg) else {
            return;
        };
        if let Some(p) = pre.iter_mut().find(|p| p.slug == slug) {
            p.score = p.score.max(crate::thread_route::LEARNED);
            p.learned = true;
        }
    }

    /// Jev's choice of thread, waited for at most `route_wait_ms`: the inbound
    /// point begun now (no added call), the answer fused with the pre-router.
    async fn decide_with_jev(
        &self,
        p: &TurnSubmitParams,
        conn: Conn<'_>,
        (session, class, place, turn_id): (
            &SessionRecord,
            crate::places::PlaceClass,
            Option<&str>,
            &str,
        ),
        (live, pre): (&[&ThreadInfo], &[Pre]),
        row: &mut RoutedRow,
        (early_out, judgment): (&mut Option<Early>, &mut Judged),
    ) -> Decision {
        let judge = &self.runner.judge;
        let home = &session.session_id;
        let acting = judge.config().enabled
            && judge.reachable()
            && judge.mode_for(CLASSIFY_PACK, home).mode >= crate::config::PackMode::Canary;
        let (mut jev, mut voice) = (None, Voice::Alone);
        if acting {
            let route = self
                .cfg
                .routing
                .pack_mode(judge.mode_for(ROUTE_PACK, home).mode);
            let route = match crate::routing::chosen(p) {
                Some(_) => route.min(crate::config::PackMode::Shadow),
                None => route,
            };
            let started = judge.inbound_early(Inbound {
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
                persona: self.runner.persona_ctx(home, class),
                // The owner's own words: `directive.v1` rides this call, and
                // the turn says where the message was written (`Early::said`).
                capture: self.capture_wanted(p, conn) && crate::capture::owner_alone(class, place),
                data_names: self.cfg.privacy.data_names(),
                attachments: Vec::new(),
            });
            if let Some(mut e) = started {
                if let Some(rx) = e.thread.take() {
                    let wait = Duration::from_millis(self.cfg.threads.route_wait_ms);
                    let t0 = Instant::now();
                    match tokio::time::timeout(wait, rx).await {
                        Ok(Ok(a)) => {
                            row.waited_ms = Some(t0.elapsed().as_millis() as u64);
                            if a.acts {
                                jev = pick_of(&a, live);
                                voice = Voice::Jev;
                                row.jev = Some(JevRow {
                                    pick: a.choice.clone(),
                                    p: a.confidence,
                                    judgment: a.judgment.clone(),
                                    acts: a.acts,
                                });
                                *judgment = Some((a.judgment.clone(), a.offered));
                            }
                        }
                        Ok(Err(_)) => row.waited_ms = Some(t0.elapsed().as_millis() as u64),
                        Err(_) => {
                            row.timed_out = true;
                            row.waited_ms = Some(wait.as_millis() as u64);
                        }
                    }
                }
                row.early = true;
                *early_out = Some(e);
            }
        }
        let board = self.threads();
        let taken: Vec<&str> = board.all().map(|t| t.slug.as_str()).collect();
        let (title, _) = propose(&p.input, &taken);
        let sticky_alone_asks = self.cfg.threads.sticky_alone_asks;
        let rules = Rules {
            sticky_alone_asks,
            new_entities: match sticky_alone_asks {
                true => new_entities(&p.input, live),
                false => 0,
            },
            ask_every_thread: self.cfg.threads.ask_every_thread,
        };
        decide(live, pre, jev.as_ref(), voice, Some(&title), rules)
    }

    /// The router could not tell: one line, the options, and a wait for the
    /// owner's word. Returns the tag the answer gives (a new thread made on a
    /// yes), or home when nobody answers in time.
    async fn ask_thread(
        &self,
        decision: Decision,
        p: &TurnSubmitParams,
        conn: Conn<'_>,
        (home, place): (&str, Option<&str>),
        live: &[&ThreadInfo],
        row: &mut RoutedRow,
    ) -> ThreadTag {
        let board = self.threads();
        let taken: Vec<&str> = board.all().map(|t| t.slug.as_str()).collect();
        let (new_title, new_slug) = propose(&p.input, &taken);
        let (text, options): (String, Vec<AskChoice>) = match decision {
            Decision::Ask { options } => (question(&options), options),
            Decision::OfferNew { .. } => {
                let options = crate::thread_route::offer_new(&new_title);
                (
                    format!("Start a new thread \u{ab}{new_title}\u{bb} for this?"),
                    options,
                )
            }
            Decision::Route(_) => unreachable!("a route needs no question"),
        };
        let ask_id = crate::new_id("ask");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let wait = Duration::from_secs(self.cfg.threads.ask_wait_secs);
        let state = &self.runner.threads.route;
        let (slugs, others) = self.ask_over(live, &options);
        state.ask(
            &ask_id,
            Pending {
                home: home.to_string(),
                text: p.input.clone(),
                options: options.clone(),
                slugs,
                tx,
            },
        );
        let ask = ThreadAsk {
            ask_id: ask_id.clone(),
            session_id: home.to_string(),
            place: place.map(str::to_string),
            question: text,
            options: options.iter().map(|o| o.option.clone()).collect(),
            message: preview(&p.input),
            expires_ms: theseus_protocol::now_unix_ms() + wait.as_millis() as u64,
            others,
        };
        let sink = EventSink::new(
            self.bus.clone(),
            home,
            Some((conn.client.to_string(), conn.tx.clone())),
        );
        let rec = Rec {
            narrator: &self.narrator,
            session: Some(home),
            turn: None,
            to: To::Sink(&sink),
            store: &self.store,
        };
        rec.record(&ThreadAsked(&ask));
        row.ask = Some(ask_id.clone());
        let t0 = Instant::now();
        let answered = tokio::time::timeout(wait, rx).await;
        let waited_ms = t0.elapsed().as_millis() as u64;
        let (answer, key) = match answered {
            Ok(Ok(a)) => {
                // A slug answers for a thread the options did not list.
                let key = options
                    .iter()
                    .find(|o| o.choice == a.choice)
                    .map(|o| o.option.key.clone())
                    .or_else(|| match &a.choice {
                        Choice::Thread(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                (Some(a), key)
            }
            _ => {
                state.expire(&ask_id);
                (None, "expired".to_string())
            }
        };
        let tag = self.answer_tag(answer.as_ref(), live, p, (&new_slug, &new_title));
        let (who, via) = answer
            .as_ref()
            .map_or(("nobody".to_string(), "expired".to_string()), |a| {
                (a.who.label.clone(), a.who.via())
            });
        rec.record(&ThreadAnswered {
            ask: &ask_id,
            choice: &key,
            routed: tag.slug.as_deref(),
            who: &who,
            via: &via,
            waited_ms,
        });
        tag
    }

    /// What an ask is asked over (theseus-98bz.4): the slugs of the live,
    /// unsealed threads, any of which answers it, and, when it offers
    /// `other`, those threads as the options `other` stands for.
    fn ask_over(
        &self,
        live: &[&ThreadInfo],
        options: &[AskChoice],
    ) -> (Vec<String>, Vec<theseus_protocol::thread_route::AskOption>) {
        if !self.cfg.threads.ask_every_thread {
            return (Vec::new(), Vec::new());
        }
        let askable = crate::thread_route::askable(live);
        let others = match options.iter().any(|o| o.choice == Choice::Other) {
            true => askable
                .iter()
                .map(|t| theseus_protocol::thread_route::AskOption {
                    key: t.slug.clone(),
                    label: t.title.clone(),
                })
                .collect(),
            false => Vec::new(),
        };
        (askable.iter().map(|t| t.slug.clone()).collect(), others)
    }

    /// The tag an answer gives: the thread picked, a new one made on a tap, or
    /// home (asked, or unanswered and movable).
    fn answer_tag(
        &self,
        answer: Option<&Answered>,
        live: &[&ThreadInfo],
        p: &TurnSubmitParams,
        (new_slug, new_title): (&str, &str),
    ) -> ThreadTag {
        match answer {
            Some(Answered {
                choice: Choice::Thread(slug),
                ..
            }) => ThreadTag {
                slug: Some(slug.clone()),
                title: live
                    .iter()
                    .find(|t| &t.slug == slug)
                    .map_or_else(|| slug.clone(), |t| t.title.clone()),
                how: RouteHow::Asked,
                confidence: 1.0,
                movable: false,
            },
            Some(Answered {
                choice: Choice::New,
                who,
            }) => self.make_thread(
                new_slug,
                new_title,
                &p.input,
                p.session_id
                    .as_deref()
                    .and_then(|s| self.runner.target_of(s)),
                who,
            ),
            _ => ThreadTag {
                slug: None,
                title: "home".into(),
                how: if answer.is_some() {
                    RouteHow::Asked
                } else {
                    RouteHow::Home
                },
                confidence: 1.0,
                movable: answer.is_none(),
            },
        }
    }

    /// A thread made on a tap: the message's first words name it, the
    /// message is its goal. A refusal sends the message home.
    fn make_thread(
        &self,
        slug: &str,
        title: &str,
        text: &str,
        place: Option<String>,
        who: &Answerer,
    ) -> ThreadTag {
        let goal: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let goal: String = goal
            .chars()
            .take(theseus_protocol::threads::MAX_LINE_CHARS)
            .collect();
        let made = self.thread_new(
            &ThreadNewParams {
                slug: slug.to_string(),
                title: Some(title.to_string()),
                goal: Some(goal),
                place,
                ..ThreadNewParams::default()
            },
            who.clone(),
        );
        match made {
            Ok(r) => ThreadTag {
                slug: Some(r.thread.slug),
                title: r.thread.title,
                how: RouteHow::NewThread,
                confidence: 1.0,
                movable: false,
            },
            Err(e) => {
                tracing::warn!(error = %e.message, "a thread offered on a tap was not made: the message runs at home");
                ThreadTag {
                    slug: None,
                    title: "home".into(),
                    how: RouteHow::Home,
                    confidence: 1.0,
                    movable: true,
                }
            }
        }
    }

    /// After the routed turn ran: its handoff is gone, the thread's turn is
    /// known by its id (a reply to its reply finds the thread), and the place
    /// has heard a reply.
    pub(super) fn thread_routed_turn(&self, r: &Routing, res: Option<&TurnSubmitResult>) {
        let state = &self.runner.threads.route;
        state.clear_handoff(&r.session);
        self.tools.misroute.disarm(&r.session);
        let Some(res) = res else { return };
        self.claim_home_turn(r, res);
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

    /// `thread.answer`: the owner's word on a waiting question.
    pub(super) fn thread_answer(
        &self,
        p: theseus_protocol::thread_route::ThreadAnswerParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::thread_route::ThreadAnswerResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.judge_thread_answer(&who, &p.ask_id)?;
        let (_, choice) = self
            .runner
            .threads
            .route
            .answer(&p.ask_id, &p.choice, who)
            .map_err(|why| RpcFailure::new(error_code::NOT_FOUND, why))?;
        Ok(theseus_protocol::thread_route::ThreadAnswerResult {
            routed: match choice {
                Choice::Thread(s) => Some(s),
                _ => None,
            },
        })
    }
}
