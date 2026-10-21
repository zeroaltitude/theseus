#![recursion_limit = "512"]
//! `why`'s words (theseus-ighf): the one renderer the CLI and Discord share,
//! held to its goldens in `tests/why/`. `THESEUS_GOLDEN=write` rewrites them.

use std::path::PathBuf;

use serde_json::{json, Value};
use theseus_protocol::context::ContextExplainResult;
use theseus_protocol::why::{tools_called, why_lines, why_short, ToolCalled, WhyInput};
use theseus_protocol::{NodeListResult, PolicyExplainResult};

/// A private place's turn: a route decision, one recalled note from an
/// imported chat and one from this conversation, a drop for the budget and
/// one for the place, and a context file carried as its header alone.
pub fn explain(class: &str) -> ContextExplainResult {
    serde_json::from_value(json!({
        "session_id": "ses_reef42", "title": "the harbour survey", "kind": "conversation",
        "class": class, "place": if class == "private" { "CLI" } else { "#pier" },
        "turns": [], "turn_id": "trn_k3m9x2",
        "compiled": {"session_id": "ses_reef42", "turn_id": "trn_k3m9x2", "loop": 0,
            "decision": "append", "withheld": 1},
        "profile": "deep", "model": "glm-x", "window": 200000,
        "parts": [
            {"block": "header", "name": "persona", "bytes": 4800, "tokens": 1200, "then": "same"},
            {"block": "context", "name": "AGENTS.md", "bytes": 9000, "tokens": 2250, "then": "same"},
            {"block": "context", "name": "NOTES.md", "bytes": 80, "tokens": 20,
             "note": "a shared place's file, carried as its header alone"},
            {"block": "guidance", "name": "preamble", "bytes": 400, "tokens": 100, "version": 3},
            {"block": "tools", "name": "tools", "bytes": 12000, "tokens": 3000,
             "note": "fs.read, proc.run"},
            {"block": "recall", "name": "ses_ep9 · chat with the tide office", "bytes": 800, "tokens": 210},
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
                {"node_id": "nod_b", "chunk": 0, "session_id": "ses_reef42", "position": 9, "kind": "note",
                 "rank": 2, "fused": 0.71, "tokens": 90,
                 "sources": {"entity": {"rank": 1, "score": 1.0}}}],
            "dropped": [
                {"node_id": "nod_c", "chunk": 0, "session_id": "ses_ep9", "reason": "budget", "fused": 0.4, "tokens": 300},
                {"node_id": "nod_d", "chunk": 0, "session_id": "ses_priv1", "reason": "place", "fused": 0.3}],
            "drops": {"budget": 1, "place": 1},
            "budget_tokens": 400, "used_tokens": 300,
            "timings": {"index_ms": 3.0, "pack_ms": 1.0, "total_ms": 5.0, "deadline_ms": 250}}],
        "noted": {"nod_a": 1772640000000u64, "nod_b": 1772700000000u64},
        "sources": {
            "ses_ep9": {"place": "CLI", "imported": {
                "session_id": "ses_ep9", "episode_id": "ep9", "tag": "chat", "source": "chat export",
                "place_kind": "chat", "place_name": "tide office", "start_ms": 1772582400000u64,
                "end_ms": 1772668800000u64, "sensitivity": "personal", "topics": [], "messages": 3, "summary": true, "file": "chat.jsonl", "line": 1, "imported_at_ms": 1u64}},
            "ses_reef42": {"place": "CLI", "title": "the harbour survey"}},
        "ms": 4.0
    }))
    .unwrap()
}

pub fn route() -> Value {
    json!({"mode": "deep_coding", "reason": "verdict", "from": "quick", "profile": "deep",
        "effort": "high", "effort_reason": "applied", "effort_applied": true, "effort_ran": "high"})
}

pub fn policy() -> PolicyExplainResult {
    serde_json::from_value(json!({"roots": ["/work"], "places": [{
        "place": "cli", "name": "CLI", "class": "private", "session_id": "ses_reef42",
        "tools": [
            {"tool": "fs.read", "class": "read", "offered": true, "result": "open", "reason": "r",
             "layers": [{"layer": "class", "says": "reads are open", "result": "open"},
                        {"layer": "posture", "says": "open", "result": "open"}]},
            {"tool": "proc.run", "class": "run", "offered": true, "result": "approve", "reason": "r",
             "layers": [{"layer": "posture", "says": "approve", "result": "open"},
                        {"layer": "floor", "says": "the place's floor asks", "result": "approve",
                         "setting": "[policy] floor = \"approve\"", "raised": true}]}]}]}))
    .unwrap()
}

/// The turn's `tool_call` nodes, as `node.list` answers them: two reads and
/// a run the gate asked about, and a call of another turn.
pub fn calls() -> NodeListResult {
    let node = |id: &str, turn: &str, pos: u64, tool: &str, gate: &str, reason: &str| {
        json!({"node_id": id, "kind": "tool_call", "session_id": "ses_reef42", "position": pos,
            "at_unix_ms": 1, "turn_id": turn, "text": "", "bytes": 0,
            "detail": {"tool": tool, "gate": {"result": {"gate": gate}, "decision": {"reason": reason}}}})
    };
    serde_json::from_value(json!({"total": 4, "nodes": [
        node("n3", "trn_k3m9x2", 30, "fs_read", "open", "reads are open"),
        node("n2", "trn_other1", 25, "fs_write", "approve", "writes ask"),
        node("n1", "trn_k3m9x2", 20, "proc_run", "approve", "the place's floor asks"),
        node("n4", "trn_k3m9x2", 40, "fs_read", "open", "reads are open")]}))
    .unwrap()
}

fn golden(name: &str, got: &str) {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "why",
        &format!("{name}.txt"),
    ]
    .iter()
    .collect();
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (THESEUS_GOLDEN=write)", path.display()));
    assert_eq!(got, want, "{name} differs from {}", path.display());
}

fn input<'a>(
    x: &'a ContextExplainResult,
    p: &'a PolicyExplainResult,
    r: &'a Value,
    tools: &'a [ToolCalled],
) -> WhyInput<'a> {
    WhyInput {
        explain: x,
        policy: Some(p),
        route: Some(r),
        tools,
        hide_sources: false,
    }
}

#[test]
fn a_turn_with_a_route_a_recall_a_drop_and_a_gated_tool_in_full() {
    let (x, p, r) = (explain("private"), policy(), route());
    let tools = tools_called(&calls(), "trn_k3m9x2");
    let names: Vec<_> = tools.iter().map(|t| (t.tool.as_str(), t.calls)).collect();
    assert_eq!(
        names,
        [("proc_run", 1), ("fs_read", 2)],
        "in the order of first call, once each"
    );
    let got = why_lines(&input(&x, &p, &r, &tools)).join("\n") + "\n";
    golden("full", &got);
}

#[test]
fn a_shared_place_shows_that_a_source_was_there_and_nothing_of_it() {
    let (x, p, r) = (explain("shared"), policy(), route());
    let tools = tools_called(&calls(), "trn_k3m9x2");
    let got = why_lines(&input(&x, &p, &r, &tools)).join("\n") + "\n";
    for secret in ["tide office", "chat export", "ses_ep9", "2026-03-04"] {
        assert!(
            !got.contains(secret),
            "{secret} reached a shared place:\n{got}"
        );
    }
    assert!(got.contains("an imported source (not shown in a shared place)"));
    golden("shared", &got);
}

#[test]
fn the_short_form_keeps_the_top_items_and_counts_the_rest_inside_its_limit() {
    let (x, p, r) = (explain("private"), policy(), route());
    let tools = tools_called(&calls(), "trn_k3m9x2");
    let got = why_short(&input(&x, &p, &r, &tools), 2, 1800);
    assert!(got.chars().count() <= 1800);
    assert!(got.contains("… and 5 more"), "{got}");
    golden("short", &(got + "\n"));
    // A tiny budget cuts whole lines and says how many it left.
    let tiny = why_short(&input(&x, &p, &r, &tools), 2, 300);
    assert!(tiny.chars().count() <= 300, "{tiny}");
    assert!(tiny.contains("more lines"), "{tiny}");
}

#[test]
fn a_session_with_no_turn_says_so() {
    let mut x = explain("private");
    x.turn_id = None;
    let got = why_lines(&WhyInput {
        explain: &x,
        policy: None,
        route: None,
        tools: &[],
        hide_sources: false,
    });
    assert!(got
        .iter()
        .any(|l| l.contains("no turn of this session has compiled")));
}

#[test]
fn a_parts_long_note_is_clipped_to_one_line_of_it() {
    let mut x = explain("private");
    let tools = x.parts.iter_mut().find(|p| p.block == "tools").unwrap();
    tools.note = Some("fs.read, ".repeat(60));
    let lines = why_lines(&WhyInput {
        explain: &x,
        policy: None,
        route: None,
        tools: &[],
        hide_sources: false,
    });
    let line = lines
        .iter()
        .find(|l| l.trim_start().starts_with("tools "))
        .unwrap();
    assert!(line.ends_with("…") && line.chars().count() < 160, "{line}");
}

/// A detoured turn (theseus-r5ydi): no compile, so the parts are the ones its
/// `context.detoured` row named, and the answer says what it left out.
fn detour_explain() -> ContextExplainResult {
    serde_json::from_value(json!({
        "session_id": "ses_reef42", "title": "the harbour survey", "kind": "conversation",
        "class": "private", "place": "CLI",
        "turns": [{"turn_id": "trn_k3m9x2", "at_ms": 1, "loops": 1, "est_tokens": 7300,
                   "compilation_id": "", "detoured": true}],
        "turn_id": "trn_k3m9x2",
        "detour": {"kind": "quick", "profile": "haiku", "model": "claude-haiku-5-5"},
        "profile": "haiku", "model": "claude-haiku-5-5", "window": 200000,
        "parts": [
            {"block": "header", "name": "the profile's header and the standing records", "bytes": 0, "tokens": 720},
            {"block": "session", "name": "the session's directory", "bytes": 0, "tokens": 15},
            {"block": "tools", "name": "17 tools", "bytes": 0, "tokens": 6400},
            {"block": "exchanges", "name": "2 earlier exchanges", "bytes": 0, "tokens": 90},
            {"block": "message", "name": "the message", "bytes": 0, "tokens": 24},
            {"block": "project", "name": "the project's slice", "bytes": 0, "tokens": 120}],
        "digest_now": "", "recalls": [], "sources": {}, "ms": 1.0
    }))
    .unwrap()
}

fn detour_route() -> Value {
    json!({"mode": "quick", "reason": "detour", "detour": true, "from": "sonnet", "profile": "haiku"})
}

#[test]
fn a_detoured_turn_says_it_detoured_and_what_it_left_out() {
    let (x, r) = (detour_explain(), detour_route());
    let got = why_lines(&WhyInput {
        explain: &x,
        policy: None,
        route: Some(&r),
        tools: &[],
        hide_sources: false,
    })
    .join("\n")
        + "\n";
    assert!(
        !got.contains("no turn of this session has compiled"),
        "{got}"
    );
    golden("detour", &got);
    // The context's short form is the same parts against the window.
    let short = theseus_protocol::fill::context_short(&x).join("\n") + "\n";
    golden("detour_context", &short);
    // A trivial detour also names its own recall among what it left out.
    let mut trivial = detour_explain();
    trivial.detour.as_mut().unwrap().kind = "trivial".into();
    let lines = why_lines(&WhyInput {
        explain: &trivial,
        policy: None,
        route: None,
        tools: &[],
        hide_sources: false,
    })
    .join("\n");
    assert!(
        lines.contains("this turn's own recall: a trivial detour carries none"),
        "{lines}"
    );
}
