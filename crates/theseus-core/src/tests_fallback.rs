//! A refusal's client-side fallback (theseus-7gir.18), through the whole core
//! with a scripted stand-in provider and no key:
//! - Sonnet 5.5 refuses and Sonnet 5 answers the same request, in one turn,
//!   with one `provider.fallback` row, the reply's line, and both calls'
//!   spend, each reserved and priced as its own model's;
//! - a refusal there too ends the turn as a refusal: never a chain, never a
//!   loop, even where the fallback names a fallback of its own;
//! - a model without a fallback, and one the provider's own fallback rides,
//!   refuse as before;
//! - `[model.retries] refusal = false` turns it off;
//! - after a tool call, the fallback's requests carry both models' thinking
//!   and leave the refused answer out, each the one before it extended.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{micros_to_usd, Execution};
use theseus_protocol::route::TurnFallback;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>) -> Rig {
    rig_with(script, |_| {})
}

fn rig_with(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

/// A conversation bound to a place, so that its reply is an outbox post.
fn bound_session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place("dm:7", &rec.session_id).unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: "discord:dm:7".into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    rec.session_id
}

/// One input turn on the live profile, or on `model` when one is named.
async fn turn(core: &Arc<Core>, sid: &str, model: Option<&str>) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core
        .runner
        .resolve_target(&live, None, None, model)
        .unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("open the archive and read what the tide table says".into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .expect("the turn ends")
}

/// The ledger rows of `kind` in the session, oldest first.
fn rows(core: &Core, sid: &str, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(1000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(sid))
        .map(|(_, r)| r.data)
        .collect()
}

fn execution(core: &Core, sid: &str) -> Execution {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    core.kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
}

/// A mid-stream refusal, as b5's crack-7z-hash got one: some text and a call,
/// then `refusal` with its category.
fn refused(category: &str) -> Scripted {
    Scripted::Refused {
        blocks: vec![
            json!({"type": "text", "text": "Let me look at the archive."}),
            json!({"type": "tool_use", "id": "toolu_refused", "name": "fs_read", "input": {"path": "harbor.txt"}}),
        ],
        category: Some(category.into()),
    }
}

/// Every request but its model and output cap.
fn body(r: &ProviderRequest) -> Value {
    json!({"system": r.system, "messages": r.messages, "tools": r.tools, "thinking": r.thinking,
           "output_config": r.output_config, "betas": r.betas, "extra": r.extra})
}

/// What a usage costs at `model`'s catalog price, in dollars.
fn priced(core: &Core, model: &str, usage: &theseus_protocol::Usage) -> f64 {
    micros_to_usd(core.runner.catalog.get(model).unwrap().cost_micros(usage))
}

/// A turn Sonnet 5.5 refuses and Sonnet 5 answers, Sonnet 5 priced apart
/// from Sonnet 5.5 so that the books show which priced which; a second
/// answer waits for the session's next turn.
async fn fell_back() -> (Rig, String, TurnSubmitResult) {
    let script = vec![
        refused("cyber"),
        Scripted::text("The archive says four."),
        Scripted::text("Still four."),
    ];
    let r = rig_with(script, |c| {
        c.catalog.insert(
            "claude-sonnet-5".into(),
            crate::catalog::CatalogRow {
                output_per_mtok: Some(30.0),
                ..Default::default()
            },
        );
    });
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    (r, sid, res)
}

#[tokio::test]
async fn a_refusal_on_sonnet_5_5_is_answered_by_sonnet_5_with_its_row_and_line() {
    let (r, sid, res) = fell_back().await;
    // One turn: the answer is Sonnet 5's, and the refused text left the reply.
    assert_eq!(res.output, "The archive says four.");
    assert_eq!(
        (res.stop_reason.as_str(), res.loops),
        ("no_tool_calls", 2),
        "{res:?}"
    );
    assert_eq!(
        (res.model.as_str(), res.profile.as_str()),
        ("claude-sonnet-5", "sonnet")
    );
    let fell = TurnFallback {
        from: "claude-sonnet-5-5".into(),
        to: "claude-sonnet-5".into(),
        category: Some("cyber".into()),
        answered: true,
    };
    assert_eq!(res.fallback.as_ref(), Some(&fell));
    assert_eq!(
        fell.line(&res.stop_reason),
        "Sonnet 5.5 declined (cyber); Sonnet 5 answered."
    );
    // The same request, on the fallback: only the model differs.
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(
        (reqs[0].model.as_str(), reqs[1].model.as_str()),
        ("claude-sonnet-5-5", "claude-sonnet-5")
    );
    assert_eq!(body(&reqs[1]), body(&reqs[0]));
    let sent = serde_json::to_string(&reqs[1].messages).unwrap();
    assert!(
        !sent.contains("toolu_refused") && !sent.contains("Let me look"),
        "{sent}"
    );
    // One row for the switch, beside the refusal's.
    let switch = rows(&r.core, &sid, "provider.fallback");
    assert_eq!(switch.len(), 1, "{switch:?}");
    let refused_node = &switch[0]["refused"];
    assert_eq!(
        switch[0],
        json!({"from": "claude-sonnet-5-5", "to": "claude-sonnet-5", "category": "cyber", "loop": 0, "refused": refused_node})
    );
    assert_eq!(rows(&r.core, &sid, "provider.refusal").len(), 1);
    let ended = rows(&r.core, &sid, "turn.ended");
    assert_eq!(ended[0]["fallback"], json!(fell), "{ended:?}");
    assert_eq!(ended[0]["model"], "claude-sonnet-5");
    // The reply's post carries it, for Discord's line.
    let replies: Vec<Value> = r
        .core
        .outbox
        .open_for("discord:dm:7")
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "reply")
        .map(|a| crate::outbox::body_of(a).clone())
        .collect();
    assert_eq!(replies[0]["result"]["fallback"], json!(fell), "{replies:?}");
}

#[tokio::test]
async fn the_fallbacks_call_is_spent_and_reserved_as_its_own_and_the_session_keeps_its_model() {
    let (r, sid, res) = fell_back().await;
    // Both calls are the turn's spend, each priced as its own model.
    let calls = rows(&r.core, &sid, "provider.call");
    assert_eq!(calls.len(), 2);
    let usage = |i: usize| serde_json::from_value(calls[i]["usage"].clone()).unwrap();
    let want = priced(&r.core, "claude-sonnet-5-5", &usage(0))
        + priced(&r.core, "claude-sonnet-5", &usage(1));
    assert!(
        (res.cost_usd.unwrap() - want).abs() < 1e-9,
        "{res:?} {want}"
    );
    let e = execution(&r.core, &sid);
    assert!(
        (micros_to_usd(e.budget.spent_micros) - want).abs() < 1e-6,
        "{:?}",
        e.budget
    );
    // Each call reserved its own model's worst case.
    let planned: Vec<f64> = rows(&r.core, &sid, "action.planned")
        .iter()
        .filter(|p| p["tool"] == "provider.messages")
        .map(|p| p["reserved_usd"].as_f64().unwrap())
        .collect();
    assert_eq!(planned.len(), 2, "{planned:?}");
    assert!(
        planned[1] > planned[0] * 2.0,
        "Sonnet 5's output costs 3 times: {planned:?}"
    );
    // The session keeps its own target: its next turn starts on Sonnet 5.5,
    // its request the fallback's extended, the refused answer left out by
    // the rule alone.
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.last_target.unwrap().model, "claude-sonnet-5-5");
    let next = turn(&r.core, &sid, None).await;
    assert_eq!(
        (next.model.as_str(), next.fallback),
        ("claude-sonnet-5-5", None)
    );
    let reqs = r.fake.requests();
    assert_eq!(
        &reqs[2].messages[..reqs[1].messages.len()],
        &reqs[1].messages[..]
    );
    let sent = serde_json::to_string(&reqs[2].messages).unwrap();
    assert!(
        sent.contains("The archive says four.") && !sent.contains("toolu_refused"),
        "{sent}"
    );
}

#[tokio::test]
async fn a_refusal_on_both_ends_the_turn_as_a_refusal_and_never_chains() {
    // Sonnet 5 given a fallback of its own: the turn still falls back once.
    let r = rig_with(vec![refused("cyber"), refused("cyber")], |c| {
        c.catalog.insert(
            "claude-sonnet-5".into(),
            crate::catalog::CatalogRow {
                refusal_fallback_model: Some("claude-opus-4-8".into()),
                ..Default::default()
            },
        );
    });
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(res.stop_reason, "refusal", "{res:?}");
    assert_eq!(r.fake.requests().len(), 2, "one fallback, no chain");
    assert_eq!(res.model, "claude-sonnet-5");
    let fell = res.fallback.expect("it fell back");
    assert!(!fell.answered);
    assert_eq!(
        fell.line(&res.stop_reason),
        "Sonnet 5.5 declined (cyber), and so did Sonnet 5."
    );
    assert_eq!(rows(&r.core, &sid, "provider.fallback").len(), 1);
    assert_eq!(rows(&r.core, &sid, "provider.refusal").len(), 2);
}

#[tokio::test]
async fn a_model_without_a_fallback_or_with_the_providers_own_refuses_as_before() {
    // Opus 5.5 names none.
    let r = rig(vec![refused("cyber"), Scripted::text("never sent")]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, Some("claude-opus-5-5")).await;
    assert_eq!(
        (res.stop_reason.as_str(), res.model.as_str()),
        ("refusal", "claude-opus-5-5")
    );
    assert_eq!(r.fake.requests().len(), 1);
    assert!(res.fallback.is_none());
    assert!(rows(&r.core, &sid, "provider.fallback").is_empty());
    // Opus 5 takes the provider's own fallback, which rides its request: a
    // client-side one named beside it is not used.
    let r = rig_with(vec![refused("cyber"), Scripted::text("never sent")], |c| {
        c.catalog.insert(
            "claude-opus-5".into(),
            crate::catalog::CatalogRow {
                refusal_fallback_model: Some("claude-opus-4-8".into()),
                ..Default::default()
            },
        );
    });
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, Some("claude-opus-5")).await;
    assert_eq!(res.stop_reason, "refusal");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].extra.get("fallbacks"), Some(&json!("default")));
    assert!(res.fallback.is_none());
}

#[tokio::test]
async fn the_switch_off_lets_a_refusal_end_its_turn() {
    let r = rig_with(vec![refused("cyber"), Scripted::text("never sent")], |c| {
        c.model.retries.refusal = false;
    });
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(
        (res.stop_reason.as_str(), res.model.as_str()),
        ("refusal", "claude-sonnet-5-5")
    );
    assert_eq!(r.fake.requests().len(), 1);
    assert!(res.fallback.is_none());
    assert!(rows(&r.core, &sid, "provider.fallback").is_empty());
}

/// b5's vulnerable-secret refused on its second loop, after a call. The
/// fallback's first request is the refused one on Sonnet 5, Sonnet 5.5's
/// thinking in it unchanged (the provider drops what Sonnet 5 cannot read);
/// its next request extends it with Sonnet 5's own answer, thinking kept, and
/// never the refused answer.
#[tokio::test]
async fn after_a_call_the_fallbacks_requests_keep_both_models_thinking_and_extend_each_other() {
    let thinking =
        |t: &str| json!({"type": "thinking", "thinking": t, "signature": format!("sig-{t}")});
    let call = |id: &str, t: &str| Scripted::Blocks {
        blocks: vec![
            thinking(t),
            json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": "harbor.txt"}}),
        ],
        stop_reason: "tool_use".into(),
    };
    let r = rig(vec![
        call("toolu_a", "five-five reads"),
        refused("cyber"),
        call("toolu_b", "five reads"),
        Scripted::text("Four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!((res.output.as_str(), res.loops), ("Four.", 4), "{res:?}");
    let reqs = r.fake.requests();
    let models: Vec<&str> = reqs.iter().map(|q| q.model.as_str()).collect();
    assert_eq!(
        models,
        [
            "claude-sonnet-5-5",
            "claude-sonnet-5-5",
            "claude-sonnet-5",
            "claude-sonnet-5"
        ]
    );
    assert_eq!(
        body(&reqs[2]),
        body(&reqs[1]),
        "the refused request, on Sonnet 5"
    );
    let text = |i: usize| serde_json::to_string(&reqs[i].messages).unwrap();
    assert!(text(2).contains("sig-five-five reads"), "{}", text(2));
    let n = reqs[2].messages.len();
    assert_eq!(&reqs[3].messages[..n], &reqs[2].messages[..], "an append");
    assert!(text(3).contains("sig-five reads"), "{}", text(3));
    for i in 2..4 {
        assert!(!text(i).contains("toolu_refused"), "{}", text(i));
    }
}

/// A fallback's request is one its own model takes (theseus-7gir.18): with
/// Haiku 4.5 named, it carries no adaptive thinking and asks for Haiku's
/// 64,000 tokens at most; and the provider's own fallback, which Opus 5
/// takes, rides no fallback's request: a turn falls back once.
#[tokio::test]
async fn a_fallbacks_request_is_one_its_model_takes_and_carries_no_fallback_of_its_own() {
    let naming = |to: &str| {
        let to = to.to_string();
        rig_with(vec![refused("cyber"), Scripted::text("Four.")], move |c| {
            c.catalog.insert(
                "claude-opus-5-5".into(),
                crate::catalog::CatalogRow {
                    refusal_fallback_model: Some(to),
                    ..Default::default()
                },
            );
        })
    };
    let r = naming("claude-haiku-4-5");
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, Some("claude-opus-5-5")).await;
    assert_eq!(
        (res.output.as_str(), res.model.as_str()),
        ("Four.", "claude-haiku-4-5")
    );
    let reqs = r.fake.requests();
    assert_eq!(reqs[0].thinking.as_ref().unwrap()["type"], "adaptive");
    assert_eq!(
        (reqs[1].thinking.as_ref(), reqs[1].max_tokens),
        (None, 64_000)
    );
    assert_eq!(reqs[1].messages, reqs[0].messages);
    let r = naming("claude-opus-5");
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, Some("claude-opus-5-5")).await;
    assert_eq!(res.model, "claude-opus-5");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 2);
    assert!(reqs[1].betas.is_empty(), "{:?}", reqs[1].betas);
    assert!(
        !reqs[1].extra.contains_key("fallbacks"),
        "{:?}",
        reqs[1].extra
    );
}

/// A fallback is another priced model of the same provider (theseus-7gir.18):
/// the config refuses one that is not, or the model itself.
#[test]
fn a_fallback_must_be_another_model_the_same_provider_serves() {
    let named = |model: &str, to: &str| {
        let mut c = Config::example();
        c.catalog.insert(
            model.into(),
            crate::catalog::CatalogRow {
                refusal_fallback_model: Some(to.into()),
                ..Default::default()
            },
        );
        c.validate().map_err(|e| format!("{e:#}"))
    };
    assert!(named("claude-opus-5-5", "claude-opus-5").is_ok());
    for (model, to) in [
        ("claude-sonnet-5-5", "glm-5.3"),
        ("claude-sonnet-5-5", "no-such-model"),
        ("claude-sonnet-5-5", "claude-sonnet-5-5"),
    ] {
        let e = named(model, to).expect_err(to);
        assert!(
            e.contains("refusal_fallback_model") && e.contains(to),
            "{e}"
        );
    }
}
