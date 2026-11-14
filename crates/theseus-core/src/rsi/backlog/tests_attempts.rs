//! A cap on attempts per item (issue 6, conditions 4 and 5): what a try is, read from the
//! item's own evidence; the notice's words; and the setting that holds the number.

use theseus_protocol::tasks::{TaskEvidence, TaskRecord};

use super::attempts::{capped, last_retry, marker, notice, of_cause, red_tonight, tries, CAPPED};
use crate::Config;

fn ev(identity: &str, note: &str, at_ms: u64) -> TaskEvidence {
    TaskEvidence {
        node: None,
        identity: identity.into(),
        note: (!note.is_empty()).then(|| note.into()),
        by: "test".into(),
        at_ms,
    }
}

fn item(id: &str, evidence: Vec<TaskEvidence>) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        title: "Trim the sails".into(),
        evidence,
        ..TaskRecord::default()
    }
}

/// KEY: a try is a red gate, a rejecting review, or a build that died with no gate; a green
/// gate, a gate that could not run and a build's end after its gate are not.
#[test]
fn a_try_is_read_from_the_evidence() {
    let t = item(
        "x1",
        vec![
            ev("session:s1", "", 1),
            ev("gate:red", "its gate ended red in suite", 2),
            ev("ended:no-branch", "its build ended", 3),
            ev("session:s2", "", 4),
            ev("ended:no-branch", "", 5),
            ev("session:s3", "", 6),
            ev("gate:unrunnable", "no room", 7),
            ev("ended:no-branch", "", 8),
            ev("session:s4", "", 9),
            ev("gate:green", "ended green", 10),
            ev(
                "review:self/x@abc",
                "rejected: no test fails without the fix",
                11,
            ),
        ],
    );
    let why: Vec<String> = tries(&t, 0).into_iter().map(|t| t.why).collect();
    assert_eq!(
        why,
        [
            "its gate ended red in suite",
            "its build ended with no green branch",
            "rejected: no test fails without the fix"
        ],
        "{why:?}"
    );
}

/// The owner's retry starts the count again, for the item and for its whole cause.
#[test]
fn the_owners_retry_starts_the_count_again() {
    let a = item(
        "x1",
        vec![
            ev("gate:red", "one", 10),
            ev("gate:red", "two", 20),
            ev(CAPPED, "failed after 2 tries", 21),
            ev("retried:owner", "", 30),
            ev("gate:red", "three", 40),
        ],
    );
    // A sibling of the same cause, failed before the retry.
    let b = item("x2", vec![ev("gate:red", "earlier", 5)]);
    assert_eq!(last_retry([&a, &b]), 30);
    let all: Vec<String> = of_cause(&[&a, &b]).into_iter().map(|t| t.why).collect();
    assert_eq!(all, ["three"], "only what came after the retry");
    assert!(!capped(&a), "the marker is older than the retry");
    let capped_now = item("x3", vec![ev(CAPPED, "failed after 3 tries", 50)]);
    assert!(capped(&capped_now));
}

/// Condition 5: a second red inside the night is not announced on its own account.
#[test]
fn a_red_notice_holds_the_next_ones_back_for_a_night() {
    let now = theseus_protocol::now_unix_ms();
    let hour = 3_600_000;
    let fresh = item("x1", vec![ev("gate:red", "red", now - hour)]);
    let old = item("x1", vec![ev("gate:red", "red", now - 13 * hour)]);
    let green = item("x1", vec![ev("gate:green", "green", now - hour)]);
    assert!(red_tonight(&fresh));
    assert!(!red_tonight(&old), "a new night, a new notice");
    assert!(!red_tonight(&green), "only a red holds a red back");
}

/// The one notice: plain words, every try's reason, the way back, and no internal names.
#[test]
fn the_notice_says_what_went_wrong_each_time_and_the_way_back() {
    let t = item(
        "x1",
        vec![
            ev("gate:red", "its gate ended red in suite", 1),
            ev("gate:red", "its gate ended red in clippy", 2),
            ev("gate:red", "its gate ended red in the door", 3),
        ],
    );
    let all = tries(&t, 0);
    let text = notice("x1", "Trim the sails", &all, 3);
    assert!(
        text.starts_with("ℹ️ FYI · Theseus stopped trying an item\n"),
        "{text}"
    );
    for want in [
        "\"Trim the sails\" did not work out after 3 tries, so Theseus stopped building it.",
        "Nothing waits for you.",
        "- try 1: its gate ended red in suite",
        "- try 3: its gate ended red in the door",
        "To let it try again: `theseus self retry x1`",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
    let m = marker(&all, 9);
    assert_eq!((m.identity.as_str(), m.at_ms), (CAPPED, 9));
    assert!(m
        .note
        .unwrap()
        .starts_with("failed after 3 tries: its gate ended red in suite | "));
}

/// The setting: three by default, named in the template, and never zero.
#[test]
fn the_cap_defaults_to_three_and_is_never_zero() {
    let mut cfg = Config::example();
    assert_eq!(cfg.self_improve.max_attempts, 3);
    assert!(Config::EXAMPLE_TOML.contains("\n# max_attempts = 3 "));
    cfg.validate().unwrap();
    cfg.self_improve.max_attempts = 0;
    let zero = cfg.validate().unwrap_err();
    assert!(format!("{zero:#}").contains("at least one try"), "{zero:#}");
}
