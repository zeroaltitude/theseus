//! Google's one-time consent from Discord (issue 59), through the real RPC on the Discord surface
//! with the fake Google: the owner's own press in a private place begins and the pasted address
//! finishes, kept exactly as `theseus google auth` keeps it (`redeem_and_keep`); a stranger, a
//! shared place and a call that names no presser are refused; the revoke by his press; the pasted
//! value and the code in it appear in no exit; and a turn only offers.

use serde_json::{json, Value};
use theseus_google::fake::{CODE, REFRESH_TOKEN};

use super::tests::*;
use crate::approval::Surface;
use crate::tests_mcp_memory::rpc;

/// Discord ids: 15 to 21 digits, as `[places] owner` is checked.
const OWNER: &str = "100000000000000042";
const STRANGER: &str = "100000000000000077";

fn state_of(url: &str) -> String {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .expect("the consent URL carries a state")
        .to_string()
}

fn pasted(state: &str) -> String {
    format!("http://127.0.0.1:41999/?code={CODE}&state={state}&scope=ignored")
}

fn presser(user: &str, dm: bool) -> Value {
    let mut d = json!({"user_id": user, "channel_id": if dm { "500" } else { "900" }});
    if !dm {
        d["guild_id"] = json!("1");
    }
    json!({"author": format!("discord:{user}"), "discord": d})
}

fn with(mut params: Value, who: Value) -> Value {
    for (k, v) in who.as_object().unwrap() {
        params[k] = v.clone();
    }
    params
}

async fn owned() -> Rig {
    rig_with(
        None,
        |d| format!("file:{}/google-refresh-token", d.display()),
        vec![],
        |cfg| cfg.places.owner = Some(vec![format!("discord:{OWNER}")]),
    )
    .await
}

fn refused(r: &Result<Value, theseus_protocol::RpcError>) -> bool {
    r.as_ref().err().is_some_and(|e| {
        e.code == theseus_protocol::error_code::REFUSED
            && e.message
                .starts_with("Google's consent from Discord is the owner's own press")
    })
}

/// The owner's press in a DM begins; the pasted address finishes, the token kept in the store
/// `[secrets]` names (as the CLI's consent keeps it); the pasted address and the code and token
/// are in no row or node.
#[tokio::test]
async fn the_owners_press_connects_google_and_the_paste_is_in_no_exit() {
    let r = owned().await;
    let begun = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.begin",
        with(json!({"manual": true}), presser(OWNER, true)),
    )
    .await
    .unwrap();
    let paste = pasted(&state_of(begun["url"].as_str().unwrap()));
    let done = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.finish",
        with(
            json!({"auth_id": begun["auth_id"], "pasted": paste}),
            presser(OWNER, true),
        ),
    )
    .await
    .unwrap();
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["store"], "file");
    let kept = std::fs::read_to_string(r.dir.path().join("google-refresh-token")).unwrap();
    assert!(kept.contains(REFRESH_TOKEN), "kept where the CLI keeps it");
    let all = everything(&r.core, &[]);
    for v in [paste.as_str(), CODE, REFRESH_TOKEN] {
        assert!(!all.contains(v), "a value is in a row");
    }
    // The revoke, by his press.
    let out = rpc(
        &r.core,
        Surface::Discord,
        "google.revoke",
        with(json!({}), presser(OWNER, true)),
    )
    .await
    .unwrap();
    assert_eq!(out["cleared"], true, "{out}");
}

/// A stranger in a DM and the owner in a shared channel are refused, and shown no address; a
/// Discord call that names no presser is refused in the CLI's old words; `google.status` stays the
/// CLI's alone.
#[tokio::test]
async fn a_stranger_a_shared_place_and_no_presser_are_refused() {
    let r = owned().await;
    for who in [presser(STRANGER, true), presser(OWNER, false)] {
        let begun = rpc(
            &r.core,
            Surface::Discord,
            "google.auth.begin",
            with(json!({"manual": true}), who.clone()),
        )
        .await;
        assert!(refused(&begun), "{begun:?}");
        let out = rpc(
            &r.core,
            Surface::Discord,
            "google.revoke",
            with(json!({}), who),
        )
        .await;
        assert!(refused(&out), "{out:?}");
    }
    let plain = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.begin",
        json!({"manual": true}),
    )
    .await;
    assert!(
        plain
            .as_ref()
            .err()
            .is_some_and(|e| e.message.contains("the operator's, from the CLI")),
        "{plain:?}"
    );
    let status = rpc(
        &r.core,
        Surface::Discord,
        "google.status",
        presser(OWNER, true),
    )
    .await;
    assert!(status.is_err(), "status stays the CLI's: {status:?}");
    assert!(r.fake.forms().is_empty(), "nothing was redeemed");
}

/// A turn's tools never begin, agree to or finish a consent: the offer is among Google's tools and
/// says nothing began.
#[tokio::test]
async fn a_turn_only_offers_the_google_consent() {
    let r = owned().await;
    let tools: Vec<String> = r
        .core
        .tools
        .registry
        .all()
        .map(|t| t.name().to_string())
        .collect();
    assert!(tools.iter().any(|t| t == super::offer::OFFER), "{tools:?}");
    assert!(tools
        .iter()
        .all(|t| !t.starts_with("google.auth") && t != "google.revoke"));
    assert!(super::offer::OFFERED.contains("Nothing has started"));
    assert!(r.fake.forms().is_empty());
}
