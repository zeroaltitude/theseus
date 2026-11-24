//! What the trim of unused judge questions and modes retired (issue 44): an
//! older settings file that still sets one of its keys loads, the key does
//! nothing, and the load says so in one line naming it.

use super::Config;

/// The least a settings file holds to load: a model key's reference.
const BASE: &str = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n";

fn parse(text: &str) -> anyhow::Result<(Config, Vec<String>)> {
    Config::parse(&format!("{BASE}{text}"))
}

/// The one warning that names the trim's retired keys.
fn trimmed(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("nothing reads it any more"))
        .collect()
}

/// `[judge.signals]` (CONTINUE's candidate signals) and a retired pack's
/// table load, and one line names them both.
#[test]
fn an_old_settings_file_with_the_judges_retired_keys_loads() {
    let text = "[judge]\n\
                [judge.signals]\n\
                dormancy_minutes = 1\n\
                tail_band = 0.25\n\
                [judge.packs.\"continue.v1\"]\n\
                rung = \"off\"\n\
                [judge.packs.\"role.v1\"]\n\
                sample = 0.5\n";
    let (cfg, warnings) = parse(text).unwrap();
    let lines = trimmed(&warnings);
    assert_eq!(lines.len(), 1, "{warnings:?}");
    for key in [
        "judge.signals",
        "judge.packs.\"continue.v1\"",
        "judge.packs.\"role.v1\"",
    ] {
        assert!(lines[0].contains(key), "{key}: {}", lines[0]);
    }
    assert_eq!(cfg.judge.retired().len(), 3);
    // A retired pack's table is never asked: nothing wires it.
    assert!(crate::judge::WIRED
        .iter()
        .all(|(p, _)| *p != "continue.v1" && *p != "role.v1"));
}

/// The template and an empty file set none of them.
#[test]
fn the_template_sets_no_retired_key() {
    let (_, warnings) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert!(trimmed(&warnings).is_empty(), "{warnings:?}");
    let (_, warnings) = parse("").unwrap();
    assert!(trimmed(&warnings).is_empty(), "{warnings:?}");
}

/// A pack no build ever had is still refused by name.
#[test]
fn an_unknown_pack_is_still_refused() {
    let e = parse("[judge.packs.\"nonesuch.v1\"]\nrung = \"off\"\n").unwrap_err();
    assert!(
        format!("{e:#}").contains("is no pack this build has"),
        "{e:#}"
    );
}

/// `[routing] rung` and `[memory] small_talk_bar` load whatever their value,
/// do nothing, and are named in the same one line. The route pack's own
/// `[judge.packs."<pack>"] rung` is what lowers routing now.
#[test]
fn an_old_settings_file_with_the_retired_modes_loads() {
    use crate::config::PackMode;
    let text = "[routing]\nrung = \"shadow\"\n\n[memory]\nsmall_talk_bar = true\n";
    let (cfg, warnings) = parse(text).unwrap();
    let lines = trimmed(&warnings);
    assert_eq!(lines.len(), 1, "{warnings:?}");
    for key in ["routing.rung", "memory.small_talk_bar"] {
        assert!(lines[0].contains(key), "{key}: {}", lines[0]);
    }
    assert_eq!(
        cfg.routing.pack_mode(PackMode::Live),
        PackMode::Live,
        "the retired rung lowers nothing"
    );
    let (cfg, warnings) = parse("[routing]\nrung = \"live\"\n").unwrap();
    assert_eq!(trimmed(&warnings).len(), 1, "{warnings:?}");
    assert_eq!(cfg.routing.trimmed(), ["routing.rung"]);
}

/// The one line says what changes when a retired key set something: a
/// shadow rung no longer holds routing back, and a small-talk bar that was on
/// no longer withholds recall. A value that changed nothing says nothing more.
#[test]
fn the_line_says_what_changes_with_each_retired_mode() {
    let route = crate::judge::inbound::ROUTE_PACK;
    let routing = format!("routing now follows `[judge.packs.\"{route}\"] rung` and may act live");
    let recall = "small-talk turns now carry admitted recall";
    let line = |text: &str| -> String {
        let (_, warnings) = parse(text).unwrap();
        let lines = trimmed(&warnings);
        assert_eq!(lines.len(), 1, "{warnings:?}");
        lines[0].clone()
    };
    let both = line("[routing]\nrung = \"shadow\"\n\n[memory]\nsmall_talk_bar = true\n");
    assert!(both.contains(&routing) && both.contains(recall), "{both}");
    let shadow = line("[routing]\nrung = \"shadow\"\n");
    assert!(
        shadow.contains(&routing) && !shadow.contains(recall),
        "{shadow}"
    );
    let bar = line("[memory]\nsmall_talk_bar = true\n");
    assert!(bar.contains(recall) && !bar.contains(&routing), "{bar}");
    for quiet in [
        "[routing]\nrung = \"live\"\n",
        "[memory]\nsmall_talk_bar = false\n",
    ] {
        let l = line(quiet);
        assert!(!l.contains("What changes"), "{l}");
    }
}

/// What the cut retired from the old way of building a request (issue 44): `[memory] summary_profile`
/// (compaction's summary), which the owner-shaped settings file sets, loads whatever its value, does nothing,
/// and a load warning names it. An unknown key in the section is still refused. (`[context] kept_mb` is the
/// retired-keys table's alone, the del-turn lane's.)
#[test]
fn an_old_settings_file_with_compactions_summary_profile_loads() {
    let (_, warnings) = parse("[memory]\nsummary_profile = \"haiku\"\n").unwrap();
    let named = warnings.iter().any(|w| w.contains("summary_profile"));
    assert!(named, "{warnings:?}");
    assert!(parse("[memory]\nsummary_profil = \"off\"\n").is_err());
}

/// A settings file written for route.v11, retired by route.v12 (issue 46), still loads: its table is read,
/// named in the one line, and applies to nothing. No alias carries it to the new version.
#[test]
fn a_settings_table_for_route_v11_loads_and_is_named_retired() {
    let (cfg, warnings) = parse("[judge.packs.\"route.v11\"]\nrung = \"shadow\"\n").unwrap();
    let lines = trimmed(&warnings);
    assert_eq!(lines.len(), 1, "{warnings:?}");
    assert!(
        lines[0].contains("judge.packs.\"route.v11\""),
        "{}",
        lines[0]
    );
    assert_eq!(cfg.judge.retired(), ["judge.packs.\"route.v11\""]);
    assert!(crate::judge::WIRED.iter().all(|(p, _)| *p != "route.v11"));
}
