//! `theseus self pause` and `unpause` (issue 6, theseus-7glm6): the verbs parse, and their answers say
//! what happened in words.

use clap::Parser;
use theseus_protocol::rsi::SelfPauseResult;

use super::backlog::pause_words;
use super::SelfCmd;

fn result(paused: bool, changed: bool, words: Option<&str>) -> SelfPauseResult {
    SelfPauseResult {
        paused,
        changed,
        words: words.map(str::to_string),
    }
}

#[test]
fn the_verbs_parse() {
    match crate::Cli::try_parse_from(["theseus", "self", "pause", "a", "benchmark", "runs"])
        .unwrap()
        .cmd
    {
        crate::Cmd::SelfImprove {
            cmd: SelfCmd::Pause { why },
        } => assert_eq!(why.join(" "), "a benchmark runs"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        crate::Cli::try_parse_from(["theseus", "self", "unpause"])
            .unwrap()
            .cmd,
        crate::Cmd::SelfImprove {
            cmd: SelfCmd::Unpause
        }
    ));
}

#[test]
fn the_answers_say_what_happened() {
    let words = "paused by the owner at 12:30: benchmarks. No new self build starts until it is unpaused or the daemon restarts";
    let set = pause_words(&result(true, true, Some(words)), false);
    assert!(
        set.starts_with("New self builds are paused by the owner"),
        "{set}"
    );
    assert!(set.contains("or the daemon restarts"), "{set}");
    assert!(pause_words(&result(true, false, Some(words)), false).starts_with("Already paused"));
    assert_eq!(
        pause_words(&result(false, true, None), true),
        "Unpaused: new self builds may start again."
    );
    assert_eq!(
        pause_words(&result(false, false, None), true),
        "Nothing was paused."
    );
}
