//! A background job's model (route.v10 row 4, theseus-ej6bj): the one rule
//! every site that is not a turn keeps, against the fake Jev.
//!
//! A configured profile key is a pin; an unset key asks Jev through the job
//! builder (one paragraph, the place `job`), waiting at most `[routing]
//! max_wait_ms` (the design's 300 ms), and a job that cannot use the answer
//! runs on `live`. Private material never reaches a model of China origin.
//! The pack that asks `model` is route.v10's, which is not the acting route
//! pack until its row flips it, so these tests install a small one with
//! `JudgeService::job_pack_with`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_judge::builders::{JobKind, RestrictedModels};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_judge::Pack;

use crate::job_model::{Choice, How, Site, Why};
use crate::provider::FakeProvider;
use crate::tests_route::{rig_parts, Rig};

const PACK: &str = include_str!("judge/tests_job_pack.toml");

/// The test pack, parsed.
pub(crate) fn model_pack() -> Arc<Pack> {
    Arc::new(Pack::parse(PACK).expect("the test pack parses"))
}

/// Jev's `model` answer.
pub(crate) fn pick(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "model",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

/// A rig with the test pack acting. `max_wait_ms` is the design's 300 ms.
pub(crate) fn rig_job(jev: &FakeJev, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let r = rig_parts(
        Some(jev),
        1,
        |c| {
            c.routing.max_wait_ms = 300;
            tweak(c);
        },
        |p| {
            // DeepSeek's provider, so a Flash pick has somewhere to go.
            p.providers
                .insert("deepseek".into(), Arc::new(FakeProvider::default()));
        },
    );
    r.core.runner.judge.job_pack_with(model_pack());
    r
}

/// A card-sized job over the owner's material: US options only.
pub(crate) fn owner_site<'a>(pin: Option<&'a str>) -> Site<'a> {
    Site {
        name: "card",
        pin,
        legacy: Some(crate::config::legacy::CHEAP),
        job: JobKind::Card { clusters: 7 },
        restricted: RestrictedModels::NotAllowed,
    }
}

/// A job that is no one's private material, in a session whose labels allow
/// a model of restricted origin.
fn open_site<'a>() -> Site<'a> {
    Site {
        name: "compaction",
        pin: None,
        legacy: None,
        job: JobKind::Compaction {
            oldest_tokens: 9_000,
            summary_tokens: 4_096,
            labels: Vec::new(),
        },
        restricted: RestrictedModels::Allowed,
    }
}

fn model_of(c: &Choice) -> &str {
    match c {
        Choice::Model { model, .. } => model,
        Choice::Profile(p) => p,
    }
}

/// The state of the one request Jev saw.
fn state_seen(jev: &FakeJev) -> String {
    let seen = jev.seen();
    assert_eq!(seen.len(), 1, "one request");
    seen[0].body["state"].to_string()
}

/// An unset key asks Jev, as one paragraph in the place `job`, with the
/// facts, and runs on the model it names.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unset_key_asks_jev_through_the_job_builder() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "opus", 0.95);
    let r = rig_job(&jev, |_| {});
    let (choice, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    assert_eq!(
        how,
        How::Jev {
            option: "opus".into(),
            confidence: 0.95
        }
    );
    assert_eq!(
        choice,
        Choice::Model {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into()
        }
    );
    assert_eq!(jev.connections(), 1);
    let state = state_seen(&jev);
    assert!(state.contains("job"), "the place: {state}");
    assert!(state.contains("7 clusters"), "the paragraph: {state}");
    assert!(state.contains("not_allowed"), "the floor: {state}");
    assert!(state.contains("price_period"), "the facts: {state}");
}

/// A configured key is a pin: the job runs on it and Jev is asked nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_configured_key_is_a_pin_and_asks_nothing() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "opus", 0.95);
    let r = rig_job(&jev, |_| {});
    let (choice, how) = r
        .core
        .runner
        .job_choice("sonnet", owner_site(Some("opus")))
        .await;
    assert_eq!((choice, how), (Choice::Profile("opus".into()), How::Pinned));
    assert_eq!(jev.connections(), 0);
}

/// While the acting pack asks no `model` (every pack before route.v10), an
/// unset key runs on the profile it always did and asks nothing: installing
/// the row changes no live decision.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn while_the_pack_asks_no_model_an_unset_key_runs_its_legacy_profile() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "opus", 0.95);
    let r = rig_parts(Some(&jev), 1, |c| c.routing.max_wait_ms = 300, |_| {});
    let (choice, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    assert_eq!(
        (choice, how),
        (Choice::Profile("haiku".into()), How::Legacy)
    );
    // A site whose legacy is the live profile runs on it.
    let (choice, how) = r.core.runner.job_choice("sonnet", open_site()).await;
    assert_eq!(
        (choice, how),
        (Choice::Profile("sonnet".into()), How::Legacy)
    );
    assert_eq!(jev.connections(), 0);
}

/// The design's hard requirement: a background ask never blocks a job past
/// `max_wait_ms`. Jev holds its answer (lateness by order, not by a delay a
/// loaded host could erase); the job runs on `live` at about 300 ms, not
/// when Jev answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_jev_never_holds_a_job_past_max_wait() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "opus", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig_job(&jev, |c| c.judge.total_secs = 30);
    let t0 = Instant::now();
    let (choice, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    let waited = t0.elapsed();
    assert_eq!(
        (choice, how),
        (Choice::Profile("sonnet".into()), How::Live(Why::Late))
    );
    assert!(
        waited >= Duration::from_millis(250),
        "it waited for the answer: {waited:?}"
    );
    assert!(
        waited < Duration::from_millis(2_500),
        "it did not wait for Jev: {waited:?}"
    );
    // The answer that comes after is dropped; it moves nothing.
    jev.release();
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// A Jev that answers after a delay past the bound, by its own latency, is
/// just as late.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_jev_slower_than_the_bound_by_latency_is_late_too() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "opus", 0.95);
    jev.set_latency(Duration::from_millis(1_500), Duration::from_millis(1_500));
    let r = rig_job(&jev, |c| c.judge.total_secs = 30);
    let t0 = Instant::now();
    let (_, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    assert_eq!(how, How::Live(Why::Late));
    assert!(
        t0.elapsed() < Duration::from_millis(1_400),
        "{:?}",
        t0.elapsed()
    );
}

/// A Jev that is down gives no answer at once: `live`, with no wait to speak of.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_jev_that_is_down_runs_the_job_on_live() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Down);
    let r = rig_job(&jev, |_| {});
    let (choice, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    assert_eq!(choice, Choice::Profile("sonnet".into()));
    assert!(matches!(how, How::Live(_)), "{how:?}");
}

/// An answer under the pack's confirm bar is not a pick: `live`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_under_the_confirm_bar_runs_the_job_on_live() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "opus", 0.2);
    let r = rig_job(&jev, |_| {});
    let (choice, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    assert_eq!(
        (choice, how),
        (Choice::Profile("sonnet".into()), How::Live(Why::Unsure))
    );
}

/// The data rule: the owner's material never reaches a China-origin model.
/// The state says `not_allowed`, and a pick of Flash anyway runs on `live`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_material_never_goes_to_a_china_origin_model() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, |_| {});
    let (choice, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    assert_eq!(
        (choice, how),
        (Choice::Profile("sonnet".into()), How::Live(Why::DataGate))
    );
    assert!(state_seen(&jev).contains("not_allowed"));
}

/// The floor is the job's, not the caller's: a caller that says `allowed` for
/// a job over the owner's material is still refused Flash, and the state
/// still says `not_allowed` (the builder floors it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_caller_cannot_widen_the_owner_material_floor() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, |_| {});
    let site = Site {
        restricted: RestrictedModels::Allowed,
        ..owner_site(None)
    };
    let (choice, how) = r.core.runner.job_choice("sonnet", site).await;
    assert_eq!(
        (choice, how),
        (Choice::Profile("sonnet".into()), How::Live(Why::DataGate))
    );
    assert!(state_seen(&jev).contains("not_allowed"));
}

/// A job that is not private, in a session whose labels allow it, may be sent
/// to Flash: the rule refuses what the data rule refuses, nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_the_labels_allow_may_name_a_china_origin_model() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, |_| {});
    let (choice, how) = r.core.runner.job_choice("sonnet", open_site()).await;
    assert!(matches!(how, How::Jev { .. }), "{how:?}");
    assert_eq!(model_of(&choice), "deepseek-flash");
    assert!(state_seen(&jev).contains("\"allowed\""));
}

/// And the same job in a session whose labels keep restricted models off is
/// told `not_allowed`, and refused Flash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_in_a_labelled_session_is_refused_a_china_origin_model() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, |_| {});
    let site = Site {
        restricted: RestrictedModels::NotAllowed,
        ..open_site()
    };
    let (_, how) = r.core.runner.job_choice("sonnet", site).await;
    assert_eq!(how, How::Live(Why::DataGate));
}

/// A pick whose provider is not configured runs on `live`
/// (`reason = fallback`), not as a failed call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_whose_provider_is_not_there_runs_on_live() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    // No `deepseek` provider in this rig.
    let r = rig_parts(Some(&jev), 1, |c| c.routing.max_wait_ms = 300, |_| {});
    r.core.runner.judge.job_pack_with(model_pack());
    let (choice, how) = r.core.runner.job_choice("sonnet", open_site()).await;
    assert_eq!(
        (choice, how),
        (Choice::Profile("sonnet".into()), How::Live(Why::Fallback))
    );
}

/// With routing off the pack is not asked: `live`, with no call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_routing_off_no_ask_is_made() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "opus", 0.95);
    let r = rig_job(&jev, |c| c.routing.enabled = false);
    let (choice, how) = r.core.runner.job_choice("sonnet", owner_site(None)).await;
    assert_eq!(
        (choice, how),
        (Choice::Profile("sonnet".into()), How::Live(Why::Unasked))
    );
    assert_eq!(jev.connections(), 0);
}
