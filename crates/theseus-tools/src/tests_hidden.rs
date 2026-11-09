//! `fs.grep` and `fs.glob` take `all`, as `fs.list` does (theseus-w2cs):
//! hidden and ignored files are searched on request, and `.git` never is.

use std::fs;

use serde_json::json;

use crate::fs::{Glob, Grep, List};
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
        vec![
            ".dot.txt",
            ".secret/c.txt",
            "a.txt",
            "skipped.log",
            "sub/b.txt"
        ],
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
        Glob.run(&input, &c)
            .map(|o| o.text)
            .map_err(|e| e.message)
            .unwrap()
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

/// A tree whose ignored files are ignored by different means: `.git/info/exclude`
/// (`excluded.txt`), `.gitignore` (`ignored.txt`), and a dot name.
fn ignores() -> (tempfile::TempDir, ToolCtx) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    fs::create_dir_all(p.join(".git/info")).unwrap();
    fs::write(p.join(".git/info/exclude"), "excluded.txt\n").unwrap();
    fs::write(p.join(".gitignore"), "ignored.txt\n").unwrap();
    for f in [
        "a.txt",
        ".dot.txt",
        "excluded.txt",
        "ignored.txt",
        ".git/in.txt",
    ] {
        fs::write(p.join(f), "needle\n").unwrap();
    }
    let c = ToolCtx::for_tests(p);
    (d, c)
}

/// `fs.list`'s entry names: each line, indentation and size dropped.
fn list_names(c: &ToolCtx, all: bool) -> Vec<String> {
    let out = List
        .run(&json!({"all": all}), c)
        .map_err(|e| e.message)
        .unwrap();
    let mut v: Vec<String> = out
        .text
        .lines()
        .filter(|l| l.starts_with("  "))
        .map(|l| l.trim().split(" (").next().unwrap().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn list_glob_and_grep_mean_the_same_by_ignored() {
    let (d, c) = ignores();
    let glob = |all: bool| {
        let o = Glob
            .run(&json!({"pattern": "**/*.txt", "all": all}), &c)
            .map_err(|e| e.message)
            .unwrap();
        files(&d, &o.text)
    };
    let grep_files = |all: bool| files(&d, &grep(&c, json!({"all": all})).unwrap());
    let everything = [".dot.txt", "a.txt", "excluded.txt", "ignored.txt"];
    assert_eq!(glob(false), vec!["a.txt"]);
    assert_eq!(grep_files(false), vec!["a.txt"]);
    assert_eq!(glob(true), everything);
    assert_eq!(grep_files(true), everything);
    let plain = list_names(&c, false);
    assert_eq!(plain, vec!["a.txt"], "{plain:?}");
    // `.gitignore` itself is a hidden file, so `all` lists it too; `.git` never.
    let all = list_names(&c, true);
    let mut want: Vec<&str> = everything.to_vec();
    want.push(".gitignore");
    want.sort();
    assert_eq!(all, want, "{all:?}");
}
