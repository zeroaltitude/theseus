//! A finished task's post leaves the forum once read and a day over
//! (theseus-n9dkb), end to end on the default forum's rig: a DM task works in
//! its post, the owner's Clear done on the DM's board reads it, and the sweep
//! (`Shared::sweep_once`, on a binding of the same store) archives and locks
//! it. The tests move the row's end back in time in the META list.

use serde_json::{json, Value};
use theseus_core::provider::Scripted;
use theseus_sim::fake_discord::FakeThread;

use crate::render::board_v2;
use crate::tests_default_forum::{bindings, forum_up, posts, rig};
use crate::tests_gateway::{Rig, ANA, ANA_DM};
use crate::tests_task_threads::{gate, open_gate, sleeps, start, until, ASK, REEF};

/// The open posts' META list (`forum::OPEN_KEY`).
const KEY: &str = "discord.task_posts";
const HOUR: u64 = 3_600_000;

fn rows(r: &Rig) -> Vec<Value> {
    r.core
        .store
        .get_meta::<Vec<Value>>(KEY)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Every finished row ended `ago` before now.
fn ended_ago(r: &Rig, ago: u64) {
    let now = theseus_protocol::now_unix_ms();
    let mut list = rows(r);
    for p in &mut list {
        if p["ended_ms"].is_u64() {
            p["ended_ms"] = json!(now - ago);
        }
    }
    r.core.store.put_meta(KEY, &list).unwrap();
}

/// One sweep, by a binding that reads the list afresh.
async fn sweep(r: &Rig) {
    crate::runtime::shared_for_tests(&r.core).sweep_once().await;
}

fn thread(r: &Rig, id: u64) -> FakeThread {
    r.fake.threads().into_iter().find(|t| t.id == id).unwrap()
}

/// A DM task in its post, held at the gate: the rig and the post's id.
async fn a_dm_task() -> (Rig, u64) {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], sleeps(1, "Reef charted.")].concat())],
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    let forum = forum_up(&r).await;
    until(&r, "the task's post", || !posts(&r, forum).is_empty()).await;
    let post = posts(&r, forum).remove(0).id;
    (r, post)
}

/// The task ends and its last state is in the list.
async fn ends(r: &Rig) {
    open_gate(r);
    until(r, "the post's row ended", || {
        rows(r).iter().any(|p| p["ended_ms"].is_u64())
    })
    .await;
}

/// The owner presses Clear done on the DM's board; the post's row is read.
async fn clear_done(r: &Rig) {
    until(r, "the DM's board", || board(r).is_some()).await;
    r.press(&board(r).unwrap(), "✓ Clear done", (ANA, "ana"));
    until(r, "the post's row read", || {
        rows(r).iter().any(|p| p["read_ms"].is_u64())
    })
    .await;
}

fn board(r: &Rig) -> Option<String> {
    r.posted(ANA_DM)
        .into_iter()
        .rfind(|m| m.buttons.iter().any(|b| b.custom_id == board_v2::CLEAR))
        .map(|m| m.id)
}

/// Read, and ended a day and an hour ago: archived and locked at the sweep.
#[tokio::test]
async fn a_read_post_a_day_over_is_archived_and_locked_at_the_sweep() {
    let (r, post) = a_dm_task().await;
    ends(&r).await;
    clear_done(&r).await;
    ended_ago(&r, 25 * HOUR);
    sweep(&r).await;
    let t = thread(&r, post);
    assert!(t.archived && t.locked, "{t:#?}");
}

/// Running, unread, or read but ended 23 hours ago: the post stays.
#[tokio::test]
async fn a_running_an_unread_or_a_recent_post_stays() {
    let (r, post) = a_dm_task().await;
    sweep(&r).await;
    assert!(!thread(&r, post).archived, "running");
    ends(&r).await;
    ended_ago(&r, 25 * HOUR);
    sweep(&r).await;
    assert!(!thread(&r, post).archived, "unread");
    clear_done(&r).await;
    ended_ago(&r, 23 * HOUR);
    sweep(&r).await;
    let t = thread(&r, post);
    assert!(!t.archived && !t.locked, "23 h: {t:#?}");
}

/// A post the owner opens again after the sweep archived it is left alone.
#[tokio::test]
async fn a_post_reopened_after_the_sweep_is_not_archived_again() {
    let (r, post) = a_dm_task().await;
    ends(&r).await;
    clear_done(&r).await;
    ended_ago(&r, 25 * HOUR);
    let shared = crate::runtime::shared_for_tests(&r.core);
    shared.sweep_once().await;
    assert!(thread(&r, post).archived, "archived first");
    shared
        .http
        .update_thread(twilight_model::id::Id::new(post))
        .archived(false)
        .locked(false)
        .await
        .unwrap();
    shared.sweep_once().await;
    sweep(&r).await;
    let t = thread(&r, post);
    assert!(!t.archived && !t.locked, "{t:#?}");
}

/// A Clear done pressed after the task ended but before the forum worker
/// wrote the post's end is not lost: the row never got `read_ms` (planted
/// here by taking it off), and a board that cleared since the end reads the
/// post at the sweep.
#[tokio::test]
async fn a_clear_before_the_end_still_counts_as_read() {
    let (r, post) = a_dm_task().await;
    ends(&r).await;
    let mut list = rows(&r);
    let task = list[0]["task"].as_str().unwrap().to_string();
    for p in &mut list {
        p["read_ms"] = Value::Null;
    }
    r.core.store.put_meta(KEY, &list).unwrap();
    ended_ago(&r, 25 * HOUR);
    let shared = crate::runtime::shared_for_tests(&r.core);
    shared
        .tasks
        .ended_for_tests(&task, theseus_protocol::now_unix_ms() - 25 * HOUR);
    shared.sweep_once().await;
    assert!(!thread(&r, post).archived, "never cleared: unread");
    shared.tasks.clear_done("ses_exe_chat");
    shared.sweep_once().await;
    let t = thread(&r, post);
    assert!(t.archived && t.locked, "{t:#?}");
}
