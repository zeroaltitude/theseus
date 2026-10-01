//! A tool-call node, as stored, decodes and re-encodes byte for byte
//! (theseus-0g4, finding 12): its gate record above all, which the gate wrote
//! as a JSON map with its keys sorted. `fixtures/stored_gates.jsonl` holds one
//! node of each gate shape found in the stores on this machine, invented
//! names; given `THESEUS_STORE_COPY`, a copy of a store directory, the ignored
//! test does the same for every tool-call node in it.
//!
//! Only tool-call nodes: an assistant message's `cost_usd` can re-encode a
//! digit shorter, since serde_json's default float parsing (no
//! `float_roundtrip`) may land one ULP away. That was so before this test.

use std::path::Path;

use serde_json::Value;
use theseus_core::node::{Body, Node};

/// A stored tool-call node's bytes, decoded and encoded again, and its gate
/// alone; `Ok(false)` for any other node.
fn reencode(payload: &[u8]) -> Result<bool, String> {
    let node: Node = serde_json::from_slice(payload).map_err(|e| format!("decode: {e}"))?;
    let Body::ToolCall { gate, .. } = &node.body else {
        return Ok(false);
    };
    let again = serde_json::to_vec(&node).map_err(|e| format!("encode: {e}"))?;
    if again != payload {
        return Err(format!(
            "re-encoded differently:\n stored {}\n again  {}",
            String::from_utf8_lossy(payload),
            String::from_utf8_lossy(&again)
        ));
    }
    // The gate alone, in its canonical form: its keys sorted, as stored.
    let stored: Value = serde_json::from_slice(payload).unwrap();
    let gate = serde_json::to_value(gate).unwrap().to_string();
    assert_eq!(gate, stored["body"]["gate"].to_string(), "the gate alone");
    Ok(true)
}

#[test]
fn every_stored_gate_shape_reencodes_unchanged() {
    let rows = include_str!("fixtures/stored_gates.jsonl");
    let mut calls = 0;
    for (i, row) in rows.lines().enumerate() {
        match reencode(row.as_bytes()) {
            Ok(call) => calls += usize::from(call),
            Err(e) => panic!("row {}: {e}", i + 1),
        }
    }
    assert_eq!(calls, 6);
}

#[test]
#[ignore = "reads THESEUS_STORE_COPY, a copy of a store's directory"]
fn a_store_copys_nodes_reencode_unchanged() {
    let dir = std::env::var("THESEUS_STORE_COPY").expect("THESEUS_STORE_COPY");
    // Open a copy of the copy's log: opening may truncate a torn tail.
    let wal = tempfile::tempdir().unwrap();
    for e in std::fs::read_dir(Path::new(&dir).join("wal")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), wal.path().join(e.file_name())).unwrap();
    }
    let log = theseus_store::Wal::open(wal.path(), theseus_store::WalConfig::default()).unwrap();
    let (mut nodes, mut calls, mut failed) = (0, 0, Vec::new());
    for (r, _) in log.replay_from(0).unwrap() {
        if r.kind != theseus_store::kinds::NODE {
            continue;
        }
        nodes += 1;
        match reencode(&r.payload) {
            Ok(call) => calls += usize::from(call),
            Err(e) => failed.push(format!("position {}: {e}", r.position)),
        }
    }
    eprintln!(
        "{nodes} nodes, {calls} tool calls, {} re-encoded differently",
        failed.len()
    );
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
