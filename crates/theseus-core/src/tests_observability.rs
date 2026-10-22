//! The data rule on every route row and provider call (theseus-a1wvq.11):
//! `route.decided`'s data verdict, admission, gate, provider, model and
//! origin; `provider.call`'s origin and per-request pass; the turn's footer
//! words; and the gate's counter. The rig is `tests_open_request`'s.

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;

use crate::provider::Scripted;
use crate::telemetry::tests::Receiver;
use crate::telemetry::{Telemetry, TelemetryConfig};
use crate::tests_judge::kinds;
use crate::tests_open_request::{dm, jev, rig_with, say, OPEN_A, PRIVATE};
use crate::tests_route::{call, decided, mode, rig, turn};

fn calls(core: &crate::Core) -> Vec<Value> {
    kinds(&core.store, "provider.call")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// The key test (T29). In the owner's DM, a message judged `open` whose mode
/// is `trivial` runs on `dsflash`: its route row says what Jev answered, that
/// the turn was admitted, and where it ran. A message judged `private` stays
/// on a US model, and its row says why `dsflash` was left out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_decided_records_data_clearance_gate_provider_model_origin() {
    let jev = jev(&[OPEN_A]);
    let (r, _zai, _dir) = rig_with(&jev, None, |_| {});
    say(&r.core, &dm(&r.core), OPEN_A, None).await;
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    assert_ne!(res.profile, "dsflash");
    let rows = decided(&r.core.store);
    let (a, b) = (&rows[0], &rows[1]);
    assert_eq!(
        (a["data"].as_str(), a["clearance"].as_str()),
        (Some("open"), Some("admitted")),
        "{a}"
    );
    assert!(a["data_confidence"].as_f64().unwrap() > 0.9, "{a}");
    assert_eq!(
        (
            a["profile"].as_str(),
            a["provider"].as_str(),
            a["model"].as_str(),
            a["origin"].as_str()
        ),
        (
            Some("dsflash"),
            Some("zai"),
            Some("glm-5.3-flash"),
            Some("cn")
        ),
        "{a}"
    );
    assert!(a.get("data_gate").is_none(), "{a}");
    assert_eq!(
        (b["data"].as_str(), b["clearance"].as_str()),
        (Some("private"), Some("closed")),
        "{b}"
    );
    assert_eq!(b["data_gate"], "verdict_private", "{b}");
    assert_eq!(b["origin"], "us", "{b}");
    assert_eq!(b["model"], res.model.as_str(), "{b}");
    assert_eq!(b["provider"], res.provider.as_str(), "{b}");
}

/// A plain Claude turn whose mode lists no restricted profile gains the
/// provider, model, origin and its admission, and no `data_gate`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plain_turns_route_row_gains_only_where_it_ran() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, "Talk to me about the weather.", None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(d["origin"], "us", "{d}");
    assert_eq!(d["model"], res.model.as_str(), "{d}");
    assert_eq!(d["provider"], res.provider.as_str(), "{d}");
    assert_eq!(d["clearance"], "closed", "{d}");
    // Jev's data answer is the fake's default; nothing gated the turn.
    assert!(
        d.get("data_confidence").is_some() && d.get("data_gate").is_none(),
        "{d}"
    );
    let t = res.route.unwrap().data.expect("data");
    assert_eq!(
        (t.origin.as_str(), t.gate, t.clearance.as_str()),
        ("us", None, "closed")
    );
}

/// The floor moving a turn off its pinned restricted base writes the
/// admission's word as `data_gate`: `private_view` for an admitted turn that
/// cannot be built open, `verdict_private` for one judged private.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_floor_move_names_its_why() {
    let jev = jev(&[OPEN_A]);
    mode(&jev, "chat", 0.95);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    for (text, why) in [(OPEN_A, "private_view"), (PRIVATE, "verdict_private")] {
        say(&r.core, &dm(&r.core), text, None).await;
        let d = decided(&r.core.store).pop().unwrap();
        assert_eq!(
            (d["reason"].as_str(), d["data_gate"].as_str()),
            (Some("data_gate"), Some(why)),
            "{d}"
        );
        assert_eq!(d["origin"], "us", "{d}");
    }
    assert!(zai.requests().is_empty());
}

/// An open DM request's call says it went to a `cn` model with a pass and
/// how many nodes it carried; a Claude call says its origin alone; and the
/// loop after a tool result, moved off the restricted model, names its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_call_records_origin_and_the_pass_per_request() {
    let jev = jev(&[OPEN_A]);
    let script = vec![
        Scripted::tools(
            "Looking.",
            &[("f1", "tools_find", json!({"query": "clock"}))],
        ),
        Scripted::text("Done."),
    ];
    let (r, zai, _dir) = rig_with(&jev, Some(script), |_| {});
    let sid = dm(&r.core);
    say(&r.core, &sid, OPEN_A, None).await;
    let sent = zai.requests();
    let rows = calls(&r.core);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let (first, moved) = (&rows[0], &rows[1]);
    assert_eq!(
        (first["origin"].as_str(), first["pass"].as_bool()),
        (Some("cn"), Some(true)),
        "{first}"
    );
    assert_eq!(first["run_nodes"], sent[0].messages.len(), "{first}");
    assert_eq!(moved["origin"], "us", "{moved}");
    assert!(
        moved.get("pass").is_none() && moved.get("run_nodes").is_none(),
        "{moved}"
    );
}

/// An admitted turn that runs on the `[model]` default (no request carries a
/// pass) says `admitted`, never `open`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_turns_footer_says_open_only_when_a_request_carried_a_pass() {
    let jev = jev(&[OPEN_A]);
    mode(&jev, "chat", 0.95);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    let res = say(&r.core, &dm(&r.core), OPEN_A, None).await;
    assert!(zai.requests().is_empty());
    let t = res.route.unwrap().data.expect("data");
    assert_eq!(t.clearance, "admitted", "{t:?}");
    assert!(!t.footer().unwrap_or_default().contains("open"));
    // And a request that did carry a pass is the open one.
    let jev = jev_open();
    let (r, _zai, _dir) = rig_with(&jev, None, |_| {});
    let res = say(&r.core, &dm(&r.core), OPEN_A, None).await;
    let t = res.route.unwrap().data.expect("data");
    assert_eq!(
        (t.clearance.as_str(), t.footer().as_deref()),
        ("open", Some("open"))
    );
}

fn jev_open() -> FakeJev {
    jev(&[OPEN_A])
}

/// A turn the data rule kept off `dsflash` counts once on
/// `theseus.route.data_gate`, by caller and why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_data_gate_turn_counts_once_on_the_route_data_gate_metric() {
    let rx = Receiver::start(vec![]).await;
    let jev = jev(&[OPEN_A]);
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let (r, _zai, _dir) =
        crate::tests_open_request::rig_telemetry(&jev, Telemetry::from_config(&tel, None).unwrap());
    say(&r.core, &dm(&r.core), PRIVATE, None).await;
    assert!(
        r.core
            .telemetry()
            .flush(std::time::Duration::from_secs(10))
            .await
    );
    let got = rx.at("/v1/metrics");
    let metrics = got.last().expect("an export").body["resourceMetrics"][0]["scopeMetrics"][0]
        ["metrics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let m = metrics
        .iter()
        .find(|m| m["name"] == "theseus.route.data_gate")
        .expect("the counter");
    let points = m["sum"]["dataPoints"].as_array().unwrap();
    assert_eq!(points.len(), 1, "{points:?}");
    let attr = |k: &str| {
        points[0]["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["key"] == k)
            .unwrap()["value"]["stringValue"]
            .clone()
    };
    assert_eq!(
        (
            attr("theseus.data_gate.what"),
            attr("theseus.data_gate.why")
        ),
        (json!("route"), json!("verdict_private"))
    );
    let n = &points[0]["asInt"];
    assert!(n == 1 || n == "1", "{n}");
}
