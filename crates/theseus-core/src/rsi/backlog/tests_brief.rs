//! The rules a self build is told first (issue 6, condition 1): what they say, in plain words.

use super::brief::rules;

#[test]
fn the_rules_name_formatting_a_tests_file_and_where_to_run_tests() {
    let text = rules(3);
    for want in [
        "uses one of your 3 tries",
        "already formatted",
        "tests file of its own",
        "never in the file you changed",
        "go with the change",
        "mod tests_<what>;",
        "cargo clippy -p <crate> --all-targets -- -D warnings",
        "the gate fails on any warning",
        "proc.run and sandbox: true",
        "nobody has to answer for it",
        "does not start a build while main itself is failing",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
    // The number is the setting's.
    assert!(rules(5).contains("one of your 5 tries"));
}

/// Written for a builder that is a model, but read by a person in the record: no internal
/// component's name stands in for Theseus.
#[test]
fn the_rules_name_no_internal_part() {
    let text = rules(3).to_lowercase();
    for word in ["kernel", "ledger", "pipeline", "rsi", "wal"] {
        assert!(!text.contains(word), "{word}: {text}");
    }
}
