//! The data rule and the route row's where-it-ran words, through route.v10 (theseus-h66v4.11).
//! route.v8's tests reached a restricted-origin model through a class's `cn_profiles`
//! (`trivial.cn_profiles = [dsflash]`, the DM rig) and a session's routed base through a class
//! walk; the code they tested did not go: the content floor (`content_wait`), `TurnData`'s
//! `cn`/`gate`/`origin`/`footer`, `fact/route.rs`'s provider, model and origin, and
//! `provider.call`'s origin. Here Jev picks `deepseek_flash` (a China-origin model on the
//! `deepseek` fake) for a session the data rule may allow it, or keeps off it, with route.v10's
//! shape acting (`tests_act_open`) and `tests_dm_rig`'s judge stand-ins deciding the
//! labels.
//!
//! - a judged-clean session runs on the restricted model it is pinned to, or kept on, and nothing
//!   is refused;
//! - a later loop after a flagged tool result leaves the restricted model, with `data_gate`;
//! - `provider.call` records its origin, per request.
//!
//! Two of the seventeen are not restored, each covered at the flipped tree (theseus-h66v4.11
//! proved it by a plant that goes red in a survivor with the test removed):
//! `route_decided_records_cn_gate_provider_model_origin` (a route row that loses `origin` goes red
//! in `tests_observability::a_plain_turns_route_row_gains_only_where_it_ran` and
//! `tests_route_open::a_flash_pick_runs_on_deepseek_when_the_session_may_use_it`) and
//! `the_turns_footer_words_only_the_gate` (a footer that never words the gate goes red in
//! `TurnData::footer`'s own test, the status line's, and discord's two).

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_protocol::SessionKind;

use crate::content::{Flag, ItemKind};
use crate::content_wait::judging::Judging;
use crate::provider::{FakeProvider, Scripted};
use crate::routing::Routed;
use crate::session::SessionRecord;
use crate::tests_act_open::act_open;
use crate::tests_dm_rig::{blocked, judging_with, OPEN_A};
use crate::tests_judge::kinds;
use crate::tests_route::{call, decided, restricted_placements, rig, rig_parts, turn, Rig};
use crate::tests_route_open::says;

const PLAIN: &str = "Rename the call sites in the billing module.";

/// A rig whose `deepseek` fake answers `script`, Jev picking Flash at low effort for every
/// message, route.v10's shape acting.
fn flash_rig(jev: &FakeJev, script: Vec<Scripted>) -> (Rig, Arc<FakeProvider>) {
    says(jev, "deepseek_flash", "low", 0.97);
    let deepseek = Arc::new(FakeProvider::scripted(script));
    let ds = deepseek.clone();
    let r = rig_parts(
        Some(jev),
        3,
        |c| c.serve_deepseek_flash(),
        move |p| {
            p.providers.insert("deepseek".into(), ds);
        },
    );
    act_open(&r);
    (r, deepseek)
}

fn calls(core: &crate::Core) -> Vec<Value> {
    kinds(&core.store, "provider.call")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// How a turn comes to be bound for GLM, a restricted model, with the session's labels clean.
#[derive(Debug, Clone, Copy)]
enum Case {
    NoVerdict,
    Pin,
    RoutedBase,
    RoutingOff,
}

/// One case's turn, on a fresh rig, its judge `judge`: the rig and the result.
async fn bound_for_glm(
    case: Case,
    judge: Arc<Judging>,
) -> (Rig, theseus_protocol::TurnSubmitResult) {
    let jev = FakeJev::start().unwrap();
    // A sure pick would move the session off GLM, so a case that asks the route is asked a pick
    // under the confirm bar, which keeps the session where it is; the pin asks none.
    says(&jev, "sonnet", "high", 0.2);
    let r = match case {
        Case::RoutingOff => rig(None, 2, restricted_placements),
        _ => rig(Some(&jev), 2, restricted_placements),
    };
    judging_with(&r, judge);
    if matches!(case, Case::NoVerdict) {
        jev.set_mode(FakeMode::Down);
    }
    let on_glm = !matches!(case, Case::Pin | Case::RoutedBase);
    if on_glm {
        call(&r.core, "profile.use", json!({"name": "glm"})).await;
    }
    let res = match case {
        Case::Pin => turn(&r.core, None, PLAIN, Some("glm")).await,
        Case::RoutedBase => {
            let mut rec = SessionRecord::new(SessionKind::Conversation, None);
            rec.routed = Some(Box::new(Routed {
                profile: Some("glm".into()),
                from: Some("sonnet".into()),
                ..Default::default()
            }));
            r.core.store.put_session(&rec.session_id, &rec).unwrap();
            turn(&r.core, Some(&rec.session_id), PLAIN, None).await
        }
        _ => turn(&r.core, None, PLAIN, None).await,
    };
    jev.release();
    (r, res)
}

/// A session whose judge finds nothing carries no label and its items are judged, so a turn
/// pinned to GLM, one on a session's routed base, one with no verdict and one with routing off
/// all run on GLM itself, and the chokepoint sends them. **This is the case the open view could
/// not pass**: a restricted model, a session with nothing found in it, and the turn on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_judged_clean_session_runs_on_the_restricted_model() {
    for case in [
        Case::Pin,
        Case::RoutedBase,
        Case::RoutingOff,
        Case::NoVerdict,
    ] {
        let judge = Judging::clean();
        let (r, res) = bound_for_glm(case, judge.clone()).await;
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

/// A later loop re-reads the session: a tool's result is an item, its verdict is waited for, and
/// a label it raises moves the turn off the restricted model to the `[model]` default with
/// `data_gate`, instead of failing it at the chokepoint. The first loop reaches `dsflash`, the
/// second does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_loop_after_a_flagged_tool_result_leaves_the_restricted_model() {
    let jev = FakeJev::start().unwrap();
    let calls = vec![
        Scripted::tools(
            "Looking.",
            &[("f1", "tools_find", json!({"query": "clock"}))],
        ),
        Scripted::text("Done: from dsflash."),
    ];
    let (r, deepseek) = flash_rig(&jev, calls);
    judging_with(
        &r,
        Judging::with(|kind, _| match kind {
            ItemKind::ToolResult => vec![Flag::Secret],
            _ => vec![],
        }),
    );
    let res = turn(&r.core, None, OPEN_A, None).await;
    assert_eq!(deepseek.requests().len(), 1, "the first loop alone");
    assert_eq!(res.profile, "default", "{:?}", res.route);
    let rows = decided(&r.core.store);
    assert_eq!(rows.last().unwrap()["reason"], "data_gate", "{rows:?}");
    assert_eq!(rows.last().unwrap()["data_gate"], "secret", "{rows:?}");
    let to_claude = r.claude.requests();
    let after = crate::tests_dm_rig::text(to_claude.last().unwrap());
    assert!(
        after.contains("tools_find") || after.contains("f1"),
        "{after}"
    );
    assert_eq!(blocked(&r.core), 0, "the chokepoint was never asked");
    let flags = crate::content_wait::flags_of(&r.core.store, &res.session_id).unwrap();
    assert!(flags.has(Flag::Secret), "the label stands for the session");
}

/// A request to Flash says it went to a `cn` model; a Claude call says its origin alone; and the
/// loop after a flagged tool result, moved off the restricted model, names its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_call_records_origin_per_request() {
    let jev = FakeJev::start().unwrap();
    let script = vec![
        Scripted::tools(
            "Looking.",
            &[("f1", "tools_find", json!({"query": "clock"}))],
        ),
        Scripted::text("Done."),
    ];
    let (r, _deepseek) = flash_rig(&jev, script);
    judging_with(
        &r,
        Judging::with(|kind, _| match kind {
            ItemKind::ToolResult => vec![Flag::Secret],
            _ => vec![],
        }),
    );
    turn(&r.core, None, OPEN_A, None).await;
    let rows = calls(&r.core);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let (first, moved) = (&rows[0], &rows[1]);
    assert_eq!(first["origin"], "cn", "{first}");
    assert_eq!(moved["origin"], "us", "{moved}");
    assert!(
        first.get("pass").is_none() && moved.get("pass").is_none(),
        "no pass is minted any more: {first} {moved}"
    );
}
