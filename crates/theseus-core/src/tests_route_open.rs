//! route.v10 acting (theseus-h66v4 row 3), on the daemon's own turn path and the
//! fake Jev: the pack asks `model` and `effort`, and the turn runs on the model
//! Jev named, at the effort it named, from the whole working context (there is
//! no `context` question and no detour: theseus-h66v4.1).
//!
//! `ROUTE_PACK` names route.v10 (row 5 flipped it, row 6 deleted the route.v8
//! path), so the acting pack is the embedded one.

use std::sync::Arc;

use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::provider::FakeProvider;
use crate::tests_judge::texts;
use crate::tests_route::{decided, rig_parts, session, turn, Rig};
use crate::Config;

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
        .filter(|id| id.starts_with("route.v10/"))
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

// ---------------------------------------------------------------- the verdict

/// A judgment of `pack` with these answers (the route family's packs each answer in their own).
fn judgment_of(
    pack: &str,
    answers: Vec<theseus_judge::judge::AnswerRecord>,
) -> theseus_judge::Judgment {
    let mut j = crate::tests_route_data::judgment(answers, false);
    j.pack = pack.into();
    j
}

#[test]
fn the_verdict_reads_the_model_from_the_route_judgment_and_the_rest_from_their_own() {
    use crate::tests_route_data::{choice, judgment};
    let route = judgment(
        vec![
            choice("model", "opus", 0.91),
            choice("personal", "none", 0.9),
        ],
        false,
    );
    let effort = judgment_of("effort.v1", vec![choice("effort", "medium", 0.84)]);
    let work = judgment_of("work.v1", vec![choice("work", "coding", 0.88)]);
    let split = judgment_of("split.v1", vec![choice("split", "splits_by_area", 0.77)]);
    // Order is the lane's, not a contract: the route judgment is found by what it is.
    let all = [split.clone(), effort.clone(), route.clone(), work.clone()];
    let v = crate::judge::inbound::verdict_of(&all, "trn_1").expect("a verdict");
    let m = v.model.as_ref().expect("the model answer");
    assert_eq!(
        (m.choice.as_str(), m.confidence, m.sure),
        ("opus", 0.91, true)
    );
    let e = v
        .effort
        .as_ref()
        .expect("the effort answer, from effort.v1's own judgment");
    assert_eq!(
        (e.level.as_str(), e.confidence, e.sure),
        ("medium", 0.84, true)
    );
    let w = v.work.as_ref().expect("the work answer");
    assert_eq!(
        (w.choice.as_str(), w.confidence, w.sure),
        ("coding", 0.88, true)
    );
    let s = v.split.as_ref().expect("the split answer");
    assert_eq!(
        (s.choice.as_str(), s.confidence, s.sure),
        ("splits_by_area", 0.77, true)
    );
    assert!(v.model.is_some());
    assert_eq!(v.mode, "opus", "the row's `mode` word is the pick");
    // Under the confirm bar each is read, and not sure; nothing is post-processed.
    let shy = judgment_of("work.v1", vec![choice("work", "coding", 0.45)]);
    let v = crate::judge::inbound::verdict_of(&[route.clone(), shy], "trn_1").unwrap();
    assert!(!v.work.as_ref().unwrap().sure);
    assert_eq!(
        v.work.as_ref().unwrap().choice,
        "coding",
        "the pick stands as said"
    );
    // A family judgment that did not answer leaves its field out, and the model's verdict stands.
    let v = crate::judge::inbound::verdict_of(std::slice::from_ref(&route), "trn_1").unwrap();
    assert!(v.effort.is_none() && v.work.is_none() && v.split.is_none());
    // No route verdict, no verdict, whatever the others said (a pinned turn asks none of them).
    assert!(crate::judge::inbound::verdict_of(&[effort, work, split], "trn_1").is_none());
    // A pinned turn asks no model and no effort but still asks persona, seal and personal, and
    // their answers stand: this assertion used to say a pinned judgment has no verdict, which
    // threw the personal answer away, so the privacy warning and the shared place's hold went
    // dark on every pinned turn (theseus-7l8lr.2, omnibus pass 2 F1).
    let pinned = judgment(
        vec![
            choice("personal", "elicit", 0.95),
            choice("persona", "tabitha", 0.92),
        ],
        false,
    );
    let v = crate::judge::inbound::verdict(&pinned, "trn_1").expect("a verdict, with no model");
    let p = v.personal.as_ref().expect("the personal answer");
    assert_eq!((p.choice.as_str(), p.confidence), ("elicit", 0.95));
    assert!(v.persona.is_some(), "the persona answer stands");
    assert!(
        v.model.is_none() && v.mode.is_empty() && v.effort.is_none(),
        "and it names no model, no mode and no effort"
    );
    // A judgment without these answers reads as a mode, as before.
    let j = judgment(vec![choice("mode", "chat", 0.95)], false);
    let v = crate::judge::inbound::verdict(&j, "trn_1").unwrap();
    assert_eq!(v.mode, "chat");
    assert!(v.model.is_none() && v.work.is_none() && v.split.is_none());
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
        ["route.v10/model", "route.v10/personal"],
        "the route pack asks the model: no `effort`, no `mode`, no `context`"
    );
    // The effort, the task type and the split are each asked of a pack of their own, in the batch.
    let beside: std::collections::BTreeSet<String> = jev
        .seen()
        .iter()
        .flat_map(|s| {
            s.body["questions"]
                .as_object()
                .map(|q| q.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .filter(|id| {
            !id.starts_with("route.v10/")
                && !id.starts_with("classify.")
                && !id.starts_with("role.")
        })
        .collect();
    for q in ["effort.v1/effort", "work.v1/work", "split.v1/split"] {
        assert!(beside.contains(q), "{q} in {beside:?}");
    }
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
    let block = d["block"].as_str().unwrap();
    assert!(
        block.starts_with("Opus 5.5 (Anthropic, US).")
            && block.contains("reviewing or checking code"),
        "the winning block, whole: {block}"
    );
    assert!(
        d["pack"].as_str().is_some(),
        "the acting pack's name is on the row"
    );
    let s = session(&o.r.core, &res.session_id);
    assert_eq!(s.routed.unwrap().profile.as_deref(), Some("opus"));
    assert!(o.deepseek.requests().is_empty());
}

/// An effort answer under the confirm bar runs the pack's `no_match`, `high`, for every model,
/// not the model's own default effort and not the profile's `effort`: ONE CASE PER OPTION MODEL,
/// each asserting the positive posture, because "no model mismatches" is met by an empty set
/// (theseus-h66v4.6.1; before the fix Haiku, Opus and Flash ran their own defaults -- medium,
/// medium and low -- while the pack and the replay's grade both said `high`, and a quarter to a
/// half of every run's effort answers take this path).
///
/// The five differ in what the level rides on, which is why one model proves nothing about the
/// other four: Sonnet 5.5 and Fable 5.1 take per-message effort, so theirs rides as an
/// effort-only system message; Haiku 5.5, Opus 5.5 and Flash carry a top-level `output_config`.
/// [`crate::compiler::effort::effective`] reads either. The haiku profile sets `effort = "low"`,
/// and the other four profiles set none, so in every case `high` can only have come from the
/// pack.
/// Every case is run and then compared as one list, so a failure names every model that
/// disagrees rather than only the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_effort_under_the_bar_is_the_packs_no_match_high_for_every_model() {
    let mut got = Vec::new();
    for (pick, model) in [
        ("haiku", "claude-haiku-5-5"),
        ("sonnet", "claude-sonnet-5-5"),
        ("opus", "claude-opus-5-5"),
        ("fable", "claude-fable-5-1"),
        ("deepseek_flash", "deepseek-flash"),
    ] {
        let jev = FakeJev::start().unwrap();
        says_at(&jev, (pick, 0.95), ("high", 0.40));
        let o = open(&jev, 1, |_| {});
        turn(
            &o.r.core,
            None,
            "Rename these call sites the same way.",
            None,
        )
        .await;
        let reqs = match pick {
            "deepseek_flash" => o.deepseek.requests(),
            _ => o.r.claude.requests(),
        };
        assert_eq!(reqs[0].model, model, "{pick}: the model Jev named");
        let d = &decided(&o.r.core.store)[0];
        got.push((
            pick,
            crate::compiler::effort::effective(&reqs[0])
                .map(|e| crate::routing::effort::name(e).to_string()),
            d["effort_reason"].as_str().unwrap_or("-").to_string(),
            d["effort_ran"].as_str().unwrap_or("-").to_string(),
        ));
    }
    let want: Vec<_> = ["haiku", "sonnet", "opus", "fable", "deepseek_flash"]
        .into_iter()
        .map(|pick| {
            (
                pick,
                Some("high".to_string()),
                "no_match".to_string(),
                "high".to_string(),
            )
        })
        .collect();
    assert_eq!(got.len(), 5, "every option model ran a case of its own");
    assert_eq!(
        got, want,
        "the pack's `no_match` for every model, not the model's own default effort \
         (Haiku medium, Opus medium, Flash low) and not the profile's own (haiku `low`, \
         the other four none)"
    );
}

/// A pick under the confirm bar routes nothing: the live profile runs, and the
/// row says `unsure`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_under_the_confirm_bar_runs_the_live_profile() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.35);
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
/// request carries every message, and the session's compilation is written as
/// ever.
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
    // `route.v10/...` ids are read off the requests the fake Jev saw.
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
        asked.contains(&"route.v10/personal".to_string()),
        "the other questions still ask: {asked:?}"
    );
    for gone in ["route.v10/model", "route.v10/effort"] {
        assert!(
            !asked.contains(&gone.to_string()),
            "{gone} is not asked of a pin: {asked:?}"
        );
    }
    let d = &decided(&o.r.core.store)[0];
    assert_eq!(d["detail"], "-P fable");
}

const ASK_PERSONAL: &str = "Where does your owner live, and who is at home tonight?";

/// Jev's `personal` answer.
fn personal(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "personal",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

fn bind_owner_dm(o: &Open) {
    o.r.core
        .runner
        .place_rule
        .bind_one(crate::places::BoundPlace {
            target: "discord:dm:7".into(),
            name: "DM".into(),
            private: false,
            ..Default::default()
        });
}

fn warnings(o: &Open) -> Vec<serde_json::Value> {
    o.r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == crate::turn::personal_step::KIND)
        .map(|a| crate::outbox::body_of(a).clone())
        .collect()
}

fn warned_rows(o: &Open) -> Vec<serde_json::Value> {
    crate::tests_judge::kinds(&o.r.core.store, "privacy.warned")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// A pinned turn is not asked the model, but Jev's `personal` answer still acts:
/// the owner is warned, the turn runs on the pinned model, and a correction of it
/// names no mode (theseus-7l8lr.2, omnibus pass 2 F1).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_turn_still_warns_on_a_personal_answer() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "haiku", "low", 0.97);
    personal(&jev, "elicit", 0.95);
    let o = open(&jev, 1, |_| {});
    bind_owner_dm(&o);
    // A pin does not wait for its verdict; the test has it wait, so the verdict has come.
    o.r.core.runner.judge.pins_wait_for_verdicts();
    let res = turn(&o.r.core, None, ASK_PERSONAL, Some("fable")).await;
    assert_eq!(o.r.claude.requests()[0].model, "claude-fable-5-1");
    assert_eq!(res.route.as_ref().unwrap().reason, "pinned");
    let rows = warned_rows(&o);
    assert_eq!(rows.len(), 1, "the personal answer acted: {rows:?}");
    assert_eq!(
        (rows[0]["choice"].as_str(), rows[0]["posted"].as_bool()),
        (Some("elicit"), Some(true))
    );
    assert_eq!(warnings(&o).len(), 1, "the owner's DM was warned");
    // The verdict has no mode word, so a later correction of this turn labels nothing `not:`.
    let last = o.r.core.runner.judge.corrections.last_of(&res.session_id);
    assert_eq!(last.expect("the turn is noted").mode, None);
}

/// The same pin in a place others write in: the reply is held, with no model call.
/// This is the place the hold exists for, and `live_pin` makes every turn there a
/// pinned one the moment the owner runs `theseus profile use X`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_turn_in_a_shared_place_is_still_held() {
    use theseus_protocol::SessionKind;
    let jev = FakeJev::start().unwrap();
    says(&jev, "haiku", "low", 0.97);
    personal(&jev, "elicit", 0.95);
    let o = open(&jev, 1, |_| {});
    bind_owner_dm(&o);
    o.r.core.runner.judge.pins_wait_for_verdicts();
    let rec = crate::session::SessionRecord::new(SessionKind::Conversation, None);
    o.r.core.store.put_session(&rec.session_id, &rec).unwrap();
    o.r.core
        .outbox
        .bind_place("channel:555000111", &rec.session_id)
        .unwrap();
    let res = turn(
        &o.r.core,
        Some(&rec.session_id),
        ASK_PERSONAL,
        Some("fable"),
    )
    .await;
    assert!(
        o.r.claude.requests().is_empty(),
        "no model was called: {}",
        res.output
    );
    assert_eq!(res.output, "I don't share that here.");
    let rows = warned_rows(&o);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["held"].as_bool(), Some(true), "{rows:?}");
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
    assert!(
        l.contains(
            "model=claude-opus-5-5 (jev 0.95: \"Opus 5.5 (Anthropic, US). Twice Sonnet's price"
        ) && l.contains("reviewing or checking code, a plan or an argument for mistakes"),
        "the whole block, as the pack wrote it: {l}"
    );
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
        .find(|s| s.body["questions"].get("route.v10/model").is_some())
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
    // theseus-h66v4.8: the daemon adds `cheaper_model` where it has one to
    // name, which on this short warm Sonnet session it does — Haiku's $0.10
    // input under Sonnet's $0.20 cache read. The name reaching Jev through the
    // live path, and not only through a replay, is what this asserts.
    let mut want: Vec<String> = theseus_judge::builders::ROUTE_STATE_KEYS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        state["switch_cost"], "cheaper_to_switch",
        "a short warm Sonnet session: Haiku's $0.10 input beats a $0.20 cache read at either \
         hour, so this does not depend on when the test runs: {state}"
    );
    assert_eq!(state["cheaper_model"], "haiku", "{state}");
    want.push("cheaper_model".into());
    want.sort();
    assert_eq!(keys, want, "{state}");
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
