//! A replay to another provider is a new disclosure of the whole request (issue 44): once anything was sealed,
//! moved into a thread or withdrawn after the turn began, anywhere in the store, a view turn's crossing is
//! refused in words, and no model is called; the turn still replays to its own provider. Each event is a real
//! one (the thread's seal, a split, a release taken back); a reader that cannot be read refuses too.

use theseus_protocol::own_bench::TurnReplayParams;
use theseus_protocol::threads::{ThreadNewParams, ThreadSealParams, ThreadSplitParams};

use super::replay::{CHANGED, CHANGED_UNREAD};
use crate::tests_places::Rig;
use crate::tests_view_compile::turn_as;

fn cli() -> crate::approval::Answerer {
    crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A view rig with threads, an unrelated thread `kelp`, and in another conversation an exchange of the owner's
/// (`spare`, which a split or a release can take); then a view turn of the desk. The rig, the desk, the turn,
/// the spare session and its message's node.
async fn viewed() -> (Rig, String, String, String, String) {
    let r = crate::tests_places::rig_setup(
        |c| {
            c.threads.enabled = true;
        },
        |_| {},
    );
    let c = &r.core;
    let new = ThreadNewParams {
        slug: "kelp".into(),
        title: Some("Kelp".into()),
        ..Default::default()
    };
    c.thread_new(&new, cli()).unwrap();
    let spare = crate::tests_places::session(c, Some("dm:42"));
    turn_as(c, &spare, "the offsite is on Friday", true).await;
    let said = (c.store.transcript(&spare).unwrap().into_iter())
        .find(|(_, n)| {
            matches!(&n.body, crate::node::Body::UserMessage { text, .. }
                if text.contains("the offsite is on Friday"))
        })
        .map(|(_, n)| n.id)
        .expect("the spare message");
    let desk = crate::tests_places::session(c, None);
    let t = turn_as(c, &desk, "desk: which hives wintered?", true)
        .await
        .turn_id;
    (r, desk, t, spare, said)
}

/// Every request the model was sent, once none has come for a while (a view place's ladder writer calls
/// after a turn, in the background).
async fn settled(r: &Rig) -> usize {
    let mut seen = r.requests().len();
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if r.requests().len() == seen {
            return seen;
        }
        seen = r.requests().len();
    }
    seen
}

/// The crossing of `t` to another provider says `want`, and no model is called; the replay to the turn's own
/// provider still answers.
async fn crossing_says(r: &Rig, (desk, t): (&str, &str), want: Option<&str>) {
    let c = &r.core;
    let sent = settled(r).await;
    let original = c.rebuild_first(desk, t).expect("rebuildable");
    let own = original.compilation.manifest.provider.clone();
    assert_eq!(
        c.crossing(&original, &format!("{own}-elsewhere"))
            .as_deref(),
        want
    );
    assert_eq!(c.crossing(&original, &own), None, "its own provider");
    assert_eq!(r.requests().len(), sent, "no model was called");
    if want.is_some() {
        let p = TurnReplayParams {
            session: desk.into(),
            turn: t.into(),
            profile: c.live_profile().0,
            effort: None,
            scope: None,
            compare: Some(false),
        };
        let res = c
            .turn_replay(p, "cli")
            .await
            .expect("replayed to its own provider");
        assert!(!res.replay.text.is_empty(), "the replay answered");
    }
}

/// Clear before, refused once an unrelated thread is sealed after the turn.
#[tokio::test]
async fn a_seal_after_the_turn_keeps_its_replay_with_its_own_provider() {
    let (r, desk, t, _, _) = viewed().await;
    crossing_says(&r, (&desk, &t), None).await;
    let seal = ThreadSealParams {
        slug: "kelp".into(),
        reason: Some("embargoed".into()),
        ..Default::default()
    };
    r.core.thread_seal(&seal, cli()).unwrap();
    crossing_says(&r, (&desk, &t), Some(CHANGED)).await;
}

/// Refused once a message of another conversation is split out into a thread after the turn.
#[tokio::test]
async fn a_move_after_the_turn_keeps_its_replay_with_its_own_provider() {
    let (r, desk, t, spare, said) = viewed().await;
    crossing_says(&r, (&desk, &t), None).await;
    // A turn moves whole: from its first node to its last.
    let nodes = r.core.store.transcript(&spare).unwrap();
    let turn = (nodes.iter())
        .find(|(_, n)| n.id == said)
        .and_then(|(_, n)| n.turn_id.clone())
        .unwrap();
    let at: Vec<u64> = (nodes.iter())
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn.as_str()))
        .map(|(p, _)| *p)
        .collect();
    let split = ThreadSplitParams {
        from: spare.clone(),
        slug: "offsite".into(),
        range: Some([at[0], *at.last().unwrap()]),
        ..Default::default()
    };
    r.core
        .thread_split(&split, cli())
        .map_err(|e| e.message)
        .unwrap();
    crossing_says(&r, (&desk, &t), Some(CHANGED)).await;
}

/// Refused once a release made before the turn is withdrawn after it.
#[tokio::test]
async fn a_release_withdrawn_after_the_turn_keeps_its_replay_with_its_own_provider() {
    let r = crate::tests_places::rig_setup(|_| {}, |_| {});
    let c = &r.core;
    let spare = crate::tests_places::session(c, None);
    turn_as(c, &spare, "the offsite is on Friday", true).await;
    let said = (c.store.transcript(&spare).unwrap().into_iter())
        .find(|(_, n)| {
            matches!(&n.body, crate::node::Body::UserMessage { text, .. }
                if text == "the offsite is on Friday")
        })
        .map(|(_, n)| n.id)
        .expect("the spare message");
    let digest = crate::release::digest("the offsite is on Friday");
    let rel = crate::release::grant(c, &said, &digest, "discord:channel:7", "cli").unwrap();
    let desk = crate::tests_places::session(c, None);
    let t = turn_as(c, &desk, "desk: which hives wintered?", true)
        .await
        .turn_id;
    crossing_says(&r, (&desk, &t), None).await;
    crate::release::withdraw(&c.store, &rel.id, "cli").unwrap();
    crossing_says(&r, (&desk, &t), Some(CHANGED)).await;
}

/// A reader that cannot be read (a release record that does not decode) refuses the crossing.
#[tokio::test]
async fn a_reader_that_cannot_be_read_keeps_the_replay_with_its_own_provider() {
    let (r, desk, t, _, _) = viewed().await;
    crossing_says(&r, (&desk, &t), None).await;
    (r.core.store)
        .put_meta(
            "release.id.rel_unreadable",
            &serde_json::json!(["not a release"]),
        )
        .unwrap();
    crossing_says(&r, (&desk, &t), Some(CHANGED_UNREAD)).await;
}
