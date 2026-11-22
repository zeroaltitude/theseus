//! An old store opens (issue 44, anything goes, 2026-11-22): the records the withholding wrote are no
//! longer written or honoured, and every one still decodes as it was stored. Here: what the gate gave a
//! message-less turn (`given.<turn>`) and where a split or a claim moved a node (`moved.of.<node>`). The
//! others have their own: a release and its end (`release::tests::a_stored_release_and_its_end_still_decode`),
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
