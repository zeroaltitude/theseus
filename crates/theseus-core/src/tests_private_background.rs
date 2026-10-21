//! The Google "private" mark around the background provider calls (the gw-core
//! review's F3, theseus-a1wvq.10): a session that read a Google result is
//! private, and the floor refuses any request that holds its content to a
//! model that is not of US origin. The turn and compaction set the mark; these
//! tests are for the rest: keep-warm, the memory pass (and the finding said
//! from it), the people judge, the ontology's naming call, the census's exact
//! count, and a task a private session starts.
//!
//! The rigs serve the provider from a host vouched EU, so no model is of a
//! restricted origin and only the private mark can refuse a request. Each test
//! has its negative: a session that read nothing is sent to the same model.

use std::collections::HashSet;

use theseus_protocol::census::ContextCensusParams;
use theseus_protocol::task_start::TaskStartParams;
use theseus_protocol::SessionKind;

use crate::approval::{Answerer, Surface};
use crate::config::{GoogleConfig, GoogleCredentialNames};
use crate::google::private;
use crate::judge::people::run::Session;
use crate::judge::people::Line;
use crate::keep_warm::Outcome;
use crate::provider::Scripted;
use crate::tests_keep_warm::{build, one_hour, reads, session, turn};
use crate::{Config, Core};

pub(crate) const ACCOUNT: &str = "owner@example.test";

/// `[google]` bound to an account (nothing is ever called: the mark is read
/// only when the config binds one), and the default provider served from a
/// host vouched EU.
pub(crate) fn private_on(c: &mut Config) {
    c.serve_default_from_eu();
    c.google = GoogleConfig {
        account: ACCOUNT.into(),
        credentials: GoogleCredentialNames::default(),
        ..Default::default()
    };
    c.secrets.insert(
        "google_client".into(),
        "op://vault/google client/notesPlain".into(),
    );
    c.secrets.insert(
        "google_refresh_token".into(),
        "file:/tmp/unused-google-refresh-token".into(),
    );
}

/// The session has read a Google result: its mark, as the frame that writes
/// the result writes it.
pub(crate) fn mark(core: &Core, sid: &str) {
    core.store
        .put_meta(&private::meta_key(sid), &"nd_test")
        .unwrap();
}

fn private_now(core: &Core, sid: &str) -> bool {
    private::session(&core.store, &core.cfg, sid)
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A keep-warm read sends a session's own prefix, its Google content
/// included, to its last target: for a private session it is refused before a
/// byte is sent, and a session that read nothing is read as before.
#[tokio::test]
async fn a_keep_warm_read_of_a_private_session_is_refused_by_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    private_on(&mut c);
    let (core, fake) = build(dir.path(), c);
    let (private, plain) = (
        session(&core, SessionKind::Conversation, None),
        session(&core, SessionKind::Conversation, None),
    );
    turn(&core, &private, "the first conversation").await;
    turn(&core, &plain, "the second conversation").await;
    mark(&core, &private);
    let (live, _) = core.live_profile();

    let out = core.runner.keep_warm_once(&private, &live).await;
    assert!(matches!(out, Outcome::Stopped(_)), "{out:?}");
    assert!(
        reads(&fake).is_empty(),
        "nothing was sent: {:?}",
        reads(&fake)
    );

    let out = core.runner.keep_warm_once(&plain, &live).await;
    assert!(matches!(out, Outcome::Read { .. }), "{out:?}");
    assert_eq!(reads(&fake).len(), 1);
}

/// A task a private session starts is private too: the start writes its mark
/// in the frame that opens it, so its own turns, and every background step
/// over it, are held by the floor from the first. A task of a plain session
/// is not.
#[tokio::test]
async fn a_task_a_private_session_starts_is_private() {
    let r = crate::tests_tasks::rig(|_| Scripted::text("ok"), private_on);
    let home = r.core.home().unwrap().session_id;
    let start = |brief: &str| TaskStartParams {
        brief: brief.into(),
        ..Default::default()
    };
    let plain = r.core.task_start(&start("Count the boats"), cli()).unwrap();
    assert!(
        !private_now(&r.core, &plain.session_id),
        "a plain parent's task is plain"
    );
    mark(&r.core, &home);
    let child = r.core.task_start(&start("Count the quays"), cli()).unwrap();
    assert!(
        private_now(&r.core, &child.session_id),
        "the task of a private session reads as private"
    );
    assert!(
        !private_now(&r.core, &plain.session_id),
        "an earlier task is as it was"
    );
}

/// The ontology's naming call sends what a session said: refused for a
/// private one.
#[tokio::test]
async fn the_naming_call_of_a_private_session_is_refused_by_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    private_on(&mut c);
    let (core, fake) = build(dir.path(), c);
    let (private, plain) = (
        session(&core, SessionKind::Conversation, None),
        session(&core, SessionKind::Conversation, None),
    );
    mark(&core, &private);
    let said = "The harbourmaster is Ana of the north quay.";
    let _ = core.onto_name(&private, "person", said).await;
    assert!(fake.requests().is_empty(), "nothing was sent");
    let _ = core.onto_name(&plain, "person", said).await;
    assert_eq!(fake.requests().len(), 1, "a plain session's is sent");
}

fn lines() -> Vec<Line> {
    vec![Line {
        node: "nd_1".into(),
        author: "owner".into(),
        text: "Ana of the north quay keeps the tide tables.".into(),
    }]
}

/// The people judge's extraction sends a session's lines: refused for a
/// private session, which then holds no candidate.
#[tokio::test]
async fn the_people_extraction_of_a_private_session_is_refused_by_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    private_on(&mut c);
    let (core, fake) = build(dir.path(), c);
    let (private, plain) = (
        session(&core, SessionKind::Conversation, None),
        session(&core, SessionKind::Conversation, None),
    );
    mark(&core, &private);
    let pass = |sid: &str| Session {
        sid: sid.to_string().leak(),
        title: "Harbour",
        lines: lines(),
        purpose: "live",
        mark: None,
        known: HashSet::new(),
    };
    let done = core.propose_people(pass(&private)).await.unwrap();
    assert!(done.failed, "a refused request is a failed pass");
    assert!(fake.requests().is_empty(), "nothing was sent");
    core.propose_people(pass(&plain)).await.unwrap();
    assert_eq!(fake.requests().len(), 1, "a plain session's is sent");
}

/// `context.census --exact` sends a session's whole request to the provider's
/// counter: refused for a private session.
#[tokio::test]
async fn the_exact_census_of_a_private_session_is_refused_by_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    private_on(&mut c);
    let (core, _fake) = build(dir.path(), c);
    let (private, plain) = (
        session(&core, SessionKind::Conversation, None),
        session(&core, SessionKind::Conversation, None),
    );
    turn(&core, &private, "the first conversation").await;
    turn(&core, &plain, "the second conversation").await;
    mark(&core, &private);
    let census = |sid: &str| ContextCensusParams {
        session_id: Some(sid.to_string()),
        exact: true,
        ..ContextCensusParams::default()
    };
    let out = core.context_census_exact(census(&private)).await.unwrap();
    assert!(out.exact.is_none(), "{:?}", out.exact);
    assert!(out.exact_error.is_some(), "the counter refused it");
    let out = core.context_census_exact(census(&plain)).await.unwrap();
    assert!(out.exact.is_some(), "{:?}", out.exact_error);
}

/// One request over several sessions is private when any one of them is, not
/// when the first is: the memory pass, the learning writer, a card and a
/// thread import each hold content from many.
#[tokio::test]
async fn a_request_over_several_sessions_is_private_when_any_one_is() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    private_on(&mut c);
    let (core, _fake) = build(dir.path(), c);
    let ids: Vec<String> = (0..3)
        .map(|_| session(&core, SessionKind::Conversation, None))
        .collect();
    let any = |ids: &[String]| private::any_session(&core.store, &core.cfg, ids);
    assert!(!any(&ids), "none read anything");
    mark(&core, &ids[2]);
    assert!(any(&ids), "the last one did");
    assert!(any(&ids[2..]));
    assert!(!any(&ids[..2]), "the others did not");
    assert!(!any(&[]), "no session, no mark");
}

/// A bound provider holds its mark whatever task makes the call and however
/// late: the call is the one the floor sees, as the turn's is.
#[tokio::test]
async fn a_bound_provider_is_held_by_the_floor_on_any_task() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    private_on(&mut c);
    let (core, fake) = build(dir.path(), c);
    let provider = core.runner.providers.get("anthropic").cloned().unwrap();
    let request = crate::provider::ProviderRequest {
        model: "claude-sonnet-5-5".into(),
        max_tokens: 16,
        system: vec![],
        messages: vec![serde_json::json!({"role": "user", "content": "Hello."})],
        tools: vec![],
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: vec![],
        extra: Default::default(),
        image_tokens: 0,
        data_pass: None,
    };
    let send = |private: bool| {
        let (p, r) = (provider.clone(), request.clone());
        tokio::spawn(async move {
            let p = private::bound(p, Some("ses_x"), private);
            let mut quiet = |_: crate::provider::Delta<'_>| {};
            p.stream_message(&r, &mut quiet).await.map(drop)
        })
    };
    assert!(send(true).await.unwrap().is_err(), "a private call");
    assert!(fake.requests().is_empty(), "nothing was sent");
    send(false).await.unwrap().unwrap();
    assert_eq!(fake.requests().len(), 1, "a plain call goes");
}
