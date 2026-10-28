//! `[content_based_security]` and `[privacy]`'s two new keys (theseus-gyjyo.2): the surface every later row of the
//! principles migration reads. What loads, what the old `[content]` says, and the one combination that refuses to
//! start.

use super::Config;

fn doc(rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"env:ANTHROPIC_API_KEY\"\n\n{rest}\n")
}

/// The old table is a hard error, and each key it held says where it went: not "unknown field".
#[test]
fn the_old_content_table_names_the_new_keys() {
    for (old, new) in [
        (
            "enabled = false",
            "`[content_based_security] session_flags`",
        ),
        (
            "injection = \"notify\"",
            "`[content_based_security] injection`",
        ),
        ("wait_ms = 1000", "`[content_based_security] wait_ms`"),
        ("max_pieces = 3", "`[content_based_security] max_pieces`"),
    ] {
        let key = old.split(' ').next().unwrap();
        let e = format!(
            "{:#}",
            Config::parse(&doc(&format!("[content]\n{old}"))).unwrap_err()
        );
        assert!(
            e.contains(&format!("`[content] {key}` was renamed to {new}")),
            "{key}: {e}"
        );
    }
}

/// With nothing judged `follow_flags` has no labels to read, and the old behaviour (refuse every restricted request)
/// is the hazard this removes: the config refuses to start, and says what to write instead.
#[test]
fn session_flags_off_with_follow_flags_refuses_to_start() {
    let e = format!(
        "{:#}",
        Config::parse(&doc("[content_based_security]\nsession_flags = false")).unwrap_err()
    );
    for word in ["session_flags", "follow_flags", "never", "always"] {
        assert!(e.contains(word), "{word}: {e}");
    }
    let e = format!(
        "{:#}",
        Config::parse(&doc(
            "[content_based_security]\nsession_flags = false\ncn_models = \"follow_flags\""
        ))
        .unwrap_err()
    );
    assert!(e.contains("follow_flags"), "{e}");
}

/// The way out in each direction loads, and so does every `cn_models` with the flags on.
#[test]
fn the_other_combinations_load() {
    for cn in ["never", "always"] {
        Config::parse(&doc(&format!(
            "[content_based_security]\nsession_flags = false\ncn_models = \"{cn}\""
        )))
        .unwrap_or_else(|e| panic!("{cn}: {e:#}"));
    }
    for cn in ["always", "follow_flags", "never"] {
        Config::parse(&doc(&format!(
            "[content_based_security]\nsession_flags = true\ncn_models = \"{cn}\""
        )))
        .unwrap_or_else(|e| panic!("{cn}: {e:#}"));
    }
    let e = format!(
        "{:#}",
        Config::parse(&doc("[content_based_security]\ncn_models = \"sometimes\"")).unwrap_err()
    );
    assert!(e.contains("sometimes"), "{e}");
}

/// An unknown key in the new table is refused rather than ignored.
#[test]
fn an_unknown_key_is_refused() {
    let e = format!(
        "{:#}",
        Config::parse(&doc("[content_based_security]\nenabled = true")).unwrap_err()
    );
    assert!(e.contains("enabled"), "{e}");
}

/// The keys the later rows read parse and load.
#[test]
fn the_keys_the_later_rows_read_parse() {
    Config::parse(&doc(
        "[content_based_security]\ncontext = \"A personal assistant.\"\nmax_pieces = 3\n\
         wait_ms = 0\ninjection = \"notify\"\n\
         [privacy]\npublic_paths = [\"~/projects/theseus\"]\nredact_outbound = false\n",
    ))
    .unwrap_or_else(|e| panic!("{e:#}"));
}
