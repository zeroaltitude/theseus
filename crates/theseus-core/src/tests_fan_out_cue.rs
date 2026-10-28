//! The static fan-out cue (theseus-z9mze.4; `turn/route_step/fan_out_cue.rs`): a `fan_out.cue`
//! row for every verdict of a class that may fan out, fired or not, and on a fire one tail line.
//! The rig is `tests_coding_hint`'s.

use serde_json::Value;
use theseus_judge::fake::FakeJev;

use crate::provider::ProviderRequest;
use crate::tests_injection_tiers::tiered;
use crate::tests_judge::kinds;
use crate::tests_route::{glm_placements, mode, rig_parts, turn};

/// The cue's line is the one tail block that begins with these words.
const LINE: &str = "You may fan out";

fn is_line(b: &Value) -> bool {
    b["text"].as_str().is_some_and(|t| t.starts_with(LINE))
}

fn carries(req: &ProviderRequest) -> bool {
    req.messages.iter().any(|m| {
        m["content"]
            .as_array()
            .is_some_and(|b| b.iter().any(is_line))
    })
}

const OPEN: &str = "Audit every mooring on the outer wall and say which were checked.";
const PLAIN: &str = "Which moorings on the outer wall were checked?";

fn rows(r: &crate::tests_route::Rig) -> Vec<Value> {
    kinds(&r.core.store, "fan_out.cue")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// The key test: an open-ended ask in a class that may fan out fires the cue, one row says so, and
/// the line is the request's last block with the system and tools byte for byte as an ordinary
/// turn's (it is no part of the cached prefix).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_firing_cue_writes_its_row_and_one_tail_line_and_moves_no_prefix_byte() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = tiered(&jev, 2, true, |_| {});
    let one = turn(&r.core, None, PLAIN, None).await;
    mode(&jev, "deep_coding", 0.95);
    let _ = turn(&r.core, Some(&one.session_id), OPEN, None).await;
    let q = r.claude.requests();
    let (a, b) = (&q[0], &q[1]);
    assert!(!carries(a) && carries(b));
    assert_eq!(
        serde_json::to_string(&a.system).unwrap(),
        serde_json::to_string(&b.system).unwrap()
    );
    assert_eq!(a.tools, b.tools, "the tools of the first request");
    let last = b.messages.last().unwrap()["content"]
        .as_array()
        .unwrap()
        .last()
        .cloned();
    assert!(is_line(&last.unwrap()), "in the tail");
    let got = rows(&r);
    assert_eq!(got.len(), 1, "a chat turn writes no row");
    assert_eq!(got[0]["class"], "deep_coding");
    assert_eq!(got[0]["fired"], true);
    assert_eq!(got[0]["words"], serde_json::json!(["audit", "every"]));
    assert!(got[0].get("words_rev").is_some());
}

/// A miss is logged too: the class may fan out, the ask has no open-ended word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cue_that_did_not_fire_is_logged_and_adds_no_line() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = tiered(&jev, 1, true, |_| {});
    let _ = turn(&r.core, None, PLAIN, None).await;
    assert!(!carries(&r.claude.requests()[0]));
    let got = rows(&r);
    assert_eq!(got.len(), 1);
    assert_eq!(
        (got[0]["fired"].clone(), got[0]["words"].clone()),
        (false.into(), serde_json::json!([]))
    );
}

/// The capacity it was given: with no task-class slot to spare, the words alone do not fire it, and
/// the row says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_room_for_two_helpers_holds_the_cue_back() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = tiered(&jev, 1, true, |c| c.kernel.admission_ceiling = 2);
    let _ = turn(&r.core, None, OPEN, None).await;
    assert!(!carries(&r.claude.requests()[0]));
    let got = rows(&r);
    assert_eq!(got[0]["fired"], false);
    assert_eq!(got[0]["words"], serde_json::json!(["audit", "every"]));
}

/// A routed switch rebuilds the request spec and its deferral (`route_to`), so the cue's admission
/// is made again there: a `sophisticated` ask that switches to Opus, with a cue that fired, still
/// carries the helpers family on the request that goes to Opus (theseus-z9mze.4 join fix).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_switch_after_a_fired_cue_still_carries_the_helpers_family() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig_parts(Some(&jev), 2, glm_placements, |parts| {
        parts.cfg.tools.defer.enabled = true;
    });
    let res = turn(&r.core, None, OPEN, None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    let _ = turn(&r.core, None, PLAIN, None).await;
    let q = r.claude.requests();
    assert_eq!(q[0].model, "claude-opus-5-5", "the turn switched");
    assert_eq!(rows(&r)[0]["fired"], true, "the cue fired");
    // With tool search the helpers' tools are in the request, hidden or not: `defer_loading` says.
    let hidden = |req: &ProviderRequest, tool: &str| {
        let t = req.tools.iter().find(|t| t["name"] == tool);
        t.unwrap_or_else(|| panic!("{tool} is not in the request"))["defer_loading"] == true
    };
    for tool in ["task_fan_out", "task_wait"] {
        assert!(
            hidden(&q[1], tool),
            "{tool}: an ordinary turn leaves the family hidden"
        );
        assert!(
            !hidden(&q[0], tool),
            "{tool}: the cue's admission is lost on the switch"
        );
    }
}
