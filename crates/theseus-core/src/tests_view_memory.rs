//! Memory intact on the view (issue 44): slot H lays what recall found and admitted, as a transcript
//! turn's `Recall` note carried it, whatever the item was (an imported session's, a conversation's from
//! before the ledger, a node that is no turn's), once each; and never in company what the note would not
//! have carried, nor a source sealed since, even between two calls of one turn.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use crate::config::assembly::{Assembly, AssemblySetting};
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, turn, LAB};
use crate::tests_private_view::{index_of, owners_rig, session_in, ASKED, EPISODE};
use crate::tests_view_compile::{kelp, seal_kelp, texts};

/// Said in an imported session.
const IMPORTED: &str = "the imported note says the Hollowmere hives wintered in the orchard";
/// Said in a turn of a conversation from before the ledger (no exchange).
const BEFORE: &str = "before the ledger the Hollowmere hives lost their queen";

fn view(c: &mut crate::Config) {
    c.context.assembly = Some(AssemblySetting::All(Assembly::View));
}

/// The requests with tools (a turn's calls), oldest first.
fn calls(r: &crate::tests_places::Rig) -> Vec<ProviderRequest> {
    (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty())
        .collect()
}

/// An imported session holding `IMPORTED`, and a session whose turn from before the ledger said `BEFORE`.
fn old_memories(c: &crate::Core) -> Vec<String> {
    use crate::session::SessionRecord;
    let mut imported = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    imported.session_id = format!("{}{}", crate::import::SESSION_PREFIX, "a".repeat(64));
    c.store
        .put_session(&imported.session_id, &imported)
        .unwrap();
    let n = crate::node::Node::user(&imported.session_id, None, "import", IMPORTED);
    let old = crate::tests_places::session(c, None);
    let m = crate::node::Node::user(&old, Some("turn_before_the_ledger"), "test", BEFORE);
    c.store
        .append(&[n.record().unwrap(), m.record().unwrap()])
        .unwrap();
    assert!(crate::exchange::exchange(&c.store, "turn_before_the_ledger")
        .unwrap()
        .is_none());
    vec![imported.session_id, old]
}

/// The owner alone at his terminal: a recalled node of an imported session, of a turn from before the
/// ledger, and a node that is no turn's are each in the request with the recalled words, once each.
#[tokio::test]
async fn the_owner_alone_is_shown_every_recalled_item_once() {
    let (r, ids) = owners_rig(view);
    let c = &r.core;
    let mut sources = old_memories(c);
    sources.push(ids[3].clone());
    c.runner.memory.set_ask(index_of(c, sources));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    let q = calls(&r).pop().unwrap();
    let sent = texts(&q);
    for words in [IMPORTED, BEFORE, EPISODE] {
        assert_eq!(sent.matches(words).count(), 1, "{words:?} once: {sent}");
    }
    let compiled = crate::test_support::rows(c, "context.compiled");
    assert_eq!(compiled.last().unwrap()["strategy"], "view");
}

/// In a shared channel the owner's private memory is not laid: the view lays no more than the transcript's
/// `Recall` note carried there, which is nothing of it.
#[tokio::test]
async fn in_company_a_private_item_is_not_laid() {
    let mut got = Vec::new();
    for on_view in [false, true] {
        let (r, ids) = owners_rig(|c| {
            if on_view {
                view(c);
            }
        });
        let c = &r.core;
        let mut sources = old_memories(c);
        sources.push(ids[3].clone());
        c.runner.memory.set_ask(index_of(c, sources));
        let shared = crate::tests_places::session(c, Some(&format!("channel:{LAB}")));
        turn(c, &shared, ASKED).await;
        let sent = texts(&calls(&r).pop().unwrap());
        got.push([IMPORTED, BEFORE, EPISODE].map(|w| sent.contains(w)));
    }
    let (transcript, view) = (got[0], got[1]);
    for (i, laid) in view.iter().enumerate() {
        assert!(!laid, "item {i} is not laid in company: {view:?}");
        assert!(!laid || transcript[i], "never more than the transcript: {transcript:?} {view:?}");
    }
}

/// A recalled item whose source thread is sealed is not laid.
#[tokio::test]
async fn a_sealed_threads_recalled_item_is_not_laid() {
    const SECRET: &str = "the kelp vault code for the Hollowmere hives is 5519";
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup(
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
            view(c);
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, true),
    );
    let c = &r.core;
    let thread = thread.into_inner();
    let n = crate::node::Node::user(&thread, None, "test", SECRET);
    c.store.append(&[n.record().unwrap()]).unwrap();
    c.runner.memory.set_ask(index_of(c, vec![thread]));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    let sent = texts(&calls(&r).pop().unwrap());
    assert!(!sent.contains("5519"), "{sent}");
}

/// A recalled item laid in a turn's first call whose source thread is sealed before its second call is
/// gone from the second.
#[tokio::test]
async fn a_recalled_item_sealed_between_two_calls_is_gone_from_the_second() {
    const SECRET: &str = "the kelp vault code for the Hollowmere hives is 5519";
    let seen = Arc::new(AtomicU32::new(0));
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let (count, held) = (seen.clone(), core.clone());
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
            view(c);
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, false),
        move |q| {
            if q.tools.is_empty() {
                return Scripted::text("aside");
            }
            if answers_a_call(q) {
                return Scripted::text("Done.");
            }
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                seal_kelp(held.get().unwrap());
            }
            Scripted::tools(
                "Looking.",
                &[("b1", "fs_list", serde_json::json!({"path": "."}))],
            )
        },
    );
    let _ = core.set(r.core.clone());
    let c = &r.core;
    let thread = thread.into_inner();
    let n = crate::node::Node::user(&thread, None, "test", SECRET);
    c.store.append(&[n.record().unwrap()]).unwrap();
    c.runner.memory.set_ask(index_of(c, vec![thread]));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    let asked = calls(&r);
    assert!(asked.len() >= 2, "{}", asked.len());
    assert!(texts(&asked[0]).contains("5519"), "laid while clear: {}", texts(&asked[0]));
    for q in &asked[1..] {
        assert!(!texts(q).contains("5519"), "{}", texts(q));
    }
}
