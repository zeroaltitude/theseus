//! The live board and the cards' buttons in a place (theseus-2j2ka). A child
//! of `runtime`, apart from it for the shape budget's file ceiling.
//!
//! - **The board.** A place whose tasks changed (`PlaceMsg::Tasks`, from
//!   `tasks.rs`) draws its three parts (`Tasks::project`) as one V2 message and
//!   hands it to its lane under `render::BOARD_KEY` (`Buttons::V2`). The lane
//!   edits it, at most once in two seconds (`courier/v2.rs`). A place with
//!   nothing to show sends the empty board as an *idle* one: written only
//!   where a board is pinned already, so a restart cannot leave a stale
//!   "3 running" behind and no place gets a board for nothing.
//! - **The buttons.** `tb:` is the board's (Clear done, Quiet 1 h, the select,
//!   Answer ↗, and the Stop in a task's view), `wa:` a card's or a view's
//!   answer. Each press is the place's to handle (`PlaceMsg::Press`): an
//!   answer is `work.answer` as the presser, so the core judges it as it
//!   judges the CLI's; a refusal is told to the presser alone and the card
//!   keeps its buttons; a counted one is settled by the outbox's settle post
//!   on every surface that shows the card.
//! - **A task's view** is ephemeral: the presser's alone.

use serde_json::{json, Value};
use theseus_protocol::answer::{WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::work::{answer_options, WorkKind};
use theseus_protocol::LedgerKind;
use tokio::sync::mpsc;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::InteractionResponseType;

use super::{Place, PlaceMsg, Shared};
use crate::courier::LaneMsg;
use crate::render::board_v2::{self, Parts};
use crate::render::{card_v2, task_view, v2, Buttons, Op, BOARD_KEY};

/// A press on a board or a card, handed to the place it happened in.
pub(crate) struct TaskPress {
    i: Interaction,
    who: String,
    custom_id: String,
    values: Vec<String>,
}

impl Shared {
    /// Whether the board and the needs-you cards are Components V2: the
    /// config says so and this bot has not been refused one.
    pub(crate) fn v2_cards(&self) -> bool {
        self.core.cfg.discord.live_board && !self.tasks.v2_refused()
    }

    /// The bot cannot send V2: each place draws its text board instead.
    pub(crate) fn wake_places_for_board(&self) {
        let r = self.routes.lock().unwrap();
        for tx in r.by_session.values() {
            let _ = tx.send(PlaceMsg::Board);
        }
    }

    /// Answer an interaction with a V2 message: `kind` is 4 (a new message)
    /// or 7 (the pressed message, updated).
    async fn respond_v2(&self, i: &Interaction, kind: u8, comps: &[Value], ephemeral: bool) {
        let mut flags = v2::IS_COMPONENTS_V2;
        if ephemeral && kind == 4 {
            flags |= MessageFlags::EPHEMERAL.bits();
        }
        let body = json!({"type": kind, "data": {"flags": flags, "components": comps,
                          "allowed_mentions": {"parse": []}}});
        let req = Request::builder(&HttpRoute::InteractionCallback {
            interaction_id: i.id.get(),
            interaction_token: &i.token,
            with_response: false,
        })
        .use_authorization_token(false)
        .json(&body)
        .build();
        match req {
            Ok(req) => {
                if let Err(e) = self.http.request::<Value>(req).await {
                    self.board.error("interaction response", None, e);
                }
            }
            Err(e) => self.board.error("interaction response", None, e),
        }
    }

    /// The pressed message, edited (after a deferred update).
    async fn edit_original_v2(&self, i: &Interaction, comps: &[Value]) {
        let body = json!({"flags": v2::IS_COMPONENTS_V2, "components": comps,
                          "allowed_mentions": {"parse": []}});
        let req = Request::builder(&HttpRoute::UpdateInteractionOriginal {
            application_id: i.application_id.get(),
            interaction_token: &i.token,
        })
        .use_authorization_token(false)
        .json(&body)
        .build();
        match req {
            Ok(req) => {
                if let Err(e) = self.http.request::<Value>(req).await {
                    self.board.error("interaction edit", None, e);
                }
            }
            Err(e) => self.board.error("interaction edit", None, e),
        }
    }
}

/// A press on a board or a card: whether it was one, handed on to its place.
pub(super) fn work_interaction(
    i: &Interaction,
    place: &mpsc::UnboundedSender<PlaceMsg>,
    who: &str,
) -> bool {
    let Some(InteractionData::MessageComponent(c)) = &i.data else {
        return false;
    };
    if !c.custom_id.starts_with(board_v2::PREFIX) && !c.custom_id.starts_with(card_v2::PREFIX) {
        return false;
    }
    let _ = place.send(PlaceMsg::Press(Box::new(TaskPress {
        i: i.clone(),
        who: who.to_string(),
        custom_id: c.custom_id.clone(),
        values: c.values.clone(),
    })));
    true
}

impl Place {
    /// The boards' messages: a task record changed (the text board), the work
    /// engine's tasks changed (the live one), or a button was pressed.
    pub(super) async fn task_msg(&mut self, m: PlaceMsg) {
        match m {
            PlaceMsg::Press(p) => self.task_press(*p).await,
            PlaceMsg::Board => self.board(),
            _ => self.live_board(),
        }
    }

    /// Draw this place's tasks as the live board (`PlaceMsg::Tasks`).
    pub(super) fn live_board(&mut self) {
        if !self.shared.v2_cards() {
            return;
        }
        let p = self.shared.tasks.project(&self.session_id);
        let quiet = self.shared.tasks.quiet_until(&self.target);
        let comps = board_v2::render(&Parts {
            needs: &p.needs,
            working: &p.working,
            done: &p.done,
            quiet_until_ms: quiet,
        });
        let content = serde_json::to_string(&comps).unwrap_or_default();
        let _ = self.lane.send(if p.is_empty() {
            LaneMsg::BoardIdle(content)
        } else {
            LaneMsg::Live(Op::Upsert {
                key: BOARD_KEY.into(),
                content,
                buttons: Buttons::V2,
            })
        });
    }

    /// A press on the board or on a card (`PlaceMsg::Press`).
    pub(super) async fn task_press(&mut self, p: TaskPress) {
        let id = p.custom_id.clone();
        if let Some((q, option)) = card_v2::parse(&id) {
            return self.answer_press(&p, q, option).await;
        }
        let s = self.shared.clone();
        match id.as_str() {
            board_v2::CLEAR | board_v2::QUIET => {
                s.respond(
                    &p.i,
                    InteractionResponseType::DeferredUpdateMessage,
                    None,
                    false,
                )
                .await;
                if id == board_v2::CLEAR {
                    s.tasks.clear_done(&self.session_id);
                } else {
                    s.tasks.toggle_quiet(&self.target);
                }
                self.live_board();
            }
            board_v2::PICK => {
                if let Some(task) = p.values.first() {
                    self.open_task(&p, task).await;
                }
            }
            _ => {
                if let Some(task) = id.strip_prefix(board_v2::OPEN) {
                    self.open_task(&p, task).await;
                } else if let Some(task) = id.strip_prefix(task_view::STOP) {
                    // Stop asks once more.
                    if let Some(v) = s.tasks.view(task) {
                        s.respond_v2(&p.i, 7, &task_view::confirm_stop(&v), true)
                            .await;
                    }
                } else if let Some(task) = id.strip_prefix(task_view::STOP_YES) {
                    self.stop_task(&p, task).await;
                } else if let Some(task) = id.strip_prefix(task_view::STOP_NO) {
                    self.open_task_in_place(&p, task).await;
                }
            }
        }
    }

    /// A task's view, to the presser alone.
    async fn open_task(&self, p: &TaskPress, task: &str) {
        let s = &self.shared;
        let Some(v) = s.tasks.view(task) else {
            s.respond(
                &p.i,
                InteractionResponseType::ChannelMessageWithSource,
                Some("That task is not on the board any more.".into()),
                true,
            )
            .await;
            return;
        };
        s.respond_v2(&p.i, 4, &self.detail_of(&v), true).await;
    }

    /// The same view, in the message the presser's press came from.
    async fn open_task_in_place(&self, p: &TaskPress, task: &str) {
        if let Some(v) = self.shared.tasks.view(task) {
            self.shared
                .respond_v2(&p.i, 7, &self.detail_of(&v), true)
                .await;
        }
    }

    fn detail_of(&self, v: &theseus_protocol::work::WorkView) -> Vec<Value> {
        let tasks = &self.shared.tasks;
        let calls: Vec<String> = tasks
            .children_of(&v.id, WorkKind::Job)
            .into_iter()
            .map(|j| j.title)
            .collect();
        let q = tasks.question_of(&v.id);
        let options = q.as_ref().map(answer_options);
        task_view::detail(v, &calls, q.as_ref().zip(options.as_deref()))
    }

    /// Stop, sure: the same `task.cancel` as `/cancel`.
    async fn stop_task(&self, p: &TaskPress, task: &str) {
        let s = &self.shared;
        let target = s
            .tasks
            .view(task)
            .and_then(|v| v.session_id)
            .unwrap_or_else(|| task.to_string());
        let words = self
            .cancel_task(target, &p.who, "no wake of that name either")
            .await;
        s.respond(
            &p.i,
            InteractionResponseType::UpdateMessage,
            Some(words),
            true,
        )
        .await;
    }

    /// An answer pressed: `work.answer` as the presser.
    async fn answer_press(&self, p: &TaskPress, question: &str, option: &str) {
        let s = &self.shared;
        s.respond(
            &p.i,
            InteractionResponseType::DeferredUpdateMessage,
            None,
            false,
        )
        .await;
        let discord = super::origin_of(
            p.i.author().map(|u| u.id.get()),
            p.i.channel.as_ref().map(|c| c.id.get()),
            p.i.guild_id,
        );
        let r = s
            .rpc
            .call::<_, WorkAnswerResult>(
                theseus_protocol::method::WORK_ANSWER,
                WorkAnswerParams {
                    question_id: question.to_string(),
                    option: option.to_string(),
                    author: Some(p.who.clone()),
                    discord,
                    ..Default::default()
                },
            )
            .await;
        let approve = theseus_protocol::answer::approves(option);
        s.core.binding_ledger(
            LedgerKind::DiscordConfirm,
            None,
            json!({"correlation_id": question, "approve": approve, "option": option, "by": p.who,
                   "ok": r.is_ok(), "error": r.as_ref().err().map(|e| e.message.clone())}),
        );
        match r {
            Err(e) if e.code == theseus_protocol::error_code::REFUSED => {
                // The answer did not count: the card keeps its buttons, and
                // only the presser is told why.
                let why = e.data.get("why").and_then(Value::as_str);
                s.followup(
                    &p.i,
                    &format!(
                        "🔐 Your answer did not count: {}. It keeps waiting for an answer that \
                         does.",
                        why.unwrap_or(&e.message)
                    ),
                )
                .await;
            }
            Err(e) => s.followup(&p.i, &format!("⚠️ Could not answer: {e}")).await,
            Ok(done) => {
                // An outbox card is settled by its settle post, on every
                // surface that shows it; any other message (a task's view)
                // is settled here.
                if !s.core.outbox.has_card(&done.question_id) {
                    let word = if approve {
                        "✅ **Approved**"
                    } else {
                        "❎ **Declined**"
                    };
                    let line = format!(
                        "{word} by {} · `…{}`",
                        p.who,
                        board_v2::short(&done.question_id)
                    );
                    s.edit_original_v2(&p.i, &card_v2::closed(&line)).await;
                }
            }
        }
    }
}
