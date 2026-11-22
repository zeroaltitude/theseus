//! A turn built from a view is replayed on any provider's model (anything goes, 2026-11-22): `turn.replay`
//! of it to a profile on another provider sends it there, and a profile of its own provider replays it too.

use std::sync::Arc;

use theseus_protocol::own_bench::TurnReplayParams;

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
async fn a_view_turn_is_replayed_on_another_providers_model() {
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
    settled(&r).await;
    let other = r.core.turn_replay(p("glm"), "cli").await.unwrap();
    assert_eq!(other.profile, "glm");
    assert_eq!(other.turn, t);
    assert_eq!(zai.requests().len(), 1, "the other provider was sent it");
    let ok = r.core.turn_replay(p("haiku"), "cli").await.unwrap();
    assert_eq!(ok.profile, "haiku");
    assert!(!ok.replay.text.is_empty(), "the replay answered");
}
