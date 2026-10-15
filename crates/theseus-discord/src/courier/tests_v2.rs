//! The live board on a lane (theseus-2j2ka): created once, pinned, silent, and
//! edited at most once in `BOARD_EVERY` however fast the tasks change; found
//! again after a restart; and an idle one written only where one is pinned.
//! The lane is driven directly against the stand-in, as `tests_bound` does.

use std::time::Duration;

use serde_json::Value;
use theseus_sim::fake_discord::{FakeDiscord, Msg, IS_COMPONENTS_V2};
use tokio::time::Instant;

use super::tests_bound::{lane, CHANNEL};
use super::*;
use crate::render::board_v2::{self, Parts};
use crate::render::v2::texts;

/// A board with `n` running tasks, as the place sends it.
fn board(n: usize) -> String {
    let w: Vec<theseus_protocol::work::WorkView> = (0..n).map(task).collect();
    let c = board_v2::render(&Parts {
        needs: &[],
        working: &w,
        done: &[],
        quiet_until_ms: None,
    });
    serde_json::to_string(&c).unwrap()
}

fn task(n: usize) -> theseus_protocol::work::WorkView {
    use theseus_protocol::work::{WorkKind, WorkState};
    use theseus_protocol::{Attention, Level};
    theseus_protocol::work::WorkView {
        id: format!("exe_01J0000000000000000000{n:04}"),
        kind: WorkKind::Task,
        position: 1,
        at_ms: 0,
        parent: None,
        root: "exe_chat".into(),
        session_id: None,
        title: format!("Task number {n}"),
        place: None,
        state: WorkState::Running,
        previous: None,
        attention: Attention {
            level: Level::Working,
            label: "turn 1".into(),
            since_ms: 0,
        },
        started_at_ms: 1_790_000_000_000,
        state_since_ms: 0,
        ended_at_ms: None,
        doing: None,
        progress: None,
        question: None,
        cost: None,
        eta: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        flagged: false,
    }
}

fn live(content: String) -> LaneMsg {
    LaneMsg::Live(Op::Upsert {
        key: render::BOARD_KEY.into(),
        content,
        buttons: Buttons::V2,
    })
}

/// The boards the bot holds in the channel: V2 messages with a Clear button.
fn boards(fake: &FakeDiscord) -> Vec<Msg> {
    fake.messages(CHANNEL)
        .into_iter()
        .filter(|m| {
            m.flags & IS_COMPONENTS_V2 != 0
                && m.buttons.iter().any(|b| b.custom_id == board_v2::CLEAR)
        })
        .collect()
}

#[tokio::test]
async fn the_board_is_made_once_pinned_and_silent_and_then_edited_never_posted_anew() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    lane.take(live(board(1)));
    lane.apply_live().await;
    let b = boards(&fake);
    assert_eq!(b.len(), 1);
    assert!(b[0].silent(), "the board never pings: {:?}", b[0].flags);
    assert!(b[0].pinned, "pinned");
    assert!(b[0].content.contains("1 running"), "{}", b[0].content);
    assert_eq!(b[0].edits, 0);
    // A later state, after the interval, edits that message.
    tokio::time::sleep(v2::BOARD_EVERY + Duration::from_millis(100)).await;
    lane.take(live(board(2)));
    lane.apply_live().await;
    let b = boards(&fake);
    assert_eq!(b.len(), 1, "edited, not posted anew");
    assert_eq!(b[0].edits, 1);
    assert!(b[0].content.contains("2 running"), "{}", b[0].content);
    // The same state again writes nothing.
    tokio::time::sleep(v2::BOARD_EVERY + Duration::from_millis(100)).await;
    lane.take(live(board(2)));
    lane.apply_live().await;
    assert_eq!(boards(&fake)[0].edits, 1, "an unchanged board is not sent");
}

#[tokio::test]
async fn states_inside_the_interval_wait_and_the_latest_replaces_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    lane.take(live(board(1)));
    lane.apply_live().await;
    for n in 2..=5 {
        lane.take(live(board(n)));
        lane.apply_live().await;
    }
    assert_eq!(boards(&fake)[0].edits, 0, "none sent inside the interval");
    assert_eq!(lane.live.len(), 1, "the latest waits, one of them");
    let due = lane.v2.due().expect("the lane wakes for it");
    assert!(due > Instant::now());
    tokio::time::sleep_until(due).await;
    lane.apply_live().await;
    let b = boards(&fake);
    assert_eq!(
        (b[0].edits, b[0].content.contains("5 running")),
        (1, true),
        "{}",
        b[0].content
    );
    assert!(lane.v2.due().is_none());
}

/// A busy day: ten tasks and two hundred transitions inside four seconds make
/// at most four edits, one per `BOARD_EVERY`, and the last state is the one
/// shown. The worst minute is therefore 30 edits (60 s / 2 s) and one create
/// for a place, against Discord's 5 edits in 5 s a channel.
#[tokio::test]
async fn two_hundred_transitions_edit_the_board_at_most_once_in_two_seconds() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let t0 = std::time::Instant::now();
    let mut last = 0;
    for i in 0..200 {
        last = i % 11;
        lane.take(live(board(last)));
        lane.apply_live().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let ran = t0.elapsed();
    let edits = boards(&fake)[0].edits;
    let most = (ran.as_secs_f64() / v2::BOARD_EVERY.as_secs_f64()).ceil() as u32;
    assert!(edits <= most, "{edits} edits in {ran:?}; at most {most}");
    // What waits is written once its interval is over.
    if let Some(due) = lane.v2.due() {
        tokio::time::sleep_until(due).await;
        lane.apply_live().await;
    }
    let b = &boards(&fake)[0];
    assert!(
        b.content.contains(&format!("{last} running")) || last == 0,
        "{}",
        b.content
    );
    assert_eq!(fake.messages(CHANNEL).len(), 1, "never posted anew");
}

#[tokio::test]
async fn after_a_restart_the_pinned_board_is_found_and_edited() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut first = lane(d.path(), &fake);
    first.take(live(board(1)));
    first.apply_live().await;
    assert_eq!(boards(&fake).len(), 1);
    // A new process: a fresh lane, which knows no message.
    let d2 = tempfile::tempdir().unwrap();
    let mut second = lane(d2.path(), &fake);
    assert!(second.msgs.is_empty());
    second.take(live(board(3)));
    second.apply_live().await;
    let b = boards(&fake);
    assert_eq!(b.len(), 1, "no second board: {:#?}", fake.messages(CHANNEL));
    assert_eq!(b[0].edits, 1);
    assert!(b[0].content.contains("3 running"), "{}", b[0].content);
}

#[tokio::test]
async fn an_idle_board_is_written_only_where_one_is_pinned() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    lane.take(LaneMsg::BoardIdle(board(0)));
    lane.apply_live().await;
    assert!(
        boards(&fake).is_empty(),
        "no board for nothing: {:#?}",
        fake.messages(CHANNEL)
    );
    // With tasks there is one; later nothing is left, and it says all clear.
    lane.take(live(board(1)));
    lane.apply_live().await;
    tokio::time::sleep(v2::BOARD_EVERY + Duration::from_millis(100)).await;
    lane.take(LaneMsg::BoardIdle(board(0)));
    lane.apply_live().await;
    let b = boards(&fake);
    assert_eq!(b.len(), 1);
    assert!(b[0].content.contains("all clear"), "{}", b[0].content);
}

#[tokio::test]
async fn a_bot_refused_components_v2_falls_back_and_says_so_once() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    // 41 text displays: a board the stand-in refuses, as Discord refuses a
    // bot that may not send V2 or a message past its limits.
    let too_many: Vec<Value> = (0..41)
        .map(|i| crate::render::v2::text(format!("line {i}")))
        .collect();
    let content = serde_json::to_string(&too_many).unwrap();
    assert!(!lane.shared.tasks.v2_refused());
    lane.take(live(content));
    lane.apply_live().await;
    assert!(lane.shared.tasks.v2_refused(), "the process falls back");
    assert!(!lane.shared.v2_cards());
    assert!(boards(&fake).is_empty());
    assert!(
        lane.retry_at.is_some(),
        "the lane tries again as the fallback"
    );
    assert!(texts(&[]).is_empty());
}

#[test]
fn a_card_pings_unless_another_that_pinged_inside_the_window_still_waits() {
    let mut s = v2::State::default();
    let now = Instant::now();
    let open = |_: &str| true;
    let closed = |_: &str| false;
    // The first pings; two more inside the window, with the first waiting, are silent.
    assert!(s.card_may_ping("q1", now, open));
    s.card_pinged("q1", now);
    assert!(!s.card_may_ping("q2", now + Duration::from_secs(5), open));
    assert!(!s.card_may_ping("q3", now + Duration::from_secs(29), open));
    assert_eq!(s.due(), Some(now + v2::CARD_EVERY));
    // The first answered: the next one pings again, whatever the clock.
    assert!(s.card_may_ping("q4", now + Duration::from_secs(10), closed));
    // Past the window it pings.
    assert!(s.card_may_ping("q5", now + Duration::from_secs(31), open));
}

/// The board is a state: an outage that comes while it waits does not drop it,
/// and the state that comes during the outage replaces it, so the board is
/// brought up to date when Discord is back rather than at the next change.
#[tokio::test]
async fn the_latest_board_waits_out_an_outage_and_is_written_when_discord_is_back() {
    use theseus_sim::fake_discord::Mode;
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    lane.take(live(board(1)));
    lane.apply_live().await;
    assert_eq!(boards(&fake).len(), 1);
    tokio::time::sleep(v2::BOARD_EVERY + Duration::from_millis(100)).await;
    fake.set_mode(Mode::Down);
    lane.take(live(board(2)));
    lane.apply_live().await;
    assert!(lane.retry_at.is_some(), "Discord is away");
    assert_eq!(lane.live.len(), 1, "the board waits");
    // A newer state during the outage replaces it.
    lane.take(live(board(3)));
    assert_eq!(lane.live.len(), 1);
    fake.set_mode(Mode::Up);
    tokio::time::sleep_until(lane.retry_at.unwrap()).await;
    lane.retry_at = None;
    lane.apply_live().await;
    let b = boards(&fake);
    assert_eq!(b.len(), 1);
    assert!(b[0].content.contains("3 running"), "{}", b[0].content);
}
