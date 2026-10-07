//! A routed turn's model (theseus-490i): routing moves a turn after its
//! trace began, and `route_to` moves the trace's root with it, so the root's
//! `profile`, `provider` and `model`, and with them every point of the
//! turn's metrics (`theseus.turns`'s `gen_ai.request.model`, read from the
//! root), name the model the call went to. Before, the root kept the model
//! before routing: a switch to Opus was counted under Sonnet's model, and a
//! detour to GLM 5.3 Flash under Sonnet's. The rig is `tests_route`'s, with
//! telemetry's receiver.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::TurnSubmitResult;

use crate::telemetry::tests::Receiver;
use crate::telemetry::{Telemetry, TelemetryConfig};
use crate::tests_route::{call, mode, rig_parts, until_route_rows};

/// `theseus.turns`'s points in the receiver's last metrics, each by its
/// attributes (values as text).
fn turns(rx: &Receiver) -> Vec<BTreeMap<String, String>> {
    let got = rx.at("/v1/metrics");
    let last = got.last().expect("a metrics export");
    let metrics = last.body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    metrics
        .iter()
        .filter(|m| m["name"] == "theseus.turns")
        .flat_map(|m| {
            m["sum"]["dataPoints"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|p| {
            let attrs = p["attributes"].as_array().cloned().unwrap_or_default();
            attrs
                .iter()
                .map(|kv| {
                    let v = kv["value"]["stringValue"].as_str().unwrap_or_default();
                    (kv["key"].as_str().unwrap().to_string(), v.to_string())
                })
                .collect()
        })
        .collect()
}

/// The trace root's `profile`, `provider` and `model`.
fn root(res: &TurnSubmitResult) -> (String, String, String) {
    let attrs = &res.trace.as_ref().expect("a trace").attrs;
    let s = |k: &str| attrs[k].as_str().unwrap_or_default().to_string();
    (s("profile"), s("provider"), s("model"))
}

/// A switch (a hard question to Opus) and a detour (thanks, to GLM 5.3
/// Flash), each from Sonnet: the trace's root and the turn's
/// `theseus.turns` point name the model the call went to.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turns_metrics_name_the_model_it_ran_on() {
    let rx = Receiver::start(vec![]).await;
    let jev = FakeJev::start().unwrap();
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let r = rig_parts(Some(&jev), 2, crate::tests_route::glm_placements, |p| {
        p.telemetry = Some(Telemetry::from_config(&tel, None).unwrap())
    });
    let submit = |input: &str| {
        let core = r.core.clone();
        let params = json!({"input": input});
        async move {
            let v: Value = call(&core, "turn.submit", params).await;
            serde_json::from_value::<TurnSubmitResult>(v).unwrap()
        }
    };
    mode(&jev, "sophisticated", 0.95);
    let hard = submit("Weigh two designs for a crash-safe write-ahead log.").await;
    assert_eq!(hard.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(r.claude.requests()[0].model, "claude-opus-5-5");
    assert_eq!(
        root(&hard),
        ("opus".into(), "anthropic".into(), "claude-opus-5-5".into())
    );
    mode(&jev, "trivial", 0.95);
    let thanks = submit("thank you!").await;
    assert_eq!(thanks.route.as_ref().unwrap().reason, "detour");
    assert_eq!(r.zai.requests()[0].model, "glm-5.3-flash");
    assert_eq!(
        root(&thanks),
        ("glm".into(), "zai".into(), "glm-5.3-flash".into())
    );
    until_route_rows(&r.core.store, 2).await;
    assert!(r.core.telemetry().flush(Duration::from_secs(10)).await);
    let points = turns(&rx);
    let named = |profile: &str| {
        let p: Vec<_> = points
            .iter()
            .filter(|a| a.get("theseus.profile").map(String::as_str) == Some(profile))
            .collect();
        assert_eq!(p.len(), 1, "{profile}: {points:#?}");
        (
            p[0]["gen_ai.provider.name"].clone(),
            p[0]["gen_ai.request.model"].clone(),
        )
    };
    assert_eq!(
        named("opus"),
        ("anthropic".into(), "claude-opus-5-5".into())
    );
    assert_eq!(named("glm"), ("zai".into(), "glm-5.3-flash".into()));
    assert_eq!(points.len(), 2, "{points:#?}");
}

/// One `turn.submit` through the protocol server: its error, which `call`
/// refuses to see.
async fn failed_call(
    core: &std::sync::Arc<crate::Core>,
    params: Value,
) -> theseus_protocol::RpcError {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), "turn.submit", params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = tokio::io::BufReader::new(cr).lines();
    let r = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    r.error.expect("the turn fails")
}

/// A switch to Opus whose provider call fails (theseus-udzb): the failure's
/// `theseus.turns` point and its provider-error count name the model the
/// call went to, as a finished turn's do, not the base's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_routed_turn_is_counted_under_the_model_it_ran_on() {
    let rx = Receiver::start(vec![]).await;
    let jev = FakeJev::start().unwrap();
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let r = rig_parts(
        Some(&jev),
        0,
        |_| {},
        |p| p.telemetry = Some(Telemetry::from_config(&tel, None).unwrap()),
    );
    {
        let mut script = r.claude.script.lock().unwrap();
        for _ in 0..8 {
            script.push_back(crate::provider::Scripted::Fail(
                crate::provider::ProviderError::Server {
                    status: 500,
                    message: "refused".into(),
                },
            ));
        }
    }
    mode(&jev, "sophisticated", 0.95);
    let e = failed_call(
        &r.core,
        json!({"input": "Weigh two designs for a crash-safe write-ahead log."}),
    )
    .await;
    assert_eq!(e.code, theseus_protocol::error_code::PROVIDER, "{e:?}");
    assert_eq!(r.claude.requests()[0].model, "claude-opus-5-5");
    assert!(r.core.telemetry().flush(Duration::from_secs(10)).await);
    let failed: Vec<_> = turns(&rx)
        .into_iter()
        .filter(|a| a.get("theseus.outcome").map(String::as_str) == Some("failed"))
        .collect();
    assert_eq!(failed.len(), 1, "{failed:#?}");
    assert_eq!(failed[0]["theseus.profile"], "opus", "{failed:#?}");
    assert_eq!(failed[0]["gen_ai.request.model"], "claude-opus-5-5");
    let errors = rx.at("/v1/metrics");
    let body = errors.last().unwrap().body.to_string();
    assert!(
        body.contains("theseus.provider.errors") && !body.contains("claude-sonnet"),
        "{body}"
    );
}
