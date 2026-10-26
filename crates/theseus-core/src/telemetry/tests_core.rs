//! The corrections (theseus-yf1), through the whole core: a real turn's calls
//! reach the tool metrics, a failed continuation and a failed turn's tokens
//! are counted, and the wakes and language-server requests a turn made are
//! timed.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::Usage;
use tokio::io::AsyncWriteExt;

use super::tests::{
    attr, attrs_of, failed_trace, flushed, last_metrics, named, pipeline, point_with, points_of,
    result_with, s, spans_of, tool_span, tuning, Receiver, STATUS_CODE_ERROR,
};
use super::{spans, FailedTurn};

/// A core whose provider follows `script`, whose tools work in a scratch
/// folder holding `harbor.txt`, and whose telemetry posts to `endpoint`.
/// What the template leaves to enforcement (the writers and `proc.run`)
/// waits for the operator.
fn core_with(script: Vec<crate::provider::Scripted>, endpoint: &str) -> CoreRig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let root = root.canonicalize().unwrap();
    let mut c = crate::Config::example();
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    c.tools.roots = vec![];
    c.policy.enforcement = crate::policy::Posture::Approve;
    c.model.retries.transient = 0; // a failed call fails its turn: no retry inside it
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::scripted(script));
    let core = crate::Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(endpoint, None, tuning())),
        ..crate::rpc::Parts::for_tests(c, fake, store)
    })
    .unwrap();
    CoreRig { core, _dir: dir }
}

struct CoreRig {
    core: Arc<crate::Core>,
    _dir: tempfile::TempDir,
}

/// `turn.submit` through the protocol, as a client sends it: its answer.
async fn submit(core: &Arc<crate::Core>, input: &str) -> theseus_protocol::Response {
    use theseus_protocol::{method, Id, Message, Request, TurnSubmitParams};
    use tokio::io::{AsyncBufReadExt, BufReader};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let params = TurnSubmitParams {
        surface: None,
        dir: None,
        carried: false,
        prompt: None,
        session_id: None,
        input: input.into(),
        profile: None,
        provider: None,
        model: None,
        author: None,
        attachments: vec![],
        reply_to: None,
        opened_from: None,
        thread: None,
        speaking: None,
        in_reply_to: None,
        route_at_ms: None,
        deadline_secs: None,
        via: None,
    };
    let mut line =
        serde_json::to_string(&Request::new(Id::Num(1), method::TURN_SUBMIT, params)).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let answer = loop {
        let line = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&line).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop((cw, lines));
    let _ = srv.await;
    answer
}

/// A real turn's calls, through the gate and the tool runtime, reach the
/// tool metrics, and their spans, with the registered tool's name, family,
/// and backend and what became of each: a read that answered, a read that
/// failed, a tool that is not registered, and a program that waits for the
/// operator, which its span says and the metrics count at its answer.
#[tokio::test]
async fn a_turns_calls_reach_the_tool_metrics_as_the_runtime_ran_them() {
    use crate::provider::Scripted;
    let rx = Receiver::start(vec![]).await;
    let r = core_with(
        vec![
            Scripted::tools(
                "Looking.",
                &[
                    ("t1", "fs_read", json!({"path": "harbor.txt"})),
                    ("t2", "fs_read", json!({"path": "missing.txt"})),
                    ("t3", "frob_it", json!({})),
                ],
            ),
            Scripted::tools("", &[("t4", "proc_run", json!({"argv": ["echo", "tide"]}))]),
        ],
        &rx.endpoint(),
    );
    let answer = submit(&r.core, "what does harbor.txt say?").await;
    assert!(answer.error.is_none(), "{answer:?}");
    flushed(r.core.telemetry()).await;
    let got = rx.got();
    let metrics = last_metrics(&got);
    let want = [
        ["fs.read", "fs", "inproc", "ok"],
        ["fs.read", "fs", "inproc", "error"],
        ["frob_it", "unknown", "none", "error"],
    ];
    assert_eq!(
        points_of(&metrics, "theseus.tool.calls").len(),
        want.len(),
        "{metrics:#?}"
    );
    for [name, family, backend, outcome] in want {
        let with = [
            ("theseus.tool.name", name),
            ("theseus.tool.family", family),
            ("theseus.tool.backend", backend),
            ("theseus.tool.outcome", outcome),
            ("theseus.outcome", "complete"),
        ];
        assert_eq!(
            point_with(&metrics, "theseus.tool.calls", &with)["asInt"],
            "1"
        );
        assert_eq!(
            point_with(&metrics, "theseus.tool.duration_ms", &with)["count"],
            "1"
        );
    }
    let spans = spans_of(&got);
    let run = named(&spans, "tool proc_run");
    for (k, v) in [
        ("tool", "proc.run"),
        ("family", "proc"),
        ("backend", "job"),
        ("result", "awaiting_confirm"),
    ] {
        assert_eq!(attr(run, k), Some(&json!({ "stringValue": v })), "{k}");
    }
}

/// A continuation turn that fails (here the driver's retry of a 529) is
/// counted as the client's own failed turn is: in `theseus.turns` as failed
/// and in `theseus.provider.errors`, in the same series, and in health's
/// count. Until theseus-yf1 only `turn.submit` counted a failure. The retry
/// that answers is counted complete.
#[tokio::test]
async fn a_failed_continuation_is_counted_as_a_failed_turn_is() {
    use crate::provider::{ProviderError, Scripted};
    let overloaded = || {
        Scripted::Fail(ProviderError::Overloaded {
            message: "overloaded_error: Overloaded".into(),
        })
    };
    let rx = Receiver::start(vec![]).await;
    let r = core_with(
        vec![
            overloaded(),
            overloaded(),
            Scripted::text("The tide turns at four."),
        ],
        &rx.endpoint(),
    );
    let answer = submit(&r.core, "when does the tide turn?").await;
    let data = answer.error.expect("the provider is overloaded").data;
    assert_eq!(data["class"], "overloaded");
    let session: crate::session::SessionRecord = r
        .core
        .store
        .get_session(data["session_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let exec = session.execution_id.clone().unwrap();
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert!(e.resume_pending, "the retry waits for the driver: {e:?}");
    assert_eq!(r.core.health().provider_errors, 1);

    let failed = r
        .core
        .continue_execution(&exec)
        .await
        .expect_err("the retry fails the same way");
    assert_eq!(
        failed
            .downcast_ref::<crate::turn::TurnError>()
            .unwrap()
            .class,
        "overloaded"
    );
    assert_eq!(r.core.health().provider_errors, 2, "health counts it");
    let answered = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(answered.output, "The tide turns at four.");

    flushed(r.core.telemetry()).await;
    let got = rx.got();
    let metrics = last_metrics(&got);
    let failed = [("theseus.outcome", "failed")];
    assert_eq!(
        point_with(&metrics, "theseus.turns", &failed)["asInt"],
        "2",
        "the client's failed turn and the driver's, in one series"
    );
    assert_eq!(
        point_with(&metrics, "theseus.turn.duration_ms", &failed)["count"],
        "2"
    );
    let errors = points_of(&metrics, "theseus.provider.errors");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0]["asInt"], "2");
    let a = attrs_of(errors[0]);
    assert_eq!(
        (
            a["theseus.error.class"].as_str(),
            a["theseus.error.transient"].as_str()
        ),
        ("overloaded", "true")
    );
    let complete = [("theseus.outcome", "complete")];
    assert_eq!(
        point_with(&metrics, "theseus.turns", &complete)["asInt"],
        "1"
    );
    // Three turns, three traces, counted by trace id: a post answered after
    // the tuning's timeout is posted again, and a receiver counts both
    // (theseus-qjd6).
    let traces: std::collections::BTreeSet<String> = spans_of(&rx.at("/v1/traces"))
        .iter()
        .map(|s| s["traceId"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(traces.len(), 3, "each turn's trace: {traces:?}");
}

/// What a failed turn's finished loops spent is in `theseus.tokens` and
/// `theseus.cost.usd`, under outcome `failed` (theseus-b85w).
#[tokio::test]
async fn a_failed_turns_tokens_and_dollars_are_counted() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = failed_trace();
    let usage = Usage {
        input_tokens: 1_200,
        output_tokens: 340,
        cache_read_input_tokens: 5_000,
        ..Usage::default()
    };
    tel.record_failure(&FailedTurn {
        usage: &usage,
        cost_usd: Some(0.0425),
        ..failed_turn(&trace)
    });
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let failed = ("theseus.outcome", "failed");
    for (dir, n) in [("input", "1200"), ("output", "340"), ("cache_read", "5000")] {
        let p = point_with(
            &metrics,
            "theseus.tokens",
            &[failed, ("theseus.token.direction", dir)],
        );
        assert_eq!(p["asInt"], n, "{dir}");
    }
    assert_eq!(
        points_of(&metrics, "theseus.tokens").len(),
        3,
        "no cache_write"
    );
    let cost = point_with(&metrics, "theseus.cost.usd", &[failed]);
    assert_eq!(cost["asDouble"], json!(0.0425));
    assert_eq!(attrs_of(cost)["gen_ai.request.model"], "glm-5.1");
}

/// A tool span whose call failed has OTel's error status, though its
/// `outcome` is the call's Debug form; `unknown`, `cancelled`, and `declined`
/// are not failures of the call and stay unset (theseus-iu3a).
#[test]
fn a_failed_tool_calls_span_has_error_status() {
    let call = |wire: &str, outcome: &str, result: &str| {
        s(
            &format!("tool {wire}"),
            "tool",
            10,
            20,
            json!({"tool_use_id": "tu_1", "outcome": outcome, "tool": wire,
                "family": "fs", "backend": "inproc", "result": result}),
            vec![],
        )
    };
    let root = s(
        "turn",
        "turn",
        0,
        100,
        json!({"origin_unix_ms": 1u64, "outcome": "complete"}),
        vec![
            call("fs_ok", "Done { status: Ok }", "ok"),
            call("fs_bad", "Done { status: Error }", "error"),
            call("fs_maybe", "Done { status: Unknown }", "unknown"),
            call("fs_cut", "Done { status: Cancelled }", "cancelled"),
            call("fs_no", "Done { status: Declined }", "declined"),
            call(
                "fs_wait",
                "AwaitingConfirm { correlation_id: \"c\" }",
                "awaiting_confirm",
            ),
        ],
    );
    let out = spans::spans(&root);
    let status = |name: &str| {
        out.iter()
            .find(|x| x.name == name)
            .unwrap()
            .status
            .as_ref()
            .map(|st| (i64::from(st.code), st.message.clone()))
    };
    assert_eq!(
        status("tool fs_bad"),
        Some((STATUS_CODE_ERROR, "failed".into()))
    );
    for ok in [
        "turn",
        "tool fs_ok",
        "tool fs_maybe",
        "tool fs_cut",
        "tool fs_no",
        "tool fs_wait",
    ] {
        assert_eq!(status(ok), None, "{ok}");
    }
}

/// Every provider call's first token is a point of `theseus.provider.first_token_ms`,
/// from its own span's mark, with the provider and model the call's time
/// has; a call with no first token (a failure) has none (theseus-8u02).
#[tokio::test]
async fn every_provider_calls_first_token_is_recorded() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let call = |start: u64, end: u64, provider: &str, model: &str, token: Option<u64>| {
        let marks = token
            .map(|at| {
                vec![s(
                    "first_token",
                    "mark",
                    start + at,
                    start + at,
                    Value::Null,
                    vec![],
                )]
            })
            .unwrap_or_default();
        s(
            "provider.call",
            "provider",
            start,
            end,
            json!({"provider": provider, "model": model}),
            marks,
        )
    };
    let trace = s(
        "turn",
        "turn",
        0,
        9_000_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![
            s(
                "loop 0",
                "loop",
                0,
                4_000_000,
                Value::Null,
                vec![call(100, 3_000_000, "zai", "glm-5.1", Some(400_000))],
            ),
            s(
                "loop 1",
                "loop",
                4_000_000,
                9_000_000,
                Value::Null,
                vec![
                    call(4_000_100, 5_000_000, "zai", "glm-5.1", Some(900_000)),
                    call(5_000_100, 6_000_000, "anthropic", "claude-x", Some(250_000)),
                    call(6_000_100, 7_000_000, "anthropic", "claude-x", None),
                ],
            ),
        ],
    );
    let mut r = result_with(trace);
    // The result's own figure, the last call's, is no longer what is recorded.
    r.first_token_ms = Some(250);
    tel.record_turn(&r);
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let name = "theseus.provider.first_token_ms";
    assert_eq!(points_of(&metrics, name).len(), 2);
    let zai = point_with(&metrics, name, &[("gen_ai.provider.name", "zai")]);
    assert_eq!(
        attrs_of(zai),
        BTreeMap::from([
            ("gen_ai.provider.name".to_string(), "zai".to_string()),
            ("gen_ai.request.model".to_string(), "glm-5.1".to_string()),
        ])
    );
    assert_eq!(
        (&zai["count"], &zai["min"], &zai["max"], &zai["sum"]),
        (&json!("2"), &json!(400.0), &json!(900.0), &json!(1300.0))
    );
    let anthropic = point_with(&metrics, name, &[("gen_ai.provider.name", "anthropic")]);
    assert_eq!(attrs_of(anthropic)["gen_ai.request.model"], "claude-x");
    assert_eq!(
        (&anthropic["count"], &anthropic["sum"]),
        (&json!("1"), &json!(250.0)),
        "the call with no first token has none"
    );
}

/// Each wake a turn took is counted by whether it repeats, and its lateness
/// is a histogram by the same (37a): `theseus.wakes.fired{repeat}` and
/// `theseus.wakes.late_ms`, from the `wake.fired` points on the trace.
#[tokio::test]
async fn the_wakes_a_turn_took_are_counted_by_repeat_with_their_lateness() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let fired = |at: u64, repeat: bool, late_ms: u64| {
        s(
            "wake.fired",
            "wake",
            at,
            at,
            json!({"wake_id": "wak_x", "repeat": repeat, "late_ms": late_ms}),
            vec![],
        )
    };
    let trace = s(
        "turn",
        "turn",
        0,
        9_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![
            s(
                "continuation",
                "tool",
                0,
                100,
                Value::Null,
                vec![
                    fired(10, true, 120),
                    fired(11, true, 61_000),
                    fired(12, false, 40),
                ],
            ),
            s("loop 0", "loop", 200, 1_000, Value::Null, vec![]),
        ],
    );
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let name = "theseus.wakes.fired";
    assert_eq!(points_of(&metrics, name).len(), 2, "one series a kind");
    let repeating = point_with(&metrics, name, &[("theseus.wake.repeat", "true")]);
    assert_eq!(repeating["asInt"], "2");
    assert_eq!(attrs_of(repeating).len(), 1, "repeat alone");
    let once = point_with(&metrics, name, &[("theseus.wake.repeat", "false")]);
    assert_eq!(once["asInt"], "1");
    let late = point_with(
        &metrics,
        "theseus.wakes.late_ms",
        &[("theseus.wake.repeat", "true")],
    );
    assert_eq!(
        (&late["count"], &late["min"], &late["max"], &late["sum"]),
        (&json!("2"), &json!(120.0), &json!(61000.0), &json!(61120.0))
    );
}

/// Each language-server request a tool call made (L2), its `lsp.request`
/// span under the call's, is timed in `theseus.lsp.request.duration`, by its
/// server, method, and outcome.
#[tokio::test]
async fn a_calls_lsp_requests_are_timed_by_server_method_and_outcome() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let req = |start: u64, end: u64, method: &str, outcome: &str| {
        crate::fact::lsp::request_span("rust-analyzer", method, outcome, start, end)
    };
    let mut call = tool_span(
        "lsp_definition",
        100,
        9_000,
        ["lsp.definition", "lsp", "async", "ok"],
    );
    call.children = vec![
        req(200, 2_200, "textDocument/definition", "ok"),
        req(3_000, 4_000, "textDocument/definition", "ok"),
        req(5_000, 8_000, "textDocument/references", "timeout"),
    ];
    let trace = s(
        "turn",
        "turn",
        0,
        10_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![call],
    );
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let name = "theseus.lsp.request.duration";
    assert_eq!(
        points_of(&metrics, name).len(),
        2,
        "one series a method and outcome"
    );
    let def = point_with(&metrics, name, &[("lsp.method", "textDocument/definition")]);
    assert_eq!(
        attrs_of(def),
        BTreeMap::from([
            (
                "lsp.method".to_string(),
                "textDocument/definition".to_string()
            ),
            ("lsp.server".to_string(), "rust-analyzer".to_string()),
            ("theseus.outcome".to_string(), "ok".to_string()),
        ])
    );
    assert_eq!((&def["count"], &def["sum"]), (&json!("2"), &json!(3.0)));
    let refs = point_with(&metrics, name, &[("theseus.outcome", "timeout")]);
    assert_eq!((&refs["count"], &refs["sum"]), (&json!("1"), &json!(3.0)));
}
