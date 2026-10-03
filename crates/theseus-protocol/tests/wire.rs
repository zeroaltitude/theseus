//! The notifications' bytes on the wire (theseus-0g4, finding 12). Each case is
//! one notification as the daemon sends it, with fixed values: its line is
//! compared with `tests/wire/<case>.json`, byte for byte.
//!
//! The fixtures were captured from the senders as they stood at 2ae535d. A
//! payload the sender built with `json!` is built here with the same `json!`
//! (the sender's file is named on each), so the fixture is that expression's
//! output. `THESEUS_GOLDEN=write` rewrites the fixtures; a change that means to
//! change the wire does that, and says so.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Value};
use theseus_protocol::*;

const S: &str = "ses_q7f3k2";
const T: &str = "turn_m4p8z1";
const X: &str = "exe_q7f3k2";

/// The line the daemon writes for this notification.
fn line(method: &str, params: impl Serialize) -> String {
    serde_json::to_string(&Message::Notification(Notification::new(method, params))).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "wire",
        &format!("{name}.json"),
    ]
    .iter()
    .collect()
}

/// Compare with the fixture, or write it under `THESEUS_GOLDEN=write`.
fn wire(name: &str, got: &str) {
    let path = fixture(name);
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{got}\n")).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (write it with THESEUS_GOLDEN=write)",
            path.display()
        )
    });
    assert_eq!(
        got,
        want.trim_end_matches('\n'),
        "{name}: the bytes on the wire changed"
    );
}

// ------------------------------------------------- values the senders hold

fn external() -> Value {
    json!({"since_ms": 1_759_300_000_000u64, "tool": "http.fetch", "url": "notes.example/today",
           "node_id": "nod_e1", "via": "fs.read"})
}

fn plan(argv: bool) -> Value {
    let mut p =
        json!({"resources": [{"path": "/w/notes", "access": "read"}], "summary": "list notes"});
    if argv {
        p["argv"] = json!(["make", "notes"]);
        p["resources"] = json!([{"path": "/w/notes", "access": "exec"}]);
        p["summary"] = json!("run make notes");
    }
    p
}

fn proposal(tool: &str) -> Value {
    json!({"tool": tool, "args": {"path": "notes"}, "resource": "/w/notes",
           "policy_context": {}})
}

// ----------------------------------------- toolrun.rs: the gate's record

/// `toolrun.rs`, `gate()`: the record a tool-call node keeps and
/// `tool.proposed` shows.
fn gate_record(
    result: Value,
    decision: Option<Value>,
    plan: Option<Value>,
    proposal: Value,
) -> Value {
    json!({
        "result": result,
        "validated": decision.is_some(),
        "decision": decision,
        "plan": plan,
        "proposal": proposal,
    })
}

fn proposed(tool: &str, gate: Value) -> String {
    line(
        notify::TOOL_PROPOSED,
        json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_1", "tool": tool, "input": {"path": "notes"}, "gate": gate}),
    )
}

#[test]
fn tool_proposed() {
    wire(
        "tool_proposed_allow",
        &proposed(
            "fs.list",
            gate_record(
                json!({"gate": "allow"}),
                Some(json!({"posture": "open", "reason": "fs.list — open (policy.tools)"})),
                Some(plan(false)),
                proposal("fs.list"),
            ),
        ),
    );
    wire(
        "tool_proposed_notify",
        &proposed(
            "proc.run",
            gate_record(
                json!({"gate": "allow"}),
                Some(
                    json!({"posture": "notify", "reason": "proc.run — notify (enforcement = notify)",
                    "notify": {"kind": "notify", "setting": "enforcement = notify",
                               "rule": "proc.run — notify (enforcement = notify)"},
                    "granted": "gh got GH_TOKEN"}),
                ),
                Some(plan(true)),
                proposal("proc.run"),
            ),
        ),
    );
    wire(
        "tool_proposed_confirm",
        &proposed(
            "fs.write",
            gate_record(
                json!({"gate": "needs_confirm", "by": "operator"}),
                Some(
                    json!({"posture": "approve", "reason": "write outside the roots: fs.write — approve",
                    "floor": true, "external": external()}),
                ),
                Some(plan(false)),
                proposal("fs.write"),
            ),
        ),
    );
    wire(
        "tool_proposed_invalid",
        &proposed(
            "fs.read",
            gate_record(
                json!({"gate": "deny", "reason": "validation: missing field `path`"}),
                None,
                None,
                proposal("fs.read"),
            ),
        ),
    );
}

// ------------------------------------------------------ turn.rs, loops

#[test]
fn turn_and_loop() {
    wire(
        "turn_started",
        &line(
            notify::TURN_STARTED,
            TurnStarted {
                session_id: S.into(),
                turn_id: T.into(),
                execution_id: Some(X.into()),
                continuation: false,
            },
        ),
    );
    wire(
        "loop_started",
        &line(
            notify::LOOP_STARTED,
            LoopStarted {
                turn_id: T.into(),
                loop_index: 0,
                model: "glm-x".into(),
                tools_offered: 14,
            },
        ),
    );
    wire(
        "model_delta",
        &line(
            notify::MODEL_DELTA,
            ModelDelta {
                turn_id: T.into(),
                loop_index: 1,
                text: "Two files:\n- a.md".into(),
            },
        ),
    );
    // turn.rs, the provider's thinking deltas.
    let (tid, i, text) = (T, 1u32, "Look first.\n");
    wire(
        "model_thinking",
        &line(
            notify::MODEL_THINKING,
            json!({"turn_id": tid, "loop_index": i, "text": text}),
        ),
    );
    for (name, provider_stop_reason) in [
        ("loop_ended", Some("tool_use")),
        ("loop_ended_budget", None),
    ] {
        wire(
            name,
            &line(
                notify::LOOP_ENDED,
                LoopEnded {
                    turn_id: T.into(),
                    loop_index: 0,
                    provider_stop_reason: provider_stop_reason.map(Into::into),
                    tool_calls: 2,
                    advancer: "until_no_tool_calls".into(),
                    decision: "continue".into(),
                },
            ),
        );
    }
    let ended: TurnSubmitResult = serde_json::from_value(json!({
        "session_id": S, "turn_id": T, "loops": 2, "output": "Two files.",
        "stop_reason": "end_turn", "provider_stop_reason": "end_turn", "model": "glm-x",
        "provider": "zed", "profile": "quick",
        "usage": {"input_tokens": 5230, "output_tokens": 212, "cache_read_input_tokens": 4096,
                  "cache_creation_input_tokens": 1024, "cache_creation_1h_input_tokens": 1024},
        "elapsed_ms": 3021, "first_token_ms": 640, "request_id": "req_a1",
        "trace": {"name": "turn", "kind": "turn", "start_us": 0, "end_us": 3_021_000,
                  "children": [{"name": "loop 0", "kind": "loop", "start_us": 10, "end_us": 99,
                                "attrs": {"model": "glm-x"}}]},
        "execution_id": X, "cost_usd": 0.0031, "tool_calls": 3, "awaiting_confirm": "act_k4",
        "stop_details": {"category": "none"}, "continuation": true
    }))
    .unwrap();
    wire("turn_ended", &line(notify::TURN_ENDED, &ended));
    for (name, class, then) in [
        ("turn_failed_backoff", Some("rate_limited"), Some("backoff")),
        ("turn_failed_plain", None, None),
    ] {
        wire(
            name,
            &line(
                notify::TURN_FAILED,
                TurnFailed {
                    session_id: S.into(),
                    turn_id: class.map(|_| T.into()),
                    execution_id: Some(X.into()),
                    continuation: class.is_none(),
                    class: class.map(Into::into),
                    error: "429 Too Many Requests".into(),
                    then: then.map(Into::into),
                },
            ),
        );
    }
}

// --------------------------------------------- turn.rs, the compile step

/// `turn.rs`, the loop's compile: the summary the ledger, the trace, and
/// `context.compiled` share.
#[test]
fn context_compiled() {
    let summary = |recompile: bool| {
        let mut summary = json!({
            "session_id": S,
            "turn_id": T,
            "loop": 1u32,
            "decision": if recompile { "recompile" } else { "append" },
            "trigger": if recompile { Some("window") } else { None },
            "compilation_id": "cmp_b2",
            "strategy": "fresh",
            "prefix_nodes": 2usize,
            "tail_nodes": 6usize,
            "messages": 7usize,
            "est_tokens": 4210u64,
            "digest": "9f2c1a0b7d3e4f51",
            "repairs": if recompile { vec!["tu_lost1".to_string()] } else { vec![] },
            "tools": 14usize,
            "nodes_scanned": 11usize,
        });
        if recompile {
            summary["context_files"] = json!([
                {"path": "/w/AGENTS.md", "digest": "a1b2c3d4e5f60718", "bytes": 2048,
                 "public": true},
                {"path": "/w/persona/clerk.md", "bytes": 65536, "cut": true, "persona": "clerk",
                 "digest": "0f1e2d3c4b5a6978"},
                {"path": "/w/missing.md", "bytes": 0, "missing": "not found"},
                {"path": "/w/notes.md", "bytes": 0, "withheld": "not public, in a shared place"}
            ]);
            summary["persona"] = json!("clerk");
            // The place rule: a shared place's compile, and the file it withheld.
            summary["class"] = json!("shared");
            summary["withheld"] = json!(1);
        }
        summary["cache"] = json!({
            "breakpoints": if recompile { vec!["header", "conversation"] } else { vec![] },
            "ttl": "1h",
            "conversation_ttl": "5m",
        });
        summary
    };
    wire(
        "context_compiled_recompile",
        &line(notify::CONTEXT_COMPILED, summary(true)),
    );
    wire(
        "context_compiled_append",
        &line(notify::CONTEXT_COMPILED, summary(false)),
    );
}

// ---------------------------------------------------- toolrun.rs, calls

#[test]
fn tool_started_and_ended() {
    // `run_harness` and `run_inproc`.
    for (name, backend) in [
        ("tool_started_harness", "harness"),
        ("tool_started_inproc", "inproc"),
    ] {
        wire(
            name,
            &line(
                notify::TOOL_STARTED,
                json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_3", "tool": "fs.read", "correlation_id": "act_k3", "backend": backend}),
            ),
        );
    }
    // A job's start: its pid, argv, and directory, and what the broker gave it.
    for (name, granted, withheld) in [
        ("tool_started_job", None, vec![]),
        (
            "tool_started_job_granted",
            Some("gh got GH_TOKEN"),
            vec!["git got no GIT_TOKEN".to_string()],
        ),
    ] {
        let (pid, argv, cwd) = (4242u32, vec!["make", "notes"], PathBuf::from("/w/notes"));
        wire(
            name,
            &line(
                notify::TOOL_STARTED,
                json!({"session_id": S, "turn_id": T, "tool_use_id": "tu_2", "tool": "proc.run", "correlation_id": "act_k2", "backend": "job", "pid": pid, "argv": argv, "cwd": cwd, "granted": granted, "withheld": withheld}),
            ),
        );
    }
    // `announce_end`: a result node's line.
    for (name, correlation_id, exit_code, stopped_by, late) in [
        ("tool_ended_ok", Some("act_k2"), Some(json!(0)), None, false),
        (
            "tool_ended_stopped",
            Some("act_k3"),
            None,
            Some(json!("the CLI")),
            true,
        ),
        ("tool_ended_unknown_tool", None, None, None, false),
    ] {
        let mut meta = json!({});
        if let Some(c) = exit_code {
            meta["exit_code"] = c;
        }
        if let Some(by) = stopped_by {
            meta["stopped_by"] = by;
        }
        let content = "a.md\nb.md\n".repeat(300);
        wire(
            name,
            &line(
                notify::TOOL_ENDED,
                json!({
                    "session_id": S,
                    "turn_id": T,
                    "tool_use_id": "tu_2",
                    "tool": "proc.run",
                    "status": "ok",
                    "duration_ms": Some(1520u64),
                    "correlation_id": correlation_id,
                    "late": late,
                    "truncated": true,
                    "bytes": 70_000u64,
                    "node_id": "nod_r2",
                    "exit_code": meta.get("exit_code"),
                    "stopped_by": meta.get("stopped_by"),
                    "preview": content.chars().take(2000).collect::<String>(),
                }),
            ),
        );
    }
    // `node_written`.
    wire(
        "node_written",
        &line(
            notify::NODE_WRITTEN,
            json!({"session_id": S, "node_id": "nod_r2", "kind": "tool_result"}),
        ),
    );
}

/// `toolrun.rs`, `notified`: a `notify` posture's notice.
#[test]
fn policy_notified() {
    for (name, granted, task) in [
        ("policy_notified", None, None),
        (
            "policy_notified_task",
            Some("gh got GH_TOKEN"),
            Some("t4sk01"),
        ),
    ] {
        let mut v = json!({"session_id": S, "turn_id": T,
            "tool_use_id": "tu_2", "correlation_id": "act_k2", "tool": "proc.run",
            "input": {"argv": ["make", "notes"]}, "summary": "run make notes", "kind": "notify",
            "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)", "granted": granted});
        if let Some(t) = task {
            v["task"] = json!(t);
        }
        wire(name, &line(notify::POLICY_NOTIFIED, v));
    }
}

// --------------------------------------------------- confirmations

fn confirm(budget: bool) -> ConfirmRequest {
    serde_json::from_value(if budget {
        json!({"correlation_id": "act_b5", "session_id": S, "execution_id": X,
               "tool": "budget.reset", "input": {}, "reason": "The session reached its $1.00 limit",
               "by": "operator", "requested_at_ms": 1_759_300_200_000u64, "expires_at_ms": 0,
               "budget": {"spent_usd": 1.0, "limit_usd": 1.0, "needed_usd": 0.02,
                          "lifetime_usd": 3.5},
               "task": {"task_id": "ses_t4sk01", "short": "t4sk01", "title": "Index the notes"}})
    } else {
        json!({"correlation_id": "act_k4", "session_id": S, "execution_id": X,
               "tool": "fs.write", "input": {"path": "/srv/motd", "text": "hello"},
               "resource": "/srv/motd", "reason": "write outside the roots (/w)",
               "by": "operator", "requested_at_ms": 1_759_300_100_000u64,
               "expires_at_ms": 1_759_301_000_000u64, "floor": true,
               "external_text": external()})
    })
    .unwrap()
}

#[test]
fn confirm_requested_and_resolved() {
    wire(
        "confirm_requested_tool",
        &line(notify::CONFIRM_REQUESTED, confirm(false)),
    );
    wire(
        "confirm_requested_budget",
        &line(notify::CONFIRM_REQUESTED, confirm(true)),
    );
    let (by, corr) = ("the CLI", "act_k4");
    // rpc/confirms.rs: an answer to a tool call, and one to a budget question.
    wire(
        "confirm_resolved_answer",
        &line(
            notify::CONFIRM_RESOLVED,
            json!({"session_id": S, "correlation_id": corr, "approved": true, "by": by, "trust": true}),
        ),
    );
    wire(
        "confirm_resolved_budget",
        &line(
            notify::CONFIRM_RESOLVED,
            json!({"session_id": S, "correlation_id": "act_b5", "approved": false, "by": "discord:ana"}),
        ),
    );
    // rpc/confirms.rs: a raised limit withdrew the budget question.
    wire(
        "confirm_resolved_withdrawn",
        &line(
            notify::CONFIRM_RESOLVED,
            json!({"session_id": S, "correlation_id": "act_b5", "approved": false, "withdrawn": true, "by": "config"}),
        ),
    );
    // rpc/driver.rs: a cancel, and a stop.
    wire(
        "confirm_resolved_cancelled",
        &line(
            notify::CONFIRM_RESOLVED,
            json!({"session_id": S, "correlation_id": corr, "approved": false, "cancelled": true, "by": by}),
        ),
    );
    wire(
        "confirm_resolved_stopped",
        &line(
            notify::CONFIRM_RESOLVED,
            json!({"session_id": S, "correlation_id": corr, "approved": false, "stopped": true, "by": by}),
        ),
    );
    // turn.rs: new input superseded the budget question; toolrun.rs: a call's.
    wire(
        "confirm_resolved_superseded",
        &line(
            notify::CONFIRM_RESOLVED,
            json!({"session_id": S, "correlation_id": "act_b5", "approved": false, "superseded": true, "by": "the web UI"}),
        ),
    );
    wire(
        "confirm_resolved_superseded_call",
        &line(
            notify::CONFIRM_RESOLVED,
            json!({"session_id": S, "correlation_id": corr, "approved": false, "superseded": true}),
        ),
    );
}

// ---------------------------------------------- policy, trust, refusals

fn tighten_result(changed: bool) -> TightenResult {
    TightenResult {
        tool: "proc.run".into(),
        by: "the CLI".into(),
        tightening: Tightening {
            tool: "proc.run".into(),
            posture: "approve".into(),
            by: "the CLI".into(),
            who: "sock#3".into(),
            via: "cli".into(),
            at_ms: 1_759_300_300_250,
            correlation_id: Some("act_k2".into()),
            session_id: Some(S.into()),
            digest: Some("d1g35t".into()),
        },
        posture: "approve".into(),
        setting: "tightened by the CLI".into(),
        config_posture: "notify".into(),
        config_setting: "enforcement = notify".into(),
        changed,
        already: !changed,
    }
}

#[test]
fn policy_trust_and_refusals() {
    wire(
        "policy_tightened",
        &line(notify::POLICY_TIGHTENED, tighten_result(true)),
    );
    wire(
        "policy_untightened",
        &line(notify::POLICY_UNTIGHTENED, tighten_result(false)),
    );
    let trusted: TrustResult = serde_json::from_value(json!({"session_id": S, "by": "the CLI",
        "who": "sock#3", "via": "cli", "how": "approval", "correlation_id": "act_k4",
        "at_ms": 1_759_300_400_000u64, "held": external(), "since_local": "14:02"}))
    .unwrap();
    wire("session_trusted", &line(notify::SESSION_TRUSTED, trusted));
    wire(
        "profile_changed",
        &line(
            notify::PROFILE_CHANGED,
            ProfileChanged {
                previous: "quick".into(),
                live: "deep".into(),
                by: "the CLI".into(),
            },
        ),
    );
    // rpc/confirms.rs, `refused` and `refused_from_job`: the ledger row's
    // fields, then the act and the session. peer.rs, `Traced::json`: the asker.
    let (who, via, why, by) = (
        "sock#9",
        "cli",
        "from a Theseus job's process (job act_j1, pid 4300, theseus)",
        "the CLI",
    );
    let answer = {
        let mut data = json!({"correlation_id": "act_k4", "tool": "fs.write", "approve": true,
                       "who": who, "via": via, "why": why, "by": by});
        data["asker"] = json!({"pid": 4300u32, "argv0": "theseus", "trace_us": 210u64,
            "job": "act_j1", "wrapper_pid": 4290u32});
        data["from_job"] = json!(true);
        data["act"] = json!("action.confirm");
        data["session_id"] = json!(Some(S));
        data
    };
    wire(
        "approval_refused_answer",
        &line(notify::APPROVAL_REFUSED, answer),
    );
    let tighten = {
        let mut data = json!({"act": "policy.tighten", "tool": "proc.run", "who": who, "via": via,
                       "why": why, "by": by});
        data["asker"] = json!({"pid": 4301u32, "argv0": "sh", "trace_us": 95u64,
            "under_other_daemon": 777u32});
        data["from_job"] = json!(true);
        data["act"] = json!("policy.tighten");
        data["session_id"] = json!(None::<&str>);
        data
    };
    wire(
        "approval_refused_tighten",
        &line(notify::APPROVAL_REFUSED, tighten),
    );
    let trust = {
        let mut data = json!({"act": "policy.trust", "session_id": S, "who": who, "via": via,
                       "why": why, "by": by});
        data["asker"] = json!({"untraceable": "no such process", "trace_us": 12u64});
        data["from_job"] = json!(true);
        data["act"] = json!("policy.trust");
        data["session_id"] = json!(Some(S));
        data
    };
    wire(
        "approval_refused_trust",
        &line(notify::APPROVAL_REFUSED, trust),
    );
    let orphan = {
        let mut data = json!({"act": "policy.untighten", "tool": "proc.run", "who": who,
                       "via": via, "why": why, "by": by});
        data["asker"] = json!({"pid": 4302u32, "argv0": "bash", "trace_us": 40u64,
            "under_daemon": 4000u32});
        data["from_job"] = json!(true);
        data["act"] = json!("policy.untighten");
        data["session_id"] = json!(None::<&str>);
        data
    };
    wire(
        "approval_refused_orphan",
        &line(notify::APPROVAL_REFUSED, orphan),
    );
}

#[test]
fn narrative_line() {
    wire(
        "narrative_line",
        &line(
            notify::NARRATIVE_LINE,
            NarrativeLine {
                seq: 7,
                at_unix_ms: 1_759_300_500_000,
                part: NarrativePart::Tool,
                session_id: Some(S.into()),
                turn_id: None,
                text: "The model called fs.read.".into(),
            },
        ),
    );
}

// ------------------------------- the typed events write the captured bytes

/// The typed event, as a sender builds it, writes the fixture's bytes.
fn typed(name: &str, e: Event) {
    wire(name, &serde_json::to_string(&Message::from(e)).unwrap());
}

fn external_typed() -> ExternalText {
    ExternalText {
        since_ms: 1_759_300_000_000,
        tool: "http.fetch".into(),
        url: "notes.example/today".into(),
        node_id: "nod_e1".into(),
        via: Some("fs.read".into()),
        ..Default::default()
    }
}

fn plan_typed(argv: bool) -> Plan {
    Plan {
        resources: vec![Resource {
            path: "/w/notes".into(),
            access: if argv { Access::Exec } else { Access::Read },
        }],
        argv: argv.then(|| vec!["make".into(), "notes".into()]),
        url: None,
        summary: if argv { "run make notes" } else { "list notes" }.into(),
        ..Default::default()
    }
}

fn proposal_typed(tool: &str) -> Proposal {
    Proposal {
        tool: tool.into(),
        args: json!({"path": "notes"}),
        resource: Some("/w/notes".into()),
        policy_context: json!({}),
    }
}

fn proposed_typed(tool: &str, gate: GateRecord) -> Event {
    Event::ToolProposed(ToolProposed {
        session_id: S.into(),
        turn_id: T.into(),
        tool_use_id: "tu_1".into(),
        tool: tool.into(),
        input: json!({"path": "notes"}),
        gate,
    })
}

#[test]
fn typed_tool_proposed() {
    let allow = GateResult {
        gate: "allow".into(),
        ..Default::default()
    };
    typed(
        "tool_proposed_allow",
        proposed_typed(
            "fs.list",
            GateRecord {
                result: allow.clone(),
                validated: true,
                decision: Some(GateDecision {
                    posture: Some("open".into()),
                    reason: "fs.list — open (policy.tools)".into(),
                    ..Default::default()
                }),
                plan: Some(plan_typed(false)),
                proposal: proposal_typed("fs.list"),
            },
        ),
    );
    typed(
        "tool_proposed_notify",
        proposed_typed(
            "proc.run",
            GateRecord {
                result: allow,
                validated: true,
                decision: Some(GateDecision {
                    posture: Some("notify".into()),
                    reason: "proc.run — notify (enforcement = notify)".into(),
                    notify: Some(Notice {
                        kind: "notify".into(),
                        setting: "enforcement = notify".into(),
                        rule: "proc.run — notify (enforcement = notify)".into(),
                    }),
                    granted: Some("gh got GH_TOKEN".into()),
                    ..Default::default()
                }),
                plan: Some(plan_typed(true)),
                proposal: proposal_typed("proc.run"),
            },
        ),
    );
    typed(
        "tool_proposed_confirm",
        proposed_typed(
            "fs.write",
            GateRecord {
                result: GateResult {
                    gate: "needs_confirm".into(),
                    reason: None,
                    by: Some("operator".into()),
                },
                validated: true,
                decision: Some(GateDecision {
                    posture: Some("approve".into()),
                    reason: "write outside the roots: fs.write — approve".into(),
                    floor: true,
                    external: Some(external_typed()),
                    ..Default::default()
                }),
                plan: Some(plan_typed(false)),
                proposal: proposal_typed("fs.write"),
            },
        ),
    );
    typed(
        "tool_proposed_invalid",
        proposed_typed(
            "fs.read",
            GateRecord {
                result: GateResult {
                    gate: "deny".into(),
                    reason: Some("validation: missing field `path`".into()),
                    by: None,
                },
                validated: false,
                decision: None,
                plan: None,
                proposal: proposal_typed("fs.read"),
            },
        ),
    );
}

/// A `context.compiled` for a recompile, or an append (the shapes' sample).
fn summary(recompile: bool) -> ContextCompiled {
    ContextCompiled {
        session_id: S.into(),
        turn_id: T.into(),
        loop_index: 1,
        decision: if recompile { "recompile" } else { "append" }.into(),
        trigger: recompile.then(|| "window".into()),
        compilation_id: "cmp_b2".into(),
        strategy: "fresh".into(),
        prefix_nodes: 2,
        tail_nodes: 6,
        messages: 7,
        est_tokens: 4210,
        estimate: None,
        digest: "9f2c1a0b7d3e4f51".into(),
        repairs: if recompile {
            vec!["tu_lost1".into()]
        } else {
            vec![]
        },
        tools: 14,
        nodes_scanned: 11,
        context_files: if recompile {
            vec![
                ContextFileRef {
                    path: "/w/AGENTS.md".into(),
                    digest: Some("a1b2c3d4e5f60718".into()),
                    bytes: 2048,
                    cut: false,
                    missing: None,
                    persona: None,
                    public: true,
                    withheld: None,
                },
                ContextFileRef {
                    path: "/w/persona/clerk.md".into(),
                    digest: Some("0f1e2d3c4b5a6978".into()),
                    bytes: 65536,
                    cut: true,
                    missing: None,
                    persona: Some("clerk".into()),
                    public: false,
                    withheld: None,
                },
                ContextFileRef {
                    path: "/w/missing.md".into(),
                    digest: None,
                    bytes: 0,
                    cut: false,
                    missing: Some("not found".into()),
                    persona: None,
                    public: false,
                    withheld: None,
                },
                ContextFileRef {
                    path: "/w/notes.md".into(),
                    digest: None,
                    bytes: 0,
                    cut: false,
                    missing: None,
                    persona: None,
                    public: false,
                    withheld: Some("not public, in a shared place".into()),
                },
            ]
        } else {
            vec![]
        },
        persona: recompile.then(|| "clerk".into()),
        cache: CacheSummary {
            breakpoints: if recompile {
                vec!["header".into(), "conversation".into()]
            } else {
                vec![]
            },
            ttl: "1h".into(),
            conversation_ttl: "5m".into(),
        },
        // The place rule: a shared place's compile, which withheld one file;
        // an append from a daemon before it has neither.
        class: recompile.then_some(PlaceClass::Shared),
        withheld: if recompile { 1 } else { 0 },
    }
}

#[test]
fn typed_model_thinking_and_context() {
    typed(
        "model_thinking",
        Event::ModelThinking(ModelDelta {
            turn_id: T.into(),
            loop_index: 1,
            text: "Look first.\n".into(),
        }),
    );
    typed(
        "context_compiled_recompile",
        Event::ContextCompiled(summary(true)),
    );
    typed(
        "context_compiled_append",
        Event::ContextCompiled(summary(false)),
    );
}

#[test]
fn typed_tool_started_ended_and_written() {
    for (name, backend) in [
        ("tool_started_harness", "harness"),
        ("tool_started_inproc", "inproc"),
    ] {
        typed(
            name,
            Event::ToolStarted(ToolStarted {
                session_id: S.into(),
                turn_id: T.into(),
                tool_use_id: "tu_3".into(),
                tool: "fs.read".into(),
                correlation_id: "act_k3".into(),
                backend: backend.into(),
                ..Default::default()
            }),
        );
    }
    for (name, granted, withheld) in [
        ("tool_started_job", None, vec![]),
        (
            "tool_started_job_granted",
            Some("gh got GH_TOKEN"),
            vec!["git got no GIT_TOKEN".to_string()],
        ),
    ] {
        typed(
            name,
            Event::ToolStarted(ToolStarted {
                session_id: S.into(),
                turn_id: T.into(),
                tool_use_id: "tu_2".into(),
                tool: "proc.run".into(),
                correlation_id: "act_k2".into(),
                backend: "job".into(),
                pid: Some(4242),
                argv: Some(vec!["make".into(), "notes".into()]),
                cwd: Some("/w/notes".into()),
                granted: Some(granted.map(Into::into)),
                withheld: Some(withheld),
                class: None,
                egress: None,
            }),
        );
    }
    for (name, correlation_id, exit_code, stopped_by, late) in [
        ("tool_ended_ok", Some("act_k2"), Some(0), None, false),
        (
            "tool_ended_stopped",
            Some("act_k3"),
            None,
            Some("the CLI"),
            true,
        ),
        ("tool_ended_unknown_tool", None, None, None, false),
    ] {
        typed(
            name,
            Event::ToolEnded(ToolEnded {
                session_id: S.into(),
                turn_id: T.into(),
                tool_use_id: "tu_2".into(),
                tool: "proc.run".into(),
                status: "ok".into(),
                duration_ms: Some(1520),
                correlation_id: correlation_id.map(Into::into),
                late,
                truncated: true,
                bytes: 70_000,
                node_id: "nod_r2".into(),
                exit_code,
                stopped_by: stopped_by.map(Into::into),
                preview: "a.md\nb.md\n".repeat(300).chars().take(2000).collect(),
                scratch: None,
                reached: None,
                verified: None,
            }),
        );
    }
    typed(
        "node_written",
        Event::NodeWritten(NodeWritten {
            session_id: S.into(),
            node_id: "nod_r2".into(),
            kind: "tool_result".into(),
        }),
    );
}

#[test]
fn typed_tool_started_in_l1_with_egress() {
    // An L1 job with an egress list (M4 18c): its class and its hosts.
    typed(
        "tool_started_l1_egress",
        Event::ToolStarted(ToolStarted {
            session_id: S.into(),
            turn_id: T.into(),
            tool_use_id: "tu_3".into(),
            tool: "proc.run".into(),
            correlation_id: "act_k4".into(),
            backend: "job".into(),
            pid: Some(4243),
            argv: Some(vec!["python3".into(), "fetch.py".into()]),
            cwd: Some("/w/notes".into()),
            granted: Some(None),
            withheld: Some(vec![]),
            class: Some("l1".into()),
            egress: Some(vec!["api.example.test:443".into()]),
        }),
    );
}

#[test]
fn typed_policy_notified_and_resolved() {
    for (name, granted, task) in [
        ("policy_notified", None, None),
        (
            "policy_notified_task",
            Some("gh got GH_TOKEN"),
            Some("t4sk01"),
        ),
    ] {
        typed(
            name,
            Event::PolicyNotified(PolicyNotified {
                session_id: S.into(),
                turn_id: T.into(),
                tool_use_id: "tu_2".into(),
                correlation_id: "act_k2".into(),
                tool: "proc.run".into(),
                input: json!({"argv": ["make", "notes"]}),
                summary: "run make notes".into(),
                notice: Notice {
                    kind: "notify".into(),
                    setting: "enforcement = notify".into(),
                    rule: "proc.run — notify (enforcement = notify)".into(),
                },
                granted: granted.map(Into::into),
                task: task.map(Into::into),
            }),
        );
    }
    let resolved = |corr: &str, by: Option<&str>| ConfirmResolved {
        session_id: S.into(),
        correlation_id: corr.into(),
        by: by.map(Into::into),
        ..Default::default()
    };
    for (name, r) in [
        (
            "confirm_resolved_answer",
            ConfirmResolved {
                approved: true,
                trust: Some(true),
                ..resolved("act_k4", Some("the CLI"))
            },
        ),
        (
            "confirm_resolved_budget",
            resolved("act_b5", Some("discord:ana")),
        ),
        (
            "confirm_resolved_withdrawn",
            ConfirmResolved {
                withdrawn: true,
                ..resolved("act_b5", Some("config"))
            },
        ),
        (
            "confirm_resolved_cancelled",
            ConfirmResolved {
                cancelled: true,
                ..resolved("act_k4", Some("the CLI"))
            },
        ),
        (
            "confirm_resolved_stopped",
            ConfirmResolved {
                stopped: true,
                ..resolved("act_k4", Some("the CLI"))
            },
        ),
        (
            "confirm_resolved_superseded",
            ConfirmResolved {
                superseded: true,
                ..resolved("act_b5", Some("the web UI"))
            },
        ),
        (
            "confirm_resolved_superseded_call",
            ConfirmResolved {
                superseded: true,
                ..resolved("act_k4", None)
            },
        ),
    ] {
        typed(name, Event::ConfirmResolved(r));
    }
}

#[test]
fn typed_approval_refused() {
    let refused = |act: &str, session: Option<&str>, asker: Asker| ApprovalRefused {
        act: act.into(),
        session_id: session.map(Into::into),
        who: "sock#9".into(),
        via: "cli".into(),
        why: "from a Theseus job's process (job act_j1, pid 4300, theseus)".into(),
        by: "the CLI".into(),
        asker: Some(asker),
        from_job: true,
        ..Default::default()
    };
    let seen = |pid: u32, argv0: &str, trace_us: u64| Asker {
        pid: Some(pid),
        argv0: Some(argv0.into()),
        trace_us,
        ..Default::default()
    };
    typed(
        "approval_refused_answer",
        Event::ApprovalRefused(ApprovalRefused {
            correlation_id: Some("act_k4".into()),
            tool: Some("fs.write".into()),
            approve: Some(true),
            ..refused(
                "action.confirm",
                Some(S),
                Asker {
                    job: Some("act_j1".into()),
                    wrapper_pid: Some(4290),
                    ..seen(4300, "theseus", 210)
                },
            )
        }),
    );
    typed(
        "approval_refused_tighten",
        Event::ApprovalRefused(ApprovalRefused {
            tool: Some("proc.run".into()),
            ..refused(
                "policy.tighten",
                None,
                Asker {
                    under_other_daemon: Some(777),
                    ..seen(4301, "sh", 95)
                },
            )
        }),
    );
    typed(
        "approval_refused_trust",
        Event::ApprovalRefused(refused(
            "policy.trust",
            Some(S),
            Asker {
                untraceable: Some("no such process".into()),
                trace_us: 12,
                ..Default::default()
            },
        )),
    );
    typed(
        "approval_refused_orphan",
        Event::ApprovalRefused(ApprovalRefused {
            tool: Some("proc.run".into()),
            ..refused(
                "policy.untighten",
                None,
                Asker {
                    under_daemon: Some(4000),
                    ..seen(4302, "bash", 40)
                },
            )
        }),
    );
}

/// Every fixture decodes as its `Event` and writes the same bytes again: what a
/// client reads is what the daemon sent.
#[test]
fn every_fixture_decodes_as_an_event_and_writes_the_same_bytes() {
    let dir = fixture("x");
    let dir = dir.parent().unwrap();
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let line = text.trim_end_matches('\n');
        let Message::Notification(note) = serde_json::from_str(line).unwrap() else {
            panic!("{}: not a notification", path.display());
        };
        let event = Event::from_notification(&note.method, &note.params)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .unwrap_or_else(|| panic!("{}: an unknown method", path.display()));
        assert_eq!(event.method(), note.method);
        let again = serde_json::to_string(&Message::from(event)).unwrap();
        assert_eq!(again, line, "{}", path.display());
        n += 1;
    }
    assert_eq!(n, 44);
}

/// Every line of a real daemon's capture (`theseus --json watch`, its path in
/// `THESEUS_WIRE_CAPTURE`) decodes as its `Event` and writes the same bytes
/// again: the typed structs cover what a daemon sends.
#[test]
#[ignore = "reads THESEUS_WIRE_CAPTURE, a capture of `theseus --json watch`"]
fn a_captured_session_decodes_and_writes_the_same_bytes() {
    let path = std::env::var("THESEUS_WIRE_CAPTURE").expect("THESEUS_WIRE_CAPTURE");
    let mut methods = std::collections::BTreeMap::<String, usize>::new();
    for (i, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
        let Message::Notification(note) = serde_json::from_str(line).unwrap() else {
            panic!("line {}: not a notification", i + 1);
        };
        let event = Event::from_notification(&note.method, &note.params)
            .unwrap_or_else(|e| panic!("line {} ({}): {e}", i + 1, note.method))
            .unwrap_or_else(|| panic!("line {}: unknown method {}", i + 1, note.method));
        let again = serde_json::to_string(&Message::from(event)).unwrap();
        assert_eq!(again, line, "line {} ({})", i + 1, note.method);
        *methods.entry(note.method).or_default() += 1;
    }
    eprintln!("{methods:?}");
}
