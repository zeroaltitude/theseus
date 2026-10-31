//! A routed session's base (theseus-0j2.17): the profile routing first moved
//! it from (`Routed.from`). Once `route.v1` stops acting, the session runs
//! there, so a pane's `-P` profile comes back; a turn whose base changed (the
//! live profile, a place's) clears the move; the pane's carried routed profile
//! is never a new base. A second switch keeps the first base, and a switch
//! back to it ends the move (theseus-zvpl). The rig is `tests_route`'s.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::routing::Routed;
use crate::session::SessionRecord;
use crate::tests_judge::board;
use crate::tests_route::{call, mode, rig, rig_on, session, until_route_rows, Rig};

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

/// A routed record written before `from` (format 15 to 20) reads as before:
/// its turn's base is taken as the one it was moved from, so while routing
/// acts it stays moved, and once it does not, the turn runs on that base.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_record_without_its_base_reads_as_before() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = rig(Some(&jev), 3, |c| {
        c.routing.max_wait_ms = 5_000;
        crate::tests_route::glm_placements(c);
    });
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

// ------------------------------------------- a second switch, and a switch back (theseus-zvpl)
