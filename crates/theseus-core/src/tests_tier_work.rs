//! The injection tier, driven by `work.v1` (theseus-h66v4.9; `turn::tier`). route.v8's tier tests
//! set a turn's tier through the class `mode`, which route.v10 retires, so the live tier code lost
//! its end-to-end tests with the class walk (the kn9or review's F2). Here `work = coding` at the
//! confirm bar is what makes a turn Deep, and each test holds the model pick still, so the work
//! label is the only thing that moves.
//!
//! - the tier sizes the tail alone: no cached prefix byte moves when the tier does;
//! - admission follows the tier: deep's ceiling raises a request 3x, ordinary's 2x;
//! - `theseus why` names the tier and its recall pack from the route row;
//! - a v10 turn the data rule kept off a restricted model counts once on `theseus.route.data_gate`.
//!
//! The rig is `tests_injection_tiers`'s (recall live over twenty harbour notes), with route.v10's
//! shape standing in as the acting route pack (`tests_route_open::open_pack`).

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::context::ContextExplainParams;

use crate::judge::inbound::ROUTE_PACK;
use crate::tests_injection_tiers::tiered;
use crate::tests_route::{decided, turn, Rig};
use crate::tests_route_open::{open_pack, says};

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
    r.core.runner.judge.stand_in(ROUTE_PACK, open_pack());
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
/// Opus on both arms (off the session's Sonnet), so only `work` differs. The pick has to be a
/// switch: the tools settle once at the turn's start, before a verdict has landed (the tier reads
/// ordinary then), and again on the routed spec after it; a pick that stays on the session's model
/// has no second settle, and its tools keep the ordinary ceiling whatever the route row says.
async fn admitted_at(label: &str) -> Value {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.95);
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
    r.core.runner.judge.stand_in(ROUTE_PACK, open_pack());
    let ask = "Look over the harbour's whole setup and plan the next week's work.";
    let res = turn(&r.core, None, ask, None).await;
    assert_eq!(res.profile, "opus", "{:?}", res.route);
    let used = crate::tests_places::rows(&r.core, "tools.used").remove(0);
    used["raised"].clone()
}

/// Admission follows the tier: deep's ceiling (`work = coding`) is 3x the cli's budget and admits
/// a judged family the ordinary 2x refuses; ordinary's is 2x.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_follows_the_tier() {
    let budget = crate::first_request::CLI_TASK_TOKENS;
    let ordinary = admitted_at("conversation").await;
    let deep = admitted_at("coding").await;
    let families = |v: &Value| -> Vec<String> {
        v["families"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|f| f["family"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    eprintln!(
        "admitted past the cli's {budget}: ordinary (2.0) {:?} to {}; deep (3.0) {:?} to {}",
        families(&ordinary),
        ordinary["tokens"],
        families(&deep),
        deep["tokens"]
    );
    assert_eq!(ordinary["ceiling"], 2 * budget, "{ordinary}");
    assert_eq!(deep["ceiling"], 3 * budget, "{deep}");
    let more: Vec<String> = families(&deep)
        .into_iter()
        .filter(|f| !families(&ordinary).contains(f))
        .collect();
    assert!(
        !more.is_empty(),
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
    r.core.runner.judge.stand_in(ROUTE_PACK, open_pack());
    judging_with(&r, Judging::with(|_, _| vec![Flag::Confidential]));
    let res = turn(&r.core, None, "Rename the call sites.", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "data_gate");
    assert!(deepseek.requests().is_empty(), "Flash was sent nothing");
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
