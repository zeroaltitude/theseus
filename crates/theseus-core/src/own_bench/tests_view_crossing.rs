//! A view turn's replay to another provider reads its sources from what its view record named, read once
//! with the rebuild (`Original::laid`), and fails closed (issue 44): a turn whose recall laid a real
//! imported item labelled `personal` does not cross; nor one whose provenance, a recalled node, or a laid
//! session's record cannot be read; the same provider, and the same item no longer veiled, still cross.

use crate::tests_places::{turn, Rig};
use crate::tests_private_view::{index_of, owners_rig, session_in, ASKED};

/// Said in the imported episode.
const SAID: &str = "the imported note says the Hollowmere hives wintered in the orchard";

/// An imported episode labelled `sensitivity`, holding one `Body::Imported` message: its session id.
fn episode(c: &crate::Core, sensitivity: &str) -> String {
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.session_id = format!("{}{}", crate::import::SESSION_PREFIX, "b".repeat(64));
    rec.imported = Some(Box::new(labelled(sensitivity)));
    c.store.put_session(&rec.session_id, &rec).unwrap();
    let body = crate::node::Body::Imported {
        text: SAID.into(),
        integrity: crate::import::Integrity::Operator,
        source: "wiki".into(),
        unit: "u1".into(),
        sha256: "0".repeat(64),
        idx: 0,
    };
    let n = crate::node::Node::imported("imp_hives_0".into(), &rec.session_id, "wren", 1_000, body);
    c.store.append(&[n.record().unwrap()]).unwrap();
    rec.session_id
}

fn labelled(sensitivity: &str) -> crate::import::ImportedFrom {
    serde_json::from_value(serde_json::json!({
        "tag": "tern-2026", "episode_id": "ep1", "hash": "h", "source": "wiki",
        "place": {"kind": "cli"}, "labels": {"sensitivity": sensitivity},
        "as_of": {"start_ms": 1, "end_ms": 2}, "messages": 1, "summary": false,
        "file": "f", "line": 1, "imported_at_ms": 3
    }))
    .unwrap()
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

#[tokio::test]
async fn a_view_turn_that_laid_a_veiled_import_does_not_cross_and_fails_closed() {
    let (r, _) = owners_rig(|c| {
        c.context.assembly = Some(crate::config::assembly::AssemblySetting::All(
            crate::config::assembly::Assembly::View,
        ))
    });
    let c = &r.core;
    let imported = episode(c, "personal");
    c.runner.memory.set_ask(index_of(c, vec![imported.clone()]));
    let sid = session_in(c, None, &r.root);
    let t = turn(c, &sid, ASKED).await.turn_id;
    let sent = settled(&r).await;
    let original = c.rebuild_first(&sid, &t).expect("rebuildable");
    let laid = original.laid.clone().expect("a view turn's provenance");
    assert!(
        !laid.recalled.is_empty(),
        "the recall laid the imported item: {laid:?}"
    );
    let own = original.compilation.manifest.provider.clone();
    let other = format!("{own}-elsewhere");
    // Veiled: refused to another provider, with the rule's words; the same provider sends nothing new.
    assert_eq!(
        c.crossing(&original, &other).as_deref(),
        Some("its recall carries a personal source")
    );
    assert_eq!(c.crossing(&original, &own), None, "the same provider");
    // Its provenance not read: refused.
    let mut none = c.rebuild_first(&sid, &t).unwrap();
    none.laid = None;
    assert_eq!(
        c.crossing(&none, &other).as_deref(),
        Some("what its view laid cannot be read")
    );
    // A recalled node that cannot be read: refused, never skipped.
    let mut gone = c.rebuild_first(&sid, &t).unwrap();
    gone.laid.as_mut().unwrap().recalled = vec!["nod_not_there".into()];
    assert_eq!(
        c.crossing(&gone, &other).as_deref(),
        Some("a node its recall laid cannot be read")
    );
    // The item's session record (its labels) that does not read: refused.
    c.store
        .put_session(&imported, &serde_json::json!({"not": "a session"}))
        .unwrap();
    assert_eq!(
        c.crossing(&original, &other).as_deref(),
        Some("a source its view laid cannot be read")
    );
    // The same item no longer veiled crosses.
    let mut open =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    open.session_id.clone_from(&imported);
    open.imported = Some(Box::new(labelled("internal")));
    c.store.put_session(&imported, &open).unwrap();
    assert_eq!(c.crossing(&original, &other), None, "not veiled");
    // A view record that does not read: the rebuild itself refuses.
    let key = format!(
        "{}{}",
        crate::view::record::VIEW_PREFIX,
        original.compilation.id
    );
    c.store
        .put_meta(&key, &serde_json::json!(["not a record"]))
        .unwrap();
    let Err(e) = c.rebuild_first(&sid, &t) else {
        panic!("rebuilt from a record that does not read");
    };
    assert_eq!(e.part, "view", "{e}");
    assert_eq!(r.requests().len(), sent, "no model was called");
}
