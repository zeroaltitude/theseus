//! The narrow-read hint and window for `deep_coding` turns (theseus-tdkcn;
//! `turn::tier`): an acting `deep_coding` verdict puts one harness block in
//! the request's tail and narrows a bare `fs.read`; no other turn gets either,
//! and no cached prefix byte moves. The rig is `tests_injection_tiers`'.

use theseus_judge::fake::{FakeJev, FakeMode};

use crate::provider::ProviderRequest;
use crate::tests_injection_tiers::tiered;
use crate::tests_route::{mode, turn};
use crate::toolrun::session_dir::read_window::{self};
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
