//! A turn built from a view is replayed only with its own provider (issue 44): `turn.replay` of it to a
//! profile on another provider is refused in words and nothing reaches either provider, while a profile of
//! its own provider still replays it. (A stored turn of the old kind keeps main's rules for another
//! provider, unchanged: `tests_sampler::an_attachment_is_never_sent_to_another_provider`.)

use std::sync::Arc;

use theseus_protocol::own_bench::TurnReplayParams;

use super::replay::{Crossing, VIEW_HOME};
use super::tests::script;
use crate::provider::FakeProvider;
use crate::tests_recall::{session, turn, Rig};

/// A view rig whose `glm` profile has a provider of its own (`tests_sampler::two_providers`'s, with the view).
fn two_providers() -> (Rig, Arc<FakeProvider>) {
    let zai = Arc::new(FakeProvider::default());
    let z = zai.clone();
    let r = crate::tests_recall::rig_built(
        crate::config::MemoryMode::Off,
        |c| {
            c.serve_glm_as("claude-sonnet-5");
            c.catalog.insert(
                "claude-sonnet-5".into(),
                crate::catalog::CatalogRow {
                    input_per_mtok: Some(1.0),
                    output_per_mtok: Some(5.0),
                    ..Default::default()
                },
            );
        },
        move |p| {
            p.providers.insert("zai".into(), z);
        },
    );
    (r, zai)
}

/// Every request the model was sent, once none has come for a while (a view place's ladder writer calls
/// after a turn, in the background).
async fn settled(r: &Rig) -> usize {
    let mut seen = r.model.requests().len();
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if r.model.requests().len() == seen {
            return seen;
        }
        seen = r.model.requests().len();
    }
    seen
}

#[tokio::test]
async fn a_view_turn_is_replayed_only_with_its_own_provider() {
    let (r, zai) = two_providers();
    script(
        &r,
        &["The kestrel relay listens on port 7714.", "Port 7714."],
    );
    let sid = session(&r.core, None, &[]);
    let t = turn(&r.core, &sid, "Which port does the kestrel relay use?")
        .await
        .turn_id;
    let p = |profile: &str| TurnReplayParams {
        session: sid.clone(),
        turn: t.clone(),
        profile: profile.into(),
        compare: Some(false),
        ..Default::default()
    };
    let sent = settled(&r).await;
    let err = r.core.turn_replay(p("glm"), "cli").await.unwrap_err();
    assert!(err.downcast_ref::<Crossing>().is_some(), "{err:#}");
    assert!(format!("{err:#}").contains(VIEW_HOME), "{err:#}");
    assert!(
        zai.requests().is_empty(),
        "nothing reached the other provider"
    );
    assert_eq!(r.model.requests().len(), sent, "nor its own");
    // A profile of its own provider still replays it.
    let ok = r.core.turn_replay(p("haiku"), "cli").await.unwrap();
    assert!(!ok.replay.text.is_empty(), "the replay answered");
}
