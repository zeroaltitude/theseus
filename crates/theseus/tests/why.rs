#![recursion_limit = "512"]
//! `theseus why` (theseus-ighf): the turn's context, why each piece was
//! there, and the gate's posture for each tool it called, from a scripted
//! daemon: the words (golden, the same renderer as Discord's `/why`), a turn
//! named by a prefix, `--json` as the daemon answered, and the requests it
//! made: reads alone, so nothing is written.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
const S: &str = "ses_reef42";
const T: &str = "trn_k3m9x2";
const OLDER: &str = "trn_b7q1w4";

/// The requests the scripted daemon expects, in order, each with its answer.
type Script = Vec<(&'static str, Value)>;

fn run(args: &[&str], script: Script) -> String {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut seen: Vec<String> = Vec::new();
        for (method, result) in script {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let req: Value = serde_json::from_str(&line).unwrap();
            seen.push(format!(
                "{} {}",
                req["method"].as_str().unwrap(),
                req["params"]
            ));
            let answer = if req["method"] == method {
                json!({"jsonrpc": "2.0", "id": req["id"], "result": result})
            } else {
                json!({"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32601,
                    "message": format!("the script expected {method}")}})
            };
            (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
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
    format!(
        "$ theseus {}\n--- requests\n{}\n--- stdout\n{}--- stderr\n{}--- exit {}\n",
        args.join(" "),
        seen.join("\n"),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
        out.status.code().unwrap_or(-1)
    )
}

fn golden(name: &str, got: &str) {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "golden",
        &format!("{name}.txt"),
    ]
    .iter()
    .collect();
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (write it with THESEUS_GOLDEN=write)",
            path.display()
        )
    });
    assert_eq!(got, want, "{name} differs from {}", path.display());
}

fn sessions() -> Value {
    json!({"sessions": [{"session_id": S, "kind": "conversation", "label": null,
        "created_at_unix_ms": 1_759_300_000_000u64, "turns": 2}]})
}

/// `context.explain` for `turn`: the newest turn compiled `T`, an older one
/// `OLDER`. A private place's, with a recall from an imported chat and one
/// from this conversation, a drop for the budget and one for the place.
fn explain(turn: &str) -> Value {
    json!({
        "session_id": S, "title": "the harbour survey", "kind": "conversation",
        "class": "private", "place": "CLI",
        "turns": [
            {"turn_id": T, "at_ms": 1_759_300_100_000u64, "loops": 2, "est_tokens": 11780, "compilation_id": "cmp_a"},
            {"turn_id": OLDER, "at_ms": 1_759_300_000_000u64, "loops": 1, "est_tokens": 9000, "compilation_id": "cmp_a"}],
        "turn_id": turn,
        "profile": "deep", "model": "glm-x", "window": 200000,
        "parts": [
            {"block": "header", "name": "persona", "bytes": 4800, "tokens": 1200, "then": "same"},
            {"block": "context", "name": "AGENTS.md", "bytes": 9000, "tokens": 2250, "then": "same"},
            {"block": "tools", "name": "tools", "bytes": 12000, "tokens": 3000, "note": "fs.read, proc.run"},
            {"block": "conversation", "name": "conversation", "bytes": 20000, "tokens": 5000}],
        "digest_now": "d1", "digest_then": "d1", "unchanged": true,
        "recalls": [{
            "recall_id": "rcl_1", "mode": "shadow", "science": "baseline@ab12", "outcome": "ran",
            "place": "private", "query_chars": 40, "query_digest": "0011223344556677",
            "candidates": 14, "sources": {"bm25": 9, "vector": 8},
            "admitted": [
                {"node_id": "nod_a", "chunk": 0, "session_id": "ses_ep9", "position": 5, "kind": "message",
                 "rank": 1, "fused": 0.93, "tokens": 210,
                 "sources": {"bm25": {"rank": 1, "score": 4.2}, "vector": {"rank": 3, "score": 0.81}}},
                {"node_id": "nod_b", "chunk": 0, "session_id": S, "position": 9, "kind": "note",
                 "rank": 2, "fused": 0.71, "tokens": 90, "sources": {"entity": {"rank": 1, "score": 1.0}}}],
            "dropped": [
                {"node_id": "nod_c", "chunk": 0, "session_id": "ses_ep9", "reason": "budget", "fused": 0.4, "tokens": 300},
                {"node_id": "nod_d", "chunk": 0, "session_id": "ses_priv1", "reason": "place", "fused": 0.3}],
            "drops": {"budget": 1, "place": 1}, "budget_tokens": 400, "used_tokens": 300,
            "timings": {"index_ms": 3.0, "pack_ms": 1.0, "total_ms": 5.0, "deadline_ms": 250}}],
        "noted": {"nod_a": 1772640000000u64, "nod_b": 1772700000000u64},
        "sources": {
            "ses_ep9": {"place": "CLI", "imported": {
                "session_id": "ses_ep9", "episode_id": "ep9", "tag": "chat", "source": "chat export",
                "place_kind": "chat", "place_name": "tide office", "start_ms": 1_772_582_400_000u64,
                "end_ms": 1_772_668_800_000u64, "sensitivity": "personal", "topics": [],
                "messages": 3, "summary": true, "file": "chat.jsonl", "line": 1, "imported_at_ms": 1}},
            S: {"place": "CLI", "title": "the harbour survey"}},
        "ms": 4.0
    })
}

fn policy() -> Value {
    json!({"roots": ["/work"], "places": [{
        "place": "cli", "name": "CLI", "class": "private", "session_id": S,
        "tools": [
            {"tool": "fs.read", "class": "read", "offered": true, "result": "open", "reason": "r",
             "layers": [{"layer": "posture", "says": "open", "result": "open"}]},
            {"tool": "proc.run", "class": "run", "offered": true, "result": "approve", "reason": "r",
             "layers": [{"layer": "posture", "says": "approve", "result": "open"},
                        {"layer": "floor", "says": "the place's floor asks", "result": "approve",
                         "setting": "[policy] floor = \"approve\"", "raised": true}]}]}]})
}

/// The turn's `route.decided` row, and another turn's.
fn route() -> Value {
    let row = |turn: &str, pos: u64, mode: &str| {
        json!({"position": pos, "at_unix_ms": 1, "kind": "route.decided", "session_id": S,
            "turn_id": turn, "data": {"mode": mode, "reason": "verdict", "from": "quick", "profile": "deep",
            "effort": "high", "effort_reason": "applied", "effort_applied": true, "effort_ran": "high"}})
    };
    json!({"rows": [row(T, 50, "deep_coding"), row(OLDER, 20, "chat")], "total": 2})
}

fn calls() -> Value {
    let node = |id: &str, turn: &str, pos: u64, tool: &str, gate: &str| {
        json!({"node_id": id, "kind": "tool_call", "session_id": S, "position": pos, "at_unix_ms": 1,
            "turn_id": turn, "text": "", "bytes": 0,
            "detail": {"tool": tool, "gate": {"result": {"gate": gate}, "decision": {"reason": "r"}}}})
    };
    json!({"total": 3, "nodes": [
        node("n1", T, 30, "proc_run", "approve"),
        node("n2", OLDER, 10, "fs_read", "open"),
        node("n3", T, 40, "fs_read", "open")]})
}

fn script(turn: &str) -> Script {
    vec![
        ("context.explain", explain(turn)),
        ("policy.explain", policy()),
        ("ledger.tail", route()),
        ("node.list", calls()),
    ]
}

#[test]
fn why_prints_the_last_turn_its_route_its_recalls_its_drops_and_its_gated_tools() {
    let mut s = vec![("session.list", sessions())];
    s.extend(script(T));
    let got = run(&["why"], s);
    assert!(
        got.contains("routing: deep_coding, by the route pack's verdict"),
        "{got}"
    );
    assert!(
        got.contains("recall: 1 candidate left out for the budget"),
        "{got}"
    );
    assert!(
        got.contains("proc_run               approve  set by floor"),
        "{got}"
    );
    assert!(
        !got.contains("chat, by the route"),
        "another turn's route stayed out: {got}"
    );
    golden("why", &got);
}

#[test]
fn a_prefix_names_an_older_turn_and_a_session_flag_names_the_session() {
    let mut s = vec![("context.explain", explain(T))];
    s.extend(script(OLDER));
    let got = run(&["why", "--session", S, "b7q1w4"], s);
    assert!(got.contains(&format!("turn {OLDER} ")), "{got}");
    assert!(
        got.contains("routing: chat, by the route pack's verdict"),
        "{got}"
    );
    assert!(got.contains("fs_read"), "{got}");
    assert!(
        !got.contains("proc_run"),
        "the newer turn's call stayed out: {got}"
    );
    golden("why_older", &got);
}

#[test]
fn an_unknown_or_too_short_turn_is_refused_before_anything_else_is_read() {
    let got = run(
        &["why", "--session", S, "zzzzzz"],
        vec![("context.explain", explain(T))],
    );
    assert!(
        got.contains("no turn of this session is named `zzzzzz`"),
        "{got}"
    );
    assert!(got.contains("--- exit 1"), "{got}");
    let got = run(
        &["why", "--session", S, "b7"],
        vec![("context.explain", explain(T))],
    );
    assert!(got.contains("too short to name a turn"), "{got}");
}

#[test]
fn json_prints_what_the_daemon_answered_and_every_request_is_a_read() {
    let mut s = vec![("session.list", sessions())];
    s.extend(script(T));
    let got = run(&["--json", "why"], s);
    let out = got
        .split("--- stdout\n")
        .nth(1)
        .unwrap()
        .split("--- stderr")
        .next()
        .unwrap();
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["context"], explain(T));
    assert_eq!(v["policy"], policy());
    assert_eq!(v["route"], route());
    assert_eq!(v["nodes"], calls());
    let requests = got
        .split("--- requests\n")
        .nth(1)
        .unwrap()
        .split("--- stdout")
        .next()
        .unwrap();
    let methods: Vec<&str> = requests
        .lines()
        .filter_map(|l| l.split(' ').next())
        .collect();
    assert_eq!(
        methods,
        [
            "session.list",
            "context.explain",
            "policy.explain",
            "ledger.tail",
            "node.list"
        ],
        "reads alone"
    );
    golden("why_json", &got);
}
