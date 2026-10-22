//! `thread.move` (theseus-rha2): "that was for the security fix". The owner
//! says an unaddressed message was routed wrong; it is run again where they
//! say. In this order, each step standing if a later one fails:
//!
//! 1. the wrong thread (when it was a thread) is told, as a harness note in
//!    its session: "the message at @p was for «security fix»; disregard it
//!    here". A turn still running there for the message is stopped first;
//! 2. the correction is recorded in one frame: the owner's label on the
//!    `thread` question of the judgment that chose (the right class), the
//!    `thread.moved` row, and, in memory, an entry in the correction layer
//!    (`Steer::Thread`), so a close next message routes where the owner said.
//!    A "wrong thread" label counts on `classify.v2`'s rollback rule;
//! 3. the message is submitted again, once, to the right session, the routing
//!    of unaddressed messages off.
//!
//! The owner's act, from a private place, judged as every move of a thread is.

use anyhow::anyhow;
use serde_json::json;
use theseus_protocol::thread_route::{RouteHow, ThreadMoveParams, ThreadMoveResult, ThreadTag};
use theseus_protocol::{error_code, method, TurnSubmitParams, TurnSubmitResult};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::correction::layer::{words, Entry, Steer};
use crate::fact;
use crate::fact::thread_route::ThreadMoved;
use crate::judge::inbound::{CLASSIFY_PACK, THREAD_QUESTION};
use crate::node::{Body, Node, Origin};
use crate::thread_flow::Routed;

/// The author of the note left in the wrong thread.
const NOTE_AUTHOR: &str = "thread";

impl Core {
    /// `thread.move`.
    pub(super) async fn thread_move_message(
        &self,
        p: ThreadMoveParams,
        conn: Conn<'_>,
    ) -> Result<ThreadMoveResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.judge_thread(&who, method::THREAD_MOVE, &p.to)?;
        Ok(self.thread_move_run(p, who, conn).await?.0)
    }

    /// "That was for the security fix", typed as a message in a private place
    /// with a message the router placed: the move, and the turn it ran again
    /// as. The place is the judge here (the board offers a shared place no
    /// thread), as it is for the router's own choice.
    pub(super) async fn moved_by_words(
        &self,
        p: &TurnSubmitParams,
        conn: Conn<'_>,
    ) -> Result<Option<TurnSubmitResult>, RpcFailure> {
        let Some(session) = p
            .session_id
            .as_deref()
            .filter(|_| self.words_may_move(p, conn))
        else {
            return Ok(None);
        };
        let board = self.threads();
        let live = board.live(self.runner.class_of(session));
        let Some(to) = crate::thread_route::moved_words(&p.input, &live) else {
            return Ok(None);
        };
        if self
            .runner
            .threads
            .route
            .find_routed(session, None)
            .is_none()
        {
            return Ok(None);
        }
        let who = conn.answerer(p.author.clone(), None);
        let mp = ThreadMoveParams {
            session_id: session.to_string(),
            to,
            turn_id: None,
            author: p.author.clone(),
            discord: None,
        };
        Ok(Some(self.thread_move_run(mp, who, conn).await?.1))
    }

    /// The session a moved message goes to, opened if it is a thread's first,
    /// and the title it shows under.
    fn move_target(&self, home: &str, to: &str) -> Result<(String, String), RpcFailure> {
        match to {
            "home" => Ok((home.to_string(), "home".to_string())),
            slug => {
                let s = self.thread_session_open(slug)?;
                let title = self
                    .thread_named(slug)
                    .map(|t| t.title)
                    .unwrap_or_else(|_| slug.to_string());
                Ok((s.session_id, title))
            }
        }
    }

    /// The move itself: `who` was judged by the caller.
    async fn thread_move_run(
        &self,
        p: ThreadMoveParams,
        who: crate::approval::Answerer,
        conn: Conn<'_>,
    ) -> Result<(ThreadMoveResult, TurnSubmitResult), RpcFailure> {
        self.threads_on()?;
        let state = &self.runner.threads.route;
        let routed = state
            .find_routed(&p.session_id, p.turn_id.as_deref())
            .ok_or_else(|| {
                RpcFailure::new(
                    error_code::NOT_FOUND,
                    "no message routed here since the daemon started: only a message that named no \
                     thread can be moved",
                )
            })?;
        let from = routed.to.clone().unwrap_or_else(|| "home".into());
        let to = p.to.trim().to_string();
        if !fact::thread_route::is_target(&to) {
            return Err(RpcFailure::invalid(anyhow!(
                "{to:?} is neither a thread's slug nor `home`"
            )));
        }
        if to == from {
            return Err(RpcFailure::invalid(anyhow!(
                "that message already ran in {from}"
            )));
        }
        let (target_session, title) = self.move_target(&routed.home, &to)?;
        let via = who.via();
        let who_label = who.who().to_string();

        // 1. The wrong thread is told.
        let redirected = match &routed.to {
            Some(_) => self.redirect_note(&routed, &title).await,
            None => false,
        };

        // 2. The correction, in one frame.
        let rcx = crate::new_id("rcx");
        let label_id = self.write_correction(&routed, &to, &rcx, &who_label, &via, &title)?;

        // 3. The message again, once, to the right session.
        let place = self.outbox.target(&routed.home);
        if to != "home" {
            if let Err(e) = self.outbox.show_at(&target_session, place.as_deref()) {
                tracing::warn!(error = %format!("{e:#}"), "a thread's reply place was not written");
            }
        }
        state.arrived(
            &routed.home,
            (to != "home").then_some(to.as_str()),
            theseus_protocol::now_unix_ms(),
        );
        let submit = resubmission(&target_session, &routed);
        // Boxed: a turn's words can be a move, which runs a turn.
        let mut result = Box::pin(self.turn_submit(submit, conn, false)).await?;
        let tag = ThreadTag {
            slug: (to != "home").then(|| to.clone()),
            title: title.clone(),
            how: RouteHow::Moved,
            confidence: 1.0,
            movable: false,
        };
        result.thread = Some(tag);
        state.moved(&routed.turn);
        if to != "home" {
            state.turn_ran(&result.turn_id, &to);
        }
        state.routed(Routed {
            turn: result.turn_id.clone(),
            home: routed.home.clone(),
            to: (to != "home").then(|| to.clone()),
            session: target_session,
            text: routed.text.clone(),
            author: routed.author.clone(),
            how: RouteHow::Moved,
            judgment: None,
            at_ms: theseus_protocol::now_unix_ms(),
        });
        state.replied(&routed.home, theseus_protocol::now_unix_ms());
        self.session_rec(&routed.home).record(&ThreadMoved {
            turn: &routed.turn,
            from: &from,
            to: &to,
            who: &who_label,
            via: &via,
            resubmitted: &result.turn_id,
            redirected,
            labeled: label_id.as_deref(),
        });
        Ok((
            ThreadMoveResult {
                from,
                to,
                turn_id: result.turn_id.clone(),
                redirected,
            },
            result,
        ))
    }

    /// The wrong thread's session is told to disregard the message. A turn
    /// still running there is stopped first. False when the thread has no
    /// session to tell.
    async fn redirect_note(&self, r: &Routed, right: &str) -> bool {
        let Ok(Some(rec)) = self
            .store
            .get_session::<crate::session::SessionRecord>(&r.session)
        else {
            return false;
        };
        let Some(exec) = rec.execution_id.clone() else {
            return false;
        };
        let at = self
            .store
            .session_nodes(&r.session)
            .ok()
            .and_then(|nodes| {
                nodes.into_iter().find_map(|(pos, n)| {
                    (n.turn_id.as_deref() == Some(r.turn.as_str())
                        && matches!(n.body, Body::UserMessage { .. }))
                    .then_some(pos)
                })
            })
            .map_or_else(|| "the last message".to_string(), |pos| format!("@{pos}"));
        let text = format!(
            "The message at {at} was for \u{ab}{right}\u{bb}; disregard it here. It was moved there \
             at the owner's word."
        );
        let node = Node::relayed(&r.session, None, Origin::Harness, NOTE_AUTHOR, &text);
        for attempt in 0..2 {
            let wrote = self.kernel.frame(&[&exec], |k| {
                if k.holds_turn(&exec) {
                    anyhow::bail!("a turn is running in {}", r.session);
                }
                k.stage(&[node.record()?])?;
                Ok(())
            });
            match wrote {
                Ok(()) => return true,
                Err(e) if attempt == 0 => {
                    tracing::info!(error = %format!("{e:#}"), "stopping the wrong thread's turn to tell it");
                    if self
                        .stop_execution(&exec, "the owner's move")
                        .await
                        .is_err()
                    {
                        return false;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "the wrong thread was not told");
                }
            }
        }
        false
    }

    /// The label on the `thread` question (the right class), when Jev was
    /// asked, and the layer's entry, in memory; the `thread.moved` row is the
    /// caller's. One frame for the label; the ladder counts it.
    fn write_correction(
        &self,
        r: &Routed,
        to: &str,
        rcx: &str,
        who: &str,
        via: &str,
        title: &str,
    ) -> Result<Option<String>, RpcFailure> {
        let judge = &self.runner.judge;
        let mut label_id = None;
        if let Some((judgment, offered)) = &r.judgment {
            let pack = judge
                .pack(CLASSIFY_PACK)
                .ok_or_else(|| RpcFailure::invalid(anyhow!("this build has no {CLASSIFY_PACK}")))?;
            // A class the question did not offer is no label of it: `home`
            // and the slugs on offer are.
            if to == "home" || offered.iter().any(|o| o == to) {
                let label =
                    crate::learning::labels::check(&pack, Some(THREAD_QUESTION), &json!(to))
                        .map_err(|e| RpcFailure::invalid(anyhow!(e)))?;
                let id = crate::new_id("lbl");
                let note = format!(
                    "the owner moved the message to {title} ({via}): the thread question's answer was wrong"
                );
                let f = fact::judge::JudgeLabel {
                    id: &id,
                    judgment,
                    pack: CLASSIFY_PACK,
                    question: Some(THREAD_QUESTION),
                    about: None,
                    label,
                    source: "operator",
                    who,
                    via,
                    weight: 1.0,
                    note: &note,
                    correlation_id: None,
                    rule: None,
                };
                let mut rec =
                    fact::row(&f, Some(&r.home), Some(&r.turn)).map_err(RpcFailure::invalid)?;
                rec.key = Some(id.clone());
                let rec = rec.scoped(&super::judge::scope_of(CLASSIFY_PACK));
                self.store.append(&[rec]).map_err(RpcFailure::invalid)?;
                self.session_rec(&r.home).announce(&f);
                label_id = Some(id);
            }
        }
        judge.wrong_thread();
        if to != "home" {
            let w = words(&r.text);
            if !w.is_empty() {
                judge.corrections.add(
                    Entry {
                        id: rcx.to_string(),
                        label: label_id.clone(),
                        pack: CLASSIFY_PACK.to_string(),
                        session: r.home.clone(),
                        turn: r.turn.clone(),
                        words: w,
                        steer: Steer::Thread(to.to_string()),
                        at_ms: theseus_protocol::now_unix_ms(),
                    },
                    &self.cfg.routing.corrections,
                );
            }
        }
        Ok(label_id)
    }
}

/// The message again, as its author sent it, to the session it moved to.
fn resubmission(session: &str, routed: &Routed) -> TurnSubmitParams {
    TurnSubmitParams {
        session_id: Some(session.to_string()),
        input: routed.text.clone(),
        profile: None,
        provider: None,
        model: None,
        carried: false,
        author: Some(routed.author.clone()),
        attachments: Vec::new(),
        reply_to: None,
        opened_from: None,
        prompt: None,
        dir: None,
        thread: None,
        speaking: None,
        in_reply_to: None,
        route_at_ms: None,
        via: None,
        surface: None,
    }
}
