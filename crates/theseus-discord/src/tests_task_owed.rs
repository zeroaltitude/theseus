//! A task that finishes at once still has its post (theseus-hzdlf, the owner's rule: threads === tasks), through
//! the stand-ins as `tests_default_forum.rs` has them. The decisions are `runtime/threads/tests_owed.rs`'s: how
//! the sync races a fast task cannot be pinned from here, so this holds what must be true however it lands.

use theseus_core::provider::Scripted;

use crate::tests_default_forum::{bindings, forum_up, posts, rig};
use crate::tests_gateway::ANA;
use crate::tests_task_threads::{start, until, ASK, REEF};

/// A task that answers at once, with no program to hold it: whether a sync saw it running or only over,
/// it ends with exactly one post, named with the done mark and tagged done.
#[tokio::test]
async fn a_fast_task_has_exactly_one_post_in_its_final_state() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |_| vec![(REEF, vec![Scripted::text("Reef charted.")])],
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    let forum = forum_up(&r).await;
    until(&r, "the task's one post, over", || {
        posts(&r, forum)
            .first()
            .is_some_and(|p| p.names.last().is_some_and(|n| n.starts_with("✅ ")))
    })
    .await;
    // Every sync after it makes none.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(posts(&r, forum).len(), 1, "{:#?}", posts(&r, forum));
}
