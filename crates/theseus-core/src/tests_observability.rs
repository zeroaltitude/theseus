//! The data rule on every route row and provider call (theseus-a1wvq.11,
//! theseus-9ymz0): `route.decided`'s `cn` word, gate, provider, model and
//! origin; `provider.call`'s origin; the turn's footer words; and the gate's
//! counter. The rig is `tests_dm_rig`'s.

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;

use crate::content::{Flag, ItemKind};
use crate::content_wait::judging::{Judging, Silent};
use crate::provider::Scripted;
use crate::telemetry::tests::Receiver;
use crate::telemetry::{Telemetry, TelemetryConfig};
use crate::tests_dm_rig::{
    dm, gate_counts, jev, judging, judging_with, rig_telemetry_with, rig_with, say, telemetry,
    OPEN_A, PRIVATE,
};
use crate::tests_judge::kinds;
use crate::tests_route::{call, decided, mode, rig, turn};

fn calls(core: &crate::Core) -> Vec<Value> {
    kinds(&core.store, "provider.call")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// A judge that raises `confidential` on `PRIVATE`'s marker and nothing else.
fn finds_private() -> std::sync::Arc<Judging> {
    Judging::with(|_, text| match text.contains("marker-7c1q") {
        true => vec![Flag::Confidential],
        false => vec![],
    })
}

/// The key test (T29). In the owner's DM, a message whose mode is `trivial`
/// runs on `dsflash` when the session carries no label: its route row says
/// the session was allowed and where it ran. A message the judge labelled
/// stays on a US model, and its row says which flag left `dsflash` out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_decided_records_cn_gate_provider_model_origin() {
    let jev = jev(&[]);
    let (r, _zai, _dir) = rig_with(&jev, None, |_| {});
    judging_with(&r, finds_private());
    say(&r.core, &dm(&r.core), OPEN_A, None).await;
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    assert_ne!(res.profile, "dsflash");
    let rows = decided(&r.core.store);
    let (a, b) = (&rows[0], &rows[1]);
    assert_eq!(a["cn"], "allowed", "{a}");
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
    assert_eq!(b["cn"], "confidential", "{b}");
    assert_eq!(b["data_gate"], "confidential", "{b}");
    assert_eq!(b["origin"], "us", "{b}");
    assert_eq!(b["model"], res.model.as_str(), "{b}");
    assert_eq!(b["provider"], res.provider.as_str(), "{b}");
}

/// A plain Claude turn whose mode lists no restricted profile gains the
/// provider, model and origin, a `cn` word, and no `data_gate`: no label
/// keeps its session off a restricted model, and it never waited for any.
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
    assert_eq!(d["cn"], "allowed", "{d}");
    assert!(d.get("data_gate").is_none(), "{d}");
    let t = res.route.unwrap().data.expect("data");
    assert_eq!(
        (t.origin.as_str(), t.gate, t.cn.as_str()),
        ("us", None, "allowed")
    );
}

/// The floor moving a turn off its restricted base writes the why as
/// `data_gate`: a flag's word for a session labelled. A session whose ask never
/// lands in time is not moved: it goes on the labels it has (theseus-gyjyo.6).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_floor_move_names_its_why() {
    let jev = jev(&[]);
    mode(&jev, "chat", 0.95);
    let (r, zai, _dir) = rig_with(&jev, None, |c| c.content_based_security.wait_ms = 50);
    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    judging_with(&r, finds_private());
    say(&r.core, &dm(&r.core), PRIVATE, None).await;
    let d = decided(&r.core.store).pop().unwrap();
    assert_eq!(
        (d["reason"].as_str(), d["data_gate"].as_str()),
        (Some("data_gate"), Some("confidential")),
        "{d}"
    );
    assert_eq!(d["origin"], "us", "{d}");
    r.core
        .runner
        .verdicts
        .set(std::sync::Arc::new(Silent::default()));
    say(&r.core, &dm(&r.core), OPEN_A, None).await;
    let d = decided(&r.core.store).pop().unwrap();
    assert!(d.get("data_gate").is_none(), "no label, no move: {d}");
    assert_eq!(d["cn"], "allowed", "{d}");
    assert_eq!(zai.requests().len(), 1, "it waited 50 ms, then went");
}

/// A DM request's call says it went to a `cn` model; a Claude call says its
/// origin alone; and the loop after a flagged tool result, moved off the
/// restricted model, names its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_call_records_origin_per_request() {
    let jev = jev(&[]);
    let script = vec![
        Scripted::tools(
            "Looking.",
            &[("f1", "tools_find", json!({"query": "clock"}))],
        ),
        Scripted::text("Done."),
    ];
    let (r, _zai, _dir) = rig_with(&jev, Some(script), |_| {});
    judging_with(
        &r,
        Judging::with(|kind, _| match kind {
            ItemKind::ToolResult => vec![Flag::Secret],
            _ => vec![],
        }),
    );
    let sid = dm(&r.core);
    say(&r.core, &sid, OPEN_A, None).await;
    let rows = calls(&r.core);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let (first, moved) = (&rows[0], &rows[1]);
    assert_eq!(first["origin"], "cn", "{first}");
    assert_eq!(moved["origin"], "us", "{moved}");
    assert!(
        first.get("pass").is_none() && moved.get("pass").is_none(),
        "no pass is minted any more: {first} {moved}"
    );
}

/// A turn that ran on a restricted model words nothing in its footer; one the
/// data rule kept off it says `data gate: <why>`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_turns_footer_words_only_the_gate() {
    let jev = jev(&[]);
    let (r, _zai, _dir) = rig_with(&jev, None, |_| {});
    judging(&r);
    let res = say(&r.core, &dm(&r.core), OPEN_A, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let t = res.route.unwrap().data.expect("data");
    assert_eq!((t.cn.as_str(), t.footer()), ("allowed", None));
    // The same, the judge finding `PRIVATE` confidential.
    let jev = crate::tests_dm_rig::jev(&[]);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    judging_with(&r, finds_private());
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    assert!(zai.requests().is_empty());
    let t = res.route.unwrap().data.expect("data");
    assert_eq!(t.footer().as_deref(), Some("data gate: confidential"));
}

/// A turn the data rule kept off `dsflash` counts once on
/// `theseus.route.data_gate`, by caller and why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_data_gate_turn_counts_once_on_the_route_data_gate_metric() {
    let rx = Receiver::start(vec![]).await;
    let jev = jev(&[]);
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let (r, _zai, _dir) =
        crate::tests_dm_rig::rig_telemetry(&jev, Telemetry::from_config(&tel, None).unwrap());
    judging_with(&r, finds_private());
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
        (json!("route"), json!("confidential"))
    );
    let n = &points[0]["asInt"];
    assert!(n == 1 || n == "1", "{n}");
}

fn count_is_one(n: &Value) -> bool {
    n == 1 || n == "1"
}

/// theseus-4h85l: the provider chokepoint counts each refusal on
/// `theseus.route.data_gate`, by `chokepoint` and its why word: a call made for
/// no session, here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_chokepoint_counts_its_refusal() {
    let (rx, tel) = telemetry().await;
    let jev = jev(&[]);
    let (r, _zai, _d) = crate::tests_dm_rig::rig_telemetry(&jev, tel);
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
        (&json!("chokepoint"), &json!("no_session"))
    );
    assert!(count_is_one(&counts[0].2), "{counts:?}");
}

/// The key test of the footer (theseus-4h85l, ported): a labelled turn in a mode
/// that lists a China-origin profile is gated only when that profile would
/// have won had the session been allowed it. With Haiku ahead of it in the
/// list, the US model wins either way: no gate on the row, no footer, nothing
/// counted. With it first, the gate shows, the footer says it, and the counter
/// counts it once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_us_model_that_wins_anyway_shows_no_data_gate() {
    let (rx, tel) = telemetry().await;
    let jev = jev(&[]);
    let (r, _zai, _d) = rig_telemetry_with(&jev, tel, |c| {
        c.routing.classes.trivial.profiles = vec!["haiku".into(), "cheapest".into()];
        c.routing.classes.trivial.cn_after = Some("haiku".into());
    });
    judging_with(&r, finds_private());
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(res.profile, "haiku", "{d}");
    assert_eq!(d["cn"], "allowed", "the judge was never waited for: {d}");
    assert!(d.get("data_gate").is_none(), "{d}");
    let data = res.route.unwrap().data.expect("data");
    assert_eq!((data.gate.clone(), data.footer()), (None, None));
    assert_eq!(gate_counts(&r.core, &rx).await, vec![]);

    let (rx, tel) = telemetry().await;
    let (r, _zai, _d) = rig_telemetry_with(&jev, tel, |_| {});
    judging_with(&r, finds_private());
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(d["data_gate"], "confidential", "{d}");
    let data = res.route.unwrap().data.expect("data");
    assert_eq!(data.footer().as_deref(), Some("data gate: confidential"));
    let counts = gate_counts(&r.core, &rx).await;
    assert_eq!(counts.len(), 1, "{counts:?}");
    assert_eq!(
        (&counts[0].0, &counts[0].1),
        (&json!("route"), &json!("confidential"))
    );
    assert!(count_is_one(&counts[0].2), "{counts:?}");
}

/// Its mirror: the mode's list is restricted first, but the verdict is below
/// the mode's bar, so the decision stays on the base whatever the labels say.
/// The rule cost the turn nothing: no word, no count.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsure_verdict_that_stays_on_its_base_shows_no_data_gate() {
    let (rx, tel) = telemetry().await;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.3);
    let (r, _zai, _d) = rig_telemetry_with(&jev, tel, |c| {
        c.routing.classes.trivial.profiles = vec![];
        c.routing.classes.trivial.cn_after = None;
    });
    judging_with(&r, finds_private());
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(d["reason"], "unsure", "{d}");
    assert!(d.get("data_gate").is_none(), "{d}");
    let data = res.route.unwrap().data.expect("data");
    assert_eq!((data.gate.clone(), data.footer()), (None, None));
    assert_eq!(gate_counts(&r.core, &rx).await, vec![]);
}
