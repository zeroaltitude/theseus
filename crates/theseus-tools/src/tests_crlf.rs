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
