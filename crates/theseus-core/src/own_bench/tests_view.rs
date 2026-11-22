//! own-benchmark on a turn the view built (issue 44, `compiler::view::kept`): its first request rebuilt byte
//! for byte from the blobs its view record names; a front that did not change kept once; a turn from before
//! views kept anything refused in words; a turn a source of which was sealed since never replayed; and
//! `turn.replay` on a view turn as on any other.

use theseus_protocol::own_bench::TurnReplayParams;
use theseus_protocol::LedgerKind;

use super::rebuild::{VIEW_HELD, VIEW_REFUSED};
use super::tests::{board, script};
use crate::compiler::view::kept::Kept;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::config::MemoryMode;
use crate::tests_recall::{session, turn, Rig};

const ASK: &str = "Which port does the kestrel relay listen on?";
const ORIGINAL: &str = "The kestrel relay listens on port 7714.";
const REPLAYED: &str = "Port 7714.";

fn view_rig() -> Rig {
    crate::tests_recall::rig_with_secrets(MemoryMode::Off, board(), |c| {
        c.context.assembly = Some(AssemblySetting::All(Assembly::View));
    })
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
    assert!(got.view.is_some(), "rebuilt from its view");
    let sent = &rig.model.requests()[0];
    assert_eq!(got.digest, sent.digest());
    assert_eq!(&got.request, sent, "the rebuilt request is the one sent");
    assert_eq!((got.ask.as_str(), got.said.text.as_str()), (ASK, ORIGINAL));
}

/// Two turns of one conversation whose front did not change keep it once: the second names the same front
/// blob, so the store holds one file for both, and each turn is rebuilt.
#[tokio::test]
async fn a_front_that_did_not_change_is_kept_once() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL, "Still 7714."]);
    let sid = session(&rig.core, None, &[]);
    let first = turn(&rig.core, &sid, ASK).await.turn_id;
    let second = turn(&rig.core, &sid, "And the heron relay?").await.turn_id;
    let (a, b) = (kept_of(&rig.core, &first), kept_of(&rig.core, &second));
    assert_eq!(a.front, b.front, "the same front, one blob");
    assert!(rig.core.store.blobs().path(&a.front).exists());
    // The second turn's window is the first turn's exchange: one blob that joined it, nothing else new.
    assert_eq!(b.window.len(), a.window.len() + 1, "{a:?} {b:?}");
    assert_eq!(
        b.window[..a.window.len()],
        a.window[..],
        "older exchanges keep their blobs"
    );
    for t in [&first, &second] {
        rig.core.rebuild_first(&sid, t).expect("rebuildable");
    }
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

/// A view turn whose window laid a thread's exchange is rebuilt while the thread is clear; once the thread
/// is sealed, the turn is refused in words and `turn.replay` calls no model.
#[tokio::test]
async fn a_view_turn_is_not_replayed_once_a_source_it_laid_is_sealed() {
    use crate::tests_view_compile::{kelp, seal_kelp, turn_as};
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, false),
    );
    let c = &r.core;
    let thread = thread.into_inner();
    turn_as(c, &thread, "kelp: the embargoed figure is 7741", true).await;
    let desk = crate::tests_places::session(c, None);
    let t = turn_as(c, &desk, "desk: and now?", true).await.turn_id;
    assert!(
        kept_of(c, &t).sessions.contains(&thread),
        "the thread was laid"
    );
    c.rebuild_first(&desk, &t).expect("rebuildable while clear");
    seal_kelp(c);
    let Err(e) = c.rebuild_first(&desk, &t) else {
        panic!("a turn that laid a sealed thread was rebuilt");
    };
    assert_eq!((e.part, e.why.as_str()), ("view", VIEW_HELD));
    let sent = r.requests().len();
    let p = TurnReplayParams {
        session: desk.clone(),
        turn: t,
        profile: "haiku".into(),
        effort: None,
        scope: None,
        compare: Some(false),
    };
    let refused = c.turn_replay(p, "cli").await;
    assert!(refused.is_err(), "not replayed");
    assert_eq!(r.requests().len(), sent, "no model was called");
}

/// `turn.replay` of a view turn sends the original's system, tools and messages to the profile it names,
/// and writes one `turn.replayed` row.
#[tokio::test]
async fn a_view_turn_is_replayed_with_its_own_request() {
    let rig = view_rig();
    script(&rig, &[ORIGINAL, REPLAYED]);
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
    assert_eq!(
        (r.original.text.as_str(), r.replay.text.as_str()),
        (ORIGINAL, REPLAYED)
    );
    let sent = rig.model.requests();
    assert_eq!(sent.len(), 2, "one call for the replay");
    let (orig, re) = (&sent[0], &sent[1]);
    assert_ne!(re.model, orig.model, "the other profile's model");
    assert_eq!(re.messages, orig.messages, "the same messages");
    assert_eq!(re.tools, orig.tools, "the same tools");
    let text = |q: &crate::provider::ProviderRequest| -> Vec<serde_json::Value> {
        q.system.iter().map(|b| b["text"].clone()).collect()
    };
    assert_eq!(text(re), text(orig), "the same system");
    let replayed = rig.core.rows_since(LedgerKind::TurnReplayed, 0).unwrap();
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].turn_id.as_deref(), Some(t.as_str()));
}
