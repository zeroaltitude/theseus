//! `[content_based_security]` and `[privacy]`'s two new keys (theseus-gyjyo.2): the surface every later row of the
//! principles migration reads. What loads, and what the old `[content]` says.

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

/// The retired `cn_models` loads with the flags off as well as on, whatever word it holds: the
/// combination that refused to start (flags off, `follow_flags`) loads too (2026-11-21).
#[test]
fn every_retired_cn_models_loads() {
    for flags in ["true", "false"] {
        for cn in ["always", "follow_flags", "never", "sometimes"] {
            Config::parse(&doc(&format!(
                "[content_based_security]\nsession_flags = {flags}\ncn_models = \"{cn}\""
            )))
            .unwrap_or_else(|e| panic!("{flags} {cn}: {e:#}"));
        }
    }
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

/// A config that names no `[content_based_security]` table at all — the owner's, and so the shipped behaviour —
/// leaves `injection` at `notify`: a labelled session's call runs and the operator is told. His ruling of
/// 2026-11-12 (S17-D8): L0 is unsandboxed and default-trusted, "we return to the notify but execute situation",
/// and Jev's classification of the content WARNS. An explicit `refuse` still freezes, for the specialised
/// security use L1 is kept for.
#[test]
fn no_table_at_all_leaves_injection_at_notify() {
    use crate::content::InjectionMode;
    for (rest, want, why) in [
        ("", InjectionMode::Notify, "no table at all"),
        (
            "[content_based_security]",
            InjectionMode::Notify,
            "an empty table",
        ),
        (
            "[content_based_security]\ninjection = \"notify\"",
            InjectionMode::Notify,
            "the default written out",
        ),
        (
            "[content_based_security]\ninjection = \"refuse\"",
            InjectionMode::Refuse,
            "refuse is not dead",
        ),
    ] {
        let (c, _) = Config::parse(&doc(rest)).unwrap_or_else(|e| panic!("{why}: {e:#}"));
        assert_eq!(c.content_based_security.injection, want, "{why}");
    }
}

/// The keys the later rows read parse and load.
#[test]
fn the_keys_the_later_rows_read_parse() {
    Config::parse(&doc(
        "[content_based_security]\ncontext = \"A personal assistant.\"\nmax_pieces = 3\n\
         wait_ms = 0\ninjection = \"notify\"\n\
         [privacy]\nopen_source_paths = [\"~/projects/theseus\"]\nredact_outbound = false\n",
    ))
    .unwrap_or_else(|e| panic!("{e:#}"));
}
