//! A server that signs in, round 5 of its review (issue 55): a value is not
//! kept if it equals anything Theseus already shows, checked at the doors
//! it enters by (a sign-in's redemption, every renewal); a signed-in
//! server's tools are not seeded from the store; a server with no sign-in
//! says its changed tools as main does; the count is said once per change
//! of the dropped set's names. Every test scans every exit of the whole test.

use std::time::Duration;

use serde_json::{json, Value};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};

use super::oauth::{call, login, rig_edit, Rig};
use super::oauth_exits::{all_exits, capture, issued_in};
use super::{ledgered, session, turn, until};
use crate::approval::Surface;
use crate::config::mcp::McpOauth;
use crate::config::McpServerConfig;
use crate::mcp::doors::{RENEWAL_SHOWN, SHOWN_WORDS};
use crate::mcp::State;

/// A configured name long enough to be a value the scrubber keeps.
pub(super) const NAME: &str = "oauth_notes_1";

/// A server with no sign-in at `url`.
fn plain_at(url: String) -> McpServerConfig {
    McpServerConfig {
        command: vec![],
        env: Default::default(),
        url: Some(url),
        auth_secret: None,
        oauth: None,
        read: vec![],
        sandbox: Default::default(),
        egress: vec![],
        frozen: None,
        external: true,
        enabled: true,
        start_timeout_secs: 10,
        call_timeout_secs: 30,
    }
}

/// `docs` under `NAME`, which signs in at a fake issuer configured by
/// `auth`, and `plain`, a server with no sign-in served by its own fake.
pub(super) async fn rig_doors(auth: AuthConfig) -> (Rig, std::sync::Arc<theseus_mcp::fake::Fake>) {
    rig_doors_with(auth, |_| {}).await
}

/// `rig_doors`, `edit` on `plain`'s settings first.
pub(super) async fn rig_doors_with(
    auth: AuthConfig,
    edit: impl FnOnce(&mut McpServerConfig),
) -> (Rig, std::sync::Arc<theseus_mcp::fake::Fake>) {
    let plain = theseus_mcp::fake::Fake::new(theseus_mcp::fake::Config {
        name: "plain".into(),
        ..Default::default()
    });
    let addr = plain.clone().serve_http(0).await.unwrap();
    let r = rig_edit(
        FakeAuth::start(auth).await,
        McpOauth::default(),
        |_| {},
        |cfg| {
            let docs = cfg.mcp.servers.remove("docs").unwrap();
            cfg.mcp.servers.insert(NAME.into(), docs);
            let mut p = plain_at(format!("http://{addr}/mcp"));
            edit(&mut p);
            cfg.mcp.servers.insert("plain".into(), p);
        },
    )
    .await;
    (r, plain)
}

pub(super) fn state(r: &Rig, name: &str) -> String {
    r.core
        .mcp
        .status()
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.state)
        .unwrap_or_default()
}

/// The sign-in of `NAME`, as the CLI makes it: begin, the browser, finish.
pub(super) async fn sign_in(r: &Rig) -> Value {
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": NAME}),
    )
    .await;
    let url = begun["url"].as_str().unwrap_or_else(|| panic!("{begun}"));
    r.auth.browse(url, true).await;
    call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "wait_secs": 10}),
    )
    .await
}

/// Until `plain`'s list holds `tool`.
pub(super) async fn plain_lists(r: &Rig, tool: &str) {
    until("plain's list", || {
        r.core
            .mcp
            .catalog
            .all()
            .iter()
            .any(|t| t.server.name == "plain" && t.listed.name == tool)
    })
    .await;
}

/// A redemption whose access token equals something Theseus already
/// shows (the server's configured name, its host, a tool's name in the
/// current list, a string inside a tool's schema) is not kept: the finish
/// fails in fixed words, nothing is written, the server still needs its
/// sign-in, the candidate was never sent and stays known to the scrubber,
/// and no other value the issuer gave is anywhere. The clean control signs
/// in.
#[tokio::test]
async fn a_redemption_that_returns_a_shown_value_is_not_kept() {
    let cases = [
        ("the server's configured name", NAME.to_string()),
        ("its host", "127.0.0.1".to_string()),
        (
            "a tool's name in the current list",
            "says_schema_key".to_string(),
        ),
        (
            "a string inside a tool's schema",
            "only-in-a-schema-01".to_string(),
        ),
    ];
    for (what, candidate) in cases {
        let (buf, _guard) = capture();
        let (r, plain) = rig_doors(AuthConfig {
            access_token_is: Some(candidate.clone()),
            ..AuthConfig::default()
        })
        .await;
        let sid = session(&r.core, None);
        plain.set_says(Some("plain-says-a-value".into()));
        plain.set_schema_holds(Some("only-in-a-schema-01".into()));
        r.core.mcp.start();
        plain_lists(&r, "holds_in_schema").await;
        let done = sign_in(&r).await;
        assert_eq!(done["error"]["message"], SHOWN_WORDS, "{what}: {done}");
        assert!(
            !crate::mcp::oauth::file(&crate::mcp::oauth::dir(&r.dir.path().join("store")), NAME)
                .exists(),
            "{what}: nothing is written"
        );
        assert_eq!(state(&r, NAME), State::NeedsSignIn.as_str(), "{what}");
        assert_ne!(
            r.auth.last_bearer().as_deref(),
            Some(candidate.as_str()),
            "{what}"
        );
        let known = crate::scrub::Scrubber::from_board(r.core.secrets.clone());
        assert!(
            known.scrub(&candidate).1 > 0,
            "{what}: the candidate stays held"
        );
        turn(&r.core, &sid, "use the docs").await;
        let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
        let exits = all_exits(&r, &sid, &log).await;
        let found = issued_in(&r, &exits, &[&candidate]);
        assert!(
            found.is_empty(),
            "{what}: {} values in the exits",
            found.len()
        );
    }
    // The clean control: a redemption that shows nothing signs in.
    let (r, _plain) = rig_doors(AuthConfig::default()).await;
    r.core.mcp.start();
    let done = sign_in(&r).await;
    assert_eq!(done["state"], "done", "{done}");
    assert!(
        crate::mcp::oauth::file(&crate::mcp::oauth::dir(&r.dir.path().join("store")), NAME)
            .exists()
    );
}

/// A renewal whose access token equals a tool's name in the list already
/// accepted, with no `list_changed`: the renewal is refused, the sign-in is
/// lost for good (the server needs a sign-in again), the candidate is never
/// sent nor written, and no other value is in any exit of the whole test.
/// The clean control: the renewals before it kept the server connected.
#[tokio::test]
async fn a_renewal_that_returns_a_shown_value_is_refused() {
    let (buf, _guard) = capture();
    // Every token runs out within the renewal margin: each is renewed.
    let (r, _plain) = rig_doors(AuthConfig {
        access_ttl: 30,
        ..AuthConfig::default()
    })
    .await;
    let sid = session(&r.core, None);
    r.fake.set_says(Some("docs-says-a-value".into()));
    r.core.mcp.start();
    let done = sign_in(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    until("docs' list", || {
        r.core
            .mcp
            .catalog
            .all()
            .iter()
            .any(|t| t.server.name == NAME && t.listed.name == "says_schema_key")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        r.auth.seen().refreshes >= 1,
        "the clean renewals went through"
    );
    assert_eq!(state(&r, NAME), State::Ready.as_str());
    let candidate = "says_schema_key".to_string();
    r.auth
        .set(|c| c.renewed_access_is = Some(candidate.clone()));
    // A request: its token is due, so it is renewed, and refused.
    let client = r
        .core
        .mcp
        .server(NAME)
        .unwrap()
        .client(Duration::from_secs(5))
        .await
        .unwrap();
    let _ = client.call_tool("echo", json!({"text": "a"})).await;
    until("the sign-in lost", || {
        state(&r, NAME) == State::NeedsSignIn.as_str()
    })
    .await;
    // The list accepted before stays offered: the refused value changed
    // nothing in it.
    assert!(r
        .core
        .mcp
        .catalog
        .all()
        .iter()
        .any(|t| t.server.name == NAME && t.listed.name == "says_schema_key"));
    assert_ne!(r.auth.last_bearer().as_deref(), Some(candidate.as_str()));
    let kept = std::fs::read_to_string(crate::mcp::oauth::file(
        &crate::mcp::oauth::dir(&r.dir.path().join("store")),
        NAME,
    ))
    .unwrap_or_default();
    assert!(!kept.contains(&candidate), "the candidate is not written");
    turn(&r.core, &sid, "use the docs").await;
    // The log of the whole test, read at its end (round 6).
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(log.contains(RENEWAL_SHOWN), "the loss says why");
    let exits = all_exits(&r, &sid, &log).await;
    let found = issued_in(&r, &exits, &[&candidate]);
    assert!(found.is_empty(), "{} values in the exits", found.len());
}

/// A store that holds a list for a server that signs in, the server not
/// reachable at start: none of its tools is in `mcp.list` or the
/// definitions; a server with no sign-in seeds its stored list as on main.
#[tokio::test]
async fn a_signed_in_servers_stored_list_is_not_seeded() {
    let (buf, _guard) = capture();
    let list = |name: &str| crate::mcp::StoredList {
        digest: "seeded".into(),
        tools: vec![
            serde_json::from_value(json!({"name": name, "description": "from the store",
            "inputSchema": {"type": "object"}}))
            .unwrap(),
        ],
    };
    let r = rig_edit(
        FakeAuth::start(AuthConfig::default()).await,
        McpOauth::default(),
        |store_dir| {
            let store = crate::store::Store::open(store_dir).unwrap();
            store
                .put_meta(
                    &format!("{}docs", crate::mcp::STORED_PREFIX),
                    &list("signed_seeded"),
                )
                .unwrap();
            store
                .put_meta(
                    &format!("{}plain", crate::mcp::STORED_PREFIX),
                    &list("plain_seeded"),
                )
                .unwrap();
        },
        |cfg| {
            // Nothing answers here: neither server lists at start.
            cfg.mcp
                .servers
                .insert("plain".into(), plain_at("http://127.0.0.1:9/mcp".into()));
        },
    )
    .await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    let listed = call(&r.core, Surface::Cli, "mcp.list", json!({})).await;
    let names: Vec<&str> = listed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["tool"].as_str())
        .collect();
    assert!(!names.contains(&"signed_seeded"), "{listed}");
    assert!(
        names.contains(&"plain_seeded"),
        "the keyless control seeds: {listed}"
    );
    turn(&r.core, &sid, "use the docs").await;
    let sent = r.model.requests.lock().unwrap().clone();
    for req in &sent {
        let tools = serde_json::to_string(&req.tools).unwrap();
        assert!(!tools.contains("signed_seeded"), "{tools}");
        assert!(tools.contains("plain_seeded"), "{tools}");
    }
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[]);
    assert!(found.is_empty(), "{} values in the exits", found.len());
}

/// The count is said once per change of the dropped set's names: a list
/// with the same dropped tools in another order says nothing more; a list
/// with none dropped says nothing; and a tool dropped only at the last
/// wire-name check of `rebuild`, through a real collision, is counted with
/// the rest. `docs`'s tool `_x` and `docs_`'s tool `x` both make
/// `mcp__docs___x`, so `rebuild` gives each its own digest; the board holds
/// `docs`'s, so that wire name says a kept value while nothing `offered`
/// reads does. The whole-exit scan ends the test.
#[tokio::test]
async fn the_count_is_said_once_per_change_of_the_dropped_names() {
    let (buf, _guard) = capture();
    let x: crate::mcp::Listed = serde_json::from_value(
        json!({"name": "x", "description": "a tool", "inputSchema": {"type": "object"}}),
    )
    .unwrap();
    let r = rig_edit(
        FakeAuth::start(AuthConfig::default()).await,
        McpOauth::default(),
        |store_dir| {
            let store = crate::store::Store::open(store_dir).unwrap();
            let list = crate::mcp::StoredList {
                digest: "seeded".into(),
                tools: vec![x.clone()],
            };
            store
                .put_meta(&format!("{}docs_", crate::mcp::STORED_PREFIX), &list)
                .unwrap();
        },
        |cfg| {
            cfg.mcp
                .servers
                .insert("docs_".into(), plain_at("http://127.0.0.1:9/mcp".into()));
        },
    )
    .await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    let token = r.auth.last_bearer().unwrap();
    let s = r.core.mcp.server("docs").unwrap().clone();
    let tool = |name: &str, about: &str| -> crate::mcp::Listed {
        serde_json::from_value(json!({"name": name, "description": about,
            "inputSchema": {"type": "object"}}))
        .unwrap()
    };
    let rows = || ledgered(&r.core, "mcp.tools_withheld");
    // Rows are written off the caller: each count is waited for, and a
    // "nothing more" is read after the writes have settled.
    let settled = |n: usize| {
        let rows = &rows;
        async move {
            until("the count's rows", || rows().len() >= n).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(rows().len(), n);
        }
    };
    let before = rows().len();
    let a = tool("dropped_a", &token);
    let b = tool("dropped_b", &format!("about {token}"));
    let ok = tool("kept_one", "fine");
    r.core.mcp.apply(&s, vec![a.clone(), b.clone(), ok.clone()]);
    settled(before + 1).await;
    assert_eq!(rows().last().unwrap()["count"], 2);
    r.core.mcp.apply(&s, vec![ok.clone(), b, a.clone()]);
    settled(before + 1).await;
    r.core.mcp.apply(&s, vec![ok.clone()]);
    settled(before + 1).await;
    r.core.mcp.apply(&s, vec![a.clone(), ok.clone()]);
    settled(before + 2).await;
    assert_eq!(rows().last().unwrap()["count"], 1);
    // The real collision: `docs`'s `_x` against `docs_`'s seeded `x`.
    let wires = theseus_mcp::names::wire_names(&[("docs", "_x"), ("docs_", "x")]);
    assert_eq!(theseus_mcp::names::wire_name("docs", "_x"), "mcp__docs___x");
    assert_ne!(wires[0], wires[1]);
    assert!(wires[0].starts_with("mcp__docs___x_"), "{wires:?}");
    crate::mcp::oauth::Held::new("test", r.core.secrets.clone()).hold("wire", &wires[0]);
    r.core.mcp.apply(&s, vec![a, ok, tool("_x", "collides")]);
    settled(before + 3).await;
    assert_eq!(
        rows().last().unwrap()["count"],
        2,
        "the wire's drop counted with the rest"
    );
    let catalog = r.core.mcp.catalog.all();
    assert!(!catalog
        .iter()
        .any(|t| t.server.name == "docs" && t.listed.name == "_x"));
    assert!(catalog
        .iter()
        .any(|t| t.server.name == "docs_" && t.wire == wires[1]));
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[]);
    assert!(found.is_empty(), "{} values in the exits", found.len());
}
