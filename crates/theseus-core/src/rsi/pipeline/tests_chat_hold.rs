//! A finding filed from chat waits a minute after its note is written before its first build
//! (issue 57, `chat_hold`): by the pipeline's ticks and by the starter they share; the minute
//! starts once the frame holding the filing and its note is written, however long that took; a
//! restart takes the whole minute again. A filing from the owner's own shell does not wait.

use std::time::Duration;

use theseus_protocol::rsi::SelfFileResult;

use super::chat_hold::{ChatHolds, CHAT_HOLD};
use super::tests::{rig_with, Rig};
use crate::owner_note::told::{self, Undo};
use crate::rsi::backlog::tool::OWNER;

fn holds(r: &Rig) -> &ChatHolds {
    &r.core.rsi.pipeline.chat_holds
}

/// A finding filed from chat with its note, as `self.file` files it; the note takes `delay` to
/// stage. Its item's id.
fn filed_from_chat(r: &Rig, delay: Duration) -> String {
    let note = |f: &SelfFileResult| {
        std::thread::sleep(delay);
        told::stage(
            &r.core.outbox,
            None,
            "A finding was filed for the self loop",
            "In a test, from this chat",
            "",
            &Undo::SelfDrop(f.item.clone()),
        )
    };
    r.core
        .self_file_in_chat_told(
            "The digest folds two kinds into one line.",
            None,
            OWNER,
            &note,
        )
        .unwrap()
        .item
}

#[tokio::test]
async fn a_chat_filing_waits_a_minute_after_its_note_and_a_shells_does_not() {
    let r = rig_with(5, 100.0);
    let chat = filed_from_chat(&r, Duration::ZERO);
    let shell = r
        .core
        .self_file_in_chat("The gate runs clippy twice.", None, "the CLI")
        .unwrap()
        .item;
    let tick = r.tick().await;
    assert!(tick.started.contains(&shell), "{:?}", tick.started);
    assert!(!tick.started.contains(&chat), "held: {:?}", tick.started);
    // A minute on: it starts as any item does.
    holds(&r).age(CHAT_HOLD);
    let tick = r.tick().await;
    assert!(tick.started.contains(&chat), "{:?}", tick.started);
}

/// A note that is slow to write does not eat the minute: it starts once the frame is written.
#[tokio::test]
async fn the_minute_starts_once_the_note_is_written() {
    let r = rig_with(5, 100.0);
    let chat = filed_from_chat(&r, Duration::from_millis(1_500));
    let left = holds(&r).left(&chat).expect("its hold");
    assert!(
        left > CHAT_HOLD - Duration::from_millis(500),
        "the hold began before the note was written: {left:?} left"
    );
}

/// A restart inside the minute: nothing durable says when the note was written, so the item is
/// held a whole minute from the restart, though more than a minute has passed since its filing.
#[tokio::test]
async fn a_restart_inside_the_minute_holds_it_a_whole_minute_again() {
    let r = rig_with(5, 100.0);
    let chat = filed_from_chat(&r, Duration::ZERO);
    holds(&r).age(Duration::from_secs(30));
    holds(&r).restart();
    holds(&r).age(Duration::from_secs(45));
    let tick = r.tick().await;
    assert!(
        !tick.started.contains(&chat),
        "held after the restart: {:?}",
        tick.started
    );
    holds(&r).age(Duration::from_secs(16));
    let tick = r.tick().await;
    assert!(tick.started.contains(&chat), "{:?}", tick.started);
}

/// The starter every path shares holds it too, not only the pipeline's ready queue.
#[tokio::test]
async fn the_shared_starter_holds_a_chat_filing_too() {
    let r = rig_with(5, 100.0);
    let chat = filed_from_chat(&r, Duration::ZERO);
    let early = r.core.backlog_start_item(&chat).unwrap();
    assert!(early.is_none(), "started inside its minute");
    holds(&r).age(CHAT_HOLD);
    let late = r.core.backlog_start_item(&chat).unwrap();
    assert!(late.is_some(), "not started after its minute");
}
