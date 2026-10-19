//! What a plain turn does with the tool catalog before its request leaves (theseus-nsp9l.2). The catalog is
//! 100 KB of JSON: made again each turn, cloned into each spec, serialized and hashed for each manifest, it cost a
//! person ~25 ms of the wait for the model's first byte. These count the work, not the time: a turn after the
//! first makes no catalog and walks no tool JSON, and a catalog is made again when what it is made from changes.

use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::ceiling::PlaceView;
use crate::places::PlaceClass;
use crate::provider::Scripted;
use crate::secrets::SecretBoard;
use crate::tests_places::{rig_deferring, session, Rig};
use crate::tool_set;
use crate::toolrun::{build_runtime, InlineLauncher, ToolRuntime};
use crate::{Config, Core};

fn rig() -> Rig {
    rig_deferring(|_| {}, |_| Scripted::text("Hello."))
}

async fn say(core: &Arc<Core>, sid: &str, input: &str) {
    crate::test_support::turn(core, sid, input).await;
}

#[tokio::test]
async fn a_plain_turn_after_the_first_makes_no_tool_catalog_and_walks_no_tool_json() {
    let r = rig();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "warm up").await;
    let made = r.core.runner.tools.catalogs.built();
    assert!(made >= 1, "the first turn made the catalog");
    let walked = tool_set::walks();
    say(&r.core, &sid, "hi").await;
    assert_eq!(
        r.core.runner.tools.catalogs.built(),
        made,
        "the second turn made a catalog again"
    );
    assert_eq!(
        tool_set::walks() - walked,
        0,
        "the second turn walked the tools' JSON"
    );
    let reqs = r.requests();
    assert_eq!(
        serde_json::to_string(&reqs[0].tools).unwrap(),
        serde_json::to_string(&reqs[1].tools).unwrap(),
        "the requests' tools are byte for byte the same"
    );
}

fn runtime(cfg: &Config) -> ToolRuntime {
    let board = SecretBoard::new(cfg.secrets.keys().cloned(), Instant::now());
    build_runtime(cfg, None, Arc::default(), Arc::new(InlineLauncher), board).unwrap()
}

fn view(class: PlaceClass) -> PlaceView {
    class.into()
}

fn names(defs: &[Value]) -> Vec<&str> {
    defs.iter().filter_map(|d| d["name"].as_str()).collect()
}

#[test]
fn a_catalog_is_kept_per_place_and_made_again_when_what_it_is_made_from_changes() {
    let mut rt = runtime(&Config::example());
    let private = rt.definitions_for(view(PlaceClass::Private));
    let again = rt.definitions_for(view(PlaceClass::Private));
    assert!(
        std::ptr::eq(private.as_ptr(), again.as_ptr()),
        "one catalog, shared"
    );
    assert_eq!(rt.catalogs.built(), 1);
    // Another class is offered other tools: its own catalog.
    let shared = rt.definitions_for(view(PlaceClass::Shared));
    assert_eq!(rt.catalogs.built(), 2);
    assert!(names(&shared).len() < names(&private).len());
    // The wait cap is in the job tools' words: a change of it is a new catalog.
    rt.job_wait_max_secs = 123;
    let capped = rt.definitions_for(view(PlaceClass::Private));
    assert_eq!(rt.catalogs.built(), 3);
    assert_ne!(
        serde_json::to_string(&*capped).unwrap(),
        serde_json::to_string(&*private).unwrap()
    );
    // The public roots are what a shared place's file tools are offered under.
    rt.public_roots = vec!["/srv/open".into()];
    let rooted = rt.definitions_for(view(PlaceClass::Shared));
    assert_eq!(rt.catalogs.built(), 4);
    assert!(names(&rooted).len() >= names(&shared).len());
    // And the first is still there, not rebuilt.
    rt.public_roots = vec![];
    rt.job_wait_max_secs = crate::config::default_job_wait_max_secs();
    let back = rt.definitions_for(view(PlaceClass::Private));
    assert!(std::ptr::eq(back.as_ptr(), private.as_ptr()));
    assert_eq!(rt.catalogs.built(), 4);
}
