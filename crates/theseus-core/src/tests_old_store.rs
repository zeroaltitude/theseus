//! An old store opens (issue 44, anything goes, 2026-11-22): the records the withholding and the threads
//! wrote are no longer written or honoured, and every one still decodes as it was stored. Here: what the
//! gate gave a message-less turn (`given.<turn>`), where a split or a claim moved a node (`moved.of.<node>`),
//! a thread's record with its seal, its pending seal or its merge (`thread:<slug>`), and the `thread.*`
//! ledger rows. The others have their own: a release and its end (`release::tests::a_stored_release_and_its_end_still_decode`),
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
/// decodes whole; `thread list` and `thread show` read these records
/// (`rpc::tests_thread_list::list_and_show_read_the_stored_records`).
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
