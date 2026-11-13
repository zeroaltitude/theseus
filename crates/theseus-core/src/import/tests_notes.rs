//! The agent-notes switch, through the whole core
//! (`b4-theseus-holds-its-own-notes`): with `[import.notes]` off every note
//! is refused and its refusal carries the durable-versus-run-state rule's
//! reading, which is what makes the off state the dry run; with the path on
//! a durable note is written and a run-state one still refused; with both
//! switches on the run-state one goes in too. The notes are invented and
//! hold no person's name and no real path.
//!
//! The rule itself is tested pure in `notes/tests.rs`; this file tests the
//! GATE — that the config reaches the write, that no client goes round it,
//! and that a refusal is not a store write.

use serde_json::{json, Value};

use super::tests::{import, lines};
use super::{episode, notes, session_id_of};
use crate::config::MemoryMode;
use crate::session::SessionRecord;
use crate::tests_recall::{rig_with, Rig};

/// A core whose `[import.notes]` switches are these two, and nothing else
/// changed from the import tests' own rig.
fn rig_with_notes(enabled: bool, keep_run_state: bool) -> Rig {
    rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.import.notes.enabled = enabled;
        c.import.notes.keep_run_state = keep_run_state;
    })
}

const TAG: &str = "agent-notes-2026-11";

/// A rules note: source (1) and (4)'s shape.
const RULES: &str = "\
- The gate runs before every commit, and you chain the commit with `&&`.
- Every commit subject is `area: what changed (<issue id>)`, in lower case.
- A lane never writes a shared file: those change only at the join.
";

/// A run log: most of source (2)'s shape.
const LOG: &str = "Ran the suite after the rebase. Pushed it. Waiting for the reviewer now.\n";

/// One note as the exporter writes it: one message holding the whole note,
/// its place `file` and the place's name the note's path.
fn note(name: &str, text: &str, n: u64) -> Value {
    let mut v = json!({
        "format": 1,
        "import_tag": TAG,
        "episode_id": format!("ep_{:064x}", 0x9f31_0000u64 + n),
        "source": episode::NOTES_SOURCE,
        "agent": "wren",
        "place": {"kind": "file", "name": name},
        "as_of": {"start": "2026-11-09T08:00:00Z", "end": "2026-11-09T08:00:00Z"},
        "labels": {"sensitivity": "company-confidential", "topic": ["theseus/building"]},
        "summary": null,
        "messages": [{
            "idx": 0, "time": "2026-11-09T08:00:00Z", "author": "wren",
            "integrity": "operator", "text": text,
            "unit": format!("note-{n}"), "sha256": "cd".repeat(32),
        }],
    });
    v["hash"] = json!(episode::hash_of(&v));
    v
}

/// The two notes every test sends: a rules note and a run log.
fn both() -> Vec<Value> {
    vec![
        note("refs/building.md", RULES, 1),
        note("notes/day.md", LOG, 2),
    ]
}

/// Whether the note's imported session is in the store.
fn stored(core: &std::sync::Arc<crate::Core>, e: &Value) -> bool {
    let sid = session_id_of(e["episode_id"].as_str().unwrap());
    core.store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .is_some()
}

/// The default. Both notes are refused, nothing is written, and each
/// refusal names the rule, its document and what it read the note as: the
/// off state is the dry run the owner is shown.
#[tokio::test]
async fn with_the_switch_off_every_note_is_refused_with_its_reading_and_nothing_is_written() {
    let r = rig_with_notes(false, false);
    let c = &r.core;
    let eps = both();
    let before = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(&eps)).await;
    assert_eq!((got.read, got.imported, got.nodes), (2, 0, 0), "{got:?}");
    assert_eq!(got.rejected.len(), 2, "{:?}", got.rejected);
    assert_eq!(
        c.store.stats().unwrap().frames_appended - before,
        0,
        "a refusal writes no frame"
    );
    for x in &got.rejected {
        assert!(x.why.contains("[import.notes] enabled is off"), "{x:?}");
        assert!(x.why.contains(notes::RULE_DOC), "it names the rule: {x:?}");
    }
    // The dry run's point: the reading, note by note, with nothing written.
    assert!(
        got.rejected[0].why.contains("durable (3 rule lines)"),
        "{:?}",
        got.rejected[0]
    );
    assert!(
        got.rejected[1]
            .why
            .contains("run state (it states no rule)"),
        "{:?}",
        got.rejected[1]
    );
    for e in &eps {
        assert!(!stored(c, e), "nothing was written for {}", e["place"]);
    }
}

/// The path on, run state still out: the durable note is written and the
/// run log is refused and named.
#[tokio::test]
async fn with_the_path_on_the_durable_note_is_written_and_the_run_log_is_refused() {
    let r = rig_with_notes(true, false);
    let c = &r.core;
    let eps = both();
    let got = import(c, lines(&eps)).await;
    assert_eq!((got.read, got.imported, got.nodes), (2, 1, 1), "{got:?}");
    assert_eq!(got.rejected.len(), 1, "{:?}", got.rejected);
    let why = &got.rejected[0].why;
    assert!(why.contains("keep_run_state is off"), "{why}");
    assert!(why.contains("run state"), "{why}");
    assert!(stored(c, &eps[0]), "the rules note went in");
    assert!(!stored(c, &eps[1]), "the run log did not");
}

/// Both on: the run-state half goes in as well, which is the only state in
/// which it does.
#[tokio::test]
async fn with_both_switches_on_the_run_log_goes_in_too() {
    let r = rig_with_notes(true, true);
    let c = &r.core;
    let eps = both();
    let got = import(c, lines(&eps)).await;
    assert_eq!((got.read, got.imported, got.nodes), (2, 2, 2), "{got:?}");
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    assert!(stored(c, &eps[0]) && stored(c, &eps[1]));
}

/// The switch is the NOTES source's alone: it holds up no other import. The
/// positive count is asserted beside the zero, so an empty batch cannot
/// pass this.
#[tokio::test]
async fn the_switch_holds_back_no_other_source() {
    let r = rig_with_notes(false, false);
    let c = &r.core;
    let mut other = note("refs/building.md", RULES, 7);
    other["source"] = json!(episode::SYNC_SOURCE);
    other["place"] = json!({"kind": "dm", "name": "quay"});
    other["hash"] = json!(episode::hash_of(&other));
    let eps = vec![other, note("notes/day.md", LOG, 8)];
    let got = import(c, lines(&eps)).await;
    assert_eq!((got.read, got.imported), (2, 1), "{got:?}");
    assert_eq!(got.rejected.len(), 1, "{:?}", got.rejected);
    assert!(stored(c, &eps[0]), "the sync's episode went in");
    assert!(!stored(c, &eps[1]), "the agent note did not");
}
