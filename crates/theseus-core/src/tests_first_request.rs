//! The first request's measure, in tokens as sent (theseus-to0xw): the census's own number, for every kind of
//! place, each counted with its own families.

use std::sync::Arc;

use theseus_protocol::census::PLACES;

use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

fn rig() -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    // The daemon's own defaults, as the bench's scratch daemon runs them (the template's every section adds
    // AWS, GitHub and MCP tools).
    let mut cfg = Config::parse("[secrets]\nanthropic_api_key = \"env:THESEUS_TEST_KEY\"\n")
        .unwrap()
        .0;
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    let mut parts = crate::rpc::Parts::for_tests(cfg, fake, store);
    // Tests build with deferral off; a daemon has it on.
    parts.cfg.tools.defer.enabled = true;
    let core = Core::build(parts).unwrap();
    (core, dir)
}

/// The measure is what `theseus census` prints as the request's total, for every kind of place it counts.
#[test]
fn the_measure_is_the_censuss_own_total() {
    let (core, _dir) = rig();
    for place in PLACES {
        let (census, _, _) = core
            .context_census(&theseus_protocol::census::ContextCensusParams {
                session_id: None,
                place: Some(place.to_string()),
                exact: false,
            })
            .unwrap();
        let first = core.first_request(place).unwrap();
        assert_eq!(first.tokens, census.total_tokens, "{place}");
        assert!(first.tokens > census.system_tokens, "{place}: tools count");
    }
}

/// A kind of place loads its own families: a DM's request is not the CLI's (the census once counted
/// every fresh session as the CLI's, whatever `--place` said).
#[test]
fn each_kind_of_place_is_counted_with_its_own_families() {
    let (core, _dir) = rig();
    let loaded = |p: &str| core.first_request(p).unwrap().loaded;
    assert!(loaded("dm").contains(&"task_create".to_string()));
    assert!(loaded("dm").contains(&"http_fetch".to_string()));
    assert!(!loaded("cli").contains(&"task_create".to_string()));
    assert!(!loaded("task").contains(&"memory_lookup".to_string()));
    assert!(loaded("headless").contains(&"term_open".to_string()));
    assert!(
        !loaded("channel").contains(&"fs_read".to_string()),
        "a channel is offered the public tools"
    );
}
