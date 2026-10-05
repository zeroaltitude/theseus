//! Routing (M5 25e; `crate::routing`, `turn::route_step`): `route.v1` at
//! `inbound`, live, in the same request as `classify.v1` and `role.v1`, and
//! the turn it moves, against the fake Jev, with the profiles on two fake
//! providers (`anthropic` and `zai`), so each request shows where it went.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_judge::{board, kinds, off, texts};
use crate::turn::TurnRequest;
use crate::Config;

struct Rig {
    core: Arc<Core>,
    /// The `anthropic` provider's fake: sonnet, opus, fable.
    claude: Arc<FakeProvider>,
    /// The `zai` provider's fake: glm (5.3 Flash) and glm53.
    zai: Arc<FakeProvider>,
    /// A second rig on the same dir is a restart.
    dir: Arc<tempfile::TempDir>,
}

/// Every judge pack but the inbound point's off, so its one call is the
/// only one; `route.v1` live, as the build gives it.
fn routing_only(c: &mut Config) {
    c.judge.packs.insert("loop.v1".into(), off());
    for p in crate::judge::gate::GATE_PACKS {
        c.judge.packs.insert(p.into(), off());
    }
    for p in [
        crate::judge::compile::CONTINUE_PACK,
        crate::judge::categorize::PACK,
        crate::judge::rerank::RERANK_PACK,
    ] {
        c.judge.packs.insert(p.into(), off());
    }
}

fn rig(jev: Option<&FakeJev>, n: usize, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = Arc::new(tempfile::tempdir().unwrap());
    rig_on(dir, jev, n, tweak, board())
}

/// The rig on `dir`'s store, with `secrets` as the board.
fn rig_on(
    dir: Arc<tempfile::TempDir>,
    jev: Option<&FakeJev>,
    n: usize,
    tweak: impl FnOnce(&mut Config),
    secrets: Arc<crate::secrets::SecretBoard>,
) -> Rig {
    let mut cfg = crate::tests_judge::judge_config(dir.path(), jev);
    routing_only(&mut cfg);
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let claude = Arc::new(FakeProvider::scripted(texts(n)));
    let zai = Arc::new(FakeProvider::scripted(texts(n)));
    let mut p = Parts::for_tests(cfg, claude.clone(), store);
    p.providers.insert("zai".into(), zai.clone());
    p.secrets = secrets;
    Rig {
        core: Core::build(p).unwrap(),
        claude,
        zai,
        dir,
    }
}

/// A person's message: on the live profile, or on `chosen` as the owner's
/// choice (`ask -P`).
async fn turn(
    core: &Arc<Core>,
    session: Option<&str>,
    input: &str,
    chosen: Option<&str>,
) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => core
            .store
            .get_session::<SessionRecord>(id)
            .unwrap()
            .unwrap(),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let (live, _) = core.live_profile();
    let mut target = core
        .runner
        .resolve_target(&live, chosen, None, None)
        .unwrap();
    target.chosen = chosen.map(|p| format!("profile {p}"));
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

fn mode(jev: &FakeJev, m: &str, confidence: f64) {
    jev.script(
        "mode",
        Jev::Choice {
            option: m.into(),
            confidence,
        },
    );
}

fn decided(store: &Store) -> Vec<Value> {
    kinds(store, "route.decided")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

fn session(core: &Core, id: &str) -> SessionRecord {
    core.store
        .get_session::<SessionRecord>(id)
        .unwrap()
        .unwrap()
}

/// Wait (on the runtime's timer) until the `judge:route` scope has `n` rows.
async fn until_route_rows(store: &Store, n: usize) -> Vec<LedgerRow> {
    let t0 = Instant::now();
    loop {
        let rows: Vec<LedgerRow> = store
            .scope_after("judge:route", 0)
            .unwrap()
            .into_iter()
            .map(|r| r.decode().unwrap())
            .collect();
        if rows.len() >= n {
            return rows;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} route rows",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// One request carries all three inbound packs; `route.v1`'s row is live,
/// and its verdict moves the turn: a hard design question goes to Opus.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_call_asks_three_packs_and_a_hard_question_goes_to_opus() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe write-ahead log.",
        None,
    )
    .await;
    assert_eq!(jev.connections(), 1, "one request for the message");
    let q = &jev.seen()[0].body["questions"];
    for id in ["classify.v1/kind", "role.v1/role", "route.v1/mode"] {
        assert!(q.get(id).is_some(), "{id}: {q}");
    }
    let rows = until_route_rows(&r.core.store, 1).await;
    assert_eq!(rows[0].data["mode"], "live");
    assert_eq!(rows[0].data["call"]["packs"], 3);
    assert_eq!(r.claude.requests()[0].model, "claude-opus-5-5");
    assert!(r.zai.requests().is_empty());
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("opus", "claude-opus-5-5")
    );
    let route = res.route.unwrap();
    assert_eq!(
        (
            route.mode.as_deref(),
            route.reason.as_str(),
            route.from.as_str()
        ),
        (Some("sophisticated"), "verdict", "sonnet")
    );
    let d = decided(&r.core.store);
    assert_eq!(d.len(), 1);
    assert_eq!(
        (d[0]["profile"].as_str(), d[0]["switch"].as_bool()),
        (Some("opus"), Some(true))
    );
    // The session moved: its next message runs there without a verdict.
    let s = session(&r.core, &res.session_id);
    assert_eq!(s.routed.unwrap().profile.as_deref(), Some("opus"));
    assert_eq!(s.last_target.unwrap().profile, "opus");
    // Only the compilation the call used is stored, and it is Opus's.
    let c = r
        .core
        .store
        .get_compilation(s.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(c.manifest.model, "claude-opus-5-5");
    assert_eq!(
        r.core
            .store
            .session_compilations(&res.session_id)
            .unwrap()
            .len(),
        1
    );
}

/// Routine coding goes to GLM 5.3, on the other provider.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn routine_coding_goes_to_glm53() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(
        &r.core,
        None,
        "Rename these twelve call sites the same way.",
        None,
    )
    .await;
    assert_eq!(r.zai.requests()[0].model, "glm-5.3");
    assert!(r.claude.requests().is_empty());
    assert_eq!(res.profile, "glm53");
}

/// "thank you!" detours to the cheapest usable profile (GLM 5.3 Flash) for
/// that turn alone: the session's profile, `last_target`, and compilation
/// stay, so the next request on the session's own begins with the bytes of
/// the one before the detour.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trivial_message_detours_and_the_next_prefix_is_byte_identical() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 3, |_| {});
    let first = turn(&r.core, None, "What does the store's manifest hold?", None).await;
    let sid = first.session_id.clone();
    let before = session(&r.core, &sid);
    mode(&jev, "trivial", 0.95);
    let thanks = turn(&r.core, Some(&sid), "thank you!", None).await;
    assert_eq!(
        (thanks.profile.as_str(), thanks.model.as_str()),
        ("glm", "glm-5.3-flash")
    );
    assert_eq!(thanks.route.as_ref().unwrap().reason, "detour");
    // The detour's request: the last exchange and the message, no more.
    assert_eq!(r.zai.requests()[0].messages.len(), 3);
    let after = session(&r.core, &sid);
    assert_eq!(
        after.compilation_id, before.compilation_id,
        "the detour wrote no compilation"
    );
    assert_eq!(after.last_target, before.last_target);
    assert!(after.routed.is_none());
    mode(&jev, "chat", 0.95);
    let next = turn(&r.core, Some(&sid), "And where is it written?", None).await;
    assert_eq!(next.profile, "sonnet");
    let reqs = r.claude.requests();
    let (a, c) = (&reqs[0], &reqs[1]);
    assert_eq!(a.system, c.system);
    assert_eq!(a.tools, c.tools);
    assert_eq!(
        c.messages[..a.messages.len()],
        a.messages[..],
        "the prefix, byte for byte"
    );
    assert_eq!(session(&r.core, &sid).compilation_id, before.compilation_id);
}

/// Above `cold_switch_tokens`, a switch waits for a second turn in a row
/// that agrees; under `switch_confidence`, nothing routes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn above_cold_switch_tokens_a_switch_waits_for_a_second_agreeing_turn() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 3, |c| c.routing.cold_switch_tokens = 1);
    let one = turn(
        &r.core,
        None,
        "Design the write-ahead log's segment format.",
        None,
    )
    .await;
    assert_eq!(one.profile, "sonnet");
    assert_eq!(one.route.as_ref().unwrap().reason, "cache_hold");
    let hold = session(&r.core, &one.session_id)
        .routed
        .unwrap()
        .hold
        .unwrap();
    assert_eq!(hold.profile, "opus");
    let two = turn(
        &r.core,
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
    assert!(session(&r.core, &one.session_id)
        .routed
        .unwrap()
        .hold
        .is_none());
    // Under the confidence, the session stays where it is.
    mode(&jev, "routine_coding", 0.5);
    let three = turn(&r.core, Some(&one.session_id), "Now tidy the names.", None).await;
    assert_eq!(
        (
            three.profile.as_str(),
            three.route.as_ref().unwrap().reason.as_str()
        ),
        ("opus", "unsure")
    );
}

/// A turn whose profile the owner chose is not routed: its verdict is
/// recorded in shadow, and the turn says `pinned`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_turn_is_recorded_in_shadow_and_never_moved() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(
        &r.core,
        None,
        "Weigh two designs for the log.",
        Some("sonnet"),
    )
    .await;
    assert_eq!(res.profile, "sonnet");
    assert_eq!(r.claude.requests()[0].model, "claude-sonnet-5-5");
    assert_eq!(res.route.unwrap().reason, "pinned");
    let rows = until_route_rows(&r.core.store, 1).await;
    assert_eq!(rows[0].data["mode"], "shadow");
    assert_eq!(rows[0].data["context"]["pinned"], true);
    assert_eq!(rows[0].data["context"]["chosen"], "profile sonnet");
    assert!(session(&r.core, &res.session_id).routed.is_none());
}

/// The pane's carried profile is no choice; `-P`, `-p`, and `-m` are.
#[test]
fn the_panes_carried_profile_is_no_pin() {
    let p = |v: Value| serde_json::from_value::<theseus_protocol::TurnSubmitParams>(v).unwrap();
    let chosen = |v| crate::routing::chosen(&p(v));
    assert_eq!(chosen(serde_json::json!({"input": "hi"})), None);
    assert_eq!(
        chosen(serde_json::json!({"input": "hi", "profile": "opus", "carried": true})),
        None
    );
    assert_eq!(
        chosen(serde_json::json!({"input": "hi", "profile": "opus"})).as_deref(),
        Some("profile opus")
    );
    assert_eq!(
        chosen(serde_json::json!({"input": "hi", "model": "glm-5.3"})).as_deref(),
        Some("model glm-5.3")
    );
    assert_eq!(
        chosen(serde_json::json!({"input": "hi", "profile": "opus", "carried": true, "provider": "zai"})).as_deref(),
        Some("provider zai")
    );
}

/// Each failing Jev, and the judge off, leave the request as it is unrouted;
/// none waits past the bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_jev_or_the_judge_off_leaves_the_request_unrouted() {
    let off_rig = rig(None, 1, |_| {});
    let base = turn(&off_rig.core, None, "Say done.", None).await;
    assert!(base.route.is_none(), "the judge off: nothing asked");
    let base_req = serde_json::to_value(&off_rig.claude.requests()[0]).unwrap();
    for (m, reason) in [
        (FakeMode::Down, "no_verdict"),
        (FakeMode::Slow(Duration::from_secs(3)), "late"),
        (
            FakeMode::RateLimited {
                retry_after_secs: 7,
            },
            "no_verdict",
        ),
        (FakeMode::Malformed, "no_verdict"),
    ] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "sophisticated", 0.95);
        jev.set_mode(m.clone());
        let r = rig(Some(&jev), 1, |_| {});
        let t0 = Instant::now();
        let res = turn(&r.core, None, "Say done.", None).await;
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "{m:?}: {:?}",
            t0.elapsed()
        );
        assert_eq!(res.profile, "sonnet", "{m:?}");
        assert_eq!(res.route.as_ref().unwrap().reason, reason, "{m:?}");
        let req = serde_json::to_value(&r.claude.requests()[0]).unwrap();
        assert_eq!(req, base_req, "{m:?}: the request");
    }
}

/// A verdict past the wait applies from the next message: the first turn
/// says `late` and stays; the next, whose own verdict is late too, moves on
/// the first's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_applies_from_the_next_message() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    jev.set_mode(FakeMode::Slow(Duration::from_millis(500)));
    let r = rig(Some(&jev), 2, |_| {});
    let one = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(
        (
            one.profile.as_str(),
            one.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "late")
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    let two = turn(&r.core, Some(&one.session_id), "And the other one?", None).await;
    assert_eq!(two.profile, "opus", "{:?}", two.route);
    let d = decided(&r.core.store);
    assert_eq!(
        d[1]["judgment"],
        until_route_rows(&r.core.store, 1).await[0].data["id"]
    );
}

/// The greeting of 2026-10-04 23:45 (theseus-6n5j): route.v1 said trivial
/// at 0.45, under the section's 0.6, and the turn stayed on Sonnet. Trivial's
/// bar is 0.4: the same verdict detours, and a switch mode at 0.45 still
/// routes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_greeting_judged_trivial_at_045_detours() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.45);
    let r = rig(Some(&jev), 2, |_| {});
    let hi = turn(&r.core, None, "hey, good evening", None).await;
    assert_eq!(
        (
            hi.profile.as_str(),
            hi.route.as_ref().unwrap().reason.as_str()
        ),
        ("glm", "detour")
    );
    let d = decided(&r.core.store);
    assert_eq!(d[0]["confidence"], 0.45);
    mode(&jev, "sophisticated", 0.45);
    let hard = turn(&r.core, Some(&hi.session_id), "Weigh two designs.", None).await;
    assert_eq!(
        (
            hard.profile.as_str(),
            hard.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "unsure")
    );
}

/// A late trivial verdict never applies to a later message (theseus-6n5j):
/// the first message's verdict, trivial, comes after its wait; the second's
/// own is late too, and the second stays on the session's own profile, where
/// a late switch verdict would have moved it (`a_late_verdict_applies_from_
/// the_next_message`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_trivial_verdict_never_applies_to_the_next_message() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    jev.set_mode(FakeMode::Slow(Duration::from_millis(500)));
    let r = rig(Some(&jev), 2, |_| {});
    let one = turn(&r.core, None, "thanks!", None).await;
    assert_eq!(
        (
            one.profile.as_str(),
            one.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "late")
    );
    // Its verdict lands while no message waits for it.
    tokio::time::sleep(Duration::from_millis(800)).await;
    mode(&jev, "sophisticated", 0.95);
    let two = turn(
        &r.core,
        Some(&one.session_id),
        "Now weigh two designs for the log.",
        None,
    )
    .await;
    assert_eq!(
        (
            two.profile.as_str(),
            two.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "late"),
        "{:?}",
        two.route
    );
    assert!(r.zai.requests().is_empty(), "nothing detoured");
}

/// A person's message warms Jev's connections as it arrives (theseus-otny):
/// two `HEAD`s of the judge's path, nothing billed, beside the admission's
/// frames; a message while the client is warm sends none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_warms_jevs_connections_once_while_they_stay_warm() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 2, |_| {});
    let one = turn(&r.core, None, "What does the manifest hold?", None).await;
    assert_eq!(one.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(jev.warmups(), 2, "two connections opened at the message");
    assert_eq!(jev.connections(), 1, "and one call");
    turn(&r.core, Some(&one.session_id), "And where is it?", None).await;
    assert_eq!(jev.warmups(), 2, "warm: no second warm-up");
    assert_eq!(jev.connections(), 2);
}

/// route.v1's verdict never waits on its state's blob (theseus-otny): the
/// blob's two syncs (1.4 s each under a neighbour's IO, 2026-10-04) came
/// before the call, and made the verdict late. Held here, the verdict still
/// routes the turn; the sink writes the blob before the row that names it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_verdict_never_waits_on_its_states_blob() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let release = r.core.store.blobs().hold_puts();
    let thanks = turn(&r.core, None, "thanks!", None).await;
    assert_eq!(
        thanks.route.as_ref().unwrap().reason,
        "detour",
        "{:?}",
        thanks.route
    );
    drop(release);
    let rows = until_route_rows(&r.core.store, 1).await;
    let blob = rows[0].data["context"]["blob"].as_str().unwrap();
    assert!(
        r.core.store.blobs().path(blob).exists(),
        "the blob is written before its row"
    );
}

/// Jev known unreachable (theseus-otny): once a try to reach it fails to
/// connect, and nothing has answered since, a message's turn does not wait
/// for a verdict that cannot come. (A closed loopback port refuses on most
/// machines, and drops on this one, so the connect fails one way or the
/// other within its 1 s timeout.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_does_not_wait_on_an_unreachable_jev() {
    let jev = FakeJev::start().unwrap();
    let r = rig(Some(&jev), 2, |c| {
        c.judge.api_base = "http://127.0.0.1:9".into()
    });
    let one = turn(&r.core, None, "Say done.", None).await;
    assert_eq!(one.profile, "sonnet");
    let t0 = Instant::now();
    while !r.core.runner.judge.jev_unreachable() {
        assert!(t0.elapsed() < Duration::from_secs(10), "never unreachable");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let two = turn(&r.core, Some(&one.session_id), "Say done again.", None).await;
    let d = decided(&r.core.store);
    assert_eq!(d.len(), 2);
    assert!(d[1]["wait_ms"].as_u64().unwrap() < 50, "no wait: {}", d[1]);
    let reason = two.route.as_ref().unwrap().reason.clone();
    assert!(
        ["unreachable", "no_verdict"].contains(&reason.as_str()),
        "{reason}"
    );
}

/// The owner's choice of a profile within 10 minutes after a routed turn
/// labels its mode; 11 minutes after does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_choice_after_a_routed_turn_labels_its_mode() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = rig(Some(&jev), 2, |_| {});
    let one = turn(
        &r.core,
        None,
        "Why does the reader panic on this frame?",
        None,
    )
    .await;
    until_route_rows(&r.core.store, 1).await;
    // The owner picks GLM 5.3 (routine coding's alone) for the next one.
    turn(
        &r.core,
        Some(&one.session_id),
        "Just rename them.",
        Some("glm53"),
    )
    .await;
    until_route_rows(&r.core.store, 2).await;
    let scope = crate::learning::read_scope(&r.core.store, "route").unwrap();
    let mut seen: Vec<_> = scope.judgments.values().flatten().cloned().collect();
    seen.sort_by_key(|s| s.at_ms);
    let labels = |after_ms: u64| {
        let mut s = crate::learning::read_scope(&r.core.store, "route").unwrap();
        for v in s.judgments.values_mut().flatten() {
            if v.judgment.id == seen[1].judgment.id {
                v.at_ms = seen[0].at_ms + after_ms;
            }
        }
        r.core.system_labels("route", &s, seen[0].at_ms + 3_600_000)
    };
    let nine = labels(9 * 60_000);
    assert_eq!(nine.len(), 1, "{nine:?}");
    assert_eq!(nine[0].judgment, seen[0].judgment.id);
    assert_eq!(nine[0].label, serde_json::json!("routine_coding"));
    assert_eq!(nine[0].rule, "chosen");
    assert!(labels(11 * 60_000).is_empty());
}

/// A late verdict applies to the session's next message alone: a message
/// whose own verdict comes in time takes it, so a later late one never moves
/// on a verdict two messages old (review R3's join fix; seen live, a sed
/// one-liner on Opus, and a design question on GLM 5.3 Flash as a detour).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_applies_to_the_next_message_alone() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    jev.set_mode(FakeMode::Slow(Duration::from_millis(500)));
    let r = rig(Some(&jev), 3, |_| {});
    let one = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(one.route.as_ref().unwrap().reason, "late");
    // Its verdict lands while no message waits for it.
    tokio::time::sleep(Duration::from_millis(800)).await;
    // The next message's own verdict comes in time: chat's.
    mode(&jev, "chat", 0.95);
    jev.set_mode(FakeMode::Up);
    let two = turn(&r.core, Some(&one.session_id), "What is a frame?", None).await;
    assert_eq!(
        (
            two.profile.as_str(),
            two.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "verdict")
    );
    // The third's own verdict is late, and the first's is two messages old.
    jev.set_mode(FakeMode::Slow(Duration::from_millis(500)));
    let three = turn(&r.core, Some(&one.session_id), "And the other one?", None).await;
    assert_eq!(
        (
            three.profile.as_str(),
            three.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "late"),
        "{:?}",
        three.route
    );
}

/// A ladder rollback of route.v1 stops it routing (batch 5's join,
/// theseus-9j7x): a hard question stays on the session's own profile, its
/// verdict recorded in shadow, and health says the pack is rolled back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ladder_rollback_of_route_stops_it_routing() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    r.core
        .pack_rollback(
            &theseus_protocol::packs::PackRollbackParams {
                pack: "route.v1".into(),
                why: None,
            },
            "cli",
        )
        .unwrap();
    let res = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe write-ahead log.",
        None,
    )
    .await;
    assert_eq!(res.profile, "sonnet");
    assert_eq!(r.claude.requests()[0].model, "claude-sonnet-5-5");
    assert_eq!(res.route.unwrap().reason, "shadow");
    let rows = until_route_rows(&r.core.store, 1).await;
    assert_eq!(rows[0].data["mode"], "shadow");
    let h = r.core.health().judge.unwrap();
    assert!(
        h.packs
            .contains(&"route.v1: rolled back (owner: the owner rolled it back)".to_string()),
        "{:?}",
        h.packs
    );
}

/// route.v1's ladder events today: `pinned` ones.
fn pins(core: &Core) -> usize {
    let day = core.runner.judge.today();
    core.store
        .scope_after(&crate::judge::ladder::events_scope("route", &day), 0)
        .unwrap()
        .into_iter()
        .filter(|r| {
            let row: LedgerRow = r.decode().unwrap();
            row.data["event"]["event"] == "pinned"
        })
        .count()
}

/// The owner's pin of another profile within 10 minutes after a routed turn
/// lands on route.v1's ladder, once (26a's `pins_per_day`; batch 5's join,
/// theseus-9j7x); a pin in a session with no routed turn before it lands
/// none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pin_after_a_routed_turn_lands_on_the_ladder() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = rig(Some(&jev), 3, |_| {});
    turn(&r.core, None, "Just rename them.", Some("glm53")).await;
    let one = turn(
        &r.core,
        None,
        "Why does the reader panic on this frame?",
        None,
    )
    .await;
    assert_ne!(one.profile, "glm53", "routed elsewhere");
    turn(
        &r.core,
        Some(&one.session_id),
        "Just rename them.",
        Some("glm53"),
    )
    .await;
    let t0 = Instant::now();
    while pins(&r.core) == 0 {
        assert!(t0.elapsed() < Duration::from_secs(10), "no pin landed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        pins(&r.core),
        1,
        "one pin: the lone session's pin landed none"
    );
}

// ------------------------------------------- routing's state only while it acts (theseus-9yyr)

/// One request through the protocol server, as a client sends it: its
/// result.
async fn call(core: &Arc<Core>, method: &str, params: Value) -> Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = tokio::io::BufReader::new(cr).lines();
    let r = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    assert!(r.error.is_none(), "{method}: {:?}", r.error);
    r.result.unwrap_or(Value::Null)
}

/// A board on which Jev's key did not resolve.
fn keyless() -> Arc<crate::secrets::SecretBoard> {
    let b = crate::secrets::SecretBoard::new(["jev_api_key".to_string()], Instant::now());
    b.publish(
        std::collections::BTreeMap::from([(
            "jev_api_key".to_string(),
            Err("the vault does not hold it".to_string()),
        )]),
        "test",
    );
    b
}

/// A session routing moved to Opus, and its rig (`n` answers a provider).
/// The wait for a verdict is long, so a loaded machine never makes one late;
/// it ends as the verdict lands.
async fn moved_to_opus(jev: &FakeJev, n: usize) -> (Rig, String) {
    mode(jev, "sophisticated", 0.95);
    let r = rig(Some(jev), n, |c| c.routing.max_wait_ms = 5_000);
    let one = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe write-ahead log.",
        None,
    )
    .await;
    assert_eq!(one.profile, "opus");
    assert_eq!(
        session(&r.core, &one.session_id)
            .routed
            .unwrap()
            .profile
            .as_deref(),
        Some("opus")
    );
    until_route_rows(&r.core.store, 1).await;
    (r, one.session_id)
}

/// The ladder's rollback of route.v1 (its safety net) returns a session
/// routing moved to its own profile at its next message, its `routed`
/// cleared; and once route.v1 is live again, the session stays on its own
/// until a verdict moves it: nothing of the old move comes back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ladder_rollback_returns_a_routed_session_to_its_own_profile() {
    let jev = FakeJev::start().unwrap();
    let (r, sid) = moved_to_opus(&jev, 3).await;
    r.core
        .pack_rollback(
            &theseus_protocol::packs::PackRollbackParams {
                pack: "route.v1".into(),
                why: None,
            },
            "cli",
        )
        .unwrap();
    let two = turn(&r.core, Some(&sid), "And its recovery path?", None).await;
    assert_eq!(
        (two.profile.as_str(), two.model.as_str()),
        ("sonnet", "claude-sonnet-5-5")
    );
    assert_eq!(two.route.unwrap().reason, "shadow");
    let s = session(&r.core, &sid);
    assert!(s.routed.is_none(), "{:?}", s.routed);
    assert_eq!(s.last_target.unwrap().profile, "sonnet");
    r.core
        .pack_promote(
            &theseus_protocol::packs::PackPromoteParams {
                pack: "route.v1".into(),
                to: "live".into(),
                share: None,
                report: None,
            },
            "cli",
        )
        .unwrap();
    mode(&jev, "chat", 0.95);
    let three = turn(&r.core, Some(&sid), "What is a frame?", None).await;
    assert_eq!(
        (
            three.profile.as_str(),
            three.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "verdict")
    );
    let models: Vec<String> = r
        .claude
        .requests()
        .iter()
        .map(|q| q.model.clone())
        .collect();
    assert_eq!(
        models,
        ["claude-opus-5-5", "claude-sonnet-5-5", "claude-sonnet-5-5"]
    );
}

/// Routing off, routing in shadow, the judge off, and Jev's key gone, each
/// after a restart onto it: a session routing moved runs on its own profile
/// at its next message, and its `routed` is cleared.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn routing_off_shadow_the_judge_off_or_no_jev_return_a_routed_session() {
    use crate::config::routing::RoutingMode;
    type Tweak = fn(&mut Config);
    let cases: [(&str, Tweak, bool); 4] = [
        ("routing off", |c| c.routing.enabled = false, true),
        (
            "routing in shadow",
            |c| c.routing.mode = RoutingMode::Shadow,
            true,
        ),
        ("the judge off", |c| c.judge.enabled = false, true),
        ("Jev's key gone", |_| {}, false),
    ];
    let jev = FakeJev::start().unwrap();
    for (what, tweak, key) in cases {
        let (r, sid) = moved_to_opus(&jev, 1).await;
        let dir = r.dir.clone();
        drop(r);
        let secrets = if key { board() } else { keyless() };
        let r = rig_on(dir, Some(&jev), 1, tweak, secrets);
        let two = turn(&r.core, Some(&sid), "And its recovery path?", None).await;
        assert_eq!(
            (two.profile.as_str(), two.model.as_str()),
            ("sonnet", "claude-sonnet-5-5"),
            "{what}"
        );
        assert_eq!(r.claude.requests()[0].model, "claude-sonnet-5-5", "{what}");
        let s = session(&r.core, &sid);
        assert!(s.routed.is_none(), "{what}: {:?}", s.routed);
    }
}

/// `profile.use` moves a routed session to the profile it names at its next
/// message; it is no pin (25e's design), so routing moves the session again
/// on a later verdict.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_use_moves_a_routed_session_and_pins_nothing() {
    let jev = FakeJev::start().unwrap();
    let (r, sid) = moved_to_opus(&jev, 3).await;
    call(&r.core, "profile.use", serde_json::json!({"name": "glm"})).await;
    mode(&jev, "chat", 0.95);
    let two = turn(&r.core, Some(&sid), "What is a frame?", None).await;
    assert_eq!(
        (two.profile.as_str(), two.model.as_str()),
        ("glm", "glm-5.3-flash")
    );
    assert_eq!(r.zai.requests()[0].model, "glm-5.3-flash");
    assert!(session(&r.core, &sid).routed.is_none());
    mode(&jev, "sophisticated", 0.95);
    let three = turn(&r.core, Some(&sid), "Weigh the two once more.", None).await;
    assert_eq!(
        (
            three.profile.as_str(),
            three.route.as_ref().unwrap().reason.as_str()
        ),
        ("opus", "verdict")
    );
}

/// The pane carries the profile its session's last turn ran on, so after a
/// switch it carries the routed one: while routing acts, that runs there;
/// once it does not, the carried profile names nothing and the session's own
/// runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_panes_carried_routed_profile_runs_only_while_routing_acts() {
    let jev = FakeJev::start().unwrap();
    let (r, sid) = moved_to_opus(&jev, 3).await;
    let pane = |input: &str| {
        serde_json::json!({
            "session_id": sid, "input": input, "profile": "opus", "carried": true
        })
    };
    mode(&jev, "chat", 0.95);
    let two: TurnSubmitResult =
        serde_json::from_value(call(&r.core, "turn.submit", pane("What is a frame?")).await)
            .unwrap();
    assert_eq!(two.profile, "opus", "routing acts: the session stays moved");
    r.core
        .pack_rollback(
            &theseus_protocol::packs::PackRollbackParams {
                pack: "route.v1".into(),
                why: None,
            },
            "cli",
        )
        .unwrap();
    let three: TurnSubmitResult =
        serde_json::from_value(call(&r.core, "turn.submit", pane("And a segment?")).await).unwrap();
    assert_eq!(
        (three.profile.as_str(), three.model.as_str()),
        ("sonnet", "claude-sonnet-5-5")
    );
    assert!(session(&r.core, &sid).routed.is_none());
}
