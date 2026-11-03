//! Injection sized to the prompt (theseus-d9je7, rows 2, 4 and 5;
//! `turn::tier`): the route pack's mode folds into three tiers, which size the
//! recall pack and how far admitted tool families may raise a request, and the
//! ontology walk's tokens ride `context.compiled`. The rig is `tests_route`'s,
//! with recall live over a stand-in index of twenty harbour notes.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_protocol::context::ContextExplainParams;
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::TurnSubmitResult;

use crate::recall::{Ask, AskFuture};
use crate::tests_route::{decided, mode, rig, turn, Rig};
use crate::Config;

/// Twenty notes of about 120 tokens each: more than any tier's items fit.
pub(crate) fn notes() -> Vec<String> {
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
pub(crate) fn tiered(
    jev: &FakeJev,
    n: usize,
    landed: bool,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let r = rig(Some(jev), n, |c| {
        c.memory.mode = crate::config::MemoryMode::Live;
        c.memory.recall_deadline_ms = 5_000;
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
