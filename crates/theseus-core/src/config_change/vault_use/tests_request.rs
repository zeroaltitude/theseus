//! `http.request` (issue 65), played through the whole core against a test's own server on
//! loopback, which the test's web client takes as a public address (`web::tests::web`) and which
//! the test-only `plaintext` rule lets an `http` URL reach. The owner's own turn sends one request
//! with a vault item as its key: no card, no settings, one told note, the key in its one place,
//! the value in no exit. It asks first after outside text and in a turn the owner did not begin,
//! keeps the wall, sends nothing to a private address, follows no redirect.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::WebToolsConfig;
use crate::config_change::tests::{rig_with_tools, Rig};
use crate::content::OutsideMode;
use crate::node::ResultStatus;
use crate::secrets::look::tests::{logged, stub};

pub(super) const VALUE: &str = "INVENTED-FIELD-VALUE-9";
/// The fixed words that say an item is a key Theseus itself runs on.
pub(super) const RUNS_ON_WORDS: &str = "a key Theseus itself runs on";

/// One request the server saw.
#[derive(Debug, Clone)]
pub(super) struct Hit {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// A test's HTTP/1.1 server on 127.0.0.1 that keeps each request whole.
pub(super) struct Server {
    pub port: u16,
    pub hits: Arc<Mutex<Vec<Hit>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    pub fn seen(&self) -> Vec<Hit> {
        self.hits.lock().unwrap().clone()
    }
}

/// The value in the four spellings an answer may echo it in.
pub(super) fn echoes() -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(format!("lamplighter:{VALUE}"));
    let pct = format!("key%3D{VALUE}");
    format!(
        "{{\"verbatim\": \"{VALUE}\", \"basic\": \"{b64}\", \"percent\": \"{pct}\", \"json\": {}}}",
        serde_json::to_string(&format!("a\"{VALUE}")).unwrap()
    )
}

/// What the server answers each path with: status, headers, body, and how long it waits first.
fn route(
    method: &str,
    path: &str,
    port: u16,
    other: u16,
) -> (u16, Vec<(String, String)>, Vec<u8>, u64) {
    let json = |s: &str| ("Content-Type".to_string(), s.to_string());
    match path {
        "/v1/things" if method == "GET" => (
            200,
            vec![json("application/json")],
            br#"[{"id":"t0"}]"#.to_vec(),
            0,
        ),
        "/v1/things" => (
            201,
            vec![
                json("application/json"),
                ("Location".into(), "/v1/things/t1".into()),
            ],
            br#"{"id":"t1"}"#.to_vec(),
            0,
        ),
        "/echo" => (
            200,
            vec![
                json("application/json"),
                ("ETag".into(), format!("\"{VALUE}\"")),
            ],
            echoes().into_bytes(),
            0,
        ),
        "/away" => (
            307,
            vec![(
                "Location".into(),
                format!("http://other.test:{other}/landing?k={VALUE}"),
            )],
            vec![],
            0,
        ),
        "/near" => (
            302,
            vec![(
                "Location".into(),
                format!("http://site.test:{port}/v1/things"),
            )],
            vec![],
            0,
        ),
        "/bad" => (
            422,
            vec![json("application/json")],
            br#"{"error":"the field name is missing"}"#.to_vec(),
            0,
        ),
        "/upload" => (
            200,
            vec![json("application/json")],
            br#"{"ok":true}"#.to_vec(),
            0,
        ),
        "/slow" => (200, vec![json("text/plain")], b"late".to_vec(), 3_000),
        "/hang" => (200, vec![json("text/plain")], b"never".to_vec(), 600_000),
        _ => (404, vec![json("text/plain")], b"no such page".to_vec(), 0),
    }
}

/// A server whose redirect goes to the server on port `other`.
pub(super) async fn serve_to(other: u16) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let seen = hits.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let hits = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let head_end = loop {
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                    match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let mut lines = head.split("\r\n");
                let first: Vec<&str> = lines.next().unwrap_or("").split(' ').collect();
                let (method, path) = (
                    first[0].to_string(),
                    first.get(1).unwrap_or(&"/").to_string(),
                );
                let headers: BTreeMap<String, String> = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                    .collect();
                let len: usize = headers
                    .get("content-length")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let mut body = buf[head_end..].to_vec();
                while body.len() < len {
                    match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => body.extend_from_slice(&chunk[..n]),
                    }
                }
                let bare = path.split('?').next().unwrap_or("/").to_string();
                hits.lock().unwrap().push(Hit {
                    method: method.clone(),
                    path,
                    headers,
                    body,
                });
                let (status, extra, data, delay) = route(&method, &bare, port, other);
                if delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                let mut out = format!(
                    "HTTP/1.1 {status} X\r\nConnection: close\r\nContent-Length: {}\r\n",
                    data.len()
                );
                for (k, v) in extra {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.write_all(&data).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Server { port, hits, task }
}

pub(super) async fn serve() -> Server {
    serve_to(1).await
}

/// A daemon with the stub vault and `http.request` over a test's client: `site.test` and
/// `other.test` reach the servers, taken as public when `public`.
pub(super) struct Req {
    pub r: Rig,
    pub vault: PathBuf,
    pub server: Server,
    _d: tempfile::TempDir,
}

pub(super) async fn rig_req_with(
    outside: OutsideMode,
    public: bool,
    tweak: impl FnOnce(&mut crate::Config) + Send + 'static,
) -> Req {
    let server = serve().await;
    let web = crate::web::tests::web(server.port, WebToolsConfig::default(), public);
    rig_on(server, web, outside, tweak)
}

pub(super) fn rig_on(
    server: Server,
    web: Arc<crate::web::Web>,
    outside: OutsideMode,
    tweak: impl FnOnce(&mut crate::Config) + Send + 'static,
) -> Req {
    let mut tools = web.tools();
    tools.push(web.request_plaintext());
    let r = rig_with_tools(tools, move |c, _| {
        c.content_based_security.outside = outside;
        tweak(c);
    });
    let d = tempfile::tempdir().unwrap();
    r.core.tools.look.set_vault(Arc::new(stub(d.path(), "ok")));
    // The owner's turns are his own typed input, as `turn.submit` judges it.
    r.owner_types();
    Req {
        r,
        vault: d.path().to_path_buf(),
        server,
        _d: d,
    }
}

pub(super) async fn rig_req() -> Req {
    rig_req_with(OutsideMode::default(), true, |_| {}).await
}

/// A call: `method` to `path` on `site.test`, the key as a bearer token, with `more`.
pub(super) fn call(port: u16, method: &str, path: &str, more: Value) -> Value {
    let mut c = json!({
        "method": method,
        "url": format!("http://site.test:{port}{path}"),
        "key": {"vault": "Harbor", "item": "Lantern key", "field": "credential", "as": {"bearer": {}}},
        "reason": "send the lantern service its things"
    });
    if let (Some(c), Value::Object(m)) = (c.as_object_mut(), more) {
        c.extend(m);
    }
    c
}

/// The FYI notes the owner was told of a request.
pub(super) fn notes(r: &Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter_map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|t| t.starts_with("ℹ️ FYI") && t.contains("sent in a request"))
        .collect()
}

/// Whether the vault was asked for a value.
pub(super) fn read_vault(o: &Req) -> bool {
    logged(&o.vault).iter().any(|l| l.starts_with("read"))
}

/// The latest result of the call `call` (each turn's calls are `p1`, `p2`, …): its status and text.
pub(super) fn last(r: &Rig, sid: &str, call: &str) -> (ResultStatus, String) {
    let (status, text, _, _) = node_of(r, sid, call);
    (status, text)
}

/// The latest result node of the call `call`: its status, its stored text, its meta and its marker.
pub(super) fn node_of(
    r: &Rig,
    sid: &str,
    call: &str,
) -> (ResultStatus, String, Value, Option<String>) {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult {
                tool_use_id,
                status,
                content,
                meta,
                external,
                ..
            } if tool_use_id == call => Some((status, content, meta, external.map(|e| e.url))),
            _ => None,
        })
        .expect("the call's result")
}

/// "Send the lantern service a new thing with my key": one step, the server has the key as a
/// bearer token, no card, the settings untouched, one FYI note naming the item, the host and the
/// path, and none of the model's words.
#[tokio::test]
async fn a_key_goes_in_one_step_with_no_card_and_no_settings_change() {
    let o = rig_req().await;
    let r = &o.r;
    let sid = r.session();
    let body = json!({"json": {"name": "lamp"}});
    r.steps(
        &sid,
        vec![(
            "http_request",
            call(o.server.port, "POST", "/v1/things", body),
        )],
    )
    .await;
    let (status, text) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("201") && text.contains(r#"{"id":"t1"}"#),
        "{text}"
    );
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    assert_eq!(r.file_text(), crate::config_change::tests::FILE);
    let seen = o.server.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].headers["authorization"], format!("Bearer {VALUE}"));
    assert_eq!(seen[0].body, br#"{"name":"lamp"}"#);
    let told = notes(r);
    assert_eq!(told.len(), 1, "{told:?}");
    let short = crate::narrative::short(&sid);
    for part in [
        short.as_str(),
        "\"Lantern key\"",
        "\"credential\"",
        "\"Harbor\"",
        "a bearer token",
        "POST",
        "site.test",
        "/v1/things",
    ] {
        assert!(told[0].contains(part), "{part} in {}", told[0]);
    }
    assert!(
        !told[0].contains("send the lantern service"),
        "no model words: {}",
        told[0]
    );
    let rows = r.rows("secret.granted");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["once"], json!(true));
    assert_eq!(rows[0]["tool"], json!("http.request"));
    assert_eq!(rows[0]["request"]["method"], json!("POST"));
    assert!(rows[0]["request"]["host"]
        .as_str()
        .unwrap()
        .starts_with("site.test"));
    assert_eq!(rows[0]["vault"]["item"], json!("Lantern key"));
    // In the next span, two calls to the same host with the same item are told once.
    let get = || call(o.server.port, "GET", "/v1/things", json!({}));
    r.steps(&sid, vec![("http_request", get()), ("http_request", get())])
        .await;
    assert_eq!(notes(r).len(), 2, "one note a span: {:?}", notes(r));
    assert_eq!(o.server.seen().len(), 3);
}

/// The server echoes the key verbatim, in base64, percent-encoded and JSON-escaped, in its body
/// and in an ETag: the value is in no exit, in no stored node, ledger row or note, and in no
/// request to the model; after a restart, with an empty board, the stored node still has none.
#[tokio::test]
async fn the_value_is_in_no_exit_when_the_answer_echoes_it() {
    let o = rig_req().await;
    let sid = o.r.session();
    let basic = json!({"key": {"vault": "Harbor", "item": "Lantern key", "field": "credential", "as": {"basic": {"username": "lamplighter"}}}});
    o.r.steps(
        &sid,
        vec![("http_request", call(o.server.port, "GET", "/echo", basic))],
    )
    .await;
    let (status, text, meta, external) = node_of(&o.r, &sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("[redacted:vault-use:"),
        "masked where it was: {text}"
    );
    assert!(external.is_some());
    let all = super::tests_once::exits(&o.r, &sid);
    assert!(!all.contains(VALUE), "the value is in an exit");
    let b64 = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(format!("lamplighter:{VALUE}"))
    };
    for spelled in [VALUE.to_string(), b64] {
        assert!(
            !text.contains(&spelled) && !meta.to_string().contains(&spelled),
            "{text}"
        );
        assert!(!all.contains(&spelled), "in an exit: {spelled}");
    }
    let Req {
        r,
        server,
        vault,
        _d,
    } = o;
    let r = r.restart(|_, _| {});
    r.core.tools.look.set_vault(Arc::new(stub(&vault, "ok")));
    r.core.hold_vault_used();
    assert!(
        !r.core
            .secrets
            .states()
            .keys()
            .any(|k| k.starts_with("vault-use:")),
        "the board starts empty"
    );
    let (_, text, meta, _) = node_of(&r, &sid, "p1");
    assert!(
        !text.contains(VALUE) && !meta.to_string().contains(VALUE),
        "{text}"
    );
    assert!(!super::tests_once::exits(&r, &sid).contains(VALUE));
    drop(server);
}

/// `bearer`, `header{name, prefix}` and `basic{username}` each put the key in one header and
/// nowhere else; a header of the model's that names a key's place is refused; there is no place
/// in the query.
#[tokio::test]
async fn each_place_for_the_key_is_one_and_only_one() {
    use base64::Engine as _;
    let o = rig_req().await;
    let sid = o.r.session();
    let port = o.server.port;
    let key = |place: Value| json!({"key": {"vault": "Harbor", "item": "Lantern key", "field": "credential", "as": place}});
    let places = [
        (
            json!({"bearer": {}}),
            "authorization",
            format!("Bearer {VALUE}"),
        ),
        (
            json!({"header": {"name": "X-Lantern-Key", "prefix": "Token "}}),
            "x-lantern-key",
            format!("Token {VALUE}"),
        ),
        (
            json!({"basic": {"username": "lamplighter"}}),
            "authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("lamplighter:{VALUE}"))
            ),
        ),
    ];
    for (i, (place, header, want)) in places.iter().enumerate() {
        o.r.steps(
            &sid,
            vec![(
                "http_request",
                call(port, "GET", "/v1/things", key(place.clone())),
            )],
        )
        .await;
        let hit = &o.server.seen()[i];
        assert_eq!(hit.headers.get(*header), Some(want), "{place}");
        let carrying: Vec<&String> = hit
            .headers
            .iter()
            .filter(|(_, v)| v.contains(VALUE) || v.contains(&want[6..]))
            .map(|(k, _)| k)
            .collect();
        assert_eq!(carrying.len(), 1, "{place}: {carrying:?}");
        assert!(!hit.path.contains(VALUE), "{}", hit.path);
    }
    let n = o.server.seen().len();
    for bad in [
        json!({"headers": {"Authorization": "Bearer x"}}),
        json!({"headers": {"x-lantern-key": "x"}, "key": {"vault": "Harbor", "item": "Lantern key", "field": "credential", "as": {"header": {"name": "X-Lantern-Key"}}}}),
        json!({"key": {"vault": "Harbor", "item": "Lantern key", "field": "credential", "as": {"query": {"name": "key"}}}}),
    ] {
        o.r.steps(
            &sid,
            vec![("http_request", call(port, "GET", "/v1/things", bad.clone()))],
        )
        .await;
        let (status, text) = last(&o.r, &sid, "p1");
        assert_eq!(status, ResultStatus::Error, "{bad}: {text}");
    }
    assert_eq!(
        o.server.seen().len(),
        n,
        "nothing was sent for a refused call"
    );
}

/// A 307 to another server is returned, not followed: that server saw nothing; the answer shows
/// the status, the masked Location and the fixed words. So for a same-host 302.
#[tokio::test]
async fn a_redirect_is_not_followed_and_carries_no_key() {
    let second = serve().await;
    let first = serve_to(second.port).await;
    let web = crate::web::tests::web(first.port, WebToolsConfig::default(), true);
    let o = rig_on(first, web, OutsideMode::default(), |_| {});
    let sid = o.r.session();
    for (path, code) in [("/away", "307"), ("/near", "302")] {
        o.r.steps(
            &sid,
            vec![(
                "http_request",
                call(o.server.port, "POST", path, json!({"json": {"a": 1}})),
            )],
        )
        .await;
        let (_, text, _, external) = node_of(&o.r, &sid, "p1");
        assert!(text.contains(code), "{text}");
        assert!(text.contains("location: "), "{text}");
        assert!(
            text.contains(crate::web::request::send::NOT_FOLLOWED),
            "{text}"
        );
        assert!(!text.contains(VALUE), "{text}");
        assert!(external.is_some(), "labelled outside");
    }
    assert!(
        second.seen().is_empty(),
        "the second server saw nothing: {:?}",
        second.seen()
    );
    assert_eq!(o.server.seen().len(), 2, "one request each, no hop");
}

/// Only https (here, the test's http) at a public address: a private, loopback, link-local or
/// metadata address, `localhost`, a URL with a user, and one with a `#` part are refused before
/// the vault is read; a name that resolves to a private address is refused at connect, with
/// nothing sent.
#[tokio::test]
async fn only_https_at_a_public_address() {
    let o = rig_req().await;
    let sid = o.r.session();
    let p = o.server.port;
    for url in [
        "http://10.0.0.7/v1".to_string(),
        format!("http://127.0.0.1:{p}/v1/things"),
        "http://169.254.169.254/latest/meta-data/".into(),
        format!("http://[::1]:{p}/v1"),
        format!("http://localhost:{p}/v1"),
        format!("http://lamp:pw@site.test:{p}/v1/things"),
        format!("http://site.test:{p}/v1/things#top"),
        "ftp://site.test/v1".into(),
    ] {
        let mut c = call(p, "GET", "/v1/things", json!({}));
        c["url"] = json!(url);
        o.r.steps(&sid, vec![("http_request", c)]).await;
        let (status, text) = last(&o.r, &sid, "p1");
        assert_eq!(status, ResultStatus::Error, "{url}: {text}");
        assert!(!read_vault(&o), "{url}: the vault was read");
    }
    assert!(o.server.seen().is_empty());
    assert!(o.r.asked().is_empty(), "an input error, not a card");
    // A name that resolves to a loopback address that is not taken as public: refused at connect.
    let o = rig_req_with(OutsideMode::default(), false, |_| {}).await;
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![(
            "http_request",
            call(o.server.port, "GET", "/v1/things", json!({})),
        )],
    )
    .await;
    let (status, text) = o.r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.starts_with("Not sent:") && text.contains("did not connect"),
        "{text}"
    );
    assert!(o.server.seen().is_empty());
}

/// After an `http.fetch` of another host in the span, at `outside = notify` and at `ask`, the
/// call is one card with Approve and Decline, and the vault is not read; a flagged session asks.
#[tokio::test]
async fn a_turn_after_outside_text_asks_first_at_both_settings() {
    for mode in [OutsideMode::Notify, OutsideMode::Ask] {
        let o = rig_req_with(mode, true, |_| {}).await;
        let sid = o.r.session();
        let p = o.server.port;
        let fetch = json!({"url": format!("http://other.test:{p}/v1/things")});
        o.r.steps(
            &sid,
            vec![
                ("http_fetch", fetch),
                (
                    "http_request",
                    call(p, "POST", "/v1/things", json!({"json": {}})),
                ),
            ],
        )
        .await;
        let asked = o.r.asked();
        assert_eq!(asked.len(), 1, "{mode:?}: {asked:?}");
        assert!(!read_vault(&o), "{mode:?}: read before the answer");
        assert_eq!(
            o.server
                .seen()
                .iter()
                .filter(|h| h.method == "POST")
                .count(),
            0
        );
    }
    // A flagged session asks with nothing read since the owner's message.
    let o = rig_req().await;
    let sid = o.r.session();
    crate::test_support::label_session(&o.r.core, &sid, crate::content::Flag::Injection);
    o.r.steps(
        &sid,
        vec![(
            "http_request",
            call(o.server.port, "GET", "/v1/things", json!({})),
        )],
    )
    .await;
    assert_eq!(o.r.asked().len(), 1, "{:?}", o.r.asked());
    assert!(!read_vault(&o));
}

/// K3: a GET then a POST to the same host with the same item, in one span, asks nothing; the
/// same POST with another item, or to another host, asks.
#[tokio::test]
async fn the_same_vendors_answer_does_not_make_the_next_call_ask() {
    let o = rig_req().await;
    let sid = o.r.session();
    let p = o.server.port;
    o.r.steps(
        &sid,
        vec![
            ("http_request", call(p, "GET", "/v1/things", json!({}))),
            (
                "http_request",
                call(p, "POST", "/v1/things", json!({"json": {"name": "lamp"}})),
            ),
        ],
    )
    .await;
    assert!(o.r.asked().is_empty(), "{:?}", o.r.asked());
    assert_eq!(o.server.seen().len(), 2);
    for other in ["another item", "another host"] {
        let o = rig_req().await;
        let sid = o.r.session();
        let p = o.server.port;
        let mut second = call(p, "POST", "/v1/things", json!({"json": {}}));
        match other {
            "another item" => second["key"]["item"] = json!("Other key"),
            _ => second["url"] = json!(format!("http://other.test:{p}/v1/things")),
        }
        o.r.steps(
            &sid,
            vec![
                ("http_request", call(p, "GET", "/v1/things", json!({}))),
                ("http_request", second),
            ],
        )
        .await;
        assert_eq!(o.r.asked().len(), 1, "{other}: {:?}", o.r.asked());
        assert!(
            o.server.seen().iter().all(|h| h.method == "GET"),
            "{other}: nothing posted"
        );
    }
}

/// A task's session, a routine (`NEVER`), the self loop, an MCP session and a shared place are
/// each refused in the wall's words, with nothing read, held or sent.
#[tokio::test]
async fn nobody_but_the_owners_own_turn_may() {
    use theseus_kernel::Authority;
    assert!(crate::routine::grant::NEVER.contains(&crate::web::request::REQUEST));
    let server = serve().await;
    let web = crate::web::tests::web(server.port, WebToolsConfig::default(), true);
    let o = rig_on(server, web, OutsideMode::default(), |c| {
        c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
    });
    let r = &o.r;
    let mut sessions = Vec::new();
    for principal in [crate::rsi::backlog::PRINCIPAL, crate::mcp_server::PRINCIPAL] {
        let sid = r.session();
        let rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
        let e = r
            .core
            .kernel
            .open_execution(
                &sid,
                rec.kind,
                Authority {
                    principal: principal.into(),
                    ..Default::default()
                },
                None,
                None,
            )
            .unwrap();
        r.core
            .store
            .update_session(&sid, |rec: &mut crate::session::SessionRecord| {
                rec.execution_id = Some(e.id.clone());
                Ok(vec![])
            })
            .unwrap();
        sessions.push((principal.to_string(), sid));
    }
    let parent = crate::config_change::tests_work_global::conversation(r).await;
    let task = crate::config_change::tests_work_global::task(r, &parent, "Chart the reef", None);
    sessions.push(("task".into(), task.session_id.clone()));
    sessions.push(("shared".into(), r.session_at("channel:2")));
    let mut refused = Vec::new();
    for (who, sid) in sessions {
        r.turn_with(
            "http_request",
            &sid,
            call(o.server.port, "POST", "/v1/things", json!({"json": {}})),
        )
        .await;
        let result = r
            .core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                crate::node::Body::ToolResult {
                    tool_use_id,
                    status,
                    content,
                    ..
                } if tool_use_id == "p1" => Some((status, content)),
                _ => None,
            });
        // A place that does not offer the tool has no result for it: the call never ran.
        if let Some((status, text)) = result {
            assert_ne!(status, ResultStatus::Ok, "{who}: {text}");
        }
        refused.push((who, r.core.store.session_nodes(&sid).unwrap().len()));
    }
    assert!(!refused.is_empty());
    assert!(!read_vault(&o), "nothing read");
    assert!(o.server.seen().is_empty(), "nothing sent");
    assert!(
        !r.core
            .secrets
            .states()
            .keys()
            .any(|k| k.starts_with("vault-use:")),
        "nothing held"
    );
}

/// One response that fetches another host's page and then POSTs with a key: the gate judged the
/// POST before the page was read, so the hold is read again when it runs, and it is not sent,
/// with nothing read from the vault.
#[tokio::test]
async fn outside_text_in_the_same_response_stops_it() {
    let o = rig_req().await;
    let sid = o.r.session();
    let p = o.server.port;
    o.r.batch_next(vec![
        (
            "http_fetch",
            json!({"url": format!("http://other.test:{p}/v1/things")}),
        ),
        (
            "http_request",
            call(p, "POST", "/v1/things", json!({"json": {"n": 1}})),
        ),
    ]);
    o.r.run_turn(&sid).await;
    let (status, text) = last(&o.r, &sid, "p2");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains(super::request::AGAIN), "{text}");
    assert!(!read_vault(&o), "nothing read");
    assert!(
        o.server.seen().iter().all(|h| h.method == "GET"),
        "nothing posted"
    );
}
