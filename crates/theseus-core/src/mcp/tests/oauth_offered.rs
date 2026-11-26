//! A server that signs in, round 4 of its review (issue 55), through a whole
//! core and the real client: a tool that says a kept value (as its name, its
//! description, a key of its schema, a value of its schema) is not offered
//! (not listed, stored, sent or callable), and the owner is told a count;
//! its prompts are cut (not listed, stored or resolved); a sign-in's every
//! public string is checked (the begin's whole result, the server's name
//! included), and a finish's and a logout's labels at their own exits. A
//! server with no sign-in that says the same is as it always was.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_client::mcp_lines::{address_lines, finish_lines, host_lines, logout_lines};
use theseus_mcp::fake::{self, Fake};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};
use theseus_protocol::mcp::{McpLoginFinishResult, McpLogoutResult, McpPromptRef};

use super::oauth::{call, login, rig_edit, Rig};
use super::oauth_exits::{capture, leaked};
use super::{ledgered, session, turn, until};
use crate::approval::Surface;
use crate::config::mcp::McpOauth;
use crate::config::McpServerConfig;
use crate::mcp::offered::withheld_words;
use crate::mcp::prompts::{Refusal, PROMPTS_CUT};

/// The four tools a fake says a value in, besides the one named after it.
const SAYS: [&str; 3] = ["says_description", "says_schema_key", "says_schema_value"];

/// `docs`, which signs in at a fake whose tools and prompts say the token it
/// was last sent, and `plain`, a server with no sign-in that says what the
/// test tells it to.
async fn rig_says(auth: AuthConfig) -> (Rig, Arc<Fake>) {
    let plain = Fake::new(fake::Config {
        name: "plain".into(),
        ..Default::default()
    });
    let addr = plain.clone().serve_http(0).await.unwrap();
    let r = rig_edit(
        FakeAuth::start(auth).await,
        McpOauth::default(),
        |_| {},
        |cfg| {
            cfg.mcp.servers.insert(
                "plain".into(),
                McpServerConfig {
                    command: vec![],
                    env: Default::default(),
                    url: Some(format!("http://{addr}/mcp")),
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
                },
            );
        },
    )
    .await;
    (r, plain)
}

fn says_token() -> AuthConfig {
    AuthConfig {
        says_token: true,
        ..AuthConfig::default()
    }
}

/// `mcp.list`'s tools of `server`, by the server's own name.
fn tools_of(list: &Value, server: &str) -> Vec<String> {
    list["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{list}"))
        .iter()
        .filter(|t| t["server"] == server)
        .map(|t| t["tool"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Every value the fake issued that `text` holds.
fn values_in(r: &Rig, text: &str) -> usize {
    r.auth
        .seen()
        .secrets
        .iter()
        .filter(|v| text.contains(v.as_str()))
        .count()
}

/// A tool of a server that signs in whose name, description, schema key or
/// schema value is the token is not in `mcp.list`, the stored list, the
/// definitions a turn sends, or the catalog (so not callable); the server's
/// other tools are; the owner is told a count in the log, a fact and a
/// notice; no value is in any node, row, notice, log line or RPC result.
/// The same four tools from a server with no sign-in are offered.
#[tokio::test]
async fn a_tool_that_says_a_kept_value_is_not_offered() {
    let (buf, _guard) = capture();
    let (r, plain) = rig_says(says_token()).await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    let token = r.auth.last_bearer().unwrap();
    until("the count's row", || {
        !ledgered(&r.core, "mcp.tools_withheld").is_empty()
    })
    .await;
    let rows = ledgered(&r.core, "mcp.tools_withheld");
    assert_eq!(rows.len(), 1, "once per change: {rows:?}");
    assert_eq!(rows[0], json!({"server": "docs", "count": 4}));
    let words = withheld_words("docs", 4);
    until("the notice", || {
        r.core
            .outbox
            .open_for(crate::outbox::OPERATOR_TARGET)
            .iter()
            .any(|a| format!("{a:?}").contains(&words))
    })
    .await;
    // `mcp.list`: the server's other tools, none of the four.
    let list = call(&r.core, Surface::Cli, "mcp.list", json!({})).await;
    let docs = tools_of(&list, "docs");
    assert!(docs.iter().any(|t| t == "echo"), "{docs:?}");
    for t in SAYS {
        assert!(!docs.iter().any(|d| d == t), "{t} offered: {docs:?}");
    }
    assert_eq!(
        values_in(&r, &list.to_string()),
        0,
        "mcp.list holds a value"
    );
    // The stored list, read back from the store.
    let stored = crate::mcp::read_stored(&r.core.store, "docs").expect("a stored list");
    let stored = serde_json::to_string(&stored.tools).unwrap();
    assert!(stored.contains("\"echo\""), "{stored}");
    assert!(SAYS.iter().all(|t| !stored.contains(t)), "{stored}");
    assert_eq!(values_in(&r, &stored), 0, "the stored list holds a value");
    // The catalog: none of them is a tool a call can name.
    let catalog: Vec<String> = r
        .core
        .mcp
        .catalog
        .all()
        .iter()
        .map(|t| t.wire.clone())
        .collect();
    assert!(
        catalog.iter().any(|w| w == "mcp__docs__echo"),
        "{catalog:?}"
    );
    assert!(
        catalog
            .iter()
            .all(|w| !w.starts_with("mcp__docs__says") && !w.contains(&token)),
        "{catalog:?}"
    );
    // The definitions a turn sends.
    turn(&r.core, &sid, "use the docs").await;
    let sent = r.model.requests.lock().unwrap().clone();
    assert!(!sent.is_empty());
    for req in &sent {
        let tools = serde_json::to_string(&req.tools).unwrap();
        assert!(tools.contains("mcp__docs__echo"), "{tools}");
        assert!(!tools.contains("mcp__docs__says"), "{tools}");
        assert_eq!(values_in(&r, &tools), 0, "a definition holds a value");
    }
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(log.contains(&words), "the count is said in the log");
    let found = leaked(&r, &sid, &log);
    assert!(found.is_empty(), "{} values leaked", found.len());
    // The control: a server with no sign-in that says the same value offers
    // all four, as on main.
    plain.set_says(Some(token.clone()));
    call(
        &r.core,
        Surface::Cli,
        "mcp.restart",
        json!({"name": "plain"}),
    )
    .await;
    let mut offered = Vec::new();
    until("plain's four tools", || {
        let catalog = r.core.mcp.catalog.all();
        offered = catalog
            .iter()
            .filter(|t| t.server.name == "plain")
            .map(|t| t.listed.name.clone())
            .collect();
        SAYS.iter().all(|t| offered.iter().any(|o| o == t))
    })
    .await;
    assert!(
        offered.contains(&token),
        "the tool named after it: {offered:?}"
    );
    assert_eq!(ledgered(&r.core, "mcp.tools_withheld").len(), 1);
}

/// A server that signs in offers no prompts: its prompt whose name, whose
/// argument's name and whose text are the token is not listed (`mcp.list`,
/// `mcp.prompt.list`), not stored, never asked for, and a get is refused in
/// fixed words; nothing of it is in any node, row, notice or log line. A
/// server with no sign-in still lists and resolves its prompts.
#[tokio::test]
async fn prompts_of_a_server_that_signs_in_are_cut() {
    let (buf, _guard) = capture();
    let (r, plain) = rig_says(says_token()).await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    let token = r.auth.last_bearer().unwrap();
    let listed = call(&r.core, Surface::Cli, "mcp.prompt.list", json!({})).await;
    let docs: Vec<&Value> = listed["prompts"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed}"))
        .iter()
        .filter(|p| p["server"] == "docs")
        .collect();
    assert!(docs.is_empty(), "{docs:?}");
    let list = call(&r.core, Surface::Cli, "mcp.list", json!({})).await;
    assert!(
        list["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["server"] != "docs"),
        "{list}"
    );
    let status = r.core.mcp.status();
    let docs_status = status.iter().find(|s| s.name == "docs").unwrap();
    assert_eq!(docs_status.prompts, 0);
    assert!(crate::mcp::prompts::read_stored(&r.core.store, "docs").is_none());
    // A get, by the prompt's name and by the token's: refused, nothing asked.
    for name in ["says".to_string(), token.clone()] {
        let got = r
            .core
            .mcp
            .resolve_prompt(&McpPromptRef {
                server: "docs".into(),
                name,
                arguments: Default::default(),
            })
            .await;
        match got {
            Err(Refusal::Invalid(why)) => assert_eq!(why, PROMPTS_CUT),
            Err(Refusal::Unavailable(why)) => panic!("refused as unavailable: {why}"),
            Ok(_) => panic!("a prompt of a server that signs in was resolved"),
        }
    }
    let asked: Vec<String> = r
        .fake
        .seen()
        .messages
        .iter()
        .filter_map(|m| m["method"].as_str().map(str::to_string))
        .filter(|m| m.starts_with("prompts/"))
        .collect();
    assert!(asked.is_empty(), "the server was asked {asked:?}");
    assert!(ledgered(&r.core, "mcp.prompt_changed").is_empty());
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let found = leaked(&r, &sid, &log);
    assert!(found.is_empty(), "{} values leaked", found.len());
    // The control: a server with no sign-in lists and resolves its prompts,
    // its `says` included, as on main.
    plain.set_says(Some(token.clone()));
    call(
        &r.core,
        Surface::Cli,
        "mcp.restart",
        json!({"name": "plain"}),
    )
    .await;
    until("plain's prompt", || {
        r.core
            .mcp
            .prompt_infos(Some("plain"))
            .iter()
            .any(|p| p.prompt == "says")
    })
    .await;
    let got = r
        .core
        .mcp
        .resolve_prompt(&McpPromptRef {
            server: "plain".into(),
            name: "greet".into(),
            arguments: [("name".to_string(), "Ada".to_string())].into(),
        })
        .await;
    assert!(got.is_ok(), "a plain server's prompt resolves");
    let got = r
        .core
        .mcp
        .resolve_prompt(&McpPromptRef {
            server: "plain".into(),
            name: "says".into(),
            arguments: Default::default(),
        })
        .await;
    assert!(got.is_ok(), "a plain server's prompt that says it resolves");
}

/// A server whose configured name is protectable, so a value can equal it.
const NAME: &str = "oauth_notes_1";

/// `rig_says`, with `docs` attached under `NAME`.
async fn rig_named(auth: AuthConfig) -> Rig {
    rig_edit(
        FakeAuth::start(auth).await,
        McpOauth::default(),
        |_| {},
        |cfg| {
            let docs = cfg.mcp.servers.remove("docs").unwrap();
            cfg.mcp.servers.insert(NAME.into(), docs);
        },
    )
    .await
}

async fn begin(r: &Rig) -> Value {
    call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": NAME}),
    )
    .await
}

/// The begin checks its whole result, the server's name included: a
/// registration whose client secret is the server's name is refused, and
/// neither the answer, the CLI's lines nor the log holds it.
#[tokio::test]
async fn a_begin_whose_name_is_a_kept_value_is_refused() {
    let r = rig_named(AuthConfig {
        client_secret_is: Some(NAME.into()),
        ..AuthConfig::default()
    })
    .await;
    r.core.mcp.start();
    let (buf, _guard) = capture();
    let begun = begin(&r).await;
    assert_eq!(
        begun["error"]["message"],
        "the sign-in would show a value Theseus keeps secret, so it is refused: nothing is shown",
        "{begun}"
    );
    assert!(!begun.to_string().contains(NAME), "{begun}");
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(!log.contains("an MCP sign-in began"), "{log}");
    assert!(!log.contains(NAME), "{log}");
    // With a secret that is not the name, the same begin is shown, and the
    // CLI prints the name: the refusal above is the name's alone.
    let r = rig_named(AuthConfig {
        client_secret: true,
        ..AuthConfig::default()
    })
    .await;
    r.core.mcp.start();
    let begun = begin(&r).await;
    let b: theseus_protocol::mcp::McpLoginBeginResult =
        serde_json::from_value(begun.clone()).unwrap_or_else(|e| panic!("{e}: {begun}"));
    let lines = [host_lines(&b), address_lines(&b)].concat().join("\n");
    assert!(lines.contains(NAME), "{lines}");
}

/// A token issued by the redemption that equals a label public before it
/// (the server's name, the sign-in's host): the finish still signs in and
/// the logout still signs out, but neither their results, the CLI's lines
/// nor their own log lines hold it; they say why in fixed words.
#[tokio::test]
async fn a_label_a_new_token_equals_is_not_shown_at_finish_or_logout() {
    for (label, is_host) in [(NAME.to_string(), false), ("127.0.0.1".to_string(), true)] {
        let r = rig_named(AuthConfig {
            access_token_is: Some(label.clone()),
            ..AuthConfig::default()
        })
        .await;
        r.core.mcp.start();
        let begun = begin(&r).await;
        let url = begun["url"].as_str().unwrap_or_else(|| panic!("{begun}"));
        assert_eq!(begun["host"] == label, is_host, "{begun}");
        r.auth.browse(url, true).await;
        let (buf, guard) = capture();
        let done = call(
            &r.core,
            Surface::Cli,
            "mcp.login.finish",
            json!({"login_id": begun["login_id"], "wait_secs": 10}),
        )
        .await;
        let finished: McpLoginFinishResult =
            serde_json::from_value(done.clone()).unwrap_or_else(|e| panic!("{e}: {done}"));
        assert_eq!(finished.state, "done", "{done}");
        let shown = [finish_lines(&finished).join("\n"), done.to_string()].join("\n");
        assert!(!shown.contains(&label), "{label}: {shown}");
        assert!(
            shown.contains("The sign-in succeeded and is kept"),
            "{shown}"
        );
        assert!(shown.contains(crate::mcp::sign_in::HIDDEN), "{shown}");
        let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
        drop(guard);
        let line = log
            .lines()
            .find(|l| l.contains("an MCP server was signed in"))
            .unwrap_or_else(|| panic!("no signed-in line: {log}"));
        assert!(!line.contains(&label), "{line}");
        assert!(line.contains("is not shown"), "{line}");
        // The logout: done, and said without the label.
        let (buf, _guard) = capture();
        let out = call(&r.core, Surface::Cli, "mcp.logout", json!({"name": NAME})).await;
        let logged_out: McpLogoutResult =
            serde_json::from_value(out.clone()).unwrap_or_else(|e| panic!("{e}: {out}"));
        assert!(logged_out.forgot, "{out}");
        assert!(!crate::mcp::oauth::file(
            &crate::mcp::oauth::dir(&r.dir.path().join("store")),
            NAME
        )
        .exists());
        let shown = [logout_lines(&logged_out).join("\n"), out.to_string()].join("\n");
        if !is_host {
            assert!(!shown.contains(&label), "{shown}");
            assert!(shown.contains("The sign-out happened"), "{shown}");
        }
        let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
        let line = log
            .lines()
            .find(|l| l.contains("an MCP server was signed out"))
            .unwrap_or_else(|| panic!("no signed-out line: {log}"));
        assert!(!line.contains(&label), "{line}");
    }
}
