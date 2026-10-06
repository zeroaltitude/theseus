//! `ListItsPrefix` in the tender's and the restore's sessions, as real S3
//! judges it (theseus-bfk9): a HEAD or GET of a missing key is judged on
//! the implied `s3:ListBucket` with `s3:prefix` set to the key, so a missing
//! key under the session's own prefix is 404 (`NoSuchKey`), not 403; a list
//! under that prefix is allowed; and a list that names no prefix, names
//! `durability/`, or names another deployment's prefix is refused. The fake
//! (`tests_durable::may_list`) judges each request so.

use serde_json::{json, Value};

use super::durable::read::{self, Digest, ReadError, Reader};
use super::durable::s3::Bucket;
use super::durable::{self, TENDER};
use super::tests::ACCOUNT;
use super::tests_durable::{layer, may_list, Fake, PREFIX};

/// The tender's policy, for the fake's account.
fn tender_policy(fake: &Fake) -> Value {
    let aws = layer(fake);
    let account = aws.accounts().next().unwrap().clone();
    durable::policy(TENDER, ACCOUNT, &account.cfg).unwrap()
}

/// What each session may list: only under its own prefix.
fn lists_only_its_own_prefix(policy: &Value) {
    for allowed in [
        PREFIX,
        "durability/theseus-lab/wal/",
        "durability/theseus-lab/wal/none",
    ] {
        assert!(may_list(policy, Some(allowed)), "{allowed} refused");
    }
    assert!(!may_list(policy, None), "a list with no prefix passed");
    for refused in [
        "",
        "durability/",
        "durability/theseus-other/",
        "durability/theseus-other/wal/none",
        "durability/theseus-lab",
    ] {
        assert!(!may_list(policy, Some(refused)), "{refused:?} passed");
    }
}

#[test]
fn the_tender_session_lists_only_its_own_prefix() {
    lists_only_its_own_prefix(&tender_policy(&Fake::start()));
}

#[test]
fn the_restore_session_lists_only_its_own_prefix() {
    lists_only_its_own_prefix(&read::policy(ACCOUNT, "us-west-2", "theseus-lab"));
}

/// The fake models `IfExists` as IAM does: a list with no prefix passes it,
/// which is the whole bucket's key names (as built before theseus-bfk9).
#[test]
fn the_fake_admits_a_list_with_no_prefix_under_if_exists_only() {
    let policy = |op: &str| {
        json!({"Statement": [{
            "Effect": "Allow",
            "Action": "s3:ListBucket",
            "Resource": "arn:aws:s3:::a-bucket",
            "Condition": {op: {"s3:prefix": [format!("{PREFIX}*")]}},
        }]})
    };
    assert!(may_list(&policy("StringLikeIfExists"), None));
    assert!(!may_list(&policy("StringLike"), None));
    for op in ["StringLike", "StringLikeIfExists"] {
        assert!(may_list(
            &policy(op),
            Some("durability/theseus-lab/wal/none")
        ));
        assert!(!may_list(&policy(op), Some("durability/")));
    }
    let exact = json!({"Statement": [{
        "Effect": "Allow", "Action": "s3:ListBucket",
        "Condition": {"StringEquals": {"s3:prefix": "durability/theseus-lab/wal/none"}},
    }]});
    assert!(may_list(&exact, Some("durability/theseus-lab/wal/none")));
    assert!(!may_list(&exact, Some("durability/theseus-lab/wal/")));
}

/// The tender's HEAD of a missing key: 404 under its own prefix (so the
/// key reads as never stored), 403 under another deployment's.
#[tokio::test]
async fn the_tender_heads_a_missing_key_under_its_prefix_as_missing() {
    let fake = Fake::start();
    let aws = layer(&fake);
    let account = aws.accounts().next().unwrap().clone();
    let bucket = Bucket {
        name: durable::bucket(ACCOUNT, &account.cfg.region),
        region: account.cfg.region.clone(),
        account,
    };
    let got = bucket.checksum(&format!("{PREFIX}wal/none")).await;
    assert!(matches!(got, Ok(None)), "{:?}", got.map_err(String::from));
    let other = bucket.checksum("durability/theseus-other/wal/none").await;
    assert!(
        other.is_err(),
        "another deployment's key: {other:?}",
        other = other.map_err(String::from)
    );
    assert_eq!(fake.ops("HeadObject").len(), 2);
}

/// The restore's GET of a missing key: `NoSuchKey` under its own prefix
/// (a missing blob is said, and the rest restored), `AccessDenied` under
/// another deployment's.
#[tokio::test]
async fn the_restore_gets_a_missing_key_under_its_prefix_as_missing() {
    let fake = Fake::start();
    let aws = layer(&fake);
    let account = aws.accounts().next().unwrap().clone();
    let creds = read::session(&account, "theseus-lab").await.unwrap();
    let reader = Reader {
        bucket: durable::bucket(ACCOUNT, &account.cfg.region),
        region: account.cfg.region.clone(),
        account,
        creds,
        chunk: read::CHUNK,
        page: None,
    };
    let want = Digest::Hex("00".repeat(32));
    let got = reader.get(&format!("{PREFIX}blobs/none"), 1, &want).await;
    assert!(matches!(got, Err(ReadError::Missing)), "{got:?}");
    let other = reader
        .get("durability/theseus-other/blobs/none", 1, &want)
        .await;
    match other {
        Err(ReadError::Other(e)) => assert!(e.contains("AccessDenied"), "{e}"),
        other => panic!("another deployment's key: {other:?}"),
    }
}

/// The live check's helper: prints the exact inline policy of the tender's
/// session and of the restore's, for a named account, region and deployment,
/// so a session can be minted with each and probed by hand. Ignored, so the
/// suite never runs it:
///
/// ```text
/// THESEUS_POLICY_ACCOUNT=<account> THESEUS_POLICY_REGION=<region> \
/// THESEUS_POLICY_DEPLOYMENT=<deployment> cargo nextest run --workspace \
///   --run-ignored only --no-capture -E 'test(print_the_durability_policies)'
/// ```
#[test]
#[ignore = "a helper for the live check: prints the policies"]
fn print_the_durability_policies() {
    let var = |k: &str, or: &str| std::env::var(k).unwrap_or_else(|_| or.to_string());
    let account = var("THESEUS_POLICY_ACCOUNT", ACCOUNT);
    let region = var("THESEUS_POLICY_REGION", "us-west-2");
    let deployment = var("THESEUS_POLICY_DEPLOYMENT", "theseus-lab");
    let cfg = crate::config::AwsAccountConfig {
        credentials: Default::default(),
        region: region.clone(),
        regions: Vec::new(),
        endpoint: None,
        owner_role: Some("theseus-owner".into()),
        deployment: Some(deployment.clone()),
        monthly_budget_usd: None,
        daily_budget_usd: None,
        hourly_alert_usd: crate::config::default_hourly_alert_usd(),
        runaway_factor: crate::config::default_runaway_factor(),
        durability: true,
        hands_network: None,
    };
    let tender = durable::policy(TENDER, &account, &cfg).unwrap();
    let restore = read::policy(&account, &region, &deployment);
    println!("# durable::policy (session theseus-durability)");
    println!("{}", serde_json::to_string_pretty(&tender).unwrap());
    println!("# read::policy (session theseus-restore)");
    println!("{}", serde_json::to_string_pretty(&restore).unwrap());
}
