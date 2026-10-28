//! The walkthrough's step 1 on Discord under the owner's posture
//! (theseus-h68q6; theseus-e31an items 4 to 6): `[policy] enforcement =
//! "notify"`, the task home on, a trusted guild with its forum. The owner's DM
//! asks for two tasks. Each must be seen: a started notice in the DM, its
//! forum post, its one quiet 🧵 line in the home, the live 📋 board there, and
//! its notified calls' lines in its post. (The second walk, a DM held by outside
//! text where the owner pressed "Approve + trust", went with the hold: R6,
//! theseus-gyjyo.8.)

use theseus_core::policy::Posture;
use theseus_core::provider::Scripted;
use theseus_sim::fake_discord::Msg;

use crate::render::board_v2;
use crate::tests_default_forum::{bindings, forum_up, guild, posts, rig_in};
use crate::tests_gateway::{Rig, ANA, ANA_DM};
use crate::tests_task_threads::{answers, gate, open_gate, replied, start, until, ASK, BUOY, REEF};

const V2: u64 = 1 << 15;

/// The owner's posture: every call notifies, the task home stands.
fn owners_posture(c: &mut theseus_core::Config) {
    c.discord.task_home = "theseus-tasks".into();
    c.policy.enforcement = Posture::Notify;
    c.policy.tools.insert("proc.run".into(), Posture::Notify);
}

async fn walk(parent: Vec<Scripted>) -> Rig {
    rig_in(
        guild(false),
        parent,
        |dir| {
            vec![
                (REEF, [vec![gate(dir)], answers("Reef charted.")].concat()),
                (BUOY, [vec![gate(dir)], answers("Buoy painted.")].concat()),
            ]
        },
        &bindings(true, ""),
        |_| {},
        owners_posture,
    )
    .await
}

fn home_record(r: &Rig) -> Option<u64> {
    let v: Option<serde_json::Value> = r.core.store.get_meta("discord.task_home").ok().flatten();
    v?["channel"].as_u64()
}

async fn home_up(r: &Rig) -> u64 {
    until(r, "the task home is recorded", || home_record(r).is_some()).await;
    home_record(r).unwrap()
}

fn boards(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.flags & V2 != 0 && m.buttons.iter().any(|b| b.custom_id == board_v2::CLEAR))
        .collect()
}

/// The DM's started notices.
fn started(r: &Rig) -> Vec<Msg> {
    r.posted(ANA_DM)
        .into_iter()
        .filter(|m| m.content.contains("🔔 started task"))
        .collect()
}

/// What the owner must see once both tasks run: two posts, each pointed at
/// once from the home and announced once in the DM, the board in the home,
/// and each task's notified call on its post.
async fn both_seen(r: &Rig, forum: u64, home: u64) {
    until(r, "two posts", || posts(r, forum).len() == 2).await;
    until(r, "the board in the home", || !boards(r, home).is_empty()).await;
    for p in posts(r, forum) {
        let line = format!("🧵 <#{}>", p.id);
        until(r, "the pointer in the home", || {
            r.posted(home).iter().any(|m| m.content.contains(&line))
        })
        .await;
        let link = format!("<#{}>", p.id);
        until(r, "the started notice in the DM", || {
            started(r).iter().any(|m| m.content.contains(&link))
        })
        .await;
    }
    assert_eq!(started(r).len(), 2, "one started notice a task");
    for m in started(r) {
        assert!(m.silent() && m.flags & V2 == 0, "a quiet line: {m:#?}");
    }
    for p in posts(r, forum) {
        let line = format!("🧵 <#{}>", p.id);
        let n = r
            .posted(home)
            .iter()
            .filter(|m| m.content.contains(&line))
            .count();
        assert_eq!(n, 1, "one pointer to {}", p.id);
    }
}

/// Each task's notified `proc.run` shows on its post, as its tool line.
async fn calls_seen(r: &Rig, forum: u64) {
    for p in posts(r, forum) {
        until(r, "the task's notified call on its post", || {
            r.posted(p.id)
                .iter()
                .any(|m| m.content.contains("`proc.run`") && m.content.contains("🔔 notified"))
        })
        .await;
    }
}

/// The DM unheld: both tasks notify, and both are seen.
#[tokio::test]
async fn a_dm_under_notify_starts_two_tasks_and_both_are_seen() {
    let r = walk(vec![
        Scripted::text("Hi."),
        start(&[REEF, BUOY]),
        Scripted::text("Started both."),
    ])
    .await;
    let forum = forum_up(&r).await;
    let home = home_up(&r).await;
    r.say((ANA, "ana"), None, "hello");
    replied(&r, ANA_DM, "Hi.").await;
    r.say((ANA, "ana"), None, ASK);
    replied(&r, ANA_DM, "Started both.").await;
    both_seen(&r, forum, home).await;
    calls_seen(&r, forum).await;
    open_gate(&r);
}

/// A thread no Discord place runs, made from the CLI.
fn harbor(r: &Rig) {
    r.core
        .thread_new(
            &theseus_protocol::threads::ThreadNewParams {
                slug: "harbor".into(),
                title: Some("Harbor".into()),
                ..Default::default()
            },
            crate::tests_default_forum::cli(),
        )
        .unwrap();
}

/// The DM routes the ask into a thread no Discord place runs (the owner's
/// answer to "which thread?" on #68, or an explicit address): the tasks run
/// under that thread's conversation, and the DM, where they were asked for,
/// still sees both. (Their first calls may run before the watch names the
/// thread: a tool line is live progress, best effort, so this test does not
/// ask for it; the two above do.)
#[tokio::test]
async fn a_dm_message_routed_to_a_placeless_thread_still_sees_its_tasks() {
    // The DM's session opens at its bind, so no later ask (a fresh session's)
    // can bring the thread in: only the turn the DM routes to it can.
    crate::tests_gateway::OPEN_AT_BIND.with(|o| o.set(true));
    let r = walk(vec![
        Scripted::text("Hi."),
        start(&[REEF, BUOY]),
        Scripted::text("Started both."),
    ])
    .await;
    let forum = forum_up(&r).await;
    let home = home_up(&r).await;
    r.say((ANA, "ana"), None, "hello");
    replied(&r, ANA_DM, "Hi.").await;
    harbor(&r);
    r.say((ANA, "ana"), None, &format!("#harbor {ASK}"));
    until(&r, "the tasks run under the thread", || {
        r.core.kernel.tasks(None).unwrap().len() == 2
    })
    .await;
    both_seen(&r, forum, home).await;
    open_gate(&r);
}
