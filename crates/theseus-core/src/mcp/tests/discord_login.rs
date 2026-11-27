//! Signing in to an MCP server from Discord (issue 59), through the real RPC
//! with the Discord surface, the board and the fake issuer: the owner's own
//! press in a private place begins, finishes from the pasted address, and
//! signs out; anyone else, a shared place, and a press that names no presser
//! are refused and are shown no address; the pasted value and the code in it
//! appear in no exit; a second begin, a wrong state, a second paste and a
//! restart each end in the daemon's words; the turn's offer begins nothing.

use serde_json::{json, Value};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};

use super::oauth::{call, rig_edit, Rig};
use super::oauth_exits::{all_exits, capture, issued_in};
use super::{session, turn};
use crate::approval::Surface;
use crate::config::mcp::McpOauth;
use crate::mcp::offer::NOT_YOURS;

const OWNER: &str = "42";
const STRANGER: &str = "77";

/// A rig whose owner is Discord user `OWNER`.
async fn rig_owned() -> Rig {
    rig_edit(
        FakeAuth::start(AuthConfig::default()).await,
        McpOauth::default(),
        |_| {},
        |cfg| cfg.places.owner = Some(vec![format!("discord:{OWNER}")]),
    )
    .await
}

/// A press by `user`: in a DM (no guild), or in guild channel 900.
fn presser(user: &str, dm: bool) -> Value {
    json!({
        "author": format!("discord:{user}"),
        "discord": {"user_id": user, "channel_id": if dm { "500" } else { "900" },
                    "guild_id": if dm { Value::Null } else { json!("1") }},
    })
}

fn with(mut params: Value, who: Value) -> Value {
    for (k, v) in who.as_object().unwrap() {
        if !v.is_null() {
            params[k] = v.clone();
        }
    }
    if let Some(d) = params["discord"].as_object_mut() {
        if d.get("guild_id").is_some_and(Value::is_null) {
            d.remove("guild_id");
        }
    }
    params
}

async fn begin(r: &Rig, who: Value) -> Value {
    call(
        &r.core,
        Surface::Discord,
        "mcp.login.begin",
        with(json!({"name": "docs", "manual": true}), who),
    )
    .await
}

async fn finish(r: &Rig, login_id: &Value, pasted: &str, who: Value) -> Value {
    call(
        &r.core,
        Surface::Discord,
        "mcp.login.finish",
        with(
            json!({"login_id": login_id, "pasted": pasted, "wait_secs": 0}),
            who,
        ),
    )
    .await
}

fn refused(v: &Value) -> bool {
    v["error"]["code"] == theseus_protocol::error_code::REFUSED
        && v["error"]["message"]
            .as_str()
            .is_some_and(|m| m.starts_with(NOT_YOURS))
}

/// The owner's press in a DM begins (the address is in its answer, to the
/// binding that shows it to him alone); the pasted address finishes the
/// sign-in; the pasted value and the code in it are in no exit of the whole
/// test, the next turn's request included.
#[tokio::test]
async fn the_owners_press_in_a_private_place_signs_in_and_the_paste_is_in_no_exit() {
    let (buf, _guard) = capture();
    let r = rig_owned().await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    let begun = begin(&r, presser(OWNER, true)).await;
    let url = begun["url"].as_str().unwrap_or_else(|| panic!("{begun}"));
    assert_eq!(begun["redirect"], "", "manual: the paste");
    let pasted = r.auth.browse(url, false).await;
    let done = finish(&r, &begun["login_id"], &pasted, presser(OWNER, true)).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["connected"], true, "{done}");
    turn(&r.core, &sid, "use the docs").await;
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let exits = all_exits(&r, &sid, &log).await;
    assert!(!exits.contains(&pasted), "the pasted address is in an exit");
    let found = issued_in(&r, &exits, &[]);
    assert!(found.is_empty(), "{} values in the exits", found.len());
}

/// A press by someone who is not the owner, in a DM, is refused, and its
/// answer holds no address; so is the owner's press in a shared place; so is
/// a Discord call that names no presser.
#[tokio::test]
async fn a_strangers_press_a_shared_place_and_no_presser_are_refused() {
    let r = rig_owned().await;
    r.core.mcp.start();
    for (what, who) in [
        ("a stranger's press", presser(STRANGER, true)),
        ("the owner, in a shared place", presser(OWNER, false)),
        ("no presser", json!({})),
    ] {
        let begun = begin(&r, who).await;
        assert!(refused(&begun), "{what}: {begun}");
        assert!(begun.get("url").is_none(), "{what}: an address was shown");
    }
    assert_eq!(r.auth.seen().registrations, 0, "nothing began");
    // The web UI is refused as before.
    let web = call(
        &r.core,
        Surface::Web,
        "mcp.login.begin",
        json!({"name": "docs", "manual": true}),
    )
    .await;
    assert_eq!(
        web["error"]["code"],
        theseus_protocol::error_code::REFUSED,
        "{web}"
    );
}

/// A finish and a sign-out by anyone but the owner in a private place are
/// refused too: the paste is never taken from a stranger.
#[tokio::test]
async fn a_strangers_finish_and_sign_out_are_refused() {
    let r = rig_owned().await;
    r.core.mcp.start();
    let begun = begin(&r, presser(OWNER, true)).await;
    let pasted = r.auth.browse(begun["url"].as_str().unwrap(), false).await;
    let done = finish(&r, &begun["login_id"], &pasted, presser(STRANGER, true)).await;
    assert!(refused(&done), "{done}");
    assert!(r.auth.seen().codes_sent.is_empty(), "no code was redeemed");
    let out = call(
        &r.core,
        Surface::Discord,
        "mcp.logout",
        with(json!({"name": "docs"}), presser(OWNER, false)),
    )
    .await;
    assert!(refused(&out), "{out}");
}

/// A second begin starts a new sign-in, and the first one's address then
/// fails in the daemon's words; a wrong state fails; the same address pasted
/// a second time is refused; a restart drops a sign-in mid-way.
#[tokio::test]
async fn a_second_begin_a_wrong_state_a_second_paste_and_a_restart_fail() {
    let r = rig_owned().await;
    r.core.mcp.start();
    let first = begin(&r, presser(OWNER, true)).await;
    let first_paste = r.auth.browse(first["url"].as_str().unwrap(), false).await;
    let second = begin(&r, presser(OWNER, true)).await;
    let old = finish(&r, &first["login_id"], &first_paste, presser(OWNER, true)).await;
    assert!(
        old["error"]["message"]
            .as_str()
            .is_some_and(|m| m.starts_with("no sign-in is open")),
        "{old}"
    );
    // A wrong state, on the open one: it ends that sign-in.
    let paste = r.auth.browse(second["url"].as_str().unwrap(), false).await;
    let wrong = paste.replace("state=", "state=x");
    let bad = finish(&r, &second["login_id"], &wrong, presser(OWNER, true)).await;
    assert!(bad.get("error").is_some(), "{bad}");
    assert!(!bad.to_string().contains(&wrong), "the paste is not echoed");
    // A good one, then the same address again.
    let third = begin(&r, presser(OWNER, true)).await;
    let paste = r.auth.browse(third["url"].as_str().unwrap(), false).await;
    let done = finish(&r, &third["login_id"], &paste, presser(OWNER, true)).await;
    assert_eq!(done["state"], "done", "{done}");
    let again = finish(&r, &third["login_id"], &paste, presser(OWNER, true)).await;
    assert!(
        again["error"]["message"]
            .as_str()
            .is_some_and(|m| m.starts_with("no sign-in is open")),
        "{again}"
    );
    // A restart: a new daemon has no sign-in open.
    let fourth = begin(&r, presser(OWNER, true)).await;
    let paste = r.auth.browse(fourth["url"].as_str().unwrap(), false).await;
    let restarted = rig_owned().await;
    restarted.core.mcp.start();
    let after = finish(
        &restarted,
        &fourth["login_id"],
        &paste,
        presser(OWNER, true),
    )
    .await;
    assert!(
        after["error"]["message"]
            .as_str()
            .is_some_and(|m| m.starts_with("no sign-in is open")),
        "{after}"
    );
}

/// The owner's Sign out press, in a private place, forgets the sign-in.
#[tokio::test]
async fn the_owners_sign_out_forgets() {
    let r = rig_owned().await;
    r.core.mcp.start();
    let begun = begin(&r, presser(OWNER, true)).await;
    let pasted = r.auth.browse(begun["url"].as_str().unwrap(), false).await;
    let done = finish(&r, &begun["login_id"], &pasted, presser(OWNER, true)).await;
    assert_eq!(done["state"], "done", "{done}");
    let out = call(
        &r.core,
        Surface::Discord,
        "mcp.logout",
        with(json!({"name": "docs"}), presser(OWNER, true)),
    )
    .await;
    assert_eq!(out["forgot"], true, "{out}");
}

/// What a turn has: the offer posts the card and begins nothing (no
/// registration, no authorization, no address in its answer); and no tool a
/// turn is offered is a sign-in's begin, agree or finish.
#[tokio::test]
async fn a_turn_only_offers_and_begins_nothing() {
    let r = rig_owned().await;
    r.core.mcp.start();
    let tools: Vec<String> = r
        .core
        .tools
        .registry
        .all()
        .map(|t| t.name().to_string())
        .collect();
    for t in &tools {
        assert!(
            !t.starts_with("mcp.login") && !t.starts_with("mcp.logout"),
            "a turn's tool reaches a sign-in: {t}"
        );
    }
    assert!(tools.iter().any(|t| t == crate::mcp::offer::OFFER));
    let text = crate::mcp::offer::offered("docs");
    assert!(text.contains("Nothing has started"), "{text}");
    assert_eq!(r.auth.seen().registrations, 0);
    assert!(r.auth.seen().paths.iter().all(|p| !p.contains("authorize")));
}

/// When the sign-in goes through a place that is not its issuer's own, the
/// owner's press must agree (the binding's I agree press) before the daemon
/// takes the paste; without it nothing is sent, as with the CLI's typed yes.
#[tokio::test]
async fn a_place_elsewhere_needs_the_agree_press() {
    let elsewhere = FakeAuth::start(AuthConfig::default()).await;
    let issuer = FakeAuth::start(AuthConfig {
        endpoints_at: Some(elsewhere.base()),
        iss_supported: true,
        ..AuthConfig::default()
    })
    .await;
    elsewhere.set(|c| c.issuer = Some(issuer.base()));
    let r = rig_edit(
        issuer.clone(),
        McpOauth::default(),
        |_| {},
        |cfg| cfg.places.owner = Some(vec![format!("discord:{OWNER}")]),
    )
    .await;
    r.core.mcp.start();
    for agreed in [false, true] {
        let begun = begin(&r, presser(OWNER, true)).await;
        assert!(
            begun["elsewhere"].as_array().is_some_and(|e| !e.is_empty()),
            "{begun}"
        );
        let pasted = elsewhere
            .browse(begun["url"].as_str().unwrap(), false)
            .await;
        let done = call(
            &r.core,
            Surface::Discord,
            "mcp.login.finish",
            with(
                json!({"login_id": begun["login_id"], "pasted": pasted, "agreed": agreed}),
                presser(OWNER, true),
            ),
        )
        .await;
        match agreed {
            false => assert!(
                done["error"]["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("not agreed to")),
                "{done}"
            ),
            true => assert_eq!(done["state"], "done", "{done}"),
        }
    }
}
