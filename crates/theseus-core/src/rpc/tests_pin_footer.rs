//! What the reply footer's `pinned` reads (theseus-h66v4.3): the route's
//! recorded `reason`, set from the turn's own pin and never from the model
//! matching a default. The config's live profile is `sonnet`, and every turn
//! below runs Sonnet 5.5, pinned and not, so a footer that guessed from the
//! model could pass only one of them. This is theseus-dq3m1's shape: a pin and
//! the config's default are one name.

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::TurnSubmitResult;

use super::tests::roundtrip;
use super::tests_live_pin::{ask, result};
use crate::tests_route::call;
use crate::tests_route_open::{open, says};

const DESIGN: &str = "design the memory layer for a million turns";

/// One turn, answered, as the route routing recorded for it.
async fn turn(core: &std::sync::Arc<crate::rpc::Core>, profile: Option<&str>) -> TurnSubmitResult {
    let res = result(
        &roundtrip(core.clone(), vec![ask(1, DESIGN, profile)]).await,
        1,
    );
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("sonnet", "claude-sonnet-5-5"),
        "the default's model, pinned or not"
    );
    res
}

/// `profile use sonnet` on a daemon whose config's live is `sonnet`: the model
/// is the default's, and the turn is pinned all the same.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pin_on_the_configs_own_model_reads_pinned() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.97);
    let o = open(&jev, 1, |_| {});
    assert_eq!(o.r.core.live_profile().0, "sonnet");
    call(&o.r.core, "profile.use", json!({"name": "sonnet"})).await;
    let route = turn(&o.r.core, None).await.route.unwrap();
    assert_eq!(route.reason, "pinned");
    assert_eq!(route.pin_footer(), Some("pinned"));
}

/// The same turn with an explicit `-P sonnet`: pinned, whatever the live
/// profile is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_profile_on_the_default_model_reads_pinned() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.97);
    let o = open(&jev, 1, |_| {});
    let route = turn(&o.r.core, Some("sonnet")).await.route.unwrap();
    assert_eq!(route.pin_footer(), Some("pinned"));
}

/// No pin, and Jev picks Sonnet: the same profile and model, chosen. Nothing
/// to say.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_model_chosen_by_jev_reads_unpinned() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.97);
    let o = open(&jev, 1, |_| {});
    let route = turn(&o.r.core, None).await.route.unwrap();
    assert_ne!(route.reason, "pinned");
    assert_eq!(route.pin_footer(), None);
}

/// A pin released leaves `live == previous == sonnet` (the config's own): the
/// turn after it is no pin. This is the case that read as pinned when the
/// answer was inferred from the names (theseus-dq3m1).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_released_pin_on_the_default_model_reads_unpinned() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.97);
    let o = open(&jev, 1, |_| {});
    call(&o.r.core, "profile.use", json!({"name": "sonnet"})).await;
    let released = call(&o.r.core, "profile.release", Value::Null).await;
    assert_eq!(released["released"], true, "{released}");
    let route = turn(&o.r.core, None).await.route.unwrap();
    assert_ne!(route.reason, "pinned");
    assert_eq!(route.pin_footer(), None);
}
