//! Tool families' use as a metric (theseus-7m7w.2): `theseus.tools.events`, by
//! event (used, searched, missed, unused), place kind and family, counted
//! where the turn's `tools.used` row and the miss's row are written.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::SessionKind;

use super::tests::{attrs_of, flushed, last_metrics, pipeline, points_of, tuning, Receiver};
use crate::bus::EventSink;
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// A stand-in model: asked to open a terminal it was never offered (the CLI
/// defers the terminals), it calls it; once a call is answered, it answers.
struct Model;

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let answered = req.messages.iter().any(|m| {
                m["content"]
                    .as_array()
                    .is_some_and(|b| b.iter().any(|b| b["type"] == "tool_result"))
            });
            let next = if answered {
                Scripted::text("done")
            } else {
                Scripted::tools("", &[("t1", "term_open", json!({"argv": ["true"]}))])
            };
            FakeProvider::scripted(vec![next])
                .stream_message(req, on_delta)
                .await
        })
    }
}

#[tokio::test]
async fn a_miss_and_the_loaded_families_nothing_used_are_counted_by_place_and_family() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, Arc::new(Model), store);
    parts.cfg.tools.defer.enabled = true;
    parts.telemetry = Some(pipeline(&rx.endpoint(), None, tuning()));
    let core = Core::build(parts).unwrap();

    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            answer_at: None,
            session: rec,
            input: Some("open a terminal".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();

    flushed(core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    let got: Vec<(String, String, String, u64)> = points_of(&metrics, "theseus.tools.events")
        .into_iter()
        .map(|p| {
            let a = attrs_of(p);
            let n = p["asInt"].as_str().unwrap().parse().unwrap();
            (
                a["theseus.tools.event"].clone(),
                a["theseus.tools.place"].clone(),
                a["theseus.tools.family"].clone(),
                n,
            )
        })
        .collect();
    let has = |e: &str, f: &str| {
        got.iter()
            .any(|(ev, pl, fam, n)| ev == e && pl == "cli" && fam == f && *n == 1)
    };
    assert!(has("missed", "terminals"), "{got:?}");
    assert!(
        has("unused", "memory"),
        "memory was loaded and nothing used it: {got:?}"
    );
    assert!(
        !got.iter()
            .any(|(e, _, f, _)| e == "unused" && f == "terminals"),
        "{got:?}"
    );
}
