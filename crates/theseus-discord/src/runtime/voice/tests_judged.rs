//! Whether a call's engine gets a judge from `[voice] judge_unclear`
//! (theseus-sa4m5, the voice-jev review): the line that attaches it ignored
//! the switch in a plant and every test stayed green, since a call with a
//! judge that has nothing to say plays as one without.

use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{Config, Engine, Speaker, StandInSpeech, WavIo};

use super::super::tests::{core_edited, core_scripted};
use super::judged::attach;
use super::tests::place;

/// An engine as `start_call` builds one, before the judge: a call with no
/// speakers heard, never run.
fn engine() -> Engine {
    let io = Box::new(WavIo::new(Duration::from_secs(1)));
    Engine::new(
        Config::new([Speaker(7)]),
        io,
        Arc::new(StandInSpeech::new()),
    )
    .0
}

/// The engine of a call whose core has `[voice]` as `section` says, after
/// `attach` has had it.
fn attached(section: Option<bool>) -> bool {
    let d = tempfile::tempdir().unwrap();
    let core = match section {
        Some(on) => core_edited(d.path(), vec![], |c| c.voice.judge_unclear = on),
        None => core_scripted(d.path(), vec![]),
    };
    let (p, _rx) = place(&core, "ses_judged");
    let e = engine();
    assert!(!e.has_judge(), "the engine starts with none");
    attach(e, &p.shared, "ses_judged", "discord:channel:1").has_judge()
}

/// The default, and `judge_unclear = true`, attach the core's judge.
#[tokio::test]
async fn a_call_gets_the_judge_by_default_and_when_on() {
    assert!(attached(None), "a config with no [voice] section");
    assert!(attached(Some(true)));
}

/// `judge_unclear = false`: nothing is asked, and the engine has no judge.
#[tokio::test]
async fn judge_unclear_off_attaches_no_judge() {
    assert!(!attached(Some(false)));
}
