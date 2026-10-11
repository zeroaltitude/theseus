//! The converters over small invented fixtures of each format: text, a tool
//! call and its result, thinking, bookkeeping lines, an unknown type, a
//! malformed line, an empty file, and a secret built at run time from parts
//! (never a whole key-shaped literal in the repository).

use std::io::Cursor;

use serde_json::{json, Value};
use theseus_protocol::import::canonical;

use super::*;

/// A key-shaped value, built from parts.
fn planted() -> String {
    format!("{}_{}", "ghp", "Qx7aB9cD2eF4gH6iJ8kL0mN2oP4qR6sT8uV0")
}

fn run(source: Source, lines: &[Value]) -> Converted {
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    convert(source, Cursor::new(text), "sess-1", "sess-1.jsonl", "tag-1").unwrap()
}

fn episode(c: &Converted) -> Value {
    serde_json::from_str(c.episode.as_deref().expect("an episode")).unwrap()
}

fn claude_fixture() -> Vec<Value> {
    vec![
        json!({"type": "summary", "summary": "Old work", "leafUuid": "u0"}),
        json!({"type": "file-history-snapshot", "snapshot": {}}),
        json!({"type": "user", "timestamp": "2026-03-04T10:00:00.250Z", "cwd": "/work/reef-survey",
               "gitBranch": "tide/log", "sessionId": "s1",
               "message": {"role": "user", "content": "Where does the tide log live?"}}),
        json!({"type": "assistant", "timestamp": "2026-03-04T10:00:05.000Z", "cwd": "/work/reef-survey",
               "message": {"role": "assistant", "content": [
                   {"type": "thinking", "thinking": "hmm, the ledger"},
                   {"type": "text", "text": "Let me look."},
                   {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls boathouse"}}]}}),
        json!({"type": "user", "timestamp": "2026-03-04T10:00:06.000Z",
               "message": {"role": "user", "content": [
                   {"type": "tool_result", "tool_use_id": "t1", "content": "ledger.txt\nlog.txt"}]}}),
        json!({"type": "assistant", "timestamp": "2026-03-04T10:00:09.000Z", "isSidechain": true,
               "message": {"role": "assistant", "content": [{"type": "text", "text": "subagent chatter"}]}}),
        json!({"type": "user", "timestamp": "2026-03-04T10:00:10.000Z", "isMeta": true,
               "message": {"role": "user", "content": "Caveat: the messages below..."}}),
        json!({"type": "assistant", "timestamp": "2026-03-04T10:00:12.000Z",
               "message": {"role": "assistant", "content": [
                   {"type": "text", "text": format!("The tide log is in the boathouse ledger; token {}", planted())}]}}),
        json!({"type": "mystery-v9", "x": 1}),
    ]
}

/// Claude Code's lines: the person's text, the assistant's, a call and its
/// result, each at its own time; thinking, summary, snapshot, sidechain, meta
/// and an unknown type left out and counted.
#[test]
fn claude_code_lines_become_messages_and_the_rest_is_counted() {
    let c = run(Source::ClaudeCode, &claude_fixture());
    let e = episode(&c);
    let m = e["messages"].as_array().unwrap();
    let got: Vec<(&str, &str, &str)> = m
        .iter()
        .map(|x| {
            (
                x["author"].as_str().unwrap(),
                x["integrity"].as_str().unwrap(),
                x["text"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(got.len(), 5, "{got:?}");
    assert_eq!(
        got[0],
        ("user", "operator", "Where does the tide log live?")
    );
    assert_eq!(got[1], ("agent:claude-code", "agent", "Let me look."));
    assert_eq!(
        got[2],
        (
            "agent:claude-code",
            "agent",
            "[tool_use] Bash {\"command\":\"ls boathouse\"}"
        )
    );
    assert_eq!(
        got[3],
        ("tool", "outside", "[tool_result] ledger.txt\nlog.txt")
    );
    assert!(got[4]
        .2
        .starts_with("The tide log is in the boathouse ledger"));
    assert_eq!(m[0]["time"], "2026-03-04T10:00:00.250Z");
    assert_eq!(m[3]["time"], "2026-03-04T10:00:06.000Z");
    assert_eq!(e["as_of"]["start"], "2026-03-04T10:00:00.250Z");
    assert_eq!(e["as_of"]["end"], "2026-03-04T10:00:12.000Z");
    assert_eq!(
        e["place"],
        json!({"kind": "cli", "name": "/work/reef-survey", "id": "tide/log"})
    );
    assert_eq!(e["labels"]["topic"], json!(["projects/reef-survey"]));
    assert_eq!(m[0]["unit"], "sess-1.jsonl#L3");
    let k = &c.counts;
    assert_eq!((k.read, k.messages, k.tool_lines), (9, 5, 2), "{k:?}");
    assert_eq!(
        k.skipped,
        [
            ("file-history-snapshot", 1),
            ("meta", 1),
            ("mystery-v9", 1),
            ("sidechain", 1),
            ("summary", 1),
            ("thinking", 1)
        ]
        .map(|(a, n)| (a.to_string(), n))
        .into()
    );
    assert!(k.rejected.is_empty());
    // No thinking text anywhere.
    assert!(!c.episode.as_deref().unwrap().contains("the ledger\""));
    assert!(!c.episode.unwrap().contains("hmm"));
}

/// An episode passes the daemon's own check: its hash is the sha256 of its
/// canonical JSON without `hash`, as `episode::hash_of` writes it.
#[test]
fn an_episode_carries_the_hash_the_daemon_checks() {
    let c = run(Source::ClaudeCode, &claude_fixture());
    let e = episode(&c);
    let mut v = e.clone();
    let hash = v.as_object_mut().unwrap().remove("hash").unwrap();
    let mut out = String::new();
    canonical(&v, true, &mut out);
    assert_eq!(hash.as_str().unwrap(), sha256_hex(out.as_bytes()));
    assert_eq!(e["episode_id"], c.episode_id.as_deref().unwrap());
    assert!(e["episode_id"].as_str().unwrap().starts_with("ep_"));
    assert_eq!(e["episode_id"].as_str().unwrap().len(), 3 + 64);
}

/// A malformed line is rejected with its number and the file still reads; a
/// blank line is no line; a message line with no time is rejected too.
#[test]
fn a_malformed_line_is_rejected_with_its_number() {
    let text = format!(
        "{}\n\nnot json at all\n[1,2]\n{}\n{}\n",
        claude_fixture()[2],
        json!({"type": "user", "message": {"role": "user", "content": "no time"}}),
        claude_fixture()[7],
    );
    let c = convert(Source::ClaudeCode, Cursor::new(text), "s", "s.jsonl", "t").unwrap();
    let rej: Vec<u64> = c.counts.rejected.iter().map(|r| r.0).collect();
    assert_eq!(rej, vec![3, 4, 5], "{:?}", c.counts.rejected);
    assert!(c.counts.rejected[0].1.starts_with("not JSON"));
    assert_eq!(c.counts.messages, 2);
    // A line that is not UTF-8.
    let mut bytes = b"{\"type\":\"summary\"}\n".to_vec();
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    let c = convert(Source::ClaudeCode, Cursor::new(bytes), "s", "s.jsonl", "t").unwrap();
    assert_eq!(c.counts.rejected, vec![(2, "not UTF-8".to_string())]);
}

/// An empty file, and a file of bookkeeping alone, have no episode.
#[test]
fn an_empty_file_has_no_episode() {
    for text in ["", "\n\n", "{\"type\":\"summary\"}\n"] {
        let c = convert(Source::ClaudeCode, Cursor::new(text), "s", "s.jsonl", "t").unwrap();
        assert!(c.episode.is_none(), "{text:?}");
        assert_eq!(c.counts.messages, 0);
    }
}

/// A result is cut at its bound and says so; a long input is cut; the
/// count says how many were.
#[test]
fn a_long_result_is_cut_and_says_how_much_was_kept() {
    let big = "x".repeat(RESULT_MAX + 500);
    let lines = [
        json!({"type": "assistant", "timestamp": "2026-03-04T10:00:05Z",
               "message": {"role": "assistant", "content": [
                   {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file": "y".repeat(900)}}]}}),
        json!({"type": "user", "timestamp": "2026-03-04T10:00:06Z",
               "message": {"role": "user", "content": [
                   {"type": "tool_result", "tool_use_id": "t1", "is_error": true,
                    "content": [{"type": "text", "text": big}, {"type": "image", "source": {}}]}]}}),
    ];
    let c = run(Source::ClaudeCode, &lines);
    let m = episode(&c)["messages"].clone();
    let call = m[0]["text"].as_str().unwrap();
    assert!(call.contains("[cut: 400 of "), "{call}");
    let result = m[1]["text"].as_str().unwrap();
    assert!(result.starts_with("[tool_result error] xxx"));
    assert!(
        result.contains(&format!("[cut: {RESULT_MAX} of ")),
        "{}",
        &result[result.len() - 60..]
    );
    assert_eq!(c.counts.cut, 2);
}

fn codex_fixture() -> Vec<Value> {
    vec![
        json!({"timestamp": "2026-04-01T09:00:00.000Z", "type": "session_meta",
               "payload": {"id": "abc", "cwd": "/work/kelp-model", "git": {"branch": "main", "commit_hash": "0"}}}),
        json!({"timestamp": "2026-04-01T09:00:01.000Z", "type": "response_item",
               "payload": {"type": "message", "role": "user",
                           "content": [{"type": "input_text", "text": "<environment_context>cwd...</environment_context>"}]}}),
        json!({"timestamp": "2026-04-01T09:00:02.000Z", "type": "response_item",
               "payload": {"type": "message", "role": "user",
                           "content": [{"type": "input_text", "text": "Fit the kelp growth curve."}]}}),
        json!({"timestamp": "2026-04-01T09:00:03.000Z", "type": "response_item",
               "payload": {"type": "reasoning", "summary": [], "encrypted_content": "zz"}}),
        json!({"timestamp": "2026-04-01T09:00:04.000Z", "type": "response_item",
               "payload": {"type": "function_call", "name": "shell", "call_id": "c1",
                           "arguments": "{\"command\":[\"python\",\"fit.py\"]}"}}),
        json!({"timestamp": "2026-04-01T09:00:05.000Z", "type": "response_item",
               "payload": {"type": "function_call_output", "call_id": "c1",
                           "output": "{\"output\":\"r2=0.93\",\"metadata\":{\"exit_code\":0}}"}}),
        json!({"timestamp": "2026-04-01T09:00:06.000Z", "type": "event_msg",
               "payload": {"type": "agent_message", "message": "duplicate"}}),
        json!({"timestamp": "2026-04-01T09:00:07.000Z", "type": "turn_context", "payload": {}}),
        json!({"timestamp": "2026-04-01T09:00:08.000Z", "type": "response_item",
               "payload": {"type": "message", "role": "assistant",
                           "content": [{"type": "output_text", "text": format!("Fitted: r2 0.93 (key {})", planted())}]}}),
        json!({"timestamp": "2026-04-01T09:00:09.000Z", "type": "response_item",
               "payload": {"type": "future_item_v3", "x": 1}}),
    ]
}

/// Codex's rollout: the meta's directory and branch, the person's words
/// (not the injected context), a call and its output (the output's own JSON
/// unwrapped), the assistant's words; reasoning, event and turn lines and an
/// unknown item left out and counted.
#[test]
fn codex_rollout_lines_become_messages_and_the_rest_is_counted() {
    let c = run(Source::Codex, &codex_fixture());
    let e = episode(&c);
    let m = e["messages"].as_array().unwrap();
    let got: Vec<(&str, &str, &str)> = m
        .iter()
        .map(|x| {
            (
                x["author"].as_str().unwrap(),
                x["integrity"].as_str().unwrap(),
                x["text"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(got.len(), 4, "{got:?}");
    assert_eq!(got[0], ("user", "operator", "Fit the kelp growth curve."));
    assert_eq!(
        got[1],
        (
            "agent:codex",
            "agent",
            "[tool_use] shell {\"command\":[\"python\",\"fit.py\"]}"
        )
    );
    assert_eq!(got[2], ("tool", "outside", "[tool_result] r2=0.93"));
    assert!(got[3].2.starts_with("Fitted: r2 0.93"));
    assert_eq!(m[0]["time"], "2026-04-01T09:00:02.000Z");
    assert_eq!(e["source"], "codex");
    assert_eq!(
        e["place"],
        json!({"kind": "cli", "name": "/work/kelp-model", "id": "main"})
    );
    let k = &c.counts;
    assert_eq!((k.messages, k.tool_lines), (4, 2));
    assert_eq!(
        k.skipped,
        [
            ("event_msg", 1),
            ("future_item_v3", 1),
            ("injected_context", 1),
            ("thinking", 1),
            ("turn_context", 1)
        ]
        .into_iter()
        .map(|(a, n)| (a.to_string(), n))
        .collect()
    );
}

/// An older rollout has its items bare on the line, with no envelope, and a
/// first line that is the session's own; a line with no time of its own is
/// read at the last time the file named.
#[test]
fn an_older_codex_rollout_reads_with_its_items_bare() {
    let lines = [
        json!({"id": "abc", "timestamp": "2026-01-02T03:04:05.000Z", "instructions": "x"}),
        json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello kelp"}]}),
        json!({"type": "function_call", "name": "shell", "arguments": "{}", "call_id": "c"}),
    ];
    let c = run(Source::Codex, &lines);
    let e = episode(&c);
    assert_eq!(e["messages"].as_array().unwrap().len(), 2);
    assert_eq!(e["messages"][0]["time"], "2026-01-02T03:04:05.000Z");
}

/// The same file is the same episode in any tag, and its hash follows what
/// the file holds: one more line is another hash under the same id.
#[test]
fn the_same_file_is_the_same_episode_and_a_grown_one_has_another_hash() {
    let a = run(Source::ClaudeCode, &claude_fixture());
    let b = run(Source::ClaudeCode, &claude_fixture());
    assert_eq!(a, b, "deterministic");
    let mut grown = claude_fixture();
    grown.push(json!({"type": "user", "timestamp": "2026-03-04T11:00:00Z",
                      "message": {"role": "user", "content": "And the ledger's key?"}}));
    let g = run(Source::ClaudeCode, &grown);
    assert_eq!(g.episode_id, a.episode_id);
    assert_ne!(episode(&g)["hash"], episode(&a)["hash"]);
    // The id is the file's, whatever the tag; the source is part of it.
    let other = convert(
        Source::ClaudeCode,
        Cursor::new(format!("{}\n", claude_fixture()[2])),
        "sess-1",
        "f",
        "tag-2",
    )
    .unwrap();
    assert_eq!(other.episode_id, a.episode_id);
    let cx = convert(
        Source::Codex,
        Cursor::new(format!("{}\n", codex_fixture()[2])),
        "sess-1",
        "f",
        "tag-1",
    )
    .unwrap();
    assert_ne!(cx.episode_id, a.episode_id);
}

/// The text is as the file has it: the CLI scrubs nothing (the daemon does).
#[test]
fn the_cli_leaves_scrubbing_to_the_daemon() {
    let c = run(Source::ClaudeCode, &claude_fixture());
    assert!(c.episode.unwrap().contains(&planted()));
}

/// Files under a directory: sorted, only `.jsonl`, and a subagent's file left
/// out and counted (Claude Code's `subagents` directory).
#[test]
fn files_under_a_directory_leave_out_subagent_files() {
    let d = tempfile::tempdir().unwrap();
    let w = |p: &str| {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, "{}\n").unwrap();
    };
    w("proj-a/b.jsonl");
    w("proj-a/a.jsonl");
    w("proj-a/a/subagents/agent-1.jsonl");
    w("proj-b/c.jsonl");
    w("proj-b/notes.txt");
    let (files, side) = run::files_under(Source::ClaudeCode, d.path()).unwrap();
    let names: Vec<String> = files
        .iter()
        .map(|f| f.strip_prefix(d.path()).unwrap().display().to_string())
        .collect();
    assert_eq!(
        names,
        ["proj-a/a.jsonl", "proj-a/b.jsonl", "proj-b/c.jsonl"]
    );
    assert_eq!(side, 1);
    let (files, side) = run::files_under(Source::Codex, d.path()).unwrap();
    assert_eq!(
        (files.len(), side),
        (4, 0),
        "Codex has no subagent directory"
    );
}

/// A file's line of the report: its counts, what was skipped by type, and
/// each rejected line by its number.
#[test]
fn a_files_line_names_its_counts_and_its_rejected_lines() {
    let text = format!(
        "{}\nnot json\n{}\n",
        claude_fixture()[0],
        claude_fixture()[2]
    );
    let c = convert(Source::ClaudeCode, Cursor::new(text), "s", "s.jsonl", "t").unwrap();
    let r = run::FileReport {
        file: "proj/s.jsonl".into(),
        empty: false,
        counts: c.counts,
    };
    assert_eq!(
        run::file_line(&r),
        "proj/s.jsonl: read 3 lines, 1 messages (0 tool lines), skipped 1 (summary 1), rejected 1\n  line 2: not JSON: expected ident at line 1 column 2\n"
    );
}

/// A second run, on any day, lands in the live tag that holds the source
/// (so an unchanged file is skipped, not rejected for the tag its hash
/// names); a tag with every session erased is not reused.
#[test]
fn the_default_tag_is_the_sources_live_tag_else_todays() {
    use theseus_protocol::import::{ImportListResult, ImportTagInfo};
    let tag = |t: &str, source: &str, sessions, erased, updated| ImportTagInfo {
        tag: t.into(),
        sessions,
        nodes: 1,
        erased,
        sources: [(source.to_string(), sessions)].into(),
        first_ms: None,
        last_ms: None,
        updated_ms: updated,
    };
    let none = ImportListResult::default();
    assert_eq!(
        run::default_tag(&none, Source::ClaudeCode, "2026-10-11"),
        "claude-code-2026-10-11"
    );
    let listed = ImportListResult {
        tags: vec![
            tag("claude-code-2026-09-01", "claude-code", 4, 0, 10),
            tag("claude-code-2026-09-02", "claude-code", 4, 0, 20),
            tag("claude-code-2026-09-03", "claude-code", 4, 4, 30),
            tag("openclaw-2026", "openclaw-store", 9, 0, 40),
        ],
    };
    assert_eq!(
        run::default_tag(&listed, Source::ClaudeCode, "2026-10-11"),
        "claude-code-2026-09-02"
    );
    assert_eq!(
        run::default_tag(&listed, Source::Codex, "2026-10-11"),
        "codex-2026-10-11"
    );
}

/// The import is the owner's act: inside a job it is refused, saying why and
/// that the operator runs it from their own shell, before any file is read.
#[test]
fn the_import_is_refused_inside_a_job() {
    let e = run::operator_only(Some("ses_0000aa1b2c3"))
        .unwrap_err()
        .to_string();
    assert!(e.starts_with("theseus import refused:"), "{e}");
    assert!(e.ends_with("Run it from your own shell."), "{e}");
    assert!(run::operator_only(None).is_ok());
}
