//! CRLF files edit normally (theseus-w2cs): `fs.read` shows a line without its
//! `\r`, so a model quotes bare `\n`; the edit and the patch must still land,
//! and leave every other line's ending as it was.

use std::fs;

use serde_json::json;

use crate::fs::{Edit, Patch};
use crate::{Tool, ToolCtx};

fn crlf_file(content: &[u8]) -> (tempfile::TempDir, ToolCtx) {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.txt"), content).unwrap();
    let c = ToolCtx::for_tests(d.path());
    (d, c)
}

fn bytes(d: &tempfile::TempDir) -> Vec<u8> {
    fs::read(d.path().join("a.txt")).unwrap()
}

#[test]
fn a_multi_line_old_string_with_bare_newlines_edits_a_crlf_file() {
    let (d, c) = crlf_file(b"one\r\ntwo\r\nthree\r\nfour\r\n");
    let out = Edit
        .run(
            &json!({"path": "a.txt", "old_string": "two\nthree", "new_string": "TWO\nTHREE\nextra"}),
            &c,
        )
        .map_err(|e| e.message)
        .unwrap();
    assert!(out.text.contains("Replaced 1 occurrence"), "{}", out.text);
    assert_eq!(
        bytes(&d),
        b"one\r\nTWO\r\nTHREE\r\nextra\r\nfour\r\n".to_vec()
    );
}

#[test]
fn a_single_line_edit_leaves_every_other_ending_alone() {
    let (d, c) = crlf_file(b"one\r\ntwo\nthree\r\nfour");
    Edit.run(
        &json!({"path": "a.txt", "old_string": "three", "new_string": "3"}),
        &c,
    )
    .map_err(|e| e.message)
    .unwrap();
    assert_eq!(bytes(&d), b"one\r\ntwo\n3\r\nfour".to_vec());
}

#[test]
fn an_lf_file_is_untouched_by_the_normalising() {
    let (d, c) = crlf_file(b"one\ntwo\nthree\n");
    Edit.run(
        &json!({"path": "a.txt", "old_string": "one\ntwo", "new_string": "1\n2"}),
        &c,
    )
    .map_err(|e| e.message)
    .unwrap();
    assert_eq!(bytes(&d), b"1\n2\nthree\n".to_vec());
}

#[test]
fn a_patch_made_from_a_read_applies_to_a_crlf_file_and_keeps_its_endings() {
    let (d, c) = crlf_file(b"one\r\ntwo\r\nthree\r\nfour\r\n");
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,4 +1,4 @@\n one\n-two\n+TWO\n three\n four\n";
    Patch
        .run(&json!({ "patch": patch }), &c)
        .map_err(|e| e.message)
        .unwrap();
    assert_eq!(bytes(&d), b"one\r\nTWO\r\nthree\r\nfour\r\n".to_vec());
}

const MIXED: &[u8] = b"one\r\ntwo\nthree\r\nfour\n";

#[test]
fn a_quote_across_a_crlf_and_an_lf_line_edits_a_mixed_file_in_place() {
    let (d, c) = crlf_file(MIXED);
    let out = Edit
        .run(
            &json!({"path": "a.txt", "old_string": "one\ntwo\nthree", "new_string": "ONE\nTWO\nTHREE"}),
            &c,
        )
        .map_err(|e| e.message)
        .unwrap();
    assert!(out.text.contains("Replaced 1 occurrence"), "{}", out.text);
    assert_eq!(bytes(&d), b"ONE\r\nTWO\nTHREE\r\nfour\n".to_vec());
}

#[test]
fn a_line_added_over_a_mixed_region_takes_the_regions_last_ending() {
    // The region's endings are CRLF then LF; the replacement's newlines take
    // them in order, and any beyond take the last (LF here).
    let (d, c) = crlf_file(MIXED);
    Edit.run(
        &json!({"path": "a.txt", "old_string": "one\ntwo\nthree", "new_string": "A\nB\nC\nD"}),
        &c,
    )
    .map_err(|e| e.message)
    .unwrap();
    assert_eq!(bytes(&d), b"A\r\nB\nC\nD\r\nfour\n".to_vec());
}

#[test]
fn replace_all_and_uniqueness_count_mixed_regions_like_matches() {
    // Neither region holds the quote as given (each has a CRLF), so both go
    // through the mixed-ending match.
    let (d, c) = crlf_file(b"a\r\nb\nc\nx\na\r\nb\r\nc\ny\n");
    let err = Edit
        .run(
            &json!({"path": "a.txt", "old_string": "a\nb\nc", "new_string": "Z"}),
            &c,
        )
        .map(|_| ())
        .unwrap_err();
    assert!(err.message.contains("occurs 2 times"), "{}", err.message);
    Edit.run(
        &json!({"path": "a.txt", "old_string": "a\nb\nc", "new_string": "P\nQ\nR", "replace_all": true}),
        &c,
    )
    .map_err(|e| e.message)
    .unwrap();
    assert_eq!(bytes(&d), b"P\r\nQ\nR\nx\nP\r\nQ\r\nR\ny\n".to_vec());
}
