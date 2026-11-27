//! The daemon's own Sign in card (issue 59): when a server that signs in loses its sign-in for good,
//! one `mcp_sign_in` card to the owner's direct messages, once for that loss: not again on a retry,
//! a restart of the server, or a daemon that starts again on the same store; the words in the
//! conversation stay; a server with no `oauth` gets none; the card is the operator's, never a
//! shared place's.

use std::time::Duration;

use serde_json::json;

use super::oauth::{attached_by, login, notes, rig, status};
use super::{session, until};

/// The `mcp_sign_in` posts waiting for the owner, and where each goes.
fn cards(r: &super::oauth::Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "mcp_sign_in")
        .map(|a| format!("{a:?}"))
        .collect()
}

#[tokio::test]
async fn a_lost_sign_in_gets_one_card_for_its_loss() {
    let r = rig().await;
    let sid = session(&r.core, None);
    attached_by(&r, &sid);
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    assert!(cards(&r).is_empty(), "no card while it is signed in");
    r.auth.revoke_all();
    let client = r
        .core
        .mcp
        .server("docs")
        .unwrap()
        .client(Duration::from_secs(5))
        .await
        .unwrap();
    assert!(client
        .call_tool("echo", json!({"text": "x"}))
        .await
        .is_err());
    until("needs sign-in", || status(&r).state == "needs sign-in").await;
    until("the card", || !cards(&r).is_empty()).await;
    until("the note", || !notes(&r, &sid).is_empty()).await;
    // A restart of the server, and the time a retry would take: no second card.
    r.core.mcp.restart("docs");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(cards(&r).len(), 1, "one card for the loss");
    assert_eq!(
        notes(&r, &sid).len(),
        1,
        "the conversation's words stay, once"
    );
    let card = &cards(&r)[0];
    assert!(card.contains("Sign in to the MCP server `docs`"), "{card}");
    // A daemon that starts again on the same store, for the same loss: the mark holds.
    let since = r
        .core
        .mcp
        .signed(r.core.mcp.server("docs").unwrap())
        .unwrap()
        .kept_since()
        .await
        .unwrap();
    r.core.mcp.offer_sign_in("docs", &format!("lost:{since}"));
    assert_eq!(cards(&r).len(), 1, "the same loss, no second card");
    // Past the one-card window (its time put back), the mark alone holds the same loss to one
    // card; another loss gets its own.
    r.core
        .store
        .put_meta("consent.card_at.mcp:docs", &0_u64)
        .unwrap();
    r.core.mcp.offer_sign_in("docs", &format!("lost:{since}"));
    assert_eq!(cards(&r).len(), 1, "the mark holds the same loss");
    r.core.mcp.offer_sign_in("docs", "lost:another");
    assert_eq!(cards(&r).len(), 2, "another loss, its own card");
    // Within the window, a third change posts none: the card is still the owner's.
    r.core.mcp.offer_sign_in("docs", "lost:a-third");
    assert_eq!(cards(&r).len(), 2, "one card per server in its window");
    // Every card is the operator's: none waits for any other place.
    let all = r.core.outbox.open_for(crate::outbox::OPERATOR_TARGET).len()
        + r.core.outbox.open_for(&sid).len();
    assert!(all >= 1);
    assert!(r
        .core
        .outbox
        .open_for(&sid)
        .iter()
        .all(|a| crate::outbox::kind_of(a) != "mcp_sign_in"));
}

/// A server with no `oauth` is never offered a card.
#[tokio::test]
async fn a_server_with_no_sign_in_gets_no_card() {
    let r = super::oauth_doors::rig_doors(theseus_mcp::oauth::fake::AuthConfig::default())
        .await
        .0;
    r.core.mcp.start();
    r.core.mcp.offer_sign_in("plain", "lost:1");
    r.core.mcp.offer_sign_in("plain", "attach:1");
    assert!(cards(&r).is_empty());
}
