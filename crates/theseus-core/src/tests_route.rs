//! Routing (M5 25e; `crate::routing`, `turn::route_step`): `route.v1` at
//! `inbound`, live, in a request of its own beside `classify.v4` and
//! `role.v1`'s (theseus-ddbi; `tests_route_wait`), and
//! the turn it moves, against the fake Jev, with the profiles on two fake
//! providers (`anthropic` and `zai`), so each request shows where it went.
//! A detour's request passes 35a's check: its window's arrangement is
//! admitted and its recall notes left out (theseus-783a).

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

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    /// The `anthropic` provider's fake: sonnet, opus, fable.
    pub(crate) claude: Arc<FakeProvider>,
    /// The `zai` provider's fake: glm (5.3 Flash) and glm53.
    pub(crate) zai: Arc<FakeProvider>,
    /// A second rig on the same dir is a restart.
    pub(crate) dir: Arc<tempfile::TempDir>,
}

/// Every judge pack but the inbound point's off, so its one call is the
/// only one; `route.v1` live, as the build gives it.
fn routing_only(c: &mut Config) {
    c.judge.packs.insert("loop.v1".into(), off());
    c.judge
        .packs
        .insert(crate::judge::gate::SECURITY_PACK.into(), off());
    for p in [
        crate::judge::compile::CONTINUE_PACK,
        crate::judge::categorize::PACK,
        crate::judge::rerank::RERANK_PACK,
    ] {
        c.judge.packs.insert(p.into(), off());
    }
}

/// The placements before Haiku 5.5 (theseus-3okf): a trivial detour to GLM
/// 5.3 Flash and routine programming on GLM 5.3, both on the `zai` fake, for
/// the tests whose detour or switch is meant to leave the session's provider.
/// GLM is only that second provider there, so its profiles run an
/// unrestricted model (`Config::serve_glm_unrestricted`, theseus-a1wvq.3).
pub(crate) fn glm_placements(c: &mut Config) {
    c.serve_glm_unrestricted();
}

/// GLM itself, of restricted origin, where routing may still name it: trivial's
/// `cn_profiles` (the data floor's tests). A mode's `profiles` may not name a
/// restricted profile (theseus-a1wvq.4), so routine programming keeps its own.
pub(crate) fn restricted_placements(_c: &mut Config) {
}

pub(crate) fn rig(jev: Option<&FakeJev>, n: usize, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = Arc::new(crate::scratch::tempdir());
    rig_on(dir, jev, n, tweak, board())
}

/// The rig, with `parts` edited before the core is built (telemetry's
/// pipeline, theseus-490i).
pub(crate) fn rig_parts(
    jev: Option<&FakeJev>,
    n: usize,
    tweak: impl FnOnce(&mut Config),
    parts: impl FnOnce(&mut Parts),
) -> Rig {
    let dir = Arc::new(crate::scratch::tempdir());
    rig_built(dir, jev, n, tweak, board(), parts)
}

/// The rig on `dir`'s store, with `secrets` as the board. A verdict's wait
/// is 5 s, which no load reaches (theseus-biy3: under starvation, a verdict
/// came after the build's 200 ms and the turn read `late`); it ends as the
/// verdict lands, so a quick one costs nothing. A test about lateness takes
/// it from the fake's `Held` answer, never from the wait.
pub(crate) fn rig_on(
    dir: Arc<tempfile::TempDir>,
    jev: Option<&FakeJev>,
    n: usize,
    tweak: impl FnOnce(&mut Config),
    secrets: Arc<crate::secrets::SecretBoard>,
) -> Rig {
    rig_built(dir, jev, n, tweak, secrets, |_| {})
}

/// The rig with the kernel on `clock`, for a test that drives time itself (theseus-2ix1k).
pub(crate) fn rig_on_clock(
    jev: Option<&FakeJev>,
    n: usize,
    tweak: impl FnOnce(&mut Config),
    clock: Arc<dyn theseus_kernel::Clock>,
) -> Rig {
    let dir = Arc::new(crate::scratch::tempdir());
    rig_with(dir, jev, n, tweak, board(), |_| {}, clock)
}

fn rig_built(
    dir: Arc<tempfile::TempDir>,
    jev: Option<&FakeJev>,
    n: usize,
    tweak: impl FnOnce(&mut Config),
    secrets: Arc<crate::secrets::SecretBoard>,
    parts: impl FnOnce(&mut Parts),
) -> Rig {
    rig_with(
        dir,
        jev,
        n,
        tweak,
        secrets,
        parts,
        Arc::new(theseus_kernel::RealClock),
    )
}

fn rig_with(
    dir: Arc<tempfile::TempDir>,
    jev: Option<&FakeJev>,
    n: usize,
    tweak: impl FnOnce(&mut Config),
    secrets: Arc<crate::secrets::SecretBoard>,
    parts: impl FnOnce(&mut Parts),
    clock: Arc<dyn theseus_kernel::Clock>,
) -> Rig {
    let mut cfg = crate::tests_judge::judge_config(dir.path(), jev);
    routing_only(&mut cfg);
    cfg.routing.max_wait_ms = 5_000;
    // The template's keys are placeholders, which name no item: a pick on a
    // provider with one is a fallback at pick time (route.v10).
    cfg.name_every_provider_key();
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let claude = Arc::new(FakeProvider::scripted(texts(n)));
    let zai = Arc::new(FakeProvider::scripted(texts(n)));
    let mut p = Parts::for_tests(cfg, claude.clone(), store);
    p.providers.insert("zai".into(), zai.clone());
    p.secrets = secrets;
    parts(&mut p);
    let core = Core::build_with_clock(p, clock).unwrap();
    crate::tests_judge::warm(&core);
    Rig {
        core,
        claude,
        zai,
        dir,
    }
}

/// A person's message: on the live profile, or on `chosen` as the owner's
/// choice (`ask -P`).
pub(crate) async fn turn(
    core: &Arc<Core>,
    session: Option<&str>,
    input: &str,
    chosen: Option<&str>,
) -> TurnSubmitResult {
    turn_attached(core, session, input, chosen, Vec::new()).await
}

/// [`turn`] with files that came with the input.
pub(crate) async fn turn_attached(
    core: &Arc<Core>,
    session: Option<&str>,
    input: &str,
    chosen: Option<&str>,
    attachments: Vec<theseus_protocol::Attachment>,
) -> TurnSubmitResult {
    try_turn_attached(core, session, input, chosen, attachments)
        .await
        .unwrap()
}

/// [`turn_attached`], the turn's failure returned.
pub(crate) async fn try_turn_attached(
    core: &Arc<Core>,
    session: Option<&str>,
    input: &str,
    chosen: Option<&str>,
    attachments: Vec<theseus_protocol::Attachment>,
) -> anyhow::Result<TurnSubmitResult> {
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
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments,
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// Jev's answer to a message, in the words of the class the old route pack
/// asked (route.v10 asks `model` and `context` instead, theseus-h66v4, row 6):
/// a trivial or quick message is Haiku answered from the last exchanges alone
/// (`short`), a review or a hard design question Opus, programming Sonnet,
/// conversation the session's own Sonnet.
pub(crate) fn mode(jev: &FakeJev, m: &str, confidence: f64) {
    let (model, context) = match m {
        "trivial" | "quick" => ("haiku", "short"),
        "sophisticated" | "review" | "deep_coding" => ("opus", "full"),
        _ => ("sonnet", "full"),
    };
    for (question, option) in [("model", model), ("context", context)] {
        jev.script(
            question,
            Jev::Choice {
                option: option.into(),
                confidence,
            },
        );
    }
}

pub(crate) fn decided(store: &Store) -> Vec<Value> {
    kinds(store, "route.decided")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

pub(crate) fn session(core: &Core, id: &str) -> SessionRecord {
    core.store
        .get_session::<SessionRecord>(id)
        .unwrap()
        .unwrap()
}

/// Wait (on the runtime's timer) until the `judge:route` scope has `n` rows.
pub(crate) async fn until_route_rows(store: &Store, n: usize) -> Vec<LedgerRow> {
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

/// Wait (on the runtime's timer) until the fake Jev has read `n` calls: the
/// turn reads `route.v1`'s answer, never the batch's beside it, so the
/// batch may still be on its way (theseus-ddbi).
pub(crate) async fn until_calls(jev: &FakeJev, n: usize) {
    let t0 = Instant::now();
    while jev.connections() < n {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} calls",
            jev.connections()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Late, by order (theseus-biy3): the fake holds every verdict (`Held`), and
/// the client's own limit is far past any turn's work, so a held verdict
/// comes only once the test releases it. The wait is the build's 200 ms, to
/// keep the test short: a held verdict is late at any wait.
fn held(c: &mut Config) {
    c.routing.max_wait_ms = 200;
    c.judge.total_secs = 30;
}

/// Wait (on the runtime's timer) until the session holds a late verdict:
/// the released one has landed, so the next message reads it.
pub(crate) async fn until_late(core: &Core, sid: &str) {
    let t0 = Instant::now();
    while !core.runner.judge.has_late(sid) {
        assert!(t0.elapsed() < Duration::from_secs(20), "no late verdict");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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

/// Each failing Jev, and the judge off, leave the request as it is unrouted.
/// A failed call ends the wait: under the rig's 5 s wait, `no_verdict` (not
/// `late`) says the failure, not the wait's end, released the turn. A held
/// verdict is `late`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_jev_or_the_judge_off_leaves_the_request_unrouted() {
    let off_rig = rig(None, 1, |_| {});
    let base = turn(&off_rig.core, None, "Say done.", None).await;
    assert!(base.route.is_none(), "the judge off: nothing asked");
    let base_req = serde_json::to_value(&off_rig.claude.requests()[0]).unwrap();
    for (m, reason) in [
        (FakeMode::Down, "no_verdict"),
        (FakeMode::Held, "late"),
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
        let r = match m {
            FakeMode::Held => rig(Some(&jev), 1, held),
            _ => rig(Some(&jev), 1, |_| {}),
        };
        let res = turn(&r.core, None, "Say done.", None).await;
        jev.release();
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
    jev.set_mode(FakeMode::Held);
    let r = rig(Some(&jev), 2, held);
    let one = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(
        (
            one.profile.as_str(),
            one.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "late")
    );
    jev.release();
    until_late(&r.core, &one.session_id).await;
    let two = turn(&r.core, Some(&one.session_id), "And the other one?", None).await;
    jev.release();
    assert_eq!(two.profile, "opus", "{:?}", two.route);
    let d = decided(&r.core.store);
    assert_eq!(
        d[1]["judgment"],
        until_route_rows(&r.core.store, 1).await[0].data["id"]
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
    jev.set_mode(FakeMode::Held);
    let r = rig(Some(&jev), 2, held);
    let one = turn(&r.core, None, "thanks!", None).await;
    assert_eq!(
        (
            one.profile.as_str(),
            one.route.as_ref().unwrap().reason.as_str()
        ),
        ("sonnet", "late")
    );
    // Its verdict lands while no message waits for it.
    jev.release();
    until_late(&r.core, &one.session_id).await;
    mode(&jev, "sophisticated", 0.95);
    let two = turn(
        &r.core,
        Some(&one.session_id),
        "Now weigh two designs for the log.",
        None,
    )
    .await;
    jev.release();
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
    until_calls(&jev, 2).await;
    assert_eq!(
        jev.connections(),
        2,
        "and two calls: route.v8's and the batch"
    );
    turn(&r.core, Some(&one.session_id), "And where is it?", None).await;
    assert_eq!(jev.warmups(), 2, "warm: no second warm-up");
    until_calls(&jev, 4).await;
    assert_eq!(jev.connections(), 4);
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

/// A late verdict applies to the session's next message alone: a message
/// whose own verdict comes in time takes it, so a later late one never moves
/// on a verdict two messages old (review R3's join fix; seen live, a sed
/// one-liner on Opus, and a design question on GLM 5.3 Flash as a detour).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_applies_to_the_next_message_alone() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    jev.set_mode(FakeMode::Held);
    // The second message's verdict must come in time: the rig's 5 s wait,
    // which a held verdict waits out.
    let r = rig(Some(&jev), 3, |c| c.judge.total_secs = 30);
    let one = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(one.route.as_ref().unwrap().reason, "late");
    // Its verdict lands while no message waits for it.
    jev.release();
    until_late(&r.core, &one.session_id).await;
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
    jev.set_mode(FakeMode::Held);
    let three = turn(&r.core, Some(&one.session_id), "And the other one?", None).await;
    jev.release();
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
pub(crate) async fn call(core: &Arc<Core>, method: &str, params: Value) -> Value {
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
pub(crate) fn keyless() -> Arc<crate::secrets::SecretBoard> {
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
pub(crate) async fn moved_to_opus(jev: &FakeJev, n: usize) -> (Rig, String) {
    moved_to_opus_with(jev, n, |_| {}).await
}

/// [`moved_to_opus`] with `tweak` applied to the config after the placements.
pub(crate) async fn moved_to_opus_with(
    jev: &FakeJev,
    n: usize,
    tweak: impl FnOnce(&mut Config),
) -> (Rig, String) {
    mode(jev, "sophisticated", 0.95);
    let r = rig(Some(jev), n, |c| {
        c.routing.max_wait_ms = 5_000;
        glm_placements(c);
        tweak(c);
    });
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

/// The note recall finds for every message in the detour tests: an invented
/// fact in a session of its own, which the stand-in index answers.
const KEY_NOTE: &str = "Remember: the lock-keeper's spare key hangs behind the tide board.";

/// A rig whose recall is live in front of the model (`[memory] mode =
/// "live"`, as the owner's install runs it), the stand-in index answering
/// `KEY_NOTE`'s session for every message, and a verdict's wait long enough
/// that a loaded machine never makes one late. With the note's node id.
fn recalling(jev: &FakeJev, n: usize) -> (Rig, String) {
    let r = rig(Some(jev), n, |c| {
        c.memory.mode = crate::config::MemoryMode::Live;
        c.routing.max_wait_ms = 5_000;
        glm_placements(c);
    });
    let notes = crate::tests_recall::session(&r.core, None, &[KEY_NOTE]);
    let ask = crate::tests_recall::index_of(&r.core, vec![notes.clone()]);
    r.core.runner.memory.set_ask(ask);
    let note = r.core.store.session_nodes(&notes).unwrap()[0].1.id.clone();
    (r, note)
}

/// A session's `Recall` nodes.
fn recall_nodes(core: &Core, sid: &str) -> Vec<crate::node::Node> {
    let nodes = core.store.session_nodes(sid).unwrap().into_iter();
    nodes
        .map(|(_, n)| n)
        .filter(|n| matches!(n.body, crate::node::Body::Recall { .. }))
        .collect()
}

/// The edges into `node`: each `Recall` node's `derived_from` to its source.
fn edges_into(core: &Core, node: &str) -> usize {
    let scope = crate::graph::Edge::scope_into(node);
    let records = core.store.scope_after(&scope, 0).unwrap();
    records
        .iter()
        .filter(|r| r.kind == theseus_store::kinds::EDGE)
        .count()
}

/// A request's message that carries the recalled note.
fn carries_note(m: &Value) -> bool {
    let m = m.to_string();
    m.contains("[Recalled by the harness") || m.contains("tide board")
}

/// A quick detour sends its turn's recall and writes it (theseus-pa1xx): a
/// quick question about what was said or decided before is answered from the
/// note, after the message, as a kept compile sends it. The `Recall` node and
/// its edge are written, the footer counts the note, and the row says `ran`.
/// Before, a fresh session's "what tag did he choose?" went to Haiku with the
/// right note admitted and thrown away (`detoured`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quick_detour_sends_its_recall_and_writes_it() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.95);
    let (r, note) = recalling(&jev, 1);
    let ask = "Where does the lock-keeper keep the spare key?";
    let res = turn(&r.core, None, ask, None).await;
    let route = res.route.as_ref().unwrap();
    assert_eq!(
        (
            res.profile.as_str(),
            route.reason.as_str(),
            route.mode.as_deref()
        ),
        ("haiku", "verdict", Some("haiku"))
    );
    let q = &r.claude.requests()[0];
    assert_eq!(q.model, "claude-haiku-5-5");
    let (last, before) = q.messages.split_last().unwrap();
    assert!(carries_note(last), "the note, after the message: {last}");
    assert!(!before.iter().any(carries_note), "{before:?}");
    assert_eq!(res.recalled, 1, "the footer counts what the model saw");
    let sid = &res.session_id;
    assert_eq!(recall_nodes(&r.core, sid).len(), 1, "its Recall node");
    assert_eq!(edges_into(&r.core, &note), 1, "its edge into the note");
    let rows = crate::tests_recall::recalls(&r.core, sid);
    assert_eq!(rows.len(), 1, "one recall row");
    assert_eq!(rows[0].outcome, "ran");
    let rec = session(&r.core, sid);
    assert!(
        rec.compilation_id.is_none(),
        "a detour writes no compilation"
    );
    assert!(unadmitted(&r.core).is_empty());
}

/// A quick detour's later loop keeps its recall (theseus-pa1xx): once the
/// first loop's plan frame has written the `Recall` node, the detour's next
/// compile lets that node, and no other note, through with its window, and
/// 35a's check admits it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quick_detours_later_loop_keeps_its_recall() {
    use crate::provider::Scripted;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.95);
    let haiku = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("t1", "thread_read", serde_json::json!({}))]),
        Scripted::text("Behind the tide board."),
    ]));
    let p = haiku.clone();
    let r = rig_parts(
        Some(&jev),
        1,
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
            c.routing.max_wait_ms = 5_000;
        },
        move |parts| {
            parts.providers.insert("anthropic".into(), p);
        },
    );
    let notes = crate::tests_recall::session(&r.core, None, &[KEY_NOTE]);
    let ask = crate::tests_recall::index_of(&r.core, vec![notes]);
    r.core.runner.memory.set_ask(ask);
    let msg = "Call the thread_read tool once, then say where the lock-keeper keeps the spare key.";
    let res = turn(&r.core, None, msg, None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    assert_eq!((res.tool_calls, res.recalled), (1, 1));
    let reqs = haiku.requests();
    assert_eq!(reqs.len(), 2, "two loops");
    for (i, q) in reqs.iter().enumerate() {
        let n = q.messages.iter().filter(|m| carries_note(m)).count();
        assert_eq!(n, 1, "loop {i} carries the note once: {:?}", q.messages);
    }
    assert!(unadmitted(&r.core).is_empty());
}

/// A non-trivial routed turn still writes and sends its recall, unchanged
/// (theseus-n7nc): the row held while routing decided is recorded once the
/// route is known, and says `ran`. A switch (Opus, compiled again on its
/// profile) and a verdict that keeps the session's own (Sonnet: the first
/// compile is the one used) each send the note after the message, write the
/// `Recall` node and its edge, and count the note in the footer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turn_still_writes_and_sends_its_recall() {
    let jev = FakeJev::start().unwrap();
    let (r, note) = recalling(&jev, 2);
    for (verdict, profile, model) in [
        ("sophisticated", "opus", "claude-opus-5-5"),
        ("chat", "sonnet", "claude-sonnet-5-5"),
    ] {
        mode(&jev, verdict, 0.95);
        let ask = "Where does the lock-keeper keep the spare key?";
        let res = turn(&r.core, None, ask, None).await;
        assert_eq!(
            (res.profile.as_str(), res.recalled),
            (profile, 1),
            "{verdict}"
        );
        let q = r.claude.requests().pop().unwrap();
        assert_eq!(q.model, model);
        let (last, before) = q.messages.split_last().unwrap();
        assert!(carries_note(last), "{verdict}: the note, after the message");
        assert!(!before.iter().any(carries_note), "{verdict}");
        let nodes = recall_nodes(&r.core, &res.session_id);
        assert_eq!(nodes.len(), 1, "{verdict}: its Recall node");
        let crate::node::Body::Recall { recall_id, .. } = &nodes[0].body else {
            unreachable!()
        };
        let rows = crate::tests_recall::recalls(&r.core, &res.session_id);
        assert_eq!(rows.len(), 1, "{verdict}: one recall row");
        assert_eq!(
            (rows[0].recall_id.as_str(), rows[0].outcome.as_str()),
            (recall_id.as_str(), "ran"),
            "{verdict}"
        );
    }
    assert_eq!(
        edges_into(&r.core, &note),
        2,
        "each turn's node, into the note"
    );
}

/// A switched turn's recall drops reach the compilation it stores
/// (theseus-3urn). Recall's pack has room for one of the two heron notes, so
/// it drops the other for its budget. On a switch, the first compile (on the
/// session's profile, then discarded) took the drops, and the stored one, on
/// Opus, named none; a verdict that keeps the first compile stored them, and
/// still does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_switched_turns_compilation_names_its_recall_drops() {
    let jev = FakeJev::start().unwrap();
    let r = rig(Some(&jev), 2, |c| {
        c.memory.mode = crate::config::MemoryMode::Live;
        c.memory.recall_budget_tokens = 30;
    });
    // Each note about 25 tokens: one fits the pack, two do not.
    let notes = crate::tests_recall::session(
        &r.core,
        None,
        &[
            "the heron nests by the weir in spring, and the reeds there are cut back each autumn",
            "the heron fishes at dawn by the mill, where the race runs shallow over the stones",
        ],
    );
    let ask = crate::tests_recall::index_of(&r.core, vec![notes.clone()]);
    r.core.runner.memory.set_ask(ask);
    let ids: Vec<String> = r
        .core
        .store
        .session_nodes(&notes)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n.id)
        .collect();
    for (verdict, profile) in [("sophisticated", "opus"), ("chat", "sonnet")] {
        mode(&jev, verdict, 0.95);
        let res = turn(&r.core, None, "Where does the heron nest and fish?", None).await;
        assert_eq!(
            (res.profile.as_str(), res.recalled),
            (profile, 1),
            "{verdict}"
        );
        let s = session(&r.core, &res.session_id);
        let c = r
            .core
            .store
            .get_compilation(s.compilation_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        let budget = c.budget.expect("a budget report");
        let dropped: Vec<&str> = budget
            .dropped
            .iter()
            .filter(|d| d.tier == "recall")
            .filter_map(|d| d.node_id.as_deref())
            .collect();
        assert_eq!(dropped.len(), 1, "{verdict}: {budget:?}");
        assert!(ids.iter().any(|id| id == dropped[0]), "{verdict}");
    }
}

/// The `context.unadmitted` rows.
fn unadmitted(core: &Core) -> Vec<Value> {
    let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(2000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == "context.unadmitted")
        .map(|(_, r)| r.data)
        .collect()
}
