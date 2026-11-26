//! A server that signs in, and what a hostile one, a failing disk or an
//! interrupted sign-in can do (issue 55), through a whole core: the real
//! board, the real HTTP client, the protocol calls the CLI makes, and the
//! fakes on loopback. Nothing a server writes reaches a status, a log line,
//! a ledger row, a note or an error; every value of all five kinds is known
//! to the scrubber; a mixed-up authorization server is never sent a code;
//! a listener lives only while its sign-in waits; a write that fails fails
//! the sign-in; a file that cannot be removed is said; a loss renews nothing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};
use theseus_protocol::mcp::McpLoginFinishParams;

use super::oauth::{
    attached_by, call, kept_file, login, notes, rig_on, rig_with, status, Rig, Sink,
};
use super::{session, turn, until};
use crate::approval::Surface;
use crate::config::mcp::McpOauth;

fn rows(r: &Rig, kind: &str) -> Vec<String> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| format!("{row:?}"))
        .collect()
}

/// A hostile server's every trick, then the scan: the token it was sent,
/// echoed in a JSON-RPC error's message and data on each call and on the
/// list a restart reads, and in its 401's `WWW-Authenticate` across more
/// renewals than any window of names would hold; then a refusal for good,
/// its note, and a sign-out. No value of any of the five kinds (code,
/// verifier, access token, refresh token, client secret) is in any node,
/// ledger row, note, log line, status, listing, health, or anything the CLI
/// was given, the pasted address's `Debug` included; and the scrubber knows
/// every one.
#[tokio::test]
async fn nothing_a_hostile_server_writes_reaches_a_word_and_the_scrubber_knows_all() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_env_filter("theseus_core=trace,theseus_mcp=trace,theseus_store=trace")
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    let auth = FakeAuth::start(AuthConfig {
        client_secret: true,
        ..AuthConfig::default()
    })
    .await;
    let r = rig_on(auth, |_| {}).await;
    let sid = session(&r.core, None);
    attached_by(&r, &sid);
    r.core.mcp.start();
    let (done, mut said) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    turn(&r.core, &sid, "use the docs").await;
    r.auth.set(|c| c.echo = true);
    turn(&r.core, &sid, "again").await;
    // Six renewals, each after a 401 that echoes the token it refused.
    for i in 0..6 {
        r.auth.expire_access();
        turn(&r.core, &sid, &format!("round {i}")).await;
    }
    assert!(r.auth.seen().refreshes >= 6, "{}", r.auth.seen().refreshes);
    // A restart lists the tools again, and the list is the echo: a crash,
    // its row and its log line.
    r.core.mcp.restart("docs");
    until("a crash's row", || !rows(&r, "mcp.exited").is_empty()).await;
    r.auth.revoke_all();
    until("needs sign-in", || status(&r).state == "needs sign-in").await;
    until("the note", || !notes(&r, &sid).is_empty()).await;
    said.push(
        call(&r.core, Surface::Cli, "mcp.logout", json!({"name": "docs"}))
            .await
            .to_string(),
    );
    said.push(format!("{:?}", r.core.mcp.status()));
    for m in ["mcp.list", "health"] {
        said.push(
            call(&r.core, Surface::Cli, m, Value::Null)
                .await
                .to_string(),
        );
    }
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    scan(&r, &sid, said, &log);
}

/// The scan: every value of all five kinds is in no node, ledger row, note,
/// log line or word said, and the scrubber knows each; and the sinks scanned
/// were written to.
fn scan(r: &Rig, sid: &str, mut said: Vec<String>, log: &str) {
    let secrets = r.auth.seen().secrets;
    let code = secrets
        .iter()
        .find(|v| v.starts_with("fake-code-"))
        .unwrap()
        .clone();
    said.push(format!(
        "{:?}",
        McpLoginFinishParams {
            login_id: "x".into(),
            pasted: Some(format!("http://127.0.0.1:1/callback?code={code}&state=s")),
            ..Default::default()
        }
    ));
    let nodes: Vec<String> = r
        .core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| format!("{n:?}"))
        .collect();
    let all_rows: Vec<String> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| format!("{row:?}"))
        .collect();
    let note = notes(r, sid);
    assert_eq!(note.len(), 1, "{note:?}");
    // The sinks scanned were written to.
    for seen in [
        "MCP server ended; restarting",
        "an MCP server's sign-in is lost",
        "an MCP server's sign-in was renewed",
    ] {
        assert!(log.contains(seen), "the log has no {seen:?}");
    }
    assert!(all_rows.iter().any(|r| r.contains("mcp.exited")));
    // All five kinds.
    for prefix in ["fake-code-", "fake-at-", "fake-rt-", "fake-cs-"] {
        assert!(secrets.iter().any(|v| v.starts_with(prefix)), "no {prefix}");
    }
    assert!(
        secrets.iter().any(|v| !v.starts_with("fake-")),
        "no verifier"
    );
    assert!(
        secrets.iter().filter(|v| v.starts_with("fake-at-")).count() > 6,
        "more access tokens than any window"
    );
    let scrubber = crate::scrub::Scrubber::from_board(r.core.secrets.clone());
    for value in &secrets {
        for (what, text) in [
            ("a node", nodes.join("\n")),
            ("the ledger", all_rows.join("\n")),
            ("a note", note.join("\n")),
            ("a log line", log.to_string()),
            ("what was said", said.join("\n")),
        ] {
            assert!(!text.contains(value.as_str()), "a value is in {what}");
        }
        let (cut, _) = scrubber.scrub(value);
        assert!(
            !cut.contains(value.as_str()),
            "the scrubber does not know a value"
        );
    }
    assert!(!log.contains("harbour portal"), "a body in a log line");
}

/// A token too short to keep from view is a bad answer: the sign-in fails,
/// nothing is kept, and the server still needs a sign-in.
#[tokio::test]
async fn a_token_too_short_to_hide_is_never_kept() {
    let auth = FakeAuth::start(AuthConfig {
        short_access: true,
        ..AuthConfig::default()
    })
    .await;
    let r = rig_on(auth, |_| {}).await;
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    let e = done["error"]["message"].as_str().unwrap_or_default();
    assert!(e.contains("not its JSON"), "{done}");
    assert!(!kept_file(r.dir.path()).exists());
    assert_eq!(status(&r).state, "needs sign-in");
}

/// A mix-up: the server's authorization server names an honest server's
/// authorization endpoint and its own token endpoint. The begin names the
/// other place; without the owner's agreement the sign-in ends; with it, the
/// honest server's redirect, which does not name the hostile issuer, is
/// refused. No credential ever reaches the hostile token endpoint.
#[tokio::test]
async fn a_mixed_up_sign_in_sends_no_code_to_the_other_server() {
    let honest = FakeAuth::start(AuthConfig {
        iss: false,
        ..AuthConfig::default()
    })
    .await;
    let hostile = FakeAuth::start(AuthConfig {
        authorization_endpoint: Some(format!("{}/authorize", honest.base())),
        iss_supported: true,
        ..AuthConfig::default()
    })
    .await;
    let r = rig_with(
        hostile.clone(),
        McpOauth {
            client_id: Some("settings-client".into()),
            ..McpOauth::default()
        },
        |_| {},
    )
    .await;
    r.core.mcp.start();
    // The one consent set: the origins not the issuer's (here, the honest
    // server's authorization endpoint).
    let honest_place = honest.base().trim_start_matches("http://").to_string();
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    assert_eq!(begun["elsewhere"], json!([honest_place]), "{begun}");
    let refused = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "wait_secs": 1}),
    )
    .await;
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not agreed to"),
        "{refused}"
    );
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    honest.browse(begun["url"].as_str().unwrap(), true).await;
    let done = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "wait_secs": 10, "agreed": true}),
    )
    .await;
    assert!(
        done["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not name its issuer"),
        "{done}"
    );
    assert!(
        !hostile.seen().paths.iter().any(|p| p == "/token"),
        "{:?}",
        hostile.seen().paths
    );
    assert!(!kept_file(r.dir.path()).exists());
}

/// Whether anything listens on `port` of this machine.
async fn listens(port: u64) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port as u16))
        .await
        .is_ok()
}

fn port_of(begun: &Value) -> u64 {
    let u = reqwest::Url::parse(begun["redirect"].as_str().unwrap()).unwrap();
    u64::from(u.port().unwrap())
}

/// A sign-in's listener lives only while it waits: a second begin closes the
/// first's, and the end of a sign-in closes its own.
#[tokio::test]
async fn a_listener_lives_only_while_its_sign_in_waits() {
    let r = super::oauth::rig().await;
    r.core.mcp.start();
    let first = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    let p1 = port_of(&first);
    assert!(listens(p1).await);
    let second = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    let p2 = port_of(&second);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !listens(p1).await,
        "the replaced sign-in's listener is closed"
    );
    let sent_to = r.auth.browse(second["url"].as_str().unwrap(), false).await;
    let done = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": second["login_id"], "pasted": sent_to}),
    )
    .await;
    assert_eq!(done["state"], "done", "{done}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !listens(p2).await,
        "the finished sign-in's listener is closed"
    );
}

/// A first write that fails fails the sign-in, in plain words: nothing is
/// kept or used, and the server still needs a sign-in.
#[tokio::test]
async fn a_sign_in_that_cannot_be_written_is_not_used() {
    let r = rig_on(FakeAuth::start(AuthConfig::default()).await, |store| {
        std::fs::create_dir_all(store).unwrap();
        // Where the sign-ins' directory goes, a file: no write can land.
        std::fs::write(crate::mcp::oauth::dir(store), b"in the way").unwrap();
    })
    .await;
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    let e = done["error"]["message"].as_str().unwrap_or_default();
    assert!(e.contains("could not be kept"), "{done}");
    assert!(e.contains("theseus mcp login docs"), "{done}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(status(&r).state, "needs sign-in");
}

/// A sign-out whose file cannot be removed says so, and never says
/// "forgotten".
#[tokio::test]
async fn a_sign_out_that_cannot_remove_its_file_says_so() {
    let r = super::oauth::rig().await;
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    let p = kept_file(r.dir.path());
    std::fs::remove_file(&p).unwrap();
    std::fs::create_dir_all(p.join("in-the-way")).unwrap();
    let out = call(&r.core, Surface::Cli, "mcp.logout", json!({"name": "docs"})).await;
    assert_eq!(out["kept_remains"], true, "{out}");
    assert_eq!(out["forgot"], false, "{out}");
    assert!(
        out["note"]
            .as_str()
            .unwrap()
            .contains("could not be removed"),
        "{out}"
    );
}

/// A renewal refused for good before a request (its token about to run out)
/// loses the sign-in at once: no token is sent from then, nothing renews,
/// and the server needs a sign-in; the authorization server saw one refusal.
#[tokio::test]
async fn a_lost_sign_in_sends_and_renews_nothing() {
    let auth = FakeAuth::start(AuthConfig {
        access_ttl: 30,
        ..AuthConfig::default()
    })
    .await;
    let r = rig_on(auth, |_| {}).await;
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    r.auth.set(|c| c.refuse_refresh = true);
    let client = r
        .core
        .mcp
        .server("docs")
        .unwrap()
        .client(Duration::from_secs(5))
        .await
        .unwrap();
    let unauthorized = r.fake.seen().unauthorized;
    for _ in 0..3 {
        assert!(client
            .call_tool("echo", json!({"text": "x"}))
            .await
            .is_err());
    }
    until("needs sign-in", || status(&r).state == "needs sign-in").await;
    r.core.mcp.stop();
    drop(client);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(r.auth.seen().refused_refreshes, 1, "one refusal, no loop");
    assert_eq!(
        r.fake.seen().unauthorized,
        unauthorized,
        "no token was sent after the loss"
    );
}
