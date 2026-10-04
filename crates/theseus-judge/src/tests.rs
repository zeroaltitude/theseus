//! The crate's tests that cross modules: the wire against fixtures of the
//! verified shape, the client against the fake Jev in each mode, and the
//! judge end to end (batching, the breaker, pricing, drift, recording).

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::band::Band;
use crate::batch::namespaced;
use crate::breaker::{BreakerConfig, Transition};
use crate::builders::{prepare, Input, ProbeInput};
use crate::client::*;
use crate::fake::{FakeJev, FakeMode, Scripted};
use crate::judge::*;
use crate::pack::{by_name, Pack};
use crate::price;
use crate::state::NoScrub;

/// Compares `actual` with a golden file under the crate; `UPDATE_GOLDEN=1`
/// writes it instead (then read it, and commit it).
pub(crate) fn golden(path: &str, actual: &str) {
    let full = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&full, format!("{actual}\n")).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&full)
        .unwrap_or_else(|_| panic!("{path} is missing; run the test with UPDATE_GOLDEN=1"));
    assert_eq!(actual, expected.trim_end_matches('\n'), "{path} changed");
}

const KEY: &str = "apik-test-0123456789-abcdefghijklmnop";

fn probe_pack() -> Arc<Pack> {
    by_name("probe.v1").unwrap()
}

fn probe_input() -> ProbeInput {
    serde_json::from_str(include_str!("../fixtures/inputs/probe.json")).unwrap()
}

/// The discovery call's questions (cause, severity, needs_human), as the
/// test pack asks them, namespaced as the live call was.
fn discovery_request() -> Request {
    let pack = probe_pack();
    let p = prepare(&pack, &Input::Probe(probe_input()), &NoScrub).unwrap();
    let questions = pack
        .ask(&p.dynamic)
        .into_iter()
        .filter(|a| a.id != "severity_applies")
        .map(|a| (namespaced(&pack.name(), &a.id), a.question))
        .collect();
    Request {
        state: p.state.json.clone(),
        model: price::JEV_MODEL.into(),
        questions,
    }
}

const DISCOVERED: &str = include_str!("../fixtures/wire/discover.response.json");

fn malformed_reason(body: &str) -> (String, Option<Usage>) {
    match parse_response(body.as_bytes(), &discovery_request()) {
        Err(JevError::Malformed { reason, usage, .. }) => (reason, usage),
        other => panic!("not malformed: {other:?}"),
    }
}

/// The discovered body with one edit.
fn edited(f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(DISCOVERED).unwrap();
    f(&mut v);
    v.to_string()
}

// --------------------------------------------------------------- the wire

#[test]
fn the_real_response_parses_against_its_request() {
    let r = parse_response(DISCOVERED.as_bytes(), &discovery_request()).unwrap();
    assert_eq!(r.model, "jev-1.13.0");
    assert_eq!(
        r.usage,
        Usage {
            input_tokens: 586,
            output_tokens: 98
        }
    );
    let ids: Vec<&str> = r.answers.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "probe.v1/cause",
            "probe.v1/needs_human",
            "probe.v1/severity"
        ]
    );
    match r.answer("probe.v1/cause").unwrap() {
        Answer::Choice {
            choice,
            probabilities,
            confidence,
        } => {
            assert_eq!(choice, "dependency_change");
            assert_eq!(*confidence, 1.0);
            // In the question's option order, whatever the body's.
            let order: Vec<&str> = probabilities.iter().map(|(o, _)| o.as_str()).collect();
            assert_eq!(
                order,
                vec![
                    "dependency_change",
                    "flaky_infrastructure",
                    "code_bug",
                    "other"
                ]
            );
        }
        a => panic!("{a:?}"),
    }
    let sev = r.answer("probe.v1/severity").unwrap();
    assert_eq!(
        *sev,
        Answer::Score {
            score: 1.81,
            probabilities: vec![0.01, 0.17, 0.82, 0.0],
            confidence: 0.81
        }
    );
    assert_eq!(sev.score_level(), Some(2));
    assert_eq!(
        *r.answer("probe.v1/needs_human").unwrap(),
        Answer::Noul { noul: 0.89 }
    );
}

#[test]
fn the_request_body_has_the_verified_shape_and_keeps_option_order() {
    let req = discovery_request();
    let body = req.body();
    golden("fixtures/wire/probe.request.json", &body);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert!(v["state"].is_object(), "a named-field state");
    assert_eq!(v["model"], "jev-1.13.0");
    let q = &v["questions"];
    assert_eq!(q["probe.v1/cause"]["type"], "choice");
    assert!(q["probe.v1/cause"]["criteria"]["other"].is_string());
    assert_eq!(q["probe.v1/severity"]["type"], "score");
    assert_eq!(
        q["probe.v1/severity"]["criteria"].as_array().unwrap().len(),
        4
    );
    assert_eq!(q["probe.v1/needs_human"]["type"], "noul");
    assert!(q["probe.v1/needs_human"]["criteria"]["true"].is_string());
    // The state's bytes go out exactly as built (and hashed).
    assert!(body.starts_with(&format!("{{\"state\":{},", req.state)));
    // Options in the pack's order, the no-match option last.
    let at = |s: &str| body.find(s).unwrap();
    assert!(
        at("\"dependency_change\"") < at("\"code_bug\"") && at("\"code_bug\"") < at("\"other\"")
    );
    // A Choice option without words goes out as null, as the verified shape has it.
    let bare = Question::Choice {
        instructions: "Which?".into(),
        options: vec![
            ChoiceOption {
                id: "a".into(),
                means: Some("A".into()),
            },
            ChoiceOption {
                id: "other".into(),
                means: None,
            },
        ],
    };
    assert_eq!(
        bare.wire(),
        r#"{"type":"choice","instructions":"Which?","criteria":{"a":"A","other":null}}"#
    );
    let noul = Question::Noul {
        instructions: "Is it?".into(),
        when_true: None,
        when_false: None,
    };
    assert_eq!(noul.wire(), r#"{"type":"noul","instructions":"Is it?"}"#);
}

#[test]
fn each_malformed_case_is_malformed_and_keeps_the_usage() {
    // A missing id.
    let (r, u) = malformed_reason(&edited(|v| {
        v["answers"]
            .as_object_mut()
            .unwrap()
            .remove("probe.v1/needs_human");
    }));
    assert!(r.contains("no answer for \"probe.v1/needs_human\""), "{r}");
    assert_eq!(u.map(|u| u.input_tokens), Some(586));
    // An extra id.
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/surprise"] = json!({"type": "noul", "noul": 0.5});
    }));
    assert!(r.contains("which was not asked"), "{r}");
    // A wrong type: a Noul's answer where a Choice was asked, typed or not.
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/cause"] = json!({"type": "noul", "noul": 0.5});
    }));
    assert!(r.contains("typed \"noul\""), "{r}");
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/cause"] = json!({"noul": 0.5});
    }));
    assert!(r.contains("no choice (wrong type)"), "{r}");
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/needs_human"]["noul"] = json!("high");
    }));
    assert!(r.contains("not a number"), "{r}");
    // A choice outside its options.
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/cause"]["choice"] = json!("cosmic_rays");
    }));
    assert!(r.contains("not one of its options"), "{r}");
    // Probabilities that don't sum to about 1.
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/cause"]["probabilities"]["other"] = json!(0.5);
    }));
    assert!(r.contains("sum to 1.5000"), "{r}");
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/severity"]["probabilities"]["3"] = json!(0.2);
    }));
    assert!(r.contains("sum to 1.2000"), "{r}");
    // The shape's own checks: a level or option left out, a legend that
    // names other levels, a value outside 0..1, a score off its scale.
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/severity"]["probabilities"]
            .as_object_mut()
            .unwrap()
            .remove("3");
    }));
    assert!(r.contains("level 3 no probability"), "{r}");
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/cause"]["probabilities"]
            .as_object_mut()
            .unwrap()
            .remove("code_bug");
    }));
    assert!(r.contains("no probability"), "{r}");
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/severity"]["legend"]["1"] = json!("Mild");
    }));
    assert!(r.contains("legend"), "{r}");
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/needs_human"]["noul"] = json!(1.2);
    }));
    assert!(r.contains("outside 0..1"), "{r}");
    let (r, _) = malformed_reason(&edited(|v| {
        v["answers"]["probe.v1/severity"]["score"] = json!(4.0);
    }));
    assert!(r.contains("outside 0..3"), "{r}");
    // No usage, or no JSON at all: malformed, and the usage is unknown.
    let (r, u) = malformed_reason(&edited(|v| {
        v.as_object_mut().unwrap().remove("usage");
    }));
    assert!(r.contains("no usage") && u.is_none(), "{r}");
    match parse_response(b"<html>gateway error</html>", &discovery_request()) {
        Err(e @ JevError::Malformed { .. }) => assert!(e.usage_unknown()),
        other => panic!("{other:?}"),
    }
    // Unknown extra fields are tolerated; only meaning is strict.
    let ok = edited(|v| {
        v["id"] = json!("resp_1");
        v["answers"]["probe.v1/needs_human"]["note"] = json!("extra");
    });
    assert!(parse_response(ok.as_bytes(), &discovery_request()).is_ok());
}

#[test]
fn errors_say_their_class_and_whether_usage_is_unknown() {
    let total = JevError::Timeout {
        phase: TimeoutPhase::Total,
        elapsed_ms: 5000,
    };
    assert!(total.is_transient() && total.usage_unknown() && total.counts_for_breaker());
    let connect = JevError::Timeout {
        phase: TimeoutPhase::Connect,
        elapsed_ms: 2000,
    };
    assert!(connect.is_transient() && !connect.usage_unknown());
    let queue = JevError::Timeout {
        phase: TimeoutPhase::Queue,
        elapsed_ms: 3000,
    };
    assert!(
        queue.is_transient() && !queue.counts_for_breaker(),
        "a local queue is not Jev's failure"
    );
    for (status, class) in [
        (429, "rate_limited"),
        (401, "auth"),
        (403, "auth"),
        (400, "invalid_request"),
        (404, "invalid_request"),
        (413, "over_state"),
        (500, "server"),
        (503, "server"),
    ] {
        let e = JevError::from_status(status, "{}", None, 10);
        assert_eq!(e.class(), class, "{status}");
    }
    let rl = JevError::from_status(429, r#"{"error":{"message":"slow down"}}"#, Some(7), 10);
    assert_eq!(
        rl,
        JevError::RateLimited {
            message: "slow down".into(),
            retry_after_secs: Some(7)
        }
    );
}

// ------------------------------------------------------ the client, faked

fn client_for(fake: &FakeJev, total_ms: u64, in_flight: usize) -> JevClient {
    JevClient::new(
        ClientConfig {
            api_base: fake.base(),
            connect: Duration::from_secs(2),
            total: Duration::from_millis(total_ms),
            max_in_flight: in_flight,
        },
        Arc::new(StaticKey::new(KEY.into())),
    )
    .unwrap()
}

fn jev_err(c: &Called) -> &JevError {
    match &c.result {
        Err(CallError::Jev(e)) => e,
        other => panic!("not a Jev error: {other:?}"),
    }
}

#[tokio::test]
async fn up_answers_as_scripted_with_the_key_as_a_bearer() {
    let fake = FakeJev::start().unwrap();
    fake.script(
        "cause",
        Scripted::Choice {
            option: "code_bug".into(),
            confidence: 0.95,
        },
    );
    fake.script(
        "probe.v1/severity",
        Scripted::Score {
            level: 3,
            confidence: 0.7,
        },
    );
    fake.script("needs_human", Scripted::Noul(0.08));
    let c = client_for(&fake, 2000, 8)
        .call(&discovery_request(), Urgency::Shadow)
        .await;
    let r = c.result.as_ref().unwrap();
    assert!(
        matches!(r.answer("probe.v1/cause"), Some(Answer::Choice { choice, .. }) if choice == "code_bug")
    );
    assert_eq!(
        r.answer("probe.v1/severity").unwrap().score_level(),
        Some(3)
    );
    assert_eq!(
        *r.answer("probe.v1/needs_human").unwrap(),
        Answer::Noul { noul: 0.08 }
    );
    let seen = fake.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].bearer_len, Some(KEY.len()));
    assert_eq!(seen[0].body["model"], "jev-1.13.0");
    assert_eq!(
        c.rate_limit.get("x-ratelimit-limit").map(String::as_str),
        Some("600")
    );
    assert!(c.timing.http_ms <= c.timing.total_ms);
}

#[tokio::test]
async fn each_fake_mode_maps_to_its_error_class() {
    let fake = FakeJev::start().unwrap();
    let client = client_for(&fake, 400, 8);
    let req = discovery_request();

    fake.set_mode(FakeMode::Down);
    let c = client.call(&req, Urgency::Shadow).await;
    let e = jev_err(&c);
    assert_eq!(e.class(), "network", "{e}");
    assert!(e.is_transient() && !e.usage_unknown());

    fake.set_mode(FakeMode::Slow(Duration::from_millis(1500)));
    let c = client.call(&req, Urgency::Shadow).await;
    let e = jev_err(&c);
    assert!(
        matches!(
            e,
            JevError::Timeout {
                phase: TimeoutPhase::Total,
                ..
            }
        ),
        "{e}"
    );
    assert!(
        e.usage_unknown(),
        "a total timeout after the send holds its reservation"
    );

    fake.set_mode(FakeMode::RateLimited {
        retry_after_secs: 7,
    });
    let c = client.call(&req, Urgency::Shadow).await;
    assert!(matches!(
        jev_err(&c),
        JevError::RateLimited {
            retry_after_secs: Some(7),
            ..
        }
    ));
    assert_eq!(
        c.rate_limit
            .get("x-ratelimit-remaining")
            .map(String::as_str),
        Some("0")
    );

    fake.set_mode(FakeMode::Malformed);
    let c = client.call(&req, Urgency::Shadow).await;
    let e = jev_err(&c);
    assert_eq!(e.class(), "malformed");
    assert!(!e.usage_unknown(), "the body still said what it cost");

    for (status, class) in [(500, "server"), (400, "invalid_request"), (403, "auth")] {
        fake.set_mode(FakeMode::Status(status));
        let c = client.call(&req, Urgency::Shadow).await;
        assert_eq!(jev_err(&c).class(), class, "{status}");
    }
}

#[tokio::test]
async fn an_auth_error_never_echoes_the_key() {
    let fake = FakeJev::start().unwrap();
    fake.set_mode(FakeMode::EchoAuth);
    let c = client_for(&fake, 2000, 8)
        .call(&discovery_request(), Urgency::Shadow)
        .await;
    let e = jev_err(&c);
    assert_eq!(e.class(), "auth");
    let shown = format!("{e} {e:?} {}", serde_json::to_string(e).unwrap());
    assert!(!shown.contains(KEY), "{shown}");
    assert!(shown.contains("[redacted]"), "{shown}");
    assert_eq!(
        format!("{:?}", StaticKey::new(KEY.into())),
        "StaticKey([redacted])"
    );
}

#[tokio::test]
async fn shadow_is_shed_with_no_free_permit_and_live_waits_to_its_deadline() {
    let fake = FakeJev::start().unwrap();
    fake.set_mode(FakeMode::Slow(Duration::from_millis(800)));
    let client = Arc::new(client_for(&fake, 3000, 1));
    let req = Arc::new(discovery_request());
    let (c2, r2) = (client.clone(), req.clone());
    let holder = tokio::spawn(async move { c2.call(&r2, Urgency::Shadow).await });
    // Let the first call take the only permit.
    for _ in 0..200 {
        if client.in_flight() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(client.in_flight(), 1);
    let shed = client.call(&req, Urgency::Shadow).await;
    assert!(matches!(shed.result, Err(CallError::Shed)));
    assert!(shed.timing.total_ms < 100, "shedding is immediate");
    assert_eq!(client.shed_total(), 1);
    let now = std::time::Instant::now();
    assert_eq!(client.take_shed_report(now), Some(1));
    assert_eq!(
        client.take_shed_report(now),
        None,
        "one report a minute at most"
    );
    let live = client
        .call(&req, Urgency::live(Duration::from_millis(150)))
        .await;
    assert!(matches!(
        live.result,
        Err(CallError::Jev(JevError::Timeout {
            phase: TimeoutPhase::Queue,
            ..
        }))
    ));
    assert!(holder.await.unwrap().result.is_ok());
    assert_eq!(client.in_flight(), 0);
    // A live call with a free permit runs under its own deadline.
    let slow_live = client
        .call(&req, Urgency::live(Duration::from_millis(300)))
        .await;
    assert!(matches!(
        slow_live.result,
        Err(CallError::Jev(JevError::Timeout {
            phase: TimeoutPhase::Total,
            ..
        }))
    ));
}

#[tokio::test]
async fn no_key_and_an_oversized_state_send_nothing() {
    struct Unsettled;
    impl KeySource for Unsettled {
        fn key(&self) -> Option<zeroize::Zeroizing<String>> {
            None
        }
    }
    let fake = FakeJev::start().unwrap();
    let client = JevClient::new(
        ClientConfig {
            api_base: fake.base(),
            ..ClientConfig::default()
        },
        Arc::new(Unsettled),
    )
    .unwrap();
    let c = client.call(&discovery_request(), Urgency::Shadow).await;
    assert!(matches!(c.result, Err(CallError::NoKey)));
    let mut big = discovery_request();
    big.state = serde_json::to_string(&json!({"log": "x".repeat(200_000)})).unwrap();
    let c = client_for(&fake, 2000, 8).call(&big, Urgency::Shadow).await;
    assert!(matches!(
        jev_err(&c),
        JevError::OverState { limit: 32_000, .. }
    ));
    assert_eq!(fake.connections(), 0);
}

// --------------------------------------------------------- the judge, faked

fn judge_for(fake: &FakeJev) -> Recording<JevJudge, Arc<MemorySink>> {
    let judge = JevJudge::new(
        client_for(fake, 2000, 8),
        price::builtin(),
        BreakerConfig::default(),
    );
    Recording::new(judge, Arc::new(MemorySink::default()))
}

fn ask_with(pack: Arc<Pack>, input: ProbeInput) -> Ask {
    let p = prepare(&pack, &Input::Probe(input), &NoScrub).unwrap();
    Ask::new(pack, &p, Mode::Shadow, json!({"session": "s1"}))
}

/// The core mints a judgment's id at its dispatch, to mark a turn's trace
/// with it: the judgment carries that id; an ask without one gets its own.
#[tokio::test]
async fn a_judgment_takes_the_id_its_dispatch_minted() {
    let fake = FakeJev::start().unwrap();
    let judge = judge_for(&fake);
    let mut minted = ask_with(probe_pack(), probe_input());
    minted.id = Some("jdg_0000minted".into());
    let js = judge
        .judge(DecisionPoint {
            asks: vec![minted, ask_with(probe_pack(), probe_input())],
            urgency: Urgency::Shadow,
        })
        .await;
    assert_eq!(js[0].id, "jdg_0000minted");
    assert!(js[1].id.starts_with("jdg_") && js[1].id != js[0].id);
}

fn versioned(v: u32, model: &str) -> Arc<Pack> {
    let mut p = (*probe_pack()).clone();
    p.version = v;
    p.jev_model = model.into();
    Arc::new(p)
}

#[tokio::test]
async fn shared_states_ride_one_call_and_split_its_cost_by_question_count() {
    let fake = FakeJev::start().unwrap();
    fake.script("needs_human", Scripted::Noul(0.95));
    fake.script(
        "cause",
        Scripted::Choice {
            option: "dependency_change".into(),
            confidence: 0.7,
        },
    );
    let judge = judge_for(&fake);
    let mut other = probe_input();
    other.event = "The nightly build passed.".into();
    let point = DecisionPoint {
        asks: vec![
            ask_with(probe_pack(), probe_input()),
            ask_with(versioned(2, "jev-1.13.0"), probe_input()),
            ask_with(versioned(3, "jev-1.13.0"), other),
        ],
        urgency: Urgency::Shadow,
    };
    let js = judge.judge(point).await;
    assert_eq!(fake.connections(), 2, "two states, two calls");
    assert_eq!(js.len(), 3);
    assert_eq!(
        js[0].call.as_ref().unwrap().id,
        js[1].call.as_ref().unwrap().id
    );
    assert_ne!(
        js[0].call.as_ref().unwrap().id,
        js[2].call.as_ref().unwrap().id
    );
    assert_eq!(js[0].call.as_ref().unwrap().questions, 8);
    let seen = fake.seen();
    let ids: Vec<String> = seen
        .iter()
        .flat_map(|s| {
            s.body["questions"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        ids.contains(&"probe.v1/cause".to_string()) && ids.contains(&"probe.v2/cause".to_string())
    );
    for j in &js {
        assert!(j.actionable(), "{:?}", j.outcome);
        assert_eq!(j.answers.len(), 4);
        assert_eq!(j.context["session"], "s1");
    }
    // Answers come back under each pack's own ids, banded.
    let nh = js[1].answer("needs_human").unwrap();
    assert!(nh.band.acts_true());
    assert_eq!(js[1].answer("cause").unwrap().band.band, Band::Confirm);
    // The shared call's usage and cost split evenly (4 questions each).
    let (u0, u1) = (js[0].usage.unwrap(), js[1].usage.unwrap());
    let call_tokens = u0.input_tokens + u1.input_tokens;
    assert!(u0.input_tokens.abs_diff(u1.input_tokens) <= 1);
    let price = price::JevPrice::jev_1_13_0();
    let call_cost = price.cost_micros(&Usage {
        input_tokens: call_tokens,
        output_tokens: u0.output_tokens + u1.output_tokens,
    });
    assert_eq!(
        js[0].cost_micros.unwrap() + js[1].cost_micros.unwrap(),
        call_cost
    );
    assert!(js[0].reserve_micros.is_some());
    // Every judgment was recorded.
    assert_eq!(judge.sink().judgments(), js);
}

#[tokio::test]
async fn an_answer_from_another_model_is_drift_and_never_actionable() {
    let fake = FakeJev::start().unwrap();
    fake.answer_as(Some("jev-1.14.0"));
    let js = judge_for(&fake)
        .judge(DecisionPoint {
            asks: vec![ask_with(probe_pack(), probe_input())],
            urgency: Urgency::Shadow,
        })
        .await;
    assert_eq!(js[0].outcome, Outcome::Answered);
    assert!(js[0].model_drift);
    assert_eq!(js[0].answered_by.as_deref(), Some("jev-1.14.0"));
    assert!(!js[0].actionable());
    assert_eq!(js[0].answers.len(), 4, "recorded, not acted on");
}

#[tokio::test]
async fn an_unpriced_model_is_never_called() {
    let fake = FakeJev::start().unwrap();
    let judge = judge_for(&fake);
    let asks = vec![ask_with(versioned(2, "jev-9.9.9"), probe_input())];
    assert_eq!(judge.inner().reserve_micros(&asks), None);
    let js = judge
        .judge(DecisionPoint {
            asks,
            urgency: Urgency::Shadow,
        })
        .await;
    assert_eq!(
        js[0].outcome,
        Outcome::Skipped {
            reason: Skip::Unpriced
        }
    );
    assert_eq!(fake.connections(), 0);
    assert_eq!(judge.sink().judgments().len(), 1, "a skip is recorded too");
}

#[tokio::test]
async fn an_outage_opens_the_breaker_and_then_costs_nothing() {
    let fake = FakeJev::start().unwrap();
    fake.set_mode(FakeMode::Down);
    let judge = judge_for(&fake);
    let one = || DecisionPoint {
        asks: vec![ask_with(probe_pack(), probe_input())],
        urgency: Urgency::Shadow,
    };
    let mut transitions = Vec::new();
    for _ in 0..5 {
        let j = judge.judge(one()).await.remove(0);
        assert!(
            matches!(&j.outcome, Outcome::Failed { class, transient: true, .. } if class == "network")
        );
        transitions.extend(j.circuit);
    }
    assert_eq!(
        transitions,
        vec![Transition::Opened {
            failures: 5,
            for_secs: 60
        }]
    );
    let before = fake.connections();
    let started = std::time::Instant::now();
    let j = judge.judge(one()).await.remove(0);
    assert_eq!(
        j.outcome,
        Outcome::Skipped {
            reason: Skip::CircuitOpen
        }
    );
    assert_eq!(fake.connections(), before, "no call while open");
    assert!(started.elapsed() < Duration::from_millis(50));
    assert!(matches!(
        judge.inner().breaker_status(),
        crate::breaker::Status::Open { .. }
    ));
}

#[tokio::test]
async fn a_pack_with_nothing_to_ask_is_skipped_without_a_call() {
    let fake = FakeJev::start().unwrap();
    let judge = judge_for(&fake);
    let mut a = ask_with(probe_pack(), probe_input());
    a.asked.clear();
    let js = judge
        .judge(DecisionPoint {
            asks: vec![a],
            urgency: Urgency::Shadow,
        })
        .await;
    assert_eq!(
        js[0].outcome,
        Outcome::Skipped {
            reason: Skip::NoQuestions
        }
    );
    assert_eq!(fake.connections(), 0);
}

/// Every L2 live call of 2026-09-30 (`jev-probe --pack`), rebuilt from its
/// fixture and batched as the judge batches it, fits inside its
/// reservation: the packs, the input, and what Jev billed.
#[test]
fn every_live_call_fits_inside_its_reservation() {
    let client = JevClient::new(
        ClientConfig::default(),
        Arc::new(StaticKey::new(KEY.into())),
    )
    .unwrap();
    let judge = JevJudge::new(client, price::builtin(), BreakerConfig::default());
    let p = price::JevPrice::jev_1_13_0();
    let calls: [(&[&str], &str, u64, u64); 6] = [
        (&["loop.v1"], "loop", 1_435, 194),
        (&["security.v1"], "security", 1_258, 207),
        (&["classify.v1", "role.v1"], "inbound", 1_353, 297),
        (&["continue.v1"], "continue", 804, 113),
        (&["categorize.v1"], "categorize", 732, 103),
        (&["role.v1"], "inbound", 664, 73),
    ];
    for (packs, input, input_tokens, output_tokens) in calls {
        let path = format!(
            "{}/fixtures/inputs/{input}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).unwrap();
        let asks: Vec<Ask> = packs
            .iter()
            .map(|n| {
                let pack = by_name(n).unwrap();
                let i = Input::parse(pack.builder, &text).unwrap();
                let prepared = prepare(&pack, &i, &NoScrub).unwrap();
                Ask::new(pack, &prepared, Mode::Shadow, json!({}))
            })
            .collect();
        let reserved = judge.reserve_micros(&asks).unwrap();
        let billed = p.cost_micros(&Usage {
            input_tokens,
            output_tokens,
        });
        assert!(
            reserved >= billed,
            "{packs:?}: reserved {reserved}, billed {billed}"
        );
    }
}
