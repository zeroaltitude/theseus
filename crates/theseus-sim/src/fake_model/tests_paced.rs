//! The paced stand-in (theseus-7gir.13): its first byte and its chunks on
//! time, a connection kept alive across requests, its log, and a stepped
//! rule's turn.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

use serde_json::{json, Value};

use super::serve::{cut, group, split_deltas};
use super::*;

/// One request on `s`, and each chunk of its answer with when it arrived
/// (`CLOCK_MONOTONIC` ns), read off the chunked body.
fn ask(s: &mut TcpStream, body: &Value) -> (u64, Vec<(u64, String)>) {
    let body = body.to_string();
    let sent = mono_ns();
    write!(
        s,
        "POST /v1/messages?beta=true HTTP/1.1\r\nhost: x\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        r.read_line(&mut line).unwrap();
        if line.trim().is_empty() {
            break;
        }
    }
    let mut chunks = Vec::new();
    loop {
        line.clear();
        r.read_line(&mut line).unwrap();
        let at = mono_ns();
        let n = usize::from_str_radix(line.trim(), 16).unwrap();
        if n == 0 {
            r.read_line(&mut line).unwrap();
            return (sent, chunks);
        }
        let mut b = vec![0; n + 2];
        r.read_exact(&mut b).unwrap();
        chunks.push((at, String::from_utf8_lossy(&b[..n]).into_owned()));
    }
}

fn text(chunks: &[(u64, String)]) -> String {
    chunks
        .iter()
        .flat_map(|(_, c)| c.lines())
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|d| serde_json::from_str::<Value>(d).ok())
        .filter_map(|v| v["delta"]["text"].as_str().map(str::to_string))
        .collect()
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

/// The first byte comes the rule's time to first byte after the request,
/// then each chunk its spacing after the one before, within a few ms; two
/// requests on one connection are answered on it (kept alive), and the log
/// says so, with the times it measured.
#[test]
fn a_paced_answer_keeps_its_first_byte_and_spacing_on_a_kept_connection() {
    let rules: Vec<Rule> = serde_json::from_value(json!([
        {"when": "pace me", "steps": [{"text": "ok-{marker} the stand-in streams its answer in eight even chunks end-{marker}"}],
         "ttfb_ms": 120, "chunks": 8, "chunk_ms": 30}
    ]))
    .unwrap();
    let fake = FakeModel::start_rules_with("127.0.0.1:0", rules, Serving::default()).unwrap();
    let mut s = TcpStream::connect(fake.addr).unwrap();
    s.set_nodelay(true).unwrap();
    let req = json!({"model": "m", "stream": true, "tools": [{"name": "Read"}],
        "messages": [{"role": "user", "content": "pace me marker=K9"}]});
    for round in 0..2 {
        let (sent, chunks) = ask(&mut s, &req);
        assert_eq!(chunks.len(), 8, "eight writes, one chunk each");
        let ttfb = ms(chunks[0].0 - sent);
        assert!(
            (115.0..170.0).contains(&ttfb),
            "the first byte at {ttfb:.1} ms, not 120"
        );
        // Each chunk on the stand-in's schedule: never before its time after
        // the first byte, and within a few ms of it (a busy reader is late,
        // never early, so the spacing is judged against the schedule).
        let first = fake.entries_at_least(round + 1)[round].first_byte_ns;
        for (k, (at, _)) in chunks.iter().enumerate() {
            let due = first + k as u64 * 30_000_000;
            let late = *at as f64 / 1e6 - due as f64 / 1e6;
            assert!(
                (0.0..40.0).contains(&late),
                "chunk {k} {late:.1} ms after its time, 30 ms apart"
            );
        }
        let whole = text(&chunks);
        assert!(whole.starts_with("ok-K9 "), "{whole}");
        assert!(whole.ends_with("end-K9"), "{whole}");
        assert!(
            text(&chunks[..1]).starts_with("ok-K9"),
            "the marker is in the first chunk"
        );
        let log = fake.entries_at_least(round + 1);
        let e = &log[round];
        assert_eq!(
            (e.conn, e.conn_req),
            (0, round as u64),
            "one connection, reused"
        );
        assert_eq!((e.chunks, e.step, e.side), (8, 0, false));
        assert_eq!(e.tools, vec!["Read".to_string()]);
        assert_eq!(e.rule.as_deref(), Some("pace me"));
        assert!(e.first_byte_ns - e.arrival_ns >= 120_000_000);
        assert!(e.last_byte_ns - e.first_byte_ns >= 7 * 30_000_000);
        assert!(e.req_bytes > 0 && e.resp_bytes > 0);
    }
}

/// A stepped rule scripts a whole turn: its calls at step 0 and 1, by the
/// tool results since the opening text, then its text; a request with no
/// tools is a side request; one that asks for no stream gets the message
/// whole; and the log is written as JSON lines.
#[test]
fn a_stepped_rule_scripts_a_turn_and_the_log_says_each_step() {
    let rules: Vec<Rule> = serde_json::from_value(json!([
        {"when": "task three", "steps": [
            {"calls": [{"name": "Read", "input": {"file_path": "notes-{marker}.txt"}}]},
            {"calls": [{"name": "Bash", "input": {"command": "wc -l notes.txt"}}]},
            {"text": "done-{marker}"}
        ]}
    ]))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.jsonl");
    let serving = Serving {
        log: Log::to_file(&path).unwrap(),
        ..Serving::default()
    };
    let fake = FakeModel::start_rules_with("127.0.0.1:0", rules, serving).unwrap();
    let mut s = TcpStream::connect(fake.addr).unwrap();
    let tools = json!([{"name": "Read"}, {"name": "Bash"}]);
    let mut msgs = vec![
        json!({"role": "user", "content": [{"type": "text", "text": "task three marker=Z1"}]}),
    ];
    let mut seen = Vec::new();
    for k in 0..3 {
        let (_, chunks) = ask(
            &mut s,
            &json!({"stream": true, "tools": tools, "messages": msgs}),
        );
        let body: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        seen.push(body.clone());
        msgs.push(json!({"role": "assistant", "content": [{"type": "tool_use", "id": format!("t{k}"), "name": "x", "input": {}}]}));
        msgs.push(json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": format!("t{k}"), "content": "ok"},
                                                     {"type": "text", "text": "<system-reminder>a reminder</system-reminder>"}]}));
    }
    assert!(
        seen[0].contains("notes-Z1.txt") && seen[0].contains("\"Read\""),
        "{}",
        seen[0]
    );
    assert!(
        seen[1].contains("\"Bash\"") && seen[1].contains("wc -l"),
        "{}",
        seen[1]
    );
    assert!(seen[2].contains("done-Z1"), "{}", seen[2]);
    let (_, side) = ask(
        &mut s,
        &json!({"stream": true, "messages": [{"role": "user", "content": "a title"}]}),
    );
    assert!(text(&side).contains(SIDE_TEXT));
    // No stream asked: one JSON message, its content whole.
    let body =
        json!({"stream": false, "tools": tools, "messages": [{"role": "user", "content": "task three marker=Y2"}]})
            .to_string();
    write!(
        s,
        "POST /v1/messages HTTP/1.1\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let msg: Value = serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(msg["content"][0]["input"]["file_path"], "notes-Y2.txt");
    assert_eq!(msg["stop_reason"], "tool_use");
    let lines: Vec<Entry> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let steps: Vec<(usize, bool, bool)> =
        lines.iter().map(|e| (e.step, e.side, e.stream)).collect();
    assert_eq!(
        steps,
        [
            (0, false, true),
            (1, false, true),
            (2, false, true),
            (0, true, true),
            (0, false, false)
        ]
    );
    assert!(lines.iter().all(|e| e.path.starts_with("/v1/messages")));
    assert_eq!(lines[3].opening, "a title");
    assert_eq!(lines[0].marker, "Z1");
}

/// The cuts: a text's first piece holds its first word, the pieces join to
/// the text, and the writes are one delta each.
#[test]
fn deltas_are_cut_into_the_chunks_asked_for() {
    let pieces = cut("ok-M1 a b c d e f g h i j", 8, true);
    assert_eq!(pieces.len(), 8);
    assert!(pieces[0].starts_with("ok-M1"));
    assert_eq!(pieces.concat(), "ok-M1 a b c d e f g h i j");
    assert_eq!(
        cut("ab", 8, true),
        vec!["ab".to_string()],
        "a word is never cut"
    );
    assert_eq!(cut("abc", 8, false).len(), 3);
    let calls = calls_turn(&[
        Call {
            name: "a".into(),
            input: json!({"x": "0123456789"}),
        },
        Call {
            name: "b".into(),
            input: json!({"y": 1}),
        },
    ]);
    let split = split_deltas(&calls, 8);
    let deltas = split
        .iter()
        .filter(|e| e["type"] == "content_block_delta")
        .count();
    assert_eq!(deltas, 8);
    let writes = group(&split);
    assert_eq!(writes.len(), 8);
    assert_eq!(writes[0][0]["type"], "message_start");
    assert_eq!(writes[7].last().unwrap()["type"], "message_stop");
    let joined: String = split
        .iter()
        .filter(|e| e["index"] == 0 && e["type"] == "content_block_delta")
        .map(|e| e["delta"]["partial_json"].as_str().unwrap())
        .collect();
    assert_eq!(
        serde_json::from_str::<Value>(&joined).unwrap()["x"],
        "0123456789"
    );
}
