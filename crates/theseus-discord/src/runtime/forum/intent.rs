//! A forum post made at most once per item, across a kill at any point. A
//! child of `forum`.
//!
//! A post's row used to be written after Discord had the post, so a kill in
//! that moment lost the row and the restarted daemon made the post again. A
//! thread create has no nonce to rely on (a forum post is a channel, not a
//! message; the cards' nonce is a message's, and the real one's window closes),
//! so the order is the guard:
//!
//! 1. **The intent first.** Before the create, `discord.post_intents` (META)
//!    holds `(item, forum, marker, when)`, written durably.
//! 2. **The create**, then the post's row, then the intent is settled (taken
//!    off the list).
//! 3. **At the next create for the item**, before any, an intent that is still
//!    on the list is reconciled (`Worker::settle_intent`): the item has a row
//!    (the kill came after it: the intent is only taken off), or the forum's
//!    active threads are read for a post that carries the item's marker (the
//!    kill came after Discord had the post: the post is the item's, and its row
//!    is written from it), or none does (the create never arrived, or truly
//!    failed: the intent is taken off and the first post is made).
//!
//! **The marker** is what every post already carries: the item's short id at
//! the head of its name (`post_name`, `task_name`: `<short> · <title>`, with a
//! mark before it once the item ends). A read that fails is not an answer: no
//! post is made until it has gone through, so an item with a post never has a
//! second. A create that failed for sure (Discord refused it) takes its own
//! intent off, so it does not wait on a read; one that may have gone (a
//! timeout, a 5xx) keeps it.
//!
//! **What this does not do.** A post that was made, left unrecorded, and
//! archived before the restart (a day of quiet: the post's auto archive) is
//! not among the active threads and is taken for not made.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;

use super::{Post, TaskPosts, Worker};
use crate::render::board_v2::short;

/// META key of the posts being made.
const KEY: &str = "discord.post_intents";

/// A post about to be made, or made and not yet recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Intent {
    pub task: String,
    pub forum: u64,
    /// What the post's name holds whatever mark it takes: `<short> · `.
    pub marker: String,
    pub at_ms: u64,
}

/// What an item's post carries in its name, whatever mark it later takes.
pub(super) fn marker(task: &str) -> String {
    format!("{} · ", short(task))
}

impl TaskPosts {
    pub(super) fn load_intents(&self, store: &theseus_core::store::Store) {
        *self.intents.lock().unwrap() = store.get_meta(KEY).ok().flatten().unwrap_or_default();
    }

    fn save_intents(&self, store: &theseus_core::store::Store) {
        let list = self.intents.lock().unwrap().clone();
        if let Err(e) = store.put_meta(KEY, &list) {
            tracing::warn!(error = %format!("{e:#}"), "the posts being made were not saved");
        }
    }

    /// The post of `task` in `forum` is about to be made: written before it.
    pub(super) fn intend(&self, store: &theseus_core::store::Store, task: &str, forum: u64) {
        {
            let mut list = self.intents.lock().unwrap();
            list.retain(|i| !(i.task == task && i.forum == forum));
            list.push(Intent {
                task: task.to_string(),
                forum,
                marker: marker(task),
                at_ms: theseus_protocol::now_unix_ms(),
            });
        }
        self.save_intents(store);
    }

    /// The post is recorded, or was never made: the intent comes off.
    pub(super) fn settle(&self, store: &theseus_core::store::Store, task: &str, forum: u64) {
        let had = {
            let mut list = self.intents.lock().unwrap();
            let before = list.len();
            list.retain(|i| !(i.task == task && i.forum == forum));
            list.len() != before
        };
        if had {
            self.save_intents(store);
        }
    }

    pub(super) fn intent_of(&self, task: &str, forum: u64) -> Option<Intent> {
        self.intents
            .lock()
            .unwrap()
            .iter()
            .find(|i| i.task == task && i.forum == forum)
            .cloned()
    }
}

impl Worker {
    /// An intent left by a post whose recording a kill may have cut off, resolved
    /// before any post is made for the item: see the module's notes. An error is
    /// a read that did not go through; the caller makes no post and tries again.
    pub(super) async fn settle_intent(
        &mut self,
        key: &(String, u64),
        guild: &str,
    ) -> anyhow::Result<()> {
        let s = self.shared.clone();
        let Some(intent) = s.task_posts.intent_of(&key.0, key.1) else {
            return Ok(());
        };
        let has_row = s
            .task_posts
            .posts
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.task == key.0 && p.forum == key.1);
        if has_row {
            s.task_posts.settle(&s.core.store, &key.0, key.1);
            return Ok(());
        }
        let found = self.find_post(guild, &intent).await?;
        match found {
            Some(post) => {
                tracing::info!(
                    forum = key.1,
                    post,
                    "a post made before a stop was recorded, found again: no second one"
                );
                s.task_posts.posts.lock().unwrap().push(Post {
                    task: key.0.clone(),
                    forum: key.1,
                    post,
                    guild: guild.to_string(),
                    created_ms: intent.at_ms,
                    ended_ms: None,
                    link: None,
                    bound: None,
                    read_ms: None,
                    archived_ms: None,
                });
                s.task_posts.save(&s.core.store);
            }
            None => tracing::info!(
                forum = key.1,
                "a post meant before a stop was never made: the first is made now"
            ),
        }
        s.task_posts.settle(&s.core.store, &key.0, key.1);
        Ok(())
    }

    /// The post of the intent among the guild's active threads: in its forum,
    /// carrying its marker in its name.
    async fn find_post(&self, guild: &str, intent: &Intent) -> anyhow::Result<Option<u64>> {
        let guild_id: u64 = guild.parse()?;
        let req = Request::builder(&HttpRoute::GetActiveThreads { guild_id }).build()?;
        let active: Value = self
            .shared
            .http
            .request::<Value>(req)
            .await?
            .model()
            .await?;
        Ok(active["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                t["parent_id"].as_str().and_then(|p| p.parse::<u64>().ok()) == Some(intent.forum)
                    && t["name"]
                        .as_str()
                        .is_some_and(|n| n.contains(&intent.marker))
            })
            .filter_map(|t| t["id"].as_str()?.parse::<u64>().ok())
            .min())
    }
}

/// Whether a create that failed with `e` certainly made nothing: Discord
/// refused it (a 4xx that is no rate limit), or it never went out. Anything else
/// (a timeout, a 5xx, a body that did not read) may have made the post.
pub(super) fn made_nothing(e: &anyhow::Error) -> bool {
    e.downcast_ref::<twilight_http::Error>().is_some_and(|e| {
        let why = crate::courier::SendErr::of(e);
        !why.away && !why.unsure
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_is_the_items_short_id_at_the_head_of_a_posts_name() {
        let id = "wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd";
        assert_eq!(marker(id), "ab12cd · ");
        // The name a post has, and the one it takes at its end.
        for name in [
            super::super::post_name(id, "Tide check"),
            format!("✅ {}", super::super::post_name(id, "Tide check")),
        ] {
            assert!(name.contains(&marker(id)), "{name}");
        }
        assert!(!"cd34ef · Tide check".contains(&marker(id)));
    }

    use super::super::tests::{worker_over_the_fake, FORUM};
    use super::super::{Seen, Worker};
    use crate::runtime::voice_status::tests::view;
    use std::collections::{HashMap, HashSet};

    const TASK: &str = "exe_01J000000000000000task01";

    fn tell(w: &mut Worker) {
        w.see(Seen {
            views: vec![view(TASK, "task", "running", "Chart the reef", None)],
            private: true,
            guild: None,
            links: HashMap::new(),
            home: None,
            homed: HashSet::new(),
            parent: None,
        });
    }

    fn posts(r: &crate::tests_gateway::Rig) -> usize {
        r.fake
            .threads()
            .into_iter()
            .filter(|t| t.parent == FORUM)
            .count()
    }

    fn rows(w: &Worker) -> usize {
        w.shared.task_posts.posts.lock().unwrap().len()
    }

    /// A post made, and then the kill: its row and the worker's memory gone,
    /// the intent still on the list, as the daemon's store holds them.
    fn killed_after_the_create(w: &mut Worker) {
        w.shared.task_posts.posts.lock().unwrap().clear();
        w.slots.clear();
        w.shared
            .task_posts
            .intend(&w.shared.core.store, TASK, FORUM);
    }

    /// The kill came after the row: the intent is only taken off.
    #[tokio::test]
    async fn an_intent_whose_row_was_written_is_only_taken_off() {
        let (r, mut w) = worker_over_the_fake().await;
        tell(&mut w);
        w.drive().await;
        assert_eq!((posts(&r), rows(&w)), (1, 1));
        w.shared
            .task_posts
            .intend(&w.shared.core.store, TASK, FORUM);
        w.slots.clear();
        w.drive().await;
        assert_eq!((posts(&r), rows(&w)), (1, 1), "no second post");
        assert!(w.shared.task_posts.intent_of(TASK, FORUM).is_none());
    }

    /// The kill came after Discord had the post and before its row: the post
    /// is found by its marker and is the item's, no second is made.
    #[tokio::test]
    async fn a_post_made_before_the_kill_and_not_recorded_is_found_not_made_again() {
        let (r, mut w) = worker_over_the_fake().await;
        tell(&mut w);
        w.drive().await;
        let first = r
            .fake
            .threads()
            .into_iter()
            .find(|t| t.parent == FORUM)
            .unwrap()
            .id;
        killed_after_the_create(&mut w);
        assert_eq!(rows(&w), 0);
        w.drive().await;
        assert_eq!(posts(&r), 1, "no second post");
        let held = w.shared.task_posts.posts.lock().unwrap().clone();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].post, first, "the row is the first post's");
        assert!(w.shared.task_posts.intent_of(TASK, FORUM).is_none());
    }

    /// The kill came before the create reached Discord: no post is found, and
    /// the item's first is made.
    #[tokio::test]
    async fn an_intent_whose_create_never_arrived_still_gets_its_first_post() {
        let (r, mut w) = worker_over_the_fake().await;
        w.shared
            .task_posts
            .intend(&w.shared.core.store, TASK, FORUM);
        tell(&mut w);
        w.drive().await;
        assert_eq!((posts(&r), rows(&w)), (1, 1));
        assert!(w.shared.task_posts.intent_of(TASK, FORUM).is_none());
    }

    /// A read that fails is no answer: no post is made until it goes through,
    /// and then the post that exists is found.
    #[tokio::test]
    async fn no_post_is_made_while_the_forum_cannot_be_read_for_an_intent() {
        use theseus_sim::fake_discord::Mode;
        let (r, mut w) = worker_over_the_fake().await;
        tell(&mut w);
        w.drive().await;
        killed_after_the_create(&mut w);
        r.fake.set_mode(Mode::Fail);
        w.drive().await;
        r.fake.set_mode(Mode::Up);
        assert_eq!(
            (posts(&r), rows(&w)),
            (1, 0),
            "nothing made, nothing recorded"
        );
        assert!(w.shared.task_posts.intent_of(TASK, FORUM).is_some());
        tokio::time::sleep(std::time::Duration::from_millis(1_300)).await;
        w.drive().await;
        assert_eq!((posts(&r), rows(&w)), (1, 1), "found, not made again");
    }

    /// A create that may have gone (Discord answered 503) keeps its intent: the
    /// next try reads the forum before it makes anything.
    #[tokio::test]
    async fn a_create_that_may_have_gone_keeps_its_intent() {
        use theseus_sim::fake_discord::Mode;
        let (r, mut w) = worker_over_the_fake().await;
        r.fake.set_mode(Mode::Fail);
        tell(&mut w);
        w.drive().await;
        r.fake.set_mode(Mode::Up);
        assert_eq!(posts(&r), 0);
        assert!(w.shared.task_posts.intent_of(TASK, FORUM).is_some());
    }
}
