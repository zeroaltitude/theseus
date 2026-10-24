//! The task commands (theseus-a5uds): `/tasks`, `/task`, `/answer`, `/quiet`
//! and the message command "Make this a task", with autocomplete for `/task`,
//! `/answer` and `/cancel`. A child of `runtime`, apart from it for the shape
//! budget's file ceiling.
//!
//! - **Autocomplete is answered from memory** (`Tasks`, the task model, for
//!   `/task` and `/cancel`; a presser's question list at most every
//!   [`answer_cmd::TTL`] for `/answer`): no daemon call for a keystroke, and
//!   the answer goes out on the interaction client, which has no limiter.
//! - **A command is the place's** (`PlaceMsg::Cmd`) when it needs the place's
//!   session or target; the answer is ephemeral (the presser's alone) or an
//!   edit, so none of it adds noise.
//! - Everything that changes something is judged by the core as the presser
//!   (`DiscordOrigin`): a refusal is told to the presser alone.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use tokio::sync::mpsc;
use twilight_model::application::command::{
    Command, CommandOptionChoice, CommandOptionChoiceValue, CommandType,
};
use twilight_model::application::interaction::application_command::{
    CommandData, CommandOptionValue,
};
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::http::interaction::{InteractionResponseData, InteractionResponseType};
use twilight_model::id::marker::UserMarker;
use twilight_model::id::Id;
use twilight_util::builder::command::{CommandBuilder, StringBuilder, UserBuilder};

use super::war_room::Guest;
use super::{answer_cmd, follow, start_task};
use super::{Place, PlaceMsg, Shared};
use crate::render::snapshot::{self, Parts, Show};

/// What the task commands keep between interactions.
#[derive(Default)]
pub(crate) struct Cmds {
    pub(super) follows: follow::Follows,
    pub(super) questions: answer_cmd::Questions,
}

/// The message command's name (Discord shows it under Apps).
pub(super) const MAKE_TASK: &str = "Make this a task";

/// A command or a modal, handed to the place it happened in.
pub(crate) struct TaskCmd {
    pub(super) i: Interaction,
    pub(super) who: String,
    pub(super) what: What,
}

pub(super) enum What {
    Tasks(Show),
    Task(String),
    /// `share:` or `unshare:` (None: both were given).
    Share(String, Option<Guest>),
    Quiet(Option<String>),
    Answer(Option<String>),
    NewTask(Option<String>),
    NewTaskSubmit(start_task::Submit),
}

/// `/tasks [show:]`.
pub(super) fn tasks_command() -> Command {
    CommandBuilder::new(
        "tasks",
        "This place's background tasks: state, spend, and what each waits on",
        CommandType::ChatInput,
    )
    .option(
        StringBuilder::new("show", "Which tasks (default: open)")
            .required(false)
            .choices([
                ("Open: needs you, working, done since you looked", "open"),
                ("Needs you", "needs_you"),
                ("Working", "working"),
                ("Done, from the record", "done"),
                ("All, with done from the record", "all"),
            ]),
    )
    .build()
}

/// The commands this row adds to the place's list.
pub(super) fn commands() -> Vec<Command> {
    vec![
        CommandBuilder::new(
            "task",
            "Open a task: its details, Stop, Follow. Or start one with new",
            CommandType::ChatInput,
        )
        .option(
            StringBuilder::new("name", "The task: its title or short id, or new")
                .required(true)
                .autocomplete(true),
        )
        .option(
            UserBuilder::new(
                "share",
                "Bring a guest in to watch and comment (owner only)",
            )
            .required(false),
        )
        .option(UserBuilder::new("unshare", "Take a guest out").required(false))
        .build(),
        CommandBuilder::new(
            "answer",
            "Answer a question that is waiting for you: buttons, a choice, a note, a file",
            CommandType::ChatInput,
        )
        .option(
            StringBuilder::new("question", "The question (default: the one waiting)")
                .required(false)
                .autocomplete(true),
        )
        .build(),
        CommandBuilder::new(
            "quiet",
            "Silence this place's task pings for a while",
            CommandType::ChatInput,
        )
        .option(
            StringBuilder::new("for", "How long (default: 1h)")
                .required(false)
                .choices([
                    ("30 minutes", "30m"),
                    ("1 hour", "1h"),
                    ("Until morning", "morning"),
                    ("End the quiet", "off"),
                ]),
        )
        .build(),
        CommandBuilder::new(MAKE_TASK, "", CommandType::Message).build(),
    ]
}

/// A string option's value.
fn opt(c: &CommandData, name: &str) -> Option<String> {
    c.options.iter().find_map(|o| match (&*o.name, &o.value) {
        (n, CommandOptionValue::String(s)) if n == name => Some(s.clone()),
        _ => None,
    })
}

/// A user option's id.
fn user_opt(c: &CommandData, name: &str) -> Option<Id<UserMarker>> {
    c.options.iter().find_map(|o| match (&*o.name, &o.value) {
        (n, CommandOptionValue::User(u)) if n == name => Some(*u),
        _ => None,
    })
}

/// What an autocomplete has typed so far, whichever option has the focus.
fn focused(c: &CommandData) -> String {
    c.options
        .iter()
        .find_map(|o| match &o.value {
            CommandOptionValue::Focused(t, _) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Autocomplete choices as Discord wants them.
pub(super) fn choice_list(pairs: Vec<(String, String)>) -> Vec<CommandOptionChoice> {
    pairs
        .into_iter()
        .map(|(name, value)| CommandOptionChoice {
            name: crate::render::clip(&name, 100),
            name_localizations: None,
            value: CommandOptionChoiceValue::String(value),
        })
        .collect()
}

/// A span of `/quiet for:` from `now_ms`, if `text` is one.
fn quiet_until_span(text: &str, now_ms: u64) -> Option<u64> {
    match text {
        "30m" => Some(now_ms + 30 * 60_000),
        "1h" => Some(now_ms + 3_600_000),
        _ => None,
    }
}

/// When `/quiet for:` ends, from `now_ms`: a span, or the next 8:00.
fn quiet_until(shared: &Shared, text: &str, now_ms: u64) -> Result<u64, String> {
    if let Some(t) = quiet_until_span(text, now_ms) {
        return Ok(t);
    }
    match text {
        "morning" => {
            let kc = shared.core.kernel.config();
            theseus_core::wake_when::parse_when("8:00", now_ms, &kc.zone, kc.wakes.max_ahead_ms)
        }
        other => Err(format!("`{other}` is not a time: say 30m, 1h or morning")),
    }
}

/// The META key of a place's quiet (the one thing `/quiet` stores).
fn quiet_key(target: &str) -> String {
    format!("discord.quiet:{target}")
}

impl Shared {
    /// An autocomplete of one of the task commands, a command of the place,
    /// or a modal of the task commands: true when it was one, handed on or
    /// answered.
    pub(super) async fn task_cmd_interaction(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: &str,
    ) -> bool {
        let who = who.to_string();
        let send = |what: What| {
            let _ = tx.send(PlaceMsg::Cmd(Box::new(TaskCmd {
                i: i.clone(),
                who: who.clone(),
                what,
            })));
            true
        };
        match &i.data {
            Some(InteractionData::ApplicationCommand(c))
                if i.kind == InteractionType::ApplicationCommandAutocomplete =>
            {
                match c.name.as_str() {
                    "task" => self.task_choices(i, tx, &focused(c)).await,
                    "cancel" => self.cancel_choices(i, tx, &focused(c)).await,
                    "answer" => self.answer_choices(i, &focused(c)).await,
                    _ => return false,
                }
                true
            }
            Some(InteractionData::ApplicationCommand(c)) => match c.name.as_str() {
                "tasks" if self.v2_cards() => send(What::Tasks(
                    opt(c, "show")
                        .and_then(|s| Show::parse(&s))
                        .unwrap_or_default(),
                )),
                "task" => {
                    let name = opt(c, "name").unwrap_or_default();
                    match (user_opt(c, "share"), user_opt(c, "unshare")) {
                        (None, None) => send(What::Task(name)),
                        (Some(u), None) => send(What::Share(name, Some(Guest::Add(u)))),
                        (None, Some(u)) => send(What::Share(name, Some(Guest::Remove(u)))),
                        _ => send(What::Share(name, None)),
                    }
                }
                "quiet" => send(What::Quiet(opt(c, "for"))),
                "answer" => send(What::Answer(opt(c, "question"))),
                MAKE_TASK => send(What::NewTask(Some(
                    c.resolved
                        .as_ref()
                        .and_then(|r| r.messages.values().next())
                        .map(|m| m.content.clone())
                        .unwrap_or_default(),
                ))),
                _ => false,
            },
            Some(InteractionData::ModalSubmit(m)) if m.custom_id.starts_with(start_task::MODAL) => {
                send(What::NewTaskSubmit(start_task::Submit::of(m)))
            }
            Some(InteractionData::ModalSubmit(m))
                if m.custom_id.starts_with(super::persona_soul::MODAL) =>
            {
                self.persona_soul_modal(i, m, &who).await;
                true
            }
            Some(InteractionData::ModalSubmit(m))
                if m.custom_id.starts_with(crate::render::answer_v2::MODAL) =>
            {
                self.answer_modal(i, m, &who).await;
                true
            }
            _ => false,
        }
    }

    /// The place's session, found from its mailbox.
    pub(super) fn session_of(&self, tx: &mpsc::UnboundedSender<PlaceMsg>) -> Option<String> {
        let r = self.routes.lock().unwrap();
        r.by_session
            .iter()
            .find(|(s, t)| !s.is_empty() && t.same_channel(tx))
            .map(|(s, _)| s.clone())
    }

    /// `/task name:` as it is typed: the place's tasks, needs-you first.
    async fn task_choices(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        typed: &str,
    ) {
        let views = self
            .session_of(tx)
            .map(|s| self.tasks.in_place(&s))
            .unwrap_or_default();
        self.autocomplete(i, snapshot::task_choices(&views, typed))
            .await;
    }

    /// `/cancel id:` as it is typed: the place's tasks and wakes that are
    /// still going, by their short ids.
    async fn cancel_choices(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        typed: &str,
    ) {
        use theseus_protocol::work::WorkKind;
        let typed = typed.trim().to_lowercase();
        let views = self
            .session_of(tx)
            .map(|s| self.tasks.in_place(&s))
            .unwrap_or_default();
        let mut open: Vec<_> = views
            .into_iter()
            .filter(|v| matches!(v.kind, WorkKind::Task | WorkKind::Wake) && !v.state.ended())
            .filter(|v| {
                typed.is_empty()
                    || v.title.to_lowercase().contains(&typed)
                    || v.id.to_lowercase().ends_with(&typed)
            })
            .collect();
        open.sort_by_key(|v| (v.kind != WorkKind::Task, v.started_at_ms, v.id.clone()));
        let pairs = open
            .into_iter()
            .take(snapshot::MAX_CHOICES)
            .map(|v| {
                let mark = if v.kind == WorkKind::Wake {
                    "⏰"
                } else {
                    "⚙️"
                };
                let short = crate::render::board_v2::short(&v.id).to_string();
                (format!("{mark} {} …{short}", v.title.trim()), short)
            })
            .collect();
        self.autocomplete(i, pairs).await;
    }

    pub(super) async fn autocomplete(&self, i: &Interaction, pairs: Vec<(String, String)>) {
        let data = InteractionResponseData {
            choices: Some(choice_list(pairs)),
            ..Default::default()
        };
        self.answer(
            i,
            InteractionResponseType::ApplicationCommandAutocompleteResult,
            Some(data),
        )
        .await;
    }

    /// A modal as the answer to an interaction (response type 9).
    pub(super) async fn respond_modal(&self, i: &Interaction, modal: &Value) {
        use twilight_http::request::Request;
        use twilight_http::routing::Route as HttpRoute;
        let req = Request::builder(&HttpRoute::InteractionCallback {
            interaction_id: i.id.get(),
            interaction_token: &i.token,
            with_response: false,
        })
        .use_authorization_token(false)
        .json(modal)
        .build();
        match req {
            Ok(req) => {
                if let Err(e) = self.interact.request::<Value>(req).await {
                    self.board.error("modal response", None, e);
                }
            }
            Err(e) => self.board.error("modal response", None, e),
        }
    }

    /// An ephemeral line, to the presser alone.
    pub(super) async fn tell(&self, i: &Interaction, text: String) {
        self.say_ephemeral(i, text).await;
    }
}

impl Place {
    /// A task command of this place (`PlaceMsg::Cmd`).
    pub(super) async fn task_cmd(&mut self, c: TaskCmd) {
        let TaskCmd { i, who, what } = c;
        let s = self.shared.clone();
        let name = match &what {
            What::Tasks(_) => "tasks",
            What::Task(_) => "task",
            What::Share(..) => "task share",
            What::Quiet(_) => "quiet",
            What::Answer(_) => "answer",
            What::NewTask(_) | What::NewTaskSubmit(_) => "task new",
        };
        match what {
            What::Tasks(show) => {
                let p = s.tasks.project(&self.session_id);
                let history = self.history(show).await;
                let parts = Parts {
                    needs: &p.needs,
                    working: &p.working,
                    done: &p.done,
                    history: &history,
                };
                let comps = snapshot::render(show, &parts, theseus_protocol::now_unix_ms());
                s.respond_v2(&i, 4, &comps, true).await;
            }
            What::Task(name) => self.task_named(&i, &name).await,
            What::Share(name, g) => self.share_task(&i, &name, g).await,
            What::Quiet(arg) => self.quiet(&i, arg.as_deref().unwrap_or("1h")).await,
            What::Answer(q) => s.answer_view(&i, q.as_deref()).await,
            What::NewTask(quote) => {
                s.respond_modal(&i, &start_task::modal(quote.as_deref(), None))
                    .await;
            }
            What::NewTaskSubmit(sub) => self.start_task(&i, &who, sub).await,
        }
        s.core.binding_ledger(
            LedgerKind::DiscordCommand,
            None,
            json!({"command": name, "by": who}),
        );
    }

    /// The finished tasks the store has for this place, for the shows that
    /// read them (`done`, `all`).
    async fn history(&self, show: Show) -> Vec<crate::render::history::Row> {
        match show {
            Show::Done | Show::All => super::tasks::finished(&self.shared, &self.target).await,
            _ => Vec::new(),
        }
    }

    /// `/task name:`: Details, or the new-task modal.
    async fn task_named(&self, i: &Interaction, name: &str) {
        let s = &self.shared;
        if name.trim() == snapshot::NEW {
            s.respond_modal(i, &start_task::modal(None, None)).await;
            return;
        }
        match s.tasks.find_task(&self.session_id, name) {
            Some(v) => s.respond_v2(i, 4, &self.detail_of(&v), true).await,
            None => {
                s.tell(
                    i,
                    format!("No single task of this place matches `{name}`: pick one as you type."),
                )
                .await
            }
        }
    }

    /// `/quiet`.
    async fn quiet(&mut self, i: &Interaction, arg: &str) {
        let s = &self.shared;
        let now = theseus_protocol::now_unix_ms();
        let text = if arg == "off" {
            s.tasks.set_quiet(&self.target, None);
            self.remember_quiet(None);
            "🔔 The quiet is over: this place's task pings are back.".to_string()
        } else {
            match quiet_until(s, arg, now) {
                Ok(until) => {
                    s.tasks.set_quiet(&self.target, Some(until));
                    self.remember_quiet(Some(until));
                    format!(
                        "🔕 Quiet until <t:{}:t>: this place's task pings are silent. The board \
                         keeps up.",
                        until / 1000
                    )
                }
                Err(e) => format!("⚠️ {e}"),
            }
        };
        s.tell(i, text).await;
        // The board says it too.
        self.live_board();
    }

    fn remember_quiet(&self, until: Option<u64>) {
        if let Err(e) = self
            .shared
            .core
            .store
            .put_meta(&quiet_key(&self.target), &until.unwrap_or(0))
        {
            self.shared.board.error("quiet", None, e);
        }
    }

    /// A restart brings back a quiet still running.
    pub(super) fn restore_quiet(&self) {
        let until = self
            .shared
            .core
            .store
            .get_meta::<u64>(&quiet_key(&self.target))
            .ok()
            .flatten()
            .filter(|t| *t > theseus_protocol::now_unix_ms());
        if let Some(t) = until {
            self.shared.tasks.set_quiet(&self.target, Some(t));
        }
    }
}

impl Place {
    /// The presses of this row's controls: a select's answer, a note's modal,
    /// Follow, Retry, Dismiss, New task and the snapshot's refresh. True when
    /// the press was one of them.
    pub(super) async fn more_press(&self, p: &super::live_board::TaskPress, id: &str) -> bool {
        use crate::render::answer_v2 as av2;
        use crate::render::task_view::{DISMISS, FOLLOW, RETRY};
        let s = &self.shared;
        if let Some(q) = av2::parse_select(id) {
            match p.values.first().map(String::as_str) {
                Some(av2::OTHER) => s.open_answer_modal(&p.i, q, av2::OTHER).await,
                Some(option) => self.answer_press(p, q, option).await,
                None => {}
            }
            return true;
        }
        if let Some((q, option)) = av2::parse_pair(av2::NOTE, id) {
            s.open_answer_modal(&p.i, q, option).await;
            return true;
        }
        if let Some(turn) = id.strip_prefix(start_task::OFFER) {
            self.offer_press(&p.i, turn).await;
            return true;
        }
        if id == super::start_task::NEW_BUTTON {
            s.respond_modal(&p.i, &start_task::modal(None, None)).await;
            return true;
        }
        if let Some(show) = id.strip_prefix(snapshot::REFRESH) {
            let pr = s.tasks.project(&self.session_id);
            let show = Show::parse(show).unwrap_or_default();
            let history = self.history(show).await;
            let parts = Parts {
                needs: &pr.needs,
                working: &pr.working,
                done: &pr.done,
                history: &history,
            };
            let comps = snapshot::render(show, &parts, theseus_protocol::now_unix_ms());
            s.respond_v2(&p.i, 7, &comps, true).await;
            return true;
        }
        if let Some(task) = id.strip_prefix(FOLLOW) {
            let line = match s.tasks.view_in_place(&self.session_id, task) {
                None => "That task is not on this board any more.".to_string(),
                Some(v) => {
                    let channel = p.i.channel.as_ref().map_or(0, |c| c.id.get());
                    match s.follow(&self.target, channel, &v) {
                        Ok(()) => format!(
                            "👁 Following **{}**: one silent message here mirrors it and ends \
                             with its report.",
                            crate::render::clip(v.title.trim(), 80)
                        ),
                        Err(why) => why,
                    }
                }
            };
            s.tell(&p.i, line).await;
            return true;
        }
        for (prefix, retry) in [(RETRY, true), (DISMISS, false)] {
            if let Some(task) = id.strip_prefix(prefix) {
                let line = self.failure_verb(&p.i, task, retry).await;
                s.tell(&p.i, line).await;
                return true;
            }
        }
        false
    }

    /// Retry or dismiss a failed task, as the presser; the core judges it.
    async fn failure_verb(&self, i: &Interaction, task: &str, retry: bool) -> String {
        use theseus_protocol::method;
        let s = &self.shared;
        if s.tasks.view_in_place(&self.session_id, task).is_none() {
            return "That task is not on this board any more.".into();
        }
        let discord = super::origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        let r = if retry {
            s.rpc
                .call::<_, theseus_protocol::TaskRetryResult>(
                    method::TASK_RETRY,
                    theseus_protocol::TaskRetryParams {
                        execution_id: task.to_string(),
                        note: None,
                        discord,
                    },
                )
                .await
                .map(|r| {
                    format!(
                        "↻ Tried again as `…{}` with ${:.2}; the failed one is dismissed.",
                        short_of(&r.task_id),
                        r.limit_usd
                    )
                })
        } else {
            s.rpc
                .call::<_, theseus_protocol::ExecutionDismissResult>(
                    method::EXECUTION_DISMISS,
                    theseus_protocol::ExecutionDismissParams {
                        execution_id: task.to_string(),
                        discord,
                    },
                )
                .await
                .map(|_| "✓ Dismissed: it stops needing you, and its record stays.".to_string())
        };
        match r {
            Ok(line) => line,
            Err(e) if e.code == theseus_protocol::error_code::REFUSED => {
                format!("🔐 That did not count: {}.", e.message)
            }
            Err(e) => format!("⚠️ {e}"),
        }
    }
}

fn short_of(id: &str) -> &str {
    crate::render::board_v2::short(id)
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn interaction_callbacks_go_out_on_a_client_with_no_rate_limiter() {
        let cfg = theseus_core::config::DiscordConfig::default();
        let lane = super::super::client_with("fake-token-not-a-secret", &cfg, true);
        let callbacks = super::super::client_with("fake-token-not-a-secret", &cfg, false);
        assert!(
            lane.ratelimiter().is_some(),
            "the lane's client waits its turn"
        );
        assert!(
            callbacks.ratelimiter().is_none(),
            "a busy lane never delays a callback's three-second deadline"
        );
    }

    #[test]
    fn a_quiet_ends_at_the_time_asked_for() {
        // The spans are plain arithmetic; "morning" is the zone's 8:00 (the daemon's parser).
        let now = 1_790_000_000_000;
        assert_eq!(super::quiet_until_span("30m", now), Some(now + 1_800_000));
        assert_eq!(super::quiet_until_span("1h", now), Some(now + 3_600_000));
        assert_eq!(super::quiet_until_span("tea", now), None);
    }
}
