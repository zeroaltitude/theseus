//! `theseus agenda` against a scripted daemon (theseus-nuhsz): the board's
//! future views and `work.agenda`'s occurrences, drawn at a fixed clock in a
//! fixed zone, against a golden; and the requests it makes, all reads.
//! `THESEUS_GOLDEN=write` rewrites the golden.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// 2026-09-21 14:13:20 UTC, which is 07:13 at UTC−7.
const NOW: u64 = 1_790_000_000_000;
const HOUR: u64 = 3_600_000;

fn view(id: &str, kind: &str, title: &str, state: &str, level: &str) -> Value {
    json!({
        "id": id, "kind": kind, "position": 10, "at_ms": NOW, "parent": "exe_home",
        "root": "exe_home", "session_id": format!("ses_{id}"), "title": title,
        "state": state,
        "attention": {"level": level, "label": "ready", "since_ms": NOW},
        "started_at_ms": NOW, "state_since_ms": NOW,
    })
}

fn board() -> Value {
    let mut remind = view(
        "wak_a8cebb",
        "wake",
        "Call the harbour master",
        "waiting",
        "working",
    );
    remind["purpose"] = json!("remind");
    remind["next_at_ms"] = json!(NOW + 3 * HOUR);
    let mut routine = view("wak_a41d04", "routine", "Morning digest", "ready", "ready");
    routine["purpose"] = json!("routine");
    routine["next_at_ms"] = json!(NOW + 20 * HOUR);
    routine["series"] =
        json!({"every": "1d", "days": ["mon", "tue", "wed", "thu", "fri"], "occurrence": 22});
    routine["routine"] = json!({"schedule": "weekdays 08:00",
        "runs_30d": {"ok": 20, "failed": 1, "skipped": 2},
        "month": {"spent_usd": 2.5, "limit_usd": 10.0}, "deliver_to": "dm"});
    let mut watch = view(
        "wak_c3d4e5",
        "watch",
        "PR 123 merged?",
        "waiting",
        "working",
    );
    watch["purpose"] = json!("watch");
    watch["watch"] = json!({"check": "gh pr view 123", "until": "equals MERGED",
        "last": {"value": "OPEN", "at_ms": NOW - HOUR}, "checks": 611,
        "expires_at_ms": NOW + 72 * HOUR});
    json!({"position": 10, "views": [remind, routine, watch], "total": 0})
}

fn occurrences() -> Value {
    json!({"from_ms": NOW, "to_ms": NOW + 30 * 24 * HOUR, "lines": [
        {"at_ms": NOW + 3 * HOUR, "id": "wak_a8cebb", "kind": "wake", "what": "due",
         "title": "Call the harbour master", "purpose": "remind"},
        {"at_ms": NOW + 20 * HOUR, "id": "wak_a41d04", "kind": "routine", "what": "run",
         "title": "Morning digest", "purpose": "routine", "occurrence": 22},
    ]})
}

/// Run `theseus ARGS` against a daemon that answers the two reads, and give
/// what it printed and what it was asked.
fn run(args: &[&str]) -> (String, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut seen = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let req: Value = serde_json::from_str(&line).unwrap();
            let m = req["method"].as_str().unwrap_or("?").to_string();
            let result = match m.as_str() {
                "work.watch" => board(),
                "work.agenda" => occurrences(),
                _ => json!({"watching": false}),
            };
            seen.push(m);
            let a = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
        }
        seen
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(args)
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env("TZ", "<-07>7")
        .output()
        .unwrap();
    let seen = daemon.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (String::from_utf8_lossy(&out.stdout).into_owned(), seen)
}

#[test]
fn the_agenda_is_drawn_from_the_boards_future_and_the_occurrences() {
    let (out, seen) = run(&["agenda", "--now", &NOW.to_string()]);
    assert_eq!(
        seen,
        ["work.watch", "work.agenda", "work.unwatch"],
        "reads alone"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/agenda.txt");
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, &out).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(out, want, "THESEUS_GOLDEN=write rewrites it");
}

#[test]
fn json_is_the_two_answers_as_they_came() {
    let (out, _) = run(&["--json", "agenda", "--now", &NOW.to_string()]);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["board"]["views"].as_array().unwrap().len(), 3);
    assert_eq!(v["agenda"]["lines"].as_array().unwrap().len(), 2);
}
