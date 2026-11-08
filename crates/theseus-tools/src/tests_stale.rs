//! A stale edit is refused (theseus-w2cs): `fs.edit` and `fs.patch` need a
//! read of the file in this session, and refuse one that changed since.

use std::fs;
use std::sync::Arc;

use serde_json::json;

use crate::fs::{Edit, Patch, Read, WriteFile};
use crate::{ReadRecord, ReadScope, Tool, ToolCtx};

struct Rig {
    d: tempfile::TempDir,
    record: Arc<ReadRecord>,
}

impl Rig {
    fn new() -> Self {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        Rig {
            d,
            record: Arc::default(),
        }
    }
    fn ctx(&self, session: u64) -> ToolCtx {
        let mut c = ToolCtx::for_tests(self.d.path());
        c.reads = Some(ReadScope::new(self.record.clone(), format!("s{session}")));
        c
    }
    fn file(&self) -> String {
        fs::read_to_string(self.d.path().join("a.txt")).unwrap()
    }
    fn read(&self, session: u64) {
        Read.run(&json!({"path": "a.txt"}), &self.ctx(session))
            .map_err(|e| e.message)
            .unwrap();
    }
    fn edit(&self, session: u64, old: &str, new: &str) -> Result<String, String> {
        Edit.run(
            &json!({"path": "a.txt", "old_string": old, "new_string": new}),
            &self.ctx(session),
        )
        .map(|o| o.text)
        .map_err(|e| e.message)
    }
}

const PATCH: &str = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";

#[test]
fn an_edit_of_a_file_never_read_is_refused() {
    let r = Rig::new();
    let e = r.edit(1, "two", "2").unwrap_err();
    assert!(e.contains("has not been read in this session"), "{e}");
    assert_eq!(r.file(), "one\ntwo\nthree\n");
}

#[test]
fn an_edit_after_a_read_lands_and_the_next_one_needs_no_second_read() {
    let r = Rig::new();
    r.read(1);
    r.edit(1, "two", "2").unwrap();
    r.edit(1, "three", "3").unwrap();
    assert_eq!(r.file(), "one\n2\n3\n");
}

#[test]
fn a_file_changed_since_the_read_is_refused_with_a_way_forward() {
    let r = Rig::new();
    r.read(1);
    fs::write(r.d.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    let e = r.edit(1, "two", "2").unwrap_err();
    assert!(e.contains("has changed since it was last read"), "{e}");
    assert!(e.contains("fs_read"), "{e}");
    assert_eq!(r.file(), "one\ntwo\nthree\nfour\n");
    // Read again, and the same edit lands.
    r.read(1);
    r.edit(1, "two", "2").unwrap();
    assert_eq!(r.file(), "one\n2\nthree\nfour\n");
}

#[test]
fn a_patch_is_held_to_the_same_rule() {
    let r = Rig::new();
    let run = |r: &Rig| {
        Patch
            .run(&json!({"patch": PATCH}), &r.ctx(1))
            .map(|o| o.text)
            .map_err(|e| e.message)
    };
    let e = run(&r).unwrap_err();
    assert!(e.contains("has not been read in this session"), "{e}");
    r.read(1);
    fs::write(r.d.path().join("a.txt"), "one\ntwo\nthree\n\n").unwrap();
    let e = run(&r).unwrap_err();
    assert!(e.contains("has changed since it was last read"), "{e}");
    r.read(1);
    run(&r).unwrap();
    assert_eq!(r.file(), "one\nTWO\nthree\n\n");
}

#[test]
fn another_sessions_read_does_not_count() {
    let r = Rig::new();
    r.read(1);
    let e = r.edit(2, "two", "2").unwrap_err();
    assert!(e.contains("has not been read in this session"), "{e}");
}

#[test]
fn a_write_by_the_session_counts_as_seen() {
    let r = Rig::new();
    WriteFile
        .run(&json!({"path": "a.txt", "content": "x\ny\n"}), &r.ctx(1))
        .map_err(|e| e.message)
        .unwrap();
    r.edit(1, "y", "z").unwrap();
    assert_eq!(r.file(), "x\nz\n");
}

#[test]
fn a_ctx_with_no_record_checks_nothing() {
    let r = Rig::new();
    let c = ToolCtx::for_tests(r.d.path());
    Edit.run(
        &json!({"path": "a.txt", "old_string": "two", "new_string": "2"}),
        &c,
    )
    .map_err(|e| e.message)
    .unwrap();
    assert_eq!(r.file(), "one\n2\nthree\n");
}
