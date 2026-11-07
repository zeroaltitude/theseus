//! The injection tier, driven by `work.v1` (theseus-h66v4.9; `turn::tier`). route.v8's tier tests
//! set a turn's tier through the class `mode`, which route.v10 retires, so the live tier code lost
//! its end-to-end tests with the class walk (the kn9or review's F2). Here `work = coding` at the
//! confirm bar is what makes a turn Deep, and each test holds the model pick still, so the work
//! label is the only thing that moves.
//!
//! - the tier sizes the tail alone: no cached prefix byte moves when the tier does;
//! - admission follows the tier: deep's ceiling raises a request 3x, ordinary's 2x — for a switch,
//!   for a pick that stays on the session's own model, and for a verdict that lands after the
//!   turn's tools have settled, which is the one the tier used to miss (theseus-a2kh6);
//! - `theseus why` names the tier and its recall pack from the route row;
//! - a v10 turn the data rule kept off a restricted model counts once on `theseus.route.data_gate`.
//!
//! The rig is `tests_injection_tiers`'s (recall live over twenty harbour notes), with route.v10's
//! shape acting (`tests_act_open`: a stand-in while `ROUTE_PACK` names route.v8, nothing once it
//! names route.v10).

use std::time::Duration;

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::context::ContextExplainParams;

use crate::tests_act_open::act_open;
use crate::tests_injection_tiers::tiered;
use crate::tests_route::{decided, turn, Rig};
use crate::tests_route_open::says;

fn work(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "work",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

/// `tiered`, with route.v10 acting: its verdict lands before the pack reads it.
fn tiered_open(jev: &FakeJev, n: usize) -> Rig {
    let r = tiered(jev, n, true, |_| {});
    act_open(&r);
    r
}

const ASK: &str = "Which moorings on the outer wall were checked?";

/// The cache rule: the tier sizes the tail alone. Two turns of one session, ordinary (`work =
/// conversation`) then deep (`work = coding`), on the same model pick, send the same system and
/// tools, byte for byte, and the second's first message is the first's; the second compile
/// appends. The tier did change (3,000 tokens then 8,000, said on the route rows), so the equality
/// is no accident of nothing moving.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tier_never_changes_a_cached_prefix_byte() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    work(&jev, "conversation", 0.95);
    let r = tiered_open(&jev, 2);
    let one = turn(&r.core, None, ASK, None).await;
    work(&jev, "coding", 0.95);
    let two = turn(&r.core, Some(&one.session_id), "And the chains?", None).await;
    assert_eq!(
        (one.profile.as_str(), two.profile.as_str()),
        ("sonnet", "sonnet"),
        "the tier is never a switch"
    );
    let tiers: Vec<(Value, Value)> = decided(&r.core.store)
        .iter()
        .map(|d| {
            (
                d["injection"]["tier"].clone(),
                d["injection"]["recall_tokens"].clone(),
            )
        })
        .collect();
    assert_eq!(
        tiers,
        [
            ("ordinary".into(), 3_000.into()),
            ("deep".into(), 8_000.into())
        ],
        "the tier moved between the two turns"
    );
    let q = r.claude.requests();
    let (a, b) = (&q[0], &q[1]);
    assert_eq!(
        serde_json::to_string(&a.system).unwrap(),
        serde_json::to_string(&b.system).unwrap(),
        "the system"
    );
    assert_eq!(a.tools, b.tools, "the tools");
    assert_eq!(a.messages[0], b.messages[0], "the first message");
    let compiled = crate::tests_judge::kinds(&r.core.store, "context.compiled");
    let decisions: Vec<&str> = compiled
        .iter()
        .map(|c| c.data["decision"].as_str().unwrap())
        .collect();
    assert_eq!(
        decisions,
        ["recompile", "append"],
        "the second turn appends"
    );
}

/// The cli's place, models with no tool search, and every hidden family judged relevant: the
/// tier's multiple is the ceiling the admitted families raise the first request to. Jev picks
/// `pick` on both arms, so only `work` differs. The tools settle once at the turn's start, before
/// a verdict has landed (the tier reads ordinary then); since theseus-a2kh6 a verdict that folds
/// deep settles them again, on the routed spec for a switch (`pick` off the session's Sonnet) and
/// on the turn's own for a pick that stays on the session's model.
async fn admitted_at(pick: &str, label: &str) -> Value {
    let jev = FakeJev::start().unwrap();
    says(&jev, pick, "high", 0.95);
    work(&jev, label, 0.95);
    for q in (1..=10).map(|n| format!("relevant.{n}")) {
        jev.script(&q, Jev::Noul(0.95));
    }
    for q in (1..=20).map(|n| format!("relevant_more.{n}")) {
        jev.script(&q, Jev::Noul(0.95));
    }
    let dir = tempfile::tempdir().unwrap();
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        1,
        |c| {
            // The template's anthropic key is a placeholder; this also names the keys a pick needs.
            c.serve_deepseek_flash();
            c.routing.max_wait_ms = 5_000;
            crate::tests_detour::owner_shape(c, dir.path());
            // A request within the budget before any family (`want::extra`): no context file.
            c.context.files = vec![];
            c.routing.cold_switch_tokens = 1_000_000;
            c.tools.defer.judge_wait_ms = 2_000;
            for model in ["claude-sonnet-5-5", "claude-opus-5-5"] {
                c.catalog.entry(model.into()).or_default().tool_search = Some(false);
            }
        },
        |parts| parts.cfg.tools.defer.enabled = true,
    );
    act_open(&r);
    let ask = "Look over the harbour's whole setup and plan the next week's work.";
    let res = turn(&r.core, None, ask, None).await;
    assert_eq!(res.profile, pick, "{:?}", res.route);
    let used = crate::tests_places::rows(&r.core, "tools.used").remove(0);
    used["raised"].clone()
}

/// Admission follows the tier: deep's ceiling (`work = coding`) is 3x the cli's budget and admits
/// a judged family the ordinary 2x refuses; ordinary's is 2x.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_follows_the_tier() {
    let budget = crate::first_request::CLI_TASK_TOKENS;
    let ordinary = admitted_at("opus", "conversation").await;
    let deep = admitted_at("opus", "coding").await;
    eprintln!(
        "admitted past the cli's {budget}: ordinary (2.0) {:?} to {}; deep (3.0) {:?} to {}",
        families(&ordinary),
        ordinary["tokens"],
        families(&deep),
        deep["tokens"]
    );
    assert_eq!(ordinary["ceiling"], 2 * budget, "{ordinary}");
    assert_eq!(deep["ceiling"], 3 * budget, "{deep}");
    assert!(
        !only_in(&deep, &ordinary).is_empty(),
        "deep admits a family ordinary refuses: {ordinary} {deep}"
    );
}

/// The same, for a pick that is the session's own model: nothing switches, so the turn keeps the
/// spec its tools settled on, and the deep arm's ceiling is still 3x. On this rig the verdict has
/// landed before the tools settle — the wait for the families judgment (`judge_wait_ms`) stands
/// between the inbound point and the settle, and Jev answers both in the same breath — so this is
/// the half of the tier's reach that worked before theseus-a2kh6. The test below is the half that
/// did not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deep_turn_that_stays_on_its_own_model_admits_at_the_deep_ceiling() {
    let budget = crate::first_request::CLI_TASK_TOKENS;
    let ordinary = admitted_at("sonnet", "conversation").await;
    let deep = admitted_at("sonnet", "coding").await;
    eprintln!(
        "on the session's own model, past the cli's {budget}: ordinary (2.0) {:?} to {}; \
         deep (3.0) {:?} to {}",
        families(&ordinary),
        ordinary["tokens"],
        families(&deep),
        deep["tokens"]
    );
    assert_eq!(ordinary["ceiling"], 2 * budget, "{ordinary}");
    assert_eq!(deep["ceiling"], 3 * budget, "{deep}");
    assert!(
        !only_in(&deep, &ordinary).is_empty(),
        "deep admits a family ordinary refuses: {ordinary} {deep}"
    );
}

/// The families a `tools.used` row's `raised` names.
fn families(v: &Value) -> Vec<String> {
    v["families"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| f["family"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The families one arm admitted and the other refused.
fn only_in(a: &Value, b: &Value) -> Vec<String> {
    families(a)
        .into_iter()
        .filter(|f| !families(b).contains(f))
        .collect()
}

/// `admitted_at`'s rig, with the verdict landing **after** the turn's tools have settled: Jev holds
/// every answer until a task releases it a breath into the turn, and the turn waits for it (up to
/// `[routing] max_wait_ms`), so the verdict is the turn's own and not a missed one.
///
/// Two things differ from `admitted_at`, and both are what make the lateness real. The families
/// judgment is off, so the turn does not wait for it before settling its tools — that wait is why
/// `admitted_at`'s verdict has already landed by then. And what may be admitted past the place's
/// budget is therefore what the message *names* (`want::named`), which is why the ask below reads
/// as a list of them.
///
/// Returns the `tools.used` row's `raised` (the ceiling the admission really used) and the
/// `route.decided` row (the ceiling the turn's budget says it used).
async fn admitted_late(label: &str) -> (Value, Value) {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    work(&jev, label, 0.95);
    let dir = tempfile::tempdir().unwrap();
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        1,
        |c| {
            c.serve_deepseek_flash();
            c.routing.max_wait_ms = 5_000;
            // A held answer must not run the judge's own deadline out.
            c.judge.total_secs = 30;
            crate::tests_detour::owner_shape(c, dir.path());
            c.context.files = vec![];
            c.routing.cold_switch_tokens = 1_000_000;
            c.tools.defer.judge_families = false;
            for model in ["claude-sonnet-5-5", "claude-opus-5-5"] {
                c.catalog.entry(model.into()).or_default().tool_search = Some(false);
            }
        },
        |parts| parts.cfg.tools.defer.enabled = true,
    );
    act_open(&r);
    // After the rig's warm-up, so nothing before the turn is held.
    jev.set_mode(FakeMode::Held);
    let held = jev.clone();
    tokio::spawn(async move {
        // Once Jev has read the turn's route request — sent at the inbound point, before the
        // tools settle — and a breath after it, so they have settled by then. Up again before the
        // release: `Held` would hold the next call as well.
        let t0 = std::time::Instant::now();
        while held.seen().is_empty() {
            assert!(t0.elapsed() < Duration::from_secs(20), "no route call");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        held.set_mode(FakeMode::Up);
        held.release();
    });
    let ask = "Check the aws logs and the aws stacks, then the projects, the threads, the \
               terminals, the channels, the reminders and the helpers.";
    let res = turn(&r.core, None, ask, None).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    let row = decided(&r.core.store).pop().unwrap();
    let used = crate::tests_places::rows(&r.core, "tools.used").remove(0);
    (used["raised"].clone(), row)
}

/// A deep turn whose verdict lands after its tools have settled admits at the deep ceiling
/// (theseus-a2kh6).
///
/// The tools settle once, at the turn's start (`turn.rs`'s `defer_tools`), before any verdict has
/// landed, so they settle at the ordinary 2x whatever the turn turns out to be; the only second
/// settle was a switch's routed spec (`route_step::route_to`, reached only where the decision
/// names another profile). So a `coding` verdict that landed afterwards left a turn whose pick is
/// its own model admitting at 2x for the whole turn — and its `route.decided` row said 3x all the
/// while, because the row is written from the budget and the budget was fixed after the tools. The
/// two numbers asserted here are exactly that pair: before the fix they disagreed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_that_folds_deep_settles_the_tools_again() {
    let budget = crate::first_request::CLI_TASK_TOKENS;
    let (ordinary, o_row) = admitted_late("conversation").await;
    let (deep, d_row) = admitted_late("coding").await;
    eprintln!(
        "a late verdict, past the cli's {budget}: ordinary {:?} to {}; deep {:?} to {}",
        families(&ordinary),
        ordinary["tokens"],
        families(&deep),
        deep["tokens"]
    );
    // The verdict was each turn's own, read in time: a missed one would say nothing about a tier.
    for row in [&o_row, &d_row] {
        assert_eq!(row["late"], false, "the verdict was the turn's: {row}");
    }
    // What the turn's budget says, and what its tools really admitted at. Both, both arms.
    assert_eq!(o_row["injection"]["admit_ceiling"], 2.0, "{o_row}");
    assert_eq!(d_row["injection"]["admit_ceiling"], 3.0, "{d_row}");
    assert_eq!(ordinary["ceiling"], 2 * budget, "{ordinary}");
    assert_eq!(deep["ceiling"], 3 * budget, "{deep}");
    assert!(
        !only_in(&deep, &ordinary).is_empty(),
        "deep admits a family ordinary refuses: {ordinary} {deep}"
    );
}

/// `theseus why` names the tier and its recall pack in one line, from the turn's route row, for
/// a turn `work.v1` made deep; and says why a turn fell back to ordinary when it did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn why_names_the_tier_and_its_pack() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    work(&jev, "coding", 0.95);
    let r = tiered_open(&jev, 2);
    let words = |r: &Rig, sid: &str| {
        let explain = r
            .core
            .context_explain(
                &ContextExplainParams {
                    session_id: sid.into(),
                    turn_id: None,
                },
                true,
            )
            .unwrap();
        let route = decided(&r.core.store).pop().unwrap();
        theseus_protocol::why::why_lines(&theseus_protocol::why::WhyInput {
            explain: &explain,
            policy: None,
            route: Some(&route),
            tools: &[],
            hide_sources: false,
        })
    };
    let one = turn(&r.core, None, ASK, None).await;
    let lines = words(&r, &one.session_id);
    let want = "injection: deep: recall 8,000 tokens, 16 items; tools to 3x the place's budget";
    assert!(lines.iter().any(|l| l == want), "{lines:#?}");
    // A coding label under its confirm bar is no deep turn, and the line says why.
    work(&jev, "coding", 0.40);
    let two = turn(&r.core, Some(&one.session_id), "And the chains?", None).await;
    let lines = words(&r, &two.session_id);
    let want =
        "injection: ordinary (unsure): recall 3,000 tokens, 8 items; tools to 2x the place's budget";
    assert!(lines.iter().any(|l| l == want), "{lines:#?}");
}

/// A turn route.v10 ran on the live profile because the data rule kept a China-origin model off
/// it counts once on `theseus.route.data_gate`, by caller (`route`) and the flag's word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_data_gate_turn_counts_once_on_the_route_data_gate_metric() {
    use crate::content::Flag;
    use crate::content_wait::judging::Judging;
    use crate::provider::FakeProvider;
    use crate::tests_dm_rig::{gate_counts, judging_with, telemetry};
    use crate::tests_judge::texts;
    use std::sync::Arc;

    let (rx, tel) = telemetry().await;
    let jev = FakeJev::start().unwrap();
    // A sure Flash pick, for a session whose labels keep a China-origin model off it.
    says(&jev, "deepseek_flash", "low", 0.97);
    let deepseek = Arc::new(FakeProvider::scripted(texts(1)));
    let ds = deepseek.clone();
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        1,
        |c| c.serve_deepseek_flash(),
        move |p| {
            p.providers.insert("deepseek".into(), ds);
            p.telemetry = Some(tel);
        },
    );
    act_open(&r);
    judging_with(&r, Judging::with(|_, _| vec![Flag::Confidential]));
    let res = turn(&r.core, None, "Rename the call sites.", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "data_gate");
    assert!(deepseek.requests().is_empty(), "Flash was sent nothing");
    // The route row carries the same word the count does.
    let row = decided(&r.core.store).pop().unwrap();
    assert_eq!(row["data_gate"], "confidential", "{row}");
    // The export spells a count as a number or a string of one.
    let counts: Vec<(Value, Value, String)> = gate_counts(&r.core, &rx)
        .await
        .into_iter()
        .map(|(what, why, n)| (what, why, n.to_string().replace('"', "")))
        .collect();
    assert_eq!(
        counts,
        [("route".into(), "confidential".into(), "1".to_string())],
        "one count, by caller and flag"
    );
}

/// A Flash pick for a session already marked as barred from restricted models (here by
/// `cn_models = never`, which reads no label) counts once: the common route gate, found when
/// the turn's floor is closed at its start (theseus-7l8lr.3, C6).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flash_pick_under_a_closed_floor_counts_once_on_the_route_data_gate_metric() {
    assert_eq!(
        flash_under_never(1_000_000).await,
        [("route".into(), "never".into(), "1".to_string())]
    );
}

/// A Flash pick the cache would have held on the profile with the floor open is still a
/// refusal by the data rule, and counts: the route row says `data_gate`, and so does the count.
/// (The counterfactual decision C6 removed asked the rule again with the floor open and counted
/// only a pick it would then have taken: a held pick went uncounted under a row that said
/// `data_gate`. theseus-7l8lr.3 chose the row's own word.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flash_pick_the_cache_would_have_held_under_a_closed_floor_counts_once() {
    assert_eq!(
        flash_under_never(1).await,
        [("route".into(), "never".into(), "1".to_string())]
    );
}

/// The same for a Flash pick whose provider is not wired: the rule refused it first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flash_pick_with_no_provider_under_a_closed_floor_counts_once() {
    use crate::config::content_based_security::CnModels;
    use crate::tests_dm_rig::{gate_counts, telemetry};

    let (rx, tel) = telemetry().await;
    let jev = FakeJev::start().unwrap();
    says(&jev, "deepseek_flash", "low", 0.97);
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        1,
        |c| {
            c.serve_deepseek_flash();
            c.content_based_security.cn_models = CnModels::Never;
        },
        move |p| p.telemetry = Some(tel),
    );
    act_open(&r);
    let res = turn(&r.core, None, "Rename the call sites.", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "data_gate");
    let counts: Vec<(Value, Value, String)> = gate_counts(&r.core, &rx)
        .await
        .into_iter()
        .map(|(what, why, n)| (what, why, n.to_string().replace('"', "")))
        .collect();
    assert_eq!(counts, [("route".into(), "never".into(), "1".to_string())]);
}

async fn flash_under_never(cold: u64) -> Vec<(Value, Value, String)> {
    use crate::config::content_based_security::CnModels;
    use crate::provider::FakeProvider;
    use crate::tests_dm_rig::{gate_counts, telemetry};
    use crate::tests_judge::texts;
    use std::sync::Arc;

    let (rx, tel) = telemetry().await;
    let jev = FakeJev::start().unwrap();
    says(&jev, "deepseek_flash", "low", 0.97);
    let deepseek = Arc::new(FakeProvider::scripted(texts(1)));
    let ds = deepseek.clone();
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        1,
        |c| {
            c.serve_deepseek_flash();
            c.content_based_security.cn_models = CnModels::Never;
            c.routing.cold_switch_tokens = cold;
        },
        move |p| {
            p.providers.insert("deepseek".into(), ds);
            p.telemetry = Some(tel);
        },
    );
    act_open(&r);
    let res = turn(&r.core, None, "Rename the call sites.", None).await;
    assert!(deepseek.requests().is_empty(), "Flash was sent nothing");
    assert_eq!(res.route.as_ref().unwrap().reason, "data_gate");
    gate_counts(&r.core, &rx)
        .await
        .into_iter()
        .map(|(what, why, n)| (what, why, n.to_string().replace('"', "")))
        .collect()
}
