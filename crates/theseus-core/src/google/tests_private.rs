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
    rig_on(jev, fake, script, false)
}

/// [`rig`], with `zai`'s `glm53` a Claude served from a host vouched EU when
/// `eu`: of no restricted origin, so only the floor for private content
/// keeps a turn off it (theseus-a1wvq.3 keeps every request off GLM itself).
fn rig_on(
    jev: Option<&FakeJev>,
    fake: &Fake,
    script: Vec<Scripted>,
    eu: bool,
) -> (RouteRig, Arc<FakeProvider>) {
    rig_tweaked(jev, fake, script, eu, |_| {})
}

/// [`rig_on`], with `tweak` applied to its config last.
pub(super) fn rig_tweaked(
    jev: Option<&FakeJev>,
    fake: &Fake,
    script: Vec<Scripted>,
    eu: bool,
    tweak: impl FnOnce(&mut crate::Config) + 'static,
) -> (RouteRig, Arc<FakeProvider>) {
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
            if eu {
                c.serve_glm_unrestricted();
                c.vouch_zai(crate::catalog::ModelOrigin::Eu);
            }
            tweak(c);
        },
        move |p| {
            // After the load's check, which refuses a restricted profile in a
            // mode's list (theseus-a1wvq.4): the floor holds on one anyway.
            p.cfg.routing.classes.routine_coding.profiles = vec!["glm53".into(), "sonnet".into()];
            p.secrets = board();
            p.providers.insert("anthropic".into(), c2);
        },
    );
    (r, claude)
}

pub(super) async fn run_on(
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

pub(super) fn new_session(core: &crate::Core) -> String {
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

/// The route's data answer is `private`: with a profile not of US origin
/// (an EU one) first in the mode's list, a private session's message goes to
/// the second, though a plain session's takes the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_session_is_routed_off_restricted_profiles() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let fake = Fake::start().await;
    let (r, _) = rig_on(Some(&jev), &fake, vec![], true);
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

/// A turn whose recall admits a note of a private session is private in
/// that same turn (theseus-39dj0), before its frame writes the mark: the
/// route skips the EU profile first in the mode's list and runs on the US
/// one, as a marked session's does. A plain note's turn takes the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_recalls_a_private_note_is_routed_off_restricted_profiles() {
    for marked in [false, true] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "routine_coding", 0.95);
        let fake = Fake::start().await;
        let (r, _) = rig_tweaked(Some(&jev), &fake, vec![], true, |c| {
            c.memory.mode = crate::config::MemoryMode::Canary;
            c.memory.canary_fraction = 1.0;
        });
        let note = "Remember: the billing module's call sites live in ledger.rs.";
        let a = crate::tests_recall::session(&r.core, None, &[note]);
        if marked {
            r.core
                .store
                .put_meta(&super::private::meta_key(&a), &"nd_test")
                .unwrap();
        }
        let b = new_session(&r.core);
        r.core
            .runner
            .memory
            .set_ask(crate::tests_recall::index_of(&r.core, vec![a]));
        let msg = "Rename the call sites in the billing module.";
        let res = run_on(&r.core, &b, msg, None).await.unwrap();
        assert_eq!(res.recalled, 1, "marked={marked}");
        let want = if marked { "sonnet" } else { "glm53" };
        assert_eq!(res.profile, want, "marked={marked}: {}", res.output);
        assert_eq!(
            super::private::session(&r.core.store, &r.core.cfg, &b),
            marked
        );
    }
}

/// What the Operations page lists (theseus-jid7t.2): a session is `open` until
/// it reads a Google result, and `private` after, in `session.list`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_google_read_shows_as_the_sessions_data_mark() {
    use theseus_protocol::ops::DataMark;
    let fake = Fake::start().await;
    fake.json(
        "GET",
        "/drive/v3/files",
        json!({"files": [{"id": "f1", "name": "Quarterly plan"}]}),
    );
    let (r, _) = rig(
        None,
        &fake,
        vec![
            call("c1", json!({"method": "drive.files.list"})),
            Scripted::text("Done."),
        ],
    );
    let core = r.core.clone();
    let mark = |sid: String| {
        let core = core.clone();
        async move {
            let all: theseus_protocol::SessionListResult = serde_json::from_value(
                crate::tests_route::call(&core, "session.list", json!({})).await,
            )
            .unwrap();
            all.sessions
                .into_iter()
                .find(|s| s.session_id == sid)
                .unwrap()
                .data
        }
    };
    let sid = new_session(&r.core);
    assert_eq!(mark(sid.clone()).await, Some(DataMark::Open));
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    assert_eq!(mark(sid).await, Some(DataMark::Private));
}
