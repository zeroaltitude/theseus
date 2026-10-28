//! The turn's floor on content flags (theseus-9ymz0, design §4.2b, §4.3 and
//! §11's tests 21 to 23): a turn bound for a model of restricted origin runs
//! on it only when its session's labels allow it and every item was judged; it
//! waits for the verdicts, at most `[content_based_security] wait_ms`, then goes to the
//! `[model]` default and completes; a turn bound for Claude never waits. The
//! rig is `tests_route`'s: `anthropic` (Claude) and `zai` (GLM, a China-origin
//! endpoint), each a fake that records its requests; `judging` stands a test's
//! judge behind the runner.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::{JudgePackConfig, PackMode};
use crate::content::{ContentFlags, Flag};
use crate::content_wait::judging::{Judging, Silent};
use crate::provider::Scripted;
use crate::routing::Routed;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::tests_dm_rig::{blocked, dm, judging_with, rig_with, say, text};
use crate::tests_route::{
    call, decided, mode, restricted_placements, rig, rig_parts, turn, until_late, Rig,
};
use crate::turn::TurnRequest;

const PLAIN: &str = "Rename the call sites in the billing module.";

/// How a turn comes to be bound for GLM.
#[derive(Debug, Clone, Copy)]
enum Case {
    NoVerdict,
    Late,
    Unsure,
    Shadow,
    Pin,
    Correction,
    Hold,
    RoutedBase,
    TrivialDetour,
    RoutingOff,
}

/// One case's turn, on a fresh rig, its judge `judge`: its result.
async fn bound_for_glm(case: Case, judge: Option<Arc<Judging>>) -> (Rig, TurnSubmitResult) {
    let jev = FakeJev::start().unwrap();
    let verdict = match case {
        Case::Unsure => ("sophisticated", 0.2),
        Case::Hold => ("routine_coding", 0.95),
        Case::TrivialDetour => ("trivial", 0.95),
        _ => ("chat", 0.95),
    };
    mode(&jev, verdict.0, verdict.1);
    let r = match case {
        Case::RoutingOff => rig(None, 2, |_| {}),
        Case::Late => rig(Some(&jev), 2, |c| {
            c.routing.max_wait_ms = 200;
            c.judge.total_secs = 30;
        }),
        Case::Shadow => rig(Some(&jev), 2, |c| {
            let shadow = JudgePackConfig {
                mode: Some(PackMode::Shadow),
                sample: None,
                notices: None,
            };
            let pack = crate::judge::inbound::ROUTE_PACK;
            c.judge.packs.insert(pack.into(), shadow);
        }),
        _ => rig(Some(&jev), 2, restricted_placements),
    };
    if let Some(j) = judge {
        judging_with(&r, j);
    }
    match case {
        Case::NoVerdict => jev.set_mode(FakeMode::Down),
        Case::Late => jev.set_mode(FakeMode::Held),
        _ => {}
    }
    let on_glm = !matches!(case, Case::Pin | Case::Correction | Case::RoutedBase);
    if on_glm {
        call(&r.core, "profile.use", json!({"name": "glm"})).await;
    }
    let res = match case {
        Case::Pin => turn(&r.core, None, PLAIN, Some("glm")).await,
        Case::Correction => {
            let one = turn(&r.core, None, PLAIN, None).await;
            assert_eq!(one.profile, "sonnet", "the live profile");
            let words = "hey, that should have been on glm";
            crate::tests_route_correct::ask(&r.core, Some(&one.session_id), words).await
        }
        Case::Hold | Case::RoutedBase => {
            let mut rec = SessionRecord::new(SessionKind::Conversation, None);
            rec.routed = Some(Box::new(match case {
                Case::Hold => Routed {
                    profile: None,
                    from: None,
                    hold: Some(crate::routing::Hold {
                        mode: "routine_coding".into(),
                        profile: "glm".into(),
                        turn: "trn_earlier".into(),
                    }),
                },
                _ => Routed {
                    profile: Some("glm".into()),
                    from: Some("sonnet".into()),
                    hold: None,
                },
            }));
            r.core.store.put_session(&rec.session_id, &rec).unwrap();
            turn(&r.core, Some(&rec.session_id), PLAIN, None).await
        }
        _ => turn(&r.core, None, PLAIN, None).await,
    };
    jev.release();
    if matches!(case, Case::Late) {
        until_late(&r.core, &res.session_id).await;
    }
    (r, res)
}

const EVERY_CASE: [Case; 10] = [
    Case::NoVerdict,
    Case::Late,
    Case::Unsure,
    Case::Shadow,
    Case::Pin,
    Case::Correction,
    Case::Hold,
    Case::RoutedBase,
    Case::TrivialDetour,
    Case::RoutingOff,
];

/// Every way a turn comes to be bound for a restricted profile, with the
/// session's items not judged and no judge to wait for (the stand-in): it runs
/// on the `[model]` default instead, with a `route.decided` row whose reason is
/// `data_gate`, and nothing reaches the restricted model. Ported from the open
/// view's floor test, whose cases it keeps: only the reason for the move
/// differs (the session's items, not a missing open verdict).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restricted_base_moves_to_the_fallback_with_data_gate() {
    for case in EVERY_CASE {
        let (r, res) = bound_for_glm(case, None).await;
        // A hold or a trivial detour toward GLM meets routing's own gate first:
        // a mode's pick never yields a restricted profile for a session that
        // may not use one (theseus-a1wvq.4), so the verdict takes the mode's
        // next usable profile, and the floor has nothing to move.
        let (profile, model, reason) = match case {
            Case::Hold => ("haikuhi", "claude-haiku-5-5", "verdict"),
            Case::TrivialDetour => ("haiku", "claude-haiku-5-5", "detour"),
            _ => ("default", "claude-sonnet-5-5", "data_gate"),
        };
        assert_eq!(
            res.profile, profile,
            "{case:?}: {} {:?}",
            res.output, res.route
        );
        assert_eq!(res.model, model, "{case:?}");
        let rows = decided(&r.core.store);
        let last = rows.last().unwrap_or_else(|| panic!("{case:?}: no row"));
        assert_eq!(last["reason"], reason, "{case:?}: {rows:?}");
        assert_eq!(
            res.route.as_ref().map(|r| r.reason.as_str()),
            Some(reason),
            "{case:?}"
        );
        assert!(
            r.zai.requests().is_empty(),
            "{case:?}: GLM was sent nothing"
        );
        assert!(
            blocked(&r.core) == 0,
            "{case:?}: it never reached the chokepoint"
        );
        assert!(!r.claude.requests().is_empty(), "{case:?}");
    }
}

/// The same cases with a judge that finds nothing: the session carries no
/// label and its items are judged, so the pinned, held and routed ones run on
/// GLM itself, and the chokepoint sends them. **This is the case the open view
/// could not pass**: a restricted model, a session with nothing found in it,
/// and the turn on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_judged_clean_session_runs_on_the_restricted_model() {
    for case in [
        Case::Pin,
        Case::RoutedBase,
        Case::RoutingOff,
        Case::NoVerdict,
    ] {
        let judge = Judging::clean();
        let (r, res) = bound_for_glm(case, Some(judge.clone())).await;
        assert_eq!(
            res.profile, "glm",
            "{case:?}: {} {:?}",
            res.output, res.route
        );
        assert!(!r.zai.requests().is_empty(), "{case:?}: GLM was sent it");
        assert_eq!(blocked(&r.core), 0, "{case:?}: nothing was refused");
        // A turn that asked no route says nothing of its data.
        if let Some(data) = res.route.and_then(|r| r.data) {
            assert_eq!(data.cn, "allowed", "{case:?}");
            assert_eq!(data.origin, "cn", "{case:?}");
        }
        assert!(
            !judge.waits.lock().unwrap().is_empty(),
            "{case:?}: the turn waited on a verdict that was not there yet"
        );
    }
}

/// Test 21: a turn bound for a restricted model with an item the judge has not
/// reached waits for its verdict, at most `[content_based_security] wait_ms`, then runs on the
/// `[model]` default with `Reason::DataGate`: it **completes**. Nothing goes to
/// the restricted model, and nothing is refused at the chokepoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cn_turn_waits_for_its_verdict_then_goes_to_claude_and_completes() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 300);
    let silent = Arc::new(Silent::default());
    r.core.runner.verdicts.set(silent.clone());
    let started = Instant::now();
    let res = turn(&r.core, None, PLAIN, Some("glm")).await;
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(300), "it waited: {took:?}");
    assert!(took < Duration::from_secs(10), "and only so long: {took:?}");
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("default", "claude-sonnet-5-5"),
        "{:?}",
        res.route
    );
    assert!(!res.output.is_empty(), "the turn completed: {res:?}");
    let rows = decided(&r.core.store);
    assert_eq!(rows.last().unwrap()["reason"], "data_gate", "{rows:?}");
    assert_eq!(rows.last().unwrap()["data_gate"], "unjudged", "{rows:?}");
    let data = res.route.unwrap().data.unwrap();
    assert_eq!(
        (data.cn.as_str(), data.gate.as_deref()),
        ("unjudged", Some("unjudged"))
    );
    assert!(
        !silent.waits.lock().unwrap().is_empty(),
        "it waited on the notify"
    );
    assert!(r.zai.requests().is_empty(), "GLM was sent nothing");
    assert_eq!(blocked(&r.core), 0, "it never met the chokepoint");
}

/// `[content_based_security] wait_ms = 0` never waits: the unjudged item sends the turn to
/// Claude at once, and the notify is not asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zero_wait_goes_to_claude_at_once() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 0);
    let silent = Arc::new(Silent::default());
    r.core.runner.verdicts.set(silent.clone());
    let res = turn(&r.core, None, PLAIN, Some("glm")).await;
    assert_eq!(res.profile, "default", "{:?}", res.route);
    assert!(silent.waits.lock().unwrap().is_empty());
    assert!(r.zai.requests().is_empty());
}

/// Test 22: a turn bound for Claude never waits, however long `wait_ms` is and
/// whatever the judge is doing: the notify is not asked, the turn is not
/// slowed, and Claude is sent it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_claude_turn_never_waits() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 60_000);
    let silent = Arc::new(Silent::default());
    r.core.runner.verdicts.set(silent.clone());
    let started = Instant::now();
    let res = turn(&r.core, None, PLAIN, None).await;
    assert!(started.elapsed() < Duration::from_secs(30), "it waited");
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    assert!(
        silent.waits.lock().unwrap().is_empty(),
        "the notify was asked"
    );
    assert_eq!(r.claude.requests().len(), 1);
    assert!(r.zai.requests().is_empty());
}

/// Test 23: a session whose `judged_through` is behind its transcript (what a
/// restart leaves, or a verdict that never came) is not cn-eligible, and
/// becomes so once the outstanding items are judged: the second turn, on a
/// judge that lands, runs on GLM, though the first did not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_behind_its_transcript_is_not_eligible_until_it_is_judged() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 100);
    let silent = Arc::new(Silent::default());
    r.core.runner.verdicts.set(silent.clone());
    let one = turn(&r.core, None, PLAIN, Some("glm")).await;
    assert_eq!(one.profile, "default", "{:?}", one.route);
    let flags: Option<ContentFlags> = crate::content_wait::flags_of(&r.core.store, &one.session_id);
    assert!(
        flags.is_none_or(|f| f.judged_through.is_none()),
        "nothing was judged"
    );
    assert!(r.zai.requests().is_empty());
    // The judge comes back: it lands the verdicts the first turn's items waited for.
    let judge = Judging::clean();
    judging_with(&r, judge.clone());
    let two = turn(
        &r.core,
        Some(&one.session_id),
        "And the tests.",
        Some("glm"),
    )
    .await;
    assert_eq!(two.profile, "glm", "{:?}", two.route);
    assert_eq!(
        r.zai.requests().len(),
        1,
        "GLM was sent the second turn alone"
    );
    let flags = crate::content_wait::flags_of(&r.core.store, &one.session_id).unwrap();
    assert!(flags.judged_through.is_some(), "the mark moved");
}

/// A label the judge finds keeps the turn off the restricted model, with the
/// flag's word on the route row and the result: the same turn that a clean
/// judge would have let through.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_found_before_the_turn_moves_it_to_claude() {
    let r = rig(None, 2, |_| {});
    judging_with(
        &r,
        Judging::with(|_, text| {
            if text.contains("Examplecorp-roadmap") {
                vec![Flag::Confidential]
            } else {
                vec![]
            }
        }),
    );
    let res = turn(
        &r.core,
        None,
        "Summarise the Examplecorp-roadmap for the board.",
        Some("glm"),
    )
    .await;
    assert_eq!(res.profile, "default", "{:?}", res.route);
    let rows = decided(&r.core.store);
    assert_eq!(rows.last().unwrap()["reason"], "data_gate", "{rows:?}");
    assert_eq!(
        rows.last().unwrap()["data_gate"],
        "confidential",
        "{rows:?}"
    );
    let data = res.route.unwrap().data.unwrap();
    assert_eq!(
        (data.cn.as_str(), data.gate.as_deref()),
        ("confidential", Some("confidential"))
    );
    assert!(r.zai.requests().is_empty());
    assert_eq!(blocked(&r.core), 0, "it never met the chokepoint");
    let flags = crate::content_wait::flags_of(&r.core.store, &res.session_id).unwrap();
    assert!(flags.has(Flag::Confidential));
}

/// A later loop re-reads the session: a tool's result is an item, its verdict
/// is waited for, and a label it raises moves the turn off the restricted model
/// to the `[model]` default with `data_gate`, instead of failing it at the
/// chokepoint. The first loop reaches `dsflash`, the second does not. (Ported
/// from the open view's later-loop test.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_loop_after_a_flagged_tool_result_leaves_the_restricted_model() {
    let jev = crate::tests_dm_rig::jev(&[]);
    let calls = vec![
        Scripted::tools(
            "Looking.",
            &[("f1", "tools_find", json!({"query": "clock"}))],
        ),
        Scripted::text("Done: from dsflash."),
    ];
    let (r, zai, _dir) = rig_with(&jev, Some(calls), |_| {});
    judging_with(
        &r,
        Judging::with(|kind, _| match kind {
            crate::content::ItemKind::ToolResult => vec![Flag::Secret],
            _ => vec![],
        }),
    );
    let sid = dm(&r.core);
    let res = say(&r.core, &sid, crate::tests_dm_rig::OPEN_A, None).await;
    assert_eq!(zai.requests().len(), 1, "the first loop alone");
    assert_eq!(res.profile, "default", "{:?}", res.route);
    let rows = decided(&r.core.store);
    assert_eq!(rows.last().unwrap()["reason"], "data_gate", "{rows:?}");
    assert_eq!(rows.last().unwrap()["data_gate"], "secret", "{rows:?}");
    let to_claude = r.claude.requests();
    let after = text(to_claude.last().unwrap());
    assert!(
        after.contains("tools_find") || after.contains("f1"),
        "{after}"
    );
    assert_eq!(blocked(&r.core), 0, "the chokepoint was never asked");
    let flags = crate::content_wait::flags_of(&r.core.store, &sid).unwrap();
    assert!(flags.has(Flag::Secret), "the label stands for the session");
}

/// Through a whole turn: a session's stored labels are the same after a plain
/// turn, which writes nothing new of them (ported from the open view's
/// `a_plain_turn_keeps_the_sessions_data_state`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plain_turn_keeps_the_sessions_labels() {
    use crate::content::{ItemKind, Label};
    let r = rig(None, 1, |_| {});
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Confidential,
        node: Some("nod_earlier".into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
    });
    flags.judged_through = Some("nod_earlier".into());
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.data = Some(Box::new(flags.clone()));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let res = turn(&r.core, Some(&rec.session_id), "Say done.", None).await;
    let after = crate::tests_route::session(&r.core, &res.session_id);
    assert_eq!(after.turns, 1, "the turn ran");
    assert_eq!(after.data.as_deref(), Some(&flags));
}

/// A turn on the live profile, run by the runner, whose failure the test
/// reads.
async fn try_turn(core: &Arc<Core>, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
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
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// With the default itself restricted there is nowhere to run: the turn fails
/// before any model call, routed or not, and both fakes record nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_usable_fallback_fails_the_turn_and_sends_nothing() {
    for routed in [true, false] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "chat", 0.95);
        // After the load's check, which refuses the owner-material keys'
        // profiles on the default's provider once it is restricted.
        let cn = |p: &mut crate::rpc::Parts| p.cfg.serve_default_from_cn();
        let r = rig_parts(routed.then_some(&jev), 1, |_| {}, cn);
        let ended = try_turn(&r.core, PLAIN).await;
        let said = format!("{ended:?}");
        assert!(
            said.contains("No model cleared for this request's data is usable"),
            "routed {routed}: {said}"
        );
        assert!(said.contains("data_gate"), "{said}");
        assert!(r.claude.requests().is_empty() && r.zai.requests().is_empty());
    }
}

/// The code's `trivial` list no longer names `glm` (theseus-a1wvq.4); GLM
/// comes in only by `cn_profiles`, and only for a session that may use a model
/// of restricted origin: with its first profile unusable, a trivial verdict
/// for a session not judged skips GLM to `cheapest`, which is Haiku's profile,
/// never GLM.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_trivial_default_skips_glm() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let r = rig_parts(
        Some(&jev),
        1,
        |c| {
            c.routing.classes.trivial = crate::config::RoutingConfig::default().classes.trivial;
            c.routing.classes.trivial.cn_profiles = vec!["glm".into()];
        },
        // After the load's check, which refuses an unknown model for the
        // owner-material keys that name `haiku`.
        |p| p.cfg.unprice_profile("haiku"),
    );
    let list = &r.core.runner.cfg.routing.classes.trivial.profiles;
    assert_eq!(list, &["haiku", "cheapest"], "the code's default");
    let res = turn(&r.core, None, "Hi there!", None).await;
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("haikuhi", "claude-haiku-5-5"),
        "{:?}",
        res.route
    );
    assert!(r.zai.requests().is_empty());
}

/// A named message runs on a Claude served from a host vouched EU: the names'
/// floor is gone (theseus-gyjyo.4), and a model of EU origin is not restricted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_request_runs_on_a_model_of_eu_origin() {
    let named = "Rename the call sites in Examplecorp's billing module.";
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 1, |c| {
        c.privacy.sensitive_names = vec!["Examplecorp".into()];
        c.serve_default_from_eu();
    });
    let origin = r
        .core
        .runner
        .cfg
        .profile_origin("default", &r.core.runner.catalog);
    assert_eq!(origin, crate::catalog::ModelOrigin::Eu, "unrestricted");
    let said = format!("{:?}", try_turn(&r.core, named).await);
    assert!(!said.contains("sensitive_names"), "{said}");
    assert_eq!(r.claude.requests().len(), 1, "{said}");
    assert!(r.zai.requests().is_empty());
}

/// A refusal is never retried into a send: the driver's rule parks a
/// `data_gate` failure on input at once, settled or not, so its one retry of a
/// lasting class (`internal` alone) never asks the model again.
#[test]
fn a_data_gate_failure_is_parked_never_retried() {
    use crate::session::{Failing, Then};
    for settled in [true, false] {
        let (_, then, _) = Failing::after(None, "data_gate", false, settled, 0, true);
        assert_eq!(then, Then::Park, "settled {settled}");
    }
}

// ----------------------------------------------- theseus-sh1n3: the two sets
//
// Every test above stands a judge behind `verdicts` that judges straight from
// the transcript, so it lands a verdict for a node **no hook site ever
// offered**. That is more generous than the running system, and it is why
// theseus-9ymz0 shipped with a gap between the set the floor blocks on and
// the set that is submitted: a node in the gap is unjudged for ever and takes
// its session off every model of restricted origin for good, in silence.
//
// These use `Judging::faithful`, which judges only what a site submitted and
// stops the mark at the first item none did — the real judge's behaviour.

/// The judge of `r`, faithful to what the hook sites actually submit.
fn faithful(r: &Rig) -> Arc<Judging> {
    let j = judging_with(r, Judging::clean());
    j.faithful(&r.core.runner.judge);
    j
}

/// A turn whose request carries a deadline, as `theseus ask --deadline` does.
async fn turn_with_deadline(
    core: &Arc<Core>,
    session: Option<&str>,
    input: &str,
    chosen: Option<&str>,
    secs: u64,
) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => crate::tests_route::session(core, id),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let arrived = Instant::now();
    crate::turn::clock_step::set_deadline(&rec.session_id, arrived, Some(secs));
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
            attachments: Vec::new(),
            arrived: Some(arrived),
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

/// **Repro 1, the turn clock** (theseus-sh1n3; found by the cell-8 bench
/// smoke, 2026-10-27). On a build carrying theseus-9ymz0 these two differed
/// only in one flag, and only the second was refused:
///
/// ```text
/// theseus ask --follow-for 0 -                 ->  the cn model answers
/// theseus ask --follow-for 0 --deadline 840 -  ->  data_gate, no model call
/// ```
///
/// A turn given a deadline appends `[The harness: N minutes left of this
/// turn's M.]`, author `harness:clock`, at its first loop. It was a user-role
/// node, so the floor counted it; nothing submits a harness-appended node, so
/// it was never judged, and the session never reached a model of restricted
/// origin again. The clock's words are the harness's own and are no item.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_does_not_take_the_session_off_the_restricted_model() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 300);
    faithful(&r);
    let with = turn_with_deadline(&r.core, None, PLAIN, Some("glm"), 840).await;
    assert_eq!(
        with.profile, "glm",
        "the same ask with --deadline must reach the same model: {:?}",
        with.route
    );
    assert_eq!(r.zai.requests().len(), 1, "GLM was sent it");
    assert_eq!(blocked(&r.core), 0, "nothing met the chokepoint");
    // And the clock's node really was written, so the test is not vacuous.
    let nodes = r.core.store.transcript(&with.session_id).unwrap();
    assert!(
        nodes
            .iter()
            .any(|(_, n)| n.author.as_deref() == Some(crate::turn::clock_step::CLOCK_AUTHOR)),
        "the turn wrote no clock line, so this proves nothing"
    );
}

/// **Repro 2, a `task.create` result** (theseus-sh1n3). Kimi's bench trial
/// failed deterministically twice: its first act was three `task.create`
/// calls, one Jev call went out for the whole trial, and the next turn was
/// refused `unjudged` with `wait_ms: 0` — it did not even wait. A harness
/// tool's result node was written by a site that queued nothing, while the
/// floor counted it like any other result.
///
/// Here the node is planted as that site wrote it — written, never offered —
/// and the floor must still let the session reach the restricted model:
/// `ask_unsubmitted` queues what the write sites did not, so a site added
/// later that forgets costs a round trip rather than a session. The site
/// itself is held to its queueing by
/// [`every_item_a_turn_writes_is_offered_for_judgment`].
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_item_no_site_submitted_is_swept_up_rather_than_wedging_the_session() {
    swept_not_wedged(300).await;
}

/// The same item, on `[content_based_security] wait_ms = 0` — a supported setting the config
/// documents as "a cn request never waits" and a test of its own asserts
/// (`config/content_based_security.rs`).
///
/// The sweep ran *inside* the settle loop at first, and that loop's first act
/// is to break on a zero wait: so on this setting the one path that makes the
/// blocked set and the submitted set equal by construction was never reached
/// at all, and the whole defect class came back on a config the operator is
/// entitled to choose (the review of 2026-10-27, B1).
///
/// What is asserted here is the **offer**, not the verdict: with a zero wait
/// this turn is meant to go to Claude without waiting, and the test judge
/// lands only when a turn waits, so no mark can move inside this test. The
/// offer is the whole of B1 — an item nobody offers is one no judge can ever
/// reach, however many turns come later.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zero_wait_still_offers_the_item_no_site_submitted() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 0);
    faithful(&r);
    let one = turn(&r.core, None, PLAIN, Some("glm")).await;
    assert_eq!(one.profile, "default", "a zero wait goes to Claude at once");
    let node = planted_result(&one.session_id);
    r.core.store.append(&[node.record().unwrap()]).unwrap();
    let two = turn(&r.core, Some(&one.session_id), "And again.", Some("glm")).await;
    assert_eq!(two.profile, "default", "still no wait, so still Claude");
    assert!(
        r.core.runner.judge.content_swept().contains(&node.id),
        "nothing offered the planted result, so nothing ever can: {:?}",
        r.core.runner.judge.content_swept()
    );
}

/// The node a harness tool's completion writes, as `harness_done` writes one
/// and as nothing queued it before this row.
fn planted_result(session: &str) -> crate::node::Node {
    crate::node::Node::tool_result(
        session,
        None,
        None,
        crate::node::Body::ToolResult {
            tool_use_id: "tu_1".into(),
            tool: crate::task::CREATE.into(),
            status: crate::node::ResultStatus::Ok,
            is_error: false,
            content: "Started task ab12".into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: serde_json::Value::Null,
            image: None,
            external: None,
        },
    )
}

async fn swept_not_wedged(wait_ms: u64) {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = wait_ms);
    faithful(&r);
    let one = turn(&r.core, None, PLAIN, Some("glm")).await;
    assert_eq!(one.profile, "glm", "{:?}", one.route);
    let node = planted_result(&one.session_id);
    r.core.store.append(&[node.record().unwrap()]).unwrap();
    let two = turn(&r.core, Some(&one.session_id), "And again.", Some("glm")).await;
    assert_eq!(
        two.profile, "glm",
        "an item nobody submitted wedged the session: {:?}",
        two.route
    );
    assert_eq!(r.zai.requests().len(), 2);
    let flags = crate::content_wait::flags_of(&r.core.store, &one.session_id).unwrap();
    let nodes = r.core.store.transcript(&one.session_id).unwrap();
    let at = |id: &str| nodes.iter().position(|(_, n)| n.id == id);
    assert!(
        at(flags.judged_through.as_deref().unwrap_or("")) >= at(&node.id),
        "the mark never reached the planted result: {:?}",
        flags.judged_through
    );
}

/// **The invariant, over a real turn** (theseus-sh1n3): every node a turn
/// writes that the floor counts as an item is a node some hook site offered
/// for judgment. Anything else is a node that can never be judged, and so a
/// session that can never again reach a model of restricted origin.
///
/// The turn here calls `task.create`, a tool the **harness** runs itself
/// (`ToolRuntime::harness_done`) — the site that wrote a result node and
/// queued nothing, which is what failed Kimi's bench trial twice. It writes
/// the operator's message, a tool call, a result and the model's reply, so
/// the walk covers the ordinary kinds too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_item_a_turn_writes_is_offered_for_judgment() {
    use crate::tests_tasks::{answers_a_call, arranged, parent_session, rig as task_rig, ASK};
    let r = task_rig(
        |req| {
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                           "arrangement": arranged()}),
                )],
            )
        },
        |_| {},
    );
    let sid = parent_session(&r.core);
    crate::test_support::turn(&r.core, &sid, ASK).await;
    let submitted = r.core.runner.judge.content_submitted();
    // Or this proves the sweep and says it proved the sites: `submitted`
    // counts what `ask_unsubmitted` offered too (the review of 2026-10-27,
    // should-fix 3). This rig routes to no restricted profile, so the floor
    // never waits and the sweep never runs — assert it, rather than rely on
    // a default nobody will remember this test depends on.
    assert!(
        r.core.runner.judge.content_swept().is_empty(),
        "the sweep ran: this test no longer holds the write sites to anything"
    );
    let nodes = r.core.store.transcript(&sid).unwrap();
    let missed: Vec<String> = nodes
        .iter()
        .filter(|(_, n)| crate::content_wait::is_item(n))
        .filter(|(_, n)| !submitted.contains(&n.id))
        .map(|(_, n)| format!("{} {:?} {:?}", n.id, n.kind, n.author))
        .collect();
    assert!(
        missed.is_empty(),
        "the floor counts these and no site submits them, so this session can \
         never reach a model of restricted origin again: {missed:#?}"
    );
    // Not vacuous: the harness tool's result really is in the transcript.
    assert!(
        nodes.iter().any(|(_, n)| matches!(&n.body,
            crate::node::Body::ToolResult { tool, .. } if tool == crate::task::CREATE)),
        "no task.create result was written"
    );
}
