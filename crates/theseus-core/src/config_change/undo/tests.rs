//! What an undo is built from, on texts alone: it puts back the settings of
//! ONE change and nothing else, and says what it overwrites.

use serde_json::{json, Value};

use super::*;
use crate::config_change::edit;

const A: &str =
    "narrative = false\n\n[kernel]\nconfirm_ttl_secs = 100\n\n[import.notes]\nenabled = true\n";
const B: &str =
    "narrative = false\n\n[kernel]\nconfirm_ttl_secs = 100\n\n[import.notes]\nenabled = false\n";
/// B, then `narrative` turned on by a later change.
const C: &str =
    "narrative = true\n\n[kernel]\nconfirm_ttl_secs = 100\n\n[import.notes]\nenabled = false\n";

fn p(s: &str) -> Vec<String> {
    edit::parse_path(s).unwrap()
}

fn entry(number: u32, before: Option<&str>, after: Option<&str>, paths: &[&str]) -> Entry {
    Entry {
        number,
        question: format!("q{number}"),
        at_ms: 1_000,
        paths: paths.iter().map(|x| p(x)).collect(),
        place: Some("discord:dm:42".into()),
        approved_by: "cli".into(),
        via: "cli".into(),
        restart: true,
        before_doc: before.and_then(history::doc_of),
        after_doc: after.and_then(history::doc_of),
        rolled_back: None,
        undone_by: None,
        undoes: None,
    }
}

/// The candidate an undo makes: only the change's settings move, the later
/// change to another setting stays.
#[test]
fn it_puts_back_the_settings_of_one_change_and_keeps_the_rest() {
    let list = [
        entry(2, Some(B), Some(C), &["narrative"]),
        entry(1, Some(A), Some(B), &["import.notes.enabled"]),
    ];
    let planned = plan(&list, Some(1), C).unwrap();
    assert_eq!(
        planned.changes,
        [Change::Set(p("import.notes.enabled"), json!(true))]
    );
    assert!(planned.undoes.since.is_empty());
    let candidate = edit::apply(C, &planned.changes).unwrap();
    assert!(candidate.contains("narrative = true"), "kept: {candidate}");
    assert!(candidate.contains("enabled = true"), "{candidate}");
    assert_ne!(candidate, A, "the old file is not written back");
    // The default is the newest change.
    let newest = plan(&list, None, C).unwrap();
    assert_eq!(newest.undoes.number, 2);
    assert_eq!(newest.changes, [Change::Set(p("narrative"), json!(false))]);
}

/// A setting the change added is removed again.
#[test]
fn a_setting_the_change_added_is_removed() {
    let before = "narrative = false\n";
    let after = "narrative = false\n[kernel]\nconfirm_ttl_secs = 7\n";
    let list = [entry(
        1,
        Some(before),
        Some(after),
        &["kernel.confirm_ttl_secs"],
    )];
    let planned = plan(&list, None, after).unwrap();
    assert_eq!(
        planned.changes,
        [Change::Remove(p("kernel.confirm_ttl_secs"))]
    );
    let candidate = edit::apply(after, &planned.changes).unwrap();
    assert!(!candidate.contains("confirm_ttl_secs"), "{candidate}");
}

/// A setting that was changed again since is put back too, and the card says
/// so, with what the change set, what it is now and what comes back; one
/// already as it was is left out and said.
#[test]
fn a_setting_changed_again_since_is_said_with_both_values() {
    let tables = |narrative: &str, ttl: u32, notes: &str| {
        format!(
            "narrative = {narrative}\n\n[kernel]\nconfirm_ttl_secs = {ttl}\n\n[import.notes]\nenabled = {notes}\n"
        )
    };
    let (before, after, now) = (
        tables("false", 100, "true"),
        tables("true", 50, "false"),
        // The owner put `narrative` back by hand, and moved the ttl again.
        tables("false", 70, "false"),
    );
    let paths = [
        "narrative",
        "kernel.confirm_ttl_secs",
        "import.notes.enabled",
    ];
    let list = [entry(1, Some(&before), Some(&after), &paths)];
    let planned = plan(&list, None, &now).unwrap();
    assert_eq!(planned.undoes.already, [p("narrative")]);
    assert_eq!(
        planned.changes,
        [
            Change::Set(p("kernel.confirm_ttl_secs"), json!(100)),
            Change::Set(p("import.notes.enabled"), json!(true)),
        ]
    );
    assert_eq!(planned.undoes.since.len(), 1, "{:?}", planned.undoes.since);
    let s = &planned.undoes.since[0];
    assert_eq!(
        (&s.made, &s.now, &s.back),
        (&Some(Some(json!(50))), &Some(json!(70)), &Some(json!(100)))
    );
    let said = preface(&planned.undoes, 61_000);
    assert!(
        said.contains("This puts back the settings change #1 made (1m ago)"),
        "{said}"
    );
    assert!(
        said.contains("Everything else stays as it is now"),
        "{said}"
    );
    assert!(
        said.contains("Already as they were before it, so not listed: narrative."),
        "{said}"
    );
    assert!(
        said.contains(
            "Changed again since, so this overwrites a later change:\n- kernel.confirm_ttl_secs: it set 50; it is now 70; this puts back 100"
        ),
        "{said}"
    );
    assert!(
        !said.contains("import.notes.enabled"),
        "unchanged-since settings are not flagged: {said}"
    );
}

/// When what the change set is no longer kept, the card cannot say whether the
/// setting was changed again, and says that instead of staying quiet.
#[test]
fn when_the_after_is_not_kept_it_is_said_not_assumed() {
    let list = [entry(1, Some(A), None, &["import.notes.enabled"])];
    let planned = plan(&list, None, B).unwrap();
    assert_eq!(planned.undoes.since[0].made, None);
    let said = preface(&planned.undoes, 2_000);
    assert!(said.contains("what it set is no longer kept"), "{said}");
}

#[test]
fn it_refuses_in_words_what_it_cannot_do() {
    let list = [
        entry(2, Some(B), Some(C), &["narrative"]),
        entry(1, Some(A), Some(B), &["import.notes.enabled"]),
    ];
    // No such change.
    let e = plan(&list, Some(9), C).err().unwrap();
    assert!(e.contains("no change #9"), "{e}");
    // Nothing made yet.
    assert!(plan(&[], None, C)
        .err()
        .unwrap()
        .contains("nothing to undo"));
    // The settings already hold their values from before the change.
    let e = plan(&list, Some(2), B).err().unwrap();
    assert!(
        e.contains("nothing to undo: every setting change #2 touched"),
        "{e}"
    );
    // Its copy is gone.
    let gone = [entry(3, None, None, &["narrative"])];
    let e = plan(&gone, None, C).err().unwrap();
    assert!(
        e.contains("no longer kept") && e.contains("cannot be undone"),
        "{e}"
    );
    // Already undone: the way back is to undo the undo.
    let mut done = entry(1, Some(A), Some(B), &["import.notes.enabled"]);
    done.undone_by = Some(2);
    let e = plan(&[done], Some(1), C).err().unwrap();
    assert!(
        e.contains("already undone, by change #2") && e.contains("undo #2"),
        "{e}"
    );
}

#[test]
fn what_the_question_carries_names_the_change_and_the_settings_changed_again() {
    let list = [entry(1, Some(A), None, &["import.notes.enabled"])];
    let planned = plan(&list, None, B).unwrap();
    let a: Value = planned.undoes.args();
    assert_eq!(a["number"], json!(1));
    assert_eq!(a["question"], json!("q1"));
    assert_eq!(a["since"], json!(["import.notes.enabled"]));
    let row = undone_row(
        "ses_1",
        "q9",
        &a,
        &json!({"paths": ["import.notes.enabled"]}),
        &json!({}),
        "cli",
        "cli",
    );
    assert_eq!(row.kind, "config.undone");
    assert_eq!(row.data["undone"], json!(1));
    assert_eq!(row.data["undone_question"], json!("q1"));
    assert_eq!(row.data["question"], json!("q9"));
    assert_eq!(row.data["changed_since"], json!(["import.notes.enabled"]));
}
