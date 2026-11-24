//! own-benchmark on a turn the view built (issue 44, `compiler::view::kept`): its first request rebuilt byte
//! for byte from the blobs its view record names; a front that did not change kept once; a turn from before
//! views kept anything refused in words; and `turn.replay` on a view turn as on any other.

use theseus_protocol::own_bench::TurnReplayParams;
use theseus_protocol::LedgerKind;

use super::rebuild::{LEGACY_REFUSED, VIEW_REFUSED};
use super::tests::{board, script};
use crate::compiler::view::kept::Kept;
use crate::config::MemoryMode;
use crate::tests_recall::{session, turn, Rig};

const ASK: &str = "Which port does the kestrel relay listen on?";
const ORIGINAL: &str = "The kestrel relay listens on port 7714.";

fn view_rig() -> Rig {
    crate::tests_recall::rig_with_secrets(MemoryMode::Off, board(), |_| {})
}

/// What a view turn's record keeps for a replay.
fn kept_of(core: &crate::Core, turn: &str) -> Kept {
    let v = crate::view::record::newest(&core.store, turn).unwrap();
    v.expect("a view record").kept.expect("kept for a replay")
}

/// A view turn's first request is rebuilt from its kept front, window and beside block and the session's
/// nodes: the digest its answer recorded, and the very request the model was sent.
#[tokio::test]
async fn a_view_turns_first_request_is_rebuilt_byte_for_byte() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL]);
    let sid = session(&rig.core, None, &[]);
    let t = turn(&rig.core, &sid, ASK).await.turn_id;
    let got = rig.core.rebuild_first(&sid, &t).expect("rebuildable");
    assert_eq!(got.compilation.strategy, crate::compiler::view::STRATEGY);
    let sent = &rig.model.requests()[0];
    assert_eq!(got.digest, sent.digest());
    assert_eq!(&got.request, sent, "the rebuilt request is the one sent");
    assert_eq!((got.ask.as_str(), got.said.text.as_str()), (ASK, ORIGINAL));
}

/// Turns of one conversation whose front did not change keep it once: each names the same front blobs, so
/// the store holds one file of each for all of them. The third turn's window is the second's and one more
/// exchange, and the exchanges both laid keep their blobs (a populated window, so that is a real check).
/// Each turn is rebuilt.
#[tokio::test]
async fn a_front_that_did_not_change_is_kept_once() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL, "Still 7714.", "The heron relay is 7716."]);
    let sid = session(&rig.core, None, &[]);
    let first = turn(&rig.core, &sid, ASK).await.turn_id;
    let second = turn(&rig.core, &sid, "And the heron relay?").await.turn_id;
    let third = turn(&rig.core, &sid, "And the wren relay?").await.turn_id;
    let [a, b, c] = [&first, &second, &third].map(|t| kept_of(&rig.core, t));
    // Kept by block (this rig's front has no fixed block or ladder, so none: `a_heading_that_changed_…`
    // holds a populated one).
    assert!(a.front.is_empty(), "kept by block: {a:?}");
    assert_eq!(
        (&a.blocks, &b.blocks),
        (&c.blocks, &c.blocks),
        "the same front blobs"
    );
    let blobs = rig.core.store.blobs();
    assert!(a.blocks.iter().all(|d| blobs.path(d).exists()));
    // The second turn's window is the first turn's exchange; the third's is that one and the second's.
    assert_eq!(b.window.len(), a.window.len() + 1, "{a:?} {b:?}");
    assert_eq!(c.window.len(), b.window.len() + 1, "{b:?} {c:?}");
    assert!(!b.window.is_empty(), "a populated window");
    assert_eq!(
        c.window[..b.window.len()],
        b.window[..],
        "older exchanges keep their blobs"
    );
    assert!(c.window.iter().all(|d| blobs.path(d).exists()));
    for t in [&first, &second, &third] {
        rig.core.rebuild_first(&sid, t).expect("rebuildable");
    }
}

/// A record kept before the front was kept by block (one blob, a JSON array of the blocks' texts) is still
/// rebuilt byte for byte.
#[tokio::test]
async fn a_front_kept_as_one_blob_is_still_rebuilt() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL]);
    let sid = session(&rig.core, None, &[]);
    let t = turn(&rig.core, &sid, ASK).await.turn_id;
    let store = &rig.core.store;
    let mut v = crate::view::record::newest(store, &t).unwrap().unwrap();
    let k = v.kept.as_mut().unwrap();
    let texts: Vec<String> = (k.blocks.iter())
        .map(|d| String::from_utf8(store.blobs().read(d).unwrap()).unwrap())
        .collect();
    k.front = store
        .blobs()
        .put(serde_json::to_string(&texts).unwrap().as_bytes())
        .unwrap();
    k.blocks.clear();
    store.put_meta(&v.key(), &v).unwrap();
    let got = rig.core.rebuild_first(&sid, &t).expect("rebuildable");
    assert_eq!(&got.request, &rig.model.requests()[0], "byte for byte");
}

/// A view turn whose record keeps nothing for a replay (built before views kept it) is refused in words,
/// before any byte is rendered again.
#[tokio::test]
async fn a_view_turn_from_before_views_kept_it_is_refused_in_words() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL]);
    let sid = session(&rig.core, None, &[]);
    let t = turn(&rig.core, &sid, ASK).await.turn_id;
    let store = &rig.core.store;
    let mut v = crate::view::record::newest(store, &t).unwrap().unwrap();
    v.kept = None;
    store.put_meta(&v.key(), &v).unwrap();
    let Err(e) = rig.core.rebuild_first(&sid, &t) else {
        panic!("a view turn with nothing kept was rebuilt");
    };
    assert_eq!((e.part, e.why.as_str()), ("compilation", VIEW_REFUSED));
    assert!(
        e.to_string()
            .contains("this turn was built before views kept what a replay needs"),
        "{e}"
    );
}

/// `turn.replay` of a view turn sends the original's system, tools and messages to the profile it names,
/// and writes one `turn.replayed` row. (A view place's ladder writer may call the model too, without tools:
/// only the requests with tools are the turn's and the replay's.)
#[tokio::test]
async fn a_view_turn_is_replayed_with_its_own_request() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL]);
    let sid = session(&rig.core, None, &[]);
    let t = turn(&rig.core, &sid, ASK).await.turn_id;
    let p = TurnReplayParams {
        session: sid.clone(),
        turn: t.clone(),
        profile: "haiku".into(),
        effort: None,
        scope: None,
        compare: Some(false),
    };
    let r = rig.core.turn_replay(p, "cli").await.unwrap();
    assert_eq!(r.original.text, ORIGINAL);
    assert!(!r.replay.text.is_empty(), "the replay answered");
    let sent: Vec<_> = (rig.model.requests().into_iter())
        .filter(|q| !q.tools.is_empty())
        .collect();
    assert_eq!(sent.len(), 2, "one call for the replay");
    let (orig, re) = (&sent[0], &sent[1]);
    assert_ne!(re.model, orig.model, "the other profile's model");
    // The same messages; each request carries its own profile's cache lifetime on the front's mark (who am I
    // is a fresh conversation's front).
    let plain = |q: &crate::provider::ProviderRequest| -> Vec<serde_json::Value> {
        q.messages.iter().map(crate::provider::binding::unmarked).collect()
    };
    assert_eq!(plain(re), plain(orig), "the same messages");
    assert_eq!(re.tools, orig.tools, "the same tools");
    let text = |q: &crate::provider::ProviderRequest| -> Vec<serde_json::Value> {
        q.system.iter().map(|b| b["text"].clone()).collect()
    };
    assert_eq!(text(re), text(orig), "the same system");
    let replayed = rig.core.rows_since(LedgerKind::TurnReplayed, 0).unwrap();
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].turn_id.as_deref(), Some(t.as_str()));
}

/// A front whose one small block changed (a person's heading, a minute later) and whose large ladder block
/// did not: keeping the second adds the changed block's few bytes, not the ladder's, and each front reads
/// back as it was laid.
#[test]
fn a_heading_that_changed_does_not_copy_the_ladder() {
    use crate::compiler::view::kept::{frozen, kept};
    use crate::compiler::view::Frozen;
    let dir = tempfile::tempdir().unwrap();
    let blobs = crate::blobs::Blobs::new(dir.path());
    let ladder = "A fold of the harbour's week: the moorings, the relays, the wall. ".repeat(2_000);
    let front = |at: &str| Frozen {
        turn_id: "t".into(),
        front: vec![
            Frozen::block(
                "s",
                "t",
                "",
                &format!("Who is speaking (at {at}): the owner."),
            ),
            Frozen::block("s", "t", "", &ladder),
        ],
        ..Frozen::default()
    };
    let (k1, t1) = kept(&front("10:41")).unwrap();
    for t in &t1 {
        blobs.put(t.as_bytes()).unwrap();
    }
    let (k2, t2) = kept(&front("10:42")).unwrap();
    let texts: Vec<&str> = t2.iter().map(String::as_str).collect();
    let new: usize = (crate::compiler::cache::fresh(&blobs, &texts).iter())
        .map(Vec::len)
        .sum();
    assert!(ladder.len() > 100_000, "a large ladder");
    assert!(new < 100, "the second front stored {new} new bytes");
    for t in &t2 {
        blobs.put(t.as_bytes()).unwrap();
    }
    for (k, at) in [(k1, "10:41"), (k2, "10:42")] {
        let back = frozen(&blobs, &k, ("s", "t")).unwrap();
        let text = |f: &Frozen| -> Vec<String> {
            f.front.iter().map(|n| format!("{:?}", n.body)).collect()
        };
        assert_eq!(text(&back), text(&front(at)), "read back as laid");
    }
}

/// A turn's end waits for its blobs' writes a bounded while: with every write held past it, the turn still
/// completes, and its replay is refused in words (naming the missing blob) with no partial request; once
/// the writes land on their own, it is rebuilt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_completes_while_its_blobs_writes_are_held() {
    use std::time::Duration;
    let rig = view_rig();
    script(&rig, &[ORIGINAL]);
    let sid = session(&rig.core, None, &[]);
    let release = rig.core.store.blobs().hold_puts();
    let asked = tokio::time::timeout(Duration::from_secs(60), turn(&rig.core, &sid, ASK));
    let t = asked
        .await
        .expect("the turn completed, its writes held")
        .turn_id;
    let Err(e) = rig.core.rebuild_first(&sid, &t) else {
        panic!("rebuilt while its blobs were not written");
    };
    // The first of its blobs a rebuild reads: the view's (none on a first turn here), then the system's.
    assert!(["view", "system"].contains(&e.part), "{e}");
    assert!(e.why.contains("is missing"), "{e}");
    drop(release);
    let t0 = std::time::Instant::now();
    while rig.core.rebuild_first(&sid, &t).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the writes never landed"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// An answer whose compilation the old transcript builder made is refused in words, before anything of it is
/// rendered or sent (issue 44, 2026-11-22: no saved request of a builder that no longer exists is sent
/// again); its compilation still decodes and lists.
#[tokio::test]
async fn a_turn_the_transcript_builder_made_is_refused_in_words() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL]);
    let sid = session(&rig.core, None, &[]);
    let t = turn(&rig.core, &sid, ASK).await.turn_id;
    let store = &rig.core.store;
    let mut c = rig
        .core
        .rebuild_first(&sid, &t)
        .expect("rebuildable")
        .compilation;
    c.strategy = "transcript".into();
    let record = theseus_store::NewRecord::json(theseus_store::kinds::COMPILATION, Some(&c.id), &c)
        .unwrap()
        .scoped(&c.session_id);
    store.append(&[record]).unwrap();
    let sent = rig.model.requests().len();
    let Err(e) = rig.core.rebuild_first(&sid, &t) else {
        panic!("a transcript-era turn was rebuilt");
    };
    assert_eq!((e.part, e.why.as_str()), ("compilation", LEGACY_REFUSED));
    assert!(
        e.to_string()
            .contains("made by a builder that no longer exists"),
        "{e}"
    );
    assert_eq!(rig.model.requests().len(), sent, "nothing was sent");
    let old = store
        .get_compilation(&c.id)
        .unwrap()
        .expect("it still decodes");
    assert_eq!(old.strategy, "transcript");
    assert!(store
        .session_compilations(&sid)
        .unwrap()
        .iter()
        .any(|k| k.id == c.id));
}
