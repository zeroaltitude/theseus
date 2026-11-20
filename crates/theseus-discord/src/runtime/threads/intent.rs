//! A task's own thread or channel made at most once across a kill. A child of
//! `threads`.
//!
//! The row of a task's thread (or channel) was written after Discord had the
//! place, so a kill in that moment lost the row and the restarted daemon made
//! the place again: two threads for one task, or a second private channel. The
//! order is the guard, as for a forum post (`forum/intent.rs`):
//!
//! 1. **The intent first.** Before the create, `discord.place_intents` (META)
//!    holds `(task, kind, parent, marker, name, when)`, written durably.
//! 2. **The create**, then the row (`TaskThreads::add` and `save`), then the
//!    intent is taken off (`relieve`).
//! 3. **At the next make for the task**, before any create, an intent still on
//!    the list is reconciled (`Shared::settle_place_intent`): the task has a
//!    row (the kill came after it: the intent is only taken off), or Discord is
//!    read for the place (the kill came after Discord had it: it is the task's,
//!    and the make goes on from it), or none is there (the create never
//!    arrived, or truly failed: the place is made).
//!
//! **How a place is found.** A thread by the guild's active threads: the bot's
//! own, in the channel it was made in, whose name holds the task's marker
//! (`<short> · `, which `task_name` puts in every name whatever mark it takes).
//! A channel by its own name (`channel_name`, exact) and the task's short id at
//! the end of its topic, among the guild's channels. A thread a member made
//! with the marker is never adopted (the owner is the bot's); no place is
//! adopted, or made, while the bot's own id is unknown.
//!
//! **A read that fails is not an answer.** Nothing is made until it has gone
//! through, so a task with a place never has a second. A thread's failed read
//! is a failed create (the task is tried again after `RETRY`); a channel's is
//! `Refused::Unsure`, which leaves the task with no thread meanwhile. A create
//! that failed for sure (Discord refused it) takes its own intent off; one that
//! may have gone (a timeout, a 5xx) keeps it.
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

use super::{Made, PlaceKind, TaskThreads, ARCHIVE};
use crate::render::board_v2::short;
use crate::runtime::task_channels::Refused;
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
    /// What the place carries whatever else it takes: a thread's name holds
    /// `<short> · `; a channel's topic ends with `…<short>`.
    pub marker: String,
    /// A channel's name as it is made (a thread's changes only at its end).
    pub name: String,
    pub at_ms: u64,
}

/// What a task's thread carries in its name, whatever mark it later takes.
pub(super) fn thread_marker(task: &str) -> String {
    format!("{} · ", short(task))
}

/// What a task's channel carries at the end of its topic (`task_channels::topic`).
pub(super) fn channel_marker(task: &str) -> String {
    format!("…{}", short(task))
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
    pub(super) fn intend(
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
                marker: match kind {
                    PlaceKind::Thread => thread_marker(task),
                    PlaceKind::Channel => channel_marker(task),
                },
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

    pub(super) fn intent_of(&self, task: &str, kind: PlaceKind) -> Option<Intent> {
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
            Err(e) => return Err(Refused::Unsure(format!("{e:#}"))),
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
    /// own, in the channel it was made in, carrying the marker in its name.
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
        Ok(active["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                // The bot's own thread, in that channel: a member's thread that
                // happens to carry the marker is never adopted.
                t["owner_id"].as_str().and_then(|o| o.parse::<u64>().ok()) == Some(bot)
                    && t["parent_id"].as_str().and_then(|p| p.parse::<u64>().ok())
                        == Some(intent.parent)
                    && t["name"]
                        .as_str()
                        .is_some_and(|n| n.contains(&intent.marker))
            })
            .filter_map(|t| t["id"].as_str()?.parse::<u64>().ok())
            .min()
            .map(|t| (t, Some(guild.get()))))
    }

    /// The channel of the intent among the guild's: a text channel of the name
    /// it was made with, whose topic ends with the task's marker.
    async fn find_channel(&self, intent: &Intent) -> anyhow::Result<Option<u64>> {
        let all = self
            .http
            .guild_channels(Id::new(intent.parent))
            .await?
            .models()
            .await?;
        Ok(all
            .iter()
            .filter(|c| c.kind == ChannelType::GuildText)
            .filter(|c| c.name.as_deref() == Some(intent.name.as_str()))
            .filter(|c| {
                c.topic
                    .as_deref()
                    .is_some_and(|t| t.ends_with(&intent.marker))
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn made(task: &str, kind: PlaceKind) -> Made {
        Made {
            task: task.into(),
            session: "ses".into(),
            thread: 1,
            kind,
            parent: 2,
            parent_session: "p".into(),
            name: "n".into(),
            guild: Some(3),
            private: false,
            ceiling: None,
            created_ms: 0,
            ended_ms: None,
        }
    }

    fn intent(task: &str, kind: PlaceKind, at_ms: u64) -> Intent {
        Intent {
            task: task.into(),
            kind,
            parent: 2,
            marker: String::new(),
            name: String::new(),
            at_ms,
        }
    }

    #[test]
    fn a_marker_is_the_tasks_short_id_where_a_thread_and_a_channel_carry_it() {
        use super::super::{task_name, Mark};
        let id = "exe_01J0aaaaaaaaaaaaaaaaaaaaab12cd";
        assert_eq!(thread_marker(id), "ab12cd · ");
        assert_eq!(channel_marker(id), "…ab12cd");
        // The names a thread has, whatever mark it takes, and the topic a channel has.
        for name in [
            task_name(Mark::Running, id, "Tide check"),
            task_name(Mark::Done, id, "Tide check"),
        ] {
            assert!(name.contains(&thread_marker(id)), "{name}");
        }
        let topic = crate::runtime::task_channels::topic(id, "Tide check");
        assert!(topic.ends_with(&channel_marker(id)), "{topic}");
        assert!(!"cd34ef · Tide check".contains(&thread_marker(id)));
    }

    #[test]
    fn a_stale_intent_is_dropped_at_load_one_with_its_row_and_one_long_gone() {
        let now = 10 * STALE_MS;
        let kept = live(
            vec![
                intent("exe_a", PlaceKind::Thread, now - 1_000),
                intent("exe_b", PlaceKind::Thread, now - 1_000),
                intent("exe_c", PlaceKind::Thread, now - STALE_MS - 1),
                // A thread's row is not the channel's: the channel's intent stays.
                intent("exe_b", PlaceKind::Channel, now - 1_000),
            ],
            &[made("exe_b", PlaceKind::Thread)],
            now,
        );
        assert_eq!(
            kept,
            [
                intent("exe_a", PlaceKind::Thread, now - 1_000),
                intent("exe_b", PlaceKind::Channel, now - 1_000),
            ]
        );
    }

    // ---- a kill in the window, on the stand-in -------------------------------
    //
    // A binding's state over one store and one Discord: `restarted` is a fresh
    // `Shared` (nothing in memory) that loads what the store holds, as a daemon
    // started again does. A create cut off in flight (`abort`) is the kill: the
    // call dies where the stand-in holds it, before or after Discord has the
    // place, with the intent written and no row.

    use std::sync::Arc;
    use std::time::Duration;

    use theseus_core::provider::FakeProvider;
    use theseus_sim::fake_discord::{Guild, HoldAt, Mode, BOT_ID, DEFAULT_GUILD};

    use crate::runtime::task_channels::{channel_name, work_view};
    use crate::tests_gateway::{Rig, ANA, BEN, CY, LAB};

    const TASK: &str = "exe_01J000000000000000task01";

    async fn over_the_fake() -> Rig {
        let guild = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
            .member(BEN, "ben")
            .member(CY, "cy")
            .private_channel(LAB, "lab", &[ANA, BEN, CY]);
        Rig::start_on(
            |_, _| Arc::new(FakeProvider::scripted(vec![])),
            guild,
            &format!(
                "guild_id = \"{DEFAULT_GUILD}\"\n[[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nprivate = true\n\
                 [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
            ),
            &[],
        )
        .await
    }

    /// A binding's state with nothing in memory, over the store as it is.
    fn restarted(r: &Rig) -> Arc<Shared> {
        let s = crate::runtime::shared_for_tests(&r.core);
        s.set_bot_id_for_tests(BOT_ID);
        s.task_threads.load(&r.core.store);
        s
    }

    fn threads_in_lab(r: &Rig) -> Vec<u64> {
        r.fake
            .threads()
            .into_iter()
            .filter(|t| t.parent == LAB)
            .map(|t| t.id)
            .collect()
    }

    fn text_channels(r: &Rig) -> Vec<u64> {
        r.fake
            .made_channels()
            .into_iter()
            .filter(|c| c.kind == 0 && !c.deleted)
            .map(|c| c.id)
            .collect()
    }

    fn a_thread_name() -> String {
        super::super::task_name(super::super::Mark::Running, TASK, "Chart the reef")
    }

    /// The make of a thread, held at `at` and cut off there: the kill.
    async fn killed_making_the_thread(r: &Rig, at: HoldAt) {
        r.fake.hold_thread_creates(Some(at));
        let doomed = restarted(r);
        let call = tokio::spawn(async move {
            let _ = doomed.thread_once(LAB, TASK, &a_thread_name()).await;
        });
        for _ in 0..1_000 {
            if r.fake.held_thread_creates() >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(r.fake.held_thread_creates(), 1, "the create is held");
        call.abort();
        let _ = call.await;
        r.fake.hold_thread_creates(None);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    /// The kill came after Discord had the thread and before its row: the
    /// thread is found by its marker and is the task's, no second is made.
    #[tokio::test]
    async fn a_thread_made_at_discord_before_the_kill_is_found_not_made_again() {
        let r = over_the_fake().await;
        killed_making_the_thread(&r, HoldAt::AfterApply).await;
        let first = threads_in_lab(&r);
        assert_eq!(first.len(), 1, "Discord has the thread");
        let s = restarted(&r);
        assert!(s.task_threads.intent_of(TASK, PlaceKind::Thread).is_some());
        let (found, guild) = s.thread_once(LAB, TASK, &a_thread_name()).await.unwrap();
        assert_eq!(found, first[0], "the task's own thread");
        assert_eq!(guild, Some(DEFAULT_GUILD));
        assert_eq!(threads_in_lab(&r).len(), 1, "no second thread");
    }

    /// The kill came before the create reached Discord: none is found, and the
    /// task's first thread is made.
    #[tokio::test]
    async fn a_thread_whose_create_never_arrived_is_made_once_after_the_kill() {
        let r = over_the_fake().await;
        killed_making_the_thread(&r, HoldAt::BeforeApply).await;
        assert!(threads_in_lab(&r).is_empty(), "Discord never had it");
        let s = restarted(&r);
        let (made, _) = s.thread_once(LAB, TASK, &a_thread_name()).await.unwrap();
        assert_eq!(threads_in_lab(&r), [made], "one thread");
    }

    /// The kill came after the row: the intent is only taken off, and the make
    /// goes on as one with no intent.
    #[tokio::test]
    async fn an_intent_whose_row_was_written_is_only_taken_off() {
        let r = over_the_fake().await;
        let s = restarted(&r);
        let (thread, guild) = s.thread_once(LAB, TASK, &a_thread_name()).await.unwrap();
        s.task_threads.add(Made {
            guild,
            thread,
            created_ms: theseus_protocol::now_unix_ms(),
            ..made(TASK, PlaceKind::Thread)
        });
        s.task_threads.save(&r.core.store);
        assert!(s.task_threads.intent_of(TASK, PlaceKind::Thread).is_some());
        let again = restarted(&r);
        assert!(
            again
                .task_threads
                .intent_of(TASK, PlaceKind::Thread)
                .is_none(),
            "taken off at load: its row is there"
        );
        assert_eq!(threads_in_lab(&r), [thread]);
    }

    /// A thread a member made, named with the marker, is never adopted: it is
    /// left as it is, and the bot's own thread is made.
    #[tokio::test]
    async fn a_members_thread_named_with_the_marker_is_left_alone_and_the_bots_is_made() {
        let r = over_the_fake().await;
        killed_making_the_thread(&r, HoldAt::BeforeApply).await;
        let theirs =
            r.fake
                .member_thread(LAB, BEN, &format!("{}Chart the reef", thread_marker(TASK)));
        let s = restarted(&r);
        let (made, _) = s.thread_once(LAB, TASK, &a_thread_name()).await.unwrap();
        assert_ne!(made, theirs, "the bot's own thread");
        assert_eq!(threads_in_lab(&r).len(), 2);
    }

    /// While the bot's own id is unknown nothing is adopted and nothing is
    /// made: an error, and a retry once it is known finds the thread.
    #[tokio::test]
    async fn no_thread_is_adopted_or_made_while_the_bots_own_id_is_unknown() {
        let r = over_the_fake().await;
        killed_making_the_thread(&r, HoldAt::AfterApply).await;
        let s = restarted(&r);
        s.set_bot_id_for_tests(0);
        assert!(s.thread_once(LAB, TASK, &a_thread_name()).await.is_err());
        assert_eq!(threads_in_lab(&r).len(), 1, "nothing made");
        s.set_bot_id_for_tests(BOT_ID);
        let (found, _) = s.thread_once(LAB, TASK, &a_thread_name()).await.unwrap();
        assert_eq!(threads_in_lab(&r), [found], "found, not made again");
    }

    /// A read that fails is no answer: no thread is made until it goes through.
    #[tokio::test]
    async fn no_thread_is_made_while_the_guild_cannot_be_read_for_an_intent() {
        let r = over_the_fake().await;
        killed_making_the_thread(&r, HoldAt::AfterApply).await;
        let s = restarted(&r);
        r.fake.set_mode(Mode::Fail);
        assert!(s.thread_once(LAB, TASK, &a_thread_name()).await.is_err());
        r.fake.set_mode(Mode::Up);
        assert_eq!(threads_in_lab(&r).len(), 1, "nothing made blind");
        let (found, _) = s.thread_once(LAB, TASK, &a_thread_name()).await.unwrap();
        assert_eq!(threads_in_lab(&r), [found]);
    }

    /// A create that may have gone (503) keeps its intent: the next try reads
    /// before it makes anything.
    #[tokio::test]
    async fn a_thread_create_that_may_have_gone_keeps_its_intent() {
        let r = over_the_fake().await;
        let s = restarted(&r);
        r.fake.set_mode(Mode::Fail);
        assert!(s.thread_once(LAB, TASK, &a_thread_name()).await.is_err());
        r.fake.set_mode(Mode::Up);
        assert!(s.task_threads.intent_of(TASK, PlaceKind::Thread).is_some());
    }

    /// The make of a channel, held at `at` and cut off there.
    async fn killed_making_the_channel(r: &Rig, at: HoldAt) -> String {
        let v = work_view(TASK, "running");
        let name = channel_name(super::super::Mark::Running, TASK, &v.title);
        r.fake.hold_channel_creates(Some(at));
        let doomed = restarted(r);
        let (n, w) = (name.clone(), v.clone());
        let call = tokio::spawn(async move {
            let _ = doomed.channel_once(DEFAULT_GUILD, &n, &w).await;
        });
        for _ in 0..1_000 {
            if r.fake.held_channel_creates() >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(r.fake.held_channel_creates(), 1, "the create is held");
        call.abort();
        let _ = call.await;
        r.fake.hold_channel_creates(None);
        tokio::time::sleep(Duration::from_millis(100)).await;
        name
    }

    /// The kill came after Discord had the channel and before its row: it is
    /// found by its name and the task's marker, and no second is made.
    #[tokio::test]
    async fn a_channel_made_at_discord_before_the_kill_is_found_not_made_again() {
        let r = over_the_fake().await;
        let name = killed_making_the_channel(&r, HoldAt::AfterApply).await;
        let first = text_channels(&r);
        assert_eq!(first.len(), 1, "Discord has the channel");
        let s = restarted(&r);
        assert!(s.task_threads.intent_of(TASK, PlaceKind::Channel).is_some());
        let found = s
            .channel_once(DEFAULT_GUILD, &name, &work_view(TASK, "running"))
            .await
            .unwrap();
        assert_eq!(found, first[0], "the task's own channel");
        assert_eq!(text_channels(&r).len(), 1, "no second channel");
    }

    /// The kill came before the create reached Discord: the task's first
    /// channel is made, and only one.
    #[tokio::test]
    async fn a_channel_whose_create_never_arrived_is_made_once_after_the_kill() {
        let r = over_the_fake().await;
        let name = killed_making_the_channel(&r, HoldAt::BeforeApply).await;
        assert!(text_channels(&r).is_empty(), "Discord never had it");
        let s = restarted(&r);
        let made = s
            .channel_once(DEFAULT_GUILD, &name, &work_view(TASK, "running"))
            .await
            .unwrap();
        assert_eq!(text_channels(&r), [made], "one channel");
    }

    /// A channel of the same name made for another task (another topic) is not
    /// the task's: the marker in the topic tells them apart.
    #[tokio::test]
    async fn a_channel_of_the_same_name_for_another_task_is_not_adopted() {
        let r = over_the_fake().await;
        let name = killed_making_the_channel(&r, HoldAt::BeforeApply).await;
        let s = restarted(&r);
        let other = work_view("exe_01J000000000000000other1", "running");
        let theirs = s
            .task_channel_create(DEFAULT_GUILD, &name, &other)
            .await
            .unwrap();
        let made = s
            .channel_once(DEFAULT_GUILD, &name, &work_view(TASK, "running"))
            .await
            .unwrap();
        assert_ne!(made, theirs, "its own, not the other task's");
        assert_eq!(text_channels(&r).len(), 2);
    }

    /// A refusal (403) made nothing: the intent comes off at once. A create
    /// that may have gone keeps it, and is no failure to fall back on.
    #[tokio::test]
    async fn a_refused_channel_create_takes_its_intent_off_and_one_that_may_have_gone_keeps_it() {
        let r = over_the_fake().await;
        let name = channel_name(super::super::Mark::Running, TASK, "Chart the reef");
        let s = restarted(&r);
        r.fake.refuse_channels(true);
        let refused = s
            .channel_once(DEFAULT_GUILD, &name, &work_view(TASK, "running"))
            .await;
        assert!(matches!(refused, Err(Refused::Forbidden)), "{refused:?}");
        assert!(s.task_threads.intent_of(TASK, PlaceKind::Channel).is_none());
        r.fake.refuse_channels(false);
        // The category is made first, then the channel's create fails 503.
        s.channel_once(
            DEFAULT_GUILD,
            &name,
            &work_view("exe_01J000000000000000other1", "running"),
        )
        .await
        .unwrap();
        r.fake.set_mode(Mode::Fail);
        let unsure = s
            .channel_once(DEFAULT_GUILD, &name, &work_view(TASK, "running"))
            .await;
        r.fake.set_mode(Mode::Up);
        assert!(matches!(unsure, Err(Refused::Unsure(_))), "{unsure:?}");
        assert!(s.task_threads.intent_of(TASK, PlaceKind::Channel).is_some());
    }
}
