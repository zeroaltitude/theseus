//! A hung fetch does not hold the pipeline's tick (theseus-6vlr4): the start
//! opens its tree through the door, whose fetch waits for its bound and then
//! goes on from the base the repo has, so the tick completes, starts the
//! build, and leaves no `git` behind.

use std::time::{Duration, Instant};

use super::tests::rig_with;
use crate::rsi::tests_fetch_bound::hanging_git;
use crate::rsi::tests_tree::hermetic;

/// KEY: with a `git` whose fetch never answers, one tick still ends within a
/// bound of its own, having started the build in a tree.
#[tokio::test]
async fn a_hung_fetch_does_not_hold_the_ticks_start() {
    let r = rig_with(1, 30.0);
    let h = hanging_git(hermetic(), Duration::from_millis(800));
    r.core.tools.self_pr.set_env(h.env.clone());
    let t = Instant::now();
    let tick = tokio::time::timeout(Duration::from_secs(30), r.tick())
        .await
        .expect("the tick was held by the fetch");
    let took = t.elapsed();
    assert!(h.hanging(), "the door fetched through the stand-in");
    assert!(took >= Duration::from_millis(800), "{took:?}");
    assert_eq!(tick.started.len(), 1, "{tick:?}");
    assert!(
        crate::rsi::tree::find(&r.f.place, &tick.started[0]).is_some(),
        "the build has its tree"
    );
    h.nothing_left().unwrap();
}
