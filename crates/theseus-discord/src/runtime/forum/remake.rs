//! A post found gone is made again at a pace, and the owner's delete of an
//! ended item's post is kept across a restart. A child of `forum`.
//!
//! - **The pace.** The first post of a key found gone is made again at once:
//!   that is the owner deleting it. A post found gone that had not stood for
//!   `STOOD` is one "in a row" with the one before (a rule or another bot that
//!   removes each new post, a forum that misanswers), and the wait before the
//!   next doubles: 2 s, 4 s, 8 s, up to the retry ceiling (`remake_wait`). After
//!   `MAX_REMAKES` posts made again that way the key is given up for this run,
//!   with one health line and one ledger row. The count lives in `Worker::remakes`
//!   and not in `failed`, which a success clears; it falls back to one when the
//!   post that was found gone had stood for `STOOD`.
//! - **The owner's delete of an ended item's post** is kept in META
//!   `discord.task_posts_gone` (the key and when), so a restart does not make
//!   the post again. A mark leaves when the item stands again, and a day after
//!   it was made (a finished item is shown a day: `DONE_SHOWN_MS`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use super::{Every, Shared, Worker, LINK_KEPT_MS};

/// META key of the posts the owner deleted after their item was over.
const GONE_KEY: &str = "discord.task_posts_gone";

/// The wait before the second post of a key is made again: it doubles from here.
pub(super) const REMAKE_FIRST: Duration = Duration::from_secs(2);

/// A post that stood this long and was then found gone was the owner's doing:
/// the count of posts found gone in a row starts again.
pub(super) const STOOD: Duration = Duration::from_secs(300);

/// A key whose post has been made again this many times in a row, each found
/// gone before it had stood `STOOD`, is given up for the run.
pub(super) const MAX_REMAKES: u32 = 5;

/// The posts of one key found gone, as the worker counts them.
pub(super) struct Remakes {
    /// How many in a row were found gone before they had stood `STOOD`.
    gone: u32,
    /// The next post is not made before this.
    not_before: Instant,
    /// No post is made again for the key this run.
    given_up: bool,
}

/// How long to wait before making the post of a key found gone `gone` times in
/// a row: none the first time, then `first`, doubling, up to `cap`.
pub(super) fn remake_wait(gone: u32, first: Duration, cap: Duration) -> Duration {
    if gone <= 1 {
        return Duration::ZERO;
    }
    first.saturating_mul(1u32 << (gone - 2).min(16)).min(cap)
}

impl Worker {
    /// A worker with nothing in hand, the owner's earlier deletes read back.
    pub(super) fn new(shared: Arc<Shared>, every: Every) -> Self {
        let deleted = load_gone(&shared);
        Self {
            shared,
            every,
            entries: HashMap::new(),
            deleted,
            slots: HashMap::new(),
            tags: HashMap::new(),
            failed: HashMap::new(),
            remakes: HashMap::new(),
        }
    }

    /// The post of `key`, made `made_ms`, was found gone while its item stands:
    /// how long to wait before it is made again, or None when the key is given
    /// up (said once, in health and the ledger, without a title).
    pub(super) fn remade_after(&mut self, key: &(String, u64), made_ms: u64) -> Option<Duration> {
        let stood = theseus_protocol::now_unix_ms().saturating_sub(made_ms)
            >= u64::try_from(self.every.stood.as_millis()).unwrap_or(u64::MAX);
        let r = self.remakes.entry(key.clone()).or_insert(Remakes {
            gone: 0,
            not_before: Instant::now(),
            given_up: false,
        });
        r.gone = if stood { 1 } else { r.gone.saturating_add(1) };
        if r.gone > MAX_REMAKES {
            r.given_up = true;
            let (forum, n) = (key.1, r.gone - 1);
            tracing::warn!(
                forum,
                n,
                "a post was deleted again and again: not made again"
            );
            self.shared.board.error(
                "task forum",
                None,
                format!(
                    "a post in forum {forum} was gone {n} times in a row soon after it was made: \
                     no more are made for it until Theseus restarts"
                ),
            );
            return None;
        }
        let wait = remake_wait(r.gone, self.every.remake, self.every.retry);
        r.not_before = Instant::now() + wait;
        Some(wait)
    }

    /// Whether a post of `key` may be made now: None when it may, Some(None)
    /// when it is given up, Some(Some(wait)) when its pace says to wait.
    pub(super) fn remake_gate(&self, key: &(String, u64)) -> Option<Option<Duration>> {
        let r = self.remakes.get(key)?;
        if r.given_up {
            return Some(None);
        }
        let now = Instant::now();
        (r.not_before > now).then(|| Some(r.not_before - now))
    }

    /// The counts of an item whose entry is gone leave with it, a key given up
    /// stays for the run.
    pub(super) fn forget_remakes(&mut self) {
        let entries = &self.entries;
        self.remakes
            .retain(|k, r| r.given_up || entries.contains_key(&k.0));
    }

    /// The owner deleted the post of an item that is over: no post is made
    /// again, now or after a restart.
    pub(super) fn mark_deleted(&mut self, key: &(String, u64)) {
        let now = theseus_protocol::now_unix_ms();
        self.deleted.insert(key.clone(), now);
        self.deleted
            .retain(|_, at| now.saturating_sub(*at) < LINK_KEPT_MS);
        self.save_gone();
    }

    /// The item stands again: a post may be made for it.
    pub(super) fn unmark_deleted(&mut self, key: &(String, u64)) {
        if self.deleted.remove(key).is_some() {
            self.save_gone();
        }
    }

    fn save_gone(&self) {
        let list: Vec<(String, u64, u64)> = self
            .deleted
            .iter()
            .map(|((task, forum), at)| (task.clone(), *forum, *at))
            .collect();
        if let Err(e) = self.shared.core.store.put_meta(GONE_KEY, &list) {
            tracing::warn!(error = %format!("{e:#}"), "the deleted task posts were not saved");
        }
    }
}

/// The marks the store holds that are not a day old.
fn load_gone(shared: &Shared) -> HashMap<(String, u64), u64> {
    let now = theseus_protocol::now_unix_ms();
    let list: Vec<(String, u64, u64)> = shared
        .core
        .store
        .get_meta(GONE_KEY)
        .ok()
        .flatten()
        .unwrap_or_default();
    list.into_iter()
        .filter(|(_, _, at)| now.saturating_sub(*at) < LINK_KEPT_MS)
        .map(|(task, forum, at)| ((task, forum), at))
        .collect()
}
