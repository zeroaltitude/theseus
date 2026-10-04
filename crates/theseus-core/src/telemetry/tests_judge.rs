//! The judge's metrics (M5 23b; design §2.13, "Telemetry"), through a whole
//! core against the fake Jev, read back from the OTLP test receiver: each
//! metric's name and attributes, as the judge's sink records them once its
//! frame is written.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};

use super::export::Tuning;
use super::tests::{attrs_of, last_metrics, pipeline, point_with, points_of, tuning, Receiver};
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, config, texts, turn, until_judged};

/// The attribute keys of every point of `name`.
fn keys_of(metrics: &[Value], name: &str) -> Vec<Vec<String>> {
    points_of(metrics, name)
        .into_iter()
        .map(|p| attrs_of(p).into_keys().collect())
        .collect()
}

/// Two judged turns, the second with Jev down: the calls by pack, mode,
/// band, and class; the call's time and the time on the turn's path by pack
/// and class; the failure by its class; the disagreement by pack; and the
/// judge's dollars as `theseus.cost.usd` with `theseus.spend = judge`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_judges_metrics_carry_their_names_and_attributes() {
    let rx = Receiver::start(vec![]).await;
    let jev = FakeJev::start().unwrap();
    jev.script(
        "work_state",
        Jev::Choice {
            option: "progressing".into(),
            confidence: 0.95,
        },
    );
    jev.script("announced_unfinished", Jev::Noul(0.97));
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), Some(&jev));
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = std::sync::Arc::new(crate::provider::FakeProvider::scripted(texts(2)));
    let mut p = Parts::for_tests(cfg, fake, store);
    p.secrets = board();
    p.telemetry = Some(pipeline(
        &rx.endpoint(),
        None,
        Tuning {
            interval: Duration::from_millis(200),
            ..tuning()
        },
    ));
    let core = Core::build(p).unwrap();
    let first = turn(&core, None, "Say done.").await;
    until_judged(&core.store, 1).await;
    jev.set_mode(FakeMode::Down);
    turn(&core, Some(&first.session_id), "Again.").await;
    let rows = until_judged(&core.store, 2).await;
    assert_eq!(rows[1].1.data["outcome"]["class"], "network");
    let got = rx
        .until("the judge's metrics", |g| {
            points_of(&last_metrics(g), "theseus.judge.calls").len() == 2
                && !points_of(&last_metrics(g), "theseus.judge.errors").is_empty()
        })
        .await;
    let m = last_metrics(&got);

    let judge = |k: &str| format!("theseus.judge.{k}");
    let calls: Vec<String> = ["band", "class", "mode", "pack"].map(judge).into();
    assert_eq!(keys_of(&m, "theseus.judge.calls"), [calls.clone(), calls]);
    let pack = ("theseus.judge.pack", "loop.v1");
    let answered = [
        pack,
        ("theseus.judge.mode", "shadow"),
        ("theseus.judge.band", "act"),
        ("theseus.judge.class", "reply"),
    ];
    assert_eq!(
        point_with(&m, "theseus.judge.calls", &answered)["asInt"],
        "1"
    );
    let failed = [pack, ("theseus.judge.band", "failed")];
    assert_eq!(point_with(&m, "theseus.judge.calls", &failed)["asInt"], "1");

    let by_class: Vec<String> = ["class", "pack"].map(judge).into();
    for name in ["theseus.judge.duration_ms", "theseus.judge.on_path_ms"] {
        assert_eq!(keys_of(&m, name), std::slice::from_ref(&by_class), "{name}");
        let p = point_with(&m, name, &[pack, ("theseus.judge.class", "reply")]);
        assert_eq!(p["count"], "2", "{name}: both judgments");
    }
    let on_path = point_with(&m, "theseus.judge.on_path_ms", &[pack]);
    assert_eq!(
        on_path["sum"], 0.0,
        "a shadow judgment keeps no turn waiting"
    );

    let errors = point_with(
        &m,
        "theseus.judge.errors",
        &[("theseus.error.class", "network")],
    );
    assert_eq!(errors["asInt"], "1");
    assert_eq!(
        keys_of(&m, "theseus.judge.errors"),
        [["theseus.error.class"]]
    );
    let disagreed = point_with(&m, "theseus.judge.disagreements", &[pack]);
    assert_eq!(disagreed["asInt"], "1", "the answered one disagrees");

    let spend = point_with(&m, "theseus.cost.usd", &[("theseus.spend", "judge")]);
    let cost = rows[0].1.data["cost_micros"].as_u64().unwrap() as f64 / 1e6;
    assert!(cost > 0.0);
    assert_eq!(spend["asDouble"].as_f64(), Some(cost));
    assert_eq!(
        attrs_of(spend),
        BTreeMap::from([
            ("theseus.judge.pack".to_string(), "loop.v1".to_string()),
            ("theseus.spend".to_string(), "judge".to_string()),
        ])
    );
}
