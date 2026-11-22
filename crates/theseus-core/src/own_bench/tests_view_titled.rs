//! A view record kept before session titles were removed from recalled headers is not replayed (issue 44):
//! its beside block's `session <short id> "<title>"` would send the title again, whatever was sealed since. A
//! kept beside block of that shape is refused in words with no model call; a record without it replays.

use theseus_protocol::own_bench::TurnReplayParams;

use super::rebuild::VIEW_TITLED;
use super::tests::{board, script};
use crate::compiler::view::kept::titled;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::config::MemoryMode;
use crate::tests_recall::{session, turn, Rig};

fn view_rig() -> Rig {
    crate::tests_recall::rig_with_secrets(MemoryMode::Off, board(), |c| {
        c.context.assembly = Some(AssemblySetting::All(Assembly::View));
    })
}

/// Every request the model was sent, once none has come for a while.
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

fn replay(sid: &str, t: &str, profile: &str) -> TurnReplayParams {
    TurnReplayParams {
        session: sid.into(),
        turn: t.into(),
        profile: profile.into(),
        compare: Some(false),
        ..Default::default()
    }
}

#[test]
fn the_old_header_shape_is_told_from_the_new() {
    assert!(titled(
        "- session cd34ef \"Staging database setup\", 2026-09-30"
    ));
    assert!(titled("session a1 \"x\""));
    for new in [
        "- session cd34ef, 2026-09-30",
        "the session \"kestrel\" was long",
        "session cd34ef7 \"seven characters is no short id\"",
        "session  \"no id\"",
    ] {
        assert!(!titled(new), "{new}");
    }
}

#[tokio::test]
async fn a_view_record_kept_with_a_titled_header_is_not_replayed() {
    let r = view_rig();
    script(
        &r,
        &["The kestrel relay listens on port 7714.", "Port 7714."],
    );
    let sid = session(&r.core, None, &[]);
    let t = turn(&r.core, &sid, "Which port does the kestrel relay use?")
        .await
        .turn_id;
    // Without the old shape: rebuilt, and replayed as before.
    r.core.rebuild_first(&sid, &t).expect("rebuildable");
    let ok = r
        .core
        .turn_replay(replay(&sid, &t, "haiku"), "cli")
        .await
        .unwrap();
    assert!(!ok.replay.text.is_empty(), "the replay answered");
    // As a record kept before the titles went: its beside block holds a recalled header with the title.
    let store = &r.core.store;
    let old =
        "Recalled\n- session cd34ef \"Staging database setup for the tide service\": the port";
    let digest = store.blobs().put(old.as_bytes()).unwrap();
    let mut v = crate::view::record::newest(store, &t).unwrap().unwrap();
    v.kept.as_mut().unwrap().beside = Some(digest);
    store.put_meta(&v.key(), &v).unwrap();
    let sent = settled(&r).await;
    let Err(e) = r.core.rebuild_first(&sid, &t) else {
        panic!("a record with a titled header was rebuilt");
    };
    assert_eq!((e.part, e.why.as_str()), ("view", VIEW_TITLED));
    assert!(r
        .core
        .turn_replay(replay(&sid, &t, "haiku"), "cli")
        .await
        .is_err());
    assert_eq!(r.model.requests().len(), sent, "no model was called");
}
