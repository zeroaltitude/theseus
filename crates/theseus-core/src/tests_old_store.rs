//! An old store opens (issue 44, anything goes, 2026-11-22): the records the withholding and the threads
//! wrote are no longer written or honoured, and every one still decodes as it was stored. Here: what the
//! gate gave a message-less turn (`given.<turn>`), where a split or a claim moved a node (`moved.of.<node>`),
//! a split's record (`thread.split.<id>`), a thread's record with its seal, its pending seal or its merge
//! (`thread:<slug>`), the card a thread's compilation kept (`thread.card.<compilation id>`, issue 46), and the
//! `thread.*` ledger rows. The others have their own: a release and its end (`release::tests::a_stored_release_and_its_end_still_decode`),
//! a fold node over a sealed source (`ladder::tests::an_old_node_over_a_sealed_source_still_reads_and_is_shown_as_it_stands`).

use serde_json::json;
use theseus_memory::gate::Present;

use crate::store::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

/// A `given.<turn>` record, as the gate wrote one for a wake that continued an earlier turn: it reads
/// back whole, its presence by name, and a word this build does not know reads as `Unknown`.
#[test]
fn an_old_given_record_still_decodes() {
    let (_d, s) = store();
    let key = |t: &str| format!("{}{t}", crate::exchange::given::GIVEN_PREFIX);
    s.put_meta(
        &key("trn_wake"),
        &json!({"turn_id": "trn_wake", "present": "private_channel", "continues": "trn_first"}),
    )
    .unwrap();
    s.put_meta(
        &key("trn_odd"),
        &json!({"turn_id": "trn_odd", "present": "a_word_from_later"}),
    )
    .unwrap();
    let g = crate::exchange::given::given(&s, "trn_wake")
        .unwrap()
        .expect("it reads");
    assert_eq!(
        (g.turn_id.as_str(), g.present(), g.continues.as_deref()),
        ("trn_wake", Present::PrivateChannel, Some("trn_first"))
    );
    let odd = crate::exchange::given::given(&s, "trn_odd")
        .unwrap()
        .expect("it reads");
    assert_eq!((odd.present(), odd.continues), (Present::Unknown, None));
    assert!(crate::exchange::given::given(&s, "trn_none")
        .unwrap()
        .is_none());
}

/// A split's record (`thread.split.<id>`), as an older build stored it, one written before `undone` existed and one
/// undone: each reads back whole through its typed decoder.
#[test]
fn an_old_split_record_still_decodes() {
    let (_d, s) = store();
    let key = crate::moved::split_key;
    s.put_meta(
        &key("spl_1"),
        &json!({"id": "spl_1", "from": "home@local", "source_session": "ses_home", "moved_node": "mov_1",
                "thread": "kelp", "target_session": "ses_kelp", "opened": true,
                "target_nodes": ["msg_brief", "arr_1"], "moved": 3, "first": 10, "last": 14,
                "gist": "the kelp survey", "at_ms": 1_790_000_000_000u64}),
    )
    .unwrap();
    s.put_meta(
        &key("spl_2"),
        &json!({"id": "spl_2", "from": "kelp", "source_session": "ses_kelp", "moved_node": "mov_2",
                "thread": "tides", "target_session": "ses_tides", "opened": false, "target_nodes": [],
                "moved": 1, "first": 20, "last": 20, "gist": "a tide line", "at_ms": 1_790_000_000_001u64,
                "undone": true}),
    )
    .unwrap();
    let read = |id: &str| {
        s.get_meta::<crate::moved::SplitRecord>(&key(id))
            .unwrap()
            .expect("the record")
    };
    let a = read("spl_1");
    assert_eq!(key("spl_1"), "thread.split.spl_1");
    assert_eq!(
        (
            a.thread.as_str(),
            a.moved,
            a.first,
            a.last,
            a.opened,
            a.undone
        ),
        ("kelp", 3, 10, 14, true, false),
        "a record from before `undone` reads as not undone"
    );
    assert_eq!(a.target_nodes, ["msg_brief", "arr_1"]);
    let b = read("spl_2");
    assert!(b.undone && !b.opened && b.target_nodes.is_empty());
}

/// A `moved.of.<node>` record, a split's and a claim's, and one an undo nulled: each reads as stored.
#[test]
fn an_old_move_record_still_decodes() {
    let (_d, s) = store();
    let key = crate::moved::moved_of_key;
    s.put_meta(&key("nd_a"), &json!({"thread": "kelp", "split": "spl_1"}))
        .unwrap();
    s.put_meta(&key("nd_b"), &json!({"thread": "kelp", "split": "claim_2"}))
        .unwrap();
    s.put_meta(&key("nd_c"), &serde_json::Value::Null).unwrap();
    let read = |n: &str| {
        s.get_meta::<Option<crate::moved::MovedTo>>(&key(n))
            .unwrap()
    };
    let a = read("nd_a").flatten().expect("a split's");
    assert_eq!((a.thread.as_str(), a.is_claim()), ("kelp", false));
    assert!(read("nd_b").flatten().expect("a claim's").is_claim());
    assert_eq!(read("nd_c"), Some(None), "an undone move reads as null");
}

/// A thread's record as the threads wrote one: sealed, with a seal proposed, merged into another, done. Each
/// decodes whole; the daemon's board reads these records
/// (`rpc::tests_thread_list::an_old_stores_threads_read_unchanged_and_a_threads_session_runs_as_any_other`).
#[test]
fn an_old_thread_record_with_its_seal_or_its_merge_still_decodes() {
    use theseus_protocol::threads::{ThreadInfo, ThreadState};
    let read = |v: serde_json::Value| serde_json::from_value::<ThreadInfo>(v).unwrap();
    let sealed = read(json!({"slug": "kelp", "title": "Kelp", "topics": ["sea"],
        "sealed": {"by": "cli", "at_ms": 1, "reason": "embargo"}}));
    assert_eq!(
        sealed.sealed.as_ref().map(|s| s.reason.as_str()),
        Some("embargo")
    );
    let pending =
        read(json!({"slug": "tide", "title": "Tide", "seal_pending": "it names the vault"}));
    assert_eq!(pending.seal_pending.as_deref(), Some("it names the vault"));
    let merged = read(
        json!({"slug": "old", "title": "Old", "state": "done", "merged_into": "kelp",
        "split_from": "tide", "links": ["tide"]}),
    );
    assert_eq!(
        (
            merged.state,
            merged.merged_into.as_deref(),
            merged.links.len()
        ),
        (ThreadState::Done, Some("kelp"), 1)
    );
}

/// The card snapshot a thread's compilation kept (`thread.card.<compilation id>`), which nothing writes or
/// sends any more (issue 46): it reads back as the text it was stored as.
#[test]
fn an_old_thread_card_snapshot_still_decodes() {
    let (_d, s) = store();
    let card = "[Thread card: kelp\nGoal: the survey\nWhere it stands:\n- the north grid is done]";
    s.put_meta("thread.card.cmp_old", &card.to_string())
        .unwrap();
    let back: Option<String> = s.get_meta("thread.card.cmp_old").unwrap();
    assert_eq!(back.as_deref(), Some(card));
}

/// The `thread.*` ledger rows an older build wrote: each decodes and its kind is still known, so the
/// ledger's readers narrate it.
#[test]
fn an_old_thread_ledger_row_still_decodes() {
    for kind in [
        "thread.created",
        "thread.sealed",
        "thread.seal_proposed",
        "thread.merged",
        "thread.split",
        "thread.routed",
        "thread.moved",
    ] {
        let text = format!(
            r#"{{"at_unix_ms":1,"kind":"{kind}","session_id":"ses_a","data":{{"slug":"kelp"}}}}"#
        );
        let row: crate::ledger::LedgerRow = serde_json::from_str(&text).unwrap();
        assert_eq!(row.kind, kind);
        assert!(
            theseus_protocol::LedgerKind::parse(&row.kind).is_some(),
            "{kind} is still a known kind"
        );
    }
}
