//! own-benchmark on a view turn that routing moved (issue 44): routing's first compile is a probe that is
//! persisted only when the call uses it; a switch to another profile, or a verdict that settles the turn's
//! tools again on its own model, makes the call's compilation again from the same frozen view. That
//! compilation keeps its own view record, so the first answer's request is rebuilt byte for byte, and is
//! the very request the model was sent.

use std::time::Duration;

use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};

use crate::config::assembly::{Assembly, AssemblySetting};
use crate::provider::ProviderRequest;
use crate::tests_route::Rig;

/// The view as the assembly.
fn viewed(c: &mut crate::Config) {
    c.context.assembly = Some(AssemblySetting::All(Assembly::View));
}

/// The requests with tools the claude fake was sent: the turn's calls (a view place's ladder writer calls
/// without tools).
fn calls(r: &Rig) -> Vec<ProviderRequest> {
    (r.claude.requests().into_iter())
        .filter(|q| !q.tools.is_empty())
        .collect()
}

/// The first answer of `sid`'s newest turn, rebuilt: the very request the model was first sent, and the
/// compilation it names is the one with its view record.
fn rebuilt_as_sent(r: &Rig, sid: &str) {
    let turn = crate::tests_route::session(&r.core, sid)
        .last_turn_id
        .expect("a turn");
    let got = (r.core.rebuild_first(sid, &turn)).unwrap_or_else(|e| panic!("rebuildable: {e}"));
    assert_eq!(got.compilation.strategy, crate::compiler::view::STRATEGY);
    assert!(got.view.is_some(), "rebuilt from its view");
    let sent = calls(r).remove(0);
    assert_eq!(got.digest, sent.digest());
    assert_eq!(got.request, sent, "the rebuilt request is the one sent");
    let record = crate::view::record::read(&r.core.store, &got.compilation.id).unwrap();
    assert!(record.and_then(|v| v.kept).is_some(), "its own record");
}

/// A switch to another profile: the call's compilation is made again on the routed spec, and its own view
/// record lets the first answer's request be rebuilt as it was sent. The discarded probe keeps none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_view_turn_routing_moved_to_another_profile_is_rebuilt_as_sent() {
    let jev = FakeJev::start().unwrap();
    let (r, sid) = crate::tests_route::moved_to_opus_with(&jev, 4, viewed).await;
    rebuilt_as_sent(&r, &sid);
    let turn = crate::tests_route::session(&r.core, &sid)
        .last_turn_id
        .unwrap();
    let views = crate::view::record::of_turn(&r.core.store, &turn).unwrap();
    let stored = |c: &str| r.core.store.get_compilation(c).unwrap().is_some();
    assert!(
        views.iter().all(|v| stored(&v.compilation)),
        "no record of a probe that was not persisted"
    );
}

/// A verdict that lands after the tools settled and folds deep, on the session's own model: the turn drops
/// its first compile and makes the call's again at the deeper ceiling (`stays_on_its_own`). That one's
/// record rebuilds the first answer's request as it was sent. (`tests_tier_work::admitted_late`'s rig.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_view_turn_whose_tools_settled_again_is_rebuilt_as_sent() {
    let (r, sid) = settled_again(true).await;
    rebuilt_as_sent(&r, &sid);
}

/// The same turn under the transcript: rebuilt as sent (the route's own reproduction, apart from the view).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transcript_turn_whose_tools_settled_again_is_rebuilt() {
    let (r, sid) = settled_again(false).await;
    let turn = crate::tests_route::session(&r.core, &sid)
        .last_turn_id
        .expect("a turn");
    let got = (r.core.rebuild_first(&sid, &turn)).unwrap_or_else(|e| panic!("rebuildable: {e}"));
    assert_eq!(
        got.request,
        calls(&r).remove(0),
        "the rebuilt request is the one sent"
    );
}

/// A turn whose late verdict folds deep on the session's own model: its rig and session.
async fn settled_again(view: bool) -> (Rig, String) {
    let jev = FakeJev::start().unwrap();
    crate::tests_route_open::says(&jev, "sonnet", "high", 0.95);
    jev.script(
        "work",
        Jev::Choice {
            option: "coding".into(),
            confidence: 0.95,
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        4,
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
            if view {
                viewed(c);
            }
        },
        |parts| parts.cfg.tools.defer.enabled = true,
    );
    crate::tests_act_open::act_open(&r);
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
    let ask = "Check the aws logs and the aws stacks, then the projects, the threads, the \
               terminals, the channels, the reminders and the helpers.";
    let res = crate::tests_route::turn(&r.core, None, ask, None).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    let row = crate::tests_route::decided(&r.core.store).pop().unwrap();
    assert_eq!(row["late"], false, "the verdict was the turn's: {row}");
    assert_eq!(
        row["injection"]["admit_ceiling"], 3.0,
        "settled again: {row}"
    );
    // The call's compilation is the one its answer names, and it keeps its own view record.
    let turn = crate::tests_route::session(&r.core, &res.session_id)
        .last_turn_id
        .unwrap();
    if view {
        let v = crate::view::record::of_turn(&r.core.store, &turn).unwrap();
        assert!(v.iter().all(|v| r
            .core
            .store
            .get_compilation(&v.compilation)
            .unwrap()
            .is_some()));
    }
    (r, res.session_id)
}
