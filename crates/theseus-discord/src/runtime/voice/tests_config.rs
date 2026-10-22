//! A call's engine config from `[voice]` (theseus-sa4m5): the three switches the
//! config owns reach the engine as set, and a config with no `[voice]` section
//! starts a call with the guard and the echo cancel on and the judge at 250 ms.
//! The engine's own default has the first two off, so a line that stopped
//! copying one would leave the feature dead in production with every test green.

use std::time::Duration;

use theseus_core::config::Config;
use theseus_core::voice::VoiceConfig;
use theseus_voice::Speaker;

use super::engine_config;

fn call_with(section: &str) -> theseus_voice::Config {
    let (cfg, _) = Config::parse(&format!("{}\n{section}\n", Config::EXAMPLE_TOML)).unwrap();
    engine_config([Speaker(7)], &cfg.voice)
}

/// A config with no `[voice]` section: the guard and the echo cancel are on and
/// the judge waits 250 ms, though the engine's own default has the first two off.
#[test]
fn a_config_with_no_voice_section_starts_a_call_with_the_guard_and_echo_cancel_on() {
    let none = Config::example();
    let c = engine_config([Speaker(7)], &none.voice);
    assert!(c.join_guard, "the join guard is on by default");
    assert!(c.echo_cancel, "the echo cancel is on by default");
    assert_eq!(c.judge_ms, Duration::from_millis(250));
    // The section's own defaults agree with a config that never had one.
    assert_eq!(VoiceConfig::default(), none.voice);
}

/// Each switch in `[voice]` reaches the engine, alone: turning one off leaves
/// the others as they were.
#[test]
fn each_voice_switch_reaches_the_engine_alone() {
    let c = call_with("[voice]\njoin_guard = false");
    assert!(!c.join_guard && c.echo_cancel);
    let c = call_with("[voice]\necho_cancel = false");
    assert!(c.join_guard && !c.echo_cancel);
    let c = call_with("[voice]\njudge_ms = 400");
    assert_eq!(c.judge_ms, Duration::from_millis(400));
    assert!(c.join_guard && c.echo_cancel, "the others stay on");
    let c = call_with("[voice]\nenabled = true");
    assert!(
        c.join_guard && c.echo_cancel,
        "a section of other keys changes neither"
    );
    assert_eq!(c.judge_ms, Duration::from_millis(250));
}

/// The listed speakers are the call's, whatever the config.
#[test]
fn the_calls_speakers_are_the_engines_listed() {
    let c = engine_config([Speaker(7), Speaker(9)], &VoiceConfig::default());
    assert_eq!(c.listed.len(), 2);
    assert!(c.listed.contains(&Speaker(9)));
}

/// A call closes an utterance after `[voice] pause_ms`: 1200 ms with no section,
/// though the engine's own default is 700 (theseus-bp0lp).
#[test]
fn a_call_closes_an_utterance_after_the_configured_pause() {
    let none = Config::example();
    let c = engine_config([Speaker(7)], &none.voice);
    assert_eq!(c.end_of_utterance, Duration::from_millis(1_200));
    let c = call_with("[voice]\npause_ms = 1400");
    assert_eq!(c.end_of_utterance, Duration::from_millis(1_400));
}

/// A call merges an unfinished utterance with the next within `[voice] merge_ms`:
/// 1500 ms with no section, and 0 turns it off.
#[test]
fn a_call_merges_an_unfinished_utterance_within_the_configured_window() {
    let c = engine_config([Speaker(7)], &Config::example().voice);
    assert_eq!(c.merge, Duration::from_millis(1_500));
    let c = call_with("[voice]\nmerge_ms = 0");
    assert_eq!(c.merge, Duration::ZERO);
}
