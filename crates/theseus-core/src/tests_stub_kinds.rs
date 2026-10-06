//! A stub's kind agrees with its body's (theseus-q0qe). A stub the node
//! cache already holds takes its kind from `Kind::of` itself, so a wrong
//! `Kind::of` arm agrees with itself there; only a stub read from the
//! record's bytes, by the peek at the serde tag, can tell. So each sample
//! node is written through a store, and read back as a stub by a handle that
//! never decoded it (a fresh cache: `decodes()` stays 0, and no stub is
//! hydrated).

use serde_json::json;
use theseus_protocol::Usage;

use crate::compiler::compaction::is_summary;
use crate::consolidate::{CitationCheck, Stage};
use crate::node::{Body, Node, ResultStatus};
use crate::store::Store;
use crate::stub::Kind;

const SID: &str = "ses_wren";

/// The sample's number: every `Body` variant has one, and the match names
/// each (no wildcard), so a variant added without a sample fails to compile
/// here.
fn number(b: &Body) -> usize {
    match b {
        Body::UserMessage { .. } => 0,
        Body::AssistantMessage { .. } => 1,
        Body::ToolCall { .. } => 2,
        Body::ToolResult { .. } => 3,
        Body::Recall { .. } => 4,
        Body::Arrangement { .. } => 5,
        Body::Summary { .. } => 6,
        Body::Synthesis { .. } => 7,
        Body::Imported { .. } => 8,
        Body::ImportedSummary { .. } => 9,
        Body::Erased { .. } => 10,
    }
}

const VARIANTS: usize = 11;

fn sample(i: usize) -> Node {
    match i {
        0 => Node::user(SID, Some("trn_1"), "test", "the wren sings at dawn"),
        1 => Node::assistant(
            SID,
            "trn_1",
            0,
            Body::AssistantMessage {
                blocks: vec![json!({"type": "text", "text": "so it does"})],
                model: "glm-4.6".into(),
                provider: "glm".into(),
                stop_reason: Some("end_turn".into()),
                usage: Usage::default(),
                cost_usd: None,
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
            },
        ),
        2 => Node::tool_call(
            SID,
            Some("trn_1"),
            Some(0),
            Body::ToolCall {
                tool_use_id: "toolu_wren".into(),
                tool: "fs.read".into(),
                wire_name: "fs_read".into(),
                input: json!({"path": "notes/wren.md"}),
                assistant_node: String::new(),
                correlation_id: None,
                gate: None,
            },
        ),
        3 => Node::tool_result(
            SID,
            Some("trn_1"),
            Some(0),
            Body::ToolResult {
                tool_use_id: "toolu_wren".into(),
                tool: "fs.read".into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "wrens nest low".into(),
                correlation_id: None,
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: serde_json::Value::Null,
                image: None,
                external: None,
            },
        ),
        4 => Node::recall(SID, "trn_1", "rcl_wren", "baseline", Vec::new()),
        5 => Node::arrangement(SID, "parent", Vec::new(), false),
        6 => Node::summary(
            SID,
            "trn_2",
            Body::Summary {
                first: 3,
                last: 7,
                nodes: 4,
                text: "The wren sings at dawn.".into(),
                profile: "glm".into(),
                model: "glm-4.6".into(),
                cost_usd: None,
                header: "[Summary of 4 earlier messages, written by glm]".into(),
            },
        ),
        7 => Node::synthesis(
            SID,
            Body::Synthesis {
                text: "The wren sings at dawn [1].".into(),
                sources: vec!["msg_x".into()],
                check: CitationCheck::Unchecked {
                    why: "the judge is off".into(),
                },
                stage: Stage::Shadow,
                cluster: "ff".into(),
                profile: "glm".into(),
                model: "glm".into(),
                cost_usd: None,
            },
        ),
        8..=10 => import_sample(i),
        _ => unreachable!(),
    }
}

/// The import's three bodies (theseus-0lrr.6): a message, a summary citing
/// it, and another message's tombstone.
fn import_sample(i: usize) -> Node {
    match i {
        8 => Node::imported(
            "imp_wren_0".into(),
            SID,
            "wren",
            1_746_194_591_000,
            Body::Imported {
                text: "the wren sings at dawn".into(),
                integrity: crate::import::Integrity::Operator,
                source: "wiki".into(),
                unit: "u0".into(),
                sha256: "ab".repeat(32),
                idx: 0,
            },
        ),
        9 => Node::imported(
            "imp_wren_summary".into(),
            SID,
            "summary:m",
            1_746_194_592_000,
            Body::ImportedSummary {
                text: "A wren's dawn song.".into(),
                cites: vec!["imp_wren_0".into()],
                model: "m".into(),
            },
        ),
        10 => Node::imported(
            "imp_wren_1".into(),
            SID,
            "wren",
            1_746_194_593_000,
            Body::Imported {
                text: "wrens nest low".into(),
                integrity: crate::import::Integrity::Outside,
                source: "wiki".into(),
                unit: "u1".into(),
                sha256: "cd".repeat(32),
                idx: 1,
            },
        )
        .erased(1_790_000_000_000, "import.erase of reef-2026"),
        _ => unreachable!(),
    }
}

/// The kind `Kind::of` gives each variant, written by hand: what a stub from
/// the bytes must say.
fn expected(i: usize) -> Kind {
    [
        Kind::UserMessage,
        Kind::AssistantMessage,
        Kind::ToolCall,
        Kind::ToolResult,
        Kind::Recall,
        Kind::Arrangement,
        Kind::Summary,
        Kind::Synthesis,
        Kind::Imported,
        Kind::ImportedSummary,
        Kind::Erased,
    ][i]
}

/// Every body's stub, read from its record's bytes, has `Kind::of`'s kind;
/// only the summary is a summary, and it knows its range's end.
#[test]
fn a_stubs_kind_agrees_with_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let samples: Vec<Node> = (0..VARIANTS).map(sample).collect();
    let mut seen: Vec<usize> = samples.iter().map(|n| number(&n.body)).collect();
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..VARIANTS).collect::<Vec<_>>(),
        "a body lacks a sample"
    );
    {
        let store = Store::open(&path).unwrap();
        for n in &samples {
            store.append(&[n.record().unwrap()]).unwrap();
        }
    }
    // A handle whose cache never saw the nodes: every stub is from a peek.
    let store = Store::open(&path).unwrap();
    let transcript = store.transcript(SID).unwrap();
    assert_eq!(transcript.len(), VARIANTS);
    for ((_, stub), n) in transcript.iter().zip(&samples) {
        let want = Kind::of(&n.body);
        assert_eq!(want, expected(number(&n.body)), "{}", n.kind_str());
        assert_eq!(stub.id, n.id);
        assert!(!stub.is_hydrated(), "{} was decoded", n.kind_str());
        assert_eq!(
            stub.kind,
            want,
            "the stub of a {} says {:?}",
            n.kind_str(),
            stub.kind
        );
        assert_eq!(
            is_summary(stub),
            n.kind_str() == "summary",
            "{}",
            n.kind_str()
        );
        assert_eq!(is_summary(n), n.kind_str() == "summary", "{}", n.kind_str());
        assert_eq!(
            stub.summary_last,
            match &n.body {
                Body::Summary { last, .. } => Some(*last),
                _ => None,
            }
        );
    }
    assert_eq!(store.node_cache().decodes(), 0, "a peek is no decode");
}
