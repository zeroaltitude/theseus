//! `[personas.<name>]`'s fields (theseus-slvl.2): parsed, defaulted, and
//! refused with the allowed values named.

use super::Config;

fn doc(rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n")
}

/// Every field is optional; the name defaults to the key, capitalised, and
/// the initiative to `normal`.
#[test]
fn a_persona_has_optional_fields_with_defaults() {
    let (cfg, w) = Config::parse(&doc(
        "[personas.pier]\nfiles = [\"~/p.md\"]\nname = \"Pier\"\nemoji = \"x\"\ncard = \"Keeps the light.\"\n\
         tts_voice = \"aura\"\navatar = \"~/a.png\"\ninitiative = \"chatty\"\n\n[personas.quiet_one]\n",
    ))
    .unwrap();
    assert!(
        w.is_empty() || w.iter().all(|x| x.contains("default_persona")),
        "{w:?}"
    );
    let v = cfg.persona_for(Some("pier"), None);
    assert_eq!(
        (
            v.name.as_str(),
            v.emoji.as_deref(),
            v.card.as_deref(),
            v.tts_voice.as_deref()
        ),
        ("Pier", Some("x"), Some("Keeps the light."), Some("aura"))
    );
    assert_eq!(
        (v.avatar.as_deref(), v.initiative.as_str()),
        (Some("~/a.png"), "chatty")
    );
    let bare = cfg.persona_for(Some("quiet_one"), None);
    assert_eq!(
        (bare.name.as_str(), bare.initiative.as_str()),
        ("Quiet_one", "normal")
    );
    assert!(bare.files.is_empty() && bare.emoji.is_none());
}

/// A value the config cannot take is refused, naming the key and the allowed
/// values.
#[test]
fn a_bad_persona_field_is_refused_naming_the_allowed_values() {
    let e = format!(
        "{:#}",
        Config::parse(&doc("[personas.p]\ninitiative = \"loud\"")).unwrap_err()
    );
    assert!(
        e.contains("quiet") && e.contains("normal") && e.contains("chatty"),
        "{e}"
    );
    for (bad, key) in [
        ("emoji = \" \"", "personas.p.emoji"),
        ("name = \"a\\nb\"", "personas.p.name"),
        ("tts_voice = \"\"", "personas.p.tts_voice"),
    ] {
        let e = format!(
            "{:#}",
            Config::parse(&doc(&format!("[personas.p]\n{bad}"))).unwrap_err()
        );
        assert!(e.contains(key), "{e}");
    }
    let long = "x".repeat(601);
    let e = format!(
        "{:#}",
        Config::parse(&doc(&format!("[personas.p]\ncard = \"{long}\""))).unwrap_err()
    );
    assert!(e.contains("personas.p.card is at most 600"), "{e}");
    let e = format!(
        "{:#}",
        Config::parse(&doc("[personas.p]\nmood = \"x\"")).unwrap_err()
    );
    assert!(e.contains("unknown field `mood`"), "{e}");
}

/// A config that sets none of the new fields shows none of them.
#[test]
fn unset_fields_are_not_written_back() {
    let (cfg, _) = Config::parse(&doc("[personas.p]\nfiles = [\"~/p.md\"]")).unwrap();
    let shown = toml::to_string_pretty(&cfg).unwrap();
    for k in ["emoji", "card", "tts_voice", "avatar", "initiative"] {
        assert!(!shown.contains(k), "{k}: {shown}");
    }
}
