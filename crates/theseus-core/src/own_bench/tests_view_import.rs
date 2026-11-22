//! The replay cache keeps no copy of an imported source's words (issue 44): a view turn whose recall laid a
//! passage of an imported session keeps nothing for a replay (no `kept`, no blob holding the passage), and is
//! refused as a turn that kept nothing; a view turn whose recall is native is kept and replays as before; and
//! a kept turn whose recalled node is erased afterwards is refused.

use super::rebuild::{VIEW_HELD, VIEW_REFUSED};
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::tests_places::{turn, Rig};
use crate::tests_private_view::{index_of, owners_rig, session_in, ASKED, EPISODE};

/// Said in the imported episode.
const SAID: &str = "the imported note says the Hollowmere hives wintered in the orchard";

fn view(c: &mut crate::Config) {
    c.context.assembly = Some(AssemblySetting::All(Assembly::View));
}

/// An imported episode holding one `Body::Imported` message saying [`SAID`]: its session id.
fn episode(c: &crate::Core) -> String {
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.session_id = format!("{}{}", crate::import::SESSION_PREFIX, "c".repeat(64));
    rec.imported = Some(Box::new(
        serde_json::from_value(serde_json::json!({
            "tag": "tern-2026", "episode_id": "ep1", "hash": "h", "source": "wiki",
            "place": {"kind": "cli"}, "labels": {"sensitivity": "internal"},
            "as_of": {"start_ms": 1, "end_ms": 2}, "messages": 1, "summary": false,
            "file": "f", "line": 1, "imported_at_ms": 3
        }))
        .unwrap(),
    ));
    c.store.put_session(&rec.session_id, &rec).unwrap();
    let body = crate::node::Body::Imported {
        text: SAID.into(),
        integrity: crate::import::Integrity::Operator,
        source: "wiki".into(),
        unit: "u1".into(),
        sha256: "0".repeat(64),
        idx: 0,
    };
    let n = crate::node::Node::imported("imp_hives_1".into(), &rec.session_id, "wren", 1_000, body);
    c.store.append(&[n.record().unwrap()]).unwrap();
    rec.session_id
}

/// Every request the model was sent, once none has come for a while.
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

/// Whether any file under `dir` holds `words`.
fn on_disk(dir: &std::path::Path, words: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        match p.is_dir() {
            true => on_disk(&p, words),
            false => std::fs::read(&p)
                .is_ok_and(|b| b.windows(words.len()).any(|w| w == words.as_bytes())),
        }
    })
}

fn replay(sid: &str, t: &str, profile: String) -> theseus_protocol::own_bench::TurnReplayParams {
    theseus_protocol::own_bench::TurnReplayParams {
        session: sid.into(),
        turn: t.into(),
        profile,
        compare: Some(false),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_view_turn_that_recalled_an_import_keeps_nothing_for_a_replay() {
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let imported = episode(c);
    c.runner.memory.set_ask(index_of(c, vec![imported]));
    let sid = session_in(c, None, &r.root);
    let t = turn(c, &sid, ASKED).await.turn_id;
    let sent = settled(&r).await;
    let texts: String = (r.requests().iter())
        .map(|q| serde_json::to_string(q).unwrap())
        .collect();
    assert!(texts.contains(SAID), "the recall laid the imported passage");
    let v = crate::view::record::newest(&c.store, &t)
        .unwrap()
        .expect("a view record");
    assert!(v.kept.is_none(), "kept nothing: {:?}", v.kept);
    assert!(
        !on_disk(c.store.blobs().dir(), SAID),
        "no blob holds the passage"
    );
    let Err(e) = c.rebuild_first(&sid, &t) else {
        panic!("a turn that recalled an import was rebuilt");
    };
    assert_eq!((e.part, e.why.as_str()), ("compilation", VIEW_REFUSED));
    let own = c.live_profile().0;
    assert!(c.turn_replay(replay(&sid, &t, own), "cli").await.is_err());
    assert_eq!(r.requests().len(), sent, "no model was called");
}

#[tokio::test]
async fn a_view_turn_with_native_recall_is_kept_and_refused_once_its_recalled_node_is_erased() {
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let t = turn(c, &sid, ASKED).await.turn_id;
    let v = crate::view::record::newest(&c.store, &t)
        .unwrap()
        .expect("a view record");
    let kept = v.kept.expect("kept for a replay");
    let recalled = kept
        .recalled
        .first()
        .expect("the recall laid the episode")
        .clone();
    c.rebuild_first(&sid, &t).expect("rebuildable");
    let own = c.live_profile().0;
    let ok = c
        .turn_replay(replay(&sid, &t, own.clone()), "cli")
        .await
        .unwrap();
    assert!(!ok.replay.text.is_empty(), "the replay answered");
    // The recalled node erased afterwards (its tombstone, as an erasure writes it): refused, its own
    // provider included, and nothing is sent.
    let (_, n) = c.store.get_node(&recalled).unwrap().unwrap();
    assert!(
        matches!(&n.body, crate::node::Body::UserMessage { text, .. } if text == EPISODE),
        "{n:?}"
    );
    let gone = n.erased(theseus_protocol::now_unix_ms(), "erased in the test");
    c.store.append(&[gone.record().unwrap()]).unwrap();
    let sent = settled(&r).await;
    let Err(e) = c.rebuild_first(&sid, &t) else {
        panic!("a turn whose recalled node was erased was rebuilt");
    };
    assert_eq!((e.part, e.why.as_str()), ("view", VIEW_HELD));
    assert!(c.turn_replay(replay(&sid, &t, own), "cli").await.is_err());
    assert_eq!(r.requests().len(), sent, "no model was called");
}
