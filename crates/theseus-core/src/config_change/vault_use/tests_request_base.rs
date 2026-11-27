//! `http.request` (issue 65) on the run form's rules: the item is remembered by name before a key
//! can leave, so a start holds its value again and every exit masks it; a record that cannot be
//! read sends nothing; a turn the owner did not type asks; a value the board holds for Theseus at
//! run time is a key Theseus runs on. What is checked is what LEAVES: the model's requests, the
//! courier and the push line.

use std::sync::Arc;

use serde_json::json;

use super::tests_once_exits::leaves;
use super::tests_request::{
    call, last, notes, read_vault, rig_req, rig_typed, serve, Req, RUNS_ON_WORDS, VALUE,
};
use crate::config::WebToolsConfig;
use crate::content::OutsideMode;
use crate::node::ResultStatus;
use crate::secrets::look::tests::stub;

/// The board's entries a start held for remembered items.
fn held_again(r: &crate::config_change::tests::Rig) -> Vec<String> {
    r.core
        .secrets
        .states()
        .keys()
        .filter(|k| k.starts_with(super::used::HANDLE))
        .cloned()
        .collect()
}

/// The service echoes the key. What leaves holds no value, before a restart and after one that
/// reads the remembered item again; the start holds it, so the model's next request, the courier
/// and the push line mask it.
#[tokio::test]
async fn what_leaves_holds_no_key_before_and_after_a_restart() {
    let o = rig_req().await;
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![(
            "http_request",
            call(o.server.port, "GET", "/echo", json!({})),
        )],
    )
    .await;
    let (status, text) = last(&o.r, &sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(!leaves(&o.r, &sid).contains(VALUE), "the value left");
    let Req {
        r, server, vault, ..
    } = o;
    let r = r.restart(|_, _| {});
    r.core.tools.look.set_vault(Arc::new(stub(&vault, "ok")));
    r.core.hold_vault_used().await;
    r.owner_types();
    assert_eq!(
        held_again(&r).len(),
        1,
        "the item was remembered and held again"
    );
    r.says("anything new?");
    r.run_turn(&sid).await;
    assert!(
        !leaves(&r, &sid).contains(VALUE),
        "the value left after a restart"
    );
    drop(server);
}

/// A record of the used items that cannot be read is not taken for an empty list: the item is not
/// remembered, so nothing is sent.
#[tokio::test]
async fn an_unreadable_record_sends_nothing() {
    let o = rig_req().await;
    o.r.core
        .store
        .put_meta(super::used::KEY, &json!("not a list"))
        .unwrap();
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![(
            "http_request",
            call(o.server.port, "GET", "/v1/things", json!({})),
        )],
    )
    .await;
    let (status, text) = last(&o.r, &sid, "p1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("could not note this item"), "{text}");
    assert!(o.server.seen().is_empty(), "nothing sent");
    assert!(notes(&o.r).is_empty());
}

/// A turn whose input `turn.submit` did not judge the owner's own typing (a job's, an agent's)
/// asks first, with nothing read.
#[tokio::test]
async fn a_turn_the_owner_did_not_type_asks() {
    let server = serve().await;
    let web = crate::web::tests::web(server.port, WebToolsConfig::default(), true);
    let o = rig_typed(server, web, OutsideMode::default(), false, |_| {});
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![(
            "http_request",
            call(o.server.port, "GET", "/v1/things", json!({})),
        )],
    )
    .await;
    let asked = o.r.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].reason.contains(super::once::NOT_BEGUN),
        "{}",
        asked[0].reason
    );
    assert!(!read_vault(&o), "nothing read before the answer");
    assert!(o.server.seen().is_empty());
}

/// A value the board holds for Theseus at run time (a signed-in server's token), copied into a
/// vault item, is a key Theseus runs on: not sent.
#[tokio::test]
async fn a_key_theseus_holds_at_run_time_is_not_sent() {
    let o = rig_req().await;
    o.r.core.secrets.hold(
        "mcp-oauth:lantern",
        crate::secrets::Secret::new(VALUE.into()),
    );
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![(
            "http_request",
            call(o.server.port, "GET", "/v1/things", json!({})),
        )],
    )
    .await;
    let (status, text) = last(&o.r, &sid, "p1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains(RUNS_ON_WORDS), "{text}");
    assert!(o.server.seen().is_empty());
}
