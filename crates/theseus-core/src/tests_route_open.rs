//! route.v10 acting (theseus-h66v4 row 3), on the daemon's own turn path and the
//! fake Jev: the pack asks `model`, `effort` and `context`, and the turn runs on
//! the model Jev named, at the effort it named, from the context it named.
//!
//! `ROUTE_PACK` still names route.v8, so these tests ask a fixture shaped like
//! route.v10 in its place (`JudgeService::stand_in`, which keeps every name the
//! daemon compares meaning what it means). The v8 path's own tests (`tests_route`
//! and the rest) are unchanged and are the proof the live path is undisturbed.

use std::sync::Arc;

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_judge::Pack;

use crate::provider::FakeProvider;
use crate::tests_judge::texts;
use crate::tests_route::{decided, rig_parts, session, turn, Rig};
use crate::Config;

/// The fixture pack, as the acting route pack (named route.v8 for the daemon).
pub(crate) fn open_pack() -> Pack {
    let mut p = Pack::parse(include_str!("../fixtures/packs/route.open.toml")).unwrap();
    assert_eq!(p.name(), "route.v10", "the fixture is shaped like v10");
    p.version = 8;
    p
}

pub(crate) struct Open {
    pub(crate) r: Rig,
    pub(crate) deepseek: Arc<FakeProvider>,
}

/// A rig on the template's profiles, plus `dsflash` (Flash on the `deepseek`
/// fake), with route.v10's shape acting.
pub(crate) fn open(jev: &FakeJev, n: usize, tweak: impl FnOnce(&mut Config)) -> Open {
    let deepseek = Arc::new(FakeProvider::scripted(texts(n)));
    let ds = deepseek.clone();
    let r = rig_parts(
        Some(jev),
        n,
        |c| {
            c.serve_deepseek_flash();
            tweak(c);
        },
        |p| {
            p.providers.insert("deepseek".into(), ds);
        },
    );
    r.core.runner.judge.stand_in("route.v8", open_pack());
    Open { r, deepseek }
}

/// Jev's three answers at one confidence.
pub(crate) fn says(jev: &FakeJev, model: &str, effort: &str, context: &str, confidence: f64) {
    for (q, option) in [("model", model), ("effort", effort), ("context", context)] {
        jev.script(
            q,
            Jev::Choice {
                option: option.into(),
                confidence,
            },
        );
    }
}

fn asked(jev: &FakeJev) -> Vec<String> {
    let mut ids: Vec<String> = jev
        .seen()
        .iter()
        .flat_map(|s| {
            s.body["questions"]
                .as_object()
                .map(|q| q.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .filter(|id| id.starts_with("route.v8/"))
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

// ---------------------------------------------------------------- the verdict

#[test]
fn the_verdict_reads_model_effort_and_context_from_a_v10_judgment() {
    use crate::tests_route_data::{choice, judgment};
    let j = judgment(
        vec![
            choice("model", "opus", 0.91),
            choice("effort", "medium", 0.84),
            choice("context", "short", 0.77),
        ],
        false,
    );
    let v = crate::judge::inbound::verdict(&j, "trn_1").expect("a verdict");
    let m = v.model.as_ref().expect("the model answer");
    assert_eq!(
        (m.choice.as_str(), m.confidence, m.sure),
        ("opus", 0.91, true)
    );
    let c = v.context.as_ref().expect("the context answer");
    assert_eq!((c.choice.as_str(), c.sure), ("short", true));
    let e = v
        .effort
        .as_ref()
        .expect("the effort answer, under route.v10's own id");
    assert_eq!(
        (e.level.as_str(), e.confidence, e.sure),
        ("medium", 0.84, true)
    );
    assert!(v.open());
    assert_eq!(v.mode, "opus", "the row's `mode` word is the pick");
    // An answer under the confirm bar is read, and not sure.
    let j = judgment(
        vec![
            choice("model", "opus", 0.45),
            choice("context", "short", 0.30),
        ],
        false,
    );
    let v = crate::judge::inbound::verdict(&j, "trn_1").unwrap();
    assert!(!v.model.as_ref().unwrap().sure && !v.short());
    // A pinned turn is not asked the model or the effort: its verdict is the context alone.
    let j = judgment(vec![choice("context", "short", 0.95)], false);
    let v = crate::judge::inbound::verdict(&j, "trn_1").expect("a context-only verdict");
    assert!(v.model.is_none() && v.effort.is_none() && v.short());
    // route.v8's judgment reads as it always did.
    let j = judgment(vec![choice("mode", "chat", 0.95)], false);
    let v = crate::judge::inbound::verdict(&j, "trn_1").unwrap();
    assert_eq!(v.mode, "chat");
    assert!(!v.open() && v.model.is_none() && v.context.is_none());
}

// ------------------------------------------------------------------ the turn

/// Jev names Opus at high effort for the whole context: the request goes to
/// Opus at high, the session moves, and the row says what Jev said.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jev_names_opus_and_the_turn_runs_there_at_the_effort_it_named() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", "full", 0.95);
    let o = open(&jev, 1, |_| {});
    let res = turn(
        &o.r.core,
        None,
        "Review this plan for the write-ahead log for mistakes.",
        None,
    )
    .await;
    let req = &o.r.claude.requests()[0];
    assert_eq!(req.model, "claude-opus-5-5");
    assert_eq!(req.output_config.as_ref().unwrap()["effort"], "high");
    assert_eq!(
        asked(&jev),
        [
            "route.v8/context",
            "route.v8/effort",
            "route.v8/model",
            "route.v8/personal"
        ],
        "the three deciding questions, and no `mode`"
    );
    let route = res.route.unwrap();
    assert_eq!(
        (
            route.mode.as_deref(),
            route.reason.as_str(),
            route.from.as_str()
        ),
        (Some("opus"), "verdict", "sonnet")
    );
    let d = &decided(&o.r.core.store)[0];
    assert_eq!(d["pick"], "opus");
    assert_eq!(d["context"], "full");
    assert_eq!(d["profile"], "opus");
    assert_eq!(d["switch"], true);
    assert_eq!(d["effort_ran"], "high");
    assert!(
        d["block"]
            .as_str()
            .unwrap()
            .starts_with("Opus 5.5 (Anthropic, US). … For reviewing code"),
        "the winning block's sentences: {}",
        d["block"]
    );
    assert!(
        d["pack"].as_str().is_some(),
        "the acting pack's name is on the row"
    );
    let s = session(&o.r.core, &res.session_id);
    assert_eq!(s.routed.unwrap().profile.as_deref(), Some("opus"));
    assert!(o.deepseek.requests().is_empty());
}

/// `default` effort is the model's own default, not the profile's `effort`
/// (the template's sonnet profile sets none, haiku's sets `low`: Haiku's
/// catalog default is medium).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_default_effort_is_the_models_own_and_not_the_profiles() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "haiku", "default", "full", 0.95);
    let o = open(&jev, 1, |_| {});
    turn(
        &o.r.core,
        None,
        "Rename these call sites the same way.",
        None,
    )
    .await;
    let req = &o.r.claude.requests()[0];
    assert_eq!(req.model, "claude-haiku-5-5");
    assert_eq!(
        req.output_config.as_ref().unwrap()["effort"],
        "medium",
        "Haiku 5.5's default effort, not the haiku profile's `low`"
    );
    let d = &decided(&o.r.core.store)[0];
    assert_eq!(d["effort_reason"], "model_default");
}

/// A pick under the confirm bar routes nothing: the live profile runs, and the
/// row says `unsure`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_under_the_confirm_bar_runs_the_live_profile() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "default", "full", 0.45);
    let o = open(&jev, 1, |_| {});
    let res = turn(&o.r.core, None, "hm, what about the other thing", None).await;
    assert_eq!(o.r.claude.requests()[0].model, "claude-sonnet-5-5");
    assert_eq!(res.profile, "sonnet");
    assert_eq!(res.route.unwrap().reason, "unsure");
}

/// Flash, for a session the data rule lets use it: the request goes to the
/// `deepseek` provider.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flash_pick_runs_on_deepseek_when_the_session_may_use_it() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "deepseek_flash", "low", "full", 0.95);
    let o = open(&jev, 1, |_| {});
    let res = turn(
        &o.r.core,
        None,
        "Rename these twelve call sites the same way.",
        None,
    )
    .await;
    assert_eq!(o.deepseek.requests()[0].model, "deepseek-flash");
    assert!(o.r.claude.requests().is_empty());
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(decided(&o.r.core.store)[0]["origin"], "cn");
}

/// The data rule is never loosened: where the session may not use a model of
/// restricted origin, a Flash pick runs the live profile with `data_gate`,
/// whatever Jev said and however sure it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flash_pick_the_floor_refuses_runs_the_live_profile_with_data_gate() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "deepseek_flash", "low", "full", 0.99);
    let o = open(&jev, 1, |c| {
        c.content_based_security.cn_models = crate::config::content_based_security::CnModels::Never;
    });
    let res = turn(
        &o.r.core,
        None,
        "Rename these twelve call sites the same way.",
        None,
    )
    .await;
    assert!(
        o.deepseek.requests().is_empty(),
        "nothing went to a restricted provider"
    );
    assert_eq!(o.r.claude.requests()[0].model, "claude-sonnet-5-5");
    let route = res.route.unwrap();
    assert_eq!(route.reason, "data_gate");
    assert_eq!(route.data.unwrap().gate.as_deref(), Some("never"));
    let d = &decided(&o.r.core.store)[0];
    assert_eq!(d["pick"], "deepseek_flash", "what Jev picked is on the row");
    assert_eq!(d["detail"], "restricted_models=not_allowed");
}

/// A pick with no profile that runs its model runs the live profile, `fallback`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_no_profile_runs_is_a_fallback_that_says_why() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "low", "full", 0.95);
    let o = open(&jev, 1, |c| c.remove_profile("fable"));
    let res = turn(&o.r.core, None, "Design a thing nobody has designed.", None).await;
    assert_eq!(o.r.claude.requests()[0].model, "claude-sonnet-5-5");
    assert_eq!(res.route.unwrap().reason, "fallback");
    assert_eq!(
        decided(&o.r.core.store)[0]["detail"],
        "no profile runs claude-fable-5-1"
    );
}

/// A `short` pick answers from the persona and the last exchange on the model
/// it names, for this message alone: the session's compilation, profile and
/// `last_target` stay, and the next full turn's prefix is byte for byte the
/// one before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_pick_answers_from_the_last_exchange_and_leaves_the_session_alone() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "default", "full", 0.95);
    let o = open(&jev, 3, |_| {});
    let first = turn(
        &o.r.core,
        None,
        "What does the store's manifest hold?",
        None,
    )
    .await;
    let sid = first.session_id.clone();
    let before = session(&o.r.core, &sid);
    says(&jev, "haiku", "default", "short", 0.95);
    let thanks = turn(&o.r.core, Some(&sid), "thank you!", None).await;
    assert_eq!(
        (thanks.profile.as_str(), thanks.model.as_str()),
        ("haiku", "claude-haiku-5-5")
    );
    assert_eq!(thanks.route.as_ref().unwrap().reason, "verdict");
    let reqs = o.r.claude.requests();
    assert_eq!(reqs[1].model, "claude-haiku-5-5");
    assert_eq!(
        reqs[1].messages.len(),
        3,
        "the last exchange and the message"
    );
    let after = session(&o.r.core, &sid);
    assert_eq!(
        after.compilation_id, before.compilation_id,
        "no compilation written"
    );
    assert_eq!(after.last_target, before.last_target);
    assert!(
        after.routed.is_none(),
        "`short` is never a switch of the session"
    );
    let d = decided(&o.r.core.store).pop().unwrap();
    assert_eq!(
        (d["detour"].clone(), d["context"].clone()),
        (Value::Bool(true), "short".into())
    );
    // The next full turn: the session's own prefix, undisturbed.
    says(&jev, "sonnet", "default", "full", 0.95);
    let next = turn(&o.r.core, Some(&sid), "And where is it written?", None).await;
    assert_eq!(next.profile, "sonnet");
    let reqs = o.r.claude.requests();
    let (a, c) = (&reqs[0], &reqs[2]);
    assert_eq!(
        c.messages[..a.messages.len()],
        a.messages[..],
        "the prefix, byte for byte"
    );
}

/// A `short` pick on the session's own model is still the short compile: only
/// the last exchanges answer it, not the whole conversation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_pick_on_the_sessions_own_model_is_still_the_short_compile() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "default", "full", 0.95);
    let o = open(&jev, 6, |_| {});
    let first = turn(
        &o.r.core,
        None,
        "What does the store's manifest hold?",
        None,
    )
    .await;
    let sid = first.session_id.clone();
    for ask in ["And where is it written?", "Who reads it?", "When?", "Why?"] {
        turn(&o.r.core, Some(&sid), ask, None).await;
    }
    let before = session(&o.r.core, &sid);
    says(&jev, "sonnet", "default", "short", 0.95);
    let res = turn(&o.r.core, Some(&sid), "ok", None).await;
    assert_eq!(res.profile, "sonnet");
    let reqs = o.r.claude.requests();
    let (full, short) = (&reqs[4], &reqs[5]);
    assert_eq!(short.model, "claude-sonnet-5-5");
    assert!(
        short.messages.len() < full.messages.len(),
        "short {} against the whole conversation's {}",
        short.messages.len(),
        full.messages.len()
    );
    let after = session(&o.r.core, &sid);
    assert_eq!(after.compilation_id, before.compilation_id);
    assert!(decided(&o.r.core.store).pop().unwrap()["detour"]
        .as_bool()
        .unwrap());
}

/// The cache hold, kept: a full pick over the cold line waits one agreeing
/// turn, and the next agreeing pick switches. A `short` pick between never
/// holds and never breaks it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_pick_over_the_cold_line_waits_one_agreeing_turn() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "default", "full", 0.95);
    let o = open(&jev, 4, |c| c.routing.cold_switch_tokens = 1);
    let one = turn(
        &o.r.core,
        None,
        "Review the segment format for mistakes.",
        None,
    )
    .await;
    assert_eq!(one.profile, "sonnet");
    assert_eq!(one.route.as_ref().unwrap().reason, "cache_hold");
    let hold = session(&o.r.core, &one.session_id)
        .routed
        .unwrap()
        .hold
        .unwrap();
    assert_eq!(
        (hold.profile.as_str(), hold.mode.as_str()),
        ("opus", "opus")
    );
    // A short pick between: runs, holds nothing, leaves the hold.
    says(&jev, "haiku", "default", "short", 0.95);
    let mid = turn(&o.r.core, Some(&one.session_id), "ok", None).await;
    assert_eq!(mid.route.as_ref().unwrap().reason, "verdict");
    let kept = session(&o.r.core, &one.session_id).routed.unwrap().hold;
    assert_eq!(
        kept.map(|h| h.profile),
        Some("opus".into()),
        "a short pick leaves the hold"
    );
    says(&jev, "opus", "default", "full", 0.95);
    let two = turn(
        &o.r.core,
        Some(&one.session_id),
        "And its recovery path?",
        None,
    )
    .await;
    assert_eq!(
        (
            two.profile.as_str(),
            two.route.as_ref().unwrap().reason.as_str()
        ),
        ("opus", "verdict")
    );
    assert!(session(&o.r.core, &one.session_id)
        .routed
        .unwrap()
        .hold
        .is_none());
}

// ------------------------------------------------------------------ the pin

/// A turn the operator pinned (`-P`) is not asked the model or the effort; the
/// other questions still ask. The pinned model runs, `reason = pinned`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_turn_is_not_asked_the_model_or_the_effort() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "haiku", "low", "full", 0.97);
    let o = open(&jev, 1, |_| {});
    // `route.v8/...` ids are read off the requests the fake Jev saw.
    let res = turn(
        &o.r.core,
        None,
        "design the memory layer for a million turns",
        Some("fable"),
    )
    .await;
    assert_eq!(o.r.claude.requests()[0].model, "claude-fable-5-1");
    assert_eq!(res.route.unwrap().reason, "pinned");
    crate::tests_route::until_calls(&jev, 2).await;
    let asked = asked(&jev);
    assert!(
        asked.contains(&"route.v8/context".to_string()),
        "the other questions still ask: {asked:?}"
    );
    for gone in ["route.v8/model", "route.v8/effort"] {
        assert!(
            !asked.contains(&gone.to_string()),
            "{gone} is not asked of a pin: {asked:?}"
        );
    }
    let d = &decided(&o.r.core.store)[0];
    assert_eq!(d["detail"], "-P fable");
}

// ------------------------------------------------------------------- `why`

fn why(o: &Open, sid: &str) -> Vec<String> {
    let x =
        o.r.core
            .context_explain(
                &theseus_protocol::context::ContextExplainParams {
                    session_id: sid.into(),
                    turn_id: None,
                },
                true,
            )
            .unwrap();
    let route = decided(&o.r.core.store).pop();
    theseus_protocol::why::why_lines(&theseus_protocol::why::WhyInput {
        explain: &x,
        policy: None,
        route: route.as_ref(),
        tools: &[],
        hide_sources: false,
    })
}

fn routing_line(lines: &[String]) -> &str {
    lines
        .iter()
        .find(|l| l.starts_with("routing: "))
        .expect("a routing line")
}

/// `theseus why` names the model, the quoted block, the effort, the context
/// and the reason in every case, honestly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn why_names_the_block_and_the_reason_in_every_case() {
    // A verdict.
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "medium", "full", 0.95);
    let o = open(&jev, 1, |_| {});
    let res = turn(&o.r.core, None, "Review this plan for mistakes.", None).await;
    let lines = why(&o, &res.session_id);
    let l = routing_line(&lines);
    assert!(l.contains("model=claude-opus-5-5 (jev 0.95: \"Opus 5.5 (Anthropic, US). … For reviewing code, a plan or an argument for mistakes"), "{l}");
    assert!(
        l.contains("effort=medium (0.95) context=full reason=verdict pack="),
        "{l}"
    );
    // The data gate.
    let jev = FakeJev::start().unwrap();
    says(&jev, "deepseek_flash", "low", "full", 0.9);
    let o = open(&jev, 1, |c| {
        c.content_based_security.cn_models = crate::config::content_based_security::CnModels::Never;
    });
    let res = turn(&o.r.core, None, "Rename these call sites.", None).await;
    let lines = why(&o, &res.session_id);
    let l = routing_line(&lines);
    assert!(
        l.contains("reason=data_gate (restricted_models=not_allowed)"),
        "{l}"
    );
    assert!(l.contains("jev picked deepseek_flash"), "{l}");
    assert!(l.contains("model=claude-sonnet-5-5"), "what ran: {l}");
    // A pin.
    let jev = FakeJev::start().unwrap();
    says(&jev, "haiku", "low", "full", 0.9);
    let o = open(&jev, 1, |_| {});
    let res = turn(&o.r.core, None, "design the memory layer", Some("fable")).await;
    let lines = why(&o, &res.session_id);
    let l = routing_line(&lines);
    assert!(
        l.contains("model=claude-fable-5-1") && l.contains("reason=pinned (-P fable)"),
        "{l}"
    );
    // The cache hold.
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "medium", "full", 0.95);
    let o = open(&jev, 1, |c| c.routing.cold_switch_tokens = 1);
    let res = turn(&o.r.core, None, "Review the segment format.", None).await;
    let lines = why(&o, &res.session_id);
    assert!(
        routing_line(&lines).contains("reason=cache_hold"),
        "{lines:?}"
    );
    // A fallback.
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "low", "full", 0.95);
    let o = open(&jev, 1, |c| c.remove_profile("fable"));
    let res = turn(&o.r.core, None, "Design a thing.", None).await;
    let lines = why(&o, &res.session_id);
    let l = routing_line(&lines);
    assert!(
        l.contains("reason=fallback (no profile runs claude-fable-5-1)"),
        "{l}"
    );
}

/// The template ships `deepseek_api_key` as a placeholder reference. A Flash
/// pick on it is a fallback at pick time, not a call that fails at dispatch,
/// and `why` says "deepseek: no key" and nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_placeholder_key_is_no_key_at_pick_time() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "deepseek_flash", "low", "full", 0.95);
    let o = open(&jev, 1, |c| {
        c.secrets.insert(
            "deepseek_api_key".into(),
            "op://<your vault>/<DeepSeek API key item>/notesPlain".into(),
        );
    });
    let res = turn(
        &o.r.core,
        None,
        "Rename these twelve call sites the same way.",
        None,
    )
    .await;
    assert!(o.deepseek.requests().is_empty(), "no call to dispatch");
    assert_eq!(o.r.claude.requests()[0].model, "claude-sonnet-5-5");
    assert_eq!(res.route.unwrap().reason, "fallback");
    assert_eq!(decided(&o.r.core.store)[0]["detail"], "deepseek: no key");
    let lines = why(&o, &res.session_id);
    let l = routing_line(&lines);
    assert!(l.contains("reason=fallback (deepseek: no key)"), "{l}");
    assert!(!l.contains("op://") && !l.contains("vault"), "{l}");
}

// -------------------------------------------------------- the state's one shape

/// Finding B: what the daemon sends Jev for a plain message holds exactly the
/// fields a replay case's state is held to (`ROUTE_STATE_KEYS`), and its
/// `est_tokens` is the conversation's size.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemons_state_has_the_fields_a_replay_case_is_held_to() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "default", "full", 0.95);
    let o = open(&jev, 2, |_| {});
    let first = turn(
        &o.r.core,
        None,
        "What does the store's manifest hold?",
        None,
    )
    .await;
    turn(
        &o.r.core,
        Some(&first.session_id),
        "And where is it written?",
        None,
    )
    .await;
    let state = jev
        .seen()
        .iter()
        .rev()
        .find(|s| s.body["questions"].get("route.v8/model").is_some())
        .map(|s| s.body["state"].clone())
        .expect("a route request");
    let dynamic = [
        "previous_human_message",
        "last_reply",
        "minutes_since_last_message",
    ];
    let mut keys: Vec<String> = state
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| !dynamic.contains(&k.as_str()))
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, theseus_judge::builders::ROUTE_STATE_KEYS, "{state}");
    assert!(state["est_tokens"].as_u64().unwrap() > 0, "{state}");
    assert_eq!(state["restricted_models"], "allowed");
}
