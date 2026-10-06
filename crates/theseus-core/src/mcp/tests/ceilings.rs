//! A place's ceiling and an MCP server's tools (step 38a's `mcp:<server>`,
//! theseus-cxqj), through whole turns: the catalog (`definitions_for`) and
//! the tools note (`mcp_note`) offer a server only where the place is
//! offered it, and the gate refuses a call to the rest.

use std::time::Duration;

use theseus_mcp::fake::Mode;
use theseus_protocol::PlaceCeiling;

use super::{offered, result_of, rig, session, turn};
use crate::node::ResultStatus;
use crate::places::BoundPlace;

const LAB: u64 = 314_159_265_358_979_323;
const DEN: u64 = 141_421_356_237_309_504;

fn ceiling(tools: &[&str]) -> Option<PlaceCeiling> {
    Some(PlaceCeiling {
        tools: Some(tools.iter().map(|t| t.to_string()).collect()),
        ..Default::default()
    })
}

fn system_of(req: &crate::provider::ProviderRequest) -> String {
    req.system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `#lab`'s ceiling lists `mcp:fake`, `#den`'s lists `web`, both private:
/// `#lab` is offered the fake's five tools alone, its tools note has the MCP
/// line, and its call runs; `#den` is offered `web_search` alone, has no MCP
/// line, and its call is refused in words naming its ceiling, never reaching
/// the server.
#[tokio::test]
async fn a_ceilings_mcp_entry_decides_which_places_are_offered_the_servers_tools() {
    let r = rig(Mode::Ok);
    r.core.mcp.start();
    let s = r.core.mcp.server("fake").unwrap().clone();
    assert!(s.client(Duration::from_secs(10)).await.is_ok());
    r.core.bind_places(vec![
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
            guild: Some("100000000000000001".into()),
            ceiling: ceiling(&["mcp:fake"]),
        },
        BoundPlace {
            target: format!("discord:channel:{DEN}"),
            name: "#den".into(),
            private: true,
            guild: Some("100000000000000001".into()),
            ceiling: ceiling(&["web"]),
        },
    ]);
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &lab, "echo in the lab").await;
    let den = session(&r.core, Some(&format!("channel:{DEN}")));
    turn(&r.core, &den, "echo in the den").await;
    let reqs = r.model.requests.lock().unwrap().clone();
    // Requests, in order: the lab's two loops, then the den's two.
    let (lab_req, den_req) = (&reqs[0], &reqs[2]);

    let mut names = offered(lab_req);
    names.sort();
    assert_eq!(
        names,
        [
            "mcp__fake__add",
            "mcp__fake__echo",
            "mcp__fake__fail",
            "mcp__fake__image",
            "mcp__fake__sleep"
        ],
        "#lab is offered the fake's tools alone"
    );
    assert!(
        system_of(lab_req).contains("Tools named mcp__<server>__<tool> come from the MCP servers"),
        "#lab's tools note has the MCP line"
    );
    let (status, text, _) = result_of(&r.core, &lab, "m1");
    assert_eq!(status, ResultStatus::Ok, "{text}");

    assert_eq!(offered(den_req), ["web_search"], "#den is offered web only");
    assert!(
        !system_of(den_req).contains("Tools named mcp__"),
        "#den's tools note has no MCP line"
    );
    let (status, text, _) = result_of(&r.core, &den, "m1");
    assert_ne!(status, ResultStatus::Ok);
    assert!(
        text.starts_with(
            "Not run: mcp:fake/echo is not offered in #den: its ceiling in the bindings file offers only web"
        ),
        "{text}"
    );
    assert_eq!(s.status().calls, 1, "only #lab's call reached the server");
    r.core.mcp.stop();
}
