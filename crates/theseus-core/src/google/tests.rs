//! Google through the whole core, against the loopback fake: the rig (a core
//! whose `[google]` points at the fake, with the secrets on its board), the
//! check and its states, and a call's result (private, outside text, one row).
//! `tests_auth` is the consent and the minting, `tests_gate` the asking and the
//! send cap, `tests_private` the data rule.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_google::fake::{Fake, REFRESH_TOKEN};
use theseus_google::{Policy, Service};
use theseus_protocol::SessionKind;

use super::{Google, Phase};
use crate::config::{GoogleConfig, GoogleCredentialNames};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::secrets::{Secret, SecretBoard};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::test_support::{turn, until};
use crate::{Config, Core};

pub(crate) const ACCOUNT: &str = "owner@example.test";
pub(crate) const CLIENT: &str = "client_id: id-1.apps.example\nclient_secret: s3cret-client-0123";

pub(crate) struct Rig {
    pub core: Arc<Core>,
    pub fake: Fake,
    pub dir: tempfile::TempDir,
    pub g: Arc<Google>,
}

/// The board: the client always, the refresh token when `token` is given (else
/// it failed to resolve, as an unminted one does).
pub(crate) fn board(token: Option<&str>) -> Arc<SecretBoard> {
    let b = SecretBoard::new(
        [
            "google_client".to_string(),
            "google_refresh_token".to_string(),
        ],
        Instant::now(),
    );
    let refresh = match token {
        Some(t) => Ok(Secret::new(t.into())),
        None => Err("op read failed: the item is not there yet".to_string()),
    };
    b.publish(
        BTreeMap::from([
            ("google_client".to_string(), Ok(Secret::new(CLIENT.into()))),
            ("google_refresh_token".to_string(), refresh),
        ]),
        "test",
    );
    b
}

pub(crate) fn config(dir: &Path, fake: &Fake, refresh_ref: &str) -> Config {
    let mut cfg = Config::example();
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.google = GoogleConfig {
        account: ACCOUNT.into(),
        endpoint: Some(fake.url()),
        credentials: GoogleCredentialNames::default(),
        ..Default::default()
    };
    cfg.secrets.insert(
        "google_client".into(),
        "op://vault/google client/notesPlain".into(),
    );
    cfg.secrets
        .insert("google_refresh_token".into(), refresh_ref.into());
    cfg
}

/// A rig with the refresh token minted (`Some`) or not, the model's script,
/// and `tweak` on the config.
pub(crate) async fn rig_with(
    token: Option<&str>,
    refresh_ref: impl FnOnce(&Path) -> String,
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), &fake, &refresh_ref(dir.path()));
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts {
        secrets: board(token),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    let g = core
        .tools
        .google
        .clone()
        .expect("[google] binds an account");
    Rig { core, fake, dir, g }
}

pub(crate) async fn rig(script: Vec<Scripted>) -> Rig {
    rig_with(
        Some(REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        script,
        |_| {},
    )
    .await
}

pub(crate) fn session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

pub(crate) fn ledgered(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

pub(crate) fn call(id: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, "google_call", input)])
}

/// Every ledger row and every node of `sessions`, as JSON text, for a grep.
pub(crate) fn everything(core: &Core, sessions: &[&str]) -> String {
    let rows = core.store.ledger_tail::<Value>(100_000).unwrap();
    let mut text = serde_json::to_string(&rows).unwrap();
    for sid in sessions {
        for (_, n) in core.store.session_nodes(sid).unwrap() {
            text.push_str(&serde_json::to_string(&n.body).unwrap_or_default());
        }
    }
    text
}

#[tokio::test]
async fn the_check_confirms_the_account_and_the_tools_follow_the_grant() {
    let r = rig(vec![]).await;
    assert_eq!(r.g.check_now().phase, Phase::Unchecked);
    let detail = r.g.check().await;
    assert_eq!(detail["state"], "ok", "{detail}");
    assert_eq!(r.g.check_now().phase, Phase::Ok);
    let s = r.g.status();
    assert_eq!((s.state.as_str(), s.account.as_str()), ("ok", ACCOUNT));
    assert!(s.tools_on.contains(&"google.call".to_string()));
    // The cheapest call was the mail profile, once, with the access token.
    let log = r.fake.log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].path, "/gmail/v1/users/me/profile");
}

#[tokio::test]
async fn a_token_for_another_account_fails_the_check_closed() {
    let r = rig(vec![]).await;
    r.fake.set_account("collaborator@example.test");
    let detail = r.g.check().await;
    assert_eq!(detail["state"], "needs_auth");
    let e = r.g.ready("google.call").await.unwrap_err();
    assert!(e.contains("different account"), "{e}");
    assert!(
        !e.contains("collaborator"),
        "the other address is not echoed: {e}"
    );
}

#[tokio::test]
async fn a_disabled_api_names_the_api_to_enable() {
    let r = rig(vec![]).await;
    r.fake.service_disabled(Service::Gmail, "424242");
    let detail = r.g.check().await;
    assert_eq!(detail["state"], "needs_auth");
    let e = r.g.ready("google.call").await.unwrap_err();
    assert!(
        e.contains("enable the Gmail API in project 424242: https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=424242"),
        "{e}"
    );
}

#[tokio::test]
async fn the_client_secret_without_a_client_id_fails_the_check_and_never_echoes_it() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &fake, "file:/tmp/unused-google-refresh-token");
    let b = SecretBoard::new(
        [
            "google_client".to_string(),
            "google_refresh_token".to_string(),
        ],
        Instant::now(),
    );
    b.publish(
        BTreeMap::from([
            (
                "google_client".to_string(),
                Ok(Secret::new("client_secret: only-the-secret-0123".into())),
            ),
            (
                "google_refresh_token".to_string(),
                Ok(Secret::new(REFRESH_TOKEN.into())),
            ),
        ]),
        "test",
    );
    let g = Google::from_config(&cfg.google, b).unwrap();
    let detail = g.check().await;
    let e = detail["error"].as_str().unwrap();
    assert_eq!(
        e,
        "the client secret `google_client` holds no client_id line"
    );
    assert!(!format!("{detail}").contains("only-the-secret"));
}

#[tokio::test]
async fn a_google_read_runs_marks_its_session_private_and_its_result_as_outside_text() {
    let r = rig(vec![
        call(
            "c1",
            json!({"method": "drive.files.list", "params": {"pageSize": 5}}),
        ),
        Scripted::text("Done."),
    ])
    .await;
    r.fake.json(
        "GET",
        "/drive/v3/files",
        json!({"files": [{"id": "f1", "name": "Quarterly plan"}]}),
    );
    let sid = session(&r.core);
    turn(&r.core, &sid, "list my files").await;
    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let result = nodes
        .iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult {
                tool,
                content,
                meta,
                external,
                status,
                ..
            } if tool == "google.call" => {
                Some((content.clone(), meta.clone(), external.clone(), *status))
            }
            _ => None,
        })
        .expect("a google.call result");
    assert_eq!(result.3, crate::node::ResultStatus::Ok, "{}", result.0);
    assert!(result.0.contains("Quarterly plan"), "{}", result.0);
    assert!(super::private::marked(&result.1), "{:?}", result.1);
    assert_eq!(
        result.2.expect("outside text").url,
        "google:drive.files.list"
    );
    // The marker is all there is: no row, no hold. The session is private from then on.
    assert!(ledgered(&r.core, "session.external_read").is_empty());
    assert!(super::private::session(&r.core.store, &r.core.cfg, &sid));
    // One row, with no content in it.
    let rows = ledgered(&r.core, "google.called");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(
        (
            row["service"].as_str(),
            row["method"].as_str(),
            row["class"].as_str(),
            row["outcome"].as_str()
        ),
        (
            Some("drive"),
            Some("drive.files.list"),
            Some("read"),
            Some("ok")
        )
    );
    assert_eq!(row["status"], 200);
    assert!(!row.to_string().contains("Quarterly"), "{row}");
}

#[tokio::test]
async fn a_failed_call_is_a_row_too_and_says_what_google_said() {
    let r = rig(vec![
        call(
            "c1",
            json!({"method": "drive.files.get", "params": {"fileId": "missing"}}),
        ),
        Scripted::text("Done."),
    ])
    .await;
    let sid = session(&r.core);
    turn(&r.core, &sid, "get it").await;
    let rows = ledgered(&r.core, "google.called");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["outcome"].as_str(), rows[0]["status"].as_u64()),
        (Some("failed"), Some(404))
    );
    assert_eq!(rows[0]["error"], "notFound");
    assert_eq!(rows[0]["resource"], "missing");
}

#[tokio::test]
async fn refresh_is_single_flight_through_the_core() {
    let r = rig(vec![]).await;
    r.fake.set_refresh_delay(Duration::from_millis(150));
    let mut all = Vec::new();
    for _ in 0..10 {
        let g = r.g.clone();
        all.push(tokio::spawn(async move {
            g.ready("google.call").await.map(|t| t.expose().to_string())
        }));
    }
    let mut seen = std::collections::BTreeSet::new();
    for h in all {
        seen.insert(h.await.unwrap().unwrap());
    }
    assert_eq!(seen.len(), 1);
    assert_eq!(
        r.fake.token_requests(),
        1,
        "the check's refresh was the only one"
    );
}

#[tokio::test]
async fn invalid_grant_sets_needs_auth_and_one_notice() {
    let r = rig(vec![]).await;
    r.fake.set_invalid_grant(true);
    r.core.google_after_serving();
    until("the lapse's row", || {
        !ledgered(&r.core, "google.auth_lapsed").is_empty()
    })
    .await;
    assert_eq!(
        r.g.check_now().phase,
        Phase::NeedsAuth(theseus_google::error::LAPSED.into())
    );
    // A second lapsed refresh, by a check and by a call: no second row.
    r.g.check().await;
    assert_eq!(
        r.g.ready("google.call").await.unwrap_err(),
        theseus_google::error::LAPSED
    );
    r.g.lapse();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ledgered(&r.core, "google.auth_lapsed").len(), 1);
    assert_eq!(r.g.status().state, "needs_auth");
}

#[test]
fn a_fast_policy_is_the_default_shape() {
    // The core's retry is the crate's table: three attempts, one second up.
    let p = Policy::default();
    assert_eq!((p.max_attempts, p.initial_backoff.as_secs()), (3, 1));
}
