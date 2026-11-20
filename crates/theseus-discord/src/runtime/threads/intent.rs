//! A task's own thread or channel made at most once across a kill. A child of
//! `threads`; the same order as a forum post's (`forum/intent.rs`).
//!
//! The row of a task's thread (or channel) was written after Discord had the
//! place, so a kill in that moment lost the row and the restarted daemon made
//! the place again: two threads for one task, or a second private channel.
//!
//! 1. **The intent first.** Before the create, `discord.place_intents` (META)
//!    holds `(task, kind, parent, name, when)`, written durably.
//! 2. **The create**, then the row (`TaskThreads::add` and `save`), then the
//!    intent is taken off (`relieve`).
//! 3. **At the next make for the task**, before any create, an intent still on
//!    the list is reconciled (`Shared::settle_place_intent`): the task has a
//!    row (the kill came after it: the intent is only taken off), or Discord is
//!    read for the place and it is adopted when it is provably the bot's own and
//!    the task's, or none is there and the first is made.
//!
//! **Whose place.** A thread is one of the guild's active threads whose
//! `parent_id` is the channel it was made in, whose `owner_id` is the bot's own
//! and whose name begins with the name's head exactly as `task_name` builds it
//! (`<icon> <short> · `, for each mark). A channel is a text channel under the
//! tasks category (the one the bot recorded) with the exact name `channel_name`
//! gave it and the task's short id at the end of its topic. Discord gives a
//! channel no owner, so the rule there is the exact name, the category, the
//! kind and the topic: a member who can manage channels could imitate it, and
//! a thread is the stronger case. No place is adopted, or made, while the bot's
//! own id is unknown, and a read that fails makes nothing for that task.
//!
//! **A create that failed for sure** (Discord refused it) takes its own intent
//! off; one that may have gone (a timeout, a 5xx) keeps it.
//!
//! **What this does not do.** A thread made, left unrecorded, and archived
//! before the restart (a day of quiet) is not among the active threads and is
//! taken for not made. A create in flight at Discord that applies after the
//! read is the one race left, with a restart or without.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::work::WorkView;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;

use super::{Made, Mark, PlaceKind, TaskThreads, ARCHIVE};
use crate::render::board_v2::short;
use crate::runtime::task_channels::{Refused, CATEGORY_KEY};
use crate::runtime::Shared;

/// META key of the places being made.
const KEY: &str = "discord.place_intents";

/// How long an intent is kept: past this, a thread made and not recorded has
/// archived (a day of quiet), and a task that never asked again is not owed one.
const STALE_MS: u64 = 3 * 24 * 3_600_000;

/// A place about to be made, or made and not yet recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Intent {
    pub task: String,
    pub kind: PlaceKind,
    /// The channel a thread is made in; the guild a channel is made in.
    pub parent: u64,
    /// A channel's name as it is made (a thread's changes only at its end).
    pub name: String,
    pub at_ms: u64,
}

/// The intents worth keeping at `now`: one whose place has its row, and one
/// older than `STALE_MS`, are not.
fn live(intents: Vec<Intent>, made: &[Made], now: u64) -> Vec<Intent> {
    intents
        .into_iter()
        .filter(|i| now < i.at_ms + STALE_MS)
        .filter(|i| !made.iter().any(|m| m.task == i.task && m.kind == i.kind))
        .collect()
}

impl TaskThreads {
    /// The intents the store holds, the stale ones left out (see `live`).
    pub(super) fn load_intents(&self, store: &theseus_core::store::Store, made: &[Made], now: u64) {
        let held: Vec<Intent> = store.get_meta(KEY).ok().flatten().unwrap_or_default();
        let kept = live(held.clone(), made, now);
        let changed = kept.len() != held.len();
        *self.intents.lock().unwrap() = kept;
        if changed {
            self.save_intents(store);
        }
    }

    fn save_intents(&self, store: &theseus_core::store::Store) {
        let list = self.intents.lock().unwrap().clone();
        if let Err(e) = store.put_meta(KEY, &list) {
            tracing::warn!(error = %format!("{e:#}"), "the places being made were not saved");
        }
    }

    /// The place of `task` is about to be made: written before it.
    fn intend(
        &self,
        store: &theseus_core::store::Store,
        task: &str,
        (kind, parent, name): (PlaceKind, u64, &str),
    ) {
        {
            let mut list = self.intents.lock().unwrap();
            list.retain(|i| !(i.task == task && i.kind == kind));
            list.push(Intent {
                task: task.to_string(),
                kind,
                parent,
                name: name.to_string(),
                at_ms: theseus_protocol::now_unix_ms(),
            });
        }
        self.save_intents(store);
    }

    /// The place is recorded, or was never made: the intent comes off.
    pub(in crate::runtime) fn relieve(
        &self,
        store: &theseus_core::store::Store,
        task: &str,
        kind: PlaceKind,
    ) {
        let had = {
            let mut list = self.intents.lock().unwrap();
            let before = list.len();
            list.retain(|i| !(i.task == task && i.kind == kind));
            list.len() != before
        };
        if had {
            self.save_intents(store);
        }
    }

    fn intent_of(&self, task: &str, kind: PlaceKind) -> Option<Intent> {
        self.intents
            .lock()
            .unwrap()
            .iter()
            .find(|i| i.task == task && i.kind == kind)
            .cloned()
    }
}

impl Shared {
    /// The task's thread, made at most once: an intent left by a kill is
    /// reconciled first (the thread Discord already has is the task's), else
    /// the intent is written and the thread made. Its id and its guild. The
    /// caller records the row and then `relieve`s the intent.
    pub(super) async fn thread_once(
        &self,
        parent: u64,
        task: &str,
        name: &str,
    ) -> anyhow::Result<(u64, Option<u64>)> {
        if let Some(found) = self.settle_place_intent(task, PlaceKind::Thread).await? {
            return Ok(found);
        }
        let store = &self.core.store;
        self.task_threads
            .intend(store, task, (PlaceKind::Thread, parent, name));
        let made = self.guild_thread_create(parent, name, "", ARCHIVE).await;
        if made.as_ref().is_err_and(made_nothing) {
            self.task_threads.relieve(store, task, PlaceKind::Thread);
        }
        made
    }

    /// The task's channel, made at most once (see `thread_once`). Its id.
    pub(in crate::runtime) async fn channel_once(
        &self,
        guild: u64,
        name: &str,
        v: &WorkView,
    ) -> Result<u64, Refused> {
        match self.settle_place_intent(&v.id, PlaceKind::Channel).await {
            Ok(Some((found, _))) => return Ok(found),
            Ok(None) => {}
            Err(e) => return Err(Refused::Other(format!("{e:#}"))),
        }
        let store = &self.core.store;
        self.task_threads
            .intend(store, &v.id, (PlaceKind::Channel, guild, name));
        let made = self.task_channel_create(guild, name, v).await;
        if matches!(made, Err(Refused::Forbidden | Refused::Other(_))) {
            self.task_threads.relieve(store, &v.id, PlaceKind::Channel);
        }
        made
    }

    /// An intent left by a place whose recording a kill may have cut off,
    /// resolved before any is made for the task: see the module's notes.
    /// `Some`: the place Discord has (the intent stays until its row is
    /// written); `None`: none is there, or the task has its row. An error is a
    /// read that did not go through.
    async fn settle_place_intent(
        &self,
        task: &str,
        kind: PlaceKind,
    ) -> anyhow::Result<Option<(u64, Option<u64>)>> {
        let Some(intent) = self.task_threads.intent_of(task, kind) else {
            return Ok(None);
        };
        let store = &self.core.store;
        if self
            .task_threads
            .all()
            .iter()
            .any(|m| m.task == task && m.kind == kind)
        {
            self.task_threads.relieve(store, task, kind);
            return Ok(None);
        }
        // Until the bot knows its own id a place of anyone's could be taken
        // for its own: nothing is adopted, and the caller tries again.
        anyhow::ensure!(self.bot_id() != 0, "the bot's own id is not known yet");
        let found = match kind {
            PlaceKind::Thread => self.find_thread(&intent).await?,
            PlaceKind::Channel => self
                .find_channel(&intent)
                .await?
                .map(|c| (c, Some(intent.parent))),
        };
        match found {
            Some((place, _)) => tracing::info!(
                place,
                "a place made before a stop was recorded, found again: no second one"
            ),
            None => tracing::info!("a place meant before a stop was never made: it is made now"),
        }
        Ok(found)
    }

    /// The thread of the intent among the guild's active threads: the bot's
    /// own, in the channel it was made in, with the name's head as `task_name`
    /// builds it.
    async fn find_thread(&self, intent: &Intent) -> anyhow::Result<Option<(u64, Option<u64>)>> {
        let parent = self
            .http
            .channel(Id::new(intent.parent))
            .await?
            .model()
            .await?;
        let Some(guild) = parent.guild_id else {
            return Ok(None);
        };
        let req = Request::builder(&HttpRoute::GetActiveThreads {
            guild_id: guild.get(),
        })
        .build()?;
        let active: Value = self.http.request::<Value>(req).await?.model().await?;
        let bot = self.bot_id();
        let heads: Vec<String> = [Mark::Running, Mark::Done, Mark::Failed]
            .iter()
            .map(|m| format!("{} {} · ", m.icon(), short(&intent.task)))
            .collect();
        Ok(active["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                t["owner_id"].as_str().and_then(|o| o.parse::<u64>().ok()) == Some(bot)
                    && t["parent_id"].as_str().and_then(|p| p.parse::<u64>().ok())
                        == Some(intent.parent)
                    && t["name"]
                        .as_str()
                        .is_some_and(|n| heads.iter().any(|h| n.starts_with(h.as_str())))
            })
            .filter_map(|t| t["id"].as_str()?.parse::<u64>().ok())
            .min()
            .map(|t| (t, Some(guild.get()))))
    }

    /// The channel of the intent among the guild's: a text channel under the
    /// tasks category, of the exact name it was made with, whose topic ends
    /// with the task's short id. None while the category is not recorded.
    async fn find_channel(&self, intent: &Intent) -> anyhow::Result<Option<u64>> {
        let recorded = (self.core.store.get_meta::<u64>(CATEGORY_KEY))
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let Some(category) = recorded else {
            return Ok(None);
        };
        let marker = format!("…{}", short(&intent.task));
        let all = self
            .http
            .guild_channels(Id::new(intent.parent))
            .await?
            .models()
            .await?;
        Ok(all
            .iter()
            .filter(|c| c.kind == ChannelType::GuildText)
            .filter(|c| c.parent_id == Some(Id::new(category)))
            .filter(|c| c.name.as_deref() == Some(intent.name.as_str()))
            .filter(|c| c.topic.as_deref().is_some_and(|t| t.ends_with(&marker)))
            .map(|c| c.id.get())
            .min())
    }
}

/// Whether a create that failed with `e` certainly made nothing: Discord
/// refused it (a 4xx that is no rate limit), or it never went out. Anything else
/// (a timeout, a 5xx, a body that did not read) may have made the place.
fn made_nothing(e: &anyhow::Error) -> bool {
    e.downcast_ref::<twilight_http::Error>().is_some_and(|e| {
        let why = crate::courier::SendErr::of(e);
        !why.away && !why.unsure
    })
}
