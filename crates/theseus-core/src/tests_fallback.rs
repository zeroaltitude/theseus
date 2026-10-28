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
use crate::test_support::session_rows_1k as rows;
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

/// The retry off (theseus-keuap): the tests that pin the fallback's own path
/// script a refusal the fallback answers at once.
fn no_retry(c: &mut Config) {
    c.model.retries.refusal_retry = false;
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
    try_turn(core, sid, model).await.expect("the turn ends")
}

/// `turn`, or the failure a turn that does not end ends with.
async fn try_turn(
    core: &Arc<Core>,
    sid: &str,
    model: Option<&str>,
) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core
        .runner
        .resolve_target(&live, None, None, model)
        .unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some("open the archive and read what the tide table says".into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
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
        answers: 1,
        refusals: 1,
        latched: true,
    };
    assert_eq!(res.fallback.as_ref(), Some(&fell));
    assert_eq!(
        fell.line(&res.stop_reason),
        // `cyber` latches, but the turn ended on that one answer, so there
        // was nothing to stay for: the line is the one it has always been.
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
        json!({"from": "claude-sonnet-5-5", "to": "claude-sonnet-5", "category": "cyber", "loop": 0, "refused": refused_node,
               "n": 1, "latched": true})
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

/// The fallback answers the refused request and no more (theseus-wf482): the
/// next request goes back to the routed model.
#[tokio::test]
async fn a_refusal_sends_one_request_to_the_fallback_and_the_next_goes_back() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "Opening the archive.",
                &[("toolu_a", "fs_read", json!({"path": "harbor.txt"}))],
            ),
            refused("reasoning_extraction"),
            Scripted::tools(
                "Reading it again.",
                &[("toolu_b", "fs_read", json!({"path": "harbor.txt"}))],
            ),
            Scripted::text("Four."),
        ],
        no_retry,
    );
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert!(res.output.ends_with("Four."), "{}", res.output);
    assert_eq!(res.loops, 4, "{res:?}");
    let models: Vec<String> = r.fake.requests().iter().map(|q| q.model.clone()).collect();
    assert_eq!(
        models,
        [
            "claude-sonnet-5-5",
            "claude-sonnet-5-5",
            "claude-sonnet-5",
            "claude-sonnet-5-5"
        ]
    );
}

/// A script's tool call, every one of them the same read.
fn read(id: &str) -> Scripted {
    Scripted::tools(
        "Reading it again.",
        &[(id, "fs_read", json!({"path": "harbor.txt"}))],
    )
}

/// The models of a turn's requests, in order.
fn models(r: &Rig) -> Vec<String> {
    r.fake.requests().iter().map(|q| q.model.clone()).collect()
}

const ROUTED: &str = "claude-sonnet-5-5";
const FALLBACK: &str = "claude-sonnet-5";

/// Three refusals with no routed answer between them (theseus-wf482): the
/// third keeps the turn on the fallback, for the rest of it.
#[tokio::test]
async fn three_refusals_in_a_row_latch_the_turn_on_the_fallback() {
    let r = rig_with(
        vec![
            read("toolu_a"),
            refused("reasoning_extraction"),
            read("toolu_b"),
            refused("reasoning_extraction"),
            read("toolu_c"),
            refused("reasoning_extraction"),
            read("toolu_d"),
            Scripted::text("Four."),
        ],
        no_retry,
    );
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(res.loops, 8, "{res:?}");
    assert_eq!(
        models(&r),
        [ROUTED, ROUTED, FALLBACK, ROUTED, FALLBACK, ROUTED, FALLBACK, FALLBACK]
    );
    let fell = res.fallback.as_ref().expect("it fell back");
    assert_eq!(
        (fell.refusals, fell.answers, fell.latched, fell.answered),
        (3, 4, true, true),
        "{fell:?}"
    );
    assert_eq!(res.model, FALLBACK);
    // One row per switch, the third saying it stayed.
    let switches = rows(&r.core, &sid, "provider.fallback");
    let flags: Vec<(u64, bool)> = switches
        .iter()
        .map(|w| (w["n"].as_u64().unwrap(), w["latched"].as_bool().unwrap()))
        .collect();
    assert_eq!(flags, [(1, false), (2, false), (3, true)], "{switches:?}");
    assert_eq!(rows(&r.core, &sid, "provider.routed_back").len(), 2);
}

/// A routed answer between refusals ends the run of them: refuse, answer,
/// refuse, answer, refuse never stays on the fallback.
#[tokio::test]
async fn a_routed_answer_between_refusals_resets_the_count() {
    let r = rig_with(
        vec![
            read("toolu_a"),
            refused("reasoning_extraction"),
            read("toolu_b"),
            read("toolu_c"),
            refused("reasoning_extraction"),
            read("toolu_d"),
            read("toolu_e"),
            refused("reasoning_extraction"),
            Scripted::text("Four."),
        ],
        no_retry,
    );
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(res.loops, 9, "{res:?}");
    assert_eq!(
        models(&r),
        [ROUTED, ROUTED, FALLBACK, ROUTED, ROUTED, FALLBACK, ROUTED, ROUTED, FALLBACK]
    );
    let fell = res.fallback.as_ref().expect("it fell back");
    assert_eq!(
        (fell.refusals, fell.answers, fell.latched),
        (3, 3, false),
        "{fell:?}"
    );
    assert_eq!(rows(&r.core, &sid, "provider.fallback").len(), 3);
    assert_eq!(rows(&r.core, &sid, "provider.routed_back").len(), 3);
    // The turn ended back on its routed model.
    assert_eq!(res.model, FALLBACK, "the last answer was the fallback's");
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.last_target.unwrap().model, ROUTED);
}

/// A genuine decline never goes back to the model that declined it: a
/// hard-decline category keeps the turn on the fallback at the first.
#[tokio::test]
async fn a_hard_decline_category_latches_at_once() {
    for category in ["cyber", "bio", "general_harms"] {
        let r = rig(vec![
            read("toolu_a"),
            refused(category),
            read("toolu_b"),
            Scripted::text("Four."),
        ]);
        let sid = bound_session(&r.core);
        let res = turn(&r.core, &sid, None).await;
        assert_eq!(
            models(&r),
            [ROUTED, ROUTED, FALLBACK, FALLBACK],
            "{category}"
        );
        assert!(res.fallback.as_ref().unwrap().latched, "{category}");
        assert!(rows(&r.core, &sid, "provider.routed_back").is_empty());
    }
    // The categories are the config's.
    let r = rig_with(
        vec![
            read("toolu_a"),
            refused("cyber"),
            read("toolu_b"),
            Scripted::text("Four."),
        ],
        |c| {
            c.model.retries.hard_decline_categories = vec![];
            no_retry(c);
        },
    );
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(models(&r), [ROUTED, ROUTED, FALLBACK, ROUTED]);
    assert!(!res.fallback.unwrap().latched);
}

/// The request that goes back is the routed model's: its own output cap, the
/// system blocks the first request carried, the fallback's request extended,
/// and the fallback's thinking, which the routed model cannot read, left out
/// (theseus-wf482). Haiku 4.5 as Opus 5.5's fallback caps at 64,000.
#[tokio::test]
async fn the_return_keeps_the_cached_prefix_and_the_routed_output_cap() {
    let thinking =
        |t: &str| json!({"type": "thinking", "thinking": t, "signature": format!("sig-{t}")});
    let call = |id: &str, t: &str| Scripted::Blocks {
        blocks: vec![
            thinking(t),
            json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": "harbor.txt"}}),
        ],
        stop_reason: "tool_use".into(),
    };
    let r = rig_with(
        vec![
            call("toolu_a", "routed reads"),
            refused("reasoning_extraction"),
            call("toolu_b", "fallback reads"),
            Scripted::text("Four."),
        ],
        |c| {
            c.catalog.insert(
                "claude-opus-5-5".into(),
                crate::catalog::CatalogRow {
                    refusal_fallback_model: Some("claude-haiku-4-5".into()),
                    ..Default::default()
                },
            );
            no_retry(c);
        },
    );
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, Some("claude-opus-5-5")).await;
    assert_eq!(res.loops, 4, "{res:?}");
    let reqs = r.fake.requests();
    let named: Vec<&str> = reqs.iter().map(|q| q.model.as_str()).collect();
    assert_eq!(
        named,
        [
            "claude-opus-5-5",
            "claude-opus-5-5",
            "claude-haiku-4-5",
            "claude-opus-5-5"
        ]
    );
    // The output cap: Haiku's on its one request, the routed model's again.
    assert_eq!(reqs[2].max_tokens, 64_000);
    assert_ne!(reqs[0].max_tokens, 64_000, "the routed cap differs");
    assert_eq!(reqs[3].max_tokens, reqs[0].max_tokens);
    // The prefix the provider caches is the first request's, byte for byte.
    assert_eq!(
        serde_json::to_string(&reqs[3].system).unwrap(),
        serde_json::to_string(&reqs[0].system).unwrap()
    );
    // The fallback's request extended, its own answer's thinking left out.
    let text = |i: usize| serde_json::to_string(&reqs[i].messages).unwrap();
    let n = reqs[2].messages.len();
    assert_eq!(&reqs[3].messages[..n], &reqs[2].messages[..], "an append");
    assert!(text(3).contains("sig-routed reads"), "{}", text(3));
    assert!(text(3).contains("toolu_b"), "{}", text(3));
    assert!(!text(3).contains("sig-fallback reads"), "{}", text(3));
    assert!(!text(3).contains("toolu_refused"), "{}", text(3));
    // The fallback's own request kept the thinking it was handed.
    assert!(!text(2).contains("toolu_refused"), "{}", text(2));
}

/// Each switch and each return is a row, and the turn's end carries the
/// counts, so the ledger needs no join.
#[tokio::test]
async fn the_ledger_rows_say_each_fallback_answer() {
    let r = rig_with(
        vec![
            read("toolu_a"),
            refused("reasoning_extraction"),
            read("toolu_b"),
            refused("reasoning_extraction"),
            Scripted::text("Four."),
        ],
        no_retry,
    );
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(models(&r), [ROUTED, ROUTED, FALLBACK, ROUTED, FALLBACK]);
    let switches = rows(&r.core, &sid, "provider.fallback");
    assert_eq!(switches.len(), 2, "{switches:?}");
    for (w, (n, at)) in switches.iter().zip([(1, 1), (2, 3)]) {
        assert_eq!(
            (w["from"].as_str(), w["to"].as_str(), w["category"].as_str()),
            (Some(ROUTED), Some(FALLBACK), Some("reasoning_extraction"))
        );
        assert_eq!((w["n"].as_u64(), w["loop"].as_u64()), (Some(n), Some(at)));
        assert_eq!(w["latched"], false);
    }
    let back = rows(&r.core, &sid, "provider.routed_back");
    assert_eq!(
        back,
        [
            json!({"to": ROUTED, "loop": 2, "after": FALLBACK}),
            json!({"to": ROUTED, "loop": 4, "after": FALLBACK})
        ]
    );
    let fell = TurnFallback {
        from: ROUTED.into(),
        to: FALLBACK.into(),
        category: Some("reasoning_extraction".into()),
        answered: true,
        answers: 2,
        refusals: 2,
        latched: false,
    };
    assert_eq!(res.fallback.as_ref(), Some(&fell));
    assert_eq!(
        fell.line(&res.stop_reason),
        "Sonnet 5.5 declined (reasoning_extraction) twice; Sonnet 5 answered twice."
    );
    let ended = rows(&r.core, &sid, "turn.ended");
    assert_eq!(ended[0]["fallback"], json!(fell), "{ended:?}");
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

mod timeout;

// ---- the same-model retry (theseus-keuap) ----------------------------------
//
// A refusal of a category that is not a genuine decline is made once more on
// the routed model before the fallback answers: at most one retry per refused
// request, three per turn, never for a hard decline.

/// The retry is the refused request again, on the same model, the refused
/// answer left out; it answers, and nothing falls back.
#[tokio::test]
async fn a_refusal_is_retried_on_the_same_model_and_the_retry_answering_ends_it() {
    let r = rig(vec![
        refused("reasoning_extraction"),
        Scripted::text("The archive says four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert!(res.output.ends_with("The archive says four."), "{res:?}");
    assert_eq!(models(&r), [ROUTED, ROUTED]);
    let reqs = r.fake.requests();
    assert_eq!(body(&reqs[1]), body(&reqs[0]), "the request, once more");
    let sent = serde_json::to_string(&reqs[1].messages).unwrap();
    assert!(!sent.contains("toolu_refused"), "{sent}");
    // No fallback, no change of target, one row for the retry.
    assert!(res.fallback.is_none(), "{res:?}");
    assert_eq!(res.model, ROUTED);
    assert!(rows(&r.core, &sid, "provider.fallback").is_empty());
    assert!(rows(&r.core, &sid, "provider.routed_back").is_empty());
    let retries = rows(&r.core, &sid, "provider.refusal_retry");
    assert_eq!(retries.len(), 1, "{retries:?}");
    assert_eq!(retries[0]["model"], ROUTED);
    assert_eq!(retries[0]["category"], "reasoning_extraction");
    assert_eq!(
        (retries[0]["loop"].as_u64(), retries[0]["n"].as_u64()),
        (Some(0), Some(1))
    );
    // The retry is a call as any other: both are the turn's spend and each
    // reserved its own worst case.
    let calls = rows(&r.core, &sid, "provider.call");
    assert_eq!(calls.len(), 2);
    let usage = |i: usize| serde_json::from_value(calls[i]["usage"].clone()).unwrap();
    let want = priced(&r.core, ROUTED, &usage(0)) + priced(&r.core, ROUTED, &usage(1));
    let e = execution(&r.core, &sid);
    assert!(
        (micros_to_usd(e.budget.spent_micros) - want).abs() < 1e-6,
        "{:?}",
        e.budget
    );
    let planned = rows(&r.core, &sid, "action.planned")
        .iter()
        .filter(|p| p["tool"] == "provider.messages")
        .count();
    assert_eq!(planned, 2);
}

/// The retry refusing too is the refusal of the fallback's path, unchanged:
/// the fallback answers that request and the next goes back to the routed
/// model. One retry per refused request, never two.
#[tokio::test]
async fn a_retry_that_refuses_too_falls_back_as_before() {
    let r = rig(vec![
        read("toolu_a"),
        refused("reasoning_extraction"),
        refused("reasoning_extraction"),
        read("toolu_b"),
        Scripted::text("Four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(models(&r), [ROUTED, ROUTED, ROUTED, FALLBACK, ROUTED]);
    assert!(res.output.ends_with("Four."), "{res:?}");
    assert_eq!(rows(&r.core, &sid, "provider.refusal_retry").len(), 1);
    let switches = rows(&r.core, &sid, "provider.fallback");
    assert_eq!(switches.len(), 1, "{switches:?}");
    assert_eq!(switches[0]["category"], "reasoning_extraction");
    assert_eq!(switches[0]["latched"], false);
    // The fallback's request leaves out both refused answers.
    let reqs = r.fake.requests();
    let sent = serde_json::to_string(&reqs[3].messages).unwrap();
    assert!(!sent.contains("toolu_refused"), "{sent}");
    assert_eq!(rows(&r.core, &sid, "provider.routed_back").len(), 1);
    let fell = res.fallback.as_ref().expect("it fell back");
    assert_eq!(
        (fell.refusals, fell.answers, fell.latched),
        (1, 1, false),
        "{fell:?}"
    );
}

/// A retry is not a routed refusal toward the latch: the streak counts the
/// refusals the fallback had to answer, and a retry that answers ends the run.
#[tokio::test]
async fn a_retry_adds_nothing_to_the_refusal_streak() {
    let r = rig(vec![
        read("toolu_a"),
        // One refusal, its retry refused: the fallback answers (streak 1).
        refused("reasoning_extraction"),
        refused("reasoning_extraction"),
        read("toolu_b"),
        // A second the same way (streak 2).
        refused("reasoning_extraction"),
        refused("reasoning_extraction"),
        read("toolu_c"),
        // A third whose retry answers: the routed model answered, the run ends.
        refused("reasoning_extraction"),
        read("toolu_d"),
        Scripted::text("Four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(
        models(&r),
        [ROUTED, ROUTED, ROUTED, FALLBACK, ROUTED, ROUTED, FALLBACK, ROUTED, ROUTED, ROUTED]
    );
    let fell = res.fallback.as_ref().expect("it fell back");
    assert_eq!((fell.refusals, fell.latched), (2, false), "{fell:?}");
    assert_eq!(rows(&r.core, &sid, "provider.refusal_retry").len(), 3);
    assert_eq!(rows(&r.core, &sid, "provider.fallback").len(), 2);
}

/// A genuine decline is never retried: it goes straight to the fallback and
/// the turn stays there, as before.
#[tokio::test]
async fn a_hard_decline_is_not_retried() {
    for category in ["cyber", "bio", "general_harms"] {
        let r = rig(vec![refused(category), Scripted::text("Four.")]);
        let sid = bound_session(&r.core);
        let res = turn(&r.core, &sid, None).await;
        assert_eq!(models(&r), [ROUTED, FALLBACK], "{category}");
        assert!(rows(&r.core, &sid, "provider.refusal_retry").is_empty());
        assert_eq!(
            rows(&r.core, &sid, "provider.fallback").len(),
            1,
            "{category}"
        );
        assert!(res.fallback.as_ref().unwrap().latched, "{category}");
    }
}

/// The bounds: one retry per refused request (the second refusal of a
/// request goes to the fallback), and three per turn (a fourth refusal goes
/// straight to it).
#[tokio::test]
async fn a_turn_makes_at_most_three_retries_and_one_per_request() {
    let r = rig(vec![
        refused("reasoning_extraction"),
        read("toolu_a"),
        refused("reasoning_extraction"),
        read("toolu_b"),
        refused("reasoning_extraction"),
        read("toolu_c"),
        // The turn's fourth refusal: no retry is left.
        refused("reasoning_extraction"),
        Scripted::text("Four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(
        models(&r),
        [ROUTED, ROUTED, ROUTED, ROUTED, ROUTED, ROUTED, ROUTED, FALLBACK]
    );
    assert_eq!(rows(&r.core, &sid, "provider.refusal_retry").len(), 3);
    let switches = rows(&r.core, &sid, "provider.fallback");
    assert_eq!(switches.len(), 1, "{switches:?}");
    assert_eq!(switches[0]["loop"], 6);
    assert!(res.output.ends_with("Four."), "{res:?}");
}

/// The latch under the default: three refusal episodes, each retried and each
/// retry refusing too, still count as three, so the third stays on the
/// fallback (theseus-keuap). The latch is the only thing that stops a genuine
/// decline bouncing back to the model that declined it.
#[tokio::test]
async fn three_retried_refusal_episodes_still_latch_the_turn() {
    let r = rig(vec![
        read("toolu_a"),
        refused("reasoning_extraction"),
        refused("reasoning_extraction"),
        read("toolu_b"),
        refused("reasoning_extraction"),
        refused("reasoning_extraction"),
        read("toolu_c"),
        refused("reasoning_extraction"),
        refused("reasoning_extraction"),
        read("toolu_d"),
        Scripted::text("Four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(
        models(&r),
        [
            ROUTED, ROUTED, ROUTED, FALLBACK, ROUTED, ROUTED, FALLBACK, ROUTED, ROUTED, FALLBACK,
            FALLBACK
        ]
    );
    assert_eq!(rows(&r.core, &sid, "provider.refusal_retry").len(), 3);
    let switches = rows(&r.core, &sid, "provider.fallback");
    let flags: Vec<(u64, bool)> = switches
        .iter()
        .map(|w| (w["n"].as_u64().unwrap(), w["latched"].as_bool().unwrap()))
        .collect();
    assert_eq!(flags, [(1, false), (2, false), (3, true)], "{switches:?}");
    assert_eq!(rows(&r.core, &sid, "provider.routed_back").len(), 2);
    let fell = res.fallback.as_ref().expect("it fell back");
    assert_eq!((fell.refusals, fell.latched), (3, true), "{fell:?}");
    assert_eq!(res.model, FALLBACK);
}
