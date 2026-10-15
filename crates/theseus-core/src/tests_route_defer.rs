//! A detour on a model without the provider's tool search reaches the
//! deferred tool its message names (theseus-zs6mm). install #31: the owner's
//! cli sent 7,318 tokens, and a quick message still got Haiku `unsearched_call`
//! misses, because the router's detour is a request of its own: the board
//! kept the note of the request the detour replaced, and the miss rule read
//! that note. The rig is `tests_route`'s, with deferral on and the Haiku
//! stand-in as the `anthropic` provider.

use std::sync::Arc;

use serde_json::json;
use theseus_judge::fake::FakeJev;

use crate::provider::{FakeProvider, Scripted};
use crate::tests_places::{offered, rows};
use crate::tests_route::{mode, rig_parts, turn, Rig};

/// A rig whose `anthropic` provider is a stand-in that, asked, calls
/// `thread_read` at once and never `tools_find`, then answers.
fn rig_detouring(jev: &FakeJev) -> (Rig, Arc<FakeProvider>) {
    let haiku = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("t1", "thread_read", json!({}))]),
        Scripted::text("It returned your newest thread."),
    ]));
    let p = haiku.clone();
    let r = rig_parts(
        Some(jev),
        2,
        |_| {},
        move |parts| {
            parts.cfg.tools.defer.enabled = true;
            parts.providers.insert("anthropic".into(), p);
        },
    );
    (r, haiku)
}

/// The owner's prompt of install #31, judged trivial, so the router detours to
/// Haiku 5.5 (whose catalog row has no `tool_search`): the detour's request
/// offers the tool the message names, the call runs, and nothing is a miss.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detour_without_tool_search_reaches_a_tool_the_message_names() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let (r, haiku) = rig_detouring(&jev);
    let res = turn(
        &r.core,
        None,
        "Call the thread_read tool once (it is read-only) to read my most recent thread, then tell me in one line what it returned.",
        None,
    )
    .await;
    let route = res.route.as_ref().unwrap();
    assert_eq!(
        (res.profile.as_str(), route.reason.as_str()),
        ("haiku", "detour")
    );
    let names = offered(&haiku.requests()[0]);
    assert!(names.contains(&"thread_read".to_string()), "{names:?}");
    assert_eq!(res.tool_calls, 1, "the call reached the tool");
    assert!(rows(&r.core, "tools.missed").is_empty());
    let used = rows(&r.core, "tools.used");
    assert_eq!(used.len(), 1, "{used:?}");
    assert_eq!(used[0]["mechanism"], "find", "{:?}", used[0]);
}
