//! A server that signs in, round 3 of its review (issue 55), through a whole
//! core and the real client: its own connection keeps the daemon's network
//! rule at connect and at every new connection; an `isError` result's words
//! reach the model masked where the core takes them; a tool named after the
//! token is scrubbed at its log, fact and notice; a begin that would show a
//! kept value is refused; one consent set for the CLI and the daemon; a
//! paste's codes held first, a bad paste ending its sign-in; a write past its
//! rename said as such.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_mcp::client::{HttpTarget, Options};
use theseus_mcp::fake::{self, Fake};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};
use theseus_mcp::{Client, Transport};

use super::oauth::{
    attached_by, call, kept_file, login, notes, rig, rig_on, rig_with, status, Rig, Sink,
};
use super::{ledgered, result_of, session, turn, until};
use crate::approval::Surface;
use crate::config::mcp::McpOauth;
use crate::node::ResultStatus;

fn opts() -> Options {
    Options {
        request_timeout: Duration::from_secs(10),
        call_timeout: Duration::from_secs(10),
        ..Options::default()
    }
}

/// The web fetch's own resolver, whose answer a test can change: a name
/// that answered a public address answers a private one.
struct Flip(Mutex<crate::web::net::Dns>);

impl reqwest::dns::Resolve for Flip {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let dns = self.0.lock().unwrap().clone();
        reqwest::dns::Resolve::resolve(&dns, name)
    }
}

/// A token source that always has the one the fake server takes.
struct Fixed;

const FIXED: &str = "fixed-bearer-value-0001";

impl theseus_mcp::TokenSource for Fixed {
    fn token(&self) -> BoxFuture<'_, Option<String>> {
        Box::pin(async { Some(FIXED.to_string()) })
    }
    fn refused<'a>(&'a self, _: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async { false })
    }
}

/// A signed-in connection is made by the daemon's rule: while the server's
/// name answers a public address, it connects; once it answers a private
/// one, the next request's new connection and a new connect are each
/// refused by the checking resolver, in fixed words, and nothing reaches
/// the server. A server with a fixed key at a plain, private address
/// connects as it always did.
#[tokio::test]
async fn a_signed_in_connection_reaches_only_a_public_address() {
    let server = Fake::new(fake::Config {
        bearer: Some(FIXED.into()),
        server_stream: false,
        ..Default::default()
    });
    let addr = server.clone().serve_http(0).await.unwrap();
    let lo: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    let mut dns = crate::web::net::Dns::checked();
    dns.hosts.insert("mcp.example.invalid".into(), vec![lo]);
    // Taken as public, as a test's server is, until it is not.
    dns.public = vec![lo];
    let flip = Arc::new(Flip(Mutex::new(dns)));
    let rule = theseus_mcp::oauth::Rule {
        resolver: flip.clone(),
        plaintext: |_| true,
        ..crate::mcp::oauth::daemon_rule()
    };
    let target = || {
        let mut t = HttpTarget::new(format!("http://mcp.example.invalid:{}/mcp", addr.port()));
        t.token = Some(Arc::new(Fixed));
        t.rule = Some(rule.clone());
        t.server_stream = false;
        t
    };
    let (c, _events) = Client::connect(Transport::Http(target()), opts())
        .await
        .unwrap();
    c.call_tool("echo", json!({"text": "public"}))
        .await
        .unwrap();
    let before = server.seen().messages.len();
    flip.0.lock().unwrap().public.clear();
    let e = c
        .call_tool("echo", json!({"text": "private"}))
        .await
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "the server is unreachable: its address is not public, so it was not reached: nothing \
         was sent"
    );
    let e = Client::connect(Transport::Http(target()), opts())
        .await
        .err()
        .unwrap();
    assert!(e.to_string().contains("its address is not public"), "{e}");
    assert_eq!(server.seen().messages.len(), before, "nothing reached it");
    // A literal private address is refused as written, at connect.
    let mut t = target();
    t.url = format!("https://10.0.0.5:{}/mcp", addr.port());
    t.rule = Some(crate::mcp::oauth::daemon_rule());
    let e = Client::connect(Transport::Http(t), opts())
        .await
        .err()
        .unwrap();
    assert!(e.to_string().contains("not public"), "{e}");
    // A fixed key at a plain, private address: the client it always had.
    let mut t = HttpTarget::new(format!("http://{addr}/mcp"));
    t.bearer = Some(FIXED.into());
    t.server_stream = false;
    let (c, _events) = Client::connect(Transport::Http(t), opts()).await.unwrap();
    c.ping().await.unwrap();
}

/// The words of a captured log.
fn capture() -> (Arc<Mutex<Vec<u8>>>, tracing::subscriber::DefaultGuard) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_env_filter("theseus_core=trace,theseus_mcp=trace,theseus_store=trace")
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(sub);
    (buf, guard)
}

/// Every token the fake issued, in any node, ledger row, note or log line.
fn leaked(r: &Rig, sid: &str, log: &str) -> Vec<String> {
    let nodes: Vec<String> = r
        .core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| format!("{n:?}"))
        .collect();
    let rows: Vec<String> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| format!("{row:?}"))
        .collect();
    let notices: Vec<String> = r
        .core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .map(|a| format!("{a:?}"))
        .collect();
    let all = [nodes, rows, notices, notes(r, sid), vec![log.to_string()]].concat();
    let text = all.join("\n");
    r.auth
        .seen()
        .secrets
        .into_iter()
        .filter(|v| text.contains(v.as_str()))
        .collect()
}

/// A call answered with `isError = true` and the very token it was sent in
/// its content and structured content: the model reads the server's words
/// with the token masked; no node, row, note or log line holds it; a result
/// prints nothing of itself in `Debug`. An ordinary result is as it came.
#[tokio::test]
async fn an_error_result_is_masked_where_the_core_takes_it() {
    let (buf, _guard) = capture();
    let r = rig().await;
    let sid = session(&r.core, None);
    attached_by(&r, &sid);
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    turn(&r.core, &sid, "use the docs").await;
    let (st, text, _) = result_of(&r.core, &sid, "o1");
    assert_eq!(
        (st, text.as_str()),
        (ResultStatus::Ok, "signed in"),
        "as it came"
    );
    r.auth.set(|c| c.error_echo = true);
    let sid2 = session(&r.core, None);
    turn(&r.core, &sid2, "again").await;
    let (st, text, _) = result_of(&r.core, &sid2, "o1");
    assert_ne!(st, ResultStatus::Ok);
    assert!(text.contains("refused: [redacted:"), "{text}");
    let token = r.auth.last_bearer().unwrap();
    assert!(!text.contains(&token));
    let result: theseus_mcp::types::CallToolResult = serde_json::from_value(json!({
        "content": [{"type": "text", "text": token}],
        "structuredContent": {"token": token}, "isError": true}))
    .unwrap();
    assert!(!format!("{result:?}").contains(&token));
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    for s in [&sid, &sid2] {
        let found = leaked(&r, s, &log);
        assert!(found.is_empty(), "{} values leaked", found.len());
    }
}

/// A tool named after the token, listed again after a `list_changed`: the
/// change's log line, its fact and its notice hold the name scrubbed.
#[tokio::test]
async fn a_tool_named_after_the_token_is_scrubbed_where_it_is_said() {
    let (buf, _guard) = capture();
    let r = rig().await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    r.auth.set(|c| c.tool_named_token = true);
    turn(&r.core, &sid, "use the docs").await;
    until("the change's row", || {
        !ledgered(&r.core, "mcp.tools_changed").is_empty()
    })
    .await;
    until("the notice", || {
        r.core
            .outbox
            .open_for(crate::outbox::OPERATOR_TARGET)
            .iter()
            .any(|a| crate::outbox::kind_of(a) == "mcp_changed")
    })
    .await;
    let row = &ledgered(&r.core, "mcp.tools_changed")[0];
    assert!(row["added"].to_string().contains("[redacted:"), "{row}");
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(log.contains("an MCP server's tools changed"));
    let found = leaked(&r, &sid, &log);
    assert!(found.is_empty(), "{} values leaked", found.len());
}

/// A begin that would show a value the daemon keeps is refused in constant
/// words, and shows nothing: a client id that is its client secret, and an
/// old token given as the client id.
#[tokio::test]
async fn a_begin_that_would_show_a_kept_value_is_refused() {
    let r = rig_on(
        FakeAuth::start(AuthConfig {
            client_id_is_secret: true,
            ..AuthConfig::default()
        })
        .await,
        |_| {},
    )
    .await;
    r.core.mcp.start();
    let refused = |v: &Value| {
        v["error"]["message"].as_str().unwrap_or_default()
            == "the sign-in would show a value Theseus keeps secret, so it is refused: nothing \
                is shown"
    };
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    assert!(refused(&begun), "{begun}");
    let secret = r
        .auth
        .seen()
        .secrets
        .into_iter()
        .find(|v| v.starts_with("fake-cs-"));
    assert!(!begun.to_string().contains(&secret.unwrap()));
    // An old token, aliased as the client id of a later registration.
    let r = rig().await;
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    let old = r.auth.last_bearer().unwrap();
    r.auth.set(|c| c.client_id_alias = Some(old.clone()));
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    assert!(refused(&begun), "{begun}");
    assert!(!begun.to_string().contains(&old));
}

/// Every endpoint at one other public place, its metadata saying the
/// redirect names its issuer: the begin names that place (the CLI asks), a
/// finish without the owner's agreement ends it, one with it signs in.
#[tokio::test]
async fn one_consent_set_for_the_cli_and_the_daemon() {
    let elsewhere = FakeAuth::start(AuthConfig::default()).await;
    let issuer = FakeAuth::start(AuthConfig {
        endpoints_at: Some(elsewhere.base()),
        iss_supported: true,
        ..AuthConfig::default()
    })
    .await;
    elsewhere.set(|c| c.issuer = Some(issuer.base()));
    let r = rig_with(issuer.clone(), McpOauth::default(), |_| {}).await;
    r.core.mcp.start();
    let place = elsewhere.base().trim_start_matches("http://").to_string();
    for agreed in [false, true] {
        let begun = call(
            &r.core,
            Surface::Cli,
            "mcp.login.begin",
            json!({"name": "docs"}),
        )
        .await;
        assert_eq!(begun["elsewhere"], json!([place]), "{begun}");
        elsewhere.browse(begun["url"].as_str().unwrap(), true).await;
        let done = call(
            &r.core,
            Surface::Cli,
            "mcp.login.finish",
            json!({"login_id": begun["login_id"], "wait_secs": 10, "agreed": agreed}),
        )
        .await;
        match agreed {
            false => assert!(
                done["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("not agreed to"),
                "{done}"
            ),
            true => assert_eq!(done["state"], "done", "{done}"),
        }
    }
    assert!(kept_file(r.dir.path()).exists());
}

/// A paste's codes are held before anything is checked (a wrong sign-in's
/// id); a paste that is not the redirect ends its sign-in and closes its
/// listener at once.
#[tokio::test]
async fn a_paste_is_held_first_and_a_bad_one_ends_its_sign_in() {
    let r = rig().await;
    r.core.mcp.start();
    // A sign-in is open; the paste names another.
    call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    let stray = "http://127.0.0.1:1/callback?code=code-of-a-wrong-sign-in&state=x";
    let e = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": "not-this-one", "pasted": stray}),
    )
    .await;
    assert!(e["error"].is_object(), "{e}");
    let (cut, _) =
        crate::scrub::Scrubber::from_board(r.core.secrets.clone()).scrub("code-of-a-wrong-sign-in");
    assert!(cut.contains("[redacted:"), "the code is known: {cut}");
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    let port = reqwest::Url::parse(begun["redirect"].as_str().unwrap())
        .unwrap()
        .port()
        .unwrap();
    let e = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "pasted": "not the address"}),
    )
    .await;
    assert!(e["error"].is_object(), "{e}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err(),
        "its listener closed"
    );
    let sent_to = r.auth.browse(begun["url"].as_str().unwrap(), false).await;
    let e = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "pasted": sent_to}),
    )
    .await;
    assert!(
        e["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no sign-in is open"),
        "{e}"
    );
}

/// A write that fails after its rename (its directory not synced) says the
/// file may already hold the sign-in: never "nothing changed".
#[tokio::test]
async fn a_write_past_its_rename_says_the_file_may_hold_it() {
    let r = rig().await;
    r.core.mcp.start();
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    r.auth.browse(begun["url"].as_str().unwrap(), true).await;
    crate::mcp::oauth::FAIL_DIR_SYNC.with(|f| f.set(true));
    let done = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "wait_secs": 10}),
    )
    .await;
    crate::mcp::oauth::FAIL_DIR_SYNC.with(|f| f.set(false));
    let e = done["error"]["message"].as_str().unwrap_or_default();
    assert!(e.contains("its file may already hold it"), "{done}");
    assert!(!e.contains("nothing changed"), "{done}");
    assert!(kept_file(r.dir.path()).exists());
    assert_eq!(status(&r).state, "needs sign-in");
}
