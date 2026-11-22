//! own-benchmark refuses a view turn it cannot replay as it was, in words and before any model is called
//! (issue 44): a blob of its front, window or beside block missing or not what its name says, each named; a
//! record durable while none of its blobs were written (a turn cancelled or crashed before its batch
//! landed); a message it laid moved since into a sealed thread; a node its recall laid sealed since; and a
//! ladder fold's constituent sealed since.

use theseus_protocol::own_bench::TurnReplayParams;

use super::rebuild::{NotRebuildable, VIEW_HELD};
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
                "front" => k.front.clone(),
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
    for d in std::iter::once(&k.front).chain(&k.window).chain(&k.beside) {
        spoil(&rig.core, d, false);
    }
    let n = || rig.model.requests().len();
    let e = refused(&rig.core, n, &sid, &t).await;
    assert_eq!(e.part, "view", "{e}");
    assert!(e.why.contains("is missing"), "{e}");
}

/// A thread's rig: the view on, threads on, `kelp` a thread sealed as `seal` says.
fn thread_rig(seal: bool) -> (crate::tests_places::Rig, String) {
    use crate::tests_view_compile::kelp;
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup(
        |c| {
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, seal),
    );
    let thread = thread.into_inner();
    (r, thread)
}

/// A message the view laid, moved since into a sealed thread: refused, `VIEW_HELD`.
#[tokio::test]
async fn a_view_turn_whose_laid_message_moved_since_is_refused() {
    use crate::tests_view_compile::turn_as;
    const SAID: &str = "desk: the embargoed figure is 7741";
    let (r, _) = thread_rig(true);
    let c = &r.core;
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, SAID, true).await;
    let t = turn_as(c, &desk, "desk: and now?", true).await.turn_id;
    c.rebuild_first(&desk, &t)
        .expect("rebuildable before the move");
    for (_, n) in c.store.session_nodes(&desk).unwrap() {
        if matches!(&n.body, crate::node::Body::UserMessage { text, .. } if text == SAID) {
            let to = serde_json::json!({"thread": "kelp", "split": "spl_x"});
            c.store
                .put_meta(&crate::moved::moved_of_key(&n.id), &to)
                .unwrap();
        }
    }
    let e = refused(c, || r.requests().len(), &desk, &t).await;
    assert_eq!((e.part, e.why.as_str()), ("view", VIEW_HELD));
}

/// A node the view's recall laid, sealed since (its thread sealed): refused, `VIEW_HELD`. The record names
/// the node as a recall would.
#[tokio::test]
async fn a_view_turn_whose_recalled_node_was_sealed_since_is_refused() {
    use crate::tests_view_compile::{seal_kelp, turn_as};
    let (r, thread) = thread_rig(false);
    let c = &r.core;
    turn_as(c, &thread, "kelp: the embargoed figure is 7741", true).await;
    let node = (c.store.session_nodes(&thread).unwrap().into_iter())
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .map(|(_, n)| n.id)
        .expect("a kelp message");
    let desk = crate::tests_places::session(c, None);
    let t = turn_as(c, &desk, "desk: and now?", true).await.turn_id;
    edit_kept(c, &t, |k| {
        k.recalled.push(node);
        k.sessions.retain(|s| *s != thread);
        k.exchanges.clear();
    });
    c.rebuild_first(&desk, &t).expect("rebuildable while clear");
    seal_kelp(c);
    let e = refused(c, || r.requests().len(), &desk, &t).await;
    assert_eq!((e.part, e.why.as_str()), ("view", VIEW_HELD));
}

/// A ladder fold's constituent (its exchange and session, which the view lists beside the window's)
/// sealed after the turn: refused, `VIEW_HELD`.
#[tokio::test]
async fn a_view_turn_whose_folds_constituent_was_sealed_since_is_refused() {
    use crate::tests_view_compile::{seal_kelp, turn_as};
    let (r, thread) = thread_rig(false);
    let c = &r.core;
    let kt = turn_as(c, &thread, "kelp: the embargoed figure is 7741", true)
        .await
        .turn_id;
    let desk = crate::tests_places::session(c, None);
    let t = turn_as(c, &desk, "desk: and now?", true).await.turn_id;
    // The kelp exchange as a fold's constituent: listed, whether or not the window laid it.
    edit_kept(c, &t, |k| {
        if !k.sessions.contains(&thread) {
            k.sessions.push(thread.clone());
        }
        if !k.exchanges.contains(&kt) {
            k.exchanges.push(kt.clone());
        }
    });
    c.rebuild_first(&desk, &t).expect("rebuildable while clear");
    seal_kelp(c);
    let e = refused(c, || r.requests().len(), &desk, &t).await;
    assert_eq!((e.part, e.why.as_str()), ("view", VIEW_HELD));
}
