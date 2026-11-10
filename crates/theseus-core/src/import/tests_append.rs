//! A session that grew (theseus-pjmj7): the same episode id with another
//! hash whose messages begin with the stored ones appends only the new
//! nodes, with the session's record and the tag's counts in the same frame;
//! a changed, removed or reordered message is refused and named by its
//! idx; an erased session is never appended to.

use serde_json::{json, Value};
use sha2::Digest;
use theseus_protocol::import::ImportListResult;
use theseus_protocol::method;

use super::episode;
use super::tests::{call, import, lines, live};
use super::{node_id_of, session_id_of};
use crate::node::Body;
use crate::session::SessionRecord;

pub(crate) const TAG: &str = "openclaw-sync-2026-10";

/// A key-shaped value, built from parts.
pub(crate) fn token() -> String {
    format!("{}_{}", "ghp", "Zy7aB9cD2eF4gH6iJ8kL0mN2oP4qR6sT8uV0")
}

/// The text of message `idx`: the second carries a secret.
fn text(idx: u32) -> String {
    if idx == 1 {
        format!("The deploy key for the quay is {}", token())
    } else {
        format!("Message {idx}: the tide log stays in the boathouse ledger.")
    }
}

/// An episode of an OpenClaw session with the messages `idxs` (their `seq`s).
pub(crate) fn sync(n: u64, tag: &str, idxs: &[u32]) -> Value {
    let messages: Vec<Value> = idxs
        .iter()
        .map(|&i| {
            let (author, integrity) = match i % 4 {
                0 | 2 => ("operator", "operator"),
                3 => ("person:Marlo Quill", "outside"),
                _ => ("agent:main", "agent"),
            };
            json!({"idx": i, "time": format!("2026-10-05T10:{:02}:00.000Z", i % 60),
                   "author": author, "integrity": integrity, "text": text(i),
                   "unit": format!("main/s{n}#{i}"), "sha256": "ab".repeat(32)})
        })
        .collect();
    let last = idxs.last().copied().unwrap_or(0) % 60;
    let mut v = json!({
        "format": 1, "import_tag": tag, "episode_id": format!("ep_{:064x}", 0x5eed + n),
        "source": "openclaw-sync", "agent": "main",
        "place": {"kind": "dm", "name": null, "id": null},
        "as_of": {"start": "2026-10-05T10:00:00.000Z", "end": format!("2026-10-05T10:{last:02}:00.000Z")},
        "labels": {"sensitivity": "personal", "partner": null, "topic": [],
                   "book_hint": null, "credential_redacted": false},
        "summary": null,
        "messages": messages,
    });
    v["hash"] = json!(episode::hash_of(&v));
    v
}

fn counts(list: &Value, tag: &str) -> (u64, u64) {
    let l: ImportListResult = serde_json::from_value(list.clone()).unwrap();
    let t = l.tags.iter().find(|t| t.tag == tag).expect("the tag");
    (t.sessions, t.nodes)
}

async fn list(c: &std::sync::Arc<crate::Core>) -> Value {
    call(c, method::IMPORT_LIST, json!({})).await.unwrap()
}

/// An append writes only the new messages, in one frame with the session's
/// record and the tag's counts; the second run, and a repeat of the older
/// episode, write nothing.
#[tokio::test]
async fn an_append_writes_only_the_new_nodes_and_the_counts_add_up() {
    let r = live();
    let c = &r.core;
    let first = sync(1, TAG, &[0, 1, 2]);
    let got = import(c, lines(std::slice::from_ref(&first))).await;
    assert_eq!(
        (got.imported, got.appended, got.nodes),
        (1, 0, 3),
        "{got:?}"
    );
    let sid = session_id_of(first["episode_id"].as_str().unwrap());
    let before = c.store.session_nodes(&sid).unwrap();
    assert_eq!(counts(&list(c).await, TAG), (1, 3));

    // Seq 3 and 5 are new (4 was a custom event the exporter skipped).
    let grown = sync(1, "openclaw-sync-2026-11", &[0, 1, 2, 3, 5]);
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(std::slice::from_ref(&grown))).await;
    assert_eq!(
        (got.imported, got.appended, got.skipped, got.nodes),
        (0, 1, 0, 2),
        "{got:?}"
    );
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    assert_eq!(
        c.store.stats().unwrap().frames_appended,
        frames + 1,
        "the nodes, the session and the counts ride one frame"
    );
    let after = c.store.session_nodes(&sid).unwrap();
    assert_eq!(after.len(), 5);
    assert_eq!(
        after[..3].iter().map(|n| n.0).collect::<Vec<_>>(),
        before.iter().map(|n| n.0).collect::<Vec<_>>(),
        "the stored nodes are untouched"
    );
    let ids: Vec<String> = after.iter().map(|n| n.1.id.clone()).collect();
    assert_eq!(ids[3], node_id_of(&sid, 3));
    assert_eq!(ids[4], node_id_of(&sid, 5));
    let rec: SessionRecord = c.store.get_session(&sid).unwrap().unwrap();
    let imp = rec.imported.as_deref().unwrap();
    assert_eq!(
        imp.tag, TAG,
        "an appended session keeps the tag it began under"
    );
    assert_eq!(
        (imp.messages, imp.hash.as_str()),
        (5, grown["hash"].as_str().unwrap())
    );
    assert!(imp.as_of.end_ms > imp.as_of.start_ms);
    assert_eq!(rec.last_active_ms, imp.as_of.end_ms);
    assert_eq!(counts(&list(c).await, TAG), (1, 5));
    assert!(
        list(c).await["tags"].as_array().unwrap().len() == 1,
        "no tag for the month the append came in"
    );

    // The same file again, and the older one, write nothing.
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(std::slice::from_ref(&grown))).await;
    assert_eq!((got.skipped, got.appended, got.nodes), (1, 0, 0), "{got:?}");
    let got = import(c, lines(&[first])).await;
    assert_eq!(got.appended, 0);
    assert!(
        got.rejected[0]
            .why
            .contains("message 3 is not in this episode"),
        "{:?}",
        got.rejected
    );
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
}

/// A rewritten message, a removed one, a reordered pair and a different first
/// message that add nothing after the last matching message are each taken
/// quietly (theseus-ectzg): nothing is written but the session's record,
/// which takes the new hash and the mark at the first differing idx, and the
/// next run skips it. **The same messages under another hash, diverged
/// nowhere, are taken the same quiet way but with no mark** (the owner's
/// word, 2026-11-10: the monthly import tag is in the hash and is the month
/// of the run, so refused it was offered and refused every night of the next
/// month, forever). Refused and named, moving nothing: a cut tail, which is
/// the stored session's own first messages and no more (an older view of it:
/// taken, it would move the hash back). With a new message after the last
/// match, a removal and a reorder append it and are marked.
#[tokio::test]
async fn a_rewrite_a_removal_and_a_reorder_are_taken_marked_and_not_offered_again() {
    let r = live();
    let c = &r.core;

    // Each on a session of its own, judged against that session as stored:
    // the messages it is sent, whether message 2 is rewritten, and the idx
    // the mark names.
    let cases: [(u64, Vec<u32>, bool, u32); 4] = [
        (2, vec![0, 1, 2, 3], true, 2),
        (12, vec![0, 2, 3], false, 1),
        (13, vec![0, 2, 1, 3], false, 1),
        (14, vec![1, 2, 3], false, 0),
    ];
    for (n, idxs, rewrite, at) in cases {
        import(c, lines(&[sync(n, TAG, &[0, 1, 2, 3])])).await;
        let frames = c.store.stats().unwrap().frames_appended;
        let mut e = sync(n, "openclaw-sync-2026-12", &idxs);
        if rewrite {
            e["messages"][2]["text"] = json!("Message 2: the tide log moved to the quay.");
        }
        e["hash"] = json!(episode::hash_of(&e));
        let got = import(c, lines(std::slice::from_ref(&e))).await;
        assert!(got.rejected.is_empty(), "{idxs:?}: {:?}", got.rejected);
        assert_eq!(
            (got.imported, got.appended, got.skipped, got.nodes),
            (0, 1, 0, 0),
            "{idxs:?}: {got:?}"
        );
        assert_eq!(c.store.stats().unwrap().frames_appended, frames + 1);
        let (rec, stored_idxs) = stored(c, n);
        assert_eq!(stored_idxs, vec![0, 1, 2, 3], "{idxs:?}: nothing written");
        let imp = rec.imported.as_deref().unwrap();
        assert_eq!(imp.diverged_at, vec![at], "{idxs:?}");
        assert_eq!(imp.hash, e["hash"].as_str().unwrap(), "{idxs:?}");
        assert!(imp.labels.topic.is_empty(), "{idxs:?}");
        let nodes = c.store.session_nodes(&rec.session_id).unwrap();
        let Body::Imported { text, .. } = &nodes[2].1.body else {
            panic!()
        };
        assert!(text.contains("boathouse"), "{idxs:?}: never overwritten");

        // The next run holds it: skipped, not rejected, nothing written.
        let frames = c.store.stats().unwrap().frames_appended;
        let got = import(c, lines(&[e])).await;
        assert!(got.rejected.is_empty(), "{idxs:?}: {:?}", got.rejected);
        assert_eq!(
            (got.skipped, got.appended, got.nodes),
            (1, 0, 0),
            "{idxs:?}: {got:?}"
        );
        assert_eq!(c.store.stats().unwrap().frames_appended, frames);
    }

    // The same messages under another hash, diverged nowhere: TAKEN the same
    // quiet way, and with no mark, because nothing diverged. The hash moves,
    // no node is written, the tag it was first imported with stays, and the
    // next run holds it.
    import(c, lines(&[sync(16, TAG, &[0, 1, 2, 3])])).await;
    let frames = c.store.stats().unwrap().frames_appended;
    let e = sync(16, "openclaw-sync-2026-12", &[0, 1, 2, 3]);
    let got = import(c, lines(std::slice::from_ref(&e))).await;
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    let counted = (got.imported, got.appended, got.skipped, got.nodes);
    assert_eq!(counted, (0, 1, 0, 0), "{got:?}");
    assert!(got.touched.is_empty(), "nothing new to the people step");
    assert_eq!(c.store.stats().unwrap().frames_appended, frames + 1);
    let (rec, stored_idxs) = stored(c, 16);
    assert_eq!(stored_idxs, vec![0, 1, 2, 3], "nothing written");
    let imp = rec.imported.as_deref().unwrap();
    assert_eq!(imp.hash, e["hash"].as_str().unwrap(), "the hash moved");
    assert!(imp.diverged_at.is_empty(), "nothing diverged: no mark");
    assert_eq!(imp.tag, TAG, "the tag it was first imported with");
    assert_eq!(imp.messages, 4, "the count did not move");

    // The next run holds it: skipped, not rejected, nothing written.
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(&[e])).await;
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    assert_eq!((got.skipped, got.appended, got.nodes), (1, 0, 0), "{got:?}");
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);

    // A cut tail, the stored messages' own first ones and no more: refused,
    // and the session keeps the hash it has, so what the source holds now is
    // still judged against it.
    let first = sync(15, TAG, &[0, 1, 2, 3]);
    import(c, lines(std::slice::from_ref(&first))).await;
    let frames = c.store.stats().unwrap().frames_appended;
    let mut cut = sync(15, "openclaw-sync-2026-12", &[0, 1, 2]);
    cut["hash"] = json!(episode::hash_of(&cut));
    let got = import(c, lines(&[cut])).await;
    assert_eq!((got.appended, got.nodes), (0, 0), "{got:?}");
    assert!(
        got.rejected[0]
            .why
            .contains("message 3 is not in this episode"),
        "{:?}",
        got.rejected
    );
    let imp = stored(c, 15).0.imported.unwrap();
    assert!(imp.diverged_at.is_empty());
    assert_eq!(imp.hash, first["hash"].as_str().unwrap(), "not moved back");
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);

    // With a message after the last match, a removal and a reorder append it
    // and are marked at the first stored idx that differs.
    for (idxs, at) in [(vec![0, 2, 3, 4], 1), (vec![0, 2, 1, 3, 4], 1)] {
        let n = 20 + at as u64 + idxs.len() as u64;
        let f = sync(n, TAG, &[0, 1, 2, 3]);
        import(c, lines(&[f])).await;
        let mut e = sync(n, "openclaw-sync-2026-12", &idxs);
        e["hash"] = json!(episode::hash_of(&e));
        let got = import(c, lines(&[e])).await;
        assert_eq!((got.appended, got.nodes), (1, 1), "{idxs:?}: {got:?}");
        let (rec, stored_idxs) = stored(c, n);
        assert_eq!(stored_idxs, vec![0, 1, 2, 3, 4], "{idxs:?}");
        let imp = rec.imported.as_deref().unwrap();
        assert_eq!(imp.diverged_at, vec![at], "{idxs:?}");
        assert!(imp.labels.topic.is_empty(), "{idxs:?}");
    }
}

/// **The quiet take of a tag-only episode is gated on the LENGTH, never on
/// nothing counting as added.** An episode whose stored prefix matches whole
/// and which carries one more message AT a stored idx adds nothing by that
/// count — `plan`'s `added` skips a stored idx — so a gate written
/// `added == 0` would take it, move the hash, and drop that message for good:
/// the next run would find the hash held and skip it. `plan` refuses it, by
/// `e.messages.len() == stored.len()`.
///
/// `plan` is called directly because this episode cannot be put on the wire:
/// `episode::parse` refuses an idx twice (`episode::messages_of`, "its idx
/// twice"), asserted below. That is a second guard, not this one — the length
/// test holds whatever a caller hands `plan`.
#[tokio::test]
async fn an_episode_that_gained_a_message_at_a_stored_idx_is_refused_not_taken() {
    let r = live();
    let c = &r.core;
    import(c, lines(&[sync(30, TAG, &[0, 1, 2, 3])])).await;
    let (rec, idxs) = stored(c, 30);
    assert_eq!(idxs, vec![0, 1, 2, 3]);

    // The same four messages under November's tag: taken (the plan is Ok and
    // writes no message node).
    let mut e = parsed(&sync(30, "openclaw-sync-2026-11", &[0, 1, 2, 3]));
    let p = super::append::plan(&c.store, &rec, &e)
        .unwrap()
        .expect("the tag-only episode is taken");
    assert_eq!((p.from, p.diverged, p.summary), (4, None, false));

    // The same episode plus one more message at stored idx 2, holding text
    // the store has not: refused. `added` is 0 for it either way.
    let mut extra = e.messages[2].clone();
    extra.text = "Message 2: the tide log moved to the quay.".into();
    extra.sha256 = hex::encode(sha2::Sha256::digest(extra.text.as_bytes()));
    e.messages.push(extra);
    let Err(why) = super::append::plan(&c.store, &rec, &e).unwrap() else {
        panic!("refused, never taken");
    };
    assert!(why.contains("at a stored idx"), "{why}");

    // And the same shape on the wire does not read at all.
    let mut v = sync(30, "openclaw-sync-2026-11", &[0, 1, 2, 3, 2]);
    v["messages"][4]["text"] = json!("Message 2: the tide log moved to the quay.");
    v["hash"] = json!(episode::hash_of(&v));
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(&[v])).await;
    assert_eq!((got.appended, got.nodes), (0, 0), "{got:?}");
    assert!(got.rejected[0].why.contains("its idx twice"), "{got:?}");

    // Nothing moved: the stored hash, the four nodes and message 2's text.
    let (after, idxs) = stored(c, 30);
    assert_eq!(idxs, vec![0, 1, 2, 3], "nothing written");
    assert_eq!(
        after.imported.as_deref().unwrap().hash,
        rec.imported.as_deref().unwrap().hash,
        "the stored hash did not move"
    );
    let nodes = c.store.session_nodes(&after.session_id).unwrap();
    let Body::Imported { text, .. } = &nodes[2].1.body else {
        panic!()
    };
    assert!(text.contains("boathouse"), "never overwritten");
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
}

/// The quiet take of a tag-only episode needs a source whose digests THIS
/// build took (`episode::DIGESTED_SOURCES`). For any other source the digests
/// are the client's claim and the import never checks them, so an episode
/// whose text changed while its declared digest did not would be taken and
/// its new text lost for good. It stays refused, as on main.
#[tokio::test]
async fn a_tag_only_episode_whose_digests_this_build_did_not_take_is_refused() {
    let r = live();
    let c = &r.core;
    // `openclaw-store` is not in DIGESTED_SOURCES: the episode's own sha256s
    // are stored as the client gave them.
    let mut first = sync(31, TAG, &[0, 1, 2, 3]);
    first["source"] = json!("openclaw-store");
    first["hash"] = json!(episode::hash_of(&first));
    import(c, lines(std::slice::from_ref(&first))).await;
    assert_eq!(stored(c, 31).1, vec![0, 1, 2, 3]);

    // Only the month's tag differs, and every digest matches: still refused.
    let frames = c.store.stats().unwrap().frames_appended;
    let mut next = first.clone();
    next["import_tag"] = json!("openclaw-sync-2026-11");
    next["hash"] = json!(episode::hash_of(&next));
    let got = import(c, lines(&[next])).await;
    assert_eq!((got.appended, got.nodes), (0, 0), "{got:?}");
    assert!(got.rejected[0].why.contains("adds nothing"), "{got:?}");
    let imp = stored(c, 31).0.imported.unwrap();
    assert_eq!(imp.hash, first["hash"].as_str().unwrap(), "the hash held");
    assert!(imp.diverged_at.is_empty());
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
}

/// `episode::parse` of `v` with each message's digest taken over its text, as
/// the import computes it for a harness source (`write::digested`).
fn parsed(v: &Value) -> episode::Episode {
    let mut e = episode::parse(&serde_json::to_string(v).unwrap()).expect("the episode reads");
    for m in &mut e.messages {
        m.sha256 = hex::encode(sha2::Sha256::digest(m.text.as_bytes()));
    }
    e
}

/// An erased session is never appended to.
#[tokio::test]
async fn an_erased_session_is_not_appended_to() {
    let r = live();
    let c = &r.core;
    let first = sync(3, TAG, &[0, 1, 2]);
    import(c, lines(std::slice::from_ref(&first))).await;
    call(c, method::IMPORT_ERASE, json!({"tag": TAG}))
        .await
        .unwrap();
    let grown = sync(3, TAG, &[0, 1, 2, 3]);
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(&[grown])).await;
    assert_eq!(
        (got.imported, got.appended, got.nodes),
        (0, 0, 0),
        "{got:?}"
    );
    assert!(
        got.rejected[0].why.contains("erased by import.erase"),
        "{got:?}"
    );
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
}

/// Every byte under `dir` holds `needle`.
fn holds(dir: &std::path::Path, needle: &str) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(b) = std::fs::read(&p) {
                if b.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                    return true;
                }
            }
        }
    }
    false
}

/// A sync episode is written as it came, and so is what is appended after
/// it: the key is in the node and in the store (the WAL). The append's prefix
/// check reads the stored digests, which are the stored text's, so the same
/// first messages sent again still match.
#[tokio::test]
async fn a_sync_episode_is_written_as_it_came_and_so_is_its_append() {
    let r = live();
    let c = &r.core;
    let first = sync(5, TAG, &[0, 1, 2]);
    let raw = first["messages"][1]["text"].as_str().unwrap().to_string();
    assert!(raw.contains(&token()), "the fixture plants it");
    import(c, lines(std::slice::from_ref(&first))).await;
    let sid = session_id_of(first["episode_id"].as_str().unwrap());
    let Body::Imported { text, sha256, .. } = &c.store.session_nodes(&sid).unwrap()[1].1.body
    else {
        panic!()
    };
    assert_eq!(text, &raw, "the key is stored as it came");
    assert_eq!(
        *sha256,
        hex::encode(<sha2::Sha256 as sha2::Digest>::digest(text.as_bytes())),
        "the stored digest is the stored text's"
    );
    // The same session grown, its first messages carrying the key again.
    let grown = sync(5, "openclaw-sync-2026-11", &[0, 1, 2, 3]);
    let got = import(c, lines(&[grown])).await;
    assert_eq!((got.appended, got.nodes), (1, 1), "{got:?}");
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    assert!(
        holds(c.store.dir(), &token()),
        "the store holds the key as it came"
    );
    assert!(
        holds(c.store.dir(), "boathouse ledger"),
        "the grep reads the store"
    );
}

/// The session `n` as the store holds it: its record and its message
/// nodes' idxs, in write order.
fn stored(c: &std::sync::Arc<crate::Core>, n: u64) -> (SessionRecord, Vec<u32>) {
    let sid = session_id_of(sync(n, TAG, &[0])["episode_id"].as_str().unwrap());
    let rec = c.store.get_session::<SessionRecord>(&sid).unwrap().unwrap();
    let idxs = c
        .store
        .session_nodes(&sid)
        .unwrap()
        .iter()
        .filter_map(|(_, x)| match &x.body {
            Body::Imported { idx, .. } => Some(*idx),
            _ => None,
        })
        .collect();
    (rec, idxs)
}

/// An episode of `0..len` with message 3's text rewritten.
fn rewritten_at_3(n: u64, tag: &str, len: u32) -> Value {
    let idxs: Vec<u32> = (0..len).collect();
    let mut e = sync(n, tag, &idxs);
    e["messages"][3]["text"] = json!("Message 3: the tide log was rewritten at the quay.");
    e["hash"] = json!(episode::hash_of(&e));
    e
}

/// A stored 10-message episode whose message 3 was rewritten, and which
/// gained messages 10 and 11 (the 11th and 12th), imports the two after the
/// last message that still matches, records the divergence once in a field
/// of its own and leaves `labels.topic` alone, and writes nothing a second
/// time (theseus-ectzg).
#[tokio::test]
async fn a_session_rewritten_at_3_that_grew_appends_the_new_turns_and_is_labelled() {
    let r = live();
    let c = &r.core;
    let first = sync(6, TAG, &(0..10).collect::<Vec<u32>>());
    import(c, lines(&[first])).await;
    let grown = rewritten_at_3(6, "openclaw-sync-2026-11", 12);
    let got = import(c, lines(std::slice::from_ref(&grown))).await;
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    assert_eq!((got.appended, got.nodes), (1, 2), "{got:?}");
    let (rec, idxs) = stored(c, 6);
    assert_eq!(idxs, (0..12).collect::<Vec<u32>>());
    let imp = rec.imported.as_deref().unwrap();
    assert_eq!(imp.messages, 12);
    assert_eq!(imp.diverged_at, vec![3], "the mark, in a field of its own");
    assert!(imp.labels.topic.is_empty(), "no topic is made of it");
    let sid = rec.session_id.clone();
    let nodes = c.store.session_nodes(&sid).unwrap();
    let Body::Imported { text, .. } = &nodes[3].1.body else {
        panic!()
    };
    assert!(
        text.contains("boathouse"),
        "the stored message 3 is not overwritten"
    );

    // The same input again: nothing is written twice, one mark.
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(&[grown])).await;
    assert_eq!((got.appended, got.nodes), (0, 0), "{got:?}");
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
    // A later growth appends only its new message and keeps the one mark.
    let mut more = rewritten_at_3(6, "openclaw-sync-2026-12", 13);
    more["hash"] = json!(episode::hash_of(&more));
    let got = import(c, lines(&[more])).await;
    assert_eq!((got.appended, got.nodes), (1, 1), "{got:?}");
    let (rec, idxs) = stored(c, 6);
    assert_eq!(idxs.len(), 13);
    let imp = rec.imported.as_deref().unwrap();
    assert_eq!(imp.diverged_at, vec![3]);
    assert!(imp.labels.topic.is_empty());
}

/// When no stored message matches, it is another episode under a reused id:
/// refused, after the stored messages were read.
#[tokio::test]
async fn an_episode_matching_no_stored_message_is_still_refused() {
    let r = live();
    let c = &r.core;
    let first = sync(7, TAG, &[0, 1, 2, 3]);
    import(c, lines(&[first])).await;
    let (_, before) = stored(c, 7);
    assert_eq!(before.len(), 4, "the stored messages exist");
    let mut other = sync(7, "openclaw-sync-2026-11", &[0, 1, 2, 3, 4, 5]);
    for i in 0..6 {
        other["messages"][i]["text"] = json!(format!("Another tale entirely, line {i}."));
    }
    other["hash"] = json!(episode::hash_of(&other));
    let got = import(c, lines(&[other])).await;
    assert_eq!((got.appended, got.nodes), (0, 0), "{got:?}");
    assert_eq!(got.rejected.len(), 1, "{got:?}");
    assert_eq!(stored(c, 7).1.len(), 4);
}

/// The mark is a stored field of its own: no key on a session that never
/// diverged (a record from before reads with none, and writes back the same
/// bytes), and after a divergence the key holds the idx while the labels the
/// pipeline gave are as they were (theseus-ectzg).
#[tokio::test]
async fn the_mark_is_a_stored_field_that_a_record_from_before_reads_without() {
    let r = live();
    let c = &r.core;
    import(c, lines(&[sync(9, TAG, &(0..10).collect::<Vec<u32>>())])).await;
    let (rec, _) = stored(c, 9);
    let before = serde_json::to_value(&rec).unwrap();
    assert!(
        before["imported"].get("diverged_at").is_none(),
        "no key until a divergence: {before}"
    );
    let old: SessionRecord = serde_json::from_value(before.clone()).unwrap();
    assert!(old.imported.as_deref().unwrap().diverged_at.is_empty());
    assert_eq!(serde_json::to_value(&old).unwrap(), before);

    import(c, lines(&[rewritten_at_3(9, "openclaw-sync-2026-11", 12)])).await;
    let (rec, _) = stored(c, 9);
    let after = serde_json::to_value(&rec).unwrap();
    assert_eq!(after["imported"]["diverged_at"], json!([3]), "{after}");
    assert_eq!(
        after["imported"]["labels"], before["imported"]["labels"],
        "the labels are the pipeline's, untouched"
    );
}

/// A stored 10-message episode whose message 3 was rewritten and which did
/// not grow is taken once, not refused: its record moves to the new hash
/// with the mark in its field, no node is written, and it is not offered
/// again the next run, so the nightly refusal count for it stays at zero
/// (theseus-ectzg: "the 49 refusals go to zero"). A later growth still
/// appends past the rewrite and keeps the one mark.
#[tokio::test]
async fn a_session_rewritten_at_3_that_did_not_grow_is_taken_once_and_never_offered_again() {
    let r = live();
    let c = &r.core;
    import(c, lines(&[sync(11, TAG, &(0..10).collect::<Vec<u32>>())])).await;
    let (before, idxs) = stored(c, 11);
    assert_eq!(idxs, (0..10).collect::<Vec<u32>>(), "the stored messages");
    let sid = before.session_id.clone();
    let nodes_before = c.store.session_nodes(&sid).unwrap();
    let frames = c.store.stats().unwrap().frames_appended;

    let quiet = rewritten_at_3(11, "openclaw-sync-2026-11", 10);
    let got = import(c, lines(std::slice::from_ref(&quiet))).await;
    assert!(got.rejected.is_empty(), "taken, not refused: {got:?}");
    assert_eq!(
        (got.imported, got.appended, got.skipped, got.nodes),
        (0, 1, 0, 0),
        "one take that wrote no message: {got:?}"
    );
    assert!(
        got.touched.is_empty(),
        "no new message for the people step: {got:?}"
    );
    assert_eq!(
        c.store.stats().unwrap().frames_appended,
        frames + 1,
        "the record, the counts and the batch's row ride one frame"
    );
    let (rec, idxs) = stored(c, 11);
    assert_eq!(idxs, (0..10).collect::<Vec<u32>>(), "no message written");
    let imp = rec.imported.as_deref().unwrap();
    assert_eq!(imp.hash, quiet["hash"].as_str().unwrap(), "the new hash");
    assert_eq!(imp.messages, 10);
    assert_eq!(imp.diverged_at, vec![3], "the mark, in its field");
    assert!(imp.labels.topic.is_empty(), "no topic is made of it");
    assert_eq!(imp.tag, TAG, "the session keeps the tag it began under");
    let nodes = c.store.session_nodes(&sid).unwrap();
    assert_eq!(
        nodes.iter().map(|n| n.0).collect::<Vec<_>>(),
        nodes_before.iter().map(|n| n.0).collect::<Vec<_>>(),
        "the stored nodes are untouched"
    );
    let Body::Imported { text, .. } = &nodes[3].1.body else {
        panic!()
    };
    assert!(
        text.contains("boathouse"),
        "the stored message 3 is not overwritten"
    );
    assert_eq!(counts(&list(c).await, TAG), (1, 10), "the tag's counts");

    // The next run, the same line: skipped, never refused, nothing written.
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(&[quiet])).await;
    assert!(got.rejected.is_empty(), "never refused again: {got:?}");
    assert_eq!(
        (got.imported, got.appended, got.skipped, got.nodes),
        (0, 0, 1, 0),
        "{got:?}"
    );
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);

    // A later growth appends past the rewrite and keeps the one mark.
    let got = import(c, lines(&[rewritten_at_3(11, "openclaw-sync-2026-12", 12)])).await;
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    assert_eq!((got.appended, got.nodes), (1, 2), "{got:?}");
    let (rec, idxs) = stored(c, 11);
    assert_eq!(idxs, (0..12).collect::<Vec<u32>>());
    let imp = rec.imported.as_deref().unwrap();
    assert_eq!((imp.messages, imp.diverged_at.clone()), (12, vec![3]));
}

/// The row's own bar in the shape of a sync: a night's file of sessions
/// stored before, one grown past a rewrite, one rewritten that did not grow,
/// one compacted (messages dropped from the middle), one reordered, one
/// unchanged and one cut to its first messages. Four are taken, one skipped,
/// and the only refusal is the cut one (an older view of the session, which
/// must not move its hash back); the night after takes none, skips the five
/// and refuses that one again (theseus-ectzg). An episode no stored message
/// of which matches stays refused too
/// (`an_episode_matching_no_stored_message_...`).
#[tokio::test]
async fn a_nights_rewritten_sessions_are_all_taken_and_none_refused_the_night_after() {
    let r = live();
    let c = &r.core;
    let ten: Vec<u32> = (0..10).collect();
    let before: Vec<Value> = (30..36).map(|n| sync(n, TAG, &ten)).collect();
    let got = import(c, lines(&before)).await;
    assert_eq!((got.imported, got.nodes), (6, 60), "{got:?}");

    let mut compacted = sync(32, "openclaw-sync-2026-11", &[0, 1, 2, 3, 8, 9]);
    compacted["hash"] = json!(episode::hash_of(&compacted));
    let mut reordered = sync(33, "openclaw-sync-2026-11", &[0, 1, 3, 2, 4, 5, 6, 7, 8, 9]);
    reordered["hash"] = json!(episode::hash_of(&reordered));
    let mut cut = sync(35, "openclaw-sync-2026-11", &[0, 1, 2, 3, 4]);
    cut["hash"] = json!(episode::hash_of(&cut));
    let night = vec![
        rewritten_at_3(30, "openclaw-sync-2026-11", 12),
        rewritten_at_3(31, "openclaw-sync-2026-11", 10),
        compacted,
        reordered,
        before[4].clone(),
        cut,
    ];
    let got = import(c, lines(&night)).await;
    assert_eq!(
        (got.imported, got.appended, got.skipped, got.nodes),
        (0, 4, 1, 2),
        "four taken, one skipped, two messages written: {got:?}"
    );
    assert_eq!(
        got.rejected.len(),
        1,
        "only the cut one: {:?}",
        got.rejected
    );
    assert_eq!(got.rejected[0].line, 6, "{:?}", got.rejected);
    assert!(
        got.rejected[0]
            .why
            .contains("message 5 is not in this episode"),
        "{:?}",
        got.rejected
    );
    assert_eq!(
        got.touched.len(),
        1,
        "only the grown one has anything new: {got:?}"
    );
    for (n, mark, messages) in [(30, vec![3], 12), (31, vec![3], 10), (32, vec![4], 10)] {
        let (rec, _) = stored(c, n);
        let imp = rec.imported.as_deref().unwrap();
        assert_eq!(
            (imp.diverged_at.clone(), imp.messages),
            (mark, messages),
            "session {n}"
        );
        assert!(imp.labels.topic.is_empty(), "session {n}");
    }
    let (rec, _) = stored(c, 33);
    assert_eq!(rec.imported.as_deref().unwrap().diverged_at, vec![2]);
    let (rec, _) = stored(c, 34);
    assert!(rec.imported.as_deref().unwrap().diverged_at.is_empty());
    let (rec, _) = stored(c, 35);
    let imp = rec.imported.as_deref().unwrap();
    assert!(imp.diverged_at.is_empty(), "the cut one is not a rewrite");
    assert_eq!(imp.hash, before[5]["hash"].as_str().unwrap(), "not moved");

    // The night after: the same file. The five held with their hashes are
    // skipped, nothing is written, and the one refusal is the cut session's.
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(&night)).await;
    assert_eq!(
        (got.imported, got.appended, got.skipped, got.nodes),
        (0, 0, 5, 0),
        "{got:?}"
    );
    assert_eq!(got.rejected.len(), 1, "{:?}", got.rejected);
    assert_eq!(got.rejected[0].line, 6, "{:?}", got.rejected);
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
}

/// An episode of `0..10` whose messages carry the texts `texts[i]` at idx `i`
/// (the source renumbered: texts move between idxs, never idxs alone).
fn with_texts(n: u64, tag: &str, texts: &[u32]) -> Value {
    let idxs: Vec<u32> = (0..texts.len() as u32).collect();
    let mut e = sync(n, tag, &idxs);
    for (k, t) in texts.iter().enumerate() {
        e["messages"][k]["text"] = json!(text(*t));
    }
    e["hash"] = json!(episode::hash_of(&e));
    e
}

/// A refused renumbering writes nothing and moves nothing.
async fn refused_and_untouched(c: &std::sync::Arc<crate::Core>, n: u64, e: Value, named: &str) {
    let frames = c.store.stats().unwrap().frames_appended;
    let got = import(c, lines(std::slice::from_ref(&e))).await;
    assert_eq!(got.rejected.len(), 1, "{got:?}");
    assert!(got.rejected[0].why.contains(named), "{:?}", got.rejected);
    assert_eq!((got.appended, got.nodes), (0, 0), "{got:?}");
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
    let (rec, idxs) = stored(c, n);
    assert_eq!(idxs, (0..10).collect::<Vec<u32>>(), "the positive count");
    let imp = rec.imported.as_deref().unwrap();
    assert_ne!(
        imp.hash,
        e["hash"].as_str().unwrap(),
        "the hash is not moved"
    );
    assert!(imp.diverged_at.is_empty());
}

/// A new message inserted at 3, the rest shifted by one: refused, not taken
/// with the new message lost and the last one written twice.
#[tokio::test]
async fn an_insertion_that_renumbers_is_refused_and_named() {
    let r = live();
    let c = &r.core;
    import(c, lines(&[sync(40, TAG, &(0..10).collect::<Vec<u32>>())])).await;
    let t = [0, 1, 2, 90, 3, 4, 5, 6, 7, 8, 9];
    refused_and_untouched(
        c,
        40,
        with_texts(40, "openclaw-sync-2026-11", &t),
        "message 4 holds what stored message 3 held",
    )
    .await;
}

/// A compaction that renumbers is refused, and stays refused when the
/// session then grows.
#[tokio::test]
async fn a_renumbered_compaction_is_refused_even_when_it_grows() {
    let r = live();
    let c = &r.core;
    import(c, lines(&[sync(41, TAG, &(0..10).collect::<Vec<u32>>())])).await;
    let t = [0, 1, 2, 6, 7, 8, 9];
    refused_and_untouched(
        c,
        41,
        with_texts(41, "openclaw-sync-2026-11", &t),
        "renumbered",
    )
    .await;
    let t = [0, 1, 2, 6, 7, 8, 9, 91];
    refused_and_untouched(
        c,
        41,
        with_texts(41, "openclaw-sync-2026-12", &t),
        "renumbered",
    )
    .await;
}

/// An insertion just before the last stored message: the last text moves to
/// an idx that is not stored, and is still caught.
#[tokio::test]
async fn an_insertion_before_the_last_stored_message_is_refused() {
    let r = live();
    let c = &r.core;
    import(c, lines(&[sync(42, TAG, &(0..10).collect::<Vec<u32>>())])).await;
    let t = [0, 1, 2, 3, 4, 5, 6, 7, 8, 92, 9];
    refused_and_untouched(
        c,
        42,
        with_texts(42, "openclaw-sync-2026-11", &t),
        "renumbered",
    )
    .await;
}

/// The guard: message 3 rewritten in place and a new idx 10 repeating an
/// old text exactly is no move (no stored message left its idx): appended.
#[tokio::test]
async fn an_in_place_rewrite_with_a_new_message_repeating_an_old_text_is_appended() {
    let r = live();
    let c = &r.core;
    import(c, lines(&[sync(43, TAG, &(0..10).collect::<Vec<u32>>())])).await;
    let t = [0, 1, 2, 93, 4, 5, 6, 7, 8, 9, 4];
    let got = import(c, lines(&[with_texts(43, "openclaw-sync-2026-11", &t)])).await;
    assert!(got.rejected.is_empty(), "{:?}", got.rejected);
    assert_eq!((got.appended, got.nodes), (1, 1), "{got:?}");
    let (rec, idxs) = stored(c, 43);
    assert_eq!(idxs, (0..11).collect::<Vec<u32>>());
    assert_eq!(rec.imported.as_deref().unwrap().diverged_at, vec![3]);
}
