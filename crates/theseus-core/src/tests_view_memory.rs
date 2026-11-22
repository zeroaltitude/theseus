//! Memory intact on the view (issue 44): slot H lays what recall found and admitted, as a transcript
//! turn's `Recall` note carried it, whatever the item was (an imported session's, a conversation's from
//! before the ledger, a node that is no turn's), once each; and never in company what the note would not
//! have carried, nor a source sealed since, even between two calls of one turn.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, turn, LAB};
use crate::tests_private_view::{index_of, owners_rig, session_in, ASKED, EPISODE};
use crate::tests_view_compile::{kelp, seal_kelp, texts};

/// Said in an imported session.
const IMPORTED: &str = "the imported note says the Hollowmere hives wintered in the orchard";
/// Said in a turn of a conversation from before the ledger (no exchange).
const BEFORE: &str = "before the ledger the Hollowmere hives lost their queen";

/// Every turn is the view's (the setting is retired): nothing to set.
fn view(_: &mut crate::Config) {}

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
    assert!(
        crate::exchange::exchange(&c.store, "turn_before_the_ledger")
            .unwrap()
            .is_none()
    );
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

/// In a shared channel the owner's private memory is not laid: none of his imported, earlier or episodic
/// items reaches a request made in company.
#[tokio::test]
async fn in_company_none_of_the_owners_private_items_is_laid() {
    let (r, ids) = owners_rig(|_| {});
    let c = &r.core;
    let mut sources = old_memories(c);
    sources.push(ids[3].clone());
    c.runner.memory.set_ask(index_of(c, sources));
    let shared = crate::tests_places::session(c, Some(&format!("channel:{LAB}")));
    turn(c, &shared, ASKED).await;
    let sent = texts(&calls(&r).pop().unwrap());
    assert!(
        sent.contains(ASKED),
        "the request read is the shared turn's: {sent}"
    );
    for words in [IMPORTED, BEFORE, EPISODE] {
        assert!(
            !sent.contains(words),
            "{words:?} is not laid in company: {sent}"
        );
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
    assert!(
        texts(&asked[0]).contains("5519"),
        "laid while clear: {}",
        texts(&asked[0])
    );
    for q in &asked[1..] {
        assert!(!texts(q).contains("5519"), "{}", texts(q));
    }
}

/// An imported session holding one real `Body::Imported` message of `integrity`, its node id returned.
fn real_import(
    c: &crate::Core,
    integrity: crate::import::Integrity,
    text: &str,
) -> (String, String) {
    use crate::session::SessionRecord;
    let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.session_id = format!("{}{}", crate::import::SESSION_PREFIX, "b".repeat(64));
    c.store.put_session(&rec.session_id, &rec).unwrap();
    let body = crate::node::Body::Imported {
        text: text.into(),
        integrity,
        source: "wiki".into(),
        unit: "u1".into(),
        sha256: "0".repeat(64),
        idx: 0,
    };
    let id = format!("node_import_{}", rec.session_id.len());
    let n = crate::node::Node::imported(
        id.clone(),
        &rec.session_id,
        "someone",
        1_767_787_200_000,
        body,
    );
    c.store.append(&[n.record().unwrap()]).unwrap();
    (rec.session_id, id)
}

/// P1 of the review: two successive view turns ask different details of the same imported passage. The
/// passage is laid on the SECOND request too, also with the first turn's `Recall` note persisted in the
/// session (a note's sources exclude nothing on the view).
#[tokio::test]
async fn a_follow_up_turn_is_shown_the_passage_recalled_before() {
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let sources = old_memories(c);
    c.runner.memory.set_ask(index_of(c, sources.clone()));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    assert!(texts(&calls(&r).pop().unwrap()).contains(IMPORTED));
    // The first turn's Recall note, as a transcript turn (or the flip) persists it.
    let node = (c.store.session_nodes(&sources[0]).unwrap().into_iter())
        .next()
        .unwrap()
        .1;
    let item = crate::node::RecalledRef {
        node_id: node.id.clone(),
        session_id: sources[0].clone(),
        position: 0,
        chunk: (0, 10),
        header: "an imported message".into(),
        tokens: 10,
    };
    let note = crate::node::Node::recall(&sid, "turn_first", "rcl_first", "baseline", vec![item]);
    c.store.append(&[note.record().unwrap()]).unwrap();
    turn(c, &sid, "And where did the Hollowmere hives winter?").await;
    let second = texts(&calls(&r).pop().unwrap());
    assert!(second.contains("Hollowmere hives winter"), "{second}");
    assert_eq!(second.matches(IMPORTED).count(), 1, "{second}");
}

/// A message of the ASKING session from before the ledger (no exchange; in no window) is recalled and laid.
#[tokio::test]
async fn a_pre_ledger_message_of_the_asking_session_is_laid() {
    const OLD: &str = "long ago in this very session the Hollowmere hives were painted blue";
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let m = crate::node::Node::user(&sid, Some("turn_before_the_ledger"), "test", OLD);
    c.store.append(&[m.record().unwrap()]).unwrap();
    c.runner.memory.set_ask(index_of(c, vec![sid.clone()]));
    turn(c, &sid, ASKED).await;
    let sent = texts(&calls(&r).pop().unwrap());
    assert_eq!(sent.matches(OLD).count(), 1, "{sent}");
}

/// An exchange the window lays with its words (another session of the same place, recalled too) is laid
/// once, by the window: slot H does not repeat it.
#[tokio::test]
async fn an_exchange_in_the_window_is_not_repeated_in_slot_h() {
    const SAID: &str = "the Hollowmere hives are kept behind the north orchard wall";
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let old = session_in(c, None, &r.root);
    turn(c, &old, SAID).await;
    c.runner.memory.set_ask(index_of(c, vec![old.clone()]));
    let new = session_in(c, None, &r.root);
    turn(c, &new, ASKED).await;
    let sent = texts(&calls(&r).pop().unwrap());
    assert_eq!(sent.matches(SAID).count(), 1, "{sent}");
}

/// A real outside import, admitted with outside recall enabled, is laid wrapped as outside text; the
/// recall note's preamble opens slot H's recalled items once.
#[tokio::test]
async fn a_known_outside_import_is_wrapped_and_the_preamble_is_said_once() {
    const OUTSIDE: &str = "a web page said the Hollowmere hives belong to the county";
    let (r, _) = owners_rig(|c| {
        view(c);
        c.memory.include_external = true;
    });
    let c = &r.core;
    let (imported, _) = real_import(c, crate::import::Integrity::Outside, OUTSIDE);
    c.runner.memory.set_ask(index_of(c, vec![imported]));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    let sent = texts(&calls(&r).pop().unwrap());
    let at = sent.find(OUTSIDE).unwrap_or_else(|| panic!("laid: {sent}"));
    let before = &sent[..at];
    assert!(before.rfind("[outside text from").is_some(), "{sent}");
    assert!(sent[at..].contains("[end of outside text, fence"), "{sent}");
    assert_eq!(
        sent.matches("[Recalled by the harness:").count(),
        1,
        "{sent}"
    );
}

/// On the view alone (no transcript to compare with): in a shared channel none of the owner's private
/// memories is laid. This is the test that stays when the transcript goes.
#[tokio::test]
async fn in_company_on_the_view_no_private_memory_is_laid() {
    let (r, ids) = owners_rig(view);
    let c = &r.core;
    let mut sources = old_memories(c);
    sources.push(ids[3].clone());
    c.runner.memory.set_ask(index_of(c, sources));
    let shared = crate::tests_places::session(c, Some(&format!("channel:{LAB}")));
    turn(c, &shared, ASKED).await;
    let sent = texts(&calls(&r).pop().unwrap());
    for words in [IMPORTED, BEFORE, EPISODE] {
        assert!(
            !sent.contains(words),
            "{words:?} is not laid in company: {sent}"
        );
    }
}

/// A real imported episode labelled `sensitivity`, holding one operator message `text`.
fn labelled_import(c: &crate::Core, sensitivity: &str, text: &str, n: u8) -> String {
    use crate::import::{AsOf, EpisodeLabels, EpisodePlace, ImportedFrom, Integrity};
    use crate::session::SessionRecord;
    let sid = format!(
        "{}{}",
        crate::import::SESSION_PREFIX,
        format!("{n:x}").repeat(64).get(..64).unwrap()
    );
    let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.session_id = sid.clone();
    rec.imported = Some(Box::new(ImportedFrom {
        tag: "t".into(),
        episode_id: format!("ep_{n}"),
        hash: "h".into(),
        source: "openclaw-store".into(),
        agent: None,
        place: EpisodePlace {
            kind: "dm".into(),
            name: None,
            id: None,
        },
        labels: EpisodeLabels {
            sensitivity: sensitivity.into(),
            partner: (sensitivity == "partner-confidential")
                .then(|| "partner-candidate:kelp".into()),
            topic: vec![],
            book_hint: None,
            credential_redacted: false,
        },
        triage: None,
        as_of: AsOf {
            start_ms: 1,
            end_ms: 2,
        },
        messages: 1,
        summary: false,
        file: "x.jsonl".into(),
        line: 1,
        imported_at_ms: 3,
        erased: None,
        diverged_at: vec![],
    }));
    c.store.put_session(&sid, &rec).unwrap();
    let body = crate::node::Body::Imported {
        text: text.into(),
        integrity: Integrity::Operator,
        source: "openclaw-store".into(),
        unit: format!("u{n}"),
        sha256: "0".repeat(64),
        idx: 0,
    };
    let node = crate::node::Node::imported(
        format!("node_veil_{n}"),
        &sid,
        "operator",
        1_767_787_200_000,
        body,
    );
    c.store.append(&[node.record().unwrap()]).unwrap();
    sid
}

/// Corrected, pinned as it is (the conductor's item 5): automatic recall applies no sensitivity veil to an
/// import. A `personal` and a `partner-confidential` import are laid when the owner is alone; in a shared
/// channel, and in a private channel others may join, neither is.
#[tokio::test]
async fn veiled_imports_are_laid_only_where_the_owner_is_alone() {
    const PERSONAL: &str = "a personal diary line about the Hollowmere hives and the doctor";
    const PARTNER: &str = "a partner-confidential note on the Hollowmere hives contract";
    for place in ["alone", "shared", "private channel"] {
        let (r, _) = owners_rig(view);
        let c = &r.core;
        let sources = vec![
            labelled_import(c, "personal", PERSONAL, 1),
            labelled_import(c, "partner-confidential", PARTNER, 2),
        ];
        c.runner.memory.set_ask(index_of(c, sources));
        let sid = match place {
            "alone" => session_in(c, None, &r.root),
            "shared" => crate::tests_places::session(c, Some(&format!("channel:{LAB}"))),
            _ => {
                c.bind_places(vec![crate::places::BoundPlace {
                    target: format!("discord:channel:{LAB}"),
                    name: "#lab".into(),
                    private: true,
                    ..Default::default()
                }]);
                crate::tests_places::session(c, Some(&format!("channel:{LAB}")))
            }
        };
        turn(c, &sid, ASKED).await;
        let sent = texts(&calls(&r).pop().unwrap());
        let laid = [PERSONAL, PARTNER].map(|w| sent.contains(w));
        assert_eq!(laid, [place == "alone"; 2], "{place}: {sent}");
    }
}

/// The owner's own DM with his in-person mark recalls an import: laid. An agent's process at the terminal
/// (never the owner alone) is not shown the import, but is shown an item of its own place.
#[tokio::test]
async fn the_owners_dm_is_shown_an_import_and_an_agent_at_the_terminal_only_its_place() {
    const HERE: &str = "earlier at this terminal the Hollowmere hives got a new roof";
    // The owner's DM.
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let sources = old_memories(c);
    c.runner.memory.set_ask(index_of(c, sources));
    let dm = session_in(
        c,
        Some(&format!("dm:{}", crate::tests_places::OWNER)),
        &r.root,
    );
    turn(c, &dm, ASKED).await;
    let sent = texts(&calls(&r).pop().unwrap());
    assert!(sent.contains(IMPORTED), "the owner's DM: {sent}");
    // An agent's process at the terminal.
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let here = session_in(c, None, &r.root);
    crate::tests_view_compile::turn_as(c, &here, HERE, true).await;
    let mut sources = old_memories(c);
    sources.push(here);
    c.runner.memory.set_ask(index_of(c, sources));
    let sid = session_in(c, None, &r.root);
    crate::tests_view_compile::turn_as(c, &sid, ASKED, false).await;
    let sent = texts(&calls(&r).pop().unwrap());
    assert!(
        !sent.contains(IMPORTED),
        "an agent is not shown an import: {sent}"
    );
    assert!(
        sent.contains(HERE),
        "an agent is shown its own place: {sent}"
    );
}
