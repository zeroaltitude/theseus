//! The data rule (the Google design §4.1; shared contract 10): once a session
//! has read any Google result, its data answer is `private` by code, the route
//! skips every profile of restricted origin, and the provider floor refuses a
//! request of that session to a model of restricted origin before a byte is
//! sent. One mark; no origin logic here.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use theseus_google::fake::{Fake, REFRESH_TOKEN};
use theseus_judge::fake::FakeJev;
use theseus_protocol::SessionKind;

use super::tests::{call, config, ACCOUNT, CLIENT};
use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::secrets::{Secret, SecretBoard};
use crate::session::SessionRecord;
use crate::test_support::until;
use crate::tests_route::{mode, rig_parts, Rig as RouteRig};
use crate::turn::TurnRequest;

fn board() -> Arc<SecretBoard> {
    let b = SecretBoard::new(
        ["jev_api_key", "google_client", "google_refresh_token"].map(String::from),
        Instant::now(),
    );
    b.publish(
        BTreeMap::from([
            (
                "jev_api_key".to_string(),
                Ok(Secret::new("jev-test-key-0123456789".into())),
            ),
            ("google_client".to_string(), Ok(Secret::new(CLIENT.into()))),
            (
                "google_refresh_token".to_string(),
                Ok(Secret::new(REFRESH_TOKEN.into())),
            ),
        ]),
        "test",
    );
    b
}

/// The route rig of `tests_route` (`anthropic` and `zai`, the latter a
/// China-origin endpoint), with `[google]` pointed at `fake`, and the
/// Anthropic fake scripted by `script`.
fn rig(jev: Option<&FakeJev>, fake: &Fake, script: Vec<Scripted>) -> (RouteRig, Arc<FakeProvider>) {
    let claude = Arc::new(FakeProvider::scripted(script));
    let dir = tempfile::tempdir().unwrap();
    let google = config(dir.path(), fake, "file:/tmp/unused-google-refresh-token").google;
    let (c2, url) = (claude.clone(), fake.url());
    let r = rig_parts(
        jev,
        0,
        move |c| {
            c.google = google;
            c.google.endpoint = Some(url);
            c.secrets.insert(
                "google_client".into(),
                "op://vault/google client/notesPlain".into(),
            );
            c.secrets.insert(
                "google_refresh_token".into(),
                "file:/tmp/unused-google-refresh-token".into(),
            );
            c.routing.classes.routine_coding.profiles = vec!["glm53".into(), "sonnet".into()];
        },
        move |p| {
            p.secrets = board();
            p.providers.insert("anthropic".into(), c2);
        },
    );
    (r, claude)
}

async fn run_on(
    core: &Arc<crate::Core>,
    sid: &str,
    input: &str,
    profile: Option<&str>,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core
        .runner
        .resolve_target(&live, profile, None, None)
        .unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

fn new_session(core: &crate::Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_google_result_makes_the_session_private_and_the_floor_refuses_a_restricted_model() {
    let fake = Fake::start().await;
    fake.json(
        "GET",
        "/drive/v3/files",
        json!({"files": [{"id": "f1", "name": "Quarterly plan"}]}),
    );
    let (r, claude) = rig(
        None,
        &fake,
        vec![
            call("c1", json!({"method": "drive.files.list"})),
            Scripted::text("Done."),
            Scripted::text("Answered."),
        ],
    );
    let sid = new_session(&r.core);
    // Before any Google result the session is not private, and a restricted
    // model is open to it.
    assert!(!super::private::session(&r.core.store, &r.core.cfg, &sid));
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    assert_eq!(claude.requests().len(), 2, "the call and its answer");
    assert!(
        super::private::session(&r.core.store, &r.core.cfg, &sid),
        "the Google result marked it"
    );
    // The next turn, pinned to the restricted model, is refused before a byte is sent.
    let before = r.zai.requests().len();
    let ended = run_on(&r.core, &sid, "and now?", Some("glm53")).await;
    let said = format!("{ended:?}");
    assert!(said.contains("data_gate"), "{said}");
    assert_eq!(
        r.zai.requests().len(),
        before,
        "nothing went to the restricted model"
    );
    assert!(r.zai.requests().is_empty());
    // A session that never read Google is not held back by it.
    let other = new_session(&r.core);
    assert!(!super::private::session(&r.core.store, &r.core.cfg, &other));
    until("the row", || {
        !super::tests::ledgered(&r.core, "google.called").is_empty()
    })
    .await;
    let _ = ACCOUNT;
}

/// The route's data answer is `private`: with a restricted profile first in
/// the mode's list, a private session's message goes to the second.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_session_is_routed_off_restricted_profiles() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let fake = Fake::start().await;
    let (r, _) = rig(Some(&jev), &fake, vec![]);
    let plain = new_session(&r.core);
    let private = new_session(&r.core);
    r.core
        .store
        .put_meta(&super::private::meta_key(&private), &"nd_test")
        .unwrap();
    let msg = "Rename the call sites in the billing module.";
    let res = run_on(&r.core, &plain, msg, None).await.unwrap();
    assert_eq!(
        res.profile, "glm53",
        "a plain session takes the first profile"
    );
    let res = run_on(&r.core, &private, msg, None).await;
    // The restricted profile is skipped: the turn runs on the second.
    let res = res.unwrap();
    assert_eq!(res.profile, "sonnet", "{}", res.output);
    assert_eq!(
        r.zai.requests().len(),
        1,
        "only the plain session reached it"
    );
}
