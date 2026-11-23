//! Fixtures the tests of imported history share: episodes and their import, a stand-in words index over
//! sessions' nodes, and a model that answers "Done." (moved here from `memory.lookup`'s tests when that tool
//! went, D6 step 6).

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use crate::import::episode;
use crate::node::Node;
use crate::provider::{ProviderRequest, Scripted};
use crate::recall::{Ask, AskFuture};
use crate::Core;

/// What the veiled episodes say: no answer may carry it.
pub(crate) const HIDDEN: &str = "the lighthouse keeper's letters";
/// What the episodes the veil leaves alone say.
pub(crate) const SHOWN: &str = "the tide-table parser";

/// An episode of `sensitivity`, its one message at `at` (RFC 3339), its
/// summary saying `says`.
pub(crate) fn episode(n: usize, sensitivity: &str, at: &str, says: &str) -> Value {
    let mut v = json!({
        "format": 1, "import_tag": "kelp-2026", "episode_id": format!("ep_{:064x}", 0xbeef + n),
        "source": "claude-cli", "agent": "main",
        "place": {"kind": "cli", "name": format!("desk-{n}")},
        "as_of": {"start": at, "end": at},
        "labels": {"sensitivity": sensitivity, "topic": ["kelp/notes"], "book_hint": "casebook"},
        "summary": {"text": format!("Episode {n}: {says}."), "cites": [0], "model": "claude-opus-5-5"},
        "messages": [{"idx": 0, "time": at, "author": "wren", "integrity": "operator",
                      "text": format!("Message {n}: {says}."), "unit": format!("u-{n}"),
                      "sha256": "ab".repeat(32)}],
    });
    v["hash"] = json!(episode::hash_of(&v));
    v
}

/// Five episodes: four in March 2026 (one of each sensitivity, the veiled
/// ones saying `HIDDEN`), one in September 2026.
pub(crate) fn episodes() -> Vec<Value> {
    vec![
        episode(0, "personal", "2026-03-03T18:00:00Z", HIDDEN),
        episode(1, "company-confidential", "2026-03-10T18:00:00Z", SHOWN),
        episode(2, "partner-confidential", "2026-03-17T18:00:00Z", HIDDEN),
        episode(3, "public", "2026-03-24T18:00:00Z", SHOWN),
        episode(
            4,
            "public",
            "2026-09-15T18:00:00Z",
            "the September renderer",
        ),
    ]
}

pub(crate) fn import(core: &Core, episodes: &[Value]) -> Vec<String> {
    let p = ImportEpisodesParams {
        file: "kelp.jsonl".into(),
        lines: episodes
            .iter()
            .enumerate()
            .map(|(i, e)| ImportLine {
                line: i as u64 + 1,
                text: serde_json::to_string(e).unwrap(),
            })
            .collect(),
        ..Default::default()
    };
    let r = crate::import::write::import_batch(&core.store, &p, "cli").unwrap();
    assert_eq!(r.imported, episodes.len() as u64, "{r:?}");
    episodes
        .iter()
        .map(|e| crate::import::session_id_of(e["episode_id"].as_str().unwrap()))
        .collect()
}

/// A model that answers "Done.", to a person's message and to a call's result alike.
pub(crate) fn script(_req: &ProviderRequest) -> Scripted {
    Scripted::text("Done.")
}

/// A stand-in index over every imported node: each with its own time,
/// whatever the span (the core keeps to it).
pub(crate) fn index_over(core: &Arc<Core>, sessions: Vec<String>) -> Ask {
    let store = core.store.clone();
    Arc::new(move |_p| -> AskFuture {
        let nodes: Vec<(u64, Node)> = sessions
            .iter()
            .flat_map(|s| store.session_nodes(s).unwrap())
            .collect();
        let hits = nodes
            .into_iter()
            .enumerate()
            .map(|(i, (position, n))| IndexHit {
                text: crate::recall::text_of(&n),
                time_ms: n.created_at_ms,
                node_id: n.id,
                chunk: 0,
                session_id: n.session_id,
                position,
                kind: "imported".into(),
                origin: "import".into(),
                author: None,
                place: None,
                tool: None,
                external: false,
                entities_matched: vec![],
                entities: Vec::new(),
                sources: [(
                    "bm25".to_string(),
                    IndexSourceRank {
                        rank: i + 1,
                        score: 1.0,
                    },
                )]
                .into(),
                fused: 1.0 / (61 + i) as f64,
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}
