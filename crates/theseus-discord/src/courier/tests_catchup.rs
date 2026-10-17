//! One catch-up message after Discord was unreachable (theseus-ss36w), through
//! a lane against the stand-in's `Down` then `Up`.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_core::config::discord::Category;
use theseus_core::secrets::SecretBoard;
use theseus_sim::fake_discord::{FakeDiscord, Mode};

use super::tests_bound::CHANNEL;
use super::*;

fn lane(dir: &std::path::Path, fake: &FakeDiscord, silent: &[Category]) -> Lane {
    let secrets = SecretBoard::new([], Instant::now());
    let silent = silent.to_vec();
    let core = crate::runtime::tests::core_with(dir, secrets, |c| {
        c.discord.rest_proxy = Some(fake.addr.clone());
        c.discord.silent = silent;
    });
    let shared = crate::runtime::shared_for_tests(&core);
    Lane::new(
        shared,
        format!("discord:channel:{CHANNEL}"),
        "channel",
        "#harbor".into(),
        Some(CHANNEL),
        None,
    )
}

fn report(n: usize, outcome: &str) -> Value {
    json!({"kind": "report", "task": format!("tsk_{n}"), "short": format!("abc{n:03}"),
           "title": format!("Chart {n}"), "outcome": outcome, "turns": 1, "elapsed_ms": 4000})
}

async fn until(what: &str, f: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(15), "no {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// How many requests the stand-in has turned away.
fn refused(fake: &FakeDiscord) -> usize {
    fake.seen()
        .iter()
        .filter(|s| s.outcome == "dropped")
        .count()
}

/// A reminder pings whatever the lists say, so it posts on its own.
fn reminder() -> Value {
    json!({"kind": "reminder", "text": "Water the ferns", "wake_id": "wk_ferns"})
}

/// Down, then up: four silent reports written in the outage are one message,
/// each settled with its id, while a reminder written in it posts on its own.
#[tokio::test]
async fn four_silent_reports_written_in_an_outage_are_one_message_each_settled() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let lane = lane(d.path(), &fake, &[Category::Reports, Category::Failures]);
    let target = lane.target.clone();
    let core = lane.shared.core.clone();
    let outbox = &core.outbox;
    fake.set_mode(Mode::Down);
    // The reminder meets the outage first.
    let first = outbox.post("sess_harbor", "", &target, reminder()).unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let run = tokio::spawn(lane.run(rx));
    let _ = tx.send(LaneMsg::Wake);
    until("the outage met", || refused(&fake) >= 1).await;
    let mut reports = Vec::new();
    for n in 1..=4 {
        let outcome = if n == 4 { "failed" } else { "complete" };
        reports.push(
            outbox
                .post("sess_harbor", "", &target, report(n, outcome))
                .unwrap(),
        );
    }
    fake.set_mode(Mode::Up);
    // The lane tries again at its backoff.
    let _ = tx.send(LaneMsg::Wake);
    until("the fold and the reminder", || {
        fake.messages(CHANNEL).len() >= 2
    })
    .await;
    let got = fake.messages(CHANNEL);
    assert_eq!(got.len(), 2, "{got:#?}");
    let fold = &got[0];
    assert!(
        fold.content.starts_with("While Discord was unreachable ("),
        "{}",
        fold.content
    );
    assert!(
        fold.content.contains("3 tasks reported, 1 failed:"),
        "{}",
        fold.content
    );
    for n in 1..=4 {
        assert!(
            fold.content.contains(&format!("Chart {n}")),
            "{}",
            fold.content
        );
    }
    assert!(fold.silent(), "one silent message: {fold:#?}");
    assert!(got[1].content.contains("Water the ferns"), "{got:#?}");
    // Each folded post is settled with the fold's message id.
    until("every post settled", || outbox.open_for(&target).is_empty()).await;
    for a in &reports {
        let posted = core
            .kernel
            .outbox_action(&a.correlation_id)
            .unwrap()
            .unwrap();
        assert_eq!(posted.state, ActionState::Succeeded);
        assert_eq!(posted.external_op_id.as_deref(), Some(fold.id.as_str()));
        let detail = posted.detail.unwrap();
        assert_eq!(detail["folded_into"], fold.id.as_str());
    }
    let own = core
        .kernel
        .outbox_action(&first.correlation_id)
        .unwrap()
        .unwrap();
    assert_eq!(own.external_op_id.as_deref(), Some(got[1].id.as_str()));
    // Nothing is sent twice.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.messages(CHANNEL).len(), 2);
    drop(tx);
    run.await.unwrap();
}

/// Fewer than three, or posts that ping, are not folded: each goes on its own.
#[tokio::test]
async fn two_silent_reports_or_reports_that_ping_post_on_their_own() {
    for (silent, reports) in [(vec![Category::Reports], 2), (vec![], 4)] {
        let d = tempfile::tempdir().unwrap();
        let fake = FakeDiscord::start();
        let lane = lane(d.path(), &fake, &silent);
        let target = lane.target.clone();
        let outbox = lane.shared.core.outbox.clone();
        fake.set_mode(Mode::Down);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let run = tokio::spawn(lane.run(rx));
        outbox.post("sess_harbor", "", &target, reminder()).unwrap();
        let _ = tx.send(LaneMsg::Wake);
        until("the outage met", || refused(&fake) >= 1).await;
        for n in 1..=reports {
            outbox
                .post("sess_harbor", "", &target, report(n, "complete"))
                .unwrap();
        }
        fake.set_mode(Mode::Up);
        let _ = tx.send(LaneMsg::Wake);
        until("every post", || fake.messages(CHANNEL).len() == reports + 1).await;
        let got = fake.messages(CHANNEL);
        assert!(
            got.iter().all(|m| !m.content.starts_with("While Discord")),
            "{got:#?}"
        );
        drop(tx);
        run.await.unwrap();
    }
}
