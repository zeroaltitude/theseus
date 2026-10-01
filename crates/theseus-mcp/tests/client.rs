//! The client against the fake (36a's tests, design §3): over a pipe in
//! this process, over stdio to the fake's own process, and over streamable
//! HTTP. Every wait is on an event or the fake's record, never a sleep.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_mcp::client::{
    stdio_command, CallOptions, Client, Error, Event, Events, HttpTarget, Options, StderrLog,
    StdioServer, Transport,
};
use theseus_mcp::fake::{Config, Fake, Mode};
use theseus_mcp::LATEST_PROTOCOL_VERSION;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const WAIT: Duration = Duration::from_secs(5);

fn opts() -> Options {
    Options {
        request_timeout: Duration::from_secs(10),
        call_timeout: Duration::from_secs(10),
        ..Options::default()
    }
}

async fn connect_pipes(fake: &Arc<Fake>) -> Result<(Client, Events), Error> {
    let (ours, theirs) = tokio::io::duplex(1 << 16);
    let (sr, sw) = tokio::io::split(theirs);
    tokio::spawn(fake.clone().serve_pipes(sr, sw));
    let (cr, cw) = tokio::io::split(ours);
    Client::connect(Transport::pipes(cr, cw), opts()).await
}

async fn over_pipes(cfg: Config) -> (Arc<Fake>, Client, Events) {
    let fake = Fake::new(cfg);
    let (client, events) = connect_pipes(&fake).await.expect("a handshake over pipes");
    (fake, client, events)
}

/// The fake on HTTP, and a target for it. The connect is bounded: on some
/// hosts a connect to a loopback port nothing listens on hangs.
async fn http_fake(cfg: Config) -> (Arc<Fake>, HttpTarget) {
    let fake = Fake::new(cfg);
    let addr = fake.clone().serve_http(0).await.expect("a loopback port");
    let mut target = HttpTarget::new(format!("http://{addr}/mcp"));
    target.connect_timeout = Duration::from_secs(2);
    (fake, target)
}

async fn over_http(cfg: Config) -> (Arc<Fake>, Client, Events) {
    let (fake, target) = http_fake(cfg).await;
    let (client, events) = Client::connect(Transport::Http(target), opts())
        .await
        .expect("a handshake over HTTP");
    (fake, client, events)
}

/// The fake's own process, over stdio.
fn fake_process(args: &[&str]) -> StdioServer {
    let mut argv = vec![env!("CARGO_BIN_EXE_theseus-mcp-fake").to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let child = stdio_command(&argv)
        .expect("a command")
        .spawn()
        .expect("the fake's process");
    StdioServer {
        child,
        stderr_log: None,
    }
}

async fn over_process(args: &[&str]) -> (Client, Events) {
    Client::connect(Transport::Stdio(fake_process(args)), opts())
        .await
        .expect("a handshake over stdio")
}

/// The first event that `pred` picks, within `WAIT`.
async fn wait_event(events: &mut Events, pred: impl Fn(&Event) -> bool) -> Event {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let e = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("the event in time")
            .expect("the event stream open");
        if pred(&e) {
            return e;
        }
    }
}

/// A call and paging: every page of tools and prompts, two calls, a prompt,
/// and a ping. With the fake in this process, the cursors it was sent.
async fn basics(client: &Client, fake: Option<&Fake>) {
    let tools = client.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["echo", "add", "sleep", "image", "fail"]);
    assert_eq!(
        tools[0].annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );
    assert_eq!(tools[1].input_schema["required"], json!(["a", "b"]));
    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[0].arguments[0].required);

    let r = client
        .call_tool("echo", json!({ "text": "hello" }))
        .await
        .unwrap();
    assert!(!r.is_error);
    assert_eq!(r.text_for_model(), "hello");
    let r = client
        .call_tool("add", json!({ "a": 2, "b": 3 }))
        .await
        .unwrap();
    assert_eq!(r.structured_content, Some(json!({ "sum": 5.0 })));
    let r = client.call_tool("image", json!({})).await.unwrap();
    assert!(r.text_for_model().contains("[image, image/png"));

    let args = BTreeMap::from([("name".to_string(), "Eddie".to_string())]);
    let p = client.get_prompt("greet", &args).await.unwrap();
    assert_eq!(p.messages[0].role, "user");
    assert_eq!(p.messages[0].content.as_model_text(), "Say hello to Eddie.");
    client.ping().await.unwrap();

    if let Some(fake) = fake {
        let cursors: Vec<Value> = fake
            .seen()
            .messages
            .iter()
            .filter(|m| m["method"] == "tools/list")
            .map(|m| m["params"]["cursor"].clone())
            .collect();
        assert_eq!(cursors, [Value::Null, json!("page-2"), json!("page-4")]);
    }
}

#[tokio::test]
async fn the_handshake_and_its_revision() {
    let (fake, client, _events) = over_pipes(Config::default()).await;
    let info = client.server_info();
    assert_eq!(info.protocol_version, LATEST_PROTOCOL_VERSION);
    assert_eq!(info.server.name, "fake");
    assert!(info.capabilities.tools.as_ref().unwrap().list_changed);
    assert!(info.instructions.is_some());
    assert!(fake.wait_until(WAIT, |s| s.initialized == 1).await);
    let init = &fake.seen().messages[0];
    assert_eq!(init["method"], "initialize");
    assert_eq!(init["params"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    assert_eq!(init["params"]["clientInfo"]["name"], "theseus");
    // v1 declares no sampling, elicitation, or roots.
    assert_eq!(init["params"]["capabilities"], json!({}));

    // An older revision the client speaks is accepted.
    let (_fake, client, _events) = over_pipes(Config {
        versions: vec!["2024-11-05".into()],
        ..Config::default()
    })
    .await;
    assert_eq!(client.server_info().protocol_version, "2024-11-05");

    // One it does not speak is refused.
    let fake = Fake::new(Config {
        versions: vec!["1999-01-01".into()],
        ..Config::default()
    });
    let e = connect_pipes(&fake).await.err().expect("a refusal");
    assert!(
        matches!(&e, Error::UnsupportedVersion { answered, .. } if answered == "1999-01-01"),
        "{e:?}"
    );
    // The connection was closed without `initialized`.
    assert_eq!(fake.seen().initialized, 0);

    // Over stdio, to a process.
    let (client, _events) = over_process(&["--versions", "2025-06-18"]).await;
    assert_eq!(client.server_info().protocol_version, "2025-06-18");
    assert!(client.pid().is_some());
}

#[tokio::test]
async fn a_call_and_paging_over_pipes() {
    let (fake, client, _events) = over_pipes(Config::default()).await;
    basics(&client, Some(&fake)).await;
}

#[tokio::test]
async fn a_call_and_paging_over_stdio() {
    let (client, _events) = over_process(&[]).await;
    basics(&client, None).await;
}

#[tokio::test]
async fn a_call_and_paging_over_http() {
    let (fake, client, _events) = over_http(Config::default()).await;
    basics(&client, Some(&fake)).await;
}

#[tokio::test]
async fn a_call_and_paging_over_http_as_json() {
    let (fake, client, _events) = over_http(Config {
        sse: false,
        ..Config::default()
    })
    .await;
    basics(&client, Some(&fake)).await;
}

async fn sees_list_changed(client: &Client, events: &mut Events) {
    client
        .call_tool("echo", json!({ "text": "change" }))
        .await
        .unwrap();
    wait_event(events, |e| *e == Event::ToolListChanged).await;
    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 6);
    assert_eq!(tools[5].name, "tool_v1");
    assert_eq!(
        tools[0].description.as_deref(),
        Some("Echoes its text (list 1).")
    );
}

#[tokio::test]
async fn list_changed_over_pipes_stdio_and_http() {
    let change = Config {
        mode: Mode::ChangeTools,
        ..Config::default()
    };
    let (_fake, client, mut events) = over_pipes(change.clone()).await;
    sees_list_changed(&client, &mut events).await;

    let (client, mut events) = over_process(&["--mode", "change-tools"]).await;
    sees_list_changed(&client, &mut events).await;

    // Over HTTP it comes on the session's own stream, once the client
    // holds it.
    let (fake, client, mut events) = over_http(change).await;
    assert!(fake.wait_until(WAIT, |s| s.server_streams == 1).await);
    sees_list_changed(&client, &mut events).await;
}

#[tokio::test]
async fn a_crash_mid_call_over_stdio() {
    let (client, mut events) = over_process(&["--mode", "crash-after=1"]).await;
    client
        .call_tool("echo", json!({ "text": "one" }))
        .await
        .unwrap();
    let started = Instant::now();
    let e = client
        .call_tool("echo", json!({ "text": "two" }))
        .await
        .unwrap_err();
    // At once, not at the call's timeout.
    assert!(started.elapsed() < WAIT, "{:?}", started.elapsed());
    match &e {
        Error::Closed(why) => assert!(why.contains("exit status: 3"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(e.outcome_unknown());
    wait_event(&mut events, |e| matches!(e, Event::Closed { .. })).await;
    // Later calls fail at once.
    assert!(matches!(
        client.call_tool("echo", json!({ "text": "three" })).await,
        Err(Error::Closed(_))
    ));
    assert!(client.closed().is_some());
}

#[tokio::test]
async fn a_crash_mid_call_over_http() {
    for sse in [true, false] {
        let (_fake, client, _events) = over_http(Config {
            mode: Mode::CrashAfter(1),
            sse,
            ..Config::default()
        })
        .await;
        client
            .call_tool("echo", json!({ "text": "one" }))
            .await
            .unwrap();
        let started = Instant::now();
        let e = client
            .call_tool("echo", json!({ "text": "two" }))
            .await
            .unwrap_err();
        assert!(started.elapsed() < WAIT, "{:?}", started.elapsed());
        assert!(
            matches!(e, Error::Closed(_)) && e.outcome_unknown(),
            "sse {sse}: {e:?}"
        );
    }
}

async fn times_out(client: &Client, fake: &Fake) {
    let call = CallOptions {
        timeout: Some(Duration::from_millis(300)),
        ..CallOptions::default()
    };
    let started = Instant::now();
    let e = client
        .call_tool_with("echo", json!({ "text": "late" }), &call)
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::Timeout { .. }) && e.outcome_unknown(),
        "{e:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    // The server is told, and nothing waits on.
    assert!(fake.wait_until(WAIT, |s| s.cancelled.len() == 1).await);
    let seen = fake.seen();
    let call_id = &seen
        .messages
        .iter()
        .find(|m| m["method"] == "tools/call")
        .unwrap()["id"];
    assert_eq!(&seen.cancelled[0], call_id);
    let cancel = seen
        .messages
        .iter()
        .find(|m| m["method"] == "notifications/cancelled")
        .unwrap();
    assert!(cancel["params"]["reason"]
        .as_str()
        .unwrap()
        .contains("no answer within 300 ms"));
    assert_eq!(client.in_flight(), 0);
    // The connection lives on.
    fake.set_mode(Mode::Ok);
    let r = client
        .call_tool("echo", json!({ "text": "on time" }))
        .await
        .unwrap();
    assert_eq!(r.text_for_model(), "on time");
}

#[tokio::test]
async fn a_timeout_cancels_the_call() {
    let slow = Config {
        mode: Mode::Slow,
        slow_ms: 5_000,
        ..Config::default()
    };
    let (fake, client, _events) = over_pipes(slow.clone()).await;
    times_out(&client, &fake).await;
    let (fake, client, _events) = over_http(slow).await;
    times_out(&client, &fake).await;
}

/// A call whose future is dropped (a `/stop` aborting its task) is
/// cancelled at the server.
async fn cancels(client: &Client, fake: &Fake) {
    let c = client.clone();
    let call = tokio::spawn(async move { c.call_tool("sleep", json!({ "ms": 20_000 })).await });
    assert!(fake.wait_until(WAIT, |s| s.calls == 1).await);
    call.abort();
    assert!(fake.wait_until(WAIT, |s| s.cancelled.len() == 1).await);
    let seen = fake.seen();
    let call_id = &seen
        .messages
        .iter()
        .find(|m| m["method"] == "tools/call")
        .unwrap()["id"];
    assert_eq!(&seen.cancelled[0], call_id);
    assert_eq!(client.in_flight(), 0);
    let r = client
        .call_tool("echo", json!({ "text": "still here" }))
        .await
        .unwrap();
    assert_eq!(r.text_for_model(), "still here");
}

#[tokio::test]
async fn a_cancel_over_pipes_and_http() {
    let (fake, client, _events) = over_pipes(Config::default()).await;
    cancels(&client, &fake).await;
    let (fake, client, _events) = over_http(Config::default()).await;
    cancels(&client, &fake).await;
}

#[tokio::test]
async fn progress_rides_the_calls_own_stream() {
    let (_fake, client, mut events) = over_http(Config::default()).await;
    let call = CallOptions {
        progress_token: Some(json!("t-1")),
        ..CallOptions::default()
    };
    let r = client
        .call_tool_with("echo", json!({ "text": "with progress" }), &call)
        .await
        .unwrap();
    assert_eq!(r.text_for_model(), "with progress");
    for step in [1.0, 2.0] {
        let e = wait_event(&mut events, |e| matches!(e, Event::Progress { .. })).await;
        assert_eq!(
            e,
            Event::Progress {
                token: json!("t-1"),
                progress: step,
                total: Some(2.0),
                message: None
            }
        );
    }
}

#[tokio::test]
async fn the_session_rides_every_request_and_a_404_starts_another() {
    let (fake, client, mut events) = over_http(Config::default()).await;
    let first = client.session_id().expect("a session");
    client.list_tools().await.unwrap();
    let seen = fake.seen();
    // `initialize` carried none; everything after it carried the session
    // and the revision.
    assert_eq!(seen.sessions[0], None);
    assert_eq!(seen.versions[0], None);
    assert!(seen.sessions[1..]
        .iter()
        .all(|s| s.as_deref() == Some(first.as_str())));
    assert!(seen.versions[1..]
        .iter()
        .all(|v| v.as_deref() == Some(LATEST_PROTOCOL_VERSION)));

    // The server forgets it: the next call gets 404, a new `initialize`
    // runs, and the call goes again.
    fake.expire_sessions();
    let r = client
        .call_tool("echo", json!({ "text": "again" }))
        .await
        .unwrap();
    assert_eq!(r.text_for_model(), "again");
    wait_event(&mut events, |e| *e == Event::Reinitialized).await;
    let second = client.session_id().expect("a new session");
    assert_ne!(first, second);
    let seen = fake.seen();
    assert_eq!((seen.initializes, seen.initialized), (2, 2));
    // The call on the old session, the new `initialize` (with none), its
    // `initialized`, and the call again, on the new one.
    let last: Vec<Option<&str>> = seen.sessions[seen.sessions.len() - 4..]
        .iter()
        .map(|s| s.as_deref())
        .collect();
    assert_eq!(
        last,
        [
            Some(first.as_str()),
            None,
            Some(second.as_str()),
            Some(second.as_str())
        ]
    );
}

#[tokio::test]
async fn error_results_and_errors() {
    let (fake, client, _events) = over_pipes(Config::default()).await;
    let r = client.call_tool("fail", json!({})).await.unwrap();
    assert!(r.is_error);
    assert!(r.text_for_model().contains("failed"));
    match client.call_tool("nope", json!({})).await {
        Err(Error::Rpc { code, message, .. }) => {
            assert_eq!(code, -32602);
            assert!(message.contains("Unknown tool"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let e = client
        .get_prompt("greet", &BTreeMap::new())
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Rpc { code: -32602, .. }), "{e:?}");
    fake.set_mode(Mode::Error);
    let r = client
        .call_tool("echo", json!({ "text": "x" }))
        .await
        .unwrap();
    assert!(r.is_error && r.text_for_model().contains("failed echo"));
}

async fn answers_a_ping(client: &Client, fake: &Fake) {
    client
        .call_tool("echo", json!({ "text": "ping me" }))
        .await
        .unwrap();
    assert!(fake.wait_until(WAIT, |s| !s.answers.is_empty()).await);
    let a = &fake.seen().answers[0];
    assert!(a["id"].as_str().unwrap().starts_with("fake-ping-"), "{a}");
    assert_eq!(a["result"], json!({}));
}

#[tokio::test]
async fn the_servers_ping_is_answered() {
    let (fake, client, _events) = over_pipes(Config::default()).await;
    answers_a_ping(&client, &fake).await;
    let (fake, client, _events) = over_http(Config::default()).await;
    answers_a_ping(&client, &fake).await;
}

#[tokio::test]
async fn a_stream_closed_before_its_answer_is_resumed() {
    let (fake, client, _events) = over_http(Config {
        poll: true,
        ..Config::default()
    })
    .await;
    let r = client
        .call_tool("echo", json!({ "text": "polled" }))
        .await
        .unwrap();
    assert_eq!(r.text_for_model(), "polled");
    // Both `initialize` and the call came back on a resumed stream.
    let resumed = fake.seen().resumed;
    assert_eq!(resumed.len(), 2, "{resumed:?}");
    assert!(resumed.iter().all(|id| id.starts_with('p')));
}

#[tokio::test]
async fn the_key_rides_every_request() {
    let (fake, mut target) = http_fake(Config {
        bearer: Some("k-123".into()),
        ..Config::default()
    })
    .await;
    target.bearer = Some("k-123".into());
    let (client, _events) = Client::connect(Transport::Http(target.clone()), opts())
        .await
        .expect("a handshake with the key");
    client.ping().await.unwrap();
    assert_eq!(fake.seen().unauthorized, 0);
    target.bearer = None;
    let e = Client::connect(Transport::Http(target), opts())
        .await
        .err()
        .expect("a refusal");
    assert!(matches!(e, Error::Http { status: 401, .. }), "{e:?}");
    assert_eq!(fake.seen().unauthorized, 1);
}

#[tokio::test]
async fn close_ends_an_http_session() {
    let (fake, client, mut events) = over_http(Config::default()).await;
    let session = client.session_id().unwrap();
    client.close().await;
    assert!(
        fake.wait_until(WAIT, |s| s.deleted == [session.clone()])
            .await
    );
    let e = wait_event(&mut events, |e| matches!(e, Event::Closed { .. })).await;
    assert_eq!(
        e,
        Event::Closed {
            reason: "closed by the client".into()
        }
    );
}

fn alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[tokio::test]
async fn a_dropped_client_ends_its_server() {
    let (client, _events) = over_process(&[]).await;
    let pid = client.pid().unwrap();
    assert!(alive(pid));
    drop(client);
    // Its stdin closes and its group gets SIGTERM; the client's waiter
    // reaps it. (/proc is polled: there is no event for another process's
    // end.)
    let deadline = Instant::now() + WAIT;
    while alive(pid) {
        assert!(Instant::now() < deadline, "the server outlived its client");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_server_that_dies_at_start_is_named_by_its_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("fake.log");
    let mut server = fake_process(&["--no-such-flag"]);
    server.stderr_log = Some(StderrLog {
        path: log.clone(),
        cap_bytes: 64,
    });
    let e = Client::connect(Transport::Stdio(server), opts())
        .await
        .err()
        .expect("a failed start");
    match &e {
        Error::Closed(why) => {
            assert!(why.contains("exit status: 2"), "{why}");
            assert!(why.contains("unknown argument"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    // The log holds the start of its stderr, up to the cap, and is the
    // operator's alone.
    let kept = std::fs::read_to_string(&log).unwrap();
    assert_eq!(kept.len(), 64);
    assert!(kept.starts_with("theseus-mcp-fake: unknown argument"));
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&log).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn stray_lines_on_stdout_are_skipped() {
    let (ours, theirs) = tokio::io::duplex(1 << 16);
    let (sr, mut sw) = tokio::io::split(theirs);
    let server = tokio::spawn(async move {
        let mut lines = BufReader::new(sr).lines();
        let init: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        let answer = json!({
            "jsonrpc": "2.0",
            "id": init["id"],
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": { "name": "scripted", "version": "0" }
            }
        });
        // Not JSON, a blank line, an answer to no call, then the answer.
        let out = format!(
            "not json at all\n\n{{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{{}}}}\n{answer}\n"
        );
        sw.write_all(out.as_bytes()).await.unwrap();
        let next: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(next["method"], "notifications/initialized");
        (lines, sw)
    });
    let (cr, cw) = tokio::io::split(ours);
    let (client, _events) = Client::connect(Transport::pipes(cr, cw), opts())
        .await
        .unwrap();
    assert_eq!(client.server_info().server.name, "scripted");
    let _held = server.await.unwrap();
}
