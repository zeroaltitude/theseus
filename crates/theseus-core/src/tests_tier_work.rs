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
//! - `theseus why` names the tier and its recall pack from the route row.
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
/// tools, byte for byte, and the second's first message is the first's. The tier did change (3,000 tokens then 8,000, said on the route rows), so the equality
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
    let budget = crate::first_request::TOOLS_CLI_TASK_TOKENS;
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
    let budget = crate::first_request::TOOLS_CLI_TASK_TOKENS;
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
    let budget = crate::first_request::TOOLS_CLI_TASK_TOKENS;
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

/// A chat act offered to a turn whose late verdict folds deep is in the call's request once (issue
/// 20). The second settle (`stays_on_its_own`) runs on the spec the first settle left, which holds
/// the acts already, and the offer for the same turn is the same acts again: before the fix they
/// were added twice, and the provider refused the whole request ("Tool names must be unique"). The
/// rig is `admitted_late`'s with the import configured, which offers its sync tool to any turn of
/// a private place that a message of the owner's own started.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_act_offered_at_a_second_settle_is_in_the_request_once() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    work(&jev, "coding", 0.95);
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("openclaw").display().to_string();
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        1,
        |c| {
            c.serve_deepseek_flash();
            c.routing.max_wait_ms = 5_000;
            c.judge.total_secs = 30;
            crate::tests_detour::owner_shape(c, dir.path());
            c.context.files = vec![];
            c.routing.cold_switch_tokens = 1_000_000;
            c.tools.defer.judge_families = false;
            for model in ["claude-sonnet-5-5", "claude-opus-5-5"] {
                c.catalog.entry(model.into()).or_default().tool_search = Some(false);
            }
            c.import.openclaw.state_dir = Some(state);
        },
        |parts| parts.cfg.tools.defer.enabled = true,
    );
    act_open(&r);
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    // What `turn.submit` stages for a message of the owner's own (`hear_for_chat_acts`).
    r.core.runner.tools.chat_acts.heard.stage(
        &rec.session_id,
        std::time::Instant::now(),
        crate::chat_acts::Said {
            text: ASK.into(),
            who: crate::approval::Answerer::from("owner"),
            owner_input: true,
        },
    );
    jev.set_mode(FakeMode::Held);
    let held = jev.clone();
    tokio::spawn(async move {
        let t0 = std::time::Instant::now();
        while held.seen().is_empty() {
            assert!(t0.elapsed() < Duration::from_secs(20), "no route call");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        held.set_mode(FakeMode::Up);
        held.release();
    });
    let res = turn(&r.core, Some(&rec.session_id), ASK, None).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    // The tools did settle twice: the verdict was the turn's own and raised its ceiling.
    let row = decided(&r.core.store).pop().unwrap();
    assert_eq!(row["late"], false, "{row}");
    assert_eq!(row["injection"]["admit_ceiling"], 3.0, "{row}");
    let q = r.claude.requests();
    let names: Vec<&str> = q[0]
        .tools
        .iter()
        .filter_map(|d| d["name"].as_str())
        .collect();
    assert!(
        names.contains(&"chat_import_sync"),
        "the act was offered: {names:?}"
    );
    let mut once = names.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(once.len(), names.len(), "a tool named twice: {names:?}");
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
