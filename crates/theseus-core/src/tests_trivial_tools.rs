//! A trivial detour carries `tools_find` and what its message names (theseus-d9je7). The audit
//! found the tools were 86 to 88 per cent of a greeting's request; the owner asked for the one
//! tool that can find the rest. A quick detour keeps its place's set.

use theseus_judge::fake::FakeJev;

use crate::tests_detour::rig_haiku;
use crate::tests_places::offered;
use crate::tests_route::{mode, turn};

/// A quick detour keeps its place's families (`files` and `commands` at the cli).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quick_detour_keeps_its_tools() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.92);
    let (r, haiku) = rig_haiku(&jev, 1, |_| {});
    let res = turn(&r.core, None, "Which mooring did we survey first?", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    let names = offered(&haiku.requests()[0]);
    for kept in ["fs_read", "proc_run", "tools_find"] {
        assert!(names.contains(&kept.to_string()), "{kept} in {names:?}");
    }
}
