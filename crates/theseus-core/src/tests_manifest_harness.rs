//! A session's thread and project in the compile's manifest (theseus-w0vyp): what the compile already reads (the
//! thread's card and topics, the project's slice) is also named in the manifest's `memberships`, as the channel is,
//! so "why did it know that?" has an answer. Nothing is stored, and no request byte changes: they add no section, no
//! guidance and no skipped entry.

use theseus_ontology::Origin;
use theseus_protocol::threads::ThreadNewParams;
use theseus_protocol::OntologyCategoryAddParams;

use crate::approval::{Answerer, Surface};
use crate::ontology::{recorded, Walk};
use crate::session::SessionRecord;
use crate::tests_day_ceiling::{build, turn_in};
use theseus_protocol::PlaceClass;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A thread's session, attached to a project: the harness's own facts.
#[tokio::test]
async fn the_manifest_records_the_sessions_thread_and_project() {
    let dir = tempfile::tempdir().unwrap();
    let texts = (0..2)
        .map(|i| crate::provider::Scripted::text(&format!("noted {i}")))
        .collect();
    let (core, _fake) = build(dir.path(), texts, |_| {});
    // The ontology holds a category, so the compile has a walk to record in.
    core.ontology_category_add(
        &OntologyCategoryAddParams {
            name: "harbor".into(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    core.thread_new(
        &ThreadNewParams {
            slug: "tide-gauges".into(),
            title: Some("Tide gauges".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let sid = core
        .thread_session("tide-gauges", cli())
        .unwrap()
        .session_id;
    let mut rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    rec.project = Some("tsk_harborsurvey".into());
    core.store.put_session(&sid, &rec).unwrap();
    turn_in(&core, &sid, "where do the gauges stand?")
        .await
        .unwrap();
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let manifest = core
        .store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
        .manifest;
    let named: Vec<(&str, &str, Origin)> = manifest
        .memberships
        .iter()
        .map(|m| (m.kind.as_str(), m.category.as_str(), m.origin))
        .collect();
    assert_eq!(
        named,
        [
            ("thread", "thread:tide-gauges", Origin::Transport),
            ("project", "project:tsk_harborsurvey", Origin::Transport)
        ],
        "{named:?}"
    );
    assert!(manifest.memberships.iter().all(|m| m.confidence.is_none()));
    assert!(manifest.guidance.is_empty(), "they carry no guidance");

    // The bytes: the same walk without them composes the same sections, guidance and skips.
    let walk = core.runner.walk(&sid, PlaceClass::Private).unwrap();
    let bare = Walk::of(
        walk.snapshot.clone(),
        &sid,
        core.runner.target_of(&sid).as_deref(),
        PlaceClass::Private,
        theseus_protocol::now_unix_ms(),
    )
    .unwrap();
    assert!(bare.harness.is_empty());
    let (with, without) = (walk.compose(&walk.current), bare.compose(&bare.current));
    assert_eq!(with.render(), without.render());
    assert_eq!(with.sections, without.sections);
    assert_eq!(with.guidance, without.guidance);
    assert_eq!(with.skipped, without.skipped);
    assert!(with.skipped.is_empty());
    assert_eq!(with.memberships.len(), without.memberships.len() + 2);

    // An append composes what the manifest recorded: each once, no skip.
    let again = walk.compose(&recorded(&manifest.memberships));
    assert!(again.skipped.is_empty(), "{:?}", again.skipped);
    assert_eq!(
        again
            .memberships
            .iter()
            .filter(|m| m.kind == "thread" || m.kind == "project")
            .count(),
        2
    );
    assert_eq!(again.render(), without.render());
}

/// A session in no thread and on no project names neither, and a shared place's walk records them too.
#[tokio::test]
async fn a_plain_session_names_neither_and_a_shared_place_names_them() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _fake) = build(dir.path(), Vec::new(), |_| {});
    core.ontology_category_add(
        &OntologyCategoryAddParams {
            name: "harbor".into(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    let plain = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&plain.session_id, &plain).unwrap();
    let walk = core
        .runner
        .walk(&plain.session_id, PlaceClass::Private)
        .unwrap();
    assert!(walk.harness.is_empty());
    assert!(walk.compose(&walk.current).memberships.is_empty());
    let mut attached = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    attached.project = Some("tsk_dockyard".into());
    core.store
        .put_session(&attached.session_id, &attached)
        .unwrap();
    let shared = core
        .runner
        .walk(&attached.session_id, PlaceClass::Shared)
        .unwrap();
    let used = shared.compose(&shared.current);
    assert_eq!(used.memberships.len(), 1);
    assert_eq!(
        used.memberships[0].category.as_str(),
        "project:tsk_dockyard"
    );
    assert!(used.skipped.is_empty());
}
