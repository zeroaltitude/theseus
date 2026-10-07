//! Golden output for every CLI subcommand and its renderers (theseus-0g4,
//! finding 11): the status line, the span tree, `print_node`, `Printer`, and
//! each command's text and `--json` forms. Each scenario runs the real binary
//! against a scripted daemon on a Unix socket and compares its stdout, its
//! stderr, and its exit code with `tests/golden/<name>.txt`.
//!
//! The goldens were written by the CLI before `run()` was split and its
//! renderers moved to `render.rs`, so the split keeps every byte. A change
//! that means to change the output rewrites them:
//! `THESEUS_GOLDEN=write cargo nextest run --workspace -E 'package(theseus)'`.
//!
//! Time-dependent lines (a task's age, a wake's due time) are left to the
//! unit tests, which pass `now` in.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

const S: &str = "ses_q7f3k2";
const T: &str = "turn_m4p8z1";
const X: &str = "exe_q7f3k2";

/// One request the scripted daemon expects: the notifications it sends before
/// answering, its answer, and the notifications after it.
struct Step {
    method: &'static str,
    before: Vec<Value>,
    answer: Result<Value, Value>,
    after: Vec<Value>,
}

fn step(method: &'static str, result: Value) -> Step {
    Step {
        method,
        before: vec![],
        answer: Ok(result),
        after: vec![],
    }
}

fn note(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// Run `theseus --socket <sock> ARGS` against `steps`, in order. The daemon
/// closes the connection after the last step, as a stopped daemon would.
fn run(args: &[&str], steps: Vec<Step>) -> String {
    run_in(args, steps, None)
}

/// `run`, inside the Theseus job `job` names when it names one: its marker,
/// `THESEUS_SESSION`, in the CLI's environment (theseus-zmgb).
fn run_in(args: &[&str], steps: Vec<Step>, job: Option<&str>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut seen = Vec::new();
        for st in steps {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let req: Value = serde_json::from_str(&line).unwrap();
            seen.push(req["method"].as_str().unwrap_or("?").to_string());
            let mut out = String::new();
            if req["method"] != st.method {
                let e = json!({"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32601,
                    "message": format!("the script expected {}", st.method)}});
                (&s).write_all(format!("{e}\n").as_bytes()).unwrap();
                break;
            }
            for n in &st.before {
                out.push_str(&format!("{n}\n"));
            }
            let answer = match &st.answer {
                Ok(r) => json!({"jsonrpc": "2.0", "id": req["id"], "result": r}),
                Err(e) => json!({"jsonrpc": "2.0", "id": req["id"], "error": e}),
            };
            out.push_str(&format!("{answer}\n"));
            for n in &st.after {
                out.push_str(&format!("{n}\n"));
            }
            (&s).write_all(out.as_bytes()).unwrap();
        }
        seen
    });
    // Hermetic: a gate run inside a herdr pane would turn the watch's
    // reporter on (theseus-l1l), and one inside a Theseus job would carry its
    // marker (theseus-zmgb).
    let mut cmd = Command::new(THESEUS);
    cmd.arg("--socket")
        .arg(&sock)
        .args(args)
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH");
    if let Some(job) = job {
        cmd.env("THESEUS_SESSION", job);
    }
    let out = cmd.output().unwrap();
    let seen = daemon.join().unwrap();
    format!(
        "$ theseus {}\n--- requests\n{}\n--- stdout\n{}--- stderr\n{}--- exit {}\n",
        args.join(" "),
        seen.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
        out.status.code().unwrap_or(-1)
    )
}

/// Compare with the golden, or write it under `THESEUS_GOLDEN=write`.
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
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (write it with THESEUS_GOLDEN=write)",
            path.display()
        )
    });
    if got != want {
        let first = got
            .lines()
            .zip(want.lines())
            .position(|(g, w)| g != w)
            .unwrap_or(got.lines().count().min(want.lines().count()));
        panic!(
            "{name}: the output differs from {} at line {}\n--- got\n{got}\n--- want\n{want}",
            path.display(),
            first + 1
        );
    }
}

// ---------------------------------------------------------------- fixtures

fn turn_result(output: &str) -> Value {
    json!({
        "session_id": S, "turn_id": T, "loops": 2, "output": output,
        "stop_reason": "end_turn", "provider_stop_reason": "end_turn",
        "model": "glm-x", "provider": "zed", "profile": "quick",
        "usage": {"input_tokens": 5230, "output_tokens": 212,
                  "cache_read_input_tokens": 4096, "cache_creation_input_tokens": 0},
        "elapsed_ms": 3021, "first_token_ms": 640, "request_id": "req_a1",
        "trace": null, "execution_id": X, "cost_usd": 0.0031, "tool_calls": 3,
        "awaiting_confirm": null, "stop_details": null, "continuation": false
    })
}

fn external() -> Value {
    json!({"since_ms": 1_759_300_000_000u64, "tool": "http.fetch",
           "url": "notes.example/today", "node_id": "nod_e1"})
}

fn confirm_tool() -> Value {
    json!({"correlation_id": "act_k4", "session_id": S, "execution_id": X,
           "tool": "fs.write", "input": {"path": "/srv/motd", "text": "hello"},
           "resource": "/srv/motd", "reason": "write outside the roots (/w)",
           "by": "operator", "requested_at_ms": 1_759_300_100_000u64,
           "expires_at_ms": 1_759_301_000_000u64, "floor": true,
           "external_text": external()})
}

fn confirm_budget() -> Value {
    json!({"correlation_id": "act_b5", "session_id": S, "execution_id": X,
           "tool": "budget.reset", "input": {},
           "reason": "The session reached its $1.00 limit; the next call needs $0.02",
           "by": "operator", "requested_at_ms": 1_759_300_200_000u64, "expires_at_ms": 0,
           "budget": {"spent_usd": 1.0, "limit_usd": 1.0, "needed_usd": 0.02,
                      "lifetime_usd": 3.5}})
}

/// A question with no floor and no external text, which `confirm_tool` has.
fn confirm_plain() -> Value {
    json!({"correlation_id": "act_m3", "session_id": S, "execution_id": X,
           "tool": "proc.run", "input": {"argv": ["tide", "--port", "lantern"]},
           "reason": "proc.run — approve (enforcement = approve)",
           "by": "operator", "requested_at_ms": 1_759_300_600_000u64,
           "expires_at_ms": 1_759_301_500_000u64})
}

fn tighten_result(changed: bool, already: bool) -> Value {
    json!({"tool": "proc.run", "by": "the CLI",
           "tightening": {"tool": "proc.run", "posture": "approve", "by": "the CLI",
                          "who": "sock#3", "via": "cli", "at_ms": 1_759_300_300_250u64},
           "posture": "approve", "setting": "tightened by the CLI",
           "config_posture": "notify", "config_setting": "enforcement = notify",
           "changed": changed, "already": already})
}

/// One of every notification a turn can carry, in the order a turn sends
/// them, with the payloads the daemon builds today.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn turn_notes() -> Vec<Value> {
    vec![
        note(
            "turn.started",
            json!({"session_id": S, "turn_id": T, "execution_id": X,
            "continuation": false}),
        ),
        note(
            "context.compiled",
            json!({"session_id": S, "turn_id": T, "loop": 0,
            "decision": "recompile", "trigger": "window", "compilation_id": "cmp_b2",
            "strategy": "fresh", "prefix_nodes": 2, "tail_nodes": 6, "messages": 7,
            "est_tokens": 4210, "digest": "9f2c1a0b7d3e4f51", "repairs": ["tu_lost1"],
            "tools": 14, "nodes_scanned": 11,
            "cache": {"breakpoints": [], "ttl": "5m", "conversation_ttl": "5m"}}),
        ),
        note(
            "loop.started",
            json!({"turn_id": T, "loop_index": 0, "model": "glm-x",
            "tools_offered": 14}),
        ),
        note(
            "model.thinking",
            json!({"turn_id": T, "loop_index": 0,
            "text": "The folder has\ntwo files."}),
        ),
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 0,
            "text": "Looking at the folder."}),
        ),
        note(
            "tool.proposed",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_1",
            "tool": "proc.run", "input": {"argv": ["make", "notes"]},
            "gate": {"result": {"gate": "allow"}, "validated": true,
                     "decision": {"posture": "notify", "reason": "proc.run — notify",
                                  "notify": {"kind": "notify", "setting": "enforcement = notify",
                                             "rule": "proc.run — notify (enforcement = notify)"},
                                  "granted": "gh got GH_TOKEN"},
                     "plan": {"summary": "run make notes"}, "proposal": {"tool": "proc.run"}}}),
        ),
        note(
            "policy.notified",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_1",
            "correlation_id": "act_k2", "tool": "proc.run", "input": {"argv": ["make", "notes"]},
            "summary": "run make notes", "kind": "notify", "setting": "enforcement = notify",
            "rule": "proc.run — notify (enforcement = notify)", "granted": "gh got GH_TOKEN"}),
        ),
        note(
            "tool.started",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_1",
            "tool": "proc.run", "correlation_id": "act_k2", "backend": "job", "pid": 4242,
            "argv": ["make", "notes"], "cwd": "/w/notes", "granted": "gh got GH_TOKEN",
            "withheld": []}),
        ),
        note(
            "tool.ended",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_1",
            "tool": "proc.run", "status": "ok", "duration_ms": 1520, "correlation_id": "act_k2",
            "late": false, "truncated": true, "bytes": 70000, "node_id": "nod_r2",
            "exit_code": 0, "stopped_by": null, "preview": "built"}),
        ),
        note(
            "node.written",
            json!({"session_id": S, "node_id": "nod_r2", "kind": "tool_result"}),
        ),
        note(
            "tool.started",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_3",
            "tool": "fs.read", "correlation_id": "act_k3", "backend": "inproc"}),
        ),
        note(
            "tool.ended",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_3",
            "tool": "fs.read", "status": "cancelled", "duration_ms": 12,
            "correlation_id": "act_k3", "late": true, "truncated": false, "bytes": 0,
            "node_id": "nod_r3", "exit_code": null, "stopped_by": "the CLI", "preview": ""}),
        ),
        note(
            "tool.ended",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_5",
            "tool": "fs.write", "status": "declined", "duration_ms": 0,
            "correlation_id": "act_k5", "late": false, "truncated": false, "bytes": 31,
            "node_id": "nod_r5", "exit_code": null, "stopped_by": null,
            "preview": "declined by the operator"}),
        ),
        note("confirm.requested", confirm_tool()),
        note("confirm.requested", confirm_budget()),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_k4",
            "approved": true, "by": "the CLI", "trust": false}),
        ),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_b5",
            "approved": false, "by": "discord:ana"}),
        ),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_k6",
            "approved": false, "superseded": true, "by": "the web UI"}),
        ),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_k7",
            "approved": false, "cancelled": true, "by": "the CLI"}),
        ),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_k8",
            "approved": false, "stopped": true, "by": "the CLI"}),
        ),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_k9",
            "approved": false, "withdrawn": true, "by": "config"}),
        ),
        note("policy.tightened", tighten_result(true, false)),
        note("policy.untightened", tighten_result(true, false)),
        note(
            "session.trusted",
            json!({"session_id": S, "by": "the CLI", "who": "sock#3",
            "via": "cli", "how": "policy.trust", "at_ms": 1_759_300_400_000u64,
            "held": external()}),
        ),
        note(
            "profile.changed",
            json!({"live": "quick", "previous": "deep", "by": "the CLI"}),
        ),
        note(
            "narrative.line",
            json!({"seq": 7, "at_unix_ms": 1_759_300_500_000u64,
            "part": "turn", "session_id": S, "text": "The turn ended."}),
        ),
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 1,
            "text": "\nTwo files: a.md and b.md."}),
        ),
        note(
            "loop.ended",
            json!({"turn_id": T, "loop_index": 1,
            "provider_stop_reason": "end_turn", "tool_calls": 0, "advancer": "stop",
            "decision": "end"}),
        ),
        note(
            "turn.failed",
            json!({"session_id": S, "turn_id": T, "execution_id": X,
            "continuation": false, "class": "rate_limited", "error": "429 Too Many Requests",
            "then": "backoff"}),
        ),
        note(
            "turn.failed",
            json!({"session_id": S, "turn_id": null, "execution_id": X,
            "continuation": true, "class": null, "error": "the store is full"}),
        ),
        note(
            "turn.ended",
            turn_result("Looking at the folder.\nTwo files: a.md and b.md."),
        ),
    ]
}

/// The shapes of the `Printer`'s that `turn_notes` leaves out (theseus-7yx,
/// step 10a). They were written before its formatting moved into the
/// library, so the move is held to them: a thinking run a call ends, a call
/// with its argv and no pid yet, a call with no time, a reply line that ends
/// itself, a question with no floor and no external text, a notice with no
/// grant and no call, the other words of a tightening and of its undo, an
/// answer with no author, a recompile with no trigger, a failure retried
/// once, and one with a class and no plan. A context's `append` outside
/// `watch`, an execution's change, an empty delta, a newer daemon's method,
/// and a payload this build cannot read print nothing.
fn more_notes() -> Vec<Value> {
    let compiled = |loop_index: u32, decision: &str, nodes: u64, tokens: u64| {
        json!({"session_id": S, "turn_id": T, "loop": loop_index, "decision": decision,
            "trigger": null, "compilation_id": "cmp_c3", "strategy": "transcript",
            "prefix_nodes": 0, "tail_nodes": nodes, "messages": nodes, "est_tokens": tokens,
            "digest": "0a1b2c3d4e5f6071", "repairs": [], "tools": 14, "nodes_scanned": nodes,
            "cache": {"breakpoints": [], "ttl": "5m", "conversation_ttl": "5m"}})
    };
    vec![
        note("context.compiled", compiled(0, "recompile", 9, 1830)),
        note("context.compiled", compiled(1, "append", 10, 1990)),
        note(
            "model.thinking",
            json!({"turn_id": T, "loop_index": 0,
            "text": "Two tables;\nthe harbour's first."}),
        ),
        note(
            "tool.started",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_6",
            "tool": "proc.run", "correlation_id": "act_m1", "backend": "job",
            "argv": ["tide", "--port", "lantern"]}),
        ),
        note(
            "tool.ended",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_6",
            "tool": "proc.run", "status": "error", "duration_ms": null,
            "correlation_id": "act_m1", "late": false, "truncated": false, "bytes": 1024,
            "node_id": "nod_r6", "exit_code": 2, "stopped_by": null,
            "preview": "no such port"}),
        ),
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 1,
            "text": "The harbour's table:\n"}),
        ),
        note("confirm.requested", confirm_plain()),
        note(
            "policy.notified",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_7",
            "correlation_id": "", "tool": "web.search",
            "input": {"query": "lantern tide tables"},
            "summary": "search \"lantern tide tables\"", "kind": "notify",
            "setting": "enforcement = notify",
            "rule": "web.search — notify (enforcement = notify)"}),
        ),
        note("policy.tightened", tighten_result(false, true)),
        note("policy.tightened", tighten_result(false, false)),
        note("policy.untightened", tighten_result(false, false)),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_m3", "approved": true}),
        ),
        note(
            "execution.changed",
            push_view(60, "running", "working", "turn 4"),
        ),
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 1, "text": ""}),
        ),
        note("tides.forecast", json!({"session_id": S})),
        note("tool.started", json!({"session_id": S, "tool": 7})),
        note(
            "turn.failed",
            json!({"session_id": S, "turn_id": T, "execution_id": X,
            "continuation": false, "class": "overloaded", "error": "529 overloaded",
            "then": "retry"}),
        ),
        note(
            "turn.failed",
            json!({"session_id": S, "turn_id": T, "execution_id": X,
            "continuation": true, "class": "auth", "error": "the key was refused"}),
        ),
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 2,
            "text": "Low water at 14:10."}),
        ),
    ]
}

fn span(name: &str, kind: &str, start: u64, end: u64, children: Vec<Value>) -> Value {
    let mut s = json!({"name": name, "kind": kind, "start_us": start, "end_us": end});
    if !children.is_empty() {
        s["children"] = json!(children);
    }
    s
}

fn trace() -> Value {
    let mut call = span("provider.call", "provider", 2_000, 900_000, vec![]);
    call["attrs"] = json!({"model": "glm-x", "max_tokens": 4096, "digest": "9f2c1a0b7d3e4f51"});
    span(
        "turn",
        "turn",
        0,
        3_021_000,
        vec![
            span("admission.wait", "lock", 100, 600, vec![]),
            span(
                "loop 0",
                "loop",
                1_000,
                1_500_000,
                vec![
                    call,
                    span(
                        "tools",
                        "tools",
                        900_500,
                        1_400_000,
                        vec![
                            span("fs.list", "tool", 900_600, 1_000_000, vec![]),
                            span("proc.run", "tool", 900_700, 1_399_000, vec![]),
                        ],
                    ),
                    span("advance", "mark", 1_450_000, 1_450_000, vec![]),
                ],
            ),
            span(
                "loop 1",
                "loop",
                1_500_100,
                3_020_000,
                vec![span(
                    "provider.call",
                    "provider",
                    1_500_200,
                    3_019_000,
                    vec![],
                )],
            ),
        ],
    )
}

fn node(kind: &str, at: u64, text: &str, detail: Value) -> Value {
    json!({"node_id": format!("nod_{at}"), "kind": kind, "session_id": S, "position": at,
           "at_unix_ms": 1_759_300_000_000u64 + at * 1_234, "turn_id": T, "text": text,
           "detail": detail, "bytes": text.len()})
}

fn history() -> Value {
    let long = "Two files. ".repeat(40);
    let mut operator = node(
        "user_message",
        1,
        "Please tidy the notes folder.\nThen sum up.",
        Value::Null,
    );
    operator["author"] = json!("discord:ana");
    let mut thinker = node(
        "assistant_message",
        3,
        "",
        json!({"model": "glm-x", "provider": "zed", "stop_reason": "tool_use",
               "usage": {"input_tokens": 1200, "output_tokens": 85},
               "cost_usd": 0.0012, "request_id": "req_a1", "correlation_id": "act_p1",
               "compilation_id": "cmp_b2",
               "tool_calls": [{"id": "tu_1", "name": "fs_list", "input": {"path": "notes"}},
                              {"id": "tu_2", "name": "proc_run", "input": {"argv": ["make"]}}],
               "blocks": 3}),
    );
    thinker["thinking"] = json!("Look first,\nthen act.");
    let gate_open = json!({"tool_use_id": "tu_1", "tool": "fs.list", "input": {"path": "notes"},
        "correlation_id": "act_k1",
        "decision": {"posture": "open", "reason": "fs.list — open (enforcement = open)"},
        "result": {"gate": "allow"}, "plan": {"summary": "list notes"}});
    let gate_old = json!({"tool_use_id": "tu_2", "tool": "proc.run", "input": {"argv": ["make"]},
        "correlation_id": "act_k2", "decision": {"mode": "auto", "reason": "an older row's band"},
        "result": {"gate": "allow"}, "plan": null});
    let gate_invalid = json!({"tool_use_id": "tu_3", "tool": "fs.read", "input": {},
        "correlation_id": null, "decision": null,
        "result": {"gate": "deny", "reason": "validation: missing field `path`"}, "plan": null});
    let gate_waiting = json!({"tool_use_id": "tu_4", "tool": "fs.write",
        "input": {"path": "/srv/motd", "text": "x".repeat(200)}, "correlation_id": "act_k4",
        "decision": null, "result": {"gate": "needs_confirm", "by": "operator"}, "plan": null});
    let result = |tool: &str, status: &str, meta: Value, late: bool| {
        json!({"tool_use_id": "tu_1", "tool": tool, "status": status,
               "is_error": status != "ok", "correlation_id": "act_k1", "truncated": false,
               "duration_ms": 1520, "late": late, "meta": meta, "external": null})
    };
    json!({
        "session": {"session_id": S, "kind": "conversation", "label": null,
                    "created_at_unix_ms": 1_759_300_000_000u64, "turns": 3,
                    "usage": {"input_tokens": 9000, "output_tokens": 400},
                    "execution_id": X, "execution_state": "waiting",
                    "last_active_ms": 1_759_300_900_000u64, "cost_usd": 0.0123,
                    "tool_calls": 4, "model": "glm-x", "title": "Tidy the notes"},
        "nodes": [
            operator,
            node("user_message", 2, "And list them.", Value::Null),
            thinker,
            node("tool_call", 4, "", gate_open),
            node("tool_call", 5, "", gate_old),
            node("tool_call", 6, "", gate_invalid),
            node("tool_call", 7, "", gate_waiting),
            node("tool_result", 8, "a.md\nb.md", result("fs.list", "ok", json!({"exit_code": 0}), false)),
            node("tool_result", 9, "", result("proc.run", "cancelled",
                json!({"stopped_by": "the CLI"}), true)),
            node("tool_result", 10, "declined by the operator", result("fs.write", "declined", Value::Null, false)),
            node("tool_result", 11, "denied", result("fs.write", "denied", json!({}), false)),
            node("assistant_message", 12, &long,
                json!({"model": "glm-x", "stop_reason": "end_turn",
                       "usage": {"input_tokens": 2200, "output_tokens": 310}, "cost_usd": null,
                       "tool_calls": []})),
            node("compaction", 13, "", Value::Null),
        ],
        "pending_confirms": [confirm_tool(), confirm_budget()]
    })
}

fn health() -> Value {
    json!({
        "name": "theseusd", "version": "0.0.1", "protocol": "0.1", "uptime_secs": 42,
        "sessions": 3, "turns": 9, "model": "glm-x", "profile": "quick", "provider": "zed",
        "providers": ["zed", "orbit"], "secrets_resolved": ["ZED_KEY"],
        "secrets": {"state": "ready", "ready": ["ZED_KEY"], "method": "service account",
                    "rounds": 1, "started_ms": 10, "settled_ms": 822, "failed": []},
        "usage_total": {"input_tokens": 120000, "output_tokens": 3400,
                        "cache_read_input_tokens": 50000, "cache_creation_input_tokens": 1200},
        "provider_errors": 1, "ledger_rows": 2048,
        "kernel": {"accepting": true, "turns_held": 1, "admission_ceiling": 8,
                   "executions_by_state": {"waiting": 2, "running": 1},
                   "actions_by_state": {"planned": 1},
                   "quarantined_completions": 0, "lingering_wrappers": 1},
        "broker": [],
        "bindings": [{"kind": "discord", "state": "connected", "detail": "as Clerk",
                      "messages_in": 4, "messages_out": 6, "edits": 2, "interactions": 1,
                      "ignored": 0, "errors": 0,
                      "places": [{"kind": "channel", "label": "#lab", "session_id": S},
                                 {"kind": "dm", "label": "DM ana", "session_id": null}]}],
        "tightenings": [{"tool": "proc.run", "posture": "approve", "by": "the CLI",
                         "at_ms": 1_759_300_300_250u64}],
        "external_text": [],
    })
}

/// The core's `index.status` (roadmap row 51): a hybrid index, caught up, its
/// tender restarted once, after a SIGKILL.
fn index_ready() -> Value {
    json!({
        "state": "ready",
        "tender": {"name": "index", "state": "running", "pid": 4242, "adopted": false,
                   "started_at_ms": 1_759_300_000_000u64, "restarts": 1, "last_exit": "signal 9",
                   "last_exit_ms": 1_759_299_999_000u64, "backoff_ms": 1000,
                   "binary": "/opt/theseus/bin/theseus-index"},
        "status": {"state": "ready", "mode": "hybrid", "pid": 4242,
                   "index_dir": "/home/op/.theseus/index", "wal_dir": "/home/op/.theseus/store/wal",
                   "position": 1607, "segment": 0, "offset": 182_340, "documents": 151, "nodes": 100,
                   "lag": {"bytes": 0, "ms": 0}, "commits": 12, "records_read": 1490,
                   "nodes_indexed": 100, "nodes_skipped": 11, "undecodable": 0, "rebuilds": 0,
                   "extractor": 2, "schema": 1, "rss_bytes": 25_165_824,
                   "started_at_ms": 1_759_300_000_000u64, "last_commit_ms": 1_759_300_100_000u64,
                   "vectors": {"model": "loaded", "weights_dir": "/home/op/.cache/theseus/models",
                               "stamp": {"model": "nomic-embed-text-v1.5@e5cf08a",
                                         "weights": "ab12", "tokenizer": "cd34",
                                         "engine": "candle-0.11.0+embed.2", "precision": "f32",
                                         "dims": [256, 768]},
                               "chunks": 151, "vectors": 149, "pending": 2,
                               "backfill": {"texts": 149, "batches": 12, "tokens": 40_210,
                                            "truncated": 0, "wall_ms": 118_000, "cpu_ms": 117_000,
                                            "failed": 0},
                               "loads": 1, "unloads": 0, "load_ms": 530.0,
                               "loaded_at_ms": 1_759_300_000_600u64,
                               "last_used_ms": 1_759_300_100_000u64, "idle_unload_secs": 600,
                               "threads": "RAYON_NUM_THREADS=1 CANDLE_NUM_THREADS=1"}}
    })
}

/// The same tender, down: it exited, and waits its backoff.
fn index_down() -> Value {
    json!({
        "state": "down",
        "why": "it exited (exit 1) after 0.4 s; it starts again in 4 s",
        "tender": {"name": "index", "state": "backoff", "restarts": 3, "last_exit": "exit 1",
                   "last_exit_ms": 1_759_300_000_000u64, "next_start_ms": 1_759_300_004_000u64,
                   "backoff_ms": 4000, "why": "it exited (exit 1) after 0.4 s; it starts again in 4 s",
                   "binary": "/opt/theseus/bin/theseus-index"}
    })
}

/// The core's `index.query`: two hits, one a tool's external result held by
/// an entity alone.
fn index_hits() -> Value {
    json!({
        "hits": [
            {"node_id": "nod_u1", "chunk": 0, "session_id": S, "position": 1509,
             "kind": "user_message", "origin": "cli", "time_ms": 1_759_300_000_000u64,
             "external": false, "text": "the web UI listens on 7433,\nnot on 7434",
             "sources": {"bm25": {"rank": 1, "score": 3.2}, "vector": {"rank": 2, "score": 0.71}},
             "fused": 0.1148},
            {"node_id": "nod_t2", "chunk": 1, "session_id": "ses_b2c9w1", "position": 1388,
             "kind": "tool_result", "origin": "tool", "tool": "fs.read",
             "time_ms": 1_759_299_000_000u64, "external": true,
             "text": "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"0.0.1\"\nedition = \"2021\"\nlicense = \"MIT OR Apache-2.0\"\nrust-version = \"1.85\"\n# the port is 7433 in the template",
             "entities_matched": ["file:Cargo.toml"],
             "sources": {"entity": {"rank": 1, "score": 2.0}}, "fused": 0.0164}
        ],
        "indexed_through": 1607, "lag": {"bytes": 0, "ms": 0},
        "timings": {"bm25_ms": 0.4, "entity_ms": 0.1, "embed_ms": 74.2, "vector_ms": 0.6,
                    "fuse_ms": 0.0, "load_ms": 0.0, "total_ms": 75.5},
        "weights": {"bm25": 1.0, "entity": 1.0, "vector": 6.0}
    })
}

/// The params `theseus ARGS` sends with its one request, answered `result`.
fn request_params(args: &[&str], result: Value) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        req["params"].clone()
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(args)
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    daemon.join().unwrap()
}

fn tools() -> Value {
    json!({
        "tools": [
            {"name": "fs.read", "wire_name": "fs_read", "family": "fs", "class": "read", "backend": "inproc",
             "policy": "open", "config_posture": "open", "description": "Read a file.",
             "input_schema": {"type": "object"}, "calls": 12},
            {"name": "proc.run", "wire_name": "proc_run", "family": "proc", "class": "exec", "backend": "job",
             "policy": "approve", "config_posture": "notify",
             "tightened": {"tool": "proc.run", "posture": "approve", "by": "the CLI",
                           "at_ms": 1_759_300_300_250u64},
             "description": "Run a program.", "input_schema": {"type": "object",
             "required": ["argv"]}, "calls": 3}
        ],
        "roots": ["/w", "/srv/notes"], "calls_total": 15, "shell_fallback_ratio": 0.2
    })
}

fn execution(id: &str, state: &str, ended: Option<&str>) -> Value {
    json!({"execution_id": id, "session_id": S, "kind": "conversation", "state": state,
           "turns": 3, "interrupted": 0, "outstanding": 1, "queued_results": 0,
           "budget": {"limit_usd": 100.0, "spent_usd": 0.25, "reserved_usd": 0.01,
                      "held_unknown_usd": 0.0, "available_usd": 99.74},
           "wake": null, "ended_reason": ended,
           "created_at_ms": 1_759_300_000_000u64, "updated_at_ms": 1_759_300_900_000u64})
}

// ---------------------------------------------------------------- scenarios

#[test]
fn ask_streams_the_reply_and_every_event_line() {
    let mut s = step(
        "turn.submit",
        turn_result("Looking at the folder.\nTwo files: a.md and b.md."),
    );
    s.before = turn_notes();
    golden(
        "ask_stream",
        &run(&["ask", "--thinking", "Tidy the notes."], vec![s]),
    );
}

#[test]
fn ask_parked_on_a_confirm_says_how_to_answer() {
    let mut r = turn_result("");
    r["awaiting_confirm"] = json!("act_k4");
    r["usage"]["cache_creation_input_tokens"] = json!(512);
    r["cost_usd"] = Value::Null;
    r["first_token_ms"] = Value::Null;
    r["tool_calls"] = json!(0);
    r["continuation"] = json!(true);
    let mut s = step("turn.submit", r);
    s.before = vec![note("confirm.requested", confirm_tool())];
    golden("ask_parked", &run(&["ask", "Write the motd."], vec![s]));
}

#[test]
fn ask_without_streaming_prints_the_reply_once() {
    let mut s = step("turn.submit", turn_result("Two files."));
    s.before = turn_notes();
    golden(
        "ask_no_stream",
        &run(&["--no-stream", "ask", "List."], vec![s]),
    );
}

#[test]
fn ask_json_prints_the_result_only() {
    let mut s = step("turn.submit", turn_result("Two files."));
    s.before = turn_notes();
    golden("ask_json", &run(&["--json", "ask", "List."], vec![s]));
}

#[test]
fn ask_trace_prints_the_span_tree() {
    let mut r = turn_result("Two files.");
    r["trace"] = trace();
    golden(
        "ask_trace",
        &run(&["ask", "--trace", "List."], vec![step("turn.submit", r)]),
    );
}

#[test]
fn a_failed_ask_prints_its_trace_and_the_error() {
    let s = Step {
        method: "turn.submit",
        before: vec![note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 0, "text": "Look"}),
        )],
        answer: Err(
            json!({"code": -32003, "message": "the provider is overloaded",
            "data": {"class": "overloaded", "transient": true, "usage_unknown": false,
                     "turn_id": T, "session_id": S, "elapsed_ms": 900, "trace": trace()}}),
        ),
        after: vec![],
    };
    golden(
        "ask_failed_trace",
        &run(&["ask", "--trace", "List."], vec![s]),
    );
}

#[test]
fn watch_prints_turns_and_context_decisions() {
    let mut s = step("session.watch", json!({}));
    let mut notes = turn_notes();
    notes.insert(
        2,
        note(
            "context.compiled",
            json!({"session_id": S, "turn_id": T, "loop": 1,
            "decision": "append", "trigger": null, "compilation_id": "cmp_b2",
            "strategy": "fresh", "prefix_nodes": 2, "tail_nodes": 7, "messages": 8,
            "est_tokens": 4400, "digest": "9f2c1a0b7d3e4f51", "repairs": [], "tools": 14,
            "nodes_scanned": 12}),
        ),
    );
    notes.push(note(
        "turn.started",
        json!({"session_id": S, "turn_id": "turn_c0nt1n",
        "execution_id": X, "continuation": true}),
    ));
    s.after = notes;
    golden("watch", &run(&["watch", S, "--thinking"], vec![s]));
}

#[test]
fn watch_json_prints_each_notification() {
    let mut s = step("session.watch", json!({}));
    s.after = turn_notes().into_iter().take(6).collect();
    golden("watch_json", &run(&["--json", "watch", S], vec![s]));
}

#[test]
fn watch_finds_the_most_recent_session() {
    let list = json!({"sessions": [{"session_id": S, "kind": "conversation", "label": null,
        "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1}]});
    golden(
        "watch_recent",
        &run(
            &["watch"],
            vec![step("session.list", list), step("session.watch", json!({}))],
        ),
    );
}

/// The `Printer`'s other shapes (theseus-7yx, step 10a), as `ask` streams
/// them, as `watch` shows them (the `append` too), and as `ask --no-stream`
/// shows them: the thinking and the event lines, and none of the reply.
#[test]
fn ask_watch_and_no_stream_print_the_printers_other_shapes() {
    let ask = || {
        let mut s = step(
            "turn.submit",
            turn_result("The harbour's table: low water at 14:10."),
        );
        s.before = more_notes();
        vec![s]
    };
    golden(
        "ask_shapes",
        &run(&["ask", "--thinking", "When is low water?"], ask()),
    );
    golden(
        "ask_no_stream_thinking",
        &run(
            &["--no-stream", "ask", "--thinking", "When is low water?"],
            ask(),
        ),
    );
    let mut s = step("session.watch", json!({}));
    s.after = more_notes();
    golden("watch_shapes", &run(&["watch", S, "--thinking"], vec![s]));
}

/// `theseus watch` after `events.lost` (theseus-in3): it ends the reply's
/// line, says what it lost and where to read it, and watches on.
#[test]
fn watch_says_what_it_lost_and_where_to_read_it() {
    let mut s = step("session.watch", json!({}));
    s.after = vec![
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 0, "text": "Halfway"}),
        ),
        note(
            "events.lost",
            json!({"dropped": 1, "streams": [format!("session:{S}")]}),
        ),
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 0, "text": " there."}),
        ),
    ];
    golden("watch_lost", &run(&["watch", S], vec![s]));
}

#[test]
fn history_prints_every_node_kind_and_what_waits() {
    golden(
        "history",
        &run(&["history", S], vec![step("session.history", history())]),
    );
}

#[test]
fn history_full_prints_nodes_whole() {
    golden(
        "history_full",
        &run(
            &["history", S, "--full"],
            vec![step("session.history", history())],
        ),
    );
}

#[test]
fn history_json_is_the_raw_result() {
    golden(
        "history_json",
        &run(
            &["--json", "history", S, "-n", "3"],
            vec![step("session.history", history())],
        ),
    );
}

/// `theseus history --before` and `--after` (theseus-xo0m, theseus-kym3):
/// a page, and the command that reads past it either way.
#[test]
fn history_pages_say_how_to_read_past_them() {
    let page = |cursor: Value| {
        let mut h = history();
        h["nodes"] = json!([
            node("user_message", 1, "And list them.", Value::Null),
            node(
                "assistant_message",
                3,
                "Two files.",
                json!({"model": "glm-x"})
            ),
        ]);
        h["pending_confirms"] = json!([]);
        for (k, v) in cursor.as_object().unwrap() {
            h[k] = v.clone();
        }
        h
    };
    let back = run(
        &["history", S, "-n", "2", "--before", "5"],
        vec![step("session.history", page(json!({"older": 1})))],
    );
    let forward = run(
        &["history", S, "-n", "2", "--after", "0"],
        vec![step("session.history", page(json!({"next": 3})))],
    );
    golden("history_pages", &format!("{back}{forward}"));
}

#[test]
fn history_with_no_sessions_says_so() {
    golden(
        "history_none",
        &run(
            &["history"],
            vec![step("session.list", json!({"sessions": []}))],
        ),
    );
}

#[test]
fn confirm_lists_everything_waiting() {
    let list = json!({"confirms": [confirm_tool(), confirm_budget()]});
    golden(
        "confirm_list",
        &run(&["confirm"], vec![step("confirm.list", list.clone())]),
    );
    golden(
        "confirm_list_json",
        &run(&["--json", "confirm"], vec![step("confirm.list", list)]),
    );
    golden(
        "confirm_list_empty",
        &run(
            &["confirm"],
            vec![step("confirm.list", json!({"confirms": []}))],
        ),
    );
}

#[test]
fn confirm_follows_the_resumed_turn() {
    let mut s = step(
        "action.confirm",
        json!({"correlation_id": "act_k4", "approved": true,
        "session_id": S, "execution_id": X, "resumes": true}),
    );
    s.before = vec![note(
        "confirm.resolved",
        json!({"session_id": S,
        "correlation_id": "act_k4", "approved": true, "by": "the CLI", "trust": true}),
    )];
    let mut ended = turn_result("Wrote it.");
    ended["continuation"] = json!(true);
    ended["awaiting_confirm"] = json!("act_k10");
    s.after = vec![
        note(
            "turn.started",
            json!({"session_id": S, "turn_id": T, "execution_id": X,
            "continuation": true}),
        ),
        note(
            "tool.started",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_4",
            "tool": "fs.write", "correlation_id": "act_k4", "backend": "inproc"}),
        ),
        note(
            "tool.ended",
            json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_4",
            "tool": "fs.write", "status": "ok", "duration_ms": 3, "correlation_id": "act_k4",
            "late": false, "truncated": false, "bytes": 2100000, "node_id": "nod_r4",
            "exit_code": null, "stopped_by": null, "preview": "wrote 5 bytes"}),
        ),
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 0, "text": "Wrote it."}),
        ),
        note("turn.ended", ended),
    ];
    golden(
        "confirm_follow",
        &run(&["confirm", "act_k4", "--trust"], vec![s]),
    );
}

#[test]
fn confirm_json_follows_to_the_turns_result() {
    let mut s = step(
        "action.confirm",
        json!({"correlation_id": "act_k4", "approved": true,
        "session_id": S, "execution_id": X, "resumes": true}),
    );
    s.after = vec![
        note(
            "model.delta",
            json!({"turn_id": T, "loop_index": 0, "text": "Wrote it."}),
        ),
        note("turn.ended", turn_result("Wrote it.")),
    ];
    golden(
        "confirm_follow_json",
        &run(&["--json", "confirm", "act_k4"], vec![s]),
    );
}

#[test]
fn confirm_a_failed_resume_says_so() {
    let mut s = step(
        "action.confirm",
        json!({"correlation_id": "act_k4", "approved": true,
        "session_id": S, "execution_id": X}),
    );
    s.after = vec![note(
        "turn.failed",
        json!({"session_id": S, "turn_id": T,
        "execution_id": X, "continuation": true, "class": "auth",
        "error": "the key was refused", "then": "park"}),
    )];
    golden("confirm_failed", &run(&["confirm", "act_k4"], vec![s]));
}

#[test]
fn confirm_without_waiting_says_what_was_recorded() {
    let answered = json!({"correlation_id": "act_k4", "approved": true, "session_id": S,
        "execution_id": X, "resumes": true});
    golden(
        "confirm_no_wait",
        &run(
            &["confirm", "act_k4", "--no-wait"],
            vec![step("action.confirm", answered.clone())],
        ),
    );
    golden(
        "confirm_no_wait_json",
        &run(
            &["--json", "confirm", "act_k4", "--no-wait"],
            vec![step("action.confirm", answered)],
        ),
    );
    let budget = json!({"correlation_id": "act_b5", "approved": false, "session_id": S,
        "execution_id": X, "resumes": false});
    golden(
        "confirm_budget_declined",
        &run(
            &["confirm", "--decline", "act_b5"],
            vec![step("action.confirm", budget.clone())],
        ),
    );
    golden(
        "confirm_budget_declined_json",
        &run(
            &["--json", "confirm", "--decline", "act_b5"],
            vec![step("action.confirm", budget)],
        ),
    );
}

/// A canned `judge.prove` answer (invented data): its Markdown ends in a
/// newline, and its records are two JSON lines.
fn prove_result() -> Value {
    json!({
        "pack": "loop.v1", "since_ms": 1_759_276_800_000u64, "until_ms": null,
        "window": "every task that ended since 2026-10-01", "verdict": "insufficient",
        "report": {}, "markdown": "# loop.v1 prove\n\nverdict: insufficient\n- canary: 2 tasks\n",
        "tasks": 5, "arms": {"canary": 2, "control": 1}, "left_out": {"never_judged": 2},
        "notes": ["nudges are 0"], "classification": [], "elapsed_ms": 9,
        "records": "{\"arm\":\"canary\",\"task\":\"tsk_a1\"}\n{\"arm\":\"control\",\"task\":\"tsk_b2\"}\n",
    })
}

/// What the golden's `run` text holds between its `--- stdout` and `--- stderr` marks.
fn stdout_of(out: &str) -> &str {
    let from = out.find("--- stdout\n").unwrap() + "--- stdout\n".len();
    &out[from..out.rfind("--- stderr\n").unwrap()]
}

/// `theseus judge prove` writes the report's Markdown to stdout, byte for byte
/// (one newline neither more nor less), and what it read to stderr
/// (theseus-w38g).
#[test]
fn judge_prove_prints_the_markdown_as_it_is() {
    let r = prove_result();
    let out = run(&["judge", "prove"], vec![step("judge.prove", r.clone())]);
    golden("judge_prove", &out);
    assert_eq!(stdout_of(&out), r["markdown"].as_str().unwrap());
}

/// `--records -` prints the records instead of the report, byte for byte; the
/// read lines still go to stderr.
#[test]
fn judge_prove_records_to_stdout_are_the_records() {
    let r = prove_result();
    let out = run(
        &["judge", "prove", "--records", "-"],
        vec![step("judge.prove", r.clone())],
    );
    golden("judge_prove_records", &out);
    assert_eq!(stdout_of(&out), r["records"].as_str().unwrap());
}

/// `--records FILE` saves exactly the records and still prints the Markdown.
#[test]
fn judge_prove_records_to_a_file_are_the_records() {
    let r = prove_result();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("r.jsonl");
    let out = run(
        &["judge", "prove", "--records", file.to_str().unwrap()],
        vec![step("judge.prove", r.clone())],
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        r["records"].as_str().unwrap()
    );
    assert_eq!(stdout_of(&out), r["markdown"].as_str().unwrap());
}

#[test]
fn health_prints_every_line() {
    golden("health", &run(&["health"], vec![step("health", health())]));
    golden(
        "health_json",
        &run(&["--json", "health"], vec![step("health", health())]),
    );
}

/// A daemon with an import (theseus-revl): the owner's own sessions, then the
/// imported ones held and the erased ones; a store with none prints as before.
#[test]
fn health_names_an_import_apart_from_the_owners_sessions() {
    let mut h = health();
    h["sessions"] = json!(512);
    h["imported"] = json!({"sessions": 21151, "erased": 3});
    golden(
        "health_imported",
        &run(&["health"], vec![step("health", h.clone())]),
    );
    h["imported"] = json!({"sessions": 21151, "erased": 0});
    let out = run(&["health"], vec![step("health", h)]);
    assert!(
        out.contains(" · sessions 512 · imported 21,151 · turns 9 · "),
        "{out}"
    );
}

/// A daemon with the push (theseus-in3): health's `push:` line, seeded and
/// not.
#[test]
fn health_says_where_the_push_stands() {
    let mut h = health();
    h["push"] = json!({"seeded": true, "seed_us": 38_400, "board": 212, "questions": 1,
                       "watchers": 2, "events": 340, "waiting": 1, "lost": 865,
                       "position": 48213});
    // One-hour cache writes are named after the total (theseus-xiaz).
    h["usage_total"]["cache_creation_1h_input_tokens"] = json!(300);
    golden(
        "health_push",
        &run(&["health"], vec![step("health", h.clone())]),
    );
    h["push"] = json!({"seeded": false});
    let out = run(&["health"], vec![step("health", h)]);
    assert!(
        out.contains("push: not seeded: nothing has watched since the start"),
        "{out}"
    );
}

/// A daemon with the index tender (roadmap row 51): health's `index:` line,
/// and the tender in its `children:` line; off, and down.
#[test]
fn health_says_where_the_index_stands() {
    let mut h = health();
    h["index"] = index_ready();
    h["children"] = json!({"subreaper": true, "wrappers_running": 1, "wrappers_lingering": 0,
                           "orphans": 0, "zombies": 0, "owned": 0, "reaped_wrappers": 7,
                           "reaped_orphans": 0, "tenders": [index_ready()["tender"]]});
    golden(
        "health_index",
        &run(&["health"], vec![step("health", h.clone())]),
    );
    h["index"] = json!({"state": "off", "why": "[index] enabled = false"});
    let out = run(&["health"], vec![step("health", h.clone())]);
    assert!(
        out.contains("\nindex: off · [index] enabled = false\n"),
        "{out}"
    );
    h["index"] = index_down();
    let out = run(&["health"], vec![step("health", h)]);
    assert!(
        out.contains("\nindex: down · it exited (exit 1) after 0.4 s; it starts again in 4 s\n"),
        "{out}"
    );
}

/// `theseus index status` and `theseus index search` (roadmap row 51): the
/// index, its tender, cursor, and vectors; one that is down; the hits with
/// each source's rank, cut to one line; a search the core could not answer;
/// and the query the core gets, `--sources` included.
#[test]
fn index_status_and_search_print_the_index_and_its_hits() {
    golden(
        "index_status",
        &run(
            &["index", "status"],
            vec![step("index.status", index_ready())],
        ),
    );
    golden(
        "index_status_json",
        &run(
            &["--json", "index", "status"],
            vec![step("index.status", index_ready())],
        ),
    );
    golden(
        "index_status_down",
        &run(
            &["index", "status"],
            vec![step("index.status", index_down())],
        ),
    );
    golden(
        "index_search",
        &run(
            &["index", "search", "port", "7433", "-k", "5"],
            vec![step("index.query", index_hits())],
        ),
    );
    let mut loading = index_hits();
    loading["skipped"] = json!({"vector": "the model is loading"});
    loading["hits"] = json!([]);
    golden(
        "index_search_skipped",
        &run(
            &["index", "search", "nothing", "like", "it"],
            vec![step("index.query", loading)],
        ),
    );
    golden(
        "index_search_json",
        &run(
            &["--json", "index", "search", "port 7433"],
            vec![step("index.query", index_hits())],
        ),
    );
    let down = Step {
        method: "index.query",
        before: vec![],
        answer: Err(json!({"code": -32603, "message":
            "the index tender did not answer: it exited (exit 1) after 0.4 s; it starts again in 4 s"})),
        after: vec![],
    };
    golden(
        "index_search_down",
        &run(&["index", "search", "port", "7433"], vec![down]),
    );
    // The query as the core gets it: its words joined, `-k`, `--as-of`.
    assert_eq!(
        request_params(
            &["index", "search", "port", "7433", "-k", "5", "--as-of", "1600"],
            index_hits()
        ),
        json!({"text": "port 7433", "k": 5, "as_of": 1600})
    );
    assert_eq!(
        request_params(&["index", "search", "7433"], index_hits()),
        json!({"text": "7433", "k": 10})
    );
    // `--sources`: these sources alone.
    assert_eq!(
        request_params(
            &[
                "index",
                "search",
                "what",
                "looks",
                "--sources",
                "bm25,entity"
            ],
            index_hits()
        ),
        json!({"text": "what looks", "k": 10, "sources": ["bm25", "entity"]})
    );
}

/// `theseus watch --all` (theseus-in3): the snapshot, then each change and
/// each question. An event that came before the answer prints once, at its
/// greater position, and one the snapshot already holds is dropped.
#[test]
fn watch_all_prints_the_snapshot_then_each_change_once() {
    let view = |pos: u64, sid: &str, state: &str, prev: Option<&str>, level: &str, label: &str| {
        let mut v = json!({"position": pos, "at_ms": 1_759_300_000_000u64,
            "execution_id": format!("exe_{}", &sid[4..]), "session_id": sid,
            "kind": "conversation", "state": state, "pending": [], "turns": 2,
            "spent_usd": 0.0123, "limit_usd": 100.0,
            "attention": {"level": level, "label": label, "since_ms": 1}});
        if let Some(p) = prev {
            v["previous"] = json!(p);
        }
        v
    };
    let snapshot = json!({"position": 40, "total": 2, "confirms": [], "executions": [
        view(38, S, "running", None, "working", "turn 2"),
        view(12, "ses_b0r1ng", "waiting", None, "ready", "ready")]});
    let early = note(
        "execution.changed",
        view(
            39,
            "ses_b0r1ng",
            "queued",
            Some("waiting"),
            "working",
            "queued",
        ),
    );
    let stale = note(
        "execution.changed",
        view(38, S, "running", Some("queued"), "working", "turn 2"),
    );
    let after = vec![
        note(
            "execution.changed",
            view(
                44,
                S,
                "waiting",
                Some("running"),
                "needs_you",
                "confirm proc.run: run the gate",
            ),
        ),
        note("confirm.requested", confirm_tool()),
        note(
            "confirm.resolved",
            json!({"session_id": S, "correlation_id": "act_k2", "approved": true, "by": "the CLI"}),
        ),
        note(
            "execution.changed",
            view(51, S, "queued", Some("waiting"), "working", "queued"),
        ),
    ];
    golden(
        "watch_all",
        &run(
            &["watch", "--all"],
            vec![Step {
                method: "executions.watch",
                before: vec![early, stale],
                answer: Ok(snapshot),
                after,
            }],
        ),
    );
}

/// A view as `execution.changed` and `session.wait` carry it.
fn push_view(pos: u64, state: &str, level: &str, label: &str) -> Value {
    json!({"position": pos, "at_ms": 1_759_300_000_000u64, "execution_id": X, "session_id": S,
           "kind": "conversation", "state": state, "pending": [], "turns": 3,
           "spent_usd": 0.25, "limit_usd": 100.0,
           "attention": {"level": level, "label": label, "since_ms": 1}})
}

/// `theseus wait` (theseus-in3): what it reached and the questions; a
/// timeout exits 4.
#[test]
fn wait_prints_what_it_reached_and_a_timeout_exits_4() {
    let execs = json!({"executions": [execution(X, "waiting", None)]});
    let blocked = json!({"reached": "blocked", "already": false,
        "execution": push_view(48, "waiting", "needs_you", "confirm fs.write: write outside the roots (/w)"),
        "confirms": [confirm_tool()]});
    golden(
        "wait_blocked",
        &run(
            &["wait", "q7f3k2", "--until", "blocked", "--timeout", "90s"],
            vec![
                step("execution.list", execs.clone()),
                step("session.wait", blocked),
            ],
        ),
    );
    let timeout = json!({"reached": "timeout", "already": false,
        "execution": push_view(51, "running", "working", "turn 3"), "confirms": []});
    golden(
        "wait_timeout",
        &run(
            &["wait", S, "--after", "48"],
            vec![step("execution.list", execs), step("session.wait", timeout)],
        ),
    );
}

/// `theseus executions explain` (theseus-in3): one execution in full.
#[test]
fn executions_explain_prints_one_in_full() {
    let mut e = execution(X, "waiting", None);
    e["waiting_on"] = json!({"on": "confirm", "confirm_id": "act_k4"});
    e["attention"] = json!({"level": "needs_you", "label": "confirm fs.write: write outside the roots (/w)", "since_ms": 1});
    let rows = json!({"total": 2048, "rows": [
        {"position": 2040, "at_unix_ms": 1_759_300_000_123u64, "kind": "execution.waiting",
         "session_id": S, "turn_id": null, "data": {"execution_id": X, "wake": {"on": "confirm"}}}]});
    golden(
        "executions_explain",
        &run(
            &["executions", "explain", "q7f3k2"],
            vec![
                step("execution.list", json!({"executions": [e]})),
                step("confirm.list", json!({"confirms": [confirm_tool()]})),
                step("wake.list", json!({"wakes": []})),
                step("ledger.tail", rows),
            ],
        ),
    );
}

/// `theseus watch --all` after `events.lost` (theseus-in3): it says so, reads
/// the board again, and prints only what changed.
#[test]
fn watch_all_reads_again_after_events_lost() {
    let first = json!({"position": 40, "total": 1, "confirms": [],
        "executions": [push_view(38, "running", "working", "turn 3")]});
    let again = json!({"position": 90, "total": 1, "confirms": [],
        "executions": [push_view(88, "waiting", "ready", "ready")]});
    golden(
        "watch_all_lost",
        &run(
            &["watch", "--all"],
            vec![
                Step {
                    method: "executions.watch",
                    before: vec![],
                    answer: Ok(first),
                    after: vec![note(
                        "events.lost",
                        json!({"dropped": 4120, "streams": ["executions"]}),
                    )],
                },
                step("executions.watch", again),
            ],
        ),
    );
}

#[test]
fn tools_lists_postures_and_calls() {
    golden("tools", &run(&["tools"], vec![step("tool.list", tools())]));
    golden(
        "tools_verbose",
        &run(&["tools", "-v"], vec![step("tool.list", tools())]),
    );
    golden(
        "tools_json",
        &run(&["--json", "tools"], vec![step("tool.list", tools())]),
    );
}

#[test]
fn policy_lists_tightens_untightens_and_trusts() {
    let mut h = health();
    h["external_text"] = json!([{"session_id": S, "title": "Tidy the notes",
        "held": external(), "since_local": "14:02"}]);
    golden(
        "policy_list",
        &run(
            &["policy"],
            vec![step("tool.list", tools()), step("health", h.clone())],
        ),
    );
    golden(
        "policy_list_json",
        &run(
            &["--json", "policy", "list"],
            vec![step("tool.list", tools()), step("health", h.clone())],
        ),
    );
    for (name, args, method, r) in [
        (
            "policy_tighten",
            vec!["policy", "tighten", "proc.run"],
            "policy.tighten",
            tighten_result(true, false),
        ),
        (
            "policy_tighten_already",
            vec!["policy", "tighten", "proc.run", "--call", "act_k2"],
            "policy.tighten",
            tighten_result(false, true),
        ),
        (
            "policy_tighten_asks",
            vec!["policy", "tighten", "proc.run"],
            "policy.tighten",
            tighten_result(false, false),
        ),
        (
            "policy_untighten",
            vec!["policy", "untighten", "proc.run"],
            "policy.untighten",
            tighten_result(true, false),
        ),
        (
            "policy_untighten_asks",
            vec!["policy", "untighten", "proc.run"],
            "policy.untighten",
            tighten_result(false, false),
        ),
        (
            "policy_tighten_json",
            vec!["--json", "policy", "tighten", "proc.run"],
            "policy.tighten",
            tighten_result(true, false),
        ),
    ] {
        golden(name, &run(&args, vec![step(method, r)]));
    }
    let trusted = json!({"session_id": S, "by": "the CLI", "who": "sock#3", "via": "cli",
        "how": "policy.trust", "at_ms": 1_759_300_400_000u64, "held": external(),
        "since_local": "14:02"});
    golden(
        "policy_trust",
        &run(
            &["policy", "trust", "f3k2"],
            vec![
                step("health", h.clone()),
                step("policy.trust", trusted.clone()),
            ],
        ),
    );
    golden(
        "policy_trust_json",
        &run(
            &["--json", "policy", "trust", S],
            vec![step("health", h.clone()), step("policy.trust", trusted)],
        ),
    );
    golden(
        "policy_trust_unknown",
        &run(&["policy", "trust", "zzzz"], vec![step("health", h)]),
    );
}

#[test]
fn catalog_prints_windows_and_prices() {
    let cat = json!({"version": "2026-09-30", "models": [
        {"model": "glm-x", "entry": {"provider": "zed", "context_window": 200000,
            "max_output_tokens": 16000, "input_per_mtok": 0.6, "output_per_mtok": 2.2,
            "cache_read_per_mtok": 0.11, "cache_write_per_mtok": 0.0, "thinking": "none"},
         "profiles": ["quick", "cheap"]},
        {"model": "orbit-5", "entry": {"provider": "orbit", "context_window": 1000000,
            "max_output_tokens": 64000, "input_per_mtok": 3.0, "output_per_mtok": 15.0,
            "cache_read_per_mtok": 0.3, "cache_write_per_mtok": 3.75,
            "cache_write_1h_per_mtok": 6.0, "thinking": "adaptive"},
         "profiles": []}]});
    golden(
        "catalog",
        &run(&["catalog"], vec![step("catalog.list", cat.clone())]),
    );
    golden(
        "catalog_json",
        &run(&["--json", "catalog"], vec![step("catalog.list", cat)]),
    );
}

/// A model priced in tiers (theseus-3okf): its long tier's threshold and
/// prices, under the table.
#[test]
fn catalog_lists_a_long_prompt_tier() {
    let cat = json!({"version": "2026-10-07.1", "models": [
        {"model": "orbit-5-lite", "entry": {"provider": "orbit", "context_window": 1000000,
            "max_output_tokens": 128000, "input_per_mtok": 0.1, "output_per_mtok": 0.5,
            "cache_read_per_mtok": 0.01, "cache_write_per_mtok": 0.125,
            "cache_write_1h_per_mtok": 0.2, "thinking": "adaptive",
            "long_prompt": {"above_tokens": 100000, "input_per_mtok": 0.5, "output_per_mtok": 2.5,
                "cache_read_per_mtok": 0.05, "cache_write_per_mtok": 0.625,
                "cache_write_1h_per_mtok": 1.0}},
         "profiles": ["lite"]}]});
    golden(
        "catalog_tier",
        &run(&["catalog"], vec![step("catalog.list", cat)]),
    );
}

/// The config's `[catalog]` tables, each beside the code's row
/// (theseus-vwar): one that changes a price names it and the code's value,
/// one that adds a model says so, and one that copies the code's figures
/// says it changes nothing.
#[test]
fn catalog_says_what_the_configs_tables_change() {
    let cat = json!({"version": "2026-09-30+config:2", "models": [
        {"model": "glm-x", "entry": {"provider": "zed", "context_window": 200000,
            "max_output_tokens": 16000, "input_per_mtok": 0.6, "output_per_mtok": 2.5,
            "cache_read_per_mtok": 0.11, "cache_write_per_mtok": 0.0, "thinking": "none",
            "source": "config"},
         "profiles": ["quick"],
         "config": {"input_per_mtok": 0.6, "output_per_mtok": 2.5},
         "code": {"provider": "zed", "context_window": 200000, "max_output_tokens": 16000,
            "input_per_mtok": 0.6, "output_per_mtok": 2.2, "cache_read_per_mtok": 0.11,
            "cache_write_per_mtok": 0.0, "thinking": "none", "source": "the zed price page"}},
        {"model": "glm-y", "entry": {"provider": "zed", "context_window": 200000,
            "max_output_tokens": 16000, "input_per_mtok": 0.3, "output_per_mtok": 1.1,
            "cache_read_per_mtok": 0.05, "cache_write_per_mtok": 0.0, "thinking": "none",
            "source": "the zed price page"},
         "profiles": [],
         "config": {"input_per_mtok": 0.3, "output_per_mtok": 1.1, "source": "a price sheet"},
         "code": {"provider": "zed", "context_window": 200000, "max_output_tokens": 16000,
            "input_per_mtok": 0.3, "output_per_mtok": 1.1, "cache_read_per_mtok": 0.05,
            "cache_write_per_mtok": 0.0, "thinking": "none", "source": "the zed price page"}},
        {"model": "orbit-9", "entry": {"provider": "orbit", "context_window": 1000000,
            "max_output_tokens": 64000, "input_per_mtok": 3.0, "output_per_mtok": 15.0,
            "cache_read_per_mtok": 0.3, "cache_write_per_mtok": 3.75, "thinking": "adaptive",
            "source": "config"},
         "profiles": [],
         "config": {"provider": "orbit", "context_window": 1000000, "max_output_tokens": 64000,
            "input_per_mtok": 3.0, "output_per_mtok": 15.0, "cache_read_per_mtok": 0.3,
            "cache_write_per_mtok": 3.75}}]});
    golden(
        "catalog_config",
        &run(&["catalog"], vec![step("catalog.list", cat)]),
    );
}

#[test]
fn sessions_list_open_and_recompile() {
    let list = json!({"sessions": [
        {"session_id": S, "kind": "conversation", "label": null, "title": "Tidy the notes",
         "created_at_unix_ms": 1_759_300_000_000u64, "last_active_ms": 1_759_300_900_000u64,
         "turns": 3, "tool_calls": 4, "cost_usd": 0.0123,
         "usage": {"input_tokens": 9000, "output_tokens": 400},
         "execution_state": "waiting", "model": "glm-x"},
        {"session_id": "ses_b0r1ng", "kind": "conversation", "label": "scratch",
         "created_at_unix_ms": 1_759_200_000_000u64, "turns": 0}]});
    golden(
        "sessions",
        &run(&["sessions"], vec![step("session.list", list.clone())]),
    );
    golden(
        "sessions_json",
        &run(
            &["--json", "sessions", "list"],
            vec![step("session.list", list)],
        ),
    );
    let opened = json!({"session_id": "ses_n3wone"});
    golden(
        "sessions_open",
        &run(
            &["sessions", "open", "--label", "scratch"],
            vec![step("session.open", opened.clone())],
        ),
    );
    golden(
        "sessions_open_json",
        &run(
            &["--json", "sessions", "open"],
            vec![step("session.open", opened)],
        ),
    );
    let recompiled = json!({"session_id": S, "strategy": "transcript"});
    golden(
        "sessions_recompile",
        &run(
            &["sessions", "recompile", S, "--strategy", "transcript"],
            vec![step("session.recompile", recompiled.clone())],
        ),
    );
    golden(
        "sessions_recompile_json",
        &run(
            &["--json", "sessions", "recompile", S],
            vec![step("session.recompile", recompiled)],
        ),
    );
}

/// A daemon that sends `attention` (theseus-in3): each session's pill where
/// its state goes, and each execution's after its state. A task's line is
/// time-dependent, so its pill is `render`'s unit test.
#[test]
fn sessions_and_executions_show_attention() {
    let needs =
        json!({"level": "needs_you", "label": "confirm proc.run: run the gate", "since_ms": 1});
    let ready = json!({"level": "ready", "label": "ready", "since_ms": 1});
    let list = json!({"sessions": [
        {"session_id": S, "kind": "conversation", "label": null, "title": "Tidy the notes",
         "created_at_unix_ms": 1_759_300_000_000u64, "last_active_ms": 1_759_300_900_000u64,
         "turns": 3, "tool_calls": 4, "cost_usd": 0.0123,
         "usage": {"input_tokens": 9000, "output_tokens": 400},
         "execution_state": "waiting", "model": "glm-x", "pending_confirms": 1,
         "attention": needs},
        {"session_id": "ses_b0r1ng", "kind": "conversation", "label": "scratch",
         "created_at_unix_ms": 1_759_200_000_000u64, "turns": 1, "execution_state": "waiting",
         "attention": ready}]});
    golden(
        "sessions_attention",
        &run(&["sessions"], vec![step("session.list", list)]),
    );
    let mut e = execution(X, "waiting", None);
    e["attention"] = needs;
    e["waiting_on"] = json!({"on": "confirm", "confirm_id": "act_k2"});
    golden(
        "executions_attention",
        &run(
            &["executions"],
            vec![step("execution.list", json!({"executions": [e]}))],
        ),
    );
}

#[test]
fn executions_list_and_cancel() {
    let list = json!({"executions": [execution(X, "waiting", None),
        execution("exe_d0n3", "ended", Some("cancelled by the CLI"))]});
    golden(
        "executions",
        &run(&["executions"], vec![step("execution.list", list.clone())]),
    );
    golden(
        "executions_json",
        &run(
            &["--json", "executions", "list"],
            vec![step("execution.list", list)],
        ),
    );
    golden(
        "executions_empty",
        &run(
            &["executions"],
            vec![step("execution.list", json!({"executions": []}))],
        ),
    );
    let cancelled = json!({"execution": execution(X, "ended", Some("cancelled by the CLI")),
        "cancelled_actions": ["act_k2", "act_k3"]});
    golden(
        "executions_cancel",
        &run(
            &["executions", "cancel", X],
            vec![step("execution.cancel", cancelled.clone())],
        ),
    );
    golden(
        "executions_cancel_json",
        &run(
            &["--json", "executions", "cancel", X],
            vec![step("execution.cancel", cancelled)],
        ),
    );
}

#[test]
fn stop_names_the_session_and_says_what_goes_on() {
    let list = json!({"executions": [execution(X, "running", None)]});
    let stopped = json!({"execution": execution(X, "waiting", None), "stopped": true,
        "stopped_actions": ["act_k2"], "declined": ["act_k4"], "turn_running": true,
        "tasks_running": 1, "wakes_pending": 0});
    golden(
        "stop",
        &run(
            &["stop", "f3k2"],
            vec![
                step("execution.list", list.clone()),
                step("execution.stop", stopped.clone()),
            ],
        ),
    );
    golden(
        "stop_json",
        &run(
            &["--json", "stop", S],
            vec![
                step("execution.list", list.clone()),
                step("execution.stop", stopped),
            ],
        ),
    );
    golden(
        "stop_unknown",
        &run(&["stop", "zzzz"], vec![step("execution.list", list)]),
    );
}

#[test]
fn tasks_and_wakes_when_there_are_none() {
    golden(
        "tasks_none",
        &run(&["tasks"], vec![step("task.list", json!({"tasks": []}))]),
    );
    golden(
        "tasks_json",
        &run(
            &["--json", "tasks", "-s", S],
            vec![step("task.list", json!({"tasks": []}))],
        ),
    );
    golden(
        "wakes_none",
        &run(&["wakes"], vec![step("wake.list", json!({"wakes": []}))]),
    );
    golden(
        "wakes_json",
        &run(
            &["--json", "wakes", "-s", S],
            vec![step("wake.list", json!({"wakes": []}))],
        ),
    );
}

#[test]
fn cancel_a_wake_then_a_task_then_neither() {
    let wake = json!({"wake": {"wake_id": "wak_9a8b7c", "short": "9a8b7c", "session_id": S,
        "execution_id": X, "due_at_ms": 1_759_303_600_000u64, "due_local": "15:00",
        "note": "check the build", "set_at_ms": 1_759_300_000_000u64, "state": "cancelled"}});
    golden(
        "cancel_wake",
        &run(
            &["cancel", "9a8b7c"],
            vec![step("wake.cancel", wake.clone())],
        ),
    );
    golden(
        "cancel_wake_json",
        &run(
            &["--json", "cancel", "9a8b7c"],
            vec![step("wake.cancel", wake)],
        ),
    );
    let no_wake = Step {
        method: "wake.cancel",
        before: vec![],
        answer: Err(json!({"code": -32002, "message": "no pending wake is named `t4sk01`"})),
        after: vec![],
    };
    let task = json!({"task": {"task_id": "ses_t4sk01", "short": "t4sk01",
        "execution_id": "exe_t4sk01", "parent_session_id": S, "parent_execution_id": X,
        "title": "Index the notes", "state": "ended", "spent_usd": 0.04, "limit_usd": 1.0,
        "cost_usd": 0.04, "turns": 2, "ended_reason": "cancelled by the CLI",
        "created_at_ms": 1_759_300_000_000u64, "updated_at_ms": 1_759_300_100_000u64},
        "cancelled_actions": ["act_j1"]});
    golden(
        "cancel_task",
        &run(
            &["cancel", "t4sk01"],
            vec![no_wake, step("task.cancel", task)],
        ),
    );
    let neither = |m: &str| Step {
        method: if m == "wake" {
            "wake.cancel"
        } else {
            "task.cancel"
        },
        before: vec![],
        answer: Err(json!({"code": -32002, "message": if m == "wake" {
            "no pending wake is named `qq`".to_string() } else {
            "`qq` is too short: name at least four characters".to_string() }})),
        after: vec![],
    };
    golden(
        "cancel_neither",
        &run(&["cancel", "qq"], vec![neither("wake"), neither("task")]),
    );
}

#[test]
fn profile_list_and_use() {
    let list = json!({"live": "quick", "live_source": "the state dir", "profiles": [
        {"name": "quick", "provider": "zed", "model": "glm-x", "max_output_tokens": 8000,
         "has_system": true, "live": true},
        {"name": "deep", "provider": "orbit", "model": "orbit-5", "max_output_tokens": 32000,
         "has_system": false, "live": false}]});
    golden(
        "profile",
        &run(&["profile"], vec![step("profile.list", list.clone())]),
    );
    golden(
        "profile_json",
        &run(
            &["--json", "profile", "list"],
            vec![step("profile.list", list)],
        ),
    );
    let used = json!({"live": "deep", "previous": "quick"});
    golden(
        "profile_use",
        &run(
            &["profile", "use", "deep"],
            vec![step("profile.use", used.clone())],
        ),
    );
    golden(
        "profile_use_json",
        &run(
            &["--json", "profile", "use", "deep"],
            vec![step("profile.use", used)],
        ),
    );
}

#[test]
fn ledger_prints_rows_and_the_total() {
    let rows = json!({"total": 2048, "rows": [
        {"position": 2040, "at_unix_ms": 1_759_300_000_123u64, "kind": "turn.started",
         "session_id": S, "turn_id": T, "data": {"profile": "quick", "input_chars": 15}},
        {"position": 2041, "at_unix_ms": 1_759_300_003_456u64, "kind": "provider.call",
         "session_id": S, "turn_id": null, "data": {"note": "x".repeat(300)}}]});
    golden(
        "ledger",
        &run(
            &["ledger", "-n", "2"],
            vec![step("ledger.tail", rows.clone())],
        ),
    );
    golden(
        "ledger_json",
        &run(
            &["--json", "ledger", "-k", "turn.started", "-s", S],
            vec![step("ledger.tail", rows)],
        ),
    );
}

#[test]
fn rpc_prints_the_result_and_echoes_notifications() {
    let mut s = step(
        "turn.submit",
        json!({"session_id": S, "b": [1, 2], "a": {"z": 1, "y": null}}),
    );
    s.before = vec![note(
        "model.delta",
        json!({"turn_id": T, "loop_index": 0, "text": "hi"}),
    )];
    golden(
        "rpc",
        &run(&["rpc", "turn.submit", r#"{"input":"hi"}"#], vec![s]),
    );
    let mut s = step("health", json!({"ok": true}));
    s.before = vec![note("model.delta", json!({"text": "quiet"}))];
    golden("rpc_json", &run(&["--json", "rpc", "health"], vec![s]));
    golden(
        "rpc_bad_params",
        &run(&["rpc", "health", "{not json"], vec![]),
    );
}

#[test]
fn shutdown_prints_nothing_but_json_when_asked() {
    golden(
        "shutdown",
        &run(
            &["shutdown"],
            vec![step("shutdown", json!({"stopping": true}))],
        ),
    );
    golden(
        "shutdown_json",
        &run(
            &["--json", "shutdown"],
            vec![step("shutdown", json!({"stopping": true}))],
        ),
    );
}

/// Inside a Theseus job the CLI refuses the operator's commands before it
/// sends anything (theseus-zmgb): an answer and the undo of a tightening,
/// each saying why and that the operator runs it from their own shell. The
/// listing of what waits, which only reads, goes.
#[test]
fn inside_a_job_the_operators_commands_are_refused_and_nothing_is_sent() {
    let job = Some("ses_job01");
    golden(
        "confirm_inside_a_job",
        &run_in(&["confirm", "--approve", "act_k4"], vec![], job),
    );
    golden(
        "untighten_inside_a_job",
        &run_in(&["policy", "untighten", "fs.edit"], vec![], job),
    );
    let listed = run_in(
        &["confirm"],
        vec![step("confirm.list", json!({"confirms": []}))],
        job,
    );
    assert!(listed.contains("--- requests\nconfirm.list\n"), "{listed}");
}

#[test]
fn an_error_answer_exits_1_with_its_class() {
    let s = Step {
        method: "session.list",
        before: vec![],
        answer: Err(
            json!({"code": -32003, "message": "the secret anthropic_api_key did not resolve",
            "data": {"class": "secret_failed"}}),
        ),
        after: vec![],
    };
    golden("error_class", &run(&["sessions"], vec![s]));
}

/// `theseus reach` by a node's short id (theseus-glyw): the daemon resolves
/// it, and the answer prints as the whole id's; an end that names two nodes
/// is refused in the daemon's words.
#[test]
fn reach_takes_a_short_id_and_says_when_it_names_two() {
    let result = json!({
        "node_id": "msg_0198d3a0b1c2d3e4f5a6b7c8d9e0f1a2", "session_id": S, "position": 120,
        "direct": {"compilations": [], "loops": 1, "first_ms": 1_759_300_000_001u64,
                   "last_ms": 1_759_300_000_001u64},
        "descendants": [], "totals": {"contexts": 1, "sessions": 1}, "partial": false});
    golden(
        "reach_short",
        &run(&["reach", "msg·e0f1a2"], vec![step("node.reach", result)]),
    );
    let two = Step {
        method: "node.reach",
        before: vec![],
        answer: Err(json!({"code": -32602, "message":
            "`7c41ab` names 2 nodes (msg_0198d3a0b1c2d3e4f5a6b7c8d97c41ab in ses·q7f3k2, \
             tcl_0198d3a0ffffffffffffffffff7c41ab in ses·b2c9w1): give more of its id"})),
        after: vec![],
    };
    golden("reach_ambiguous", &run(&["reach", "7c41ab"], vec![two]));
}

/// `theseus reach` (theseus-n4m, step 12a): a task's last message, which no
/// context of its own session held, relayed into the parent by its report,
/// where one compilation and two loops held the copy. Then `--json`, a walk
/// that stopped short, and a node the daemon does not know.
#[test]
fn reach_prints_each_generation_and_what_held_it() {
    let result = json!({
        "node_id": "msg_task_last", "session_id": "ses_task01", "position": 120,
        "direct": {"compilations": [], "loops": 0},
        "descendants": [{
            "node_id": "msg_relayed", "session_id": S, "position": 140, "generation": 1,
            "via": "derived_from", "route": "report", "from": "msg_task_last",
            "compilations": [{"compilation_id": "cmp_r7", "strategy": "transcript",
                              "created_at_ms": 1_759_300_000_123u64}],
            "loops": 2, "first_ms": 1_759_300_000_123u64, "last_ms": 1_759_300_061_456u64}],
        "totals": {"contexts": 3, "sessions": 2}, "partial": false});
    golden(
        "reach",
        &run(
            &["reach", "msg_task_last"],
            vec![step("node.reach", result.clone())],
        ),
    );
    golden(
        "reach_json",
        &run(
            &["--json", "reach", "msg_task_last"],
            vec![step("node.reach", result)],
        ),
    );
    let cut = json!({
        "node_id": "msg_task_last", "session_id": "ses_task01", "position": 120,
        "direct": {"compilations": [], "loops": 1, "first_ms": 1_759_300_000_001u64,
                   "last_ms": 1_759_300_000_001u64},
        "descendants": [], "totals": {"contexts": 1, "sessions": 1}, "partial": true});
    golden(
        "reach_partial",
        &run(
            &["reach", "msg_task_last", "--generations", "0"],
            vec![step("node.reach", cut)],
        ),
    );
    let unknown = Step {
        method: "node.reach",
        before: vec![],
        answer: Err(json!({"code": -32002, "message": "no node \"msg_gone\""})),
        after: vec![],
    };
    golden("reach_unknown", &run(&["reach", "msg_gone"], vec![unknown]));
}

/// `theseus budgets` (step 42a): a conversation at its limit's question,
/// reset once, with two tasks under it and their carves; a place's limit;
/// the totals; and the judge's day.
#[test]
fn budgets_prints_each_task_under_its_parent_and_the_totals() {
    let task = |id: &str, limit: f64, spent: f64, held: f64| {
        json!({"execution_id": format!("exe_{id}"), "session_id": format!("ses_{id}"),
               "kind": "task", "state": "running", "limit_usd": limit, "limit_from": "carve",
               "limit_by": S, "spent_usd": spent, "reserved_usd": 0.0, "held_unknown_usd": 0.0,
               "available_usd": limit - spent, "lifetime_usd": spent, "resets": 0,
               "parent": X, "carve_held_usd": held})
    };
    let result = json!({
        "executions": [
            {"execution_id": X, "session_id": S, "kind": "conversation", "state": "waiting",
             "title": "the lighthouse", "limit_usd": 100.0, "limit_from": "config",
             "spent_usd": 3.25, "reserved_usd": 4.5, "held_unknown_usd": 0.0,
             "available_usd": 92.25, "lifetime_usd": 7.5, "resets": 1,
             "last_reset": {"at_ms": 1_759_300_300_000u64, "by": "cli", "spent_before_usd": 100.0},
             "question": {"correlation_id": "act_q7f3k2", "needs_usd": 0.25},
             "tasks": [task("lamp01", 3.0, 0.5, 2.5), task("lens02", 2.0, 0.0, 2.0)]},
            {"execution_id": "exe_pier01", "session_id": "ses_pier01", "kind": "conversation",
             "state": "waiting", "limit_usd": 40.0, "limit_from": "place", "limit_by": "#pier",
             "spent_usd": 1.0, "reserved_usd": 0.0, "held_unknown_usd": 0.0,
             "available_usd": 39.0, "lifetime_usd": 1.0, "resets": 2,
             "last_reset_unread": "the ledger's index is being built after the start; ask again in a moment"}
        ],
        "totals": {"executions": 2, "tasks": 2, "limit_usd": 140.0, "spent_usd": 4.25,
                   "reserved_usd": 4.5, "held_unknown_usd": 0.0, "available_usd": 131.25,
                   "lifetime_usd": 9.0, "questions": 1},
        "config_limit_usd": 100.0,
        "judge": {"enabled": true, "day": "2026-10-04", "limit_usd": 1.0, "spent_usd": 0.0125,
                  "paused": false}
    });
    golden(
        "budgets",
        &run(&["budgets"], vec![step("budget.list", result.clone())]),
    );
    golden(
        "budgets_json",
        &run(&["--json", "budgets"], vec![step("budget.list", result)]),
    );
    let none = json!({"executions": [], "totals": {"executions": 0, "tasks": 0, "limit_usd": 0.0,
        "spent_usd": 0.0, "reserved_usd": 0.0, "held_unknown_usd": 0.0, "available_usd": 0.0,
        "lifetime_usd": 0.0, "questions": 0}, "config_limit_usd": 100.0,
        "judge": {"enabled": false, "day": "2026-10-04", "limit_usd": 1.0, "spent_usd": 0.0,
                  "paused": false}});
    golden(
        "budgets_none",
        &run(&["budgets"], vec![step("budget.list", none)]),
    );
}

/// `theseus policy explain` (step 42a): each place's tools on a line, what
/// raised each, a refused one's words; with `--tool`, every layer, the
/// conditions, and the gate's reason.
#[test]
fn policy_explain_prints_each_place_and_a_tool_in_full() {
    let layer = |layer: &str, says: &str, setting: Option<&str>, result: &str, raised: bool| {
        let mut l = json!({"layer": layer, "says": says, "result": result});
        if let Some(s) = setting {
            l["setting"] = json!(s);
        }
        if raised {
            l["raised"] = json!(true);
        }
        l
    };
    let proc_lab = json!({"tool": "proc.run", "class": "run", "offered": true,
        "layers": [
            layer("place", "a private place: every tool is offered", None, "offered", false),
            layer("ceiling", "#lab's ceiling narrows no tools", None, "offered", false),
            layer("class", "L0: it runs on the host, under the floor, the lists, and its posture",
                  Some("[sandbox] default and l1_argv"), "l0", false),
            layer("posture", "the config's posture for the tool", Some("enforcement = notify"),
                  "notify", false),
            layer("tightening", "tightened by cli (\"should have asked\"): it asks first until undone",
                  Some("tightened by cli"), "approve", true),
            layer("grant", "no secret is granted to a call like this", None, "approve", false),
            layer("floor", "#lab's ceiling sets a floor of approve", Some("#lab's posture_floor"),
                  "approve", false),
            layer("hold", "this session holds no external text",
                  Some("[policy] external_text = ask"), "approve", false)],
        "conditions": [
            {"layer": "floor", "when": "it touches Theseus's own binary or state, or the 1Password CLI or its token: at every posture",
             "entries": ["/w/state/store", "theseusd", "op"], "then": "approve"},
            {"layer": "allow_argv", "when": "its argv starts with an entry of the allow list, and every path argument is inside the roots",
             "entries": ["ls", "pwd"], "then": "open"}],
        "result": "approve",
        "reason": "a call of proc.run: proc.run — approve (tightened by cli; the config says enforcement = notify)"});
    let read = json!({"tool": "fs.read", "class": "read", "offered": true,
        "layers": [
            layer("place", "a private place: every tool is offered", None, "offered", false),
            layer("posture", "the config's posture for the tool",
                  Some("[policy.tools] \"fs.read\" = open"), "open", false),
            layer("hold", "a read (or a one-shot wake) keeps its posture after external text",
                  Some("[policy] external_text = ask"), "open", false)],
        "result": "open", "reason": "fs.read — open ([policy.tools] \"fs.read\" = open)"});
    let proc_held = json!({"tool": "proc.run", "class": "run", "offered": true,
        "layers": [
            layer("posture", "the config's posture for the tool", Some("enforcement = notify"),
                  "notify", false),
            layer("hold", "this session read external text (http.fetch tides.example/today), and a call that acts waits for approval after that (§3.9)",
                  Some("[policy] external_text = ask"), "approve", true)],
        "result": "approve",
        "reason": "a call of proc.run: proc.run — approve (this session read external text)"});
    let refused = json!({"tool": "proc.run", "class": "run", "offered": false,
        "refused": "proc.run is not offered in a shared place: one others can read gets only the public tools",
        "layers": [layer("place", "a shared place: only the public tools, and the file tools under the public paths",
                         Some("the bindings file's class, and [places] public_paths"), "refused", false)],
        "result": "refused",
        "reason": "place: proc.run is not offered in a shared place: one others can read gets only the public tools"});
    let every = json!({"places": [
        {"place": "cli", "name": "CLI", "class": "private", "tools": [read, proc_held]},
        {"place": "discord:channel:314159", "name": "#lab", "class": "private",
         "ceiling": {"posture_floor": "approve"}, "session_id": S, "tools": [proc_lab]},
        {"place": "discord:channel:141421", "name": "#pier", "class": "shared",
         "tools": [refused]}],
        "roots": ["/w"]});
    golden(
        "policy_explain",
        &run(&["policy", "explain"], vec![step("policy.explain", every)]),
    );
    let one = json!({"places": [
        {"place": "discord:channel:314159", "name": "#lab", "class": "private",
         "ceiling": {"posture_floor": "approve"}, "session_id": S, "tools": [proc_lab]}],
        "roots": ["/w"]});
    golden(
        "policy_explain_tool",
        &run(
            &["policy", "explain", "--session", S, "--tool", "proc.run"],
            vec![step("policy.explain", one.clone())],
        ),
    );
    golden(
        "policy_explain_json",
        &run(
            &["--json", "policy", "explain", "--tool", "proc.run"],
            vec![step("policy.explain", one)],
        ),
    );
}
