//! The finder on hand-made ledgers (theseus-uttu): 20 approvals are a
//! candidate and 19 none; one decline, one "should have asked", one floor
//! call or a call the approve list asked about each rule a group out; a
//! shell is never a prefix; the stable prefix is the one chosen.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::find::{by_posture, find, prefix_ok, FindParams};
use crate::ledger::LedgerRow;

const NOW: u64 = 10 * 86_400_000;
const POSTURE: &str = "run `x` in /w: proc.run — approve (enforcement = approve)";

fn row(kind: &str, at: u64, data: Value) -> LedgerRow {
    let mut r = LedgerRow::named(kind, None, None, data);
    r.at_unix_ms = at;
    r
}

/// A question, as the gate's row says it.
fn ask(corr: &str, tool: &str, input: Value, reason: &str, floor: bool, at: u64) -> LedgerRow {
    row(
        "tool.confirm_requested",
        at,
        json!({"correlation_id": corr, "session_id": format!("ses_{}", at % 3), "execution_id": "e",
               "tool": tool, "input": input, "reason": reason, "by": "x",
               "requested_at_ms": at, "expires_at_ms": at + 1, "floor": floor}),
    )
}

fn approve(corr: &str, at: u64) -> LedgerRow {
    row(
        "action.confirmed",
        at,
        json!({"correlation_id": corr, "by": "the CLI"}),
    )
}

fn argv(words: &[&str]) -> Value {
    json!({"argv": words})
}

/// `n` approved asks of `words`, each with `extra` words after them.
fn approved(
    rows: &mut Vec<LedgerRow>,
    tag: &str,
    n: u64,
    words: &[&str],
    extra: &dyn Fn(u64) -> Vec<String>,
) {
    for i in 0..n {
        let mut w: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        w.extend(extra(i));
        let corr = format!("{tag}{i}");
        let at = NOW - 3 * 86_400_000 + i * 1_000;
        rows.push(ask(
            &corr,
            "proc.run",
            json!({"argv": w}),
            POSTURE,
            false,
            at,
        ));
        rows.push(approve(&corr, at + 500));
    }
}

fn none(_: u64) -> Vec<String> {
    vec![]
}

fn ids(_: &str) -> bool {
    false
}

fn run(rows: &mut [LedgerRow], min: u64) -> Vec<theseus_protocol::earned::EarnedGroup> {
    rows.sort_by_key(|r| r.at_unix_ms);
    let roots = vec![PathBuf::from("/w")];
    find(
        rows,
        &FindParams {
            now_ms: NOW,
            window_ms: 7 * 86_400_000,
            min_approvals: min,
            roots: &roots,
            skip: &ids,
        },
    )
}

#[test]
fn twenty_approvals_are_a_candidate_and_nineteen_none() {
    let mut rows = Vec::new();
    approved(
        &mut rows,
        "a",
        20,
        &["cargo", "nextest", "run", "-p", "theseus-store"],
        &none,
    );
    let found = run(&mut rows, 20);
    assert_eq!(found.len(), 1, "{found:?}");
    let g = &found[0];
    assert_eq!(g.label, "cargo nextest run -p theseus-store");
    assert_eq!(g.record.approvals, 20);
    assert_eq!(g.record.samples.len(), 1, "three at most, distinct");
    assert!(!g.open_ended);
    assert!(g.id.starts_with("ern_") && g.id.len() == 12);
    assert_eq!(g.record.sessions, 3);
    assert_eq!(g.record.digest.len(), 32);

    let mut rows = Vec::new();
    approved(
        &mut rows,
        "a",
        19,
        &["cargo", "nextest", "run", "-p", "theseus-store"],
        &none,
    );
    assert!(run(&mut rows, 20).is_empty(), "19 are not enough");
}

#[test]
fn the_stable_prefix_is_the_candidate_and_what_follows_it_varies() {
    let mut rows = Vec::new();
    // 30 and 27 approvals of two packages: two candidates, each narrow.
    approved(
        &mut rows,
        "a",
        30,
        &["cargo", "nextest", "run", "-p", "theseus-store"],
        &|i| {
            if i % 2 == 0 {
                vec!["--no-fail-fast".into()]
            } else {
                vec![]
            }
        },
    );
    approved(
        &mut rows,
        "b",
        27,
        &["cargo", "nextest", "run", "-p", "theseus-core"],
        &none,
    );
    // 57 commits, each its own message: one candidate, `git commit -m`.
    approved(&mut rows, "c", 57, &["git", "commit", "-m"], &|i| {
        vec![format!("message {i}")]
    });
    let found = run(&mut rows, 20);
    let labels: Vec<&str> = found.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "git commit -m",
            "cargo nextest run -p theseus-store",
            "cargo nextest run -p theseus-core"
        ],
        "most approved first: {found:?}"
    );
    assert!(found[0].open_ended, "anything follows `git commit -m`");
    assert!(found[1].open_ended, "`--no-fail-fast` followed the prefix");
    assert!(!found[2].open_ended);
}

#[test]
fn a_decline_a_press_or_a_floor_call_under_the_prefix_rules_it_out() {
    let words = ["cargo", "nextest", "run", "-p", "theseus-store"];
    let base = || {
        let mut rows = Vec::new();
        approved(&mut rows, "a", 25, &words, &none);
        rows
    };
    assert_eq!(run(&mut base(), 20).len(), 1);

    // One decline of a call under the prefix.
    let mut rows = base();
    let at = NOW - 86_400_000;
    rows.push(ask(
        "d1",
        "proc.run",
        argv(&["cargo", "nextest", "run", "-p", "theseus-store", "x"]),
        POSTURE,
        false,
        at,
    ));
    rows.push(row(
        "action.declined",
        at + 10,
        json!({"correlation_id": "d1", "tool": "proc.run", "by": "the CLI", "reason": "no"}),
    ));
    assert!(run(&mut rows, 20).is_empty(), "one decline");

    // A decline of another command leaves it be.
    let mut rows = base();
    rows.push(ask(
        "d1",
        "proc.run",
        argv(&["cargo", "publish"]),
        POSTURE,
        false,
        at,
    ));
    rows.push(row(
        "action.declined",
        at + 10,
        json!({"correlation_id": "d1", "tool": "proc.run"}),
    ));
    assert_eq!(run(&mut rows, 20).len(), 1);

    // One "should have asked" on a call under the prefix.
    let mut rows = base();
    rows.push(row(
        "policy.tightened",
        NOW - 1000,
        json!({"tool": "proc.run", "correlation_id": "a3"}),
    ));
    assert!(run(&mut rows, 20).is_empty(), "one press");

    // A call under the prefix that reached the floor, approved or not.
    let mut rows = base();
    rows.push(ask(
        "f1",
        "proc.run",
        argv(&[
            "cargo",
            "nextest",
            "run",
            "-p",
            "theseus-store",
            "/home/x/.theseus/store",
        ]),
        "floor",
        true,
        at,
    ));
    rows.push(approve("f1", at + 10));
    assert!(run(&mut rows, 20).is_empty(), "a floor path");

    // A decline before the window is no longer held against the group.
    let mut rows = base();
    let old = NOW - 9 * 86_400_000;
    rows.push(ask("d2", "proc.run", argv(&words), POSTURE, false, old));
    rows.push(row(
        "action.declined",
        old + 10,
        json!({"correlation_id": "d2", "tool": "proc.run"}),
    ));
    assert_eq!(run(&mut rows, 20).len(), 1);
}

#[test]
fn only_a_posture_ask_without_env_or_steps_counts() {
    let words = ["cargo", "check", "--workspace"];
    // Asked by the approve list, the roots or the external-text hold: no evidence.
    for why in [
        "run: proc.run — approve (`cargo check` matches the approve list entry `cargo`)",
        "run: proc.run — approve (/x is outside the workspace roots: /w)",
        "run: proc.run — approve (tightened by the CLI; the config says enforcement = notify)",
        "run: proc.run — approve (floor: x)",
    ] {
        let mut rows = Vec::new();
        for i in 0..25u64 {
            let at = NOW - 86_400_000 + i * 10;
            rows.push(ask(
                &format!("w{i}"),
                "proc.run",
                argv(&words),
                why,
                false,
                at,
            ));
            rows.push(approve(&format!("w{i}"), at + 1));
        }
        assert!(run(&mut rows, 20).is_empty(), "{why}");
    }
    // The external-text hold's asks.
    let mut rows = Vec::new();
    for i in 0..25u64 {
        let at = NOW - 86_400_000 + i * 10;
        let mut r = ask(
            &format!("h{i}"),
            "proc.run",
            argv(&words),
            POSTURE,
            false,
            at,
        );
        r.data["external_text"] = json!({"kind": "fetch", "what": "a page"});
        rows.push(r);
        rows.push(approve(&format!("h{i}"), at + 1));
    }
    assert!(
        run(&mut rows, 20).is_empty(),
        "the hold's questions are not the posture's"
    );
    // A call with an env, and a batch of steps.
    for input in [
        json!({"argv": words, "env": {"RUST_LOG": "debug"}}),
        json!({"steps": [{"argv": words}]}),
    ] {
        let mut rows = Vec::new();
        for i in 0..25u64 {
            let at = NOW - 86_400_000 + i * 10;
            rows.push(ask(
                &format!("e{i}"),
                "proc.run",
                input.clone(),
                POSTURE,
                false,
                at,
            ));
            rows.push(approve(&format!("e{i}"), at + 1));
        }
        assert!(run(&mut rows, 20).is_empty(), "{input}");
    }
    assert!(by_posture(POSTURE));
    assert!(by_posture(
        "a: fs.write — approve ([policy.tools] \"fs.write\" = approve)"
    ));
}

#[test]
fn a_shell_or_a_launcher_is_never_a_prefix() {
    let mut rows = Vec::new();
    approved(&mut rows, "s", 40, &["bash", "-c"], &|i| {
        vec![format!("echo {i}")]
    });
    approved(&mut rows, "t", 40, &["sh", "-c", "make"], &none);
    approved(&mut rows, "u", 40, &["env", "cargo", "test"], &none);
    approved(&mut rows, "v", 40, &["python3", "-c"], &|i| {
        vec![format!("print({i})")]
    });
    approved(&mut rows, "w", 40, &["xargs", "rm"], &none);
    assert!(run(&mut rows, 20).is_empty(), "{:?}", run(&mut rows, 20));
    for bad in [
        &["bash", "-c"][..],
        &["/bin/sh", "x"],
        &["sudo", "ls"],
        &["python3", "-c"],
        &["perl", "-e"],
        &["node", "--eval"],
        &["git", "-c", "core.pager=x"],
        &["ls"],
        &[],
        &["awk", "{print}"],
    ] {
        let p: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
        assert!(prefix_ok(&p).is_err(), "{bad:?}");
    }
    for good in [
        &["cargo", "test"][..],
        &["python3", "scripts/check.py"],
        &["git", "status"],
    ] {
        let p: Vec<String> = good.iter().map(|s| s.to_string()).collect();
        assert!(prefix_ok(&p).is_ok(), "{good:?}");
    }
}

#[test]
fn a_file_tool_groups_by_tool_and_root_and_a_program_by_its_base_name() {
    let mut rows = Vec::new();
    for i in 0..22u64 {
        let at = NOW - 86_400_000 + i * 10;
        let why = "write: fs.write — approve (enforcement = approve)";
        rows.push(ask(
            &format!("f{i}"),
            "fs.write",
            json!({"path": format!("/w/src/f{i}.rs"), "content": "x"}),
            why,
            false,
            at,
        ));
        rows.push(approve(&format!("f{i}"), at + 1));
        // Outside every root: no group.
        rows.push(ask(
            &format!("o{i}"),
            "fs.write",
            json!({"path": format!("/elsewhere/f{i}"), "content": "x"}),
            why,
            false,
            at + 2,
        ));
        rows.push(approve(&format!("o{i}"), at + 3));
    }
    approved(
        &mut rows,
        "p",
        21,
        &["/usr/bin/cargo", "fmt", "--all"],
        &none,
    );
    let found = run(&mut rows, 20);
    let labels: Vec<&str> = found.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(
        labels,
        ["fs.write under /w", "cargo fmt --all"],
        "{found:?}"
    );
    let f = found.iter().find(|g| g.tool == "fs.write").unwrap();
    assert_eq!(f.root.as_deref(), Some("/w"));
    assert!(f.prefix.is_empty());
    let p = found.iter().find(|g| g.tool == "proc.run").unwrap();
    assert_eq!(
        p.prefix,
        ["cargo", "fmt", "--all"],
        "the program by its base name"
    );
}

#[test]
fn a_group_already_open_or_passed_on_is_skipped() {
    let mut rows = Vec::new();
    approved(&mut rows, "a", 25, &["cargo", "fmt", "--all"], &none);
    rows.sort_by_key(|r| r.at_unix_ms);
    let roots = vec![PathBuf::from("/w")];
    let id = super::find::id_of(&super::find::key_of_prefix(&[
        "cargo".into(),
        "fmt".into(),
        "--all".into(),
    ]));
    let skip = |i: &str| i == id;
    let found = find(
        &rows,
        &FindParams {
            now_ms: NOW,
            window_ms: 7 * 86_400_000,
            min_approvals: 20,
            roots: &roots,
            skip: &skip,
        },
    );
    assert!(found.is_empty());
}
