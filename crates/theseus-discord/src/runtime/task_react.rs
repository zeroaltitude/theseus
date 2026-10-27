//! A task's messages as one-tap controls (theseus-bh3t.2): the owner's
//! reaction on a message that carries a task acts on the task, from a private
//! place. ⏸ pauses it and ▶️ lets it go on (`task.pause`, `task.resume`), 🛑
//! asks once more (the board's own Stop question, posted as a message) and
//! stops it on Yes, 🔁 tries a failed or cancelled task again (`task.retry`;
//! a done one starts anew from its brief, `task.start`), 👀 follows it in the
//! owner's DM, and 📌 makes it a project (`project.create`, `project.attach`).
//! 👎 on a task that started itself says "not this" (`task.not_this`: it
//! cancels the task and keeps one rolling preference against work like it);
//! on any other task or message it is left to the next reader.
//! Every call carries the reaction's `DiscordOrigin`, so the core judges the
//! owner as it judges a press.
//!
//! - **Which messages carry a task**: any message in a task's thread or
//!   channel (`TaskThreads::made_in`), a Follow mirror (`Follows::task_of`),
//!   and a task's first message in its thread and its report line, which
//!   `TaskMsgs` keeps by message (bounded). The board is one message for every
//!   task and `/task` Details is ephemeral: those stay buttons.
//! - **Anyone else, an unknown emoji, a shared place**: nothing. A guest's tap
//!   on a control is a debug line; an emoji that is no control is left to the
//!   next reader (a reply's ⬆️ and ⬇️).
//! - **After an action**: ✅ on the reacted message; a refusal is one quiet
//!   line with the core's words.
//! - **Pre-added controls**: the bot adds ⏸ 🛑 👀 📌 (▶️ in place of ⏸ while
//!   paused) on a task's first message and its Follow mirror, in order, on the
//!   client's own rate limit and off every lane; the pair flips when a
//!   reaction pauses or resumes. A child of `runtime`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::projects::{
    ProjectAttachParams, ProjectAttachResult, ProjectCreateParams, ProjectCreateResult,
};
use theseus_protocol::task_not_this::{TaskNotThisParams, TaskNotThisResult};
use theseus_protocol::task_pause::{TaskPauseParams, TaskPauseResult};
use theseus_protocol::task_start::{TaskStartParams, TaskStartResult};
use theseus_protocol::work::{WorkState, WorkView};
use theseus_protocol::{method, DiscordOrigin, TaskRetryParams, TaskRetryResult};
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;
use twilight_model::channel::message::{AllowedMentions, EmojiReactionType, MessageFlags};
use twilight_model::gateway::GatewayReaction;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};
use twilight_model::id::Id;

use super::route::place_of;
use super::Shared;
use crate::render::{clip, task_view, v2};

/// The messages and keys `TaskMsgs` keeps.
const KEPT: usize = 256;

const PAUSE: &str = "\u{23f8}\u{fe0f}";
const RESUME: &str = "\u{25b6}\u{fe0f}";
const STOP: &str = "\u{1f6d1}";
const FOLLOW: &str = "\u{1f440}";
const PROJECT: &str = "\u{1f4cc}";
const RETRY: &str = "\u{1f501}";
const DONE: &str = "\u{2705}";
const NOT_THIS: &str = "\u{1f44e}";

/// What a control does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verb {
    Pause,
    Resume,
    Stop,
    Retry,
    Follow,
    Project,
    NotThis,
}

impl Verb {
    /// The control an emoji is; none for any other emoji.
    fn of(emoji: &EmojiReactionType) -> Option<Self> {
        let EmojiReactionType::Unicode { name } = emoji else {
            return None;
        };
        Some(match name.replace('\u{fe0f}', "").as_str() {
            "\u{23f8}" => Self::Pause,
            "\u{25b6}" => Self::Resume,
            STOP => Self::Stop,
            RETRY => Self::Retry,
            FOLLOW => Self::Follow,
            PROJECT => Self::Project,
            NOT_THIS => Self::NotThis,
            _ => return None,
        })
    }
}

/// The messages of a task that are not in its thread: its first message, its
/// report line, and where its controls were added.
#[derive(Default)]
pub(crate) struct TaskMsgs {
    /// `(message, task)`: a report line, or a message given controls.
    by_message: Mutex<VecDeque<(u64, String)>>,
    /// `(outbox key, task)`: a post whose message is to carry controls.
    expecting: Mutex<VecDeque<(String, String)>>,
    /// `(task, channel, message)`: the messages that carry the controls.
    controls: Mutex<VecDeque<(String, u64, u64)>>,
}

fn push<T>(q: &Mutex<VecDeque<T>>, v: T) {
    let mut q = q.lock().unwrap_or_else(|e| e.into_inner());
    q.push_back(v);
    while q.len() > KEPT {
        q.pop_front();
    }
}

impl TaskMsgs {
    /// The post under `key` is a task's first message: its controls go on it.
    pub(crate) fn expect(&self, key: String, task: &str) {
        push(&self.expecting, (key, task.to_string()));
    }

    /// A write landed as `message` under `key`. The task it gives controls to,
    /// if it is one that is to carry them.
    pub(crate) fn landed(&self, message: u64, key: &str) -> Option<String> {
        if let Some(task) = key.strip_prefix("report:") {
            push(&self.by_message, (message, task.to_string()));
            return None;
        }
        let mut e = self.expecting.lock().unwrap_or_else(|e| e.into_inner());
        let at = e.iter().position(|(k, _)| k == key)?;
        let (_, task) = e.remove(at)?;
        drop(e);
        push(&self.by_message, (message, task.clone()));
        Some(task)
    }

    fn task_of(&self, message: u64) -> Option<String> {
        let m = self.by_message.lock().unwrap_or_else(|e| e.into_inner());
        m.iter()
            .rev()
            .find(|(x, _)| *x == message)
            .map(|(_, t)| t.clone())
    }

    fn carries(&self, task: &str, channel: u64, message: u64) {
        push(&self.controls, (task.to_string(), channel, message));
    }

    fn carriers(&self, task: &str) -> Vec<(u64, u64)> {
        let c = self.controls.lock().unwrap_or_else(|e| e.into_inner());
        c.iter()
            .filter(|(t, _, _)| t == task)
            .map(|(_, c, m)| (*c, *m))
            .collect()
    }
}

/// The controls a task shows: none once it ended.
fn controls(v: &WorkView, paused: bool) -> [&'static str; 4] {
    [if paused { RESUME } else { PAUSE }, STOP, FOLLOW, PROJECT].map(|e| {
        if v.state.ended() {
            ""
        } else {
            e
        }
    })
}

fn unicode(name: &str) -> RequestReactionType<'_> {
    RequestReactionType::Unicode { name }
}

impl Shared {
    /// A write landed: a reply is kept for a reaction to find, and a task's
    /// first message is given its controls.
    pub(crate) fn noted(self: &Arc<Self>, message: u64, channel: u64, key: &str) {
        self.replies.noted(message, key);
        if let Some(task) = self.replies.tasks().landed(message, key) {
            // A shared place has no use for them: nobody's tap would act.
            if self.is_private(&format!("discord:channel:{channel}")) {
                self.add_controls(channel, message, task);
            }
        }
    }

    /// Give `message` in `channel` the controls of `task`, one request each,
    /// in order, off the caller's path.
    pub(crate) fn add_controls(self: &Arc<Self>, channel: u64, message: u64, task: String) {
        self.replies.tasks().carries(&task, channel, message);
        let s = self.clone();
        tokio::spawn(async move {
            let Some(v) = s.tasks.view(&task) else { return };
            let paused = v.paused;
            for e in controls(&v, paused).into_iter().filter(|e| !e.is_empty()) {
                s.react(channel, message, e).await;
            }
        });
    }

    async fn react(&self, channel: u64, message: u64, emoji: &str) {
        if let Err(e) = self
            .http
            .create_reaction(Id::new(channel), Id::new(message), &unicode(emoji))
            .await
        {
            self.board.error("task control", None, e);
        }
    }

    /// The task a message carries.
    fn task_of_message(&self, channel: u64, message: u64) -> Option<String> {
        self.task_threads
            .made_in(channel)
            .map(|m| m.task)
            .or_else(|| self.cmds.follows.task_of(message))
            .or_else(|| self.replies.tasks().task_of(message))
    }

    /// A reaction on a task's message: whether it was one of the controls
    /// (acted on if the owner's, from a private place, and left alone
    /// otherwise). False for any other emoji or message, which another reader
    /// takes.
    pub(crate) async fn task_reaction(self: &Arc<Self>, r: &GatewayReaction) -> bool {
        let Some(verb) = Verb::of(&r.emoji) else {
            return false;
        };
        let (channel, message) = (r.channel_id.get(), r.message_id.get());
        let Some(task) = self.task_of_message(channel, message) else {
            return false;
        };
        // 👎 is a control only on a task that started itself: elsewhere it is
        // a rating, which the next reader takes.
        if verb == Verb::NotThis && !self.started_itself(&task) {
            return false;
        }
        let owner = self
            .core
            .runner
            .place_rule
            .owners(&self.core.cfg)
            .contains(&format!("discord:{}", r.user_id));
        if !owner || !self.is_private(&place_of(r)) {
            tracing::debug!(user = %r.user_id, channel, ?verb, "a task control from a guest or a shared place: nothing");
            return true;
        }
        let Some(v) = self.tasks.view(&task) else {
            return true;
        };
        let origin = DiscordOrigin {
            user_id: r.user_id.to_string(),
            channel_id: channel.to_string(),
            guild_id: r.guild_id.map(|g| g.to_string()),
        };
        let by = format!("discord:{}", r.user_id);
        let done = match verb {
            Verb::Pause | Verb::Resume => self.hold(&v, verb == Verb::Pause, &by, origin).await,
            Verb::Stop => self.ask_stop(channel, message, &v).await,
            Verb::Retry => self.again(&v, &by, origin).await,
            Verb::Follow => self.follow_in_dm(&v).await,
            Verb::Project => self.make_project(&v, &by, origin).await,
            Verb::NotThis => self.not_this(&v, &by, origin).await,
        };
        match done {
            Ok(true) => self.react(channel, message, DONE).await,
            Ok(false) => {}
            Err(words) => self.quiet(channel, message, &words).await,
        }
        true
    }

    /// One quiet line, a reply to the message reacted to.
    async fn quiet(&self, channel: u64, message: u64, text: &str) {
        let none = AllowedMentions::default();
        let (c, m): (Id<ChannelMarker>, Id<MessageMarker>) = (Id::new(channel), Id::new(message));
        let sent = self
            .http
            .create_message(c)
            .content(&clip(text, 1900))
            .reply(m)
            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS)
            .allowed_mentions(Some(&none))
            .await;
        if let Err(e) = sent {
            self.board.error("task control", None, e);
        }
    }

    /// ⏸ or ▶️.
    async fn hold(
        &self,
        v: &WorkView,
        pause: bool,
        by: &str,
        discord: DiscordOrigin,
    ) -> Result<bool, String> {
        let m = if pause {
            method::TASK_PAUSE
        } else {
            method::TASK_RESUME
        };
        let p = TaskPauseParams {
            task: v.session_id.clone().unwrap_or_else(|| v.id.clone()),
            author: Some(by.to_string()),
            discord: Some(discord),
        };
        let r: TaskPauseResult = self.rpc.call(m, p).await.map_err(|e| format!("⚠️ {e}"))?;
        if r.changed {
            self.flip_controls(&v.id, pause).await;
        }
        Ok(true)
    }

    /// The pair on every message that carries the task's controls: ▶️ while
    /// it is paused, ⏸ while it works.
    async fn flip_controls(&self, task: &str, paused: bool) {
        let (gone, come) = if paused {
            (PAUSE, RESUME)
        } else {
            (RESUME, PAUSE)
        };
        for (channel, message) in self.replies.tasks().carriers(task) {
            let (c, m) = (Id::new(channel), Id::new(message));
            let _ = self
                .http
                .delete_current_user_reaction(c, m, &unicode(gone))
                .await;
            self.react(channel, message, come).await;
        }
    }

    /// Whether the task named by its work id started itself
    /// (`TaskOrigin.self_started`).
    fn started_itself(&self, task: &str) -> bool {
        let Some(session) = self.tasks.view(task).and_then(|v| v.session_id) else {
            return false;
        };
        let id = theseus_core::task_graph::of_session(&session);
        theseus_core::task_graph::all(&self.core.store)
            .ok()
            .is_some_and(|all| all.iter().any(|t| t.id == id && t.origin.self_started))
    }

    /// 👎: the task this turn started, told "not this".
    async fn not_this(
        &self,
        v: &WorkView,
        by: &str,
        discord: DiscordOrigin,
    ) -> Result<bool, String> {
        let p = TaskNotThisParams {
            task: v.session_id.clone().unwrap_or_else(|| v.id.clone()),
            reason: None,
            author: Some(by.to_string()),
            discord: Some(discord),
        };
        let _: TaskNotThisResult = self
            .rpc
            .call(method::TASK_NOT_THIS, p)
            .await
            .map_err(|e| format!("⚠️ {e}"))?;
        Ok(true)
    }

    /// 🛑: the board's own Stop question, as a message in the place; its Yes
    /// and No are the buttons the board's view has.
    async fn ask_stop(&self, channel: u64, message: u64, v: &WorkView) -> Result<bool, String> {
        if v.state.ended() {
            return Err(format!(
                "**{}** has ended already.",
                clip(v.title.trim(), 80)
            ));
        }
        let body = json!({"flags": v2::IS_COMPONENTS_V2 | MessageFlags::SUPPRESS_NOTIFICATIONS.bits(),
            "components": task_view::confirm_stop(v), "allowed_mentions": {"parse": []},
            "message_reference": {"message_id": message.to_string(), "fail_if_not_exists": false}});
        let req = Request::builder(&HttpRoute::CreateMessage {
            channel_id: channel,
        })
        .json(&body)
        .build()
        .map_err(|e| format!("⚠️ {e}"))?;
        self.http
            .request::<Value>(req)
            .await
            .map_err(|e| format!("⚠️ {e}"))?;
        // The question is the answer for now: ✅ waits for the Yes.
        Ok(false)
    }

    /// 🔁: a failed task is tried again, linked to the failed one
    /// (`task.retry`); a cancelled or done one, which the core does not retry,
    /// is begun again from its brief as a task of its own.
    async fn again(&self, v: &WorkView, by: &str, discord: DiscordOrigin) -> Result<bool, String> {
        if !matches!(
            v.state,
            WorkState::Failed | WorkState::Cancelled | WorkState::Done
        ) {
            return Ok(false);
        }
        let p = TaskRetryParams {
            execution_id: v.id.clone(),
            note: None,
            discord: Some(discord.clone()),
        };
        match self
            .rpc
            .call::<_, TaskRetryResult>(method::TASK_RETRY, p)
            .await
        {
            Ok(_) => Ok(true),
            Err(_) if v.state != WorkState::Failed => self.start_again(v, by, discord).await,
            Err(e) => Err(format!("⚠️ {e}")),
        }
    }

    /// A done task, begun again from its brief as a task of its own.
    async fn start_again(
        &self,
        v: &WorkView,
        by: &str,
        discord: DiscordOrigin,
    ) -> Result<bool, String> {
        let brief = self
            .brief_of(v)
            .ok_or("⚠️ That task has no brief to start again from.")?;
        let p = TaskStartParams {
            brief,
            title: Some(v.title.clone()),
            author: Some(by.to_string()),
            discord: Some(discord),
            ..TaskStartParams::default()
        };
        let _: TaskStartResult = self
            .rpc
            .call(method::TASK_START, p)
            .await
            .map_err(|e| format!("⚠️ {e}"))?;
        Ok(true)
    }

    /// The words a task was given: its brief, without the harness's header.
    fn brief_of(&self, v: &WorkView) -> Option<String> {
        let node = theseus_core::task::brief(&self.core.store, v.session_id.as_deref()?).ok()??;
        match node.body {
            theseus_core::node::Body::UserMessage { text, .. } => Some(
                text.split_once("\n\n")
                    .map_or(text.as_str(), |(_, b)| b)
                    .to_string(),
            ),
            _ => None,
        }
    }

    /// 👀: a mirror of the task in the owner's DM.
    async fn follow_in_dm(self: &Arc<Self>, v: &WorkView) -> Result<bool, String> {
        self.open_dm_channels()
            .await
            .map_err(|_| "⚠️ Your DM could not be opened.".to_string())?;
        let Some((user, _)) = self.approval_dm(None) else {
            return Err("⚠️ No DM with you is bound to follow it in.".into());
        };
        let target = format!("discord:dm:{user}");
        let channel = self
            .dm_channel(user)
            .await
            .map_err(|_| "⚠️ Your DM could not be opened.".to_string())?;
        self.follow(&target, channel, v)?;
        Ok(true)
    }

    /// 📌: a project of the task's brief, the task's conversation attached.
    async fn make_project(
        &self,
        v: &WorkView,
        by: &str,
        discord: DiscordOrigin,
    ) -> Result<bool, String> {
        let Some(session) = v.session_id.clone() else {
            return Err("⚠️ That task has no conversation to attach.".into());
        };
        if let Ok(Some(rec)) = self
            .core
            .store
            .get_session::<theseus_core::session::SessionRecord>(&session)
        {
            if let Some(p) = rec.project {
                return Err(format!(
                    "📌 It is in project `…{}` already.",
                    crate::render::board_v2::short(&p)
                ));
            }
        }
        let attach_from = discord.clone();
        let made: ProjectCreateResult = self
            .rpc
            .call(
                method::PROJECT_CREATE,
                ProjectCreateParams {
                    title: v.title.clone(),
                    objective: self.brief_of(v),
                    author: Some(by.to_string()),
                    discord: Some(discord),
                    ..ProjectCreateParams::default()
                },
            )
            .await
            .map_err(|e| format!("⚠️ {e}"))?;
        let _: ProjectAttachResult = self
            .rpc
            .call(
                method::PROJECT_ATTACH,
                ProjectAttachParams {
                    session,
                    project: Some(made.project.id.clone()),
                    author: Some(by.to_string()),
                    discord: Some(attach_from),
                },
            )
            .await
            .map_err(|e| format!("⚠️ {e}"))?;
        Ok(true)
    }
}
