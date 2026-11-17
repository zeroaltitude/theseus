//! `config.history` over a scratch state directory: the ledger's rows and the
//! kept copies, no daemon. Each fixture change is made as the daemon makes
//! one: the text before it kept as the next numbered copy, a row naming the
//! settings and both digests.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::*;
use crate::config_change::write;

const A: &str = "narrative = false\n\n[import.notes]\nenabled = true\n";
const B: &str = "narrative = false\n\n[import.notes]\nenabled = false\n";
const C: &str = "narrative = true\n\n[import.notes]\nenabled = false\n";

fn changed(n: u32, before: &str, after: &str, paths: &[&str], at: u64) -> LedgerRow {
    let mut r = LedgerRow::new(
        LedgerKind::ConfigChanged,
        Some("ses_1"),
        None,
        json!({
            "source": "owner", "question": format!("q{n}"), "paths": paths,
            "asked_by": {"session_id": "ses_1", "place": "discord:dm:42"},
            "approved_by": "cli", "via": "cli", "restart": true, "previous_copy": n,
            "digest_before": sha256(before), "digest_after": sha256(after),
        }),
    );
    r.at_unix_ms = at;
    r
}

/// A state directory and settings file where A became B (copy 1) and B became
/// C (copy 2).
fn kept() -> (tempfile::TempDir, std::path::PathBuf, Vec<LedgerRow>) {
    let dir = tempfile::tempdir().unwrap();
    write::keep_previous(dir.path(), A).unwrap();
    write::keep_previous(dir.path(), B).unwrap();
    let file = dir.path().join("theseus.toml");
    std::fs::write(&file, C).unwrap();
    let rows = vec![
        changed(1, A, B, &["import.notes.enabled"], 1_000),
        changed(2, B, C, &["narrative"], 2_000),
    ];
    (dir, file, rows)
}

fn names(e: &[Entry]) -> Vec<u32> {
    e.iter().map(|e| e.number).collect()
}

/// The last changes, newest first, each setting with its value before and
/// after: the before from the copy kept with it, the after from the next copy
/// or the file as it is.
#[test]
fn history_lists_the_changes_newest_first_with_before_and_after() {
    let (dir, file, rows) = kept();
    let list = entries(&rows, dir.path(), Some(&file), SHOWN);
    assert_eq!(names(&list), [2, 1]);
    let newest = list[0].settings().unwrap();
    assert_eq!(
        (&newest[0].before, &newest[0].after),
        (&Some(json!(false)), &Some(json!(true)))
    );
    let older = list[1].settings().unwrap();
    assert_eq!(
        (&older[0].before, &older[0].after),
        (&Some(json!(true)), &Some(json!(false)))
    );
    let said = words(&list, 12_000);
    assert!(said.contains("#2 (10s ago)"), "{said}");
    assert!(
        said.contains("asked in discord:dm:42; approved by cli; it needed a restart."),
        "{said}"
    );
    assert!(said.contains("- narrative: off → on"), "{said}");
    assert!(said.contains("- import.notes.enabled: on → off"), "{said}");
    assert!(
        said.find("#2").unwrap() < said.find("#1").unwrap(),
        "{said}"
    );
}

#[test]
fn the_limit_is_kept_and_a_change_made_without_a_place_says_where() {
    let (dir, file, mut rows) = kept();
    rows[1].data["asked_by"]["place"] = Value::Null;
    let list = entries(&rows, dir.path(), Some(&file), 1);
    assert_eq!(names(&list), [2]);
    let said = words(&list, 5_000);
    assert!(
        said.contains("asked in a conversation in the terminal or the web UI"),
        "{said}"
    );
}

/// A copy that is gone, or is not the one the row recorded, gives no values:
/// the change is listed by its settings and says why.
#[test]
fn a_copy_that_is_gone_or_was_altered_shows_no_values() {
    let (dir, file, rows) = kept();
    std::fs::remove_file(&write::previous(dir.path())[0].1).unwrap();
    std::fs::write(&write::previous(dir.path())[0].1, "narrative = true\n").unwrap();
    let list = entries(&rows, dir.path(), Some(&file), SHOWN);
    assert!(
        list[0].settings().is_none(),
        "the altered copy is not believed"
    );
    assert!(
        list[1].settings().is_none(),
        "the missing copy shows nothing"
    );
    let said = words(&list, 5_000);
    assert_eq!(
        said.matches("no longer kept, so the values are not shown")
            .count(),
        2
    );
    assert!(said.contains("- narrative ("), "{said}");
}

/// An "after" nobody kept is said, never guessed from the file as it is now.
#[test]
fn an_after_that_is_not_kept_is_said() {
    let (dir, _, rows) = kept();
    let edited = dir.path().join("theseus.toml");
    std::fs::write(
        &edited,
        "narrative = true\n[kernel]\nconfirm_ttl_secs = 5\n",
    )
    .unwrap();
    let list = entries(&rows, dir.path(), Some(&edited), SHOWN);
    assert!(list[0].after_doc.is_none());
    let said = words(&list, 5_000);
    assert!(
        said.contains("- narrative: off → (its value is no longer kept)"),
        "{said}"
    );
}

/// A rollback, an undo and an undone change are each said, and a change the
/// vault note's own rows describe is not listed.
#[test]
fn rollbacks_and_undos_are_said_and_the_vaults_rows_are_not_changes() {
    let (dir, file, mut rows) = kept();
    rows.push(LedgerRow::new(
        LedgerKind::ConfigRolledBack,
        Some("ses_1"),
        None,
        json!({"question": "q1", "paths": ["import.notes.enabled"], "why": "it was not healthy"}),
    ));
    let mut undo = changed(3, C, A, &["narrative"], 3_000);
    undo.data["question"] = json!("q3");
    rows.push(undo);
    rows.push(LedgerRow::new(
        LedgerKind::ConfigUndone,
        Some("ses_1"),
        None,
        json!({"question": "q3", "undone": 2, "undone_question": "q2", "paths": ["narrative"]}),
    ));
    rows.push(LedgerRow::new(
        LedgerKind::ConfigChanged,
        None,
        None,
        json!({"source": "vault", "paths": ["whatever"]}),
    ));
    let list = entries(&rows, dir.path(), Some(&file), SHOWN);
    assert_eq!(names(&list), [3, 2, 1]);
    assert_eq!((list[0].undoes, list[0].undone_by), (Some(2), None));
    assert_eq!(list[1].undone_by, Some(3));
    assert_eq!(list[2].rolled_back.as_deref(), Some("it was not healthy"));
    let said = words(&list, 5_000);
    assert!(
        said.contains("This put back the settings change #2 made."),
        "{said}"
    );
    assert!(said.contains("Undone since, by change #3."), "{said}");
    assert!(
        said.contains("went back to the settings before it after the restart: it was not healthy"),
        "{said}"
    );
}

#[test]
fn no_history_says_so_and_a_limit_is_clamped() {
    assert!(words(&[], 1).contains("No change to Theseus's settings"));
    assert_eq!(limit_of(&json!({})).unwrap(), SHOWN);
    assert_eq!(limit_of(&json!({"limit": 0})).unwrap(), 1);
    assert_eq!(limit_of(&json!({"limit": 500})).unwrap(), SHOWN);
    assert!(limit_of(&json!({"limit": 3, "toml": "x"})).is_err());
}
