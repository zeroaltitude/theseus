//! Theseus's MCP server on the fake core (41a's tests, design §3), driven by
//! 36a's client, and by raw requests for what a client would never send.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use theseus_mcp::client::{Client, Error, HttpTarget, Options, Transport};
use theseus_mcp::server::{Config, FakeCore, Server, Why};
use theseus_mcp::LATEST_PROTOCOL_VERSION;

const KEY: &str = "test-key-0123456789abcdef";

fn opts() -> Options {
    Options {
        request_timeout: Duration::from_secs(10),
        call_timeout: Duration::from_secs(10),
        ..Options::default()
    }
}

async fn start(core: &Arc<FakeCore>, tweak: impl FnOnce(&mut Config)) -> Server {
    let mut cfg = Config::new("127.0.0.1:0".parse().unwrap(), KEY);
    tweak(&mut cfg);
    Server::bind(cfg, core.clone())
        .await
        .expect("a loopback port")
}

/// The server as 36a's client reaches it. Connects are bounded: on some
/// hosts a connect to a loopback port nothing listens on hangs.
fn target(server: &Server, key: Option<&str>) -> HttpTarget {
    let mut t = HttpTarget::new(format!("http://{}/mcp", server.addr()));
    t.connect_timeout = Duration::from_secs(2);
    t.bearer = key.map(String::from);
    t
}

async fn client(server: &Server) -> Client {
    Client::connect(Transport::Http(target(server, Some(KEY))), opts())
        .await
        .expect("a handshake")
        .0
}

/// A raw request, as no client of ours would send it.
async fn raw(
    server: &Server,
    method: reqwest::Method,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (u16, HeaderMap, String) {
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .unwrap();
    let mut req = http.request(method, format!("http://{}/mcp", server.addr()));
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    if let Some(b) = body {
        req = req
            .header("content-type", "application/json")
            .body(b.to_string());
    }
    let resp = req.send().await.expect("an answer");
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    (status, headers, resp.text().await.unwrap_or_default())
}

fn bearer() -> String {
    format!("Bearer {KEY}")
}

/// A request written by hand, for a form no HTTP client sends; its status
/// line.
async fn raw_tcp(server: &Server, request: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let connect = tokio::net::TcpStream::connect(server.addr());
    let mut s = tokio::time::timeout(Duration::from_secs(2), connect)
        .await
        .expect("a connect in time")
        .expect("a connection");
    s.write_all(request.as_bytes()).await.unwrap();
    let mut out = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_string(&mut out)).await;
    out.lines().next().unwrap_or("").to_string()
}

fn initialize(version: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": version, "capabilities": {}, "clientInfo": { "name": "raw", "version": "0" } }
    })
}

fn ping() -> Value {
    json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" })
}

/// Call a tool and give back its JSON.
async fn tool(client: &Client, name: &str, args: Value) -> Value {
    let r = client.call_tool(name, args).await.expect("an answer");
    assert!(!r.is_error, "{name}: {}", r.text_for_model());
    let v = r.structured_content.clone().expect("structured content");
    assert_eq!(r.text_for_model(), v.to_string());
    v
}

#[tokio::test]
async fn the_handshake() {
    let core = Arc::new(FakeCore::default());
    let server = start(&core, |_| {}).await;
    let client = client(&server).await;
    let info = client.server_info();
    assert_eq!(info.server.name, "theseus");
    assert_eq!(info.protocol_version, LATEST_PROTOCOL_VERSION);
    assert!(info.capabilities.tools.is_some());
    assert!(info
        .instructions
        .as_deref()
        .is_some_and(|i| i.contains("conversation_open")));
    assert_eq!(client.session_id().unwrap().len(), 32);
    let stats = server.stats();
    assert!(stats.listening);
    assert_eq!(stats.port, server.addr().port());
    assert_eq!(
        (stats.sessions, stats.clients.clone()),
        (1, vec!["theseus".to_string()])
    );
    // The client's GET for a server stream got 405, and the session goes on.
    client.ping().await.unwrap();

    // A revision it speaks is answered as asked; any other, with its newest.
    let auth = bearer();
    let (status, headers, body) = raw(
        &server,
        reqwest::Method::POST,
        &[("authorization", &auth)],
        Some(initialize("2025-03-26")),
    )
    .await;
    assert_eq!(status, 200);
    assert!(headers.get("mcp-session-id").is_some());
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["result"]["protocolVersion"], "2025-03-26");
    let (_, _, body) = raw(
        &server,
        reqwest::Method::POST,
        &[("authorization", &auth)],
        Some(initialize("1999-01-01")),
    )
    .await;
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    let stats = server.stats();
    assert_eq!(stats.clients, ["raw", "theseus"]);
    assert_eq!(
        (stats.opened, stats.last_client.as_deref()),
        (3, Some("raw"))
    );
}

#[tokio::test]
async fn a_wrong_key_gets_401() {
    let core = Arc::new(FakeCore::default());
    let server = start(&core, |_| {}).await;
    for key in [None, Some("wrong-key-0123456789"), Some("")] {
        let e = Client::connect(Transport::Http(target(&server, key)), opts())
            .await
            .err()
            .expect("a refusal");
        assert!(
            matches!(e, Error::Http { status: 401, .. }),
            "{key:?}: {e:?}"
        );
    }
    let (status, headers, _) = raw(&server, reqwest::Method::POST, &[], Some(ping())).await;
    assert_eq!(status, 401);
    assert_eq!(headers.get("www-authenticate").unwrap(), "Bearer");
    assert_eq!(server.stats().refused[&Why::Key], 4);
    assert_eq!(server.stats().sessions, 0);
    // Reported once this minute, however many; never with the key.
    let reported = core.refusals();
    assert_eq!(reported.len(), 1);
    assert_eq!((reported[0].why, reported[0].unreported), (Why::Key, 0));
    assert_eq!(reported[0].detail, None);
}

#[tokio::test]
async fn a_foreign_origin_is_refused() {
    let core = Arc::new(FakeCore::default());
    let server = start(&core, |_| {}).await;
    let auth = bearer();
    for origin in ["http://evil.example", "null", "http://127.0.0.1.nip.io:80"] {
        let (status, _, _) = raw(
            &server,
            reqwest::Method::POST,
            &[("authorization", &auth), ("origin", origin)],
            Some(initialize(LATEST_PROTOCOL_VERSION)),
        )
        .await;
        assert_eq!(status, 403, "{origin}");
    }
    // A page on this machine (a browser-based client) passes.
    let (status, _, _) = raw(
        &server,
        reqwest::Method::POST,
        &[
            ("authorization", &auth),
            ("origin", "http://localhost:6274"),
        ],
        Some(initialize(LATEST_PROTOCOL_VERSION)),
    )
    .await;
    assert_eq!(status, 200);
    // A rebinding page's request names its own site in Host.
    let host = format!("evil.example:{}", server.addr().port());
    let (status, _, _) = raw(
        &server,
        reqwest::Method::POST,
        &[("authorization", &auth), ("host", &host)],
        Some(initialize(LATEST_PROTOCOL_VERSION)),
    )
    .await;
    assert_eq!(status, 403);
    // So does an absolute-form target's authority, whatever the Host says.
    let port = server.addr().port();
    let body = initialize(LATEST_PROTOCOL_VERSION).to_string();
    let request = format!(
        "POST http://evil.example:{port}/mcp HTTP/1.1\r\nhost: 127.0.0.1:{port}\r\n\
         authorization: {auth}\r\ncontent-type: application/json\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    assert!(raw_tcp(&server, &request).await.starts_with("HTTP/1.1 403"));
    let stats = server.stats();
    assert_eq!(
        (stats.refused[&Why::Origin], stats.refused[&Why::Host]),
        (3, 2)
    );
    assert_eq!(stats.sessions, 1);
    let reported = core.refusals();
    assert_eq!(reported.len(), 2);
    assert_eq!(reported[0].detail.as_deref(), Some("http://evil.example"));
    assert_eq!(reported[1].why, Why::Host);
}

#[tokio::test]
async fn tools_list() {
    let core = Arc::new(FakeCore::default());
    let server = start(&core, |_| {}).await;
    let tools = client(&server).await.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "conversation_open",
            "conversation_send",
            "conversation_status",
            "task_list",
            "wake_list"
        ]
    );
    assert_eq!(
        tools[1].input_schema["required"],
        json!(["session_id", "text"])
    );
    for t in &tools[2..] {
        assert_eq!(
            t.annotations.as_ref().unwrap().read_only_hint,
            Some(true),
            "{}",
            t.name
        );
    }
    assert!(tools.iter().all(|t| t.description.is_some()));
}

#[tokio::test]
async fn a_conversation_round_trip() {
    let core = Arc::new(FakeCore::new(Duration::from_millis(30)));
    let server = start(&core, |_| {}).await;
    let client = client(&server).await;
    let opened = tool(&client, "conversation_open", json!({ "label": "review" })).await;
    let sid = opened["session_id"].as_str().unwrap().to_string();
    assert_eq!(core.labels(), ["mcp theseus review"]);
    let sent = tool(
        &client,
        "conversation_send",
        json!({ "session_id": sid, "text": "hello" }),
    )
    .await;
    assert_eq!(sent["reply"], "you said: hello");
    let turn = sent["turn_id"].as_str().unwrap().to_string();
    let status = tool(&client, "conversation_status", json!({ "session_id": sid })).await;
    assert_eq!(status["state"], "idle");
    assert_eq!(status["last_reply"], "you said: hello");
    assert_eq!(
        (status["turns"].as_u64(), status["turn_id"].as_str()),
        (Some(1), Some(turn.as_str()))
    );
    assert_eq!(
        tool(&client, "task_list", json!({})).await["tasks"],
        json!([])
    );
    assert_eq!(
        tool(&client, "wake_list", json!({ "session_id": sid })).await["session_id"],
        json!(sid)
    );
    // The core saw who called, and the connection's two ends.
    for c in core.callers() {
        assert_eq!(c.client_name, "theseus");
        assert_eq!(c.mcp_session, client.session_id().unwrap());
        assert_eq!(c.ends.server, Some(server.addr()));
        assert!(c.ends.client.ip().is_loopback());
    }
    // Each call is a ledger row.
    let rows = core.calls();
    let tools: Vec<&str> = rows.iter().map(|r| r.tool.as_str()).collect();
    assert_eq!(
        tools,
        [
            "conversation_open",
            "conversation_send",
            "conversation_status",
            "task_list",
            "wake_list"
        ]
    );
    assert!(rows.iter().all(|r| r.ok && r.client_name == "theseus"));
    assert_eq!(rows[0].session_id.as_deref(), Some(sid.as_str()));
    assert_eq!(rows[3].session_id, None);
    assert_eq!((server.stats().calls, server.stats().errors), (5, 0));
}

#[tokio::test]
async fn running_past_wait_secs() {
    let core = Arc::new(FakeCore::new(Duration::from_millis(1500)));
    let server = start(&core, |_| {}).await;
    let client = client(&server).await;
    let sid = tool(&client, "conversation_open", json!({})).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let started = Instant::now();
    let sent = tool(
        &client,
        "conversation_send",
        json!({ "session_id": sid, "text": "slow", "wait_secs": 0 }),
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(sent["status"], "running");
    let turn = sent["turn_id"].as_str().unwrap().to_string();
    let status = tool(&client, "conversation_status", json!({ "session_id": sid })).await;
    assert_eq!(status["state"], "running");
    assert_eq!(status["turn_id"], json!(turn));
    assert!(status.get("last_reply").is_none());
    // A second turn while one runs: a refusal the client's model reads.
    let busy = client
        .call_tool(
            "conversation_send",
            json!({ "session_id": sid, "text": "again", "wait_secs": 0 }),
        )
        .await
        .unwrap();
    assert!(busy.is_error && busy.text_for_model().contains("a turn is running"));
    core.turn_done(&sid).await;
    let status = tool(&client, "conversation_status", json!({ "session_id": sid })).await;
    assert_eq!(status["state"], "idle");
    assert_eq!(status["last_reply"], "you said: slow");
    assert_eq!(server.stats().errors, 1);
}

#[tokio::test]
async fn the_rate_limit() {
    let core = Arc::new(FakeCore::default());
    let server = start(&core, |c| c.requests_per_minute = 5).await;
    // No server stream, so every request is one this test makes.
    let mut t = target(&server, Some(KEY));
    t.server_stream = false;
    let (client, _events) = Client::connect(Transport::Http(t), opts()).await.unwrap();
    // `initialize` and `initialized` were two; three more pass.
    for _ in 0..3 {
        client.ping().await.unwrap();
    }
    let e = client.ping().await.unwrap_err();
    assert!(matches!(e, Error::Http { status: 429, .. }), "{e:?}");
    let auth = bearer();
    let (status, headers, _) = raw(
        &server,
        reqwest::Method::POST,
        &[("authorization", &auth)],
        Some(ping()),
    )
    .await;
    assert_eq!(status, 429);
    let after: u64 = headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((1..=12).contains(&after), "{after}");
    assert_eq!(server.stats().refused[&Why::Rate], 2);
    assert_eq!(core.refusals().len(), 1);
}

#[tokio::test]
async fn get_is_405_and_delete_ends_the_session() {
    let core = Arc::new(FakeCore::default());
    let server = start(&core, |_| {}).await;
    let auth = bearer();
    let (status, headers, _) = raw(
        &server,
        reqwest::Method::GET,
        &[("authorization", &auth)],
        None,
    )
    .await;
    assert_eq!(status, 405);
    assert_eq!(headers["allow"], "POST, DELETE");

    // A session ended under the client (the server forgot it, or another
    // client ended it): the next request gets 404, and 36a's client makes
    // a new one.
    let client = client(&server).await;
    let first = client.session_id().unwrap();
    let (status, _, _) = raw(
        &server,
        reqwest::Method::DELETE,
        &[("authorization", &auth), ("mcp-session-id", &first)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _, _) = raw(
        &server,
        reqwest::Method::POST,
        &[("authorization", &auth), ("mcp-session-id", &first)],
        Some(ping()),
    )
    .await;
    assert_eq!(status, 404);
    client.ping().await.unwrap();
    let second = client.session_id().unwrap();
    assert_ne!(first, second);

    // The client's own close ends its session.
    client.close().await;
    assert_eq!(server.stats().sessions, 0);
    server.stop();
    assert!(!server.stats().listening);
}

#[tokio::test]
async fn requests_the_server_will_not_take() {
    let core = Arc::new(FakeCore::default());
    let server = start(&core, |_| {}).await;
    let client = client(&server).await;
    let session = client.session_id().unwrap();
    let auth = bearer();
    let post = |headers: Vec<(&'static str, String)>, body: Value| {
        let server = &server;
        async move {
            let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            raw(server, reqwest::Method::POST, &h, Some(body)).await
        }
    };
    let with = |s: &str| {
        vec![
            ("authorization", auth.clone()),
            ("mcp-session-id", s.to_string()),
        ]
    };
    // No session; an unknown one; a batch; an unsupported revision.
    assert_eq!(
        post(vec![("authorization", auth.clone())], ping()).await.0,
        400
    );
    assert_eq!(post(with("not-a-session"), ping()).await.0, 404);
    assert_eq!(post(with(&session), json!([ping()])).await.0, 400);
    let mut old = with(&session);
    old.push(("mcp-protocol-version", "1999-01-01".into()));
    assert_eq!(post(old, ping()).await.0, 400);
    // A notification is accepted.
    let note = json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 9 } });
    assert_eq!(post(with(&session), note).await.0, 202);
    // An unknown method, and an unknown tool, are JSON-RPC errors.
    match client.request("resources/list", Value::Null).await {
        Err(Error::Rpc { code, .. }) => assert_eq!(code, -32601),
        other => panic!("{other:?}"),
    }
    match client.call_tool("shell", json!({})).await {
        Err(Error::Rpc { code, .. }) => assert_eq!(code, -32602),
        other => panic!("{other:?}"),
    }
    // A call missing its arguments, or naming no session the core knows,
    // is a result the model reads.
    let r = client
        .call_tool("conversation_send", json!({ "text": "hi" }))
        .await
        .unwrap();
    assert!(r.is_error && r.text_for_model().contains("needs session_id"));
    let r = client
        .call_tool("conversation_status", json!({ "session_id": "ses_nope" }))
        .await
        .unwrap();
    assert!(r.is_error && r.text_for_model().contains("no session"));
    assert_eq!(server.stats().errors, 2);
}

#[tokio::test]
async fn loopback_only_and_a_real_key() {
    let core = Arc::new(FakeCore::default());
    let e = Server::bind(Config::new("0.0.0.0:0".parse().unwrap(), KEY), core.clone())
        .await
        .err()
        .unwrap();
    assert!(e.to_string().contains("loopback only"), "{e}");
    let e = Server::bind(Config::new("127.0.0.1:0".parse().unwrap(), "short"), core)
        .await
        .err()
        .unwrap();
    assert!(e.to_string().contains("16 bytes"), "{e}");
}
