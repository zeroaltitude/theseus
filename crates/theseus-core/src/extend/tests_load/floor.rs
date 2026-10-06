//! An extension's load floor through the gate (theseus-sh9w): `Floors`' own
//! test calls `Floors::floor`, so this runs turns, and fails if the line
//! after the place's floor in `toolrun::order` is dropped.

use serde_json::json;
use theseus_protocol::extend::ExtensionRevokeParams;
use theseus_protocol::PlaceCeiling;

use super::{call, rig_with, text, Rig, LAB, TOOL, WC};
use crate::node::ResultStatus;
use crate::places::BoundPlace;
use crate::policy::Posture;

/// Another private place's guild channel, with no ceiling.
const DEN: u64 = 4_100_000_000_000_000_888;

fn places(pier_floor: Option<&str>) -> Vec<BoundPlace> {
    let bound = |id: u64, name: &str, floor: Option<&str>| BoundPlace {
        target: format!("discord:channel:{id}"),
        name: name.into(),
        private: true,
        guild: Some("100000000000000001".into()),
        ceiling: floor.map(|f| PlaceCeiling {
            posture_floor: Some(f.into()),
            ..Default::default()
        }),
    };
    vec![bound(LAB, "#pier", pier_floor), bound(DEN, "#den", None)]
}

/// The cards the core has waiting, with their reasons.
fn waiting(r: &Rig) -> Vec<String> {
    r.rows("tool.confirm_requested")
        .iter()
        .map(|c| c["reason"].as_str().unwrap_or_default().to_string())
        .filter(|why| why.starts_with("mcp:ext-wordcount/"))
        .collect()
}

/// An extension acked from `#pier`, whose ceiling floors at `approve`, runs
/// no looser than that anywhere, though `[policy.mcp]` says open and the
/// call comes from another private place: it waits, and the reason names the
/// place it was loaded under. Revoked and loaded again with no ceiling, the
/// same call runs.
#[tokio::test]
async fn an_extension_loaded_under_a_floor_waits_in_another_place_until_loaded_without_one() {
    let r = rig_with(|c| {
        c.policy.mcp.insert("ext-wordcount".into(), Posture::Open);
    });
    r.core.mcp.start();
    // Proposed from #pier (its own call is no part of this), and acked once
    // its ceiling floors at approve: the load records the ceiling it is
    // acked under.
    r.core.bind_places(places(None));
    let pier = r.session(Some(&format!("channel:{LAB}")));
    r.script(vec![
        call(
            "extend_propose",
            super::proposal(&r.server("tools/wc", "ok", WC)),
        ),
        text(),
    ]);
    r.turn(&pier, "propose the counter").await;
    let q = r.core.confirm_list().unwrap();
    assert_eq!(q.len(), 1, "{q:?}");
    r.core.bind_places(places(Some("approve")));
    r.ack(&q[0].correlation_id);
    r.until_ready().await;

    let den = r.session(Some(&format!("channel:{DEN}")));
    r.script(vec![call(TOOL, json!({"text": "two words"})), text()]);
    r.turn(&den, "count them").await;
    let asked = waiting(&r);
    assert_eq!(asked.len(), 1, "the call waits for approval: {asked:?}");
    assert!(
        asked[0].contains("approve") && asked[0].contains("#pier"),
        "its reason names the loading place's floor: {}",
        asked[0]
    );
    assert!(
        r.core
            .store
            .session_nodes(&den)
            .unwrap()
            .iter()
            .all(|(_, n)| !matches!(&n.body, crate::node::Body::ToolResult { tool_use_id, .. } if tool_use_id == "c1")),
        "nothing ran before the answer"
    );

    // Declined, revoked, and loaded again with no ceiling: it runs. The
    // waiting turn never made its last model call.
    r.model.steps.lock().unwrap().clear();
    for c in r.core.confirm_list().unwrap() {
        r.core
            .confirm_action(&c.correlation_id, false, None, "cli")
            .unwrap();
    }
    let p = ExtensionRevokeParams {
        name: "wordcount".into(),
        ..Default::default()
    };
    r.core.extension_revoke(&p, "cli").unwrap();
    r.core.bind_places(places(None));
    let q = r
        .propose(&r.server("tools/wc2", "ok", "# another counter\n"))
        .await;
    r.ack(&q);
    r.until_ready().await;
    let den = r.session(Some(&format!("channel:{DEN}")));
    r.script(vec![call(TOOL, json!({"text": "two words"})), text()]);
    r.turn(&den, "count them again").await;
    let (status, said) = r.result(&den);
    assert_eq!(status, ResultStatus::Ok, "{said}");
    assert_eq!(waiting(&r).len(), 1, "no second card: {:?}", waiting(&r));
}
