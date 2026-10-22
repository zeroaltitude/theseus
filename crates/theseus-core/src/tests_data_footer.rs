//! When the route row and the footer say `data gate` (theseus-4h85l): only
//! where the data rule kept a turn off a restricted-origin model it would
//! otherwise have taken; an open-only turn says `open` and no route reason;
//! the `sensitive` word end to end. The rig is `tests_open_request`'s.

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;

use crate::catalog::ModelOrigin;
use crate::telemetry::tests::Receiver;
use crate::telemetry::{Telemetry, TelemetryConfig};
use crate::tests_open_request::{dm, jev, say, OPEN_A, PRIVATE};
use crate::tests_route::{call, decided, mode, rig_parts, turn};

/// `theseus.route.data_gate`'s points after a flush: (what, why, count).
pub(crate) async fn gate_counts(core: &crate::Core, rx: &Receiver) -> Vec<(Value, Value, Value)> {
    let flushed = core
        .telemetry()
        .flush(std::time::Duration::from_secs(10))
        .await;
    assert!(flushed);
    let got = rx.at("/v1/metrics");
    let metrics = got
        .last()
        .map(|g| g.body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"].clone())
        .and_then(|m| m.as_array().cloned())
        .unwrap_or_default();
    let Some(m) = metrics
        .iter()
        .find(|m| m["name"] == "theseus.route.data_gate")
    else {
        return vec![];
    };
    let attr = |p: &Value, k: &str| {
        p["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["key"] == k)
            .unwrap()["value"]["stringValue"]
            .clone()
    };
    m["sum"]["dataPoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                attr(p, "theseus.data_gate.what"),
                attr(p, "theseus.data_gate.why"),
                p["asInt"].clone(),
            )
        })
        .collect()
}

fn count_is_one(n: &Value) -> bool {
    n == 1 || n == "1"
}

pub(crate) async fn telemetry() -> (Receiver, Telemetry) {
    let rx = Receiver::start(vec![]).await;
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let t = Telemetry::from_config(&tel, None).unwrap();
    (rx, t)
}

/// The key test. A private turn in a mode that lists a China-origin profile
/// is gated only when that profile would have won on a cleared ask. With
/// Haiku ahead of it in the list, the US model wins either way: no gate on
/// the row, no footer, nothing counted. With it first, the gate shows, the
/// footer says it, and the counter counts it once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_turn_a_us_model_wins_anyway_shows_no_data_gate() {
    let (rx, tel) = telemetry().await;
    let jev = jev(&[OPEN_A]);
    let dir = tempfile::tempdir().unwrap();
    let (r, _zai, _d) = rig_with_telemetry(&jev, &dir, tel, |c| {
        c.routing.classes.trivial.profiles = vec!["haiku".into(), "cheapest".into()];
        c.routing.classes.trivial.cn_after = Some("haiku".into());
    });
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(res.profile, "haiku", "{d}");
    assert_eq!(d["clearance"], "closed", "{d}");
    assert!(d.get("data_gate").is_none(), "{d}");
    let data = res.route.unwrap().data.expect("data");
    assert_eq!((data.gate.clone(), data.footer()), (None, None));
    assert_eq!(gate_counts(&r.core, &rx).await, vec![]);

    let (rx, tel) = telemetry().await;
    let (r, _zai, _d) = rig_with_telemetry(&jev, &dir, tel, |_| {});
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(d["data_gate"], "verdict_private", "{d}");
    let data = res.route.unwrap().data.expect("data");
    assert_eq!(data.footer().as_deref(), Some("data gate: verdict_private"));
    let counts = gate_counts(&r.core, &rx).await;
    assert_eq!(counts.len(), 1, "{counts:?}");
    assert_eq!(
        (&counts[0].0, &counts[0].1),
        (&json!("route"), &json!("verdict_private"))
    );
    assert!(count_is_one(&counts[0].2), "{counts:?}");
}

/// `tests_open_request::rig_telemetry`, with a tweak.
fn rig_with_telemetry(
    jev: &FakeJev,
    dir: &tempfile::TempDir,
    tel: Telemetry,
    tweak: impl FnOnce(&mut crate::Config),
) -> (
    crate::tests_route::Rig,
    std::sync::Arc<crate::provider::FakeProvider>,
    (),
) {
    let notes = dir.path().join("notes.md");
    std::fs::write(&notes, "the vault code is 4417\n").unwrap();
    let r = rig_parts(
        Some(jev),
        8,
        |c| {
            crate::tests_open_request::config(c, &notes);
            tweak(c);
        },
        move |p| p.telemetry = Some(tel),
    );
    let zai = r.zai.clone();
    (r, zai, ())
}

/// A turn that asked no route but whose request carried a pass says `open`
/// and no route reason: the route it takes has none for a surface to word
/// (the CLI and Discord word a route from its mode or reason alone).
#[test]
fn an_open_only_turn_shows_no_route_reason() {
    let mut result = None;
    crate::turn::data_step::open_route(&mut result, "dsflash", "cn");
    let route = result.expect("a route");
    assert!(route.reason.is_empty(), "{route:?}");
    assert_eq!(route.mode, None);
    let data = route.data.expect("data");
    assert_eq!(
        (data.footer().as_deref(), data.origin.as_str()),
        (Some("open"), "cn")
    );
}

/// `sensitive`, end to end. A pinned profile served from an EU host is of no
/// restricted origin, so only the names' floor keeps the turn off it: the
/// default runs, the row says `sensitive`, and the counter counts it once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_name_gate_is_on_the_row_and_counted_once() {
    let (rx, tel) = telemetry().await;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig_parts(
        Some(&jev),
        1,
        |c| {
            c.privacy.sensitive_names = vec!["Examplecorp".into()];
            c.serve_glm_unrestricted();
            c.vouch_zai(ModelOrigin::Eu);
        },
        move |p| p.telemetry = Some(tel),
    );
    call(&r.core, "profile.use", json!({"name": "glm53"})).await;
    let res = turn(
        &r.core,
        None,
        "Rename the call sites in Examplecorp's billing module.",
        Some("glm53"),
    )
    .await;
    assert_eq!(res.profile, "default");
    let d = decided(&r.core.store);
    assert_eq!(d[0]["data_gate"], "sensitive", "{d:?}");
    let data = res.route.unwrap().data.expect("data");
    assert_eq!(data.footer().as_deref(), Some("data gate: sensitive"));
    let counts = gate_counts(&r.core, &rx).await;
    assert_eq!(counts.len(), 1, "{counts:?}");
    assert_eq!(
        (&counts[0].0, &counts[0].1),
        (&json!("route"), &json!("sensitive"))
    );
    assert!(count_is_one(&counts[0].2), "{counts:?}");
}

/// theseus-4h85l: the provider chokepoint counts each refusal on
/// `theseus.route.data_gate`, by `chokepoint` and its class word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_chokepoint_counts_its_refusal() {
    let (rx, tel) = telemetry().await;
    let jev = jev(&[OPEN_A]);
    let dir = tempfile::tempdir().unwrap();
    let (r, _zai, _d) = rig_with_telemetry(&jev, &dir, tel, |_| {});
    let provider = r.core.runner.providers["zai"].clone();
    let req = crate::provider::ProviderRequest {
        model: "glm-5.3-flash".into(),
        max_tokens: 16,
        ..Default::default()
    };
    assert!(provider.count_tokens(&req).await.is_err());
    let counts = gate_counts(&r.core, &rx).await;
    assert_eq!(counts.len(), 1, "{counts:?}");
    assert_eq!(
        (&counts[0].0, &counts[0].1),
        (&json!("chokepoint"), &json!("restricted_origin"))
    );
    assert!(count_is_one(&counts[0].2), "{counts:?}");
}
