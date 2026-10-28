//! A session that grew (theseus-pjmj7): the same episode id with another
//! hash whose messages begin with the stored ones appends only the new
//! nodes, with the session's record and the tag's counts in the same frame;
//! a changed, removed or reordered message is refused and named by its
//! idx; an erased session is never appended to.

use serde_json::{json, Value};
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

/// A rewritten message, a removed one, a reordered pair and a different
/// first message are each refused, naming the first differing idx, and
/// nothing is written.
#[tokio::test]
async fn a_rewrite_a_removal_and_a_reorder_are_refused_and_named() {
    let r = live();
    let c = &r.core;
    let first = sync(2, TAG, &[0, 1, 2, 3]);
    import(c, lines(std::slice::from_ref(&first))).await;
    let sid = session_id_of(first["episode_id"].as_str().unwrap());
    let frames = c.store.stats().unwrap().frames_appended;

    let mut rewritten = sync(2, TAG, &[0, 1, 2, 3, 4]);
    rewritten["messages"][2]["text"] = json!("Message 2: the tide log moved to the quay.");
    rewritten["hash"] = json!(episode::hash_of(&rewritten));
    let mut removed = sync(2, TAG, &[0, 2, 3, 4]);
    removed["hash"] = json!(episode::hash_of(&removed));
    let reordered = sync(2, TAG, &[0, 2, 1, 3, 4]);
    let mut other_first = sync(2, TAG, &[1, 2, 3, 4]);
    other_first["hash"] = json!(episode::hash_of(&other_first));
    let got = import(c, lines(&[rewritten, removed, reordered, other_first])).await;
    assert_eq!(
        (got.imported, got.appended, got.nodes),
        (0, 0, 0),
        "{got:?}"
    );
    let why: Vec<&str> = got.rejected.iter().map(|x| x.why.as_str()).collect();
    assert_eq!(why.len(), 4, "{why:?}");
    // The batch holds the same id four times, each judged against the
    // stored session: a refused one takes nothing into the batch.
    assert!(why[0].contains("message 2 changed"), "{why:?}");
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
    assert_eq!(c.store.session_nodes(&sid).unwrap().len(), 4);
    let Body::Imported { text, .. } = &c.store.session_nodes(&sid).unwrap()[2].1.body else {
        panic!()
    };
    assert!(text.contains("boathouse"), "never overwritten");

    // Each alone, so each is judged against the stored session.
    let cases = [
        (
            vec![0, 2, 3, 4],
            "message 2 is not where the stored episode has it",
        ),
        (
            vec![0, 2, 1, 3, 4],
            "message 2 is not where the stored episode has it",
        ),
        (
            vec![1, 2, 3, 4],
            "message 1 is not where the stored episode has it",
        ),
        (vec![0, 1, 2], "message 3 is not in this episode"),
        (vec![0, 1, 2, 3], "adds nothing"),
    ];
    for (idxs, want) in cases {
        let mut e = sync(2, "openclaw-sync-2026-12", &idxs);
        e["hash"] = json!(episode::hash_of(&e));
        let got = import(c, lines(&[e])).await;
        assert_eq!((got.appended, got.nodes), (0, 0), "{idxs:?}: {got:?}");
        assert!(
            got.rejected[0].why.contains(want),
            "{idxs:?}: {:?} lacks {want:?}",
            got.rejected
        );
    }
    assert_eq!(c.store.stats().unwrap().frames_appended, frames);
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
