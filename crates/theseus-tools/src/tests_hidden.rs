//! `fs.grep` and `fs.glob` take `all`, as `fs.list` does (theseus-w2cs):
//! hidden and ignored files are searched on request, and `.git` never is.

use std::fs;

use serde_json::json;

use crate::fs::{Glob, Grep};
use crate::{Tool, ToolCtx};

fn tree() -> (tempfile::TempDir, ToolCtx) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    fs::create_dir_all(p.join("sub")).unwrap();
    fs::create_dir_all(p.join(".secret")).unwrap();
    fs::create_dir_all(p.join(".git")).unwrap();
    fs::write(p.join(".gitignore"), "skipped.log\n").unwrap();
    for f in [
        "a.txt",
        "sub/b.txt",
        ".dot.txt",
        ".secret/c.txt",
        "skipped.log",
        ".git/config.txt",
    ] {
        fs::write(p.join(f), "needle\n").unwrap();
    }
    let c = ToolCtx::for_tests(p);
    (d, c)
}

/// The result's file lines: those that name a file under the tree.
fn files(d: &tempfile::TempDir, text: &str) -> Vec<String> {
    let root = d.path().canonicalize().unwrap().display().to_string();
    let mut v: Vec<String> = text
        .lines()
        .filter_map(|l| l.strip_prefix(&format!("{root}/")))
        .map(|l| l.split(':').next().unwrap().to_string())
        .collect();
    v.sort();
    v.dedup();
    v
}

fn grep(c: &ToolCtx, extra: serde_json::Value) -> Result<String, String> {
    let mut input = json!({"pattern": "needle", "output_mode": "files"});
    for (k, v) in extra.as_object().unwrap() {
        input[k] = v.clone();
    }
    Grep.run(&input, c).map(|o| o.text).map_err(|e| e.message)
}

#[test]
fn grep_skips_hidden_by_default_and_reaches_them_with_all() {
    let (d, c) = tree();
    let plain = grep(&c, json!({})).unwrap();
    assert_eq!(files(&d, &plain), vec!["a.txt", "sub/b.txt"], "{plain}");
    let all = grep(&c, json!({"all": true})).unwrap();
    assert_eq!(
        files(&d, &all),
        vec![".dot.txt", ".secret/c.txt", "a.txt", "skipped.log", "sub/b.txt"],
        "{all}"
    );
    assert!(!all.contains(".git/"), "{all}");
}

#[test]
fn glob_skips_hidden_by_default_and_reaches_them_with_all() {
    let (d, c) = tree();
    let run = |extra: serde_json::Value| {
        let mut input = json!({"pattern": "**/*.txt"});
        for (k, v) in extra.as_object().unwrap() {
            input[k] = v.clone();
        }
        Glob.run(&input, &c).map(|o| o.text).map_err(|e| e.message).unwrap()
    };
    let plain = run(json!({}));
    assert_eq!(files(&d, &plain), vec!["a.txt", "sub/b.txt"], "{plain}");
    let all = run(json!({"all": true}));
    assert_eq!(
        files(&d, &all),
        vec![".dot.txt", ".secret/c.txt", "a.txt", "sub/b.txt"],
        "{all}"
    );
    assert!(!all.contains(".git/"), "{all}");
}
