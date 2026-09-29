use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{
    error_code, method, Id, LedgerTailParams, LedgerTailResult, Message, ProfileListResult,
    ProfileUseParams, Request, Response, SessionKind, SessionListResult, SessionOpenParams,
    TurnSubmitParams,
};

use super::*;
use crate::node::{Body, Node};
use crate::provider::{FakeProvider, Provider};
use crate::turn::OPERATOR;
use theseus_kernel::Authority;
use theseus_protocol::{notify, Notification, TurnSubmitResult};
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

fn test_core(reply: &str) -> Arc<Core> {
    let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let secrets = SecretBoard::new(["anthropic_api_key".to_string()], std::time::Instant::now());
    secrets.publish(
        [(
            "anthropic_api_key".to_string(),
            Ok(crate::secrets::Secret::new("sk-test-not-a-key".into())),
        )]
        .into(),
        "fake",
    );
    Core::build(Parts {
        secrets,
        ..Parts::for_tests(
            cfg,
            Arc::new(FakeProvider {
                reply: reply.into(),
                ..Default::default()
            }),
            store,
        )
    })
    .unwrap()
}

/// Drive a connection over an in-memory duplex: returns (lines received) after `n_requests` responses.
async fn roundtrip(core: Arc<Core>, requests: Vec<Request>) -> Vec<Message> {
    let (client, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let want = requests.len();
    for r in requests {
        let mut line = serde_json::to_string(&r).unwrap();
        line.push('\n');
        cw.write_all(line.as_bytes()).await.unwrap();
    }
    let mut lines = BufReader::new(cr).lines();
    let mut got = Vec::new();
    let mut responses = 0;
    while responses < want {
        let line = lines.next_line().await.unwrap().unwrap();
        let m: Message = serde_json::from_str(&line).unwrap();
        if matches!(m, Message::Response(_)) {
            responses += 1;
        }
        got.push(m);
    }
    cw.shutdown().await.unwrap();
    drop(cw);
    drop(lines);
    let _ = srv.await;
    got
}

fn responses(msgs: &[Message]) -> Vec<&Response> {
    msgs.iter()
        .filter_map(|m| match m {
            Message::Response(r) => Some(r),
            _ => None,
        })
        .collect()
}
fn notifications(msgs: &[Message]) -> Vec<&Notification> {
    msgs.iter()
        .filter_map(|m| match m {
            Message::Notification(n) => Some(n),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn continuations_wait_for_expected_bindings_but_not_forever() {
    let core = test_core("x");
    assert!(
        core.bindings.wait(Duration::from_millis(10)).await,
        "none expected"
    );
    core.bindings.expect();
    assert!(
        !core.bindings.wait(Duration::from_millis(120)).await,
        "times out while a binding is still starting"
    );
    let c = core.clone();
    let waiter = tokio::spawn(async move { c.bindings.wait(Duration::from_secs(5)).await });
    tokio::time::sleep(Duration::from_millis(80)).await;
    core.bindings.started();
    assert!(waiter.await.unwrap(), "released when the binding starts");
    core.bindings.started(); // a second report never underflows
    assert!(core.bindings.wait(Duration::from_millis(10)).await);
}

// ---------------------------------------------------------------- serve first (theseus-qa0)

/// A core whose secrets resolve from `vault` in the background, as the
/// daemon's do: `anthropic_api_key` (the test provider's) and one more.
fn serving_core(
    vault: crate::secrets::fake::FakeVault,
    reply: &str,
) -> (Arc<Core>, Arc<SecretBoard>) {
    let refs: BTreeMap<String, String> = [
        ("anthropic_api_key", "op://V/anthropic/notesPlain"),
        ("github_token", "op://V/github/notesPlain"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let board = SecretBoard::new(refs.keys().cloned(), std::time::Instant::now());
    tokio::spawn(crate::secrets::resolve_into(
        board.clone(),
        refs,
        Arc::new(vault),
    ));
    let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let core = Core::build(Parts {
        secrets: board.clone(),
        ..Parts::for_tests(
            cfg,
            Arc::new(FakeProvider {
                reply: reply.into(),
                ..Default::default()
            }),
            store,
        )
    })
    .unwrap();
    (core, board)
}

fn submit(id: u64, input: &str) -> Request {
    Request::new(
        Id::Num(id),
        method::TURN_SUBMIT,
        TurnSubmitParams {
            session_id: None,
            input: input.into(),
            profile: None,
            provider: None,
            model: None,
            author: None,
            attachments: vec![],
        },
    )
}

/// With a vault that never answers, `health` answers inside the cold-start
/// budget (§9, 50 ms), from the request to its answer, and says the secrets
/// are resolving. The process-level budget is the lifecycle bench's, on a
/// release build (`theseus-sim bench lifecycle`).
#[tokio::test]
async fn health_answers_at_once_while_a_hung_vault_resolves() {
    let (vault, _never) = crate::secrets::fake::FakeVault::gated(&[]);
    let (core, _) = serving_core(vault, "x");
    let (client, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let mut lines = BufReader::new(cr).lines();
    let req = Request::new(Id::Num(1), method::HEALTH, Value::Null);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    let t0 = std::time::Instant::now();
    cw.write_all(line.as_bytes()).await.unwrap();
    let answer = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
            break r;
        }
    };
    let took = t0.elapsed();
    let h = answer.result.clone().unwrap();
    assert!(took < Duration::from_millis(50), "health took {took:?}");
    assert_eq!(h["secrets"]["state"], "resolving", "{h}");
    assert_eq!(h["secrets"]["resolving"].as_array().unwrap().len(), 2);
    assert_eq!(h["secrets_resolved"], json!([]));
    cw.shutdown().await.unwrap();
    drop((cw, lines));
    let _ = srv.await;
}

/// A turn that arrives before its key waits for it, then runs; its trace
/// and the startup phases say how long it waited.
#[tokio::test]
async fn a_turn_waits_for_its_secret_then_runs() {
    let (vault, open) = crate::secrets::fake::FakeVault::gated(&[
        ("op://V/anthropic/notesPlain", "sk-test-0000000000"),
        ("op://V/github/notesPlain", "github-test-000000"),
    ]);
    let (core, board) = serving_core(vault, "after the wait");
    let c = core.clone();
    let turn = tokio::spawn(async move { roundtrip(c, vec![submit(3, "hi")]).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!turn.is_finished(), "the turn waits for its key");
    assert_eq!(board.status().state, "resolving");
    let opened = core.startup_log.us(std::time::Instant::now());
    open.send(true).unwrap();
    let msgs = tokio::time::timeout(Duration::from_secs(10), turn)
        .await
        .unwrap()
        .unwrap();
    let r = responses(&msgs)[0];
    let result: TurnSubmitResult = serde_json::from_value(r.result.clone().unwrap()).unwrap();
    assert_eq!(result.output, "after the wait");
    let trace = serde_json::to_string(&result.trace).unwrap();
    assert!(trace.contains("secrets.wait"), "{trace}");
    let phases = core.startup_log.snapshot();
    let waited = phases
        .iter()
        .find(|p| p.name == format!("provider.{}", core.cfg.model.provider))
        .unwrap();
    // The wait began before the vault answered and ended after it did.
    assert!(
        waited.start_us < opened && waited.end_us.unwrap() >= opened,
        "{waited:?}, vault opened at {opened} µs"
    );
    assert_eq!(waited.detail["outcome"], "ready");
}

/// A key the vault does not have: health names it with the reason, the
/// ledger says so, and a turn is refused with a clear class. Nothing ran
/// without the key.
#[tokio::test]
async fn a_failed_secret_is_named_and_its_turn_refused_with_a_class() {
    let vault =
        crate::secrets::fake::FakeVault::new(&[("op://V/github/notesPlain", "github-test-000000")]);
    let (core, board) = serving_core(vault, "never");
    tokio::spawn(core.clone().watch_secrets());
    board.wait_settled(Duration::from_secs(5)).await.unwrap();
    let h = core.health();
    assert_eq!(h.secrets.summary(), "failed anthropic_api_key");
    assert!(h.secrets.failed[0].error.contains("no item at"), "{h:?}");
    assert_eq!(h.secrets_resolved, ["github_token"]);
    let msgs = roundtrip(core.clone(), vec![submit(4, "hi")]).await;
    let err = responses(&msgs)[0].error.clone().unwrap();
    assert_eq!(err.code, error_code::PROVIDER);
    assert_eq!(err.data["class"], "secret_failed", "{err:?}");
    assert!(err.message.contains("anthropic_api_key"), "{}", err.message);
    let kinds: Vec<String> = core
        .store
        .ledger_tail::<LedgerRow>(50)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r.kind)
        .collect();
    assert!(kinds.iter().any(|k| k == "turn.refused"), "{kinds:?}");
    for _ in 0..50 {
        if core
            .store
            .ledger_tail::<LedgerRow>(50)
            .unwrap()
            .iter()
            .any(|(_, r)| r.kind == "secrets.failed")
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no secrets.failed row");
}

#[tokio::test]
async fn health_and_unknown_method() {
    let core = test_core("x");
    let msgs = roundtrip(
        core,
        vec![
            Request::new(Id::Num(1), method::HEALTH, Value::Null),
            Request::new(Id::Num(2), "nope.nothing", Value::Null),
        ],
    )
    .await;
    let rs = responses(&msgs);
    let health = rs.iter().find(|r| r.id == Id::Num(1)).unwrap();
    assert_eq!(health.result.as_ref().unwrap()["name"], "theseus");
    let nope = rs.iter().find(|r| r.id == Id::Num(2)).unwrap();
    assert_eq!(
        nope.error.as_ref().unwrap().code,
        error_code::METHOD_NOT_FOUND
    );
}

#[tokio::test]
async fn one_turn_is_one_loop_with_streamed_deltas() {
    let core = test_core("hello there friend");
    let msgs = roundtrip(
        core.clone(),
        vec![Request::new(
            Id::Num(7),
            method::TURN_SUBMIT,
            TurnSubmitParams {
                session_id: None,
                input: "hi".into(),
                profile: None,
                provider: None,
                model: None,
                author: None,
                attachments: vec![],
            },
        )],
    )
    .await;
    let ns: Vec<&str> = notifications(&msgs)
        .iter()
        .map(|n| n.method.as_str())
        .collect();
    assert_eq!(ns.first(), Some(&notify::TURN_STARTED));
    // The input becomes a node, the context compiles, the loop starts.
    let pos = |m: &str| {
        ns.iter()
            .position(|x| *x == m)
            .unwrap_or_else(|| panic!("no {m} in {ns:?}"))
    };
    assert!(pos(notify::NODE_WRITTEN) < pos(notify::CONTEXT_COMPILED));
    assert!(pos(notify::CONTEXT_COMPILED) < pos(notify::LOOP_STARTED));
    assert!(pos(notify::LOOP_STARTED) < pos(notify::MODEL_DELTA));
    assert!(ns.iter().filter(|m| **m == notify::MODEL_DELTA).count() >= 2);
    assert_eq!(ns[ns.len() - 2], notify::LOOP_ENDED);
    assert_eq!(ns[ns.len() - 1], notify::TURN_ENDED);
    let streamed: String = notifications(&msgs)
        .iter()
        .filter(|n| n.method == notify::MODEL_DELTA)
        .map(|n| n.params["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(streamed, "hello there friend");

    let r = responses(&msgs)[0];
    let result: TurnSubmitResult = serde_json::from_value(r.result.clone().unwrap()).unwrap();
    assert_eq!(result.loops, 1);
    assert_eq!(result.stop_reason, "no_tool_calls");
    assert_eq!(result.output, "hello there friend");
    // The exchange is content now: a user node and an assistant node.
    let nodes = core.store.session_nodes(&result.session_id).unwrap();
    let kinds: Vec<&str> = nodes.iter().map(|(_, n)| n.kind_str()).collect();
    assert_eq!(kinds, vec!["user_message", "assistant_message"]);
    assert_eq!(result.provider_stop_reason.as_deref(), Some("end_turn"));

    // No hook rows or hook spans (theseus-hco removed the hook system).
    let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(200).unwrap();
    assert!(!rows.iter().any(|(_, r)| r.kind.starts_with("hook")));
    // The trace: turn > loop 0 > provider.call > first_token.
    let tr = result.trace.as_ref().expect("trace");
    assert!(!serde_json::to_string(tr)
        .unwrap()
        .contains(r#""kind":"hook""#));
    assert_eq!(tr.name, "turn");
    assert!(tr.end_us.is_some());
    let names: Vec<&str> = tr.children.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"admission.wait"));
    assert!(names.contains(&"loop 0"));
    assert!(names.contains(&"session.write"));
    let lp = tr.children.iter().find(|c| c.name == "loop 0").unwrap();
    let lnames: Vec<&str> = lp.children.iter().map(|c| c.name.as_str()).collect();
    assert!(lnames.contains(&"compile"));
    assert!(lnames.contains(&"provider.call"));
    assert!(lnames.contains(&"advancer"));
    let pc = lp
        .children
        .iter()
        .find(|c| c.name == "provider.call")
        .unwrap();
    assert!(pc.children.iter().any(|m| m.name == "first_token"));
    assert_eq!(pc.attrs["request_id"], "req_fake");
    assert!(rows.iter().any(|(_, r)| r.kind == "turn.trace"));
    assert_eq!(core.health().turns, 1);
    assert_eq!(core.health().sessions, 1);
}

#[tokio::test]
async fn rejects_empty_input_and_unknown_session() {
    let core = test_core("x");
    let msgs = roundtrip(
        core,
        vec![
            Request::new(
                Id::Num(1),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: None,
                    input: "   ".into(),
                    profile: None,
                    provider: None,
                    model: None,
                    author: None,
                    attachments: vec![],
                },
            ),
            Request::new(
                Id::Num(2),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: Some("ses_nope".into()),
                    input: "hi".into(),
                    profile: None,
                    provider: None,
                    model: None,
                    author: None,
                    attachments: vec![],
                },
            ),
        ],
    )
    .await;
    let rs = responses(&msgs);
    let e1 = rs
        .iter()
        .find(|r| r.id == Id::Num(1))
        .unwrap()
        .error
        .as_ref()
        .unwrap();
    assert_eq!(e1.code, error_code::INVALID_PARAMS);
    let e2 = rs
        .iter()
        .find(|r| r.id == Id::Num(2))
        .unwrap()
        .error
        .as_ref()
        .unwrap();
    assert_eq!(e2.code, error_code::NOT_FOUND);
}

/// Rows written by the hook system (removed in theseus-hco) stay in old
/// stores. They still read through `ledger.tail`, which `theseus ledger`
/// and the Observatory use, and an old trace's hook spans still decode.
#[tokio::test]
async fn rows_from_the_hook_system_still_read() {
    let core = test_core("ok");
    // Byte for byte what the hook system wrote.
    let old = [
        r#"{"at_unix_ms":1759100000000,"kind":"server.started","data":{"hooks":{"event":"server.started","kind":"observe","handlers":0,"outcome":"proceed"},"startup":{}}}"#,
        r#"{"at_unix_ms":1759100000001,"kind":"session.opened","session_id":"ses_old","data":{"event":"session.opened","kind":"observe","handlers":0,"outcome":"proceed"}}"#,
        r#"{"at_unix_ms":1759100000002,"kind":"hooks.registered","data":{"event":"turn_ended","handler_id":"cli-watch","client":"cli#1"}}"#,
        r#"{"at_unix_ms":1759100000003,"kind":"hook.site","session_id":"ses_old","turn_id":"turn_old","data":{"event":"turn.starting","kind":"gate","handlers":0,"outcome":"proceed"}}"#,
        r#"{"at_unix_ms":1759100000004,"kind":"turn.trace","session_id":"ses_old","turn_id":"turn_old","data":{"name":"turn","kind":"turn","start_us":0,"end_us":900,"children":[{"name":"turn.starting","kind":"hook","start_us":10,"end_us":12,"attrs":{"kind":"gate","handlers":0,"outcome":"proceed"}}]}}"#,
    ];
    for row in old {
        let row: Value = serde_json::from_str(row).unwrap();
        core.store.append_ledger(&row).unwrap();
    }
    let tail = |id, kind: Option<&str>| {
        Request::new(
            Id::Num(id),
            method::LEDGER_TAIL,
            LedgerTailParams {
                n: Some(10),
                kind: kind.map(str::to_string),
                session_id: None,
            },
        )
    };
    let msgs = roundtrip(
        core,
        vec![
            tail(1, None),
            tail(2, Some("hook.site")),
            Request::new(Id::Num(3), "hooks.list", Value::Null),
        ],
    )
    .await;
    let rs = responses(&msgs);
    let result = |id| -> LedgerTailResult {
        let r = rs.iter().find(|r| r.id == Id::Num(id)).unwrap();
        serde_json::from_value(r.result.clone().unwrap()).unwrap()
    };
    let all = result(1);
    let kinds: Vec<&str> = all.rows.iter().map(|r| r.kind.as_str()).collect();
    for k in [
        "server.started",
        "session.opened",
        "hooks.registered",
        "hook.site",
        "turn.trace",
    ] {
        assert!(kinds.contains(&k), "{k} missing from {kinds:?}");
    }
    let sites = result(2).rows;
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].data["event"], "turn.starting");
    assert_eq!(sites[0].turn_id.as_deref(), Some("turn_old"));
    let trace = all.rows.iter().find(|r| r.kind == "turn.trace").unwrap();
    let span: theseus_protocol::Span = serde_json::from_value(trace.data.clone()).unwrap();
    assert_eq!(span.children[0].kind, "hook");
    // An old client asking for the hook list is told plainly.
    let gone = rs.iter().find(|r| r.id == Id::Num(3)).unwrap();
    assert_eq!(
        gone.error.as_ref().unwrap().code,
        error_code::METHOD_NOT_FOUND
    );
}

#[tokio::test]
async fn same_session_serializes_turns() {
    let core = test_core("r");
    let open = roundtrip(
        core.clone(),
        vec![Request::new(
            Id::Num(1),
            method::SESSION_OPEN,
            SessionOpenParams::default(),
        )],
    )
    .await;
    let sid = responses(&open)[0].result.as_ref().unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let reqs = (0..3)
        .map(|i| {
            Request::new(
                Id::Num(10 + i),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: Some(sid.clone()),
                    input: format!("turn {i}"),
                    profile: None,
                    provider: None,
                    model: None,
                    author: None,
                    attachments: vec![],
                },
            )
        })
        .collect();
    let msgs = roundtrip(core.clone(), reqs).await;
    assert_eq!(responses(&msgs).len(), 3);
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.turns, 3);
    // turn.started/turn.ended never interleave: each started is followed by its own ended.
    let seq: Vec<&str> = notifications(&msgs)
        .iter()
        .filter(|n| n.method == notify::TURN_STARTED || n.method == notify::TURN_ENDED)
        .map(|n| n.method.as_str())
        .collect();
    assert_eq!(seq, [notify::TURN_STARTED, notify::TURN_ENDED].repeat(3));
}

#[tokio::test]
async fn provider_failure_is_classified_and_ledgered() {
    let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let core = Core::build(Parts::for_tests(
        Config::example(),
        Arc::new(FakeProvider {
            fail_with: Some(crate::provider::ProviderError::Timeout {
                phase: crate::provider::TimeoutPhase::StreamIdle,
                elapsed_ms: 61_000,
            }),
            ..Default::default()
        }),
        store,
    ))
    .unwrap();
    let msgs = roundtrip(
        core.clone(),
        vec![Request::new(
            Id::Num(1),
            method::TURN_SUBMIT,
            TurnSubmitParams {
                session_id: None,
                input: "hi".into(),
                profile: None,
                provider: None,
                model: None,
                author: None,
                attachments: vec![],
            },
        )],
    )
    .await;
    let r = responses(&msgs)[0];
    let e = r.error.as_ref().expect("error response");
    assert_eq!(e.code, error_code::PROVIDER);
    assert_eq!(e.data["class"], "timeout");
    assert_eq!(e.data["transient"], true);
    assert_eq!(e.data["usage_unknown"], true);
    assert!(e.message.contains("stream_idle") || e.message.to_lowercase().contains("timeout"));
    let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(50).unwrap();
    assert!(rows
        .iter()
        .any(|(_, r)| r.kind == "provider.error" && r.data["class"] == "timeout"));
    assert!(rows.iter().any(|(_, r)| r.kind == "turn.failed"));
    let tr = &e.data["trace"];
    assert_eq!(tr["name"], "turn");
    assert_eq!(tr["attrs"]["outcome"], "failed");
    assert_eq!(core.health().provider_errors, 1);
    // The turn still counted and the session record was written.
    assert_eq!(core.health().turns, 1);
}

#[tokio::test]
async fn usage_accumulates_per_session_and_globally() {
    let core = test_core("one two three");
    let open = roundtrip(
        core.clone(),
        vec![Request::new(
            Id::Num(1),
            method::SESSION_OPEN,
            SessionOpenParams::default(),
        )],
    )
    .await;
    let sid = responses(&open)[0].result.as_ref().unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let reqs = (0..2)
        .map(|i| {
            Request::new(
                Id::Num(10 + i),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: Some(sid.clone()),
                    input: "a b".into(),
                    profile: None,
                    provider: None,
                    model: None,
                    author: None,
                    attachments: vec![],
                },
            )
        })
        .collect();
    let msgs = roundtrip(core.clone(), reqs).await;
    let mut rs: Vec<TurnSubmitResult> = responses(&msgs)
        .iter()
        .map(|r| serde_json::from_value(r.result.clone().unwrap()).unwrap())
        .collect();
    rs.sort_by_key(|r| r.usage.input_tokens);
    assert_eq!(rs[0].usage.output_tokens, 3);
    // The second turn carries the first exchange: the session has memory.
    assert!(
        rs[1].usage.input_tokens > rs[0].usage.input_tokens,
        "{rs:?}"
    );
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(
        rec.usage.input_tokens,
        rs[0].usage.input_tokens + rs[1].usage.input_tokens
    );
    assert_eq!(rec.usage.output_tokens, 6);
    assert_eq!(rec.turns, 2);
    let h = core.health();
    assert_eq!(h.usage_total.output_tokens, 6);
    assert_eq!(h.turns, 2);
    let tail = roundtrip(
        core.clone(),
        vec![Request::new(
            Id::Num(99),
            method::LEDGER_TAIL,
            LedgerTailParams {
                n: Some(5),
                kind: Some("provider.call".into()),
                session_id: None,
            },
        )],
    )
    .await;
    let t: LedgerTailResult =
        serde_json::from_value(responses(&tail)[0].result.clone().unwrap()).unwrap();
    assert_eq!(t.rows.len(), 2);
    assert!(t.rows.iter().all(|r| r.kind == "provider.call"));
}

/// A turn that fails after its first loop still books that loop: its usage,
/// cost, and tool call reach the session, the `turn.failed` row, and
/// `error.data`. The provider fails in loop 1. (A budget that runs out in
/// loop 1 no longer fails the turn: it asks, see `tests_m3`.) The failure
/// says its cause once (theseus-woy).
#[tokio::test]
async fn a_turn_that_fails_after_its_first_loop_keeps_that_loops_books() {
    use crate::provider::{ProviderError, Scripted};
    let mut wrong = Vec::new();
    for exit in ["provider"] {
        let first = Scripted::tools(
            &"word ".repeat(30_000),
            &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        );
        let second = match exit {
            "provider" => Scripted::Fail(ProviderError::Overloaded {
                message: "busy".into(),
            }),
            _ => Scripted::text("never asked"),
        };
        let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
        let store = Store::open(&dir.join("store")).unwrap();
        let fake = FakeProvider {
            chunk: usize::MAX,
            ..FakeProvider::scripted(vec![first, second])
        };
        let core = Core::build(Parts::for_tests(Config::example(), Arc::new(fake), store)).unwrap();
        let limit = None;
        let mut rec = SessionRecord::new(SessionKind::Conversation, None);
        let authority = Authority {
            principal: OPERATOR.into(),
            ..Default::default()
        };
        let e = core
            .kernel
            .open_execution(&rec.session_id, rec.kind, authority, limit, None)
            .unwrap();
        rec.execution_id = Some(e.id);
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let msgs = roundtrip(
            core.clone(),
            vec![Request::new(
                Id::Num(1),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: Some(sid.clone()),
                    input: "diff these".into(),
                    profile: None,
                    provider: None,
                    model: None,
                    author: None,
                    attachments: vec![],
                },
            )],
        )
        .await;
        let err = responses(&msgs)[0].error.clone().expect("the turn fails");
        let said: Vec<String> = notifications(&msgs)
            .iter()
            .filter(|n| n.method == notify::TURN_FAILED)
            .map(|n| n.params["error"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(
            said[0].matches("busy").count(),
            1,
            "the cause once: {said:?}"
        );
        assert!(
            !said[0].contains("failed ("),
            "the class rides beside it: {said:?}"
        );
        assert_eq!(err.message.matches("busy").count(), 1, "{}", err.message);
        let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(500).unwrap();
        let row = |kind: &str| {
            rows.iter()
                .find(|(_, r)| r.kind == kind)
                .map(|(_, r)| r.data.clone())
                .unwrap_or_default()
        };
        let (call, failed) = (row("provider.call"), row("turn.failed"));
        let (usage, cost) = (call["usage"].clone(), call["cost_usd"].clone());
        assert!(
            cost.as_f64().unwrap_or(0.0) > 0.0,
            "{exit}: loop 0 has a cost: {call}"
        );
        let s: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
        let class = "overloaded";
        for (what, got, want) in [
            ("error.data class", err.data["class"].clone(), json!(class)),
            ("session turns", json!(s.turns), json!(1)),
            ("session usage", json!(s.usage), usage.clone()),
            ("session cost_usd", json!(s.cost_usd), cost.clone()),
            ("session tool_calls", json!(s.tool_calls), json!(1)),
            (
                "health cost_usd_total",
                json!(core.health().cost_usd_total),
                cost.clone(),
            ),
            (
                "turn.failed usage_so_far",
                failed["usage_so_far"].clone(),
                usage.clone(),
            ),
            (
                "turn.failed cost_usd",
                failed["cost_usd"].clone(),
                cost.clone(),
            ),
            (
                "turn.failed tool_calls",
                failed["tool_calls"].clone(),
                json!(1),
            ),
            ("error.data usage", err.data["usage"].clone(), usage),
            ("error.data cost_usd", err.data["cost_usd"].clone(), cost),
            (
                "error.data tool_calls",
                err.data["tool_calls"].clone(),
                json!(1),
            ),
        ] {
            if got != want {
                wrong.push(format!("{exit}: {what} = {got}, want {want}"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "the failed turn lost:\n{}",
        wrong.join("\n")
    );
}

/// A store the previous binary wrote, whose executions carry unit budgets
/// (theseus-0sg): one ended `budget_exhausted` at 877,683 of 1,000,000
/// units, as Eddie's Discord session did on 2026-09-29 for $0.45, and one
/// waits on input. It serves at once. `session.list`, `session.history`,
/// and `execution.list` answer; the ended one stays ended, now read in
/// dollars at the configured limit with its units kept; the waiting one
/// takes its session's recorded cost as its spend, and its next turn runs.
#[tokio::test]
async fn a_store_with_unit_budgets_serves_and_its_sessions_list_and_read() {
    use theseus_store::{kinds, NewRecord};
    let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let mut ended = SessionRecord::new(SessionKind::Conversation, None);
    ended.cost_usd = 0.45;
    ended.turns = 15;
    ended.execution_id = Some("exe_old_exhausted".into());
    let mut open = SessionRecord::new(SessionKind::Conversation, None);
    open.cost_usd = 0.012;
    open.turns = 3;
    open.execution_id = Some("exe_old_waiting".into());
    store.put_session(&ended.session_id, &ended).unwrap();
    store.put_session(&open.session_id, &open).unwrap();
    let exec = |id: &str, session: &str, rest: &str| {
        let json = format!(
            r#"{{"id":"{id}","schema":1,"session_id":"{session}","kind":"conversation","authority":{{"principal":"operator","ceilings":{{}}}},"outstanding":[],"queued_results":[],"turns":3,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000500000,{rest}}}"#
        );
        let v: Value = serde_json::from_str(&json).unwrap();
        NewRecord::json(kinds::EXECUTION, Some(id), &v)
            .unwrap()
            .scoped(session)
    };
    store
        .append(&[
            exec(
                "exe_old_exhausted",
                &ended.session_id,
                r#""state":"budget_exhausted","ended_reason":"action provider.messages needs 172068 units, 112317 available","budget":{"limit":1000000,"spent":877683,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}}"#,
            ),
            exec(
                "exe_old_waiting",
                &open.session_id,
                r#""state":"waiting","wake":{"on":"input"},"budget":{"limit":20000000,"spent":154321,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}}"#,
            ),
            crate::node::Node::user(&open.session_id, Some("turn_old"), "discord:eddie", "hello")
                .record()
                .unwrap(),
        ])
        .unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let fake = FakeProvider {
        reply: "Still here.".into(),
        ..Default::default()
    };
    let core = Core::build(Parts::for_tests(cfg, Arc::new(fake), store)).unwrap();
    let history = |id: u64, session: &str| {
        Request::new(
            Id::Num(id),
            method::SESSION_HISTORY,
            theseus_protocol::SessionHistoryParams {
                session_id: session.into(),
                n: None,
            },
        )
    };
    let msgs = roundtrip(
        core.clone(),
        vec![
            Request::new(Id::Num(1), method::SESSION_LIST, Value::Null),
            history(2, &ended.session_id),
            history(3, &open.session_id),
            Request::new(Id::Num(4), method::EXECUTION_LIST, Value::Null),
        ],
    )
    .await;
    let rs = responses(&msgs);
    assert_eq!(rs.len(), 4);
    for r in &rs {
        assert!(r.error.is_none(), "{:?}", r.error);
    }
    let list: SessionListResult = serde_json::from_value(rs[0].result.clone().unwrap()).unwrap();
    let state = |sid: &str| {
        list.sessions
            .iter()
            .find(|s| s.session_id == sid)
            .and_then(|s| s.execution_state.clone())
    };
    assert_eq!(
        state(&ended.session_id).as_deref(),
        Some("budget_exhausted")
    );
    assert_eq!(state(&open.session_id).as_deref(), Some("waiting"));
    let h: theseus_protocol::SessionHistoryResult =
        serde_json::from_value(rs[2].result.clone().unwrap()).unwrap();
    assert_eq!(h.nodes.len(), 1);
    assert!(h.pending_confirms.is_empty());
    let execs: theseus_protocol::ExecutionListResult =
        serde_json::from_value(rs[3].result.clone().unwrap()).unwrap();
    let x = execs
        .executions
        .iter()
        .find(|e| e.execution_id == "exe_old_exhausted")
        .unwrap();
    assert_eq!(x.state, "budget_exhausted", "terminal stays terminal");
    assert_eq!((x.budget.limit_usd, x.budget.spent_usd), (100.0, 0.45));
    assert_eq!(x.budget.units_before["spent"], 877_683);
    let y = execs
        .executions
        .iter()
        .find(|e| e.execution_id == "exe_old_waiting")
        .unwrap();
    assert_eq!((y.state.as_str(), y.budget.spent_usd), ("waiting", 0.012));
    let migrated: Vec<LedgerRow> = core
        .store
        .ledger_tail::<LedgerRow>(500)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == "budget.migrated")
        .collect();
    assert_eq!(migrated.len(), 2);

    // The ended session refuses a turn as before; the waiting one runs.
    let submit = |id: u64, session: &str| {
        Request::new(
            Id::Num(id),
            method::TURN_SUBMIT,
            TurnSubmitParams {
                session_id: Some(session.into()),
                input: "hi again".into(),
                profile: None,
                provider: None,
                model: None,
                author: None,
                attachments: vec![],
            },
        )
    };
    let msgs = roundtrip(core.clone(), vec![submit(5, &ended.session_id)]).await;
    let refused = responses(&msgs)[0]
        .error
        .clone()
        .expect("an ended execution takes no turn");
    assert_eq!(
        refused.data["class"], "execution_budget_exhausted",
        "{refused:?}"
    );
    let msgs = roundtrip(core.clone(), vec![submit(6, &open.session_id)]).await;
    let ok: TurnSubmitResult =
        serde_json::from_value(responses(&msgs)[0].result.clone().unwrap()).unwrap();
    assert_eq!(ok.output, "Still here.");
    let e = core.kernel.execution("exe_old_waiting").unwrap().unwrap();
    assert!(
        e.budget.spent_micros > 12_000,
        "the old spend, then the new call: {:?}",
        e.budget
    );
}

/// Rows stored before theseus-8az renamed the decline vocabulary: a tool
/// result with status `denied` and an `action.denied` ledger row. Both
/// still decode, and the history and ledger reads serve them.
#[tokio::test]
async fn rows_stored_with_the_old_denied_names_still_decode() {
    use crate::node::ResultStatus;
    let core = test_core("hi");
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    let n = Node::tool_result(
        &sid,
        Some("turn_1"),
        Some(0),
        Body::ToolResult {
            tool_use_id: "toolu_1".into(),
            tool: "fs.write".into(),
            status: ResultStatus::Declined,
            is_error: true,
            content: "Not run: the operator declined this call (not now).".into(),
            correlation_id: Some("act_1".into()),
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: Value::Null,
            image: None,
        },
    );
    let mut old = serde_json::to_value(&n).unwrap();
    old["body"]["status"] = json!("denied");
    let r = theseus_store::NewRecord::json(theseus_store::kinds::NODE, Some(&n.id), &old)
        .unwrap()
        .scoped(&sid);
    assert!(String::from_utf8_lossy(&r.payload).contains(r#""status":"denied""#));
    core.store.append(&[r]).unwrap();
    let old_row = json!({"correlation_id": "act_1", "tool": "fs.write", "by": "operator", "reason": "not now"});
    for kind in ["action.denied", "action.declined"] {
        core.store
            .append_ledger(&LedgerRow::new(kind, Some(&sid), None, old_row.clone()))
            .unwrap();
    }

    let nodes = core.store.session_nodes(&sid).unwrap();
    assert!(
        matches!(
            &nodes[0].1.body,
            Body::ToolResult {
                status: ResultStatus::Declined,
                ..
            }
        ),
        "{nodes:?}"
    );
    let tail = |id, kind: &str| {
        Request::new(
            Id::Num(id),
            method::LEDGER_TAIL,
            LedgerTailParams {
                n: Some(10),
                kind: Some(kind.into()),
                session_id: None,
            },
        )
    };
    let got = roundtrip(
        core.clone(),
        vec![
            Request::new(
                Id::Num(1),
                method::SESSION_HISTORY,
                theseus_protocol::SessionHistoryParams {
                    session_id: sid.clone(),
                    n: None,
                },
            ),
            tail(2, "action.declined"),
            tail(3, "action.denied"),
        ],
    )
    .await;
    let rs = responses(&got);
    let result = |id| {
        rs.iter()
            .find(|r| r.id == Id::Num(id))
            .and_then(|r| r.result.clone())
            .unwrap()
    };
    let h: theseus_protocol::SessionHistoryResult = serde_json::from_value(result(1)).unwrap();
    assert_eq!(h.nodes[0].detail["status"], "declined");
    // Either name reads the rows stored under both.
    for id in [2, 3] {
        let t: LedgerTailResult = serde_json::from_value(result(id)).unwrap();
        let kinds: Vec<&str> = t.rows.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["action.denied", "action.declined"], "{t:?}");
        assert_eq!(t.rows[0].data, old_row);
    }
}

#[tokio::test]
async fn per_turn_provider_and_model_selection() {
    let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
    providers.insert(
        "anthropic".into(),
        Arc::new(FakeProvider {
            reply: "from anthropic".into(),
            ..Default::default()
        }),
    );
    providers.insert(
        "zai".into(),
        Arc::new(FakeProvider {
            reply: "from zai".into(),
            ..Default::default()
        }),
    );
    let core = Core::build(Parts {
        providers,
        ..Parts::for_tests(Config::example(), Arc::new(FakeProvider::default()), store)
    })
    .unwrap();
    let msgs = roundtrip(
        core.clone(),
        vec![
            Request::new(
                Id::Num(1),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: None,
                    input: "hi".into(),
                    profile: None,
                    provider: None,
                    model: None,
                    author: None,
                    attachments: vec![],
                },
            ),
            Request::new(
                Id::Num(2),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: None,
                    input: "hi".into(),
                    profile: None,
                    provider: Some("zai".into()),
                    model: Some("glm-5.3-flash".into()),
                    author: None,
                    attachments: vec![],
                },
            ),
            Request::new(
                Id::Num(3),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: None,
                    input: "hi".into(),
                    profile: None,
                    provider: Some("nope".into()),
                    model: None,
                    author: None,
                    attachments: vec![],
                },
            ),
        ],
    )
    .await;
    let rs = responses(&msgs);
    let r1: TurnSubmitResult = serde_json::from_value(
        rs.iter()
            .find(|r| r.id == Id::Num(1))
            .unwrap()
            .result
            .clone()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(r1.provider, "anthropic");
    assert_eq!(r1.output, "from anthropic");
    assert_eq!(r1.model, "claude-sonnet-5-5");
    let r2: TurnSubmitResult = serde_json::from_value(
        rs.iter()
            .find(|r| r.id == Id::Num(2))
            .unwrap()
            .result
            .clone()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(r2.provider, "zai");
    assert_eq!(r2.output, "from zai");
    assert_eq!(r2.model, "glm-5.3-flash");
    let e3 = rs
        .iter()
        .find(|r| r.id == Id::Num(3))
        .unwrap()
        .error
        .as_ref()
        .unwrap();
    assert_eq!(e3.code, error_code::INVALID_PARAMS);
    assert!(core.health().providers.contains(&"zai".to_string()));
}

#[tokio::test]
async fn live_profile_switch_persists_and_routes() {
    let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let mk = |store: Store| {
        let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
        providers.insert(
            "anthropic".into(),
            Arc::new(FakeProvider {
                reply: "from anthropic".into(),
                ..Default::default()
            }),
        );
        providers.insert(
            "zai".into(),
            Arc::new(FakeProvider {
                reply: "from zai".into(),
                ..Default::default()
            }),
        );
        Core::build(Parts {
            providers,
            ..Parts::for_tests(Config::example(), Arc::new(FakeProvider::default()), store)
        })
        .unwrap()
    };
    let core = mk(store.clone());
    assert_eq!(
        core.live_profile(),
        ("sonnet".to_string(), "config".to_string())
    );
    let ask = |id: u64, profile: Option<&str>| {
        Request::new(
            Id::Num(id),
            method::TURN_SUBMIT,
            TurnSubmitParams {
                session_id: None,
                input: "hi".into(),
                profile: profile.map(str::to_string),
                provider: None,
                model: None,
                author: None,
                attachments: vec![],
            },
        )
    };
    let msgs = roundtrip(
        core.clone(),
        vec![
            ask(1, None),
            Request::new(
                Id::Num(2),
                method::PROFILE_USE,
                ProfileUseParams { name: "glm".into() },
            ),
            ask(3, None),
            ask(4, Some("sonnet")),
            Request::new(
                Id::Num(5),
                method::PROFILE_USE,
                ProfileUseParams {
                    name: "nope".into(),
                },
            ),
            Request::new(Id::Num(6), method::PROFILE_LIST, Value::Null),
        ],
    )
    .await;
    let rs = responses(&msgs);
    let get = |id: u64| -> TurnSubmitResult {
        serde_json::from_value(
            rs.iter()
                .find(|r| r.id == Id::Num(id))
                .unwrap()
                .result
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    assert_eq!(get(1).profile, "sonnet");
    assert_eq!(get(1).output, "from anthropic");
    assert_eq!(get(3).profile, "glm");
    assert_eq!(get(3).provider, "zai");
    assert_eq!(get(3).model, "glm-5.3-flash");
    assert_eq!(get(3).output, "from zai");
    assert_eq!(get(4).profile, "sonnet", "explicit profile beats live");
    let bad = rs.iter().find(|r| r.id == Id::Num(5)).unwrap();
    assert_eq!(bad.error.as_ref().unwrap().code, error_code::INVALID_PARAMS);
    let list: ProfileListResult = serde_json::from_value(
        rs.iter()
            .find(|r| r.id == Id::Num(6))
            .unwrap()
            .result
            .clone()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(list.live, "glm");
    assert_eq!(list.live_source, "runtime");
    assert!(msgs
        .iter()
        .any(|m| matches!(m, Message::Notification(n) if n.method == notify::PROFILE_CHANGED)));

    // A fresh core over the same store comes up with the switched profile.
    drop(core);
    let core2 = mk(store);
    assert_eq!(
        core2.live_profile(),
        ("glm".to_string(), "runtime".to_string())
    );
    assert_eq!(core2.health().profile, "glm");
    assert_eq!(core2.health().model, "glm-5.3-flash");
}

#[tokio::test]
async fn parse_error_gets_a_response() {
    let core = test_core("x");
    let (client, server) = duplex(4096);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    cw.write_all(b"this is not json\n").await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let line = lines.next_line().await.unwrap().unwrap();
    let m: Message = serde_json::from_str(&line).unwrap();
    match m {
        Message::Response(r) => assert_eq!(r.error.unwrap().code, error_code::PARSE),
        other => panic!("expected response, got {other:?}"),
    }
    cw.shutdown().await.unwrap();
    drop(cw);
    drop(lines);
    let _ = srv.await;
}

/// Send one request on an open connection and read to its response,
/// keeping the notifications that came before it.
async fn ask<R, W>(
    w: &mut W,
    lines: &mut tokio::io::Lines<BufReader<R>>,
    req: Request,
) -> (Response, Vec<Notification>)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    w.write_all(line.as_bytes()).await.unwrap();
    let mut notes = Vec::new();
    loop {
        let l = lines.next_line().await.unwrap().unwrap();
        match serde_json::from_str::<Message>(&l).unwrap() {
            Message::Response(r) if r.id == req.id => return (r, notes),
            Message::Notification(n) => notes.push(n),
            _ => {}
        }
    }
}

/// `narrative.watch` returns the tail, then streams every line as
/// `narrative.line` on the same connection until `narrative.unwatch`; a
/// connection that arrives late gets the same lines as its tail. With
/// narration off, both methods refuse with DISABLED and health says so.
#[tokio::test]
async fn narrative_watch_streams_a_turn_and_refuses_when_off() {
    use theseus_protocol::{NarrativeLine, NarrativePart, NarrativeWatchResult};
    let core = test_core("hello there");
    assert!(core.health().narrative, "the template turns it on");
    let (client, server) = duplex(256 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "watcher".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let mut lines = BufReader::new(cr).lines();
    let watch = Request::new(Id::Num(1), method::NARRATIVE_WATCH, Value::Null);
    let (r, _) = ask(&mut cw, &mut lines, watch).await;
    let w: NarrativeWatchResult = serde_json::from_value(r.result.unwrap()).unwrap();
    assert!(w.lines.is_empty(), "{:?}", w.lines);
    assert_eq!(w.capacity, 500);
    let submit = |id, session_id: Option<String>| {
        Request::new(
            Id::Num(id),
            method::TURN_SUBMIT,
            TurnSubmitParams {
                session_id,
                input: "hi".into(),
                profile: None,
                provider: None,
                model: None,
                author: Some("discord:eddie".into()),
                attachments: vec![],
            },
        )
    };
    let (r, notes) = ask(&mut cw, &mut lines, submit(2, None)).await;
    let res: TurnSubmitResult = serde_json::from_value(r.result.unwrap()).unwrap();
    let narrated: Vec<NarrativeLine> = notes
        .iter()
        .filter(|n| n.method == notify::NARRATIVE_LINE)
        .map(|n| serde_json::from_value(n.params.clone()).unwrap())
        .collect();
    let says = |part: NarrativePart, needle: &str| {
        narrated
            .iter()
            .any(|l| l.part == part && l.text.contains(needle))
    };
    for (part, needle) in [
        (NarrativePart::Session, "opened (conversation)"),
        (NarrativePart::Session, "has a spend limit of $100."),
        (NarrativePart::Turn, "started by discord:eddie"),
        (NarrativePart::Turn, "2 characters of input"),
        (NarrativePart::Turn, "ended after 1 loop"),
        (NarrativePart::Session, "Parked until the next input"),
    ] {
        assert!(
            says(part, needle),
            "no {part:?} line says {needle:?}: {narrated:#?}"
        );
    }
    assert!(narrated
        .iter()
        .all(|l| l.session_id.as_deref() == Some(res.session_id.as_str())));
    // A connection that arrives late gets those lines as its tail.
    let late = roundtrip(
        core.clone(),
        vec![Request::new(
            Id::Num(3),
            method::NARRATIVE_WATCH,
            Value::Null,
        )],
    )
    .await;
    let tail: NarrativeWatchResult =
        serde_json::from_value(responses(&late)[0].result.clone().unwrap()).unwrap();
    assert_eq!(tail.lines, narrated);
    // Unwatched, the next turn's lines no longer come.
    let unwatch = Request::new(Id::Num(4), method::NARRATIVE_UNWATCH, Value::Null);
    let (r, _) = ask(&mut cw, &mut lines, unwatch).await;
    assert_eq!(r.result.unwrap()["watching"], false);
    let (r, notes) = ask(&mut cw, &mut lines, submit(5, Some(res.session_id.clone()))).await;
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(notes.iter().all(|n| n.method != notify::NARRATIVE_LINE));
    assert!(
        core.narrator.tail().len() > narrated.len(),
        "still narrated"
    );
    cw.shutdown().await.unwrap();
    drop(cw);
    drop(lines);
    let _ = srv.await;

    // Off: the methods refuse and health says so.
    let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.narrative = false;
    let fake = FakeProvider {
        reply: "x".into(),
        ..Default::default()
    };
    let off = Core::build(Parts::for_tests(cfg, Arc::new(fake), store)).unwrap();
    let msgs = roundtrip(
        off.clone(),
        vec![
            Request::new(Id::Num(1), method::HEALTH, Value::Null),
            Request::new(Id::Num(2), method::NARRATIVE_WATCH, Value::Null),
            Request::new(Id::Num(3), method::NARRATIVE_UNWATCH, Value::Null),
        ],
    )
    .await;
    let rs = responses(&msgs);
    let by = |id| rs.iter().find(|r| r.id == Id::Num(id)).unwrap();
    assert_eq!(by(1).result.as_ref().unwrap()["narrative"], false);
    for id in [2, 3] {
        let e = by(id).error.as_ref().expect("refused");
        assert_eq!(e.code, error_code::DISABLED);
        assert!(e.message.contains("narrative = true"), "{}", e.message);
    }
    assert!(off.narrator.tail().is_empty());
}
