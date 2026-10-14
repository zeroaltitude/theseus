//! `/thread new` and `/thread list` (theseus-nhew): the threads on virtual
//! places `thread:<slug>`, as the presser, ephemeral. A child of `runtime`,
//! apart from it for the shape budget's file ceiling.
//!
//! - **`/thread new slug: [title:]`** is `thread.new` with the presser's ids.
//! - **`/thread list`** is `thread.list`: each thread's state, last activity,
//!   context size and today's spend.
//!
//! Both are the core's to judge: a thread is private always, so only the
//! owner, from a private place, makes or lists one; anyone else reads the
//! core's refusal, and a shared place never sees a thread's words.

use theseus_protocol::method;
use theseus_protocol::threads::{ThreadListParams, ThreadListResult, ThreadNewParams, ThreadRow};
use theseus_protocol::{DiscordOrigin, LedgerKind};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::{CommandBuilder, StringBuilder, SubCommandBuilder};

use super::Shared;

/// What a message's text may hold, with room for the lines after it.
const MAX_TEXT: usize = 1800;

/// `/thread new slug: title:` and `/thread list`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "thread",
        "Threads: lines of work, each with a session of its own",
        CommandType::ChatInput,
    )
    .option(
        SubCommandBuilder::new("new", "Make a thread")
            .option(
                StringBuilder::new("slug", "Lower-case letters, digits and hyphens").required(true),
            )
            .option(StringBuilder::new("title", "What it is called")),
    )
    .option(SubCommandBuilder::new(
        "list",
        "Your threads, with their context and today's spend",
    ))
    .build()
}

/// A context size as `12.4k`.
fn tokens(t: Option<u64>) -> String {
    match t {
        None => "-".into(),
        Some(t) if t >= 1_000_000 => format!("{:.1}M", t as f64 / 1e6),
        Some(t) if t >= 1_000 => format!("{:.1}k", t as f64 / 1e3),
        Some(t) => t.to_string(),
    }
}

/// The list's text: a line a thread.
pub(super) fn text(list: &ThreadListResult) -> String {
    if list.threads.is_empty() {
        return format!(
            "No threads here ({} in all). `/thread new` makes one.",
            list.total
        );
    }
    let mut out = format!(
        "**Threads** · {} shown of {}",
        list.threads.len(),
        list.total
    );
    let mut shown = 0;
    for t in &list.threads {
        let line = format!(
            "\n{} `{}` · {} · <t:{}:R> · context {} · ${:.2} today · {}",
            match t.thread.state.as_str() {
                "active" => "🧵",
                "paused" => "⏸️",
                _ => "✅",
            },
            t.thread.slug,
            t.thread.state.as_str(),
            t.thread.last_active_ms / 1000,
            tokens(t.context_tokens),
            t.today_usd,
            t.thread.title,
        );
        if out.len() + line.len() > MAX_TEXT {
            break;
        }
        out.push_str(&line);
        shown += 1;
    }
    if shown < list.threads.len() {
        out.push_str(&format!(
            "\n…and {} more: `theseus thread list` lists them all.",
            list.threads.len() - shown
        ));
    }
    out
}

/// What a new thread said.
fn made(r: &ThreadRow) -> String {
    format!(
        "🧵 Made the thread `{}`: `theseus ask --thread {}` opens its session.",
        r.thread.slug, r.thread.slug
    )
}

/// The `/thread` subcommand and its string options.
fn parsed(options: &[CommandDataOption]) -> Option<(&str, Vec<(&str, String)>)> {
    let sub = options.first()?;
    let CommandOptionValue::SubCommand(inner) = &sub.value else {
        return None;
    };
    let args = inner
        .iter()
        .filter_map(|o| match &o.value {
            CommandOptionValue::String(s) => Some((o.name.as_str(), s.clone())),
            _ => None,
        })
        .collect();
    Some((sub.name.as_str(), args))
}

impl Shared {
    /// An interaction of `/thread`: true when it was one, and answered.
    pub(super) async fn thread_interaction(
        &self,
        i: &Interaction,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        let Some(InteractionData::ApplicationCommand(c)) = &i.data else {
            return false;
        };
        if c.name != "thread" {
            return false;
        }
        self.core.binding_ledger(
            LedgerKind::DiscordCommand,
            None,
            serde_json::json!({"command": c.name, "by": who}),
        );
        let said = match parsed(&c.options) {
            Some(("new", args)) => {
                let get = |k: &str| args.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
                let p = ThreadNewParams {
                    slug: get("slug").unwrap_or_default(),
                    title: get("title"),
                    author: Some(who.to_string()),
                    discord,
                    ..Default::default()
                };
                match self.rpc.call::<_, ThreadRow>(method::THREAD_NEW, p).await {
                    Ok(r) => made(&r),
                    Err(e) => format!("⚠️ No thread was made: {}", e.message),
                }
            }
            Some(("list", _)) => {
                let p = ThreadListParams {
                    author: Some(who.to_string()),
                    discord,
                    ..Default::default()
                };
                match self
                    .rpc
                    .call::<_, ThreadListResult>(method::THREAD_LIST, p)
                    .await
                {
                    Ok(list) => text(&list),
                    Err(e) => format!("⚠️ {}", e.message),
                }
            }
            _ => "⚠️ `/thread new slug:` or `/thread list`.".to_string(),
        };
        let resp = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(InteractionResponseData {
                content: Some(said),
                flags: Some(MessageFlags::EPHEMERAL),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            }),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("thread response", None, e);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::threads::{ThreadInfo, ThreadState, WorkingSet};

    fn row(slug: &str, state: ThreadState, tokens: Option<u64>, usd: f64) -> ThreadRow {
        ThreadRow {
            thread: ThreadInfo {
                slug: slug.into(),
                title: format!("Title of {slug}"),
                goal: String::new(),
                topics: vec![],
                working_set: WorkingSet::default(),
                card: vec![],
                persona: None,
                state,
                sealed: None,
                seal_pending: None,
                surfaces: vec![],
                parent: None,
                merged_into: None,
                split_from: None,
                session_id: None,
                last_active_ms: 1_790_000_000_000,
                created_at_ms: 1_790_000_000_000,
            },
            context_tokens: tokens,
            today_usd: usd,
        }
    }

    /// The command has the two subcommands, `new` with a required slug.
    #[test]
    fn the_command_has_new_and_list() {
        let c = command();
        assert_eq!(c.name, "thread");
        let names: Vec<&str> = c.options.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["new", "list"]);
    }

    /// The list says each thread's state, activity, context and spend, and
    /// stays within a message.
    #[test]
    fn the_list_says_each_thread_and_stays_in_a_message() {
        let list = ThreadListResult {
            threads: vec![
                row("tides", ThreadState::Active, Some(12_400), 0.31),
                row("moons", ThreadState::Paused, None, 0.0),
            ],
            total: 3,
        };
        let t = text(&list);
        assert!(t.starts_with("**Threads** · 2 shown of 3"), "{t}");
        assert!(t.contains("🧵 `tides` · active"), "{t}");
        assert!(
            t.contains("context 12.4k · $0.31 today · Title of tides"),
            "{t}"
        );
        assert!(t.contains("⏸️ `moons` · paused"), "{t}");
        let many = ThreadListResult {
            threads: (0..60)
                .map(|n| row(&format!("t{n:02}"), ThreadState::Active, Some(10), 0.0))
                .collect(),
            total: 60,
        };
        let t = text(&many);
        assert!(
            t.len() < 2000 && t.contains("more: `theseus thread list`"),
            "{}",
            t.len()
        );
        assert!(text(&ThreadListResult {
            threads: vec![],
            total: 0
        })
        .contains("No threads here"));
    }
}
