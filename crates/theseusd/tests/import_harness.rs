//! `theseus import claude-code` and `theseus import codex` (theseus-o73y),
//! the real CLI against a real daemon, on invented session files made at run
//! time: each file an imported session, the counts and the rejected lines on
//! stdout, a second run writing nothing, a file that grew reported and not
//! imported again, subagent files and empty files counted, the planted secret
//! (built from parts) stored as the session said it, and the tag's erase.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::Served;
use serde_json::{json, Value};

/// A key-shaped value, built from parts.
fn token() -> String {
    format!("{}_{}", "ghp", "Wk3aB9cD2eF4gH6iJ8kL0mN2oP4qR6sT8uV0")
}

fn cli(s: &Served, args: &[&str]) -> (i32, String, String) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(s.path("sock"))
        .args(args)
        .env_remove("THESEUS_SESSION")
        .env_remove("THESEUS_SOCKET")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write(path: &Path, lines: &[Value]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(path, text).unwrap();
}

fn claude_session(first: &str) -> Vec<Value> {
    vec![
        json!({"type": "summary", "summary": "x", "leafUuid": "u"}),
        json!({"type": "user", "timestamp": "2026-03-04T10:00:00.000Z", "cwd": "/work/reef-survey",
               "gitBranch": "tide/log", "message": {"role": "user", "content": first}}),
        json!({"type": "assistant", "timestamp": "2026-03-04T10:00:05.000Z",
               "message": {"role": "assistant", "content": [
                   {"type": "thinking", "thinking": "private musings"},
                   {"type": "text", "text": format!("The tide log is in the boathouse ledger; the key is {}", token())},
                   {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}}),
        json!({"type": "user", "timestamp": "2026-03-04T10:00:06.000Z",
               "message": {"role": "user", "content": [
                   {"type": "tool_result", "tool_use_id": "t1", "content": "ledger.txt"}]}}),
    ]
}

fn fixtures(root: &Path) -> (PathBuf, PathBuf) {
    let claude = root.join("claude/projects");
    write(
        &claude.join("-work-reef/aaaa.jsonl"),
        &claude_session("Where does the tide log live?"),
    );
    // A second session: a malformed line and an unknown type among good ones.
    let mut b = claude_session("What is the kelp count?");
    b.insert(2, json!({"type": "future-v9"}));
    let text: String = b
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 1 {
                "not json\n".to_string()
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    std::fs::write(claude.join("-work-reef/bbbb.jsonl"), text).unwrap();
    std::fs::write(claude.join("-work-reef/empty.jsonl"), "").unwrap();
    write(
        &claude.join("-work-reef/aaaa/subagents/agent-1.jsonl"),
        &claude_session("subagent"),
    );
    let codex = root.join("codex/sessions");
    write(
        &codex.join("2026/04/01/rollout-2026-04-01T09-00-00-abc.jsonl"),
        &[
            json!({"timestamp": "2026-04-01T09:00:00.000Z", "type": "session_meta",
                   "payload": {"id": "abc", "cwd": "/work/kelp-model", "git": {"branch": "main"}}}),
            json!({"timestamp": "2026-04-01T09:00:02.000Z", "type": "response_item",
                   "payload": {"type": "message", "role": "user",
                               "content": [{"type": "input_text", "text": "Fit the kelp growth curve."}]}}),
            json!({"timestamp": "2026-04-01T09:00:04.000Z", "type": "response_item",
                   "payload": {"type": "function_call", "name": "shell", "call_id": "c1", "arguments": "{}"}}),
            json!({"timestamp": "2026-04-01T09:00:08.000Z", "type": "response_item",
                   "payload": {"type": "message", "role": "assistant",
                               "content": [{"type": "output_text", "text": format!("Fitted; key {}", token())}]}}),
        ],
    );
    (claude, codex)
}

/// Everything under `dir` as bytes holds `needle`.
fn holds(dir: &Path, needle: &str) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(b) = std::fs::read(&p) {
                if b.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn both_imports_run_idempotent_raw_and_erasable() {
    let s = Served::start(|_| {}, |_| {});
    let (claude, codex) = fixtures(s.dir.path());
    let tag = claude_part(&s, &claude);
    codex_and_erase(&s, &codex, &tag);
}

/// Claude Code's import, its second run and its grown file; the tag it made.
fn claude_part(s: &Served, claude: &Path) -> String {
    let (s, claude_dir) = (s, claude.to_str().unwrap());

    // Claude Code: two sessions, one empty file, one subagent file left out.
    let (code, out, err) = cli(s, &["import", "claude-code", claude_dir]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("2 imported, 0 skipped"), "{out}");
    assert!(
        out.contains("bbbb.jsonl: read 5 lines, 3 messages (2 tool lines)"),
        "{out}"
    );
    assert!(out.contains("  line 2: not JSON"), "{out}");
    assert!(out.contains("future-v9 1"), "{out}");
    assert!(out.contains("thinking 1"), "{out}");
    assert!(out.contains("1 files with nothing to import"), "{out}");
    assert!(out.contains("1 subagent files left out"), "{out}");
    let (_, listed, _) = cli(s, &["import", "list"]);
    assert!(listed.starts_with("claude-code-"), "{listed}");
    assert!(listed.contains("2 sessions, 7 nodes"), "{listed}");
    let tag = listed.split(':').next().unwrap().to_string();

    // The record is raw: the key is in the daemon's state as it came, with its text.
    assert!(
        holds(&s.path("state"), "boathouse ledger"),
        "the grep reads the state"
    );
    assert!(
        holds(&s.path("state"), &token()),
        "the store holds the key as the session said it"
    );
    assert!(
        !holds(&s.path("state"), "private musings"),
        "thinking was imported"
    );

    // A second run writes nothing new, on whatever tag (it lands in the first's).
    let (code, out, _) = cli(s, &["import", "claude-code", claude_dir]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("into tag {tag}: 3 files, 0 imported, 2 skipped")),
        "{out}"
    );

    // A file that grew: its new message is appended, nothing imported twice.
    let mut grown = claude_session("Where does the tide log live?");
    grown.push(json!({"type": "user", "timestamp": "2026-03-04T11:00:00Z",
                      "message": {"role": "user", "content": "And its key?"}}));
    write(&claude.join("-work-reef/aaaa.jsonl"), &grown);
    let (_, out, _) = cli(s, &["import", "claude-code", claude_dir]);
    assert!(out.contains("0 imported, 1 appended, 1 skipped"), "{out}");
    assert!(!out.contains("files changed since"), "{out}");

    tag
}

/// Codex's import, then the erase of Claude Code's tag.
fn codex_and_erase(s: &Served, codex: &Path, tag: &str) {
    let (code, out, err) = cli(s, &["import", "codex", codex.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("1 imported"), "{out}");
    assert!(out.contains("3 messages (1 tool lines)"), "{out}");
    let (_, listed, _) = cli(s, &["import", "list"]);
    assert!(
        listed.contains("codex-") && listed.contains("codex 1"),
        "{listed}"
    );
    assert!(
        holds(&s.path("state"), &token()),
        "the key stays as the session said it"
    );

    // Erase by tag: the claude-code tag's two sessions.
    let (code, out, err) = cli(s, &["import", "erase", "--tag", tag]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains(&format!("erased {tag}: 2 sessions")), "{out}");
    let (_, listed, _) = cli(s, &["import", "list"]);
    assert!(
        listed.contains(&format!("{tag}: 2 sessions")) && listed.contains("2 erased"),
        "{listed}"
    );
}

/// Inside a job the import is refused before a file is read.
#[test]
fn the_import_is_refused_inside_a_job() {
    let s = Served::start(|_| {}, |_| {});
    let (claude, _) = fixtures(s.dir.path());
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(s.path("sock"))
        .args(["import", "claude-code", claude.to_str().unwrap()])
        .env("THESEUS_SESSION", "ses_0000aa1b2c3")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("theseus import refused"), "{err}");
    assert!(err.contains("Run it from your own shell"), "{err}");
    assert!(!holds(&s.path("state"), "boathouse ledger"));
}
