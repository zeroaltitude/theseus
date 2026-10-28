//! `turn.inspect` (theseus-x9swu): a turn assembled whole from the nodes and
//! the ledger it left, for the cockpit's inspector. The fixture is a turn the
//! way the loop writes it: an input, then per loop a compile row, an answer
//! with thinking, parallel tool calls and their results; a floor question
//! and its answer; a compaction; the end and the trace.

use std::time::Instant;

use serde_json::{json, Value};
use theseus_protocol::inspect::{TurnInspectParams, TurnInspectResult};
use theseus_protocol::{SessionKind, Usage};

use super::tests::{roundtrip, submit, test_core, test_core_with};
use super::Core;
use crate::node::{Body, Node, ResultStatus};
use crate::session::SessionRecord;

const T0: u64 = 1_760_000_000_000;

fn open(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

fn row(core: &Core, at: u64, kind: &str, s: &str, t: &str, data: Value) {
    core.store
        .append_ledger(
            &json!({"at_unix_ms": at, "kind": kind, "session_id": s, "turn_id": t, "data": data}),
        )
        .unwrap();
}

fn compiled(i: u32, s: &str, t: &str) -> Value {
    json!({"session_id": s, "turn_id": t, "loop": i, "decision": if i == 0 { "recompile" } else { "append" },
           "trigger": null, "compilation_id": "cmp_a", "strategy": "transcript", "prefix_nodes": 4, "tail_nodes": 2 + i,
           "messages": 6 + 3 * i, "est_tokens": 9_000 + 1_200 * i, "digest": format!("dig{i}"), "repairs": [],
           "tools": 14, "nodes_scanned": 6, "context_files": [],
           "cache": {"breakpoints": ["header", "conversation"], "ttl": "1h", "conversation_ttl": "5m", "layers": {"header": "h1"}}})
}

fn write_loop(core: &Core, s: &str, t: &str, i: u32, last: bool, calls: u32) {
    let at = T0 + 1_000 * u64::from(i + 1);
    row(core, at, "context.compiled", s, t, compiled(i, s, t));
    let mut blocks = vec![
        json!({"type": "thinking", "thinking": format!("Plan for loop {i}: list, then count."), "signature": "sig"}),
        json!({"type": "text", "text": format!("Looking, round {i}.")}),
    ];
    let uses: Vec<String> = (0..calls).map(|k| format!("tu_{i}_{k}")).collect();
    if !last {
        for u in &uses {
            blocks.push(json!({"type": "tool_use", "id": u, "name": "fs_list", "input": {"path": "/harbour"}}));
        }
    }
    let mut a = Node::assistant(
        s,
        t,
        i,
        Body::AssistantMessage {
            blocks,
            model: "claude-sonnet-5".into(),
            provider: "anthropic".into(),
            stop_reason: Some(if last { "end_turn" } else { "tool_use" }.into()),
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50 + u64::from(i),
                cache_read_input_tokens: 8_000,
                cache_creation_input_tokens: 200,
                cache_creation_1h_input_tokens: 0,
            },
            cost_usd: Some(0.01),
            catalog_version: None,
            request_id: Some(format!("req_{i}")),
            correlation_id: None,
            compilation_id: Some("cmp_a".into()),
            request_digest: Some(format!("dig{i}")),
            effort: None,
        },
    );
    a.created_at_ms = at + 400;
    let aid = a.id.clone();
    let mut recs = vec![a.record().unwrap()];
    if !last {
        for u in &uses {
            let mut c = Node::tool_call(
                s,
                Some(t),
                None,
                Body::ToolCall {
                    tool_use_id: u.clone(),
                    tool: "fs.list".into(),
                    wire_name: "fs_list".into(),
                    input: json!({"path": "/harbour"}),
                    assistant_node: aid.clone(),
                    correlation_id: Some(u.replace("tu_", "act_")),
                    gate: None,
                },
            );
            c.created_at_ms = at + 450;
            recs.push(c.record().unwrap());
            let mut r = Node::tool_result(
                s,
                Some(t),
                None,
                Body::ToolResult {
                    tool_use_id: u.clone(),
                    tool: "fs.list".into(),
                    status: ResultStatus::Ok,
                    is_error: false,
                    content: "tide.log\nquay.txt".into(),
                    correlation_id: None,
                    bytes_total: 17,
                    truncated: false,
                    full_ref: None,
                    duration_ms: Some(12),
                    late: false,
                    meta: Value::Null,
                    image: None,
                    external: None,
                },
            );
            r.created_at_ms = at + 470;
            recs.push(r.record().unwrap());
        }
    }
    core.store.append(&recs).unwrap();
    row(core, at + 100, "recall.ran", s, t, json!({"admitted": 2}));
}

fn write_end(core: &Core, s: &str, t: &str, loops: u32) {
    // A compaction, a floor question and its answer.
    row(
        core,
        T0 + 500,
        "context.compacted",
        s,
        t,
        json!({"from": 40_000, "to": 9_000}),
    );
    row(
        core,
        T0 + 600,
        "tool.confirm_requested",
        s,
        t,
        json!({"tool": "proc.run", "tool_use_id": "tu_0_0", "correlation_id": "act_q"}),
    );
    // The answer is written with no turn, as the daemon writes it, and may come long after.
    core.store
        .append_ledger(
            &json!({"at_unix_ms": T0 + 900, "kind": "action.confirm_answered", "session_id": s,
                               "data": {"correlation_id": "act_q", "option": "approve"}}),
        )
        .unwrap();
    row(
        core,
        T0 + 1_000 * u64::from(loops + 1),
        "turn.ended",
        s,
        t,
        json!({"loops": loops, "stop_reason": "end_turn", "elapsed_ms": 1_000 * (u64::from(loops) + 1)}),
    );
    row(
        core,
        T0 + 1_000 * u64::from(loops + 1),
        "turn.trace",
        s,
        t,
        json!({"name": "turn", "kind": "turn", "start_us": 0, "end_us": 5_000_000, "children": [
        {"name": "loop 0", "kind": "loop", "start_us": 0, "end_us": 2_000_000, "children": [
            {"name": "provider.messages", "kind": "provider", "start_us": 100_000, "end_us": 900_000},
            {"name": "tool fs.list", "kind": "tool", "start_us": 1_000_000, "end_us": 1_500_000},
            {"name": "tool fs.list", "kind": "tool", "start_us": 1_100_000, "end_us": 1_600_000}]}]}),
    );
    // Another turn after it, so there is a next.
    row(
        core,
        T0 + 90_000,
        "turn.started",
        s,
        "turn_after",
        json!({}),
    );
}

/// The turn: `loops` loops of `calls` parallel calls each (the last loop
/// answers with text alone). Returns (session, turn).
fn build(core: &Core, loops: u32, calls: u32) -> (String, String) {
    let (s, t) = (open(core), "turn_inspected".to_string());
    row(
        core,
        T0 - 10_000,
        "turn.started",
        &s,
        "turn_before",
        json!({}),
    );
    row(
        core,
        T0 - 40,
        "route.decided",
        &s,
        &t,
        json!({"mode": "quick", "reason": "short question", "effort_applied": "low"}),
    );
    row(
        core,
        T0,
        "turn.started",
        &s,
        &t,
        json!({"author": "web#1", "model": "claude-sonnet-5"}),
    );
    core.store
        .append(&[Node::user(
            &s,
            Some(&t),
            "web#1",
            "How many files are in the harbour directory?",
        )
        .record()
        .unwrap()])
        .unwrap();
    for i in 0..loops {
        write_loop(core, &s, &t, i, i + 1 == loops, calls);
    }
    write_end(core, &s, &t, loops);
    (s, t)
}

fn inspect(core: &Core, s: &str, t: &str, private: bool) -> TurnInspectResult {
    core.turn_inspect(
        &TurnInspectParams {
            session_id: s.into(),
            turn_id: t.into(),
        },
        private,
    )
    .unwrap()
}

/// A multi-loop turn with parallel tools, thinking, a compaction and a floor
/// question comes back whole: loops, answers, calls, totals, rows, trace.
#[test]
fn a_multi_loop_turn_comes_back_whole() {
    let core = test_core("ok");
    let (s, t) = build(&core, 3, 2);
    let r = inspect(&core, &s, &t, true);
    assert_eq!(r.outcome, "ended");
    assert_eq!(r.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(
        r.prompt.text,
        "How many files are in the harbour directory?"
    );
    assert_eq!(r.prompt.author.as_deref(), Some("web#1"));
    assert_eq!(r.totals.loops, 3);
    assert_eq!(r.totals.model_calls, 3);
    assert_eq!(
        r.totals.tool_calls, 4,
        "two parallel calls in each of two loops"
    );
    assert_eq!(r.totals.usage.cache_read_input_tokens, 24_000);
    assert!((r.totals.cost_usd - 0.03).abs() < 1e-9);
    assert_eq!(
        r.route["mode"], "quick",
        "the route decision written before turn.started is the turn's"
    );
    assert_eq!(r.prev_turn.as_deref(), Some("turn_before"));
    assert_eq!(r.next_turn.as_deref(), Some("turn_after"));
    assert!(
        r.url.ends_with(&format!("/session/{s}/turn/{t}")),
        "{}",
        r.url
    );
    assert!(r.trace.as_ref().is_some_and(|x| x.children.len() == 1));
    assert!(r.withheld.is_none());
    let kinds: Vec<&str> = r.rows.iter().map(|x| x.kind.as_str()).collect();
    for k in [
        "context.compacted",
        "tool.confirm_requested",
        "action.confirm_answered",
        "recall.ran",
        "turn.ended",
    ] {
        assert!(kinds.contains(&k), "{k} in {kinds:?}");
    }
}

/// Each loop of that turn: its compile, its answer with the thinking in full,
/// its parallel calls with their results, and what its request added.
#[test]
fn each_loop_of_a_turn_has_its_compile_its_answer_and_its_calls() {
    let core = test_core("ok");
    let (s, t) = build(&core, 3, 2);
    let r = inspect(&core, &s, &t, true);
    let l0 = &r.loops[0];
    assert_eq!(l0.calls.len(), 2);
    assert_eq!(
        l0.calls[0].correlation_id.as_deref(),
        Some("act_0_0"),
        "a call names its action's ledger rows"
    );
    assert_eq!(l0.compiled.as_ref().unwrap().est_tokens, 9_000);
    let a = l0.answer.as_ref().unwrap();
    assert_eq!(
        a.thinking_chars,
        "Plan for loop 0: list, then count.".len() as u64
    );
    assert_eq!(
        a.blocks[0]["thinking"], "Plan for loop 0: list, then count.",
        "thinking in full"
    );
    assert_eq!(a.request_digest.as_deref(), Some("dig0"));
    let c = &l0.calls[1];
    assert_eq!(
        (c.tool.as_str(), c.status.as_deref(), c.duration_ms),
        ("fs.list", Some("ok"), Some(12))
    );
    assert_eq!(c.content, "tide.log\nquay.txt");
    assert_eq!(c.input["path"], "/harbour");
    assert!(
        l0.added.is_empty(),
        "the first request adds nothing to a request before it"
    );
    assert_eq!(
        r.loops[1].added.len(),
        5,
        "the answer, two calls and two results of loop 0"
    );
    assert!(
        r.loops[2].calls.is_empty()
            && r.loops[2].answer.as_ref().unwrap().stop_reason.as_deref() == Some("end_turn")
    );
}

/// A request that carried a vault secret (theseus-gyjyo R7b): the loop's answer
/// names which, how many times, and whether it was substituted; a private
/// reader gets the names, another the count alone. A token count is no loop's
/// request, and a loop that sent none has an empty list.
#[test]
fn an_answer_names_the_vault_secrets_its_request_carried() {
    let core = test_core("ok");
    let (s, t) = build(&core, 2, 1);
    let redacted = |at: u64, op: &str, loop_: Value, names: Value, n: u64, sub: bool| {
        row(
            &core,
            at,
            "request.redacted",
            &s,
            &t,
            json!({"provider": "anthropic", "model": "claude-sonnet-5", "names": names, "count": n,
                   "substituted": sub, "op": op, "loop": loop_, "correlation_id": "act_p0"}),
        );
    };
    redacted(
        T0 + 900,
        "stream",
        json!(0),
        json!(["anthropic_key", "db_password"]),
        3,
        true,
    );
    redacted(
        T0 + 950,
        "count_tokens",
        Value::Null,
        json!(["tokens_only"]),
        1,
        true,
    );
    redacted(
        T0 + 1_950,
        "stream",
        json!(1),
        json!(["db_password"]),
        1,
        false,
    );
    // A compaction call runs on its own profile: its row says which model it
    // went to, not the loop's.
    row(
        &core,
        T0 + 920,
        "request.redacted",
        &s,
        &t,
        json!({"provider": "openai", "model": "gpt-summary", "names": ["db_password"], "count": 2,
               "substituted": true, "op": "compaction", "loop": 0, "correlation_id": "act_c0"}),
    );
    let r = inspect(&core, &s, &t, true);
    let a0 = r.loops[0].answer.as_ref().unwrap();
    assert_eq!(a0.redacted.len(), 2, "{:?}", a0.redacted);
    assert_eq!(
        (
            a0.redacted[0].op.as_str(),
            a0.redacted[0].provider.as_str(),
            a0.redacted[0].model.as_str()
        ),
        ("stream", "anthropic", "claude-sonnet-5")
    );
    assert_eq!(
        (
            a0.redacted[1].op.as_str(),
            a0.redacted[1].provider.as_str(),
            a0.redacted[1].model.as_str()
        ),
        ("compaction", "openai", "gpt-summary")
    );
    assert_eq!(a0.redacted[1].correlation_id.as_deref(), Some("act_c0"));
    assert_eq!(a0.redacted[0].names, ["anthropic_key", "db_password"]);
    assert_eq!(
        (a0.redacted[0].count, a0.redacted[0].substituted),
        (3, true)
    );
    assert_eq!(a0.redacted[0].correlation_id.as_deref(), Some("act_p0"));
    let a1 = r.loops[1].answer.as_ref().unwrap();
    assert_eq!(a1.redacted.len(), 1);
    assert_eq!(a1.redacted[0].names, ["db_password"]);
    assert!(!a1.redacted[0].substituted, "sent as written is said so");
    assert!(!serde_json::to_string(&r.loops)
        .unwrap()
        .contains("tokens_only"));
    // Another surface learns how many and whether, never which.
    let o = inspect(&core, &s, &t, false);
    let a = o.loops[0].answer.as_ref().unwrap();
    assert_eq!(a.redacted.len(), 1);
    assert!(a.redacted[0].names.is_empty());
    assert_eq!(a.redacted[0].count, 3);
    assert!(!serde_json::to_string(&o).unwrap().contains("anthropic_key"));
    // A turn with no such row has none.
    let (s2, t2) = build(&core, 1, 1);
    assert!(inspect(&core, &s2, &t2, true).loops[0]
        .answer
        .as_ref()
        .unwrap()
        .redacted
        .is_empty());
}

/// Another surface reads shapes and sizes, never thinking, texts, inputs or results.
#[test]
fn a_surface_that_reads_no_private_text_gets_the_shapes_alone() {
    let core = test_core("ok");
    let (s, t) = build(&core, 2, 1);
    let r = inspect(&core, &s, &t, false);
    assert!(r.withheld.is_some());
    assert_eq!(r.prompt.text, "");
    assert!(r.prompt.chars > 0);
    let a = r.loops[0].answer.as_ref().unwrap();
    assert!(a.thinking_chars > 0, "the size of the thinking is told");
    let wire = serde_json::to_string(&r).unwrap();
    for secret in [
        "Plan for loop",
        "Looking, round",
        "tide.log",
        "harbour directory",
        "/harbour",
    ] {
        assert!(
            !wire.contains(secret),
            "{secret} reached a surface that reads no private text"
        );
    }
    assert!(r.rows.iter().all(|x| x.data.is_null()));
    assert!(r
        .loops
        .iter()
        .flat_map(|l| &l.calls)
        .all(|c| c.gate.is_none() && c.input.is_null() && c.content.is_empty()));
    assert!(
        r.trace.as_ref().is_some_and(|t| t.attrs.is_null()),
        "the trace keeps its times and loses its attributes"
    );
}

#[test]
fn a_turn_the_session_never_had_is_refused_and_a_session_it_never_had_too() {
    let core = test_core("ok");
    let (s, _) = build(&core, 1, 1);
    let e = core
        .turn_inspect(
            &TurnInspectParams {
                session_id: s,
                turn_id: "turn_nope".into(),
            },
            true,
        )
        .unwrap_err();
    assert!(e.to_string().contains("no turn turn_nope"), "{e}");
    assert!(core
        .turn_inspect(
            &TurnInspectParams {
                session_id: "ses_none".into(),
                turn_id: "x".into()
            },
            true
        )
        .is_err());
}

/// A turn of 20 loops and 40 tool calls is read in 150 ms or less (p95, a debug build).
#[test]
fn a_twenty_loop_turn_with_forty_calls_is_read_within_150_ms() {
    let core = test_core("ok");
    let (s, t) = build(&core, 20, 2);
    let r = inspect(&core, &s, &t, true);
    assert_eq!(
        (r.totals.loops, r.totals.tool_calls),
        (20, 38),
        "19 loops of 2 calls, the last answers in text"
    );
    let mut times: Vec<f64> = (0..20)
        .map(|_| {
            let t0 = Instant::now();
            inspect(&core, &s, &t, true);
            t0.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = times[18];
    assert!(p95 < 150.0, "p95 {p95:.1} ms of {times:?}");
}

/// The inspector writes nothing.
#[test]
fn reading_a_turn_writes_no_frame() {
    let core = test_core("ok");
    let (s, t) = build(&core, 2, 1);
    let before = core.store.last_position();
    inspect(&core, &s, &t, true);
    assert_eq!(core.store.last_position(), before);
}

/// A turn with no `route.decided` row shows the target it started on.
#[test]
fn a_turn_nothing_routed_shows_the_target_it_started_on() {
    let core = test_core("ok");
    let s = open(&core);
    row(
        &core,
        T0,
        "turn.started",
        &s,
        "turn_plain",
        json!({"profile": "sonnet", "provider": "anthropic", "model": "claude-sonnet-5"}),
    );
    row(
        &core,
        T0 + 5,
        "turn.ended",
        &s,
        "turn_plain",
        json!({"loops": 0, "stop_reason": "end_turn", "elapsed_ms": 5}),
    );
    let r = inspect(&core, &s, "turn_plain", true);
    assert_eq!(r.route["model"], "claude-sonnet-5");
    assert_eq!(r.route["profile"], "sonnet");
    assert_eq!(r.outcome, "ended");
    assert!(r.loops.is_empty() && r.prev_turn.is_none() && r.next_turn.is_none());
}

/// A real turn's result carries the link built from `[cockpit] public_url`
/// (a relay's port, never a hard-coded one), and `turn.inspect` of that turn
/// answers the same link.
#[tokio::test]
async fn a_turns_result_links_to_it_at_the_configured_address() {
    let core = test_core_with("ok", |c| {
        c.cockpit.public_url = Some("http://localhost:17433".into());
    });
    let out = roundtrip(core.clone(), vec![submit(1, "hello")]).await;
    let Some(theseus_protocol::Message::Response(r)) = out
        .iter()
        .find(|m| matches!(m, theseus_protocol::Message::Response(_)))
    else {
        panic!("the turn answered nothing")
    };
    let res: theseus_protocol::TurnSubmitResult =
        serde_json::from_value(r.result.clone().expect("a result")).unwrap();
    let url = res.inspect_url.clone().expect("the result's link");
    assert_eq!(
        url,
        format!(
            "http://localhost:17433/session/{}/turn/{}",
            res.session_id, res.turn_id
        )
    );
    let seen = inspect(&core, &res.session_id, &res.turn_id, true);
    assert_eq!(seen.url, url);
    assert_eq!(seen.outcome, "ended");
    assert_eq!(seen.totals.model_calls, 1);
    assert!(seen.loops[0].answer.as_ref().unwrap().blocks[0]["text"].is_string());
    assert!(seen.loops[0].compiled.is_some(), "the loop's compile row");
    assert!(seen.trace.is_some(), "the turn's trace row");
}

/// With the web UI off and no `public_url`, `turn.inspect` answers no link: `theseus open turn`
/// then says so instead of printing a default-port address nothing serves (theseus-40cuo).
#[test]
fn no_web_ui_means_no_link() {
    let core = test_core_with("ok", |c| {
        c.web.enabled = false;
        c.web.port = 17433;
        c.cockpit.resolve(c.web.port, c.web.enabled);
    });
    let (s, t) = build(&core, 2, 1);
    assert_eq!(inspect(&core, &s, &t, true).url, "");
}

/// Over a connection the surface decides what a read holds: the Discord
/// binding's gets the shapes alone, the CLI's the texts (turn-inspector's join
/// review: the split was held below the protocol, and a planted revert that
/// read every surface as private passed every test).
#[tokio::test]
async fn the_connections_surface_decides_what_its_read_holds() {
    use crate::approval::{Client, Surface};
    let core = test_core("ok");
    let (s, t) = build(&core, 2, 1);
    let params = json!({"session_id": s, "turn_id": t});
    let read = |surface| {
        crate::tests_m3::rpc_as(
            &core,
            Client::new("test", surface),
            "turn.inspect",
            params.clone(),
        )
    };
    let other = read(Surface::Discord).await.unwrap();
    assert!(other["withheld"].is_string(), "{other}");
    assert!(!other.to_string().contains("harbour directory"), "{other}");
    let cli = read(Surface::Cli).await.unwrap();
    assert!(cli["withheld"].is_null(), "{cli}");
    assert!(cli.to_string().contains("harbour directory"));
}
