//! own-benchmark refuses a view turn it cannot replay as it was, in words and before any model is called
//! (issue 44): a blob of its front, window or beside block missing or not what its name says, each named; a
//! record durable while none of its blobs were written (a turn cancelled or crashed before its batch
//! landed). A message it laid moved since into a thread no longer refuses it (anything goes, 2026-11-22).

use theseus_protocol::own_bench::TurnReplayParams;

use super::rebuild::NotRebuildable;
use super::tests::{board, script};
use crate::compiler::view::kept::Kept;
use crate::config::MemoryMode;
use crate::tests_recall::{session, turn, Rig};
use crate::Core;

fn view_rig() -> Rig {
    crate::tests_recall::rig_with_secrets(MemoryMode::Off, board(), |_| {})
}

/// Two turns of one conversation: the rig, the session and the second turn (its window holds the first).
async fn two_turns() -> (Rig, String, String) {
    let rig = view_rig();
    script(&rig, &["Port 7714.", "Still 7714."]);
    let sid = session(&rig.core, None, &[]);
    turn(&rig.core, &sid, "Which port does the kestrel relay use?").await;
    let t = turn(&rig.core, &sid, "And now?").await.turn_id;
    (rig, sid, t)
}

/// The record of `turn`'s view, changed by `edit`.
fn edit_kept(core: &Core, turn: &str, edit: impl FnOnce(&mut Kept)) {
    let mut v = crate::view::record::newest(&core.store, turn)
        .unwrap()
        .unwrap();
    edit(v.kept.as_mut().expect("kept"));
    core.store.put_meta(&v.key(), &v).unwrap();
}

fn kept_of(core: &Core, turn: &str) -> Kept {
    let v = crate::view::record::newest(&core.store, turn).unwrap();
    v.expect("a view record").kept.expect("kept for a replay")
}

/// Blob `digest` gone, or holding bytes that are not its name's.
fn spoil(core: &Core, digest: &str, corrupt: bool) {
    let path = core.store.blobs().path(digest);
    let _ = std::fs::remove_file(&path);
    if corrupt {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not what its name says").unwrap();
    }
}

/// Every request a model was sent, once none has come for a while (a view place's ladder writer calls
/// after a turn, in the background).
async fn settled(requests: impl Fn() -> usize) -> usize {
    let mut seen = requests();
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if requests() == seen {
            return seen;
        }
        seen = requests();
    }
    seen
}

/// `turn` of `sid` refused by the rebuild, and by `turn.replay` with no model called of any kind.
async fn refused(
    core: &std::sync::Arc<Core>,
    requests: impl Fn() -> usize,
    sid: &str,
    turn: &str,
) -> NotRebuildable {
    let Err(e) = core.rebuild_first(sid, turn) else {
        panic!("rebuilt");
    };
    let sent = settled(&requests).await;
    let p = TurnReplayParams {
        session: sid.into(),
        turn: turn.into(),
        profile: "haiku".into(),
        effort: None,
        scope: None,
        compare: Some(false),
    };
    assert!(core.turn_replay(p, "cli").await.is_err(), "not replayed");
    assert_eq!(requests(), sent, "no model was called");
    e
}

/// A front, window or beside blob that is missing, or does not hash to its name: refused, part `view`,
/// naming which part.
#[tokio::test]
async fn a_missing_or_corrupt_view_blob_is_refused_naming_its_part() {
    for corrupt in [false, true] {
        for part in ["front", "window", "beside block"] {
            let (rig, sid, t) = two_turns().await;
            let k = kept_of(&rig.core, &t);
            // A part this turn laid nothing of: its record names a blob that is not there.
            let absent = |edit: fn(&mut Kept, String)| {
                let d = crate::blobs::digest(format!("a {part} never written").as_bytes());
                edit_kept(&rig.core, &t, |k| edit(k, d.clone()));
                d
            };
            let digest = match part {
                "front" => match k.blocks.first() {
                    Some(d) => d.clone(),
                    None => absent(|k, d| k.blocks.push(d)),
                },
                "window" => k.window.last().expect("a populated window").clone(),
                _ => match k.beside.clone() {
                    Some(d) => d,
                    None => absent(|k, d| k.beside = Some(d)),
                },
            };
            spoil(&rig.core, &digest, corrupt);
            let n = || rig.model.requests().len();
            let e = refused(&rig.core, n, &sid, &t).await;
            assert_eq!(e.part, "view", "{part}: {e}");
            assert!(
                e.why.contains(&format!("the view's {part} blob {digest}")),
                "{part} (corrupt {corrupt}): {e}"
            );
        }
    }
}

/// A record durable while none of its blobs were written (the turn was cancelled or crashed before its
/// batch landed): refused in words, part `view`.
#[tokio::test]
async fn a_record_whose_blobs_were_never_written_is_refused() {
    let (rig, sid, t) = two_turns().await;
    let k = kept_of(&rig.core, &t);
    for d in k.blocks.iter().chain(&k.window).chain(&k.beside) {
        spoil(&rig.core, d, false);
    }
    let n = || rig.model.requests().len();
    let e = refused(&rig.core, n, &sid, &t).await;
    assert_eq!(e.part, "view", "{e}");
    assert!(e.why.contains("is missing"), "{e}");
}

/// A message the view laid, moved since into a thread: rebuilt as before.
#[tokio::test]
async fn a_view_turn_whose_laid_message_moved_since_is_still_rebuilt() {
    let (r, sid, t) = two_turns().await;
    let c = &r.core;
    c.rebuild_first(&sid, &t)
        .expect("rebuildable before the move");
    for (_, n) in c.store.session_nodes(&sid).unwrap() {
        if matches!(&n.body, crate::node::Body::UserMessage { .. }) {
            let to = serde_json::json!({"thread": "kelp", "split": "spl_x"});
            c.store
                .put_meta(&crate::moved::moved_of_key(&n.id), &to)
                .unwrap();
        }
    }
    c.rebuild_first(&sid, &t)
        .expect("rebuildable after the move");
}
