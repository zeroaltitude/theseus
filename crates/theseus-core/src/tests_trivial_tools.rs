//! A pick on a model without tool search keeps its place's set of tools, and `tools_find`
//! beside it (theseus-d9je7).

use theseus_judge::fake::FakeJev;

use crate::tests_detour::rig_haiku;
use crate::tests_places::offered;
use crate::tests_route::{mode, turn};

/// A routed pick keeps its place's families (`files` and `commands` at the cli).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_pick_keeps_its_places_tools() {
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
