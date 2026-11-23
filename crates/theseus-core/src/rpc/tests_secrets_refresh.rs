//! `secrets.refresh` over the wire (issue 44): the owner's from the CLI, once
//! a minute, and nobody's from the MCP server's surface; a name `[secrets]`
//! lacks is told in words. What a refresh then reads, counted run by run, is
//! `secrets::tests_pace`.

use serde_json::json;
use theseus_protocol::{error_code, method};

use crate::approval::{Client, Surface};
use crate::tests_m3::rpc_as;

#[tokio::test]
async fn a_refresh_is_the_owners_from_a_private_place_once_a_minute() {
    let core = super::tests::test_core("");
    let cli = || Client::new("sock#1", Surface::Cli);
    let got = rpc_as(&core, cli(), method::SECRETS_REFRESH, json!({}))
        .await
        .unwrap();
    assert_eq!(got["asked"], json!(["anthropic_api_key"]));
    let again = rpc_as(
        &core,
        cli(),
        method::SECRETS_REFRESH,
        json!({"name": "anthropic_api_key"}),
    )
    .await
    .unwrap_err();
    assert!(
        again.message.contains("less than a minute ago"),
        "{again:?}"
    );

    let fresh = super::tests::test_core("");
    let mcp = rpc_as(
        &fresh,
        Client::new("mcp#1", Surface::Mcp),
        method::SECRETS_REFRESH,
        json!({}),
    )
    .await
    .unwrap_err();
    assert_eq!(mcp.code, error_code::REFUSED, "{mcp:?}");
    assert!(
        mcp.message.starts_with("secrets.refresh is the owner's, in a private place"),
        "{mcp:?}"
    );
    let none = rpc_as(&fresh, cli(), method::SECRETS_REFRESH, json!({"name": "nope"}))
        .await
        .unwrap_err();
    assert!(none.message.contains("no [secrets] entry"), "{none:?}");
    let first = rpc_as(&fresh, cli(), method::SECRETS_REFRESH, json!({}))
        .await
        .unwrap();
    assert_eq!(
        first["asked"],
        json!(["anthropic_api_key"]),
        "a refused ask is no refresh, and starts no minute"
    );
}
