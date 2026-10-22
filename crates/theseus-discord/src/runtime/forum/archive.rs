//! A finished task's post leaves the forum once the owner has read it and a
//! day has passed (theseus-n9dkb; the owner: "tasks in a terminal state should
//! be removed from the forum when they are 1) read, and 2) older than 24
//! hours"). A child of `forum`.
//!
//! - **Read** is the board's seen state: the owner pressed Clear done on the
//!   board of the task's place after it ended (`TaskPosts::seen`, kept on the
//!   post's row so a restart keeps it), or dismissed the task's failure (the
//!   view's `dismissed`). Discord tells a bot nothing of who opened a post.
//! - **Old** is the row's `ended_ms`, when the task's last state was shown,
//!   at least `ARCHIVE_AFTER_MS` ago.
//! - **Archived and locked**, never deleted: hidden from the forum's list,
//!   still searchable, and the owner may open it again. It is done once
//!   (`archived_ms`), so a post the owner reopens is left alone.
//! - It runs on its own hourly tick (`ARCHIVE_TICK`, `Shared::archive_tick`), so a
//!   read post leaves at most an hour after its day, and on the task channels'
//!   sweep (`Shared::sweep_once`: at the start, then daily).
//! - A Clear done pressed after the task ended but before the forum worker
//!   wrote the post's end still reads it: the tick asks the board
//!   (`Tasks::cleared_since_end`) and keeps the answer in `read_ms`.
//! - **What a restart keeps.** A finished post stays in the list until it is
//!   archived, or `UNREAD_KEPT_MS` unread; an archived one a day for its link.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use super::{Post, TaskPosts};
use crate::runtime::Shared;

/// How often the archive looks for a read post a day over.
pub(crate) const ARCHIVE_TICK: Duration = Duration::from_secs(3600);

/// A read post of a finished task is archived this long after it ended.
pub(crate) const ARCHIVE_AFTER_MS: u64 = 24 * 3_600_000;

/// An unread finished post is kept in the list this long, waiting to be read.
const UNREAD_KEPT_MS: u64 = 30 * 24 * 3_600_000;

/// Whether a restart keeps the row `p` at `now`: one still open always, an
/// unarchived finished one `UNREAD_KEPT_MS`, any finished one `link_kept`.
pub(super) fn kept(p: &Post, now: u64, link_kept: u64) -> bool {
    let Some(ended) = p.ended_ms else {
        return true;
    };
    let age = now.saturating_sub(ended);
    age < link_kept || (p.archived_ms.is_none() && age < UNREAD_KEPT_MS)
}

/// Whether the post `p` is due to leave the forum at `now`.
fn due(p: &Post, now: u64, read: bool) -> bool {
    p.archived_ms.is_none()
        && read
        && p.ended_ms
            .is_some_and(|e| now.saturating_sub(e) >= ARCHIVE_AFTER_MS)
}

impl TaskPosts {
    /// The owner has seen the end of the tasks `tasks`: their finished posts
    /// are read from now.
    pub(crate) fn seen(&self, tasks: &[String], store: &theseus_core::store::Store) {
        let now = theseus_protocol::now_unix_ms();
        let mut changed = false;
        for p in self.posts.lock().unwrap().iter_mut() {
            if p.ended_ms.is_some() && p.read_ms.is_none() && tasks.contains(&p.task) {
                p.read_ms = Some(now);
                changed = true;
            }
        }
        if changed {
            self.save(store);
        }
    }
}

/// Run `run` every `period` until `stopped`.
async fn every<F: Future<Output = ()>>(
    period: Duration,
    stopped: impl Future<Output = ()>,
    mut run: impl FnMut() -> F,
) {
    tokio::pin!(stopped);
    loop {
        tokio::select! {
            () = tokio::time::sleep(period) => {}
            () = &mut stopped => return,
        }
        run().await;
    }
}

impl Shared {
    /// The archive's own tick, until the daemon stops.
    pub(crate) fn archive_tick(self: &Arc<Self>) {
        let s = self.clone();
        tokio::spawn(async move {
            every(ARCHIVE_TICK, s.core.outbox.stopped(), || {
                s.archive_read_posts()
            })
            .await;
        });
    }

    /// Every finished post the owner has read and that ended a day ago is
    /// archived and locked, once; a refusal is tried again at the next sweep.
    pub(crate) async fn archive_read_posts(&self) {
        // The sweep may run before the forum's worker has read the list.
        self.task_posts.load(&self.core.store);
        let cleared: Vec<String> = self
            .task_posts
            .posts
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.ended_ms.is_some() && p.read_ms.is_none())
            .filter(|p| self.tasks.cleared_since_end(&p.task))
            .map(|p| p.task.clone())
            .collect();
        self.task_posts.seen(&cleared, &self.core.store);
        let now = theseus_protocol::now_unix_ms();
        let due: Vec<Post> = self
            .task_posts
            .posts
            .lock()
            .unwrap()
            .iter()
            .filter(|p| {
                let dismissed = self.tasks.view(&p.task).is_some_and(|v| v.dismissed);
                due(p, now, p.read_ms.is_some() || dismissed)
            })
            .cloned()
            .collect();
        for p in due {
            let sent = self
                .http
                .update_thread(twilight_model::id::Id::new(p.post))
                .archived(true)
                .locked(true)
                .await;
            match sent {
                Ok(_) => {}
                Err(e) if gone(&e) => {}
                Err(e) => {
                    self.board.error("task forum archive", None, e);
                    continue;
                }
            }
            for q in self.task_posts.posts.lock().unwrap().iter_mut() {
                if q.task == p.task && q.forum == p.forum {
                    q.archived_ms = Some(now);
                }
            }
            self.task_posts.save(&self.core.store);
        }
    }
}

fn gone(e: &twilight_http::Error) -> bool {
    use twilight_http::error::ErrorType as E;
    matches!(e.kind(), E::Response { status, .. } if status.get() == 404)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 3_600_000;

    fn post(ended: Option<u64>, archived: Option<u64>) -> Post {
        Post {
            task: "exe_a".into(),
            forum: 1,
            post: 2,
            guild: "3".into(),
            created_ms: 0,
            ended_ms: ended,
            link: None,
            bound: None,
            read_ms: None,
            archived_ms: archived,
        }
    }

    /// A restart keeps an unread finished post until it can be archived, and
    /// an archived one only a day for its link.
    #[test]
    fn a_restart_keeps_a_finished_post_until_it_is_archived() {
        let now = 100 * DAY;
        assert!(kept(&post(None, None), now, DAY));
        assert!(kept(&post(Some(now - 2 * DAY), None), now, DAY));
        assert!(!kept(&post(Some(now - 31 * DAY), None), now, DAY));
        assert!(kept(&post(Some(now - DAY / 2), Some(now)), now, DAY));
        assert!(!kept(&post(Some(now - 2 * DAY), Some(now)), now, DAY));
    }

    /// Both conditions: read, and ended a day ago; once.
    #[test]
    fn a_post_is_due_when_read_and_a_day_old_and_not_yet_archived() {
        let now = 100 * DAY;
        let old = post(Some(now - DAY - 3_600_000), None);
        assert!(due(&old, now, true));
        assert!(!due(&old, now, false), "unread");
        assert!(
            !due(&post(Some(now - DAY + 3_600_000), None), now, true),
            "23 h"
        );
        assert!(!due(&post(None, None), now, true), "running");
        assert!(
            !due(&post(Some(now - 2 * DAY), Some(now - DAY)), now, true),
            "once"
        );
    }

    /// The tick is within the hour: a read post a day and a minute over is
    /// archived by the next run, with no daily sweep in the way.
    #[tokio::test(start_paused = true)]
    async fn a_read_post_leaves_within_the_hour_after_its_day() {
        let ran = std::sync::atomic::AtomicU32::new(0);
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let run = every(
            ARCHIVE_TICK,
            async {
                let _ = stopped.await;
            },
            || async {
                ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
        );
        let driver = async {
            tokio::time::advance(Duration::from_secs(3601)).await;
            tokio::task::yield_now().await;
            let _ = stop.send(());
        };
        tokio::join!(run, driver);
        assert_eq!(ran.load(std::sync::atomic::Ordering::SeqCst), 1);
        let now = 100 * DAY;
        let read = post(Some(now - DAY - 60_000), None);
        assert!(due(&read, now, true));
        assert!(due(&read, now + ARCHIVE_TICK.as_millis() as u64, true));
    }
}
