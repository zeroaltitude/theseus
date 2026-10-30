//! `profile.release` (theseus-ervz9, route.v10 row 5): the runtime live profile
//! given back. `profile use` is an operator's pin when route.v10 acts, so a
//! daemon that once ran `profile use sonnet` reads every unpinned turn as pinned
//! and Jev is never asked for a model. The release sets the live profile back
//! to the config's `[model] live` (source `config`), and it persists across a
//! restart exactly as the `use` it undoes.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::ProfileChanged;

use super::tests::roundtrip;
use super::tests_live_pin::{ask, result};
use super::{Core, Parts};
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::tests_route::{call, decided};
use crate::tests_route_open::{open, says};
use crate::Config;

const DESIGN: &str = "design the memory layer for a million turns";

fn config() -> Config {
    let mut c = Config::example();
    c.serve_glm_unrestricted();
    c
}

fn core_on(store: Store) -> Arc<Core> {
    Core::build(Parts::for_tests(
        config(),
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap()
}

fn changed(v: Value) -> ProfileChanged {
    serde_json::from_value(v).unwrap()
}

/// The release names what it replaced and what stands now, and the live profile
/// is the config's again, its source `config`, so `profile list` says so.
#[tokio::test]
async fn a_release_returns_the_runtime_profile_to_the_configs_live() {
    let dir = crate::scratch::dir("t");
    let core = core_on(Store::open(&dir.join("store")).unwrap());
    call(&core, "profile.use", json!({"name": "glm"})).await;
    assert_eq!(
        core.live_profile(),
        ("glm".to_string(), "runtime".to_string())
    );
    let c = changed(call(&core, "profile.release", Value::Null).await);
    assert_eq!((c.previous.as_str(), c.live.as_str()), ("glm", "sonnet"));
    assert_eq!(
        core.live_profile(),
        ("sonnet".to_string(), "config".to_string())
    );
    let list = call(&core, "profile.list", Value::Null).await;
    assert_eq!(list["live"], "sonnet");
    assert_eq!(list["live_source"], "config");
    assert_eq!(core.health().profile, "sonnet");
}

/// A release is a profile change like any other: its ledger row, so a reader of
/// the record sees the pin end.
#[tokio::test]
async fn a_release_is_a_ledger_row() {
    let dir = crate::scratch::dir("t");
    let core = core_on(Store::open(&dir.join("store")).unwrap());
    call(&core, "profile.use", json!({"name": "glm"})).await;
    call(&core, "profile.release", Value::Null).await;
    let rows = crate::test_support::rows(&core, "profile.changed");
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[1]["previous"], "glm");
    assert_eq!(rows[1]["live"], "sonnet");
}

/// Persisted as `use` is: a daemon that restarts after a release comes up on the
/// config's profile, not on the pin the release ended; and a `use` after the
/// release is a pin again, across a restart too.
#[tokio::test]
async fn a_release_persists_across_a_restart() {
    let dir = crate::scratch::dir("t");
    let store = Store::open(&dir.join("store")).unwrap();
    let core = core_on(store.clone());
    call(&core, "profile.use", json!({"name": "glm"})).await;
    call(&core, "profile.release", Value::Null).await;
    drop(core);
    let core2 = core_on(store.clone());
    assert_eq!(
        core2.live_profile(),
        ("sonnet".to_string(), "config".to_string())
    );
    call(&core2, "profile.use", json!({"name": "glm"})).await;
    drop(core2);
    let core3 = core_on(store);
    assert_eq!(
        core3.live_profile(),
        ("glm".to_string(), "runtime".to_string())
    );
}

/// Nothing to release: the answer says the live profile did not change, no row
/// is written, and the source stays `config`.
#[tokio::test]
async fn releasing_nothing_changes_nothing() {
    let dir = crate::scratch::dir("t");
    let core = core_on(Store::open(&dir.join("store")).unwrap());
    let c = changed(call(&core, "profile.release", Value::Null).await);
    assert_eq!((c.previous.as_str(), c.live.as_str()), ("sonnet", "sonnet"));
    assert_eq!(
        core.live_profile(),
        ("sonnet".to_string(), "config".to_string())
    );
    assert!(crate::test_support::rows(&core, "profile.changed").is_empty());
}

/// The whole point: under route.v10, `profile use haiku` pins (`reason =
/// pinned`) and Jev is not asked the model; after the release the same turn goes
/// to the model Jev names (`reason = verdict`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_a_release_jev_chooses_again() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "high", "full", 0.97);
    let o = open(&jev, 2, |_| {});
    let core = &o.r.core;
    call(core, "profile.use", json!({"name": "haiku"})).await;
    let pinned = result(
        &roundtrip(core.clone(), vec![ask(1, DESIGN, None)]).await,
        1,
    );
    assert_eq!(pinned.profile, "haiku");
    assert_eq!(pinned.route.as_ref().unwrap().reason, "pinned");
    call(core, "profile.release", Value::Null).await;
    assert_eq!(core.live_profile().1, "config");
    let chose = result(
        &roundtrip(core.clone(), vec![ask(2, DESIGN, None)]).await,
        2,
    );
    assert_eq!(chose.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(chose.profile, "fable");
    let d = decided(&core.store);
    assert_eq!(d.last().unwrap()["reason"], "verdict");
}
