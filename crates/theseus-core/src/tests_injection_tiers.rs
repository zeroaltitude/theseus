//! Injection sized to the prompt (theseus-d9je7, rows 2, 4 and 5;
//! `turn::tier`): the route pack's mode folds into three tiers, which size the
//! recall pack and how far admitted tool families may raise a request, and the
//! ontology walk's tokens ride `context.compiled`. The rig is `tests_route`'s,
//! with recall live over a stand-in index of twenty harbour notes.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode, Scripted};
use theseus_protocol::context::ContextExplainParams;
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::TurnSubmitResult;

use crate::recall::{Ask, AskFuture};
use crate::tests_route::{decided, mode, rig, turn, Rig};
use crate::Config;

/// Twenty notes of about 120 tokens each: more than any tier's items fit.
fn notes() -> Vec<String> {
    (1..=20)
        .map(|i| {
            format!(
                "Harbour log {i}: mooring {i} on the outer wall was checked at low water; \
                 its chain, shackle and buoy {i} were sound, and the keeper wrote the depth. "
            )
            .repeat(3)
        })
        .collect()
}

/// The route requests Jev has read.
fn route_calls(jev: &FakeJev) -> usize {
    jev.seen()
        .iter()
        .filter(|s| {
            s.body["questions"]
                .as_object()
                .is_some_and(|q| q.keys().any(|k| k.starts_with("route.")))
        })
        .count()
}

/// The stand-in index over `sessions`, answering a query of the `n`th turn
/// (its `n`th distinct `as_of`) only once Jev has read the `n`th route request
/// and had time to answer it: the verdict has landed before the pack, whatever
/// the order the turn's work would take.
fn after_the_verdict(r: &Rig, jev: &FakeJev, sessions: Vec<String>) -> Ask {
    let inner = crate::tests_recall::index_of(&r.core, sessions);
    let (jev, turns) = (jev.clone(), Arc::new(Mutex::new(Vec::new())));
    Arc::new(move |p| -> AskFuture {
        let n = {
            let mut seen = turns.lock().unwrap();
            if !seen.contains(&p.as_of) {
                seen.push(p.as_of);
            }
            seen.iter().position(|a| *a == p.as_of).unwrap() + 1
        };
        let answer = inner(p);
        let jev = jev.clone();
        Box::pin(async move {
            let t0 = Instant::now();
            while route_calls(&jev) < n {
                assert!(t0.elapsed() < Duration::from_secs(20), "no route call");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
            answer.await
        })
    })
}

/// A rig whose recall is live in front of the model, its deep modes left on
/// the session's own profile (so a tier is never a switch), and its index
/// answering after the verdict (`landed`) or at once.
fn tiered(jev: &FakeJev, n: usize, landed: bool, tweak: impl FnOnce(&mut Config)) -> Rig {
    let r = rig(Some(jev), n, |c| {
        c.memory.mode = crate::config::MemoryMode::Live;
        c.memory.recall_deadline_ms = 5_000;
        c.routing.classes.deep_coding.profiles = vec![];
        c.routing.classes.sophisticated.profiles = vec![];
        crate::tests_route::glm_placements(c);
        tweak(c);
    });
    let said = notes();
    let said: Vec<&str> = said.iter().map(String::as_str).collect();
    let sid = crate::tests_recall::session(&r.core, None, &said);
    let ask = match landed {
        true => after_the_verdict(&r, jev, vec![sid]),
        false => crate::tests_recall::index_of(&r.core, vec![sid]),
    };
    r.core.runner.memory.set_ask(ask);
    r
}

/// The turn's one live recall row.
fn recalled(r: &Rig, res: &TurnSubmitResult) -> RecallManifest {
    let rows = crate::tests_recall::recalls(&r.core, &res.session_id);
    assert_eq!(rows.len(), 1, "one recall row");
    rows.into_iter().next().unwrap()
}

/// The last `route.decided` row's `injection`.
fn injection(r: &Rig) -> Value {
    decided(&r.core.store).pop().unwrap()["injection"].clone()
}

/// The key test: a `deep_coding` verdict that lands before the pack gets the
/// deep pack, 8,000 tokens and 16 items, of twenty notes that fit; the route
/// row says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deep_prompt_gets_the_deep_recall_pack() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = tiered(&jev, 1, true, |_| {});
    let res = turn(
        &r.core,
        None,
        "Which moorings on the outer wall were checked?",
        None,
    )
    .await;
    let m = recalled(&r, &res);
    assert_eq!(
        (m.budget_tokens, m.admitted.len(), m.outcome.as_str()),
        (8_000, 16, "ran")
    );
    assert_eq!(res.recalled, 16, "the footer counts what the model saw");
    let i = injection(&r);
    assert_eq!(
        (
            &i["tier"],
            &i["fallback"],
            &i["recall_items"],
            &i["admit_ceiling"]
        ),
        (&"deep".into(), &Value::Null, &16.into(), &3.0.into())
    );
}

/// A verdict that has not landed by the pack: the pack is ordinary and never
/// waits for it, and the route row says `verdict late`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_packs_ordinary_and_says_so() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = tiered(&jev, 1, false, |c| {
        c.routing.max_wait_ms = 200;
        c.judge.total_secs = 30;
    });
    // Released a second after the turn starts: a pack that waited for it
    // would read deep.
    let late = jev.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        late.release();
    });
    let res = turn(
        &r.core,
        None,
        "Which moorings on the outer wall were checked?",
        None,
    )
    .await;
    let m = recalled(&r, &res);
    assert_eq!((m.budget_tokens, m.admitted.len()), (3_000, 8));
    let i = injection(&r);
    assert_eq!(
        (&i["tier"], &i["fallback"]),
        (&"ordinary".into(), &"verdict late".into())
    );
}

/// An ordinary prompt (`chat`) packs 3,000 tokens and 8 items, from 1,500 and 6.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ordinary_prompt_gets_three_thousand_and_eight() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = tiered(&jev, 1, true, |_| {});
    let res = turn(
        &r.core,
        None,
        "Which moorings on the outer wall were checked?",
        None,
    )
    .await;
    let m = recalled(&r, &res);
    assert_eq!((m.budget_tokens, m.admitted.len()), (3_000, 8));
    let i = injection(&r);
    assert_eq!(
        (
            &i["tier"],
            &i["fallback"],
            &i["recall_tokens"],
            &i["admit_ceiling"]
        ),
        (&"ordinary".into(), &Value::Null, &3_000.into(), &2.0.into())
    );
}

/// A trivial detour is the small tier: its request still carries no recall
/// (theseus-n7nc), and the route row gives it none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trivial_detour_still_has_no_recall() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let r = tiered(&jev, 1, true, |_| {});
    let res = turn(&r.core, None, "thank you!", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "detour");
    assert_eq!(res.recalled, 0);
    let m = recalled(&r, &res);
    assert_eq!(m.outcome, "detoured");
    let i = injection(&r);
    assert_eq!(
        (&i["tier"], &i["recall_items"], &i["admit_ceiling"]),
        (&"small".into(), &0.into(), &1.0.into())
    );
}

/// The cache rule: the tier sizes the tail alone. Two turns of one session,
/// ordinary then deep, send the same system and tools, byte for byte, and the
/// second's messages begin with the first's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tier_never_changes_a_cached_prefix_byte() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = tiered(&jev, 2, true, |_| {});
    let ask = "Which moorings on the outer wall were checked?";
    let one = turn(&r.core, None, ask, None).await;
    mode(&jev, "deep_coding", 0.95);
    let two = turn(&r.core, Some(&one.session_id), "And the chains?", None).await;
    let rows = crate::tests_recall::recalls(&r.core, &one.session_id);
    let budgets: Vec<u64> = rows.iter().map(|m| m.budget_tokens).collect();
    assert!(
        budgets.contains(&3_000) && budgets.contains(&8_000),
        "{budgets:?}"
    );
    assert_eq!(
        (one.profile.as_str(), two.profile.as_str()),
        ("sonnet", "sonnet")
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

/// The cli's place, a model with no tool search, and every hidden family
/// judged relevant: the tier's multiple is the ceiling the admitted families
/// raise the first request to. Jev's families answer is held until the route's
/// has landed, so the tier is known before the tools are settled.
async fn admitted_at(verdict: &str) -> Value {
    let jev = FakeJev::start().unwrap();
    mode(&jev, verdict, 0.95);
    for q in (1..=10).map(|n| format!("relevant.{n}")) {
        jev.script(&q, Scripted::Noul(0.95));
    }
    for q in (1..=20).map(|n| format!("relevant_more.{n}")) {
        jev.script(&q, Scripted::Noul(0.95));
    }
    let dir = tempfile::tempdir().unwrap();
    let (r, _) = crate::tests_detour::rig_haiku(&jev, 1, |c| {
        crate::tests_detour::owner_shape(c, dir.path());
        // A request within the budget before any family (`want::extra`): no
        // context file.
        c.context.files = vec![];
        c.routing.classes.deep_coding.profiles = vec![];
        c.tools.defer.judge_wait_ms = 2_000;
        c.judge.total_secs = 30;
        c.catalog.insert(
            "claude-sonnet-5-5".into(),
            crate::catalog::CatalogRow {
                tool_search: Some(false),
                ..Default::default()
            },
        );
    });
    jev.hold_when("\"holds\":");
    let held = jev.clone();
    tokio::spawn(async move {
        let t0 = Instant::now();
        while route_calls(&held) == 0 && t0.elapsed() < Duration::from_secs(20) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        held.release();
    });
    let ask = "Look over the harbour's whole setup and plan the next week's work.";
    let res = turn(&r.core, None, ask, None).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    let used = crate::tests_places::rows(&r.core, "tools.used").remove(0);
    used["raised"].clone()
}

/// Admission follows the tier: deep's ceiling is 3x the cli's budget and
/// admits a judged family the ordinary 2x refuses; ordinary's is today's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_follows_the_tier() {
    let budget = crate::first_request::CLI_TASK_TOKENS;
    let ordinary = admitted_at("chat").await;
    let deep = admitted_at("deep_coding").await;
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

/// `theseus why` names the tier and its pack in one line, from the turn's
/// route row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn why_names_the_tier_and_its_pack() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = tiered(&jev, 1, true, |_| {});
    let res = turn(
        &r.core,
        None,
        "Which moorings on the outer wall were checked?",
        None,
    )
    .await;
    let explain = r
        .core
        .context_explain(
            &ContextExplainParams {
                session_id: res.session_id,
                turn_id: None,
            },
            true,
        )
        .unwrap();
    let route = decided(&r.core.store).pop().unwrap();
    let lines = theseus_protocol::why::why_lines(&theseus_protocol::why::WhyInput {
        explain: &explain,
        policy: None,
        route: Some(&route),
        tools: &[],
        hide_sources: false,
    });
    let want = "injection: deep: recall 8,000 tokens, 16 items; tools to 3x the place's budget";
    assert!(lines.iter().any(|l| l == want), "{lines:#?}");
}

/// The request sizes the tiers make on this rig, before (1,500 tokens and 6
/// items for every prompt) and after: printed for the report.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_sizes_before_and_after() {
    let mut out = Vec::new();
    for (label, before) in [("before", true), ("after", false)] {
        for verdict in ["chat", "deep_coding"] {
            let jev = FakeJev::start().unwrap();
            mode(&jev, verdict, 0.95);
            let r = tiered(&jev, 1, true, |c| {
                if before {
                    (c.memory.recall_budget_tokens, c.memory.recall_max_items) = (1_500, 6);
                    (c.memory.deep_recall_tokens, c.memory.deep_recall_items) = (1_500, 6);
                    c.tools.defer.admit_ceiling_deep = 2.0;
                }
            });
            let ask = "Which moorings on the outer wall were checked?";
            turn(&r.core, None, ask, None).await;
            let c = crate::tests_judge::kinds(&r.core.store, "context.compiled");
            out.push((label, verdict, c[0].data["est_tokens"].as_u64().unwrap()));
        }
    }
    eprintln!("request sizes (est_tokens): {out:?}");
    assert!(out[2].2 > out[0].2 && out[3].2 > out[2].2, "{out:?}");
}

/// The walk's guidance tokens ride `context.compiled`, the same count
/// `context.explain` gives its guidance parts (row 5, measured only).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_walks_tokens_are_on_context_compiled() {
    use theseus_protocol::{
        OntologyCategoryAddParams, OntologyGuidanceSetParams, OntologyMembershipSetParams,
    };
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let core = &r.core;
    let name = "harbour";
    core.ontology_category_add(
        &OntologyCategoryAddParams {
            name: name.into(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    core.ontology_guidance_set(
        &OntologyGuidanceSetParams {
            category: name.into(),
            text: "Name each mooring by its wall and number.\n".repeat(40),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    let sid = crate::tests_recall::session(core, None, &[]);
    core.ontology_membership_set(
        &OntologyMembershipSetParams {
            session_id: sid.clone(),
            add: vec![name.into()],
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    turn(core, Some(&sid), "Which moorings were checked?", None).await;
    let compiled = crate::tests_judge::kinds(&core.store, "context.compiled");
    let walk = compiled[0].data["walk_tokens"].as_u64().unwrap_or(0);
    let explain = core
        .context_explain(
            &ContextExplainParams {
                session_id: sid,
                turn_id: None,
            },
            true,
        )
        .unwrap();
    let parts: u64 = explain
        .parts
        .iter()
        .filter(|p| p.block == "guidance")
        .map(|p| p.tokens)
        .sum();
    assert!(parts > 300, "the guidance's parts: {parts}");
    assert_eq!(walk, parts, "context.compiled's walk_tokens");
}
