//! What the trim of unused judge questions and modes retired (issue 44): an
//! older settings file that still sets one of its keys loads, the key does
//! nothing, and the load says so in one line naming it.

use super::Config;

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
    let (cfg, warnings) = Config::parse(text).unwrap();
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
    let (_, warnings) = Config::parse("").unwrap();
    assert!(trimmed(&warnings).is_empty(), "{warnings:?}");
}

/// A pack no build ever had is still refused by name.
#[test]
fn an_unknown_pack_is_still_refused() {
    let e = Config::parse("[judge.packs.\"nonesuch.v1\"]\nrung = \"off\"\n").unwrap_err();
    assert!(
        format!("{e:#}").contains("is no pack this build has"),
        "{e:#}"
    );
}
