//! `--spawn` takes the daemon's path only when one is given (theseus-4356). The
//! help's own example, `theseus --spawn ask "..."`, has none: `ask` is the
//! subcommand. clap read it as the path, so the prompt became an unknown
//! subcommand and the run exited 2.

use clap::Parser;

use crate::{Cli, Cmd};

/// What `argv` parses to: the daemon `--spawn` names, and the ask's prompt.
fn spawned_ask(argv: &[&str]) -> (Option<String>, Option<String>) {
    let cli = Cli::try_parse_from(argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
    match cli.cmd {
        Cmd::Ask(a) => (cli.spawn, a.prompt),
        other => panic!("{argv:?} is not an ask: {other:?}"),
    }
}

/// The documented form: no path, then the subcommand and its prompt.
#[test]
fn a_bare_spawn_before_ask_leaves_ask_the_subcommand() {
    let (spawn, prompt) = spawned_ask(&["theseus", "--spawn", "ask", "chart the reef"]);
    assert_eq!(spawn.as_deref(), Some("theseusd"), "the default daemon");
    assert_eq!(prompt.as_deref(), Some("chart the reef"));
}

/// A path, with a space or with an equals sign, still names the daemon, and a
/// flag between `--spawn` and the subcommand changes nothing.
#[test]
fn a_spawn_path_still_names_the_daemon() {
    for argv in [
        [
            "theseus",
            "--spawn",
            "/opt/tide/bin/theseusd",
            "ask",
            "chart",
        ]
        .as_slice(),
        &["theseus", "--spawn=/opt/tide/bin/theseusd", "ask", "chart"],
        &[
            "theseus",
            "--spawn",
            "/opt/tide/bin/theseusd",
            "--json",
            "ask",
            "chart",
        ],
    ] {
        let (spawn, prompt) = spawned_ask(argv);
        assert_eq!(spawn.as_deref(), Some("/opt/tide/bin/theseusd"), "{argv:?}");
        assert_eq!(prompt.as_deref(), Some("chart"), "{argv:?}");
    }
    let (spawn, _) = spawned_ask(&["theseus", "--json", "--spawn", "ask", "chart"]);
    assert_eq!(spawn.as_deref(), Some("theseusd"), "after another flag");
}
