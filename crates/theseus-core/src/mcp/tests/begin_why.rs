//! A sign-in that cannot begin (issue 62), through the real RPC, the board and the fakes: each kind
//! of failure tells a Discord press the kind's marker and nothing else, and the CLI the daemon's
//! sentence as before; with no registration and no client id the press learns that kind, and with
//! a client id in the settings the same press begins; what an issuer wrote is in no answer, card
//! or log line; a press by anyone else or in a shared place learns no kind; and one warning line
//! names the server and the kind.

use serde_json::{json, Value};
use theseus_mcp::fake::{self, Fake};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};

use super::oauth::{call, rig_edit, Rig};
use super::oauth_exits::{all_exits, capture};
use super::{session, turn};
use crate::approval::Surface;
use crate::config::mcp::McpOauth;
use crate::mcp::offer::NOT_YOURS;
use crate::mcp::BeginKind;
use crate::Config;

const OWNER: &str = "42";
const STRANGER: &str = "77";
/// Invented: what a hostile issuer writes in its metadata.
const MARKER: &str = "INVENTED-ISSUER-WORDS-6170";

/// A press by `user`: in a DM, or in guild channel 900.
fn presser(user: &str, dm: bool) -> Value {
    let mut d = json!({"user_id": user, "channel_id": if dm { "500" } else { "900" }});
    if !dm {
        d["guild_id"] = json!("1");
    }
    json!({"author": format!("discord:{user}"), "discord": d})
}

async fn owned(auth: AuthConfig, oauth: McpOauth, edit: impl FnOnce(&mut Config)) -> Rig {
    rig_edit(
        FakeAuth::start(auth).await,
        oauth,
        |_| {},
        |cfg| {
            cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
            edit(cfg);
        },
    )
    .await
}

fn set_url(cfg: &mut Config, url: String) {
    cfg.mcp.servers.get_mut("docs").unwrap().url = Some(url);
}

/// `mcp.login.begin` of `name`: from the CLI, or as `who`'s press on Discord.
async fn begin(r: &Rig, name: &str, who: Option<Value>) -> Value {
    let mut params = json!({"name": name, "manual": true});
    let surface = match who {
        Some(who) => {
            for (k, v) in who.as_object().unwrap() {
                params[k] = v.clone();
            }
            Surface::Discord
        }
        None => Surface::Cli,
    };
    call(&r.core, surface, "mcp.login.begin", params).await
}

fn message(v: &Value) -> &str {
    v["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("no error: {v}"))
}

/// A case: what it is, its rig, the name begun, its kind, and the CLI's sentence.
type Case = (&'static str, Rig, &'static str, BeginKind, String);

/// Each case: the owner's press is told its kind's marker and nothing else, and the CLI the
/// daemon's sentence, byte for byte as before, with the same code. The board is not started: a
/// begin needs only the settings, and nothing here connects to an address a case names.
async fn check(cases: Vec<Case>) {
    for (what, r, name, kind, words) in cases {
        let cli = begin(&r, name, None).await;
        assert_eq!(message(&cli), words, "{what}: the CLI's sentence");
        assert_eq!(
            cli["error"]["code"],
            theseus_protocol::error_code::INVALID_PARAMS,
            "{what}"
        );
        let pressed = begin(&r, name, Some(presser(OWNER, true))).await;
        assert_eq!(
            message(&pressed),
            kind.marker(),
            "{what}: the press's marker"
        );
        assert_eq!(
            BeginKind::of_marker(message(&pressed)),
            Some(kind),
            "{what}"
        );
        assert_eq!(
            pressed["error"]["code"],
            theseus_protocol::error_code::INVALID_PARAMS,
            "{what}"
        );
    }
}

/// The kinds of the server and its settings. (`Other`, the loopback listener or the HTTP client
/// failing to start, is not reached here.)
#[tokio::test]
async fn each_kind_of_the_server_tells_a_press_its_marker_and_the_cli_its_sentence() {
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let plain = Fake::new(fake::Config {
        name: "docs".into(),
        ..Default::default()
    });
    let plain_at = plain.clone().serve_http(0).await.unwrap();
    check(vec![
        (
            "not attached",
            owned(AuthConfig::default(), McpOauth::default(), |_| {}).await,
            "nope",
            BeginKind::Settings,
            "no MCP server named nope is attached".into(),
        ),
        (
            "no oauth",
            owned(AuthConfig::default(), McpOauth::default(), |cfg| {
                cfg.mcp.servers.get_mut("docs").unwrap().oauth = None;
            })
            .await,
            "docs",
            BeginKind::Settings,
            "the MCP server docs does not sign in: its settings have no `oauth`. A server that \
             wants a sign-in is attached again with one (`oauth`), then signed in"
                .into(),
        ),
        (
            "not https",
            owned(AuthConfig::default(), McpOauth::default(), |cfg| {
                set_url(cfg, "http://198.51.100.7/mcp".into())
            })
            .await,
            "docs",
            BeginKind::Settings,
            "the MCP server docs is not at an https address, and a server that signs in must be"
                .into(),
        ),
        (
            "not reached",
            owned(AuthConfig::default(), McpOauth::default(), |cfg| {
                set_url(cfg, format!("http://127.0.0.1:{closed}/mcp"))
            })
            .await,
            "docs",
            BeginKind::Unreachable,
            "the MCP server docs could not be reached".into(),
        ),
        (
            "no sign-in needed",
            owned(AuthConfig::default(), McpOauth::default(), |cfg| {
                set_url(cfg, format!("http://{plain_at}/mcp"))
            })
            .await,
            "docs",
            BeginKind::NoNeed,
            "the MCP server docs answered without a sign-in: it does not need one".into(),
        ),
    ])
    .await;
}

/// The kinds of its authorization server, and the door at the begin.
#[tokio::test]
async fn each_kind_of_the_issuer_tells_a_press_its_marker_and_the_cli_its_sentence() {
    check(vec![
        (
            "another issuer",
            owned(
                AuthConfig {
                    issuer: Some(format!("https://{MARKER}.example.invalid")),
                    ..AuthConfig::default()
                },
                McpOauth::default(),
                |_| {},
            )
            .await,
            "docs",
            BeginKind::NoIssuer,
            "the authorization server's metadata names another issuer: refused".into(),
        ),
        (
            "no registration, no client id",
            owned(
                AuthConfig {
                    register: false,
                    ..AuthConfig::default()
                },
                McpOauth::default(),
                |_| {},
            )
            .await,
            "docs",
            BeginKind::NoClientId,
            "the authorization server of docs offers no registration, and the server's settings \
             name no `oauth.client_id`: the owner asks the server's maker for a client id, and it \
             is added to the settings"
                .into(),
        ),
        (
            "registration failed",
            owned(
                AuthConfig {
                    client_id_alias: Some(String::new()),
                    ..AuthConfig::default()
                },
                McpOauth::default(),
                |_| {},
            )
            .await,
            "docs",
            BeginKind::Registration,
            "the registration endpoint answered with something that is not its JSON".into(),
        ),
        (
            "a kept value",
            owned(
                AuthConfig {
                    client_id_is_secret: true,
                    ..AuthConfig::default()
                },
                McpOauth::default(),
                |_| {},
            )
            .await,
            "docs",
            BeginKind::Shown,
            crate::mcp::doors::SHOWN_WORDS.into(),
        ),
    ])
    .await;
}

/// No registration and no client id: the owner's press learns that kind; with a client id in the
/// settings the same press begins.
#[tokio::test]
async fn no_registration_and_no_client_id_and_then_a_client_id_begins() {
    let none = AuthConfig {
        register: false,
        ..AuthConfig::default()
    };
    let r = owned(none.clone(), McpOauth::default(), |_| {}).await;
    r.core.mcp.start();
    let pressed = begin(&r, "docs", Some(presser(OWNER, true))).await;
    assert_eq!(
        message(&pressed),
        BeginKind::NoClientId.marker(),
        "{pressed}"
    );
    let client_id = McpOauth {
        client_id: Some(none.client_id.clone()),
        ..McpOauth::default()
    };
    let r = owned(none, client_id, |_| {}).await;
    r.core.mcp.start();
    let begun = begin(&r, "docs", Some(presser(OWNER, true))).await;
    assert!(begun["url"].as_str().is_some(), "it begins: {begun}");
    assert_eq!(r.auth.seen().registrations, 0);
}

/// What an issuer wrote in its metadata is in no answer the press is given, no card, no log line,
/// no row and no request a turn makes; the press learns the kind, and the turn learns nothing of
/// the sign-in. And a sentence that carries what an issuer chose (its registration's status): the
/// CLI prints it, and no answer to the press, exit or log line holds it.
#[tokio::test]
async fn what_an_issuer_wrote_is_in_no_exit() {
    let (buf, _guard) = capture();
    let r = owned(
        AuthConfig {
            issuer: Some(format!("https://{MARKER}.example.invalid")),
            ..AuthConfig::default()
        },
        McpOauth::default(),
        |_| {},
    )
    .await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    let pressed = begin(&r, "docs", Some(presser(OWNER, true))).await;
    assert_eq!(message(&pressed), BeginKind::NoIssuer.marker(), "{pressed}");
    turn(&r.core, &sid, "use the docs").await;
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(
        log.contains("an MCP sign-in could not begin"),
        "the log was written to"
    );
    let exits = all_exits(&r, &sid, &log).await;
    for text in [pressed.to_string(), exits] {
        assert!(
            !text.contains(MARKER),
            "what the issuer wrote is in an exit"
        );
    }
    let sent: Vec<String> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .collect();
    assert!(!sent.is_empty(), "the turn asked the model");
    assert!(
        sent.iter().all(|s| !s.contains("could not begin")),
        "a turn sees the sign-in"
    );
    // A failure whose sentence carries what the issuer chose (its status): the CLI prints it, and
    // the press's answer, the exits and the log hold none of it.
    let r = owned(
        AuthConfig {
            register_status: Some(547),
            ..AuthConfig::default()
        },
        McpOauth::default(),
        |_| {},
    )
    .await;
    let sid = session(&r.core, None);
    let chosen = "HTTP 547";
    let pressed = begin(&r, "docs", Some(presser(OWNER, true))).await;
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let exits = all_exits(&r, &sid, &log).await;
    for text in [pressed.to_string(), exits] {
        assert!(
            !text.contains(chosen),
            "what the issuer chose is in an exit"
        );
    }
    assert_eq!(
        message(&pressed),
        BeginKind::Registration.marker(),
        "{pressed}"
    );
    let cli = begin(&r, "docs", None).await;
    assert!(message(&cli).contains(chosen), "the CLI prints it: {cli}");
}

/// A press by someone else, or the owner's in a shared place, gets the refusal's words and learns
/// no kind; and no warning line is written for it.
#[tokio::test]
async fn a_strangers_press_and_a_shared_place_learn_no_kind() {
    let (buf, _guard) = capture();
    let r = owned(
        AuthConfig {
            register: false,
            ..AuthConfig::default()
        },
        McpOauth::default(),
        |_| {},
    )
    .await;
    r.core.mcp.start();
    for (what, who) in [
        ("a stranger's press", presser(STRANGER, true)),
        ("the owner, in a shared place", presser(OWNER, false)),
    ] {
        let pressed = begin(&r, "docs", Some(who)).await;
        assert_eq!(
            pressed["error"]["code"],
            theseus_protocol::error_code::REFUSED,
            "{what}: {pressed}"
        );
        assert!(message(&pressed).starts_with(NOT_YOURS), "{what}");
        assert_eq!(BeginKind::of_marker(message(&pressed)), None, "{what}");
        assert!(!message(&pressed).contains("could not begin"), "{what}");
    }
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(!log.contains("could not begin"), "{log}");
}

/// One warning line for a begin that failed, with the server's name and the kind, and none of the
/// daemon's sentence; none for a begin that works.
#[tokio::test]
async fn one_warning_line_names_the_server_and_the_kind() {
    let (buf, _guard) = capture();
    let r = owned(
        AuthConfig {
            register: false,
            ..AuthConfig::default()
        },
        McpOauth::default(),
        |_| {},
    )
    .await;
    r.core.mcp.start();
    let failed = begin(&r, "docs", None).await;
    assert!(failed.get("error").is_some(), "{failed}");
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("an MCP sign-in could not begin"))
        .collect();
    assert_eq!(lines.len(), 1, "{log}");
    assert!(lines[0].contains("WARN"), "{}", lines[0]);
    assert!(lines[0].contains("server=docs"), "{}", lines[0]);
    assert!(lines[0].contains("kind=NoClientId"), "{}", lines[0]);
    assert!(!lines[0].contains("offers no registration"), "{}", lines[0]);
    drop(_guard);

    let (buf, _guard) = capture();
    let r = owned(AuthConfig::default(), McpOauth::default(), |_| {}).await;
    r.core.mcp.start();
    let begun = begin(&r, "docs", None).await;
    assert!(begun["url"].as_str().is_some(), "{begun}");
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(
        log.contains("an MCP sign-in began"),
        "the log was written to"
    );
    assert!(!log.contains("could not begin"), "{log}");
}
