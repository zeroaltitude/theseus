//! The stale edit (theseus-w2cs sub-item 2): a read record is noted when it
//! exists and checked when it exists. The rule is `docs/design/stale-edit.md`.
//!
//! The two that matter most are at the top: a write over a file that moved
//! under a read is refused in words that name the recovery, and a write to a
//! file this session never read is **unchanged**, which is what makes the
//! rule cheap. The rest hold the edges that would turn the guard into a
//! nuisance: two edits after one read, and a model that recovers by reading
//! again.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::fs::{Edit, Patch, Read, WriteFile};
use crate::reads::{Mark, Reads};
use crate::{Tool, ToolCtx};

/// A session's board, as the runtime's own (`theseus-core`'s
/// `toolrun::reads`) behaves, with no cap: the cap's own eviction is tested
/// there, and a test of two paths would never reach it.
#[derive(Debug, Default)]
struct Board(Mutex<HashMap<PathBuf, Mark>>);

impl Reads for Board {
    fn noted(&self, path: &Path, mark: Mark) {
        self.0.lock().unwrap().insert(path.to_path_buf(), mark);
    }
    fn mark_of(&self, path: &Path) -> Option<Mark> {
        self.0.lock().unwrap().get(path).copied()
    }
    fn wrote(&self, path: &Path, mark: Mark) {
        if let Some(m) = self.0.lock().unwrap().get_mut(path) {
            *m = mark;
        }
    }
}

/// A workspace with `a.txt`, and a context that carries a read record.
fn guarded() -> (tempfile::TempDir, ToolCtx) {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    let mut c = ToolCtx::for_tests(d.path());
    c.reads = Some(Arc::new(Board::default()));
    (d, c)
}

fn text(d: &tempfile::TempDir) -> String {
    fs::read_to_string(d.path().join("a.txt")).unwrap()
}

/// Something that is not a tool writes the file: a different length, so the
/// change is visible in the file's state whatever the clock's resolution.
fn changed_underneath(d: &tempfile::TempDir) {
    fs::write(d.path().join("a.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
}

fn read_it(c: &ToolCtx) {
    Read.run_with_media(&json!({"path": "a.txt"}), c)
        .map_err(|e| e.message)
        .unwrap();
}

fn edit_two(c: &ToolCtx) -> Result<String, String> {
    Edit.run(
        &json!({"path": "a.txt", "old_string": "two", "new_string": "TWO"}),
        c,
    )
    .map(|o| o.text)
    .map_err(|e| e.message)
}

#[test]
fn an_edit_of_a_file_that_moved_under_the_read_is_refused_in_words_that_name_the_recovery() {
    let (d, c) = guarded();
    read_it(&c);
    changed_underneath(&d);
    let why = edit_two(&c).expect_err("the edit should have been refused");
    assert!(why.contains("a.txt"), "names the path: {why}");
    assert!(
        why.contains("changed since you read it"),
        "says what happened: {why}"
    );
    assert!(
        why.contains("Read the file again, then write."),
        "names the recovery, read then write: {why}"
    );
    assert!(
        why.contains("Nothing was written."),
        "says the write did not land: {why}"
    );
    // And it did not land: the file is exactly what the other writer left.
    assert_eq!(text(&d), "one\ntwo\nthree\nfour\nfive\n");
}

#[test]
fn an_edit_with_no_read_at_all_writes_the_file_as_it_always_did() {
    // The non-regression the whole rule rests on. No `fs.read` ran, so the
    // board holds nothing for this path; the model may have learned the file
    // from `proc.run cat`, `git.diff` or an earlier session. It writes.
    let (d, c) = guarded();
    changed_underneath(&d);
    let said = edit_two(&c).expect("an unread file edits as it always did");
    assert!(said.contains("Replaced 1 occurrence"), "{said}");
    assert_eq!(text(&d), "one\nTWO\nthree\nfour\nfive\n");
}

#[test]
fn a_context_with_no_read_record_at_all_edits_as_before() {
    // Every `ToolCtx::for_tests` leaves `reads` None, which is how the 27
    // rig tests of the rejected shape stay green here by construction.
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    let c = ToolCtx::for_tests(d.path());
    assert!(c.reads.is_none());
    read_it(&c);
    changed_underneath(&d);
    assert!(edit_two(&c).is_ok());
    assert_eq!(text(&d), "one\nTWO\nthree\nfour\nfive\n");
}

#[test]
fn two_edits_after_one_read_both_land() {
    // The edge that would make the guard a nuisance: the first edit changes
    // the file, so without the note moving on with the write the second edit
    // would be refused by the first one's own work.
    let (d, c) = guarded();
    read_it(&c);
    edit_two(&c).expect("the first edit lands");
    let said = Edit
        .run(
            &json!({"path": "a.txt", "old_string": "three", "new_string": "THREE"}),
            &c,
        )
        .map(|o| o.text)
        .map_err(|e| e.message)
        .expect("the second edit lands too");
    assert!(said.contains("Replaced 1 occurrence"), "{said}");
    assert_eq!(text(&d), "one\nTWO\nTHREE\n");
}

#[test]
fn a_patch_over_a_file_that_moved_is_refused_and_writes_none_of_its_files() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.txt"), "one\n").unwrap();
    fs::write(d.path().join("b.txt"), "one\n").unwrap();
    let mut c = ToolCtx::for_tests(d.path());
    c.reads = Some(Arc::new(Board::default()));
    for f in ["a.txt", "b.txt"] {
        Read.run_with_media(&json!({"path": f}), &c).unwrap();
    }
    fs::write(d.path().join("b.txt"), "one\ntwo\n").unwrap();
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/b.txt\n+++ b/b.txt\n@@ -1 +1 @@\n-one\n+ONE\n";
    let why = Patch
        .run(&json!({"patch": patch}), &c)
        .map_err(|e| e.message)
        .expect_err("the patch should have been refused");
    assert!(why.contains("b.txt"), "names the moved file: {why}");
    assert!(why.contains("Read the file again, then write."), "{why}");
    // All files apply or none do, and none did.
    assert_eq!(fs::read_to_string(d.path().join("a.txt")).unwrap(), "one\n");
    assert_eq!(
        fs::read_to_string(d.path().join("b.txt")).unwrap(),
        "one\ntwo\n"
    );
}

#[test]
fn a_patch_after_a_read_lands_and_can_be_patched_again() {
    let (d, c) = guarded();
    read_it(&c);
    let one = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n-one\n+ONE\n two\n three\n";
    Patch
        .run(&json!({"patch": one}), &c)
        .map_err(|e| e.message)
        .expect("the first patch lands");
    let two = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n ONE\n-two\n+TWO\n three\n";
    Patch
        .run(&json!({"patch": two}), &c)
        .map_err(|e| e.message)
        .expect("and so does the second");
    assert_eq!(text(&d), "ONE\nTWO\nthree\n");
}

#[test]
fn a_write_brings_no_path_under_the_guard() {
    // `fs.write` replaces a file whole, so it never checks; and a write to a
    // path with no note must create none, or "no read, no refusal" would stop
    // being true for every file a session wrote without reading.
    let (d, c) = guarded();
    WriteFile
        .run(&json!({"path": "a.txt", "content": "fresh\n"}), &c)
        .map_err(|e| e.message)
        .unwrap();
    changed_underneath(&d);
    assert!(
        edit_two(&c).is_ok(),
        "a file only ever written is not guarded"
    );
    assert_eq!(text(&d), "one\nTWO\nthree\nfour\nfive\n");
}

#[test]
fn a_write_after_a_read_moves_the_note_on() {
    let (d, c) = guarded();
    read_it(&c);
    WriteFile
        .run(
            &json!({"path": "a.txt", "content": "one\ntwo\nthree\nfour\n"}),
            &c,
        )
        .map_err(|e| e.message)
        .unwrap();
    // The write changed the file; the next edit is its own session's work,
    // not a race, so it lands.
    assert!(edit_two(&c).is_ok());
    assert_eq!(text(&d), "one\nTWO\nthree\nfour\n");
}

#[test]
fn a_read_that_failed_notes_nothing() {
    let (d, c) = guarded();
    // A read of a path that is not there fails, and brings nothing under the
    // guard; `a.txt` is then edited although it moved, because it was never
    // handed over.
    assert!(Read
        .run_with_media(&json!({"path": "gone.txt"}), &c)
        .is_err());
    changed_underneath(&d);
    assert!(edit_two(&c).is_ok());
    assert_eq!(text(&d), "one\nTWO\nthree\nfour\nfive\n");
}

#[test]
fn two_sessions_keep_their_own_records() {
    // Each session has its own board, so one session's read does not guard
    // another's write. That is stated in the note as out of scope.
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    let mut a = ToolCtx::for_tests(d.path());
    a.reads = Some(Arc::new(Board::default()));
    let mut b = ToolCtx::for_tests(d.path());
    b.reads = Some(Arc::new(Board::default()));
    read_it(&a);
    changed_underneath(&d);
    assert!(edit_two(&b).is_ok(), "b never read it, so b writes");
    assert_eq!(text(&d), "one\nTWO\nthree\nfour\nfive\n");
}
