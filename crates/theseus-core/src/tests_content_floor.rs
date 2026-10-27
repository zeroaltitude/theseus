//! The turn's floor on content flags (theseus-9ymz0, design §4.2b, §4.3 and
//! §11's tests 21 to 23): a turn bound for a model of restricted origin runs
//! on it only when its session's labels allow it and every item was judged; it
//! waits for the verdicts, at most `[content] wait_ms`, then goes to the
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
/// reached waits for its verdict, at most `[content] wait_ms`, then runs on the
/// `[model]` default with `Reason::DataGate`: it **completes**. Nothing goes to
/// the restricted model, and nothing is refused at the chokepoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cn_turn_waits_for_its_verdict_then_goes_to_claude_and_completes() {
    let r = rig(None, 2, |c| c.content.wait_ms = 300);
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

/// `[content] wait_ms = 0` never waits: the unjudged item sends the turn to
/// Claude at once, and the notify is not asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zero_wait_goes_to_claude_at_once() {
    let r = rig(None, 2, |c| c.content.wait_ms = 0);
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
    let r = rig(None, 2, |c| c.content.wait_ms = 60_000);
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
    let r = rig(None, 2, |c| c.content.wait_ms = 100);
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

/// The names' floor holds beside the new one: a named message never runs on a
/// Claude served from a host vouched EU, and no label is needed for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_request_still_needs_a_us_model() {
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
    assert!(said.contains("sensitive_names"), "{said}");
    assert!(r.claude.requests().is_empty() && r.zai.requests().is_empty());
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
