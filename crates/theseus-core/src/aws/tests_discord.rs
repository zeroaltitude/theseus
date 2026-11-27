//! The alerts' confirmation from Discord (issue 59), through the real RPC on the Discord surface
//! with the fake AWS: the owner's own press in a private place confirms with the token pasted into
//! the form, as `theseus aws confirm-alerts` confirms; a stranger, a shared place and a call that
//! names no presser are refused before any AWS call; the token is in no exit; and a turn only
//! offers.

use std::sync::Arc;

use serde_json::{json, Value};

use super::tests::{account, board, sts, Fake, Reply, Seen, ACCOUNT};
use crate::approval::Surface;
use crate::tests_mcp_memory::rpc;

/// Discord ids: 15 to 21 digits, as `[places] owner` is checked.
const OWNER: &str = "100000000000000042";
const STRANGER: &str = "100000000000000077";
/// Invented: SNS's confirmation token. Never in any output.
const TOKEN: &str = "5f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

/// SNS as the confirmation needs it: the token confirms, anything else is refused.
fn sns(s: &Seen, n: usize) -> Reply {
    let xml = |body: String| {
        (
            200,
            vec![
                ("x-amzn-requestid", format!("req-{n}")),
                ("content-type", "text/xml".into()),
            ],
            body,
        )
    };
    let token_ok = s.body.split('&').any(|kv| kv == format!("Token={TOKEN}"));
    match s.action() {
        Some("GetCallerIdentity") => sts(ACCOUNT, n),
        Some("ConfirmSubscription") if token_ok => xml(format!(
            "<ConfirmSubscriptionResponse><ConfirmSubscriptionResult><SubscriptionArn>\
             arn:aws:sns:us-west-2:{ACCOUNT}:theseus-alerts:sub-1</SubscriptionArn>\
             </ConfirmSubscriptionResult><ResponseMetadata><RequestId>req-{n}</RequestId>\
             </ResponseMetadata></ConfirmSubscriptionResponse>"
        )),
        Some("GetSubscriptionAttributes") => xml(format!(
            "<GetSubscriptionAttributesResponse><GetSubscriptionAttributesResult><Attributes>\
             <entry><key>ConfirmationWasAuthenticated</key><value>true</value></entry>\
             <entry><key>PendingConfirmation</key><value>false</value></entry>\
             <entry><key>Endpoint</key><value>operator@example.com</value></entry>\
             </Attributes></GetSubscriptionAttributesResult><ResponseMetadata>\
             <RequestId>req-{n}</RequestId></ResponseMetadata></GetSubscriptionAttributesResponse>"
        )),
        _ => (
            400,
            vec![("x-amzn-requestid", format!("req-{n}"))],
            "<ErrorResponse><Error><Type>Sender</Type><Code>InvalidParameter</Code>\
             <Message>Invalid parameter: Token</Message></Error></ErrorResponse>"
                .into(),
        ),
    }
}

/// A core bound to the account at the fake, whose owner is Discord user `OWNER`.
fn owned(fake: &Fake, dir: &std::path::Path) -> Arc<crate::Core> {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.aws = account(&fake.url);
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.validate().unwrap();
    let store = crate::store::Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(crate::provider::FakeProvider::scripted(vec![]));
    crate::Core::build(crate::rpc::Parts {
        secrets: board(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap()
}

fn pressed(user: &str, dm: bool) -> Value {
    let mut d = json!({"user_id": user, "channel_id": if dm { "500" } else { "900" }});
    if !dm {
        d["guild_id"] = json!("1");
    }
    json!({
        "token": format!("https://sns.us-west-2.amazonaws.com/?Action=ConfirmSubscription&TopicArn=x&Token={TOKEN}"),
        "author": format!("discord:{user}"),
        "discord": d,
    })
}

fn refused(r: &Result<Value, theseus_protocol::RpcError>) -> bool {
    r.as_ref().err().is_some_and(|e| {
        e.code == theseus_protocol::error_code::REFUSED
            && e.message
                .starts_with("the alerts' confirmation from Discord is the owner's own press")
    })
}

/// The owner's press in a DM confirms, authenticated; the token is in no row and no open post.
#[tokio::test]
async fn the_owners_press_confirms_the_alerts_and_the_token_is_in_no_exit() {
    let fake = Fake::start(sns);
    let dir = tempfile::tempdir().unwrap();
    let core = owned(&fake, dir.path());
    let done = rpc(
        &core,
        Surface::Discord,
        "aws.confirm_alerts",
        pressed(OWNER, true),
    )
    .await
    .unwrap();
    assert_eq!(done["authenticated"], true, "{done}");
    assert_eq!(done["account"], ACCOUNT);
    let confirms = fake
        .seen()
        .into_iter()
        .filter(|s| s.action() == Some("ConfirmSubscription"))
        .count();
    assert_eq!(confirms, 1, "one confirmation, as the CLI's");
    core.store.flush_waiting();
    let all = format!(
        "{} {done} {:?}",
        crate::google::tests::everything(&core, &[]),
        core.outbox.open_for(crate::outbox::OPERATOR_TARGET)
    );
    assert!(!all.contains(TOKEN), "the token is in an exit");
}

/// A stranger in a DM and the owner in a shared channel are refused before any AWS call; a
/// Discord call that names no presser is refused in the CLI's old words.
#[tokio::test]
async fn a_stranger_a_shared_place_and_no_presser_are_refused_the_alerts() {
    let fake = Fake::start(sns);
    let dir = tempfile::tempdir().unwrap();
    let core = owned(&fake, dir.path());
    for who in [pressed(STRANGER, true), pressed(OWNER, false)] {
        let r = rpc(&core, Surface::Discord, "aws.confirm_alerts", who).await;
        assert!(refused(&r), "{r:?}");
        assert!(!format!("{r:?}").contains(TOKEN));
    }
    let plain = rpc(
        &core,
        Surface::Discord,
        "aws.confirm_alerts",
        json!({"token": TOKEN}),
    )
    .await;
    assert!(
        plain
            .as_ref()
            .err()
            .is_some_and(|e| e.message.contains("the operator's, from the CLI")),
        "{plain:?}"
    );
    assert!(
        fake.seen()
            .iter()
            .all(|s| s.action() != Some("ConfirmSubscription")),
        "nothing was confirmed"
    );
}

/// A turn's tools never confirm: the offer is there where an account is bound, takes no token, and
/// says nothing was confirmed.
#[tokio::test]
async fn a_turn_only_offers_the_alerts_confirmation() {
    let fake = Fake::start(sns);
    let dir = tempfile::tempdir().unwrap();
    let core = owned(&fake, dir.path());
    let tools: Vec<String> = core
        .tools
        .registry
        .all()
        .map(|t| t.name().to_string())
        .collect();
    assert!(tools.iter().any(|t| t == super::offer::OFFER), "{tools:?}");
    assert!(tools.iter().all(|t| !t.contains("confirm_alerts")));
    let offer = core.tools.registry.get(super::offer::OFFER).unwrap();
    assert_eq!(offer.input_schema()["properties"], json!({}));
    assert!(super::offer::OFFERED.contains("Nothing was confirmed"));
}
