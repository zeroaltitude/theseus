//! The warm-up metric's outcomes (theseus-tnky, theseus-fpwqg): a first
//! keystroke's warm-up that the data floor refuses is counted as `refused`,
//! apart from one that was opened and one that failed, through a whole core
//! whose telemetry posts to a receiver.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::{Message, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::tests::{attrs_of, flushed, last_metrics, pipeline, points_of, tuning, Receiver};
use crate::approval::{Client, Surface};
use crate::config::content_based_security::CnModels;
use crate::provider::FakeProvider;
use crate::session::TargetRef;
use crate::store::Store;
use crate::tests_recall::session;
use crate::{Config, Core};

/// One `session.typing` on the CLI's surface, answered.
async fn typed(core: &Arc<Core>, session_id: &str) {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let client = Client::new("sock#1", Surface::Cli);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(
        theseus_protocol::Id::Num(1),
        "session.typing",
        json!({ "session_id": session_id }),
    );
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            assert!(r.result.is_some(), "{:?}", r.error);
            break;
        }
    }
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
}

/// The provider warm-ups by outcome, as the last export counts them.
async fn provider_warms(core: &Core, rx: &Receiver) -> BTreeMap<String, u64> {
    flushed(core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    points_of(&metrics, "theseus.warm")
        .into_iter()
        .map(|p| (attrs_of(p), p["asInt"].as_str().unwrap().parse().unwrap()))
        .filter(|(a, _)| a["theseus.warm.target"] == "provider")
        .map(|(a, n)| (a["theseus.warm.outcome"].clone(), n))
        .collect()
}

/// Under `cn_models = "never"` a warm-up for the session's next turn on a
/// model of restricted origin is refused by the floor, and the metric says
/// `refused`; a warm-up to an unrestricted provider is counted `opened`.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_warm_up_is_counted_as_refused() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.content_based_security.cn_models = CnModels::Never;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let us = Arc::new(FakeProvider::default());
    let cn = Arc::new(FakeProvider::default());
    let core = Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(&rx.endpoint(), None, tuning())),
        providers: {
            let mut p = std::collections::BTreeMap::new();
            p.insert(cfg.model.provider.clone(), us.clone() as _);
            p.insert("zai".into(), cn.clone() as _);
            p
        },
        ..crate::rpc::Parts::for_tests(cfg, us.clone(), store)
    })
    .unwrap();

    let open = session(&core, None, &[]);
    let restricted = session(&core, None, &[]);
    let mut rec: crate::session::SessionRecord =
        core.store.get_session(&restricted).unwrap().unwrap();
    rec.last_target = Some(TargetRef {
        profile: "glm".into(),
        provider: "zai".into(),
        model: "glm-5.3".into(),
    });
    core.store.put_session(&restricted, &rec).unwrap();

    typed(&core, &open).await;
    typed(&core, &restricted).await;
    let mut seen = BTreeMap::new();
    for _ in 0..200 {
        seen = provider_warms(&core, &rx).await;
        if seen.values().sum::<u64>() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let want: BTreeMap<String, u64> =
        [("opened".to_string(), 1), ("refused".to_string(), 1)].into();
    assert_eq!(seen, want, "one opened, one refused: {seen:?}");
    assert_eq!(cn.warms.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(us.warms.load(std::sync::atomic::Ordering::SeqCst), 1);
    let rows = crate::tests_judge::kinds(&core.store, "data_gate.blocked");
    assert_eq!(rows.len(), 1, "the floor wrote the refusal's row");
}

/// The floor's refusal and the typing metric's `refused` outcome are one
/// phrase, held once in `data_gate::RESTRICTED_ORIGIN` (the owner,
/// 2026-11-07). A real refusal is driven through a whole core, and the
/// message the floor actually produced is asserted against that constant
/// rather than a phrase re-typed here: so a reworded refusal fails on the
/// constant, instead of quietly dropping the `refused` outcome to `failed`
/// because `rpc::typing`'s match no longer recognises the words.
#[tokio::test(flavor = "multi_thread")]
async fn the_refusals_words_and_the_refused_outcome_are_one_constant() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.content_based_security.cn_models = CnModels::Never;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let us = Arc::new(FakeProvider::default());
    let cn = Arc::new(FakeProvider::default());
    let core = Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(&rx.endpoint(), None, tuning())),
        providers: {
            let mut p = std::collections::BTreeMap::new();
            p.insert(cfg.model.provider.clone(), us.clone() as _);
            p.insert("zai".into(), cn.clone() as _);
            p
        },
        ..crate::rpc::Parts::for_tests(cfg, us.clone(), store)
    })
    .unwrap();

    // One session, whose next turn would run on a model of restricted
    // origin, so the only warm-up this core does is the refused one.
    let restricted = session(&core, None, &[]);
    let mut rec: crate::session::SessionRecord =
        core.store.get_session(&restricted).unwrap().unwrap();
    rec.last_target = Some(TargetRef {
        profile: "glm".into(),
        provider: "zai".into(),
        model: "glm-5.3".into(),
    });
    core.store.put_session(&restricted, &rec).unwrap();

    typed(&core, &restricted).await;
    let mut seen = BTreeMap::new();
    for _ in 0..200 {
        seen = provider_warms(&core, &rx).await;
        if seen.values().sum::<u64>() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The positive bar: the label is `refused`, and it fired exactly once.
    let want: BTreeMap<String, u64> = [("refused".to_string(), 1)].into();
    assert_eq!(seen, want, "one warm-up, counted `refused`: {seen:?}");
    assert_eq!(cn.warms.load(std::sync::atomic::Ordering::SeqCst), 0);

    // And the words that earned that label are the floor's own constant.
    let stamp = core
        .warmth
        .health()
        .provider
        .expect("the provider warm-up was stamped");
    let why = match stamp.outcome.strip_prefix("failed: ") {
        Some(why) => why,
        None => panic!("a refused warm-up is a failed outcome: {}", stamp.outcome),
    };
    assert!(
        why.contains(crate::data_gate::RESTRICTED_ORIGIN),
        "the floor's refusal must carry `{}`, the words `rpc::typing` knows it by; it said: {why}",
        crate::data_gate::RESTRICTED_ORIGIN,
    );
}
