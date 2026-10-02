//! A node, as stored, decodes and re-encodes byte for byte (theseus-0g4,
//! finding 12; every kind since theseus-k52m): a tool-call node's gate record
//! above all, which the gate wrote as a JSON map with its keys sorted.
//! `fixtures/stored_gates.jsonl` holds one node of each gate shape found in
//! the stores on this machine, invented names; given `THESEUS_STORE_COPY`, a
//! copy of a store directory, the ignored test does the same for every node in
//! it, of whatever kind.
//!
//! An assistant message's `cost_usd` once re-encoded a digit shorter in 5 of
//! the 105 nodes of a copy of the operator's store: serde_json's default float
//! parsing may land one ULP from the value a float was written from, and its
//! printer then wrote the neighbour's shorter form. The workspace turns on
//! serde_json's `float_roundtrip`, and the tests below hold it.

use std::path::Path;

use serde_json::Value;
use theseus_core::node::{Body, Node};

/// A stored node's bytes, decoded and encoded again, which must be the same
/// bytes, whatever the node's kind. `Ok(true)` for a tool-call node, whose gate
/// is also held alone; `Ok(false)` for any other.
fn reencode(payload: &[u8]) -> Result<bool, String> {
    let node: Node = serde_json::from_slice(payload).map_err(|e| format!("decode: {e}"))?;
    let again = serde_json::to_vec(&node).map_err(|e| format!("encode: {e}"))?;
    if again != payload {
        return Err(format!(
            "re-encoded differently:\n stored {}\n again  {}",
            String::from_utf8_lossy(payload),
            String::from_utf8_lossy(&again)
        ));
    }
    let Body::ToolCall { gate, .. } = &node.body else {
        return Ok(false);
    };
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

/// Floats a node holds as written: costs from prices and token counts, and
/// the one the operator's store held that re-parsed one ULP away
/// (`0.025932800000000002` read back as `0.0259328`).
const COSTS: [f64; 8] = [
    0.025_932_800_000_000_002,
    0.1 + 0.2,
    1.0 / 3.0,
    1_282_500.0 / 1e6,
    0.000_123_456_789,
    2.5e-7,
    99.578_395,
    0.018_900_000_000_000_003,
];

/// A float reads as the value it was written from, whatever its digits.
#[test]
fn a_float_reads_back_as_the_value_it_was_written_from() {
    for f in COSTS {
        let written = serde_json::to_string(&f).unwrap();
        let read: f64 = serde_json::from_str(&written).unwrap();
        assert_eq!(read.to_bits(), f.to_bits(), "{written} read as {read:?}");
    }
}

/// An assistant node's `cost_usd` re-encodes identically, as does a node of
/// every other kind: the byte-for-byte rule is for every node, not a tool
/// call's alone.
#[test]
fn a_node_of_every_kind_reencodes_unchanged() {
    use theseus_core::node::ResultStatus;
    let mut nodes = vec![
        Node::user("ses_k52m", Some("trn_1"), "operator", "what did that cost?"),
        Node::tool_result(
            "ses_k52m",
            Some("trn_1"),
            Some(0),
            Body::ToolResult {
                tool_use_id: "t1".into(),
                tool: "fs.read".into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "1\tdone\n".into(),
                correlation_id: Some("act_1".into()),
                bytes_total: 6,
                truncated: false,
                full_ref: None,
                duration_ms: Some(3),
                late: false,
                meta: serde_json::json!({"cost": 0.025_932_800_000_000_002f64}),
                image: None,
                external: None,
            },
        ),
    ];
    for f in COSTS {
        nodes.push(Node::assistant(
            "ses_k52m",
            "trn_1",
            1,
            Body::AssistantMessage {
                blocks: vec![serde_json::json!({"type": "text", "text": "about that much"})],
                model: "invented-model".into(),
                provider: "invented".into(),
                stop_reason: Some("end_turn".into()),
                usage: theseus_protocol::Usage::default(),
                cost_usd: Some(f),
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
            },
        ));
    }
    for n in &nodes {
        let stored = serde_json::to_vec(n).unwrap();
        if let Err(e) = reencode(&stored) {
            panic!("a {} node: {e}", n.kind_str());
        }
        let back: Node = serde_json::from_slice(&stored).unwrap();
        assert_eq!(&back, n, "a {} node decodes as it was", n.kind_str());
    }
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
