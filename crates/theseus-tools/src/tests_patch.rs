//! `fs.patch` with hunk headers whose counts are off (theseus-inw): they are
//! recounted from the body, the result says so, and the context still has to
//! match the file.

use std::fs;

use serde_json::json;

use crate::fs::Patch;
use crate::{Tool, ToolCtx};

fn patched(patch: &str) -> (tempfile::TempDir, Result<String, String>) {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("src")).unwrap();
    fs::write(d.path().join("src/a.rs"), "one\ntwo\nthree\nfour\n").unwrap();
    let c = ToolCtx::for_tests(d.path());
    let r = Patch
        .run(&json!({ "patch": patch }), &c)
        .map(|o| o.text)
        .map_err(|e| e.message);
    (d, r)
}

fn read(d: &tempfile::TempDir) -> String {
    fs::read_to_string(d.path().join("src/a.rs")).unwrap()
}

#[test]
fn counts_off_by_one_either_way_apply_and_say_recounted() {
    for header in ["@@ -1,2 +1,2 @@", "@@ -1,4 +1,4 @@", "@@ -1,3 +1,2 @@"] {
        let patch = format!("--- a/src/a.rs\n+++ b/src/a.rs\n{header}\n one\n-two\n+TWO\n three\n");
        let (d, r) = patched(&patch);
        let text = r.unwrap();
        assert!(
            text.contains("recounted 1 hunk header in src/a.rs"),
            "{header}: {text}"
        );
        assert_eq!(read(&d), "one\nTWO\nthree\nfour\n", "{header}");
    }
}

#[test]
fn right_counts_read_as_before() {
    let patch = "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
    let (d, r) = patched(patch);
    assert_eq!(
        r.unwrap(),
        "Applied to 1 file:\n".to_string()
            + &d.path().join("src/a.rs").display().to_string()
            + " +1 -1"
    );
}

#[test]
fn two_hunks_recounted_are_named_once_with_their_count() {
    let patch = "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,1 +1,1 @@\n one\n-two\n+TWO\n@@ -3,9 +3,9 @@\n three\n-four\n+FOUR\n";
    let (d, r) = patched(patch);
    assert!(r.unwrap().contains("recounted 2 hunk headers in src/a.rs"));
    assert_eq!(read(&d), "one\nTWO\nthree\nFOUR\n");
}

/// A blank context line written empty, as models write it, is context.
#[test]
fn an_empty_line_inside_a_hunk_is_a_blank_context_line() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("b.txt"), "one\n\nthree\n").unwrap();
    let c = ToolCtx::for_tests(d.path());
    let patch = "--- a/b.txt\n+++ b/b.txt\n@@ -1,2 +1,2 @@\n one\n\n-three\n+THREE\n\n";
    let out = Patch.run(&json!({ "patch": patch }), &c).unwrap();
    assert!(out.text.contains("recounted 1 hunk header"), "{}", out.text);
    assert_eq!(
        fs::read_to_string(d.path().join("b.txt")).unwrap(),
        "one\n\nTHREE\n"
    );
}

/// A recount never makes a wrong hunk right: context that is not the
/// file's still fails, and nothing is written.
#[test]
fn a_recounted_hunk_whose_context_does_not_match_still_fails() {
    let patch = "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n tree\n";
    let (d, r) = patched(patch);
    let e = r.unwrap_err();
    assert!(e.contains("the patch does not apply to src/a.rs"), "{e}");
    assert_eq!(read(&d), "one\ntwo\nthree\nfour\n");
}

/// A recounted section that applies and one that does not: neither is
/// written (all or nothing holds with the recount).
#[test]
fn a_recount_keeps_all_or_nothing() {
    let patch = "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,9 +1,9 @@\n one\n-two\n+TWO\n--- /dev/null\n+++ b/src/new.rs\n@@ -0,0 +1,3 @@\n+x\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -4,1 +4,1 @@\n-nothere\n+x\n";
    let (d, r) = patched(patch);
    assert!(r.is_err());
    assert_eq!(read(&d), "one\ntwo\nthree\nfour\n");
    assert!(!d.path().join("src/new.rs").exists());
}

/// A removed line starting `-- ` beside an added one starting `++ ` reads
/// as a file's header to `split_patch` (`--- ` then `+++ `), so the hunk is
/// cut there: reported in theseus-inw's notes, not fixed here.
#[test]
fn a_removed_dashes_line_beside_an_added_pluses_line_splits_the_section() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("c.sql"), "select 1;\n-- old\n").unwrap();
    let c = ToolCtx::for_tests(d.path());
    let patch = "--- a/c.sql\n+++ b/c.sql\n@@ -1,2 +1,2 @@\n select 1;\n--- old\n+++ new\n";
    assert!(Patch.run(&json!({ "patch": patch }), &c).is_err());
    assert_eq!(
        fs::read_to_string(d.path().join("c.sql")).unwrap(),
        "select 1;\n-- old\n"
    );
}
