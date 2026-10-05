//! A routed session's base (theseus-0j2.17): the profile routing first moved
//! it from (`Routed.from`). Once `route.v1` stops acting, the session runs
//! there, so a pane's `-P` profile comes back; a turn whose base changed (the
//! live profile, a place's) clears the move; the pane's carried routed profile
//! is never a new base. The rig is `tests_route`'s.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::{PlaceCeiling, SessionKind, TurnSubmitResult};

use crate::places::BoundPlace;
use crate::routing::Routed;
use crate::session::SessionRecord;
use crate::tests_judge::board;
use crate::tests_route::{
    call, keyless, mode, moved_to_opus, rig, rig_on, session, turn, until_route_rows, Rig,
};
use crate::Config;

async fn submit(r: &Rig, params: Value) -> TurnSubmitResult {
    serde_json::from_value(call(&r.core, "turn.submit", params).await).unwrap()
}

/// The pane's message, carrying `profile` as the CLI's pane does.
fn pane(sid: &str, input: &str, profile: &str) -> Value {
    json!({"session_id": sid, "input": input, "profile": profile, "carried": true})
}

fn routed(r: &Rig, sid: &str) -> Option<Routed> {
    session(&r.core, sid).routed.map(|b| *b)
}

fn moved(profile: &str, from: &str) -> Option<Routed> {
    Some(Routed {
        profile: Some(profile.into()),
        from: Some(from.into()),
        hold: None,
    })
}

/// A pane started with `theseus ask -P glm`, then a hard question in it,
/// which routing moves to Opus: the move keeps the base, `glm`; and while
/// routing acts, the pane's carried `opus` keeps it moved.
async fn pane_moved_to_opus(jev: &FakeJev) -> (Rig, String) {
    mode(jev, "chat", 0.95);
    let r = rig(Some(jev), 3, |c| c.routing.max_wait_ms = 5_000);
    let one = submit(
        &r,
        json!({"input": "Name the tide tables.", "profile": "glm"}),
    )
    .await;
    assert_eq!(
        (one.profile.as_str(), one.route.unwrap().reason.as_str()),
        ("glm", "pinned")
    );
    let sid = one.session_id;
    mode(jev, "sophisticated", 0.95);
    let ask = "Weigh two designs for a crash-safe write-ahead log.";
    let two = submit(&r, pane(&sid, ask, "glm")).await;
    assert_eq!(two.profile, "opus");
    assert_eq!(routed(&r, &sid), moved("opus", "glm"));
    mode(jev, "chat", 0.95);
    let three = submit(&r, pane(&sid, "What is a frame?", "opus")).await;
    assert_eq!(
        three.profile, "opus",
        "the carried routed profile is no new base"
    );
    assert_eq!(routed(&r, &sid), moved("opus", "glm"));
    until_route_rows(&r.core.store, 3).await;
    (r, sid)
}

/// Routing off, in shadow, the judge off, and Jev's key gone, each after a
/// restart onto it, and the ladder's rollback: the pane's next message runs
/// on its `-P` profile, not the live one, and the move is cleared.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_panes_p_profile_comes_back_once_routing_stops() {
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
    let check = |r: &Rig, sid: &str, got: &TurnSubmitResult, what: &str| {
        assert_eq!(
            (got.profile.as_str(), got.model.as_str()),
            ("glm", "glm-5.3-flash"),
            "{what}"
        );
        assert_eq!(
            r.zai.requests().last().unwrap().model,
            "glm-5.3-flash",
            "{what}"
        );
        assert!(r.claude.requests().is_empty() || what == "the ladder's rollback");
        assert_eq!(routed(r, sid), None, "{what}");
        assert_eq!(session(&r.core, sid).last_target.unwrap().profile, "glm");
    };
    for (what, tweak, key) in cases {
        let (r, sid) = pane_moved_to_opus(&jev).await;
        let dir = r.dir.clone();
        drop(r);
        let secrets = if key { board() } else { keyless() };
        let r = rig_on(dir, Some(&jev), 1, tweak, secrets);
        let next = submit(&r, pane(&sid, "And its recovery path?", "opus")).await;
        check(&r, &sid, &next, what);
    }
    let (r, sid) = pane_moved_to_opus(&jev).await;
    r.core
        .pack_rollback(
            &theseus_protocol::packs::PackRollbackParams {
                pack: "route.v1".into(),
                why: None,
                off: false,
            },
            "cli",
        )
        .unwrap();
    let next = submit(&r, pane(&sid, "And its recovery path?", "opus")).await;
    check(&r, &sid, &next, "the ladder's rollback");
    assert_eq!(next.route.unwrap().reason, "shadow");
}

/// A changed `[model] live`, after a restart: a session that follows the
/// live profile leaves routing's move and runs on the new one; the pane, its
/// base its `-P` profile, keeps its move while routing acts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_changed_live_profile_clears_the_move_of_a_session_that_follows_it() {
    let jev = FakeJev::start().unwrap();
    let (r, follows) = moved_to_opus(&jev, 3).await;
    assert_eq!(routed(&r, &follows), moved("opus", "sonnet"));
    let dir = r.dir.clone();
    drop(r);
    // The pane, on the same store.
    let (r, pane_sid) = {
        let r = rig_on(
            dir.clone(),
            Some(&jev),
            3,
            |c| c.routing.max_wait_ms = 5_000,
            board(),
        );
        mode(&jev, "chat", 0.95);
        let one = submit(
            &r,
            json!({"input": "Name the tide tables.", "profile": "glm"}),
        )
        .await;
        mode(&jev, "sophisticated", 0.95);
        let ask = "Weigh two designs for a crash-safe write-ahead log.";
        let two = submit(&r, pane(&one.session_id, ask, "glm")).await;
        assert_eq!(two.profile, "opus");
        (r, one.session_id)
    };
    until_route_rows(&r.core.store, 3).await;
    drop(r);
    let r = rig_on(
        dir,
        Some(&jev),
        3,
        |c| {
            c.routing.max_wait_ms = 5_000;
            c.model.live = "fable".into();
        },
        board(),
    );
    mode(&jev, "chat", 0.95);
    let next = turn(&r.core, Some(&follows), "What is a frame?", None).await;
    assert_eq!(
        (next.profile.as_str(), next.model.as_str()),
        ("fable", "claude-fable-5-1")
    );
    assert_eq!(r.claude.requests()[0].model, "claude-fable-5-1");
    assert_eq!(routed(&r, &follows), None);
    let p = submit(&r, pane(&pane_sid, "What is a segment?", "opus")).await;
    assert_eq!(p.profile, "opus", "the pane's base, glm, did not change");
    assert_eq!(routed(&r, &pane_sid), moved("opus", "glm"));
}

/// A place's bound profile changed under a session routing moved: its next
/// message leaves the move and runs on the place's profile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_changed_place_profile_clears_the_move() {
    const QUAY: u64 = 271_828_182_845_904_523;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 3, |c| c.routing.max_wait_ms = 5_000);
    let bind = |ceiling: Option<PlaceCeiling>| {
        r.core.bind_places(vec![BoundPlace {
            target: format!("discord:channel:{QUAY}"),
            name: "#quay".into(),
            private: true,
            guild: Some("100000000000000003".into()),
            ceiling,
        }])
    };
    bind(None);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core
        .outbox
        .bind_place(&format!("channel:{QUAY}"), &rec.session_id)
        .unwrap();
    let sid = rec.session_id;
    let ask = |input: &str| json!({"session_id": sid, "input": input});
    let one = submit(
        &r,
        ask("Weigh two designs for a crash-safe write-ahead log."),
    )
    .await;
    assert_eq!(one.profile, "opus");
    assert_eq!(routed(&r, &sid), moved("opus", "sonnet"));
    bind(Some(PlaceCeiling {
        profile: Some("glm53".into()),
        ..Default::default()
    }));
    mode(&jev, "chat", 0.95);
    let two = submit(&r, ask("What is a frame?")).await;
    assert_eq!(
        (two.profile.as_str(), two.model.as_str()),
        ("glm53", "glm-5.3")
    );
    assert_eq!(r.zai.requests()[0].model, "glm-5.3");
    assert_eq!(routed(&r, &sid), None);
}

/// A routed record written before `from` (format 15 to 20) reads as before:
/// its turn's base is taken as the one it was moved from, so while routing
/// acts it stays moved, and once it does not, the turn runs on that base.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_record_without_its_base_reads_as_before() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 3, |c| c.routing.max_wait_ms = 5_000);
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.routed = Some(Box::new(Routed {
        profile: Some("opus".into()),
        ..Default::default()
    }));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id;
    let one = submit(&r, pane(&sid, "What is a frame?", "opus")).await;
    assert_eq!(one.profile, "opus");
    assert_eq!(routed(&r, &sid), moved("opus", "sonnet"));
    until_route_rows(&r.core.store, 1).await;
    let dir: Arc<tempfile::TempDir> = r.dir.clone();
    drop(r);
    let r = rig_on(dir, Some(&jev), 1, |c| c.routing.enabled = false, board());
    let two = submit(&r, pane(&sid, "And a segment?", "opus")).await;
    assert_eq!(two.profile, "sonnet");
    assert_eq!(routed(&r, &sid), None);
}
