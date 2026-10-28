//! The future's commands (theseus-j90w2; horizon HZ9): `/agenda`, `/routine`,
//! `/snooze`, `/watch`, and the autocomplete of `/cancel`. A child of
//! `runtime`, apart from it for the shape budget's file ceiling.
//!
//! - **Private places only.** The agenda, the routines and the watches are the
//!   owner's: a shared place is told so and shown nothing, and the core judges
//!   every act as the presser (the owner, from a private place).
//! - **`/agenda [when: today|week|all]`** is ephemeral: a live read of the
//!   future, in the sections of `theseus agenda`.
//! - **`/routine add`** opens a modal (title, schedule, brief, budget, where it
//!   posts) and its submit is `routine.create`; `list` is ephemeral, a line and
//!   a Run now and a Pause or Resume for each routine, the list drawn again
//!   after a press; `pause`, `resume` and `run` take the routine by name, with
//!   autocomplete from memory.
//! - **`/snooze wake: when:`** and **`/cancel id:`** autocomplete over the
//!   pending wakes, reminders first, from the snapshot in memory
//!   (`horizon::Future`): no daemon call answers a keystroke.
//! - **`/watch add`** opens a modal for a fetch or a file check; a command
//!   check is the owner's, from the DM, and shows its command with an Approve
//!   button before `watch.create` is called (`hw:ok:<token>`, once). `list` is
//!   ephemeral.

use theseus_protocol::method;
use theseus_protocol::routines::{
    RoutineCreateParams, RoutineEditResult, RoutineInfo, RoutineNameParams, RoutinePauseParams,
};
use theseus_protocol::watches::{WatchCheck, WatchCreateParams, WatchCreateResult};
use theseus_protocol::{DiscordOrigin, WakeEditResult, WakeSnoozeParams};
use twilight_model::application::command::{
    Command, CommandOptionChoice, CommandOptionChoiceValue, CommandType,
};
use twilight_model::application::interaction::application_command::{
    CommandData, CommandDataOption, CommandOptionValue,
};
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, TextInputStyle};
use twilight_model::channel::message::{AllowedMentions, Component, MessageFlags};
use twilight_model::http::interaction::{InteractionResponseData, InteractionResponseType};
use twilight_util::builder::command::{CommandBuilder, StringBuilder, SubCommandBuilder};

use super::prompt::{fields_of, input, labelled};
use super::Shared;
use crate::render::clip;
use crate::render::horizon::{agenda_text, routine_line, Span, ROUTINE_PREFIX};

/// A routine's modal, and a watch's (then `:<kind>`).
const ROUTINE_MODAL: &str = "hzm:routine";
const WATCH_MODAL: &str = "hzm:watch:";
/// A command watch's approval buttons: `hw:ok:<token>` and `hw:no:<token>`.
const WATCH_PREFIX: &str = "hw:";
/// Discord's most choices.
const CHOICES: usize = 25;
/// How long a command watch waits to be approved.
const APPROVE_MS: u64 = 10 * 60_000;
/// The most command watches waiting to be approved.
const PENDING_MAX: usize = 8;
/// What a list message's text may hold.
const MAX_TEXT: usize = 1_800;
/// A list message's most rows of buttons (Discord's five).
const MAX_ROWS: usize = 5;

/// `/agenda`, `/routine`, `/snooze` and `/watch`.
pub(super) fn commands() -> Vec<Command> {
    let name = |what: &str| {
        StringBuilder::new("name", what.to_string())
            .required(true)
            .autocomplete(true)
    };
    vec![
        CommandBuilder::new(
            "agenda",
            "Your future in one view: what is coming up, your routines, your watches (only you see it)",
            CommandType::ChatInput,
        )
        .option(
            StringBuilder::new("when", "How far ahead: today, this week, or all of it")
                .choices([("today", "today"), ("week", "week"), ("all", "all")]),
        )
        .build(),
        CommandBuilder::new(
            "routine",
            "Routines: work that runs on a schedule by itself",
            CommandType::ChatInput,
        )
        .option(SubCommandBuilder::new("add", "Make a routine: a form asks for its schedule and brief"))
        .option(SubCommandBuilder::new("list", "Your routines, with Run now and Pause"))
        .option(SubCommandBuilder::new("pause", "Hold a routine").option(name("The routine")))
        .option(SubCommandBuilder::new("resume", "Let a held routine run again").option(name("The routine")))
        .option(SubCommandBuilder::new("run", "Run a routine now").option(name("The routine")))
        .build(),
        CommandBuilder::new(
            "snooze",
            "Move a pending reminder or wake to another time",
            CommandType::ChatInput,
        )
        .option(
            StringBuilder::new("wake", "The reminder or wake: pick one as you type")
                .required(true)
                .autocomplete(true),
        )
        .option(
            StringBuilder::new("when", "When: in 1h, tomorrow 9:00, fri 9:00, 17:30")
                .required(true)
                .autocomplete(true),
        )
        .build(),
        CommandBuilder::new(
            "watch",
            "Watches: wait on the world with no model, and be told when it is true",
            CommandType::ChatInput,
        )
        .option(
            SubCommandBuilder::new("add", "Watch a URL or a file; a command is yours from the DM")
                .option(
                    StringBuilder::new("kind", "What it looks at (default: a URL)").choices([
                        ("a URL", "fetch"),
                        ("a file", "file"),
                        ("a command (from your DM)", "command"),
                    ]),
                ),
        )
        .option(SubCommandBuilder::new("list", "Your open watches"))
        .build(),
    ]
}

/// The inner options of a command and its subcommand's name.
fn sub_of(c: &CommandData) -> (String, Vec<CommandDataOption>) {
    match c.options.first() {
        Some(o) => match &o.value {
            CommandOptionValue::SubCommand(inner) => (o.name.clone(), inner.clone()),
            _ => (String::new(), c.options.clone()),
        },
        None => (String::new(), vec![]),
    }
}

/// A text option, typed or being typed.
fn opt(opts: &[CommandDataOption], name: &str) -> Option<String> {
    opts.iter().find_map(|o| match (&o.value, o.name == name) {
        (CommandOptionValue::String(s) | CommandOptionValue::Focused(s, _), true) => {
            Some(s.clone())
        }
        _ => None,
    })
}

/// The option being typed: its name and text.
fn focused(opts: &[CommandDataOption]) -> Option<(String, String)> {
    opts.iter().find_map(|o| match &o.value {
        CommandOptionValue::Focused(s, _) => Some((o.name.clone(), s.clone())),
        _ => None,
    })
}

fn choice(name: &str, value: &str) -> CommandOptionChoice {
    CommandOptionChoice {
        name: clip(name, 100),
        name_localizations: None,
        value: CommandOptionChoiceValue::String(value.chars().take(100).collect()),
    }
}

/// The wakes a keystroke offers: reminders first, then the rest, each named
/// by its time and note, valued by its short id (`/cancel`) or its whole one
/// (`/snooze`), those containing what was typed.
pub(super) fn wake_choices(
    wakes: &[theseus_protocol::work::horizon::AgendaLine],
    typed: &str,
    whole: bool,
) -> Vec<CommandOptionChoice> {
    let t = typed.trim().to_lowercase();
    wakes
        .iter()
        .filter(|l| t.is_empty() || l.title.to_lowercase().contains(&t) || l.id.ends_with(&t))
        .take(CHOICES)
        .map(|l| {
            let icon = crate::render::horizon::emoji(l);
            let id = if whole {
                l.id.clone()
            } else {
                crate::render::board_v2::short(&l.id).to_string()
            };
            choice(
                &format!(
                    "{icon} {} · {}",
                    clip(&l.title.replace('\n', " "), 60),
                    short_time(l.at_ms)
                ),
                &id,
            )
        })
        .collect()
}

/// A time in a choice's name: the date and the clock, in UTC (a choice has no
/// Discord stamp).
fn short_time(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m) = (secs % 86_400 / 3600, secs % 3600 / 60);
    let days = secs / 86_400;
    // 1970-01-01 + days, civil from days (Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{mo:02}-{d:02} {h:02}:{m:02} UTC")
}

/// The routines a keystroke offers, those containing what was typed.
pub(super) fn routine_choices(routines: &[RoutineInfo], typed: &str) -> Vec<CommandOptionChoice> {
    let t = typed.trim().to_lowercase();
    routines
        .iter()
        .filter(|r| t.is_empty() || r.title.to_lowercase().contains(&t) || r.short.contains(&t))
        .take(CHOICES)
        .map(|r| {
            choice(
                &format!("🔁 {}{}", r.title, if r.paused { " (paused)" } else { "" }),
                &r.id,
            )
        })
        .collect()
}

/// The `/routine list` message: a line for each routine and a Run now and a
/// Pause or Resume for each, five routines at most (Discord's five rows).
pub(super) fn routine_list(
    routines: &[RoutineInfo],
    records: &std::collections::HashMap<String, (u32, u32)>,
) -> InteractionResponseData {
    if routines.is_empty() {
        return InteractionResponseData {
            content: Some("No routines. `/routine add` makes one.".into()),
            flags: Some(MessageFlags::EPHEMERAL),
            components: Some(vec![]),
            allowed_mentions: Some(AllowedMentions::default()),
            ..Default::default()
        };
    }
    let mut text = vec![format!("**🔁 Routines** · {}", routines.len())];
    text.extend(
        routines
            .iter()
            .take(MAX_ROWS)
            .map(|r| routine_line(r, records.get(&r.id).copied())),
    );
    if routines.len() > MAX_ROWS {
        text.push(format!(
            "-# +{} more: `/routine run`, `pause`, `resume` take any",
            routines.len() - MAX_ROWS
        ));
    }
    let button = |id: String, label: String| {
        Component::Button(Button {
            id: None,
            custom_id: Some(id),
            disabled: false,
            emoji: None,
            label: Some(clip(&label, 80)),
            style: ButtonStyle::Secondary,
            url: None,
            sku_id: None,
        })
    };
    let rows: Vec<Component> = routines
        .iter()
        .take(MAX_ROWS)
        .map(|r| {
            let (verb, word) = if r.paused {
                ("resume", "▶ Resume")
            } else {
                ("pause", "⏸ Pause")
            };
            Component::ActionRow(ActionRow {
                id: None,
                components: vec![
                    button(
                        format!("{ROUTINE_PREFIX}run:{}", r.id),
                        format!("▶ Run now · {}", clip(&r.title, 40)),
                    ),
                    button(
                        format!("{ROUTINE_PREFIX}{verb}:{}", r.id),
                        format!("{word} · {}", clip(&r.title, 40)),
                    ),
                ],
            })
        })
        .collect();
    InteractionResponseData {
        content: Some(clip(&text.join("\n"), MAX_TEXT)),
        flags: Some(MessageFlags::EPHEMERAL),
        components: Some(rows),
        allowed_mentions: Some(AllowedMentions::default()),
        ..Default::default()
    }
}

fn modal(
    id: String,
    title: &str,
    inputs: Vec<(&str, &str, TextInputStyle, bool, &str)>,
) -> InteractionResponseData {
    let components = inputs
        .into_iter()
        .map(|(key, label, style, required, hint)| {
            labelled(
                label,
                None,
                input(
                    key,
                    style,
                    required,
                    (!hint.is_empty()).then(|| hint.to_string()),
                ),
            )
        })
        .collect();
    InteractionResponseData {
        custom_id: Some(id),
        title: Some(title.into()),
        components: Some(components),
        ..Default::default()
    }
}

/// The routine modal.
pub(super) fn routine_modal() -> InteractionResponseData {
    modal(
        ROUTINE_MODAL.into(),
        "New routine",
        vec![
            (
                "title",
                "Name",
                TextInputStyle::Short,
                true,
                "Morning digest",
            ),
            (
                "schedule",
                "Schedule",
                TextInputStyle::Short,
                true,
                "weekdays 08:00 · every 30m · monthly 09:00",
            ),
            (
                "brief",
                "What each run does",
                TextInputStyle::Paragraph,
                true,
                "Read my mail and tell me what needs me",
            ),
            (
                "budget",
                "Budget: a run / the month, in dollars",
                TextInputStyle::Short,
                false,
                "0.50 / 10",
            ),
            (
                "where",
                "Where it posts: dm, none, or here",
                TextInputStyle::Short,
                false,
                "dm",
            ),
        ],
    )
}

/// The watch modal for a kind.
pub(super) fn watch_modal(kind: &str) -> InteractionResponseData {
    let (target_label, hint) = match kind {
        "file" => ("The file's path", "/srv/build/done.flag"),
        "command" => (
            "The command (words split on spaces)",
            "gh pr view 123 --json state",
        ),
        _ => ("The URL", "https://example.test/status"),
    };
    modal(
        format!("{WATCH_MODAL}{kind}"),
        "New watch",
        vec![
            (
                "title",
                "What you wait for, as a question",
                TextInputStyle::Short,
                true,
                "PR 123 merged?",
            ),
            ("target", target_label, TextInputStyle::Short, true, hint),
            (
                "until",
                "Until: equals X · contains X · exists · changes · above N",
                TextInputStyle::Short,
                true,
                "equals MERGED",
            ),
            (
                "every",
                "How often (default 5m, 1m at the most)",
                TextInputStyle::Short,
                false,
                "5m",
            ),
            (
                "for",
                "Give up after (default 30d)",
                TextInputStyle::Short,
                false,
                "3d",
            ),
        ],
    )
}

/// A watch from a submitted modal.
pub(super) fn watch_params(
    kind: &str,
    f: &std::collections::HashMap<String, String>,
    author: Option<String>,
    discord: Option<DiscordOrigin>,
) -> Result<WatchCreateParams, String> {
    let get = |k: &str| {
        f.get(k)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let target = get("target").ok_or("a watch needs something to look at")?;
    let check = match kind {
        "file" => WatchCheck::File { path: target },
        "command" => WatchCheck::Run {
            argv: target.split_whitespace().map(str::to_string).collect(),
            json: None,
        },
        _ => WatchCheck::Fetch {
            url: target,
            json: None,
        },
    };
    Ok(WatchCreateParams {
        title: get("title").ok_or("a watch needs a question")?,
        check,
        until: get("until").ok_or("a watch needs a condition")?,
        every: get("every"),
        for_at_most: get("for"),
        then: None,
        follow_up: None,
        session_id: None,
        author,
        discord,
    })
}

/// A routine from a submitted modal: `0.50 / 10` is a run's and the month's
/// dollars, `where` is `dm`, `none`, `here` (this place) or a place as written.
pub(super) fn routine_params(
    f: &std::collections::HashMap<String, String>,
    here: &str,
    author: Option<String>,
    discord: Option<DiscordOrigin>,
) -> Result<RoutineCreateParams, String> {
    let get = |k: &str| {
        f.get(k)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let (mut run, mut month) = (None, None);
    if let Some(b) = get("budget") {
        let mut parts = b.split('/').map(|p| p.trim().trim_start_matches('$'));
        let num = |p: Option<&str>| -> Result<Option<f64>, String> {
            match p.filter(|p| !p.is_empty()) {
                None => Ok(None),
                Some(p) => p
                    .parse::<f64>()
                    .map(Some)
                    .map_err(|_| format!("`{p}` is not a dollar amount")),
            }
        };
        run = num(parts.next())?;
        month = num(parts.next())?;
    }
    let deliver_to = match get("where").as_deref() {
        None | Some("dm") => None,
        Some("here") => Some(here.to_string()),
        Some(other) => Some(other.to_string()),
    };
    Ok(RoutineCreateParams {
        title: get("title").ok_or("a routine needs a name")?,
        schedule: get("schedule").ok_or("a routine needs a schedule")?,
        brief: get("brief").ok_or("a routine needs a brief")?,
        budget_usd: run,
        month_usd: month,
        deliver_to,
        author,
        discord,
        ..Default::default()
    })
}

/// The place an interaction came from, as the core names it.
fn target_of(i: &Interaction) -> Option<String> {
    let user = i.author()?.id.get();
    let channel = i.channel.as_ref()?.id.get();
    Some(if i.guild_id.is_none() {
        format!("discord:dm:{user}")
    } else {
        format!("discord:channel:{channel}")
    })
}

fn origin(i: &Interaction) -> Option<DiscordOrigin> {
    super::origin_of(
        i.author().map(|u| u.id.get()),
        i.channel.as_ref().map(|c| c.id.get()),
        i.guild_id,
    )
}

const SHARED: &str = "That is yours alone: ask in your DM or a private channel.";

impl Shared {
    /// A command, a button or a modal of the future: whether it was one, and
    /// answered.
    pub(super) async fn horizon_interaction(&self, i: &Interaction, who: &str) -> bool {
        match &i.data {
            Some(InteractionData::ApplicationCommand(c)) => {
                let autocomplete = i.kind == InteractionType::ApplicationCommandAutocomplete;
                match (c.name.as_str(), autocomplete) {
                    ("agenda" | "routine" | "snooze" | "watch", false) => {
                        self.horizon_command(i, c, who).await;
                        true
                    }
                    ("routine" | "snooze" | "cancel", true) => {
                        self.horizon_autocomplete(i, c).await
                    }
                    _ => false,
                }
            }
            Some(InteractionData::MessageComponent(c)) => {
                if let Some(rest) = c.custom_id.strip_prefix(ROUTINE_PREFIX) {
                    if let Some((verb, id)) = rest.split_once(':') {
                        self.routine_press(i, verb, id, who).await;
                        return true;
                    }
                }
                if let Some(rest) = c.custom_id.strip_prefix(WATCH_PREFIX) {
                    if let Some((verb, token)) = rest.split_once(':') {
                        self.watch_approval(i, verb, token, who).await;
                        return true;
                    }
                }
                if let Some(p) = super::horizon::parse_post(&c.custom_id) {
                    self.post_press(i, p, who, origin(i)).await;
                    return true;
                }
                false
            }
            Some(InteractionData::ModalSubmit(m)) if m.custom_id.starts_with("hzm:") => {
                let fields: std::collections::HashMap<String, String> =
                    fields_of(m).into_iter().collect();
                if m.custom_id == ROUTINE_MODAL {
                    self.routine_submit(i, &fields, who).await;
                } else if let Some(kind) = m.custom_id.strip_prefix(WATCH_MODAL) {
                    self.watch_submit(i, kind, &fields, who).await;
                }
                true
            }
            _ => false,
        }
    }

    /// Autocomplete from the snapshot in memory. False for a command that is
    /// not the future's (`/cancel`'s other options, `/routine`'s add).
    async fn horizon_autocomplete(&self, i: &Interaction, c: &CommandData) -> bool {
        let (_, opts) = sub_of(c);
        let Some((which, typed)) = focused(&opts).or_else(|| focused(&c.options)) else {
            return false;
        };
        if target_of(i).is_none_or(|t| !self.is_private(&t)) {
            // A shared place is offered nothing of the owner's.
            return self.offer(i, vec![]).await;
        }
        let snap = self.tasks.future.snapshot();
        let choices = match (c.name.as_str(), which.as_str()) {
            ("routine", "name") => routine_choices(&snap.routines, &typed),
            ("snooze", "wake") => wake_choices(&self.tasks.future.wakes(), &typed, true),
            ("snooze", "when") => super::remind::suggest(&typed)
                .into_iter()
                .map(|s| choice(&s, &s))
                .collect(),
            ("cancel", "id") => wake_choices(&self.tasks.future.wakes(), &typed, false),
            _ => return false,
        };
        self.offer(i, choices).await
    }

    async fn offer(&self, i: &Interaction, choices: Vec<CommandOptionChoice>) -> bool {
        let data = InteractionResponseData {
            choices: Some(choices),
            ..Default::default()
        };
        self.answer(
            i,
            InteractionResponseType::ApplicationCommandAutocompleteResult,
            Some(data),
        )
        .await;
        true
    }

    async fn horizon_command(&self, i: &Interaction, c: &CommandData, who: &str) {
        let private = target_of(i).is_some_and(|t| self.is_private(&t));
        if !private {
            return self.reply_ephemeral(i, SHARED.into()).await;
        }
        let (sub, opts) = sub_of(c);
        match (c.name.as_str(), sub.as_str()) {
            ("agenda", _) => {
                let span = Span::parse(&opt(&opts, "when").unwrap_or_default());
                if self.read_future().await {
                    self.redraw_private_boards();
                }
                let s = self.tasks.future.snapshot();
                let t = agenda_text(
                    span,
                    theseus_protocol::now_unix_ms(),
                    &s.lines,
                    &s.routines,
                    &s.watches,
                    &s.projects,
                    &[],
                );
                self.reply_ephemeral(i, t).await;
            }
            ("routine", "add") => {
                self.answer(i, InteractionResponseType::Modal, Some(routine_modal()))
                    .await;
            }
            ("routine", "list") => {
                if self.read_future().await {
                    self.redraw_private_boards();
                }
                let s = self.tasks.future.snapshot();
                let data = routine_list(&s.routines, &s.records);
                self.answer(
                    i,
                    InteractionResponseType::ChannelMessageWithSource,
                    Some(data),
                )
                .await;
            }
            ("routine", verb @ ("pause" | "resume" | "run")) => {
                let name = opt(&opts, "name").unwrap_or_default();
                let said = self.routine_verb(i, verb, &name, who).await;
                self.reply_ephemeral(i, said).await;
            }
            ("snooze", _) => {
                let (wake, when) = (
                    opt(&opts, "wake").unwrap_or_default(),
                    opt(&opts, "when").unwrap_or_default(),
                );
                let r = self
                    .rpc
                    .call::<_, WakeEditResult>(
                        method::WAKE_SNOOZE,
                        WakeSnoozeParams {
                            wake,
                            when: Some(when),
                            fired: false,
                            author: Some(who.to_string()),
                        },
                    )
                    .await;
                self.tasks.future.dirty();
                let said = match r {
                    Ok(r) => format!(
                        "😴 `{}` is set again for {}.",
                        r.wake.short, r.wake.due_local
                    ),
                    Err(e) => format!("⚠️ Not snoozed: {}", e.message),
                };
                self.reply_ephemeral(i, said).await;
            }
            ("watch", sub) => self.watch_command(i, sub, &opts).await,
            _ => self.reply_ephemeral(i, "Unknown option.".into()).await,
        }
    }

    async fn watch_command(&self, i: &Interaction, sub: &str, opts: &[CommandDataOption]) {
        match sub {
            "add" => {
                let kind = opt(opts, "kind").unwrap_or_else(|| "fetch".into());
                if kind == "command" && i.guild_id.is_some() {
                    return self
                        .reply_ephemeral(i, "A command check is offered from your DM only.".into())
                        .await;
                }
                self.answer(i, InteractionResponseType::Modal, Some(watch_modal(&kind)))
                    .await;
            }
            "list" => {
                if self.read_future().await {
                    self.redraw_private_boards();
                }
                let s = self.tasks.future.snapshot();
                let mut out = vec![format!("**👁 Watches** · {}", s.watches.len())];
                for w in s.watches.iter().take(10) {
                    out.push(format!(
                        "👁 `{}` {} · until {} · {} checks{}",
                        w.short,
                        clip(&w.title, 50),
                        clip(&w.until, 30),
                        w.checks,
                        w.value
                            .as_deref()
                            .map(|v| format!(" · last: {}", clip(v, 30)))
                            .unwrap_or_default()
                    ));
                }
                if s.watches.is_empty() {
                    out.push("none open. `/watch add` makes one.".into());
                }
                self.reply_ephemeral(i, out.join("\n")).await;
            }
            _ => self.reply_ephemeral(i, "Unknown option.".into()).await,
        }
    }

    /// A routine by name (an id, a short id, or what its title holds), and a
    /// verb on it as the presser: what to say of it.
    async fn routine_verb(&self, i: &Interaction, verb: &str, name: &str, who: &str) -> String {
        let id = {
            let s = self.tasks.future.snapshot();
            let t = name.trim().to_lowercase();
            s.routines
                .iter()
                .find(|r| r.id == name || r.short == t)
                .or_else(|| {
                    s.routines
                        .iter()
                        .find(|r| r.title.to_lowercase().contains(&t))
                })
                .map_or_else(|| name.to_string(), |r| r.id.clone())
        };
        let (author, discord) = (Some(who.to_string()), origin(i));
        let r = match verb {
            "run" => {
                self.rpc
                    .call::<_, RoutineEditResult>(
                        method::ROUTINE_RUN_NOW,
                        RoutineNameParams {
                            id,
                            author,
                            discord,
                        },
                    )
                    .await
            }
            "resume" => {
                self.rpc
                    .call::<_, RoutineEditResult>(
                        method::ROUTINE_RESUME,
                        RoutineNameParams {
                            id,
                            author,
                            discord,
                        },
                    )
                    .await
            }
            _ => {
                self.rpc
                    .call::<_, RoutineEditResult>(
                        method::ROUTINE_PAUSE,
                        RoutinePauseParams {
                            id,
                            until: None,
                            author,
                            discord,
                        },
                    )
                    .await
            }
        };
        self.tasks.future.dirty();
        match r {
            Ok(r) => match verb {
                "run" => format!("▶ **{}** is running now.", r.routine.title),
                "resume" => format!(
                    "▶ **{}** runs again{}.",
                    r.routine.title,
                    r.routine
                        .next
                        .map(|n| format!(", next {n}"))
                        .unwrap_or_default()
                ),
                _ => format!("⏸ **{}** is paused.", r.routine.title),
            },
            Err(e) => format!("⚠️ That did not count: {}", e.message),
        }
    }

    /// A routine's button on the board or in the list. In the list (an
    /// ephemeral message) the list is drawn again; elsewhere the presser is
    /// told.
    async fn routine_press(&self, i: &Interaction, verb: &str, id: &str, who: &str) {
        let said = self.routine_verb(i, verb, id, who).await;
        let in_list = i
            .message
            .as_ref()
            .is_some_and(|m| m.flags.is_some_and(|f| f.contains(MessageFlags::EPHEMERAL)));
        if in_list && !said.starts_with('⚠') {
            if self.read_future().await {
                self.redraw_private_boards();
            }
            let s = self.tasks.future.snapshot();
            let mut data = routine_list(&s.routines, &s.records);
            data.content = data.content.map(|c| format!("{c}\n-# {said}"));
            self.answer(i, InteractionResponseType::UpdateMessage, Some(data))
                .await;
        } else {
            self.reply_ephemeral(i, said).await;
        }
    }

    async fn routine_submit(
        &self,
        i: &Interaction,
        f: &std::collections::HashMap<String, String>,
        who: &str,
    ) {
        let here = target_of(i).unwrap_or_default();
        let p = match routine_params(f, &here, Some(who.to_string()), origin(i)) {
            Ok(p) => p,
            Err(e) => return self.reply_ephemeral(i, format!("⚠️ Not made: {e}")).await,
        };
        let r = self
            .rpc
            .call::<_, RoutineEditResult>(method::ROUTINE_CREATE, p)
            .await;
        self.tasks.future.dirty();
        let said =
            match r {
                Ok(r) => format!(
                "🔁 Routine **{}** `{}`: {}{} · up to ${:.2} a run, ${:.0} a month · posts to {}.",
                r.routine.title,
                r.routine.short,
                r.routine.series,
                r.routine.next.map(|n| format!(" (next {n})")).unwrap_or_default(),
                r.routine.run_usd,
                r.routine.month_usd,
                if r.routine.deliver_to == "dm" { "your DM" } else { &r.routine.deliver_to }
            ),
                Err(e) => format!("⚠️ Not made: {}", e.message),
            };
        self.reply_ephemeral(i, said).await;
    }

    async fn watch_submit(
        &self,
        i: &Interaction,
        kind: &str,
        f: &std::collections::HashMap<String, String>,
        who: &str,
    ) {
        if kind == "command" && i.guild_id.is_some() {
            return self
                .reply_ephemeral(i, "A command check is offered from your DM only.".into())
                .await;
        }
        let p = match watch_params(kind, f, Some(who.to_string()), origin(i)) {
            Ok(p) => p,
            Err(e) => return self.reply_ephemeral(i, format!("⚠️ Not made: {e}")).await,
        };
        if kind != "command" {
            let said = self.watch_create(p).await;
            return self.reply_ephemeral(i, said).await;
        }
        // A command is approved once, showing what will run.
        let WatchCheck::Run { argv, .. } = &p.check else {
            return;
        };
        let shown = argv.join(" ");
        let token = {
            let mut pending = self.tasks.future.pending.lock().unwrap();
            let now = theseus_protocol::now_unix_ms();
            pending.retain(|_, (at, _)| now.saturating_sub(*at) < APPROVE_MS);
            if pending.len() >= PENDING_MAX {
                None
            } else {
                let token = format!("{now:x}{:x}", pending.len());
                pending.insert(token.clone(), (now, p));
                Some(token)
            }
        };
        let Some(token) = token else {
            return self
                .reply_ephemeral(
                    i,
                    "Too many commands wait for your yes: answer them first.".into(),
                )
                .await;
        };
        let button = |verb: &str, label: &str, style: ButtonStyle| {
            Component::Button(Button {
                id: None,
                custom_id: Some(format!("{WATCH_PREFIX}{verb}:{token}")),
                disabled: false,
                emoji: None,
                label: Some(label.into()),
                style,
                url: None,
                sku_id: None,
            })
        };
        let data = InteractionResponseData {
            content: Some(format!(
                "👁 This watch runs a command on a timer, in a sandbox, with no model:\n```\n{}\n```\nApprove it once?",
                shown.replace("```", "'''")
            )),
            flags: Some(MessageFlags::EPHEMERAL),
            components: Some(vec![Component::ActionRow(ActionRow {
                id: None,
                components: vec![
                    button("ok", "Approve", ButtonStyle::Success),
                    button("no", "Don't", ButtonStyle::Secondary),
                ],
            })]),
            allowed_mentions: Some(AllowedMentions::default()),
            ..Default::default()
        };
        self.answer(
            i,
            InteractionResponseType::ChannelMessageWithSource,
            Some(data),
        )
        .await;
    }

    async fn watch_approval(&self, i: &Interaction, verb: &str, token: &str, _who: &str) {
        let taken = self.tasks.future.pending.lock().unwrap().remove(token);
        let said = match (verb, taken) {
            ("ok", Some((_, p))) => self.watch_create(p).await,
            ("no", Some(_)) => "Not watching.".into(),
            _ => "That approval has passed: make the watch again.".into(),
        };
        let data = InteractionResponseData {
            content: Some(said),
            components: Some(vec![]),
            allowed_mentions: Some(AllowedMentions::default()),
            ..Default::default()
        };
        self.answer(i, InteractionResponseType::UpdateMessage, Some(data))
            .await;
    }

    async fn watch_create(&self, p: WatchCreateParams) -> String {
        let r = self
            .rpc
            .call::<_, WatchCreateResult>(method::WATCH_CREATE, p)
            .await;
        self.tasks.future.dirty();
        match r {
            Ok(r) => format!(
                "👁 Watching `{}` {} · until {} · every {}. I will tell you when it is true.",
                r.watch.short, r.watch.title, r.watch.until, r.watch.every
            ),
            Err(e) => format!("⚠️ Not watching: {}", e.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use theseus_protocol::work::horizon::{AgendaLine, AgendaWhat};
    use theseus_protocol::work::WorkKind;

    fn wake(id: &str, at: u64, title: &str, purpose: &str) -> AgendaLine {
        AgendaLine {
            at_ms: at,
            id: id.into(),
            kind: WorkKind::Wake,
            what: AgendaWhat::Due,
            title: title.into(),
            session_id: None,
            purpose: Some(purpose.into()),
            occurrence: None,
        }
    }

    fn values(c: &[CommandOptionChoice]) -> Vec<String> {
        c.iter()
            .map(|c| match &c.value {
                CommandOptionChoiceValue::String(s) => s.clone(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn a_wake_is_offered_by_its_whole_id_to_snooze_and_its_short_one_to_cancel() {
        let wakes = vec![
            wake(
                "wak_01J0000000000000000aaaaaa",
                1_790_000_000_000,
                "stretch",
                "remind",
            ),
            wake(
                "wak_01J0000000000000000bbbbbb",
                1_790_000_100_000,
                "stand",
                "remind",
            ),
        ];
        assert_eq!(
            values(&wake_choices(&wakes, "", true)),
            [
                "wak_01J0000000000000000aaaaaa",
                "wak_01J0000000000000000bbbbbb"
            ]
        );
        assert_eq!(
            values(&wake_choices(&wakes, "", false)),
            ["aaaaaa", "bbbbbb"]
        );
        assert_eq!(values(&wake_choices(&wakes, "stan", false)), ["bbbbbb"]);
    }

    #[test]
    fn a_modals_fields_make_a_routine_and_say_what_is_wrong_with_a_budget() {
        let f: HashMap<String, String> = [
            ("title", "Morning digest"),
            ("schedule", "weekdays 08:00"),
            ("brief", "read the mail"),
            ("budget", "0.50 / 10"),
            ("where", "here"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let p = routine_params(&f, "discord:dm:42", Some("discord:ana".into()), None).unwrap();
        assert_eq!(p.budget_usd, Some(0.5));
        assert_eq!(p.month_usd, Some(10.0));
        assert_eq!(p.deliver_to.as_deref(), Some("discord:dm:42"));
        let mut bad = f.clone();
        bad.insert("budget".into(), "lots".into());
        assert!(routine_params(&bad, "", None, None)
            .unwrap_err()
            .contains("lots"));
        let mut dm = f;
        dm.insert("where".into(), "dm".into());
        assert_eq!(
            routine_params(&dm, "", None, None).unwrap().deliver_to,
            None
        );
    }

    #[test]
    fn a_watchs_modal_makes_each_kind_of_check() {
        let f: HashMap<String, String> = [
            ("title", "PR 123 merged?"),
            ("target", "gh pr view 123"),
            ("until", "equals MERGED"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let p = watch_params("command", &f, None, None).unwrap();
        assert_eq!(
            p.check,
            WatchCheck::Run {
                argv: vec!["gh".into(), "pr".into(), "view".into(), "123".into()],
                json: None
            }
        );
        assert!(matches!(
            watch_params("file", &f, None, None).unwrap().check,
            WatchCheck::File { .. }
        ));
        assert!(matches!(
            watch_params("fetch", &f, None, None).unwrap().check,
            WatchCheck::Fetch { .. }
        ));
        assert!(watch_params("file", &HashMap::new(), None, None).is_err());
    }

    #[test]
    fn the_commands_list_carries_the_four_of_the_future_and_every_name_is_bare() {
        let names: Vec<String> = commands().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["agenda", "routine", "snooze", "watch"]);
    }
}
