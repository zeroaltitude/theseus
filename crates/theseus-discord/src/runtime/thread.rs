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
use theseus_protocol::threads::{
    ThreadListParams, ThreadListResult, ThreadMergeParams, ThreadMergeResult, ThreadNewParams,
    ThreadRow,
};
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

use std::sync::Arc;

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
    .option(
        SubCommandBuilder::new("bind", "Run this channel, or Discord thread, as a thread")
            .option(StringBuilder::new("slug", "The thread this place runs").required(true)),
    )
    .option(SubCommandBuilder::new(
        "unbind",
        "Stop running a thread here; the place has its own session again",
    ))
    .option(
        SubCommandBuilder::new("merge", "Fold one thread into another; nothing is deleted")
            .option(StringBuilder::new("from", "The thread to retire").required(true))
            .option(StringBuilder::new("into", "The thread that takes it").required(true)),
    )
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
            "\n{} `{}`{} · {} · <t:{}:R> · context {} · ${:.2} today · {}",
            match t.thread.state.as_str() {
                "active" => "🧵",
                "paused" => "⏸️",
                _ => "✅",
            },
            t.thread.slug,
            match (&t.thread.sealed, &t.thread.seal_pending) {
                (Some(_), _) => " 🔒",
                (None, Some(_)) => " 🔒?",
                _ => "",
            },
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

/// What a merge said.
fn merged(r: &ThreadMergeResult) -> String {
    let mut out = format!(
        "🧵 Merged `{}` into `{}`: `{}` is retired, nothing of it is deleted, and its card is \
         quoted in `{}`'s. `theseus thread unmerge {}` undoes it.",
        r.from.thread.slug,
        r.into.thread.slug,
        r.from.thread.slug,
        r.into.thread.slug,
        r.from.thread.slug
    );
    if !r.topics_left_out.is_empty() {
        out.push_str(&format!(
            "\nThree topics fit; left out: {}.",
            r.topics_left_out.join(", ")
        ));
    }
    out
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
    /// An interaction, from the gateway: `/thread bind` typed in a Discord
    /// thread that is not a place yet, under one that is (or under `[threads]
    /// discord_channel`), is answered here (theseus-sjzm; the core judges the
    /// presser as it does everywhere); every other goes to its place.
    pub(super) async fn interacted(self: Arc<Self>, i: Interaction) {
        if !self.unplaced_thread_bind(&i).await {
            self.on_interaction(i).await;
        }
    }

    async fn unplaced_thread_bind(self: &Arc<Self>, i: &Interaction) -> bool {
        let Some(InteractionData::ApplicationCommand(c)) = &i.data else {
            return false;
        };
        let bind = parsed(&c.options).is_some_and(|(sub, _)| sub == "bind");
        let channel = i.channel.as_ref().map(|c| c.id.get());
        let parent = i
            .channel
            .as_ref()
            .and_then(|c| c.parent_id)
            .map(|p| p.get());
        let (placed, under) = {
            let r = self.routes.lock().unwrap();
            let placed = channel.is_some_and(|c| r.by_channel.contains_key(&c));
            let under = parent.is_some_and(|p| {
                r.by_channel.contains_key(&p)
                    || self.core.cfg.threads.discord_channel.as_deref() == Some(&p.to_string())
            });
            (placed, under)
        };
        if c.name != "thread" || !bind || placed || !under {
            return false;
        }
        let user = i.author().map(|u| (u.id.get(), u.name.clone()));
        let who = user
            .as_ref()
            .map_or("discord".into(), |(_, n)| format!("discord:{n}"));
        let origin = super::origin_of(user.map(|(u, _)| u), channel, i.guild_id);
        self.thread_interaction(i, &who, origin).await
    }

    /// An interaction of `/thread`: true when it was one, and answered.
    pub(super) async fn thread_interaction(
        self: &Arc<Self>,
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
            Some(("bind", args)) => {
                let slug = args
                    .iter()
                    .find(|(n, _)| *n == "slug")
                    .map(|(_, v)| v.as_str());
                let place = i
                    .channel
                    .as_ref()
                    .map(|c| (c.id.get(), c.parent_id.map(|p| p.get())));
                self.thread_bind(slug.unwrap_or(""), discord, who, place)
                    .await
            }
            Some(("unbind", _)) => {
                let channel = i.channel.as_ref().map(|c| c.id.get());
                self.thread_unbind(discord, who, channel).await
            }
            Some(("merge", args)) => {
                let get = |k: &str| args.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
                let p = ThreadMergeParams {
                    from: get("from").unwrap_or_default(),
                    into: get("into").unwrap_or_default(),
                    author: Some(who.to_string()),
                    discord,
                };
                match self
                    .rpc
                    .call::<_, ThreadMergeResult>(method::THREAD_MERGE, p)
                    .await
                {
                    Ok(r) => merged(&r),
                    Err(e) => format!("⚠️ Nothing was merged: {}", e.message),
                }
            }
            _ => "⚠️ `/thread new`, `list`, `bind slug:`, `unbind` or `merge from: into:`."
                .to_string(),
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
            cache_ttl_secs: None,
            cache: None,
            merged_from: vec![],
            split_to: vec![],
        }
    }

    /// The command has its subcommands, `new` and `bind` with a required slug.
    #[test]
    fn the_command_has_new_list_and_merge() {
        let c = command();
        assert_eq!(c.name, "thread");
        let names: Vec<&str> = c.options.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["new", "list", "bind", "unbind", "merge"]);
    }

    /// A sealed thread carries a lock in the list, and one with a seal
    /// proposed a lock and a question mark (theseus-f2g0).
    #[test]
    fn a_sealed_thread_carries_a_lock() {
        let mut sealed = row("fix", ThreadState::Active, None, 0.0);
        sealed.thread.sealed = Some(theseus_protocol::threads::Seal {
            by: "the CLI".into(),
            at_ms: 1,
            reason: "embargo".into(),
        });
        let mut pending = row("maybe", ThreadState::Active, None, 0.0);
        pending.thread.seal_pending = Some("looks embargoed".into());
        let t = text(&ThreadListResult {
            threads: vec![sealed, pending, row("open", ThreadState::Active, None, 0.0)],
            total: 3,
        });
        assert!(t.contains("`fix` 🔒 · active"), "{t}");
        assert!(t.contains("`maybe` 🔒? · active"), "{t}");
        assert!(t.contains("`open` · active"), "{t}");
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

    /// A merge says what was retired, where its card went, and how to undo.
    #[test]
    fn a_merge_says_what_was_retired_and_how_to_undo_it() {
        let r = ThreadMergeResult {
            from: row("io", ThreadState::Done, None, 0.0),
            into: row("tides", ThreadState::Active, None, 0.0),
            topics_left_out: vec!["gauges".into()],
        };
        let t = merged(&r);
        assert!(
            t.contains("Merged `io` into `tides`") && t.contains("`theseus thread unmerge io`"),
            "{t}"
        );
        assert!(t.contains("left out: gauges"), "{t}");
    }
}
