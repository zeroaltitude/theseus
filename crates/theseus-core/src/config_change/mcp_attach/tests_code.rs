//! Issue 57, A4: which MCP server cards also take the owner's authenticator
//! code. Every attach and every detach asks its card, as on main. Attaching a
//! server whose table names a secret, an env, an address or egress needs the
//! code too; attaching one with none of these, and removing any server, take
//! the card's yes alone. The yes is judged by the same rule the card was asked
//! by (`mcp_attach::needs`).

use serde_json::json;

use super::tests::{rig, wc};
use crate::config::Config;
use crate::node::ResultStatus;

/// The KEEP neighbour: a server given a secret asks with the code, and the yes
/// alone is refused; one with no secret, env, address or egress asks with the
/// card alone.
#[tokio::test]
async fn attaching_a_server_with_a_secret_asks_with_the_code() {
    let r = rig();
    let sid = r.session();
    let mut call = wc();
    call["env"] = json!({"WC_KEY": "wc_key"});
    r.turn(&sid, "mcp_attach", call).await;
    let q = r.asked().remove(0);
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason
            .contains("so this needs a second proof as well as your yes"),
        "{}",
        q.reason
    );
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap_err();
    assert!(
        e.to_string().contains("theseus owner proof enroll"),
        "{e:#}"
    );
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
    let sid = r.session();
    r.turn(&sid, "mcp_attach", wc()).await;
    let q = r.asked().remove(0);
    assert!(!q.needs_proof, "{}", q.reason);
    assert!(!q.reason.contains("second proof"), "{}", q.reason);
}

/// Removing a server asks its card at the default, whether it was attached by
/// asking or started from the settings file, and the card needs no code: the
/// owner's yes alone removes it, with nothing paired.
#[tokio::test]
async fn removing_a_server_asks_its_card_with_no_code() {
    let r = rig();
    r.attached(wc()).await;
    assert!(r.core.mcp.is_loaded("wc"));
    for name in ["wc", "base"] {
        let sid = r.session();
        r.turn(
            &sid,
            "mcp_detach",
            json!({"name": name, "reason": "the owner asked"}),
        )
        .await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Ok, "{name}: {text}");
        assert!(!text.contains("second proof"), "{name}: {text}");
        let q = r.asked().remove(0);
        assert!(!q.needs_proof && !q.needs_fresh_proof, "{name}: {q:?}");
        assert!(!q.reason.contains("second proof"), "{name}: {}", q.reason);
        assert!(
            Config::parse(&r.file_text())
                .unwrap()
                .0
                .mcp
                .servers
                .contains_key(name),
            "{name}: nothing written before the yes"
        );
        r.core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap();
        assert!(
            !Config::parse(&r.file_text())
                .unwrap()
                .0
                .mcp
                .servers
                .contains_key(name),
            "{name}: the yes alone removes it"
        );
    }
    assert!(!r.core.mcp.is_loaded("wc"));
}
