//! A place's profile is its cap (M5 25e, theseus-g1gl): routing never
//! climbs above it. A hard question in a place bound with `sonnet` stays on
//! `sonnet`, its row and result saying `capped`. The rig is `tests_route`'s.

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_protocol::{PlaceCeiling, SessionKind, TurnSubmitResult};

use crate::places::BoundPlace;
use crate::session::SessionRecord;
use crate::tests_route::{call, decided, mode, rig, session, until_route_rows};

const BERTH: u64 = 161_803_398_874_989_484;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hard_question_in_a_capped_place_keeps_the_places_profile() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |c| c.routing.max_wait_ms = 5_000);
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{BERTH}"),
        name: "#berth".into(),
        private: true,
        guild: Some("100000000000000004".into()),
        ceiling: Some(PlaceCeiling {
            profile: Some("sonnet".into()),
            ..Default::default()
        }),
    }]);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core
        .outbox
        .bind_place(&format!("channel:{BERTH}"), &rec.session_id)
        .unwrap();
    let sid = rec.session_id;
    let ask =
        json!({"session_id": sid, "input": "Weigh two designs for a crash-safe write-ahead log."});
    let res: TurnSubmitResult =
        serde_json::from_value(call(&r.core, "turn.submit", ask).await).unwrap();
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("sonnet", "claude-sonnet-5-5")
    );
    assert_eq!(res.route.unwrap().reason, "capped");
    assert_eq!(r.claude.requests()[0].model, "claude-sonnet-5-5");
    let rows = decided(&r.core.store);
    assert_eq!(
        (rows[0]["reason"].as_str(), rows[0]["profile"].as_str()),
        (Some("capped"), Some("sonnet"))
    );
    assert!(session(&r.core, &sid).routed.is_none());
    until_route_rows(&r.core.store, 1).await;
}
