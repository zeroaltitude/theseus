//! The narrow-read hint and window for `deep_coding` turns (theseus-tdkcn;
//! `turn::tier`): an acting `deep_coding` verdict puts one harness block in
//! the request's tail and narrows a bare `fs.read`; no other turn gets either,
//! and no cached prefix byte moves. The rig is `tests_injection_tiers`'.

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode};

use crate::provider::ProviderRequest;
use crate::tests_injection_tiers::tiered;
use crate::tests_route::{mode, turn};
use crate::toolrun::session_dir::read_window::{self, NARROW_LINES};
use crate::turn::tier::CODING_HINT;

/// Whether any message of `req` carries the hint as a block of its own.
fn carries(req: &ProviderRequest) -> bool {
    req.messages.iter().any(|m| {
        m["content"]
            .as_array()
            .is_some_and(|b| b.iter().any(|b| b["text"] == CODING_HINT))
    })
}

const ASK: &str = "Which moorings on the outer wall were checked?";

/// The key test: an ordinary turn's request has no hint, a `deep_coding`
/// turn's carries it in its tail, and the system and tools of the two are
/// byte for byte the same (the hint is no part of the cached prefix).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_a_deep_coding_turn_carries_the_hint_and_no_prefix_byte_moves() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = tiered(&jev, 2, true, |_| {});
    let one = turn(&r.core, None, ASK, None).await;
    assert_eq!(read_window::get(&one.session_id), 0, "an ordinary turn");
    mode(&jev, "deep_coding", 0.95);
    let two = turn(&r.core, Some(&one.session_id), "And the chains?", None).await;
    assert_eq!(
        read_window::get(&two.session_id),
        NARROW_LINES,
        "a deep_coding turn"
    );
    let q = r.claude.requests();
    let (a, b) = (&q[0], &q[1]);
    assert!(!carries(a), "an ordinary turn has no hint");
    assert!(carries(b), "a deep_coding turn's tail has the hint");
    assert_eq!(
        serde_json::to_string(&a.system).unwrap(),
        serde_json::to_string(&b.system).unwrap(),
        "the system"
    );
    assert_eq!(a.tools, b.tools, "the tools");
    assert_eq!(a.messages[0], b.messages[0], "the first message");
    let last = b.messages.last().unwrap()["content"]
        .as_array()
        .unwrap()
        .last()
        .cloned();
    assert_eq!(
        last.unwrap()["text"],
        Value::from(CODING_HINT),
        "in the tail"
    );
    assert!(!serde_json::to_string(&b.system)
        .unwrap()
        .contains("slices of 100-300"));
}

/// A verdict that has not landed leaves the turn ordinary: no hint, no window.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_gets_no_hint() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = tiered(&jev, 1, false, |c| {
        c.routing.max_wait_ms = 200;
        c.judge.total_secs = 30;
    });
    let late = jev.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        late.release();
    });
    let res = turn(&r.core, None, ASK, None).await;
    assert!(!carries(&r.claude.requests()[0]));
    assert_eq!(read_window::get(&res.session_id), 0);
}
