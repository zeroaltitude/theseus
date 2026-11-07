//! `/mute` and `/unmute` (theseus-k2sw): registered, mapped from the
//! interaction, private-place only like `/leave`, idempotent, and answered
//! with a fixed line that needs neither a secret exit, a mention guard nor a
//! clip (the slash reply has none of them, theseus-qtofh).

use std::time::Duration;

use theseus_voice::Command;
use tokio::sync::mpsc;

use super::super::tests::{slash, OWNER};
use super::super::tests_why::core_at;
use super::super::{shared_for_tests, Control, PlaceMsg};
use super::tests::in_owners_dm;
use super::tests_heard::lounge_scripted;
use super::{ALREADY_MUTED, MUTED, MUTE_NO_CALL, MUTE_SHARED, NOT_MUTED, UNMUTED};

const FIXED: [&str; 6] = [
    MUTED,
    UNMUTED,
    ALREADY_MUTED,
    NOT_MUTED,
    MUTE_NO_CALL,
    MUTE_SHARED,
];

/// Every line `/mute` can answer is fixed text: no person's words, no mention
/// or markup that pings, nowhere near Discord's limit. That is what makes
/// sending it as it is safe.
#[test]
fn every_reply_is_a_fixed_line_that_needs_no_masking_mention_guard_or_clip() {
    for line in FIXED {
        assert!(line.chars().count() < 2000, "{line}");
        assert!(!line.contains(['@', '<', '>']), "{line}");
        assert!(!line.contains("://"), "{line}");
    }
}

/// From a private place with a call: `/mute` sends the engine its command
/// once, `/unmute` once; the repeats say so and send nothing.
#[tokio::test]
async fn mute_and_unmute_command_the_engine_once_each() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge_scripted(d.path(), vec![]);
    let core = l.place.shared.core.clone();
    in_owners_dm(&core, &mut l.place);
    let say = |on| l.place.mute(on);
    assert_eq!(say(true), MUTED);
    assert_eq!(say(true), ALREADY_MUTED);
    assert_eq!(say(false), UNMUTED);
    assert_eq!(say(false), NOT_MUTED);
    let mut sent = Vec::new();
    while let Ok(c) = l.commands.try_recv() {
        sent.push(c);
    }
    assert_eq!(sent, vec![Command::Mute(true), Command::Mute(false)]);
}

/// A shared place, or no call, is told so, and no engine is commanded.
#[tokio::test]
async fn a_shared_place_or_no_call_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge_scripted(d.path(), vec![]);
    // The lounge, as bound, is not a private place.
    assert_eq!(l.place.mute(true), MUTE_SHARED);
    assert!(l.commands.try_recv().is_err(), "no engine was commanded");
    let core = l.place.shared.core.clone();
    in_owners_dm(&core, &mut l.place);
    l.place.shared.voice.call.lock().unwrap().take();
    assert_eq!(l.place.mute(true), MUTE_NO_CALL);
}

/// The commands are registered after `/join` and `/leave`, and the
/// interaction's name makes the place's control.
#[tokio::test]
async fn the_commands_are_registered_and_reach_the_place_as_controls() {
    let names: Vec<String> = super::commands().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["join", "leave", "mute", "unmute"]);

    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let core = core_at(d.path(), &fake.addr, vec![]);
    let shared = shared_for_tests(&core);
    let (dm_tx, mut dm_rx) = mpsc::unbounded_channel();
    shared
        .routes
        .lock()
        .unwrap()
        .by_dm_user
        .insert(OWNER, dm_tx);
    for (name, on) in [("mute", true), ("unmute", false)] {
        let h = tokio::spawn(shared.clone().on_interaction(slash(None, 444, OWNER, name)));
        let got = tokio::time::timeout(Duration::from_secs(10), dm_rx.recv())
            .await
            .unwrap();
        let Some(PlaceMsg::Control { cmd, reply, .. }) = got else {
            panic!("not a control");
        };
        assert_eq!(cmd, Control::Mute(on), "/{name}");
        let _ = reply.send(if on { MUTED } else { UNMUTED }.into());
        h.await.unwrap();
    }
}
