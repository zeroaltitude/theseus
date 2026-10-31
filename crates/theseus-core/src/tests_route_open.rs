//! route.v10 acting (theseus-h66v4 row 3), on the daemon's own turn path and the
//! fake Jev: the pack asks `model` and `effort`, and the turn runs on the model
//! Jev named, at the effort it named, from the whole working context (there is
//! no `context` question and no detour: theseus-h66v4.1).
//!
//! `ROUTE_PACK` still names route.v8, so these tests ask a fixture shaped like
//! route.v10 in its place (`JudgeService::stand_in`, which keeps every name the
//! daemon compares meaning what it means). The v8 path's own tests (`tests_route`
//! and the rest) are unchanged and are the proof the live path is undisturbed.

use std::sync::Arc;

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

/// Jev's two answers at one confidence.
pub(crate) fn says(jev: &FakeJev, model: &str, effort: &str, confidence: f64) {
    says_at(jev, (model, confidence), (effort, confidence));
}

/// Jev's two answers, each at its own confidence.
pub(crate) fn says_at(jev: &FakeJev, model: (&str, f64), effort: (&str, f64)) {
    for (q, (option, confidence)) in [("model", model), ("effort", effort)] {
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
fn the_verdict_reads_model_and_effort_from_a_v10_judgment() {
    use crate::tests_route_data::{choice, judgment};
    let j = judgment(
        vec![
            choice("model", "opus", 0.91),
            choice("effort", "medium", 0.84),
        ],
        false,
    );
    let v = crate::judge::inbound::verdict(&j, "trn_1").expect("a verdict");
    let m = v.model.as_ref().expect("the model answer");
    assert_eq!(
        (m.choice.as_str(), m.confidence, m.sure),
        ("opus", 0.91, true)
    );
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
    let j = judgment(vec![choice("model", "opus", 0.45)], false);
    let v = crate::judge::inbound::verdict(&j, "trn_1").unwrap();
    assert!(!v.model.as_ref().unwrap().sure);
    // A pinned turn is not asked the model or the effort: it has no verdict.
    let j = judgment(vec![choice("personal", "none", 0.95)], false);
    assert!(crate::judge::inbound::verdict(&j, "trn_1").is_none());
    // route.v8's judgment reads as it always did.
    let j = judgment(vec![choice("mode", "chat", 0.95)], false);
    let v = crate::judge::inbound::verdict(&j, "trn_1").unwrap();
    assert_eq!(v.mode, "chat");
    assert!(!v.open() && v.model.is_none());
}

// ------------------------------------------------------------------ the turn

/// Jev names Opus at high effort for the whole context: the request goes to
/// Opus at high, the session moves, and the row says what Jev said.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jev_names_opus_and_the_turn_runs_there_at_the_effort_it_named() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.95);
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
        ["route.v8/effort", "route.v8/model", "route.v8/personal"],
        "the two deciding questions, and no `mode` and no `context`"
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
    assert!(d.get("context").is_none(), "no context answer on the row");
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

/// An effort answer under the confirm bar runs the model's own default effort,
/// not the profile's `effort` (the template's sonnet profile sets none, haiku's
/// sets `low`: Haiku's catalog default is medium). The pack's `no_match` for
/// `effort` is `high` (theseus-h66v4.1) and the replay grades an under-bar
/// answer as it; this is what the daemon does today (see the h66v4.1 report).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_effort_under_the_bar_is_the_models_own_and_not_the_profiles() {
    let jev = FakeJev::start().unwrap();
    says_at(&jev, ("haiku", 0.95), ("high", 0.40));
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
    says(&jev, "opus", "high", 0.45);
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
    says(&jev, "deepseek_flash", "low", 0.95);
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
    says(&jev, "deepseek_flash", "low", 0.99);
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
    says(&jev, "fable", "low", 0.95);
    let o = open(&jev, 1, |c| c.remove_profile("fable"));
    let res = turn(&o.r.core, None, "Design a thing nobody has designed.", None).await;
    assert_eq!(o.r.claude.requests()[0].model, "claude-sonnet-5-5");
    assert_eq!(res.route.unwrap().reason, "fallback");
    assert_eq!(
        decided(&o.r.core.store)[0]["detail"],
        "no profile runs claude-fable-5-1"
    );
}

/// theseus-h66v4.1: there is no detour. A thank-you that Jev sends to Haiku on
/// a long session compiles the session's whole working context, on Haiku: the
/// request carries every message, the session's compilation is written as
/// ever, and the row says it is no detour.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_for_a_thank_you_still_compiles_the_whole_context() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
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
    says(&jev, "sonnet", "low", 0.95);
    let res = turn(&o.r.core, Some(&sid), "ok", None).await;
    assert_eq!(res.profile, "sonnet");
    let reqs = o.r.claude.requests();
    let (before, thanks) = (&reqs[4], &reqs[5]);
    assert!(
        thanks.messages.len() > before.messages.len(),
        "every earlier message rides, and the new ones: {} against {}",
        thanks.messages.len(),
        before.messages.len()
    );
    assert_eq!(
        thanks.messages[..before.messages.len()],
        before.messages[..],
        "the prefix, byte for byte"
    );
    let d = decided(&o.r.core.store).pop().unwrap();
    assert_eq!(d["detour"], false);
}

/// The cache hold, kept: a pick over the cold line waits one agreeing turn,
/// and the next agreeing pick switches.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_pick_over_the_cold_line_waits_one_agreeing_turn() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.95);
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
    says(&jev, "opus", "high", 0.95);
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
    says(&jev, "haiku", "low", 0.97);
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
        asked.contains(&"route.v8/personal".to_string()),
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

/// `theseus why` names the model, the quoted block, the effort and the reason
/// in every case, honestly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn why_names_the_block_and_the_reason_in_every_case() {
    // A verdict.
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "medium", 0.95);
    let o = open(&jev, 1, |_| {});
    let res = turn(&o.r.core, None, "Review this plan for mistakes.", None).await;
    let lines = why(&o, &res.session_id);
    let l = routing_line(&lines);
    assert!(l.contains("model=claude-opus-5-5 (jev 0.95: \"Opus 5.5 (Anthropic, US). … For reviewing code, a plan or an argument for mistakes"), "{l}");
    assert!(
        l.contains("effort=medium (0.95) reason=verdict pack="),
        "{l}"
    );
    // The data gate.
    let jev = FakeJev::start().unwrap();
    says(&jev, "deepseek_flash", "low", 0.9);
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
    says(&jev, "haiku", "low", 0.9);
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
    says(&jev, "opus", "medium", 0.95);
    let o = open(&jev, 1, |c| c.routing.cold_switch_tokens = 1);
    let res = turn(&o.r.core, None, "Review the segment format.", None).await;
    let lines = why(&o, &res.session_id);
    assert!(
        routing_line(&lines).contains("reason=cache_hold"),
        "{lines:?}"
    );
    // A fallback.
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "low", 0.95);
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
    says(&jev, "deepseek_flash", "low", 0.95);
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
    says(&jev, "sonnet", "high", 0.95);
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

// ------------------------------------------- the data floor, on the v10 path

/// The data rule on route.v10's own path, not v8's: a session whose labels keep
/// a China-origin model off it, and a turn that would run on one (its base is
/// `dsflash`, because the session was moved there, or the operator pinned it),
/// runs the `[model]` default with `data_gate`, and nothing reaches the
/// restricted provider or the chokepoint. Whatever Jev picks or whether it is
/// sure. The floor is `data_floor`, which every pack's turn meets; this test
/// goes red if a refactor takes route.v10 off it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restricted_base_runs_the_default_with_data_gate_under_route_v10() {
    use crate::content::Flag;
    use crate::content_wait::judging::Judging;
    use crate::routing::Routed;
    use crate::session::SessionRecord;
    use theseus_protocol::SessionKind;

    #[derive(Debug, Clone, Copy)]
    enum Base {
        /// The session was moved to Flash; Jev is unsure, so the base stays.
        MovedUnsure,
        /// The session was moved to Flash; Jev names Flash again, sure.
        MovedNamesFlash,
        /// The session was moved to Flash; Jev says a US model but says it unsure of context: stays.
        MovedNoModel,
        /// The operator pinned Flash for the turn.
        Pinned,
        /// A fresh session; Jev names Flash, sure.
        FreshNamesFlash,
    }
    for case in [
        Base::MovedUnsure,
        Base::MovedNamesFlash,
        Base::MovedNoModel,
        Base::Pinned,
        Base::FreshNamesFlash,
    ] {
        let jev = FakeJev::start().unwrap();
        match case {
            Base::MovedUnsure => says(&jev, "opus", "high", 0.2),
            Base::MovedNamesFlash | Base::FreshNamesFlash => {
                says(&jev, "deepseek_flash", "low", 0.97)
            }
            Base::MovedNoModel => says(&jev, "sonnet", "high", 0.2),
            Base::Pinned => says(&jev, "sonnet", "high", 0.97),
        }
        let o = open(&jev, 2, |_| {});
        crate::tests_dm_rig::judging_with(&o.r, Judging::with(|_, _| vec![Flag::Confidential]));
        let moved = matches!(
            case,
            Base::MovedUnsure | Base::MovedNamesFlash | Base::MovedNoModel
        );
        let res = match case {
            _ if moved => {
                let mut rec = SessionRecord::new(SessionKind::Conversation, None);
                rec.routed = Some(Box::new(Routed {
                    profile: Some("dsflash".into()),
                    from: Some("sonnet".into()),
                    hold: None,
                }));
                o.r.core.store.put_session(&rec.session_id, &rec).unwrap();
                turn(
                    &o.r.core,
                    Some(&rec.session_id),
                    "Rename the call sites.",
                    None,
                )
                .await
            }
            Base::Pinned => turn(&o.r.core, None, "Rename the call sites.", Some("dsflash")).await,
            _ => turn(&o.r.core, None, "Rename the call sites.", None).await,
        };
        // A restricted base is replaced by the default; a fresh session's pick of
        // Flash never leaves Sonnet.
        let (profile, model) = match case {
            Base::FreshNamesFlash => ("sonnet", "claude-sonnet-5-5"),
            _ => ("default", "claude-sonnet-5-5"),
        };
        assert_eq!(
            (res.profile.as_str(), res.model.as_str()),
            (profile, model),
            "{case:?}"
        );
        let rows = decided(&o.r.core.store);
        let last = rows.last().unwrap_or_else(|| panic!("{case:?}: no row"));
        assert_eq!(last["reason"], "data_gate", "{case:?}: {rows:?}");
        assert_eq!(res.route.as_ref().unwrap().reason, "data_gate", "{case:?}");
        assert!(
            o.deepseek.requests().is_empty(),
            "{case:?}: Flash was sent nothing"
        );
        assert!(
            crate::tests_dm_rig::blocked(&o.r.core) == 0,
            "{case:?}: it never reached the chokepoint"
        );
        assert!(!o.r.claude.requests().is_empty(), "{case:?}");
        assert_ne!(
            last["origin"], "cn",
            "{case:?}: the turn did not run a China-origin model"
        );
    }
}
