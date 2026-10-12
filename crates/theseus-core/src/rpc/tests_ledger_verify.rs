//! `ledger.verify` (theseus-bmkxk): the chain's walk through the core: a whole
//! chain answers its rows and head, health shows the head's short form, a
//! head recorded earlier is checked, and a bad anchor is refused. The finding
//! of each kind of tampering is `theseus-store`'s `store/tests_chain.rs`.

use serde_json::json;
use theseus_protocol::error_code;

use super::tests::test_core;

fn put(c: &super::Core, n: u64) {
    let row =
        json!({"at_unix_ms": 1_790_000_000_000u64 + n, "kind": "turn.started", "data": {"n": n}});
    c.store.append_ledger(&row).unwrap();
}

#[tokio::test]
async fn a_whole_chain_answers_its_rows_head_and_start() {
    let c = test_core("ok");
    for n in 0..5 {
        put(&c, n);
    }
    let v = c.ledger_verify(serde_json::Value::Null).await.unwrap();
    assert!(v.ok && v.broken.is_none(), "{v:?}");
    let head = v.head.clone().expect("a chain has a head");
    assert!(v.rows >= 6, "the start row and the five rows: {}", v.rows);
    assert_eq!(head.digest.len(), 64);
    assert!(v.since_ms.is_some());
    // The head the writer claims is the head the walk recomputed.
    let claimed = c.ledger_verify(json!({"head_only": true})).await.unwrap();
    assert_eq!(claimed.head, v.head);
    // Health shows its short form.
    assert_eq!(c.health().ledger_head, Some(head.short()));
    // The walk against the head it just found: whole.
    let anchored = c
        .ledger_verify(json!({"anchor": format!("{}:{}", head.seq, &head.digest[..16])}))
        .await
        .unwrap();
    assert!(anchored.ok, "{anchored:?}");
}

#[tokio::test]
async fn a_head_the_chain_does_not_hold_is_named_rewritten_or_truncated() {
    let c = test_core("ok");
    for n in 0..3 {
        put(&c, n);
    }
    let v = c.ledger_verify(serde_json::Value::Null).await.unwrap();
    let head = v.head.unwrap();
    let other = "ab".repeat(8);
    let r = c
        .ledger_verify(json!({"anchor": format!("{}:{other}", head.seq)}))
        .await
        .unwrap();
    assert!(!r.ok);
    assert_eq!(r.broken.unwrap().fault, "rewritten");
    let ahead = c
        .ledger_verify(json!({"anchor": format!("{}:{other}", head.seq + 5)}))
        .await
        .unwrap();
    let b = ahead.broken.unwrap();
    assert_eq!(b.fault, "truncated");
    assert!(b.detail.contains("5 row(s) are missing"), "{}", b.detail);
}

#[tokio::test]
async fn an_anchor_that_is_not_one_is_refused() {
    let c = test_core("ok");
    for bad in ["nope", "7", "x:abcdef12", "7:zz", "7:abc"] {
        let e = c.ledger_verify(json!({"anchor": bad})).await.unwrap_err();
        assert_eq!(e.code, error_code::INVALID_PARAMS, "{bad}");
    }
}
