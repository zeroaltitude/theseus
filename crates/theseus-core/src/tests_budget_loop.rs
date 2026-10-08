//! The loop that ends on the budget question writes its `loop.ended` row
//! (theseus-nhg4), as every other loop's end does: before, it sent the
//! notification alone, and the ledger showed the loop start and never end.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{SessionKind, Usage};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The template's live profile reserves its 128,000-token output cap at $10
/// per million ($1.28) and its input estimate at $2: a $1.40 limit fits the
/// first call, not the second once the first's 30,000 tokens out are spent.
#[tokio::test]
async fn the_loop_that_asks_the_budget_question_writes_its_loop_ended_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.kernel.spend_limit_usd = 1.40;
    cfg.kernel.spend_limit_mode = crate::config::SpendLimitMode::Ask;
    let first = Scripted::Billed {
        usage: Usage {
            input_tokens: 2_000,
            output_tokens: 30_000,
            ..Usage::default()
        },
        then: Box::new(Scripted::tools(
            "Diffing.",
            &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        )),
    };
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![first]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let res = core
        .runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("diff these".into()),
            target,
            sink: EventSink::new(core.bus.clone(), &sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
    assert_eq!(
        (res.stop_reason.as_str(), res.loops),
        ("budget", 2),
        "loop 1's call did not fit: {res:?}"
    );
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(1000).unwrap();
    let of = |kind: &str| {
        rows.iter()
            .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(sid.as_str()))
            .map(|(_, r)| r.data.clone())
            .collect::<Vec<_>>()
    };
    let started = of("loop.started");
    let ended = of("loop.ended");
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(ended.len(), 2, "every loop that started ended: {ended:?}");
    assert_eq!(
        ended[1],
        json!({
            "loop": 1,
            "outcome": {"loop_index": 1, "provider_stop_reason": null, "tool_calls": 0, "output_chars": 0},
            "advancer": "budget",
            "decision": {"decision": "budget"},
            "usage": {"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 0,
                      "cache_creation_input_tokens": 0},
        })
    );
}
