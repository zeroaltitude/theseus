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
    rig_detouring_to(jev, "thread_read", false)
}

/// The same with the tool the stand-in calls named, and, when `aws`, an account configured so the
/// `aws_*` tools are in the catalog.
fn rig_detouring_to(jev: &FakeJev, tool: &str, aws: bool) -> (Rig, Arc<FakeProvider>) {
    let haiku = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("t1", tool, json!({}))]),
        Scripted::text("It returned your newest thread."),
    ]));
    let p = haiku.clone();
    let r = rig_parts(
        Some(jev),
        2,
        move |cfg| {
            cfg.places.owner = Some(vec![format!("discord:{}", crate::tests_places::OWNER)]);
            // A trivial detour is the small tier, at 1.0 (theseus-d9je7): the raise this rig
            // proves is held at today's 2.0.
            cfg.tools.defer.admit_ceiling_small = 2.0;
            if aws {
                cfg.aws = toml::from_str(
                    "[accounts.123456789012]\nregion = \"us-west-2\"\nendpoint = \"http://127.0.0.1:9\"\n",
                )
                .unwrap();
            }
        },
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
        ("haiku", "verdict")
    );
    let names = offered(&haiku.requests()[0]);
    assert!(names.contains(&"thread_read".to_string()), "{names:?}");
    assert_eq!(res.tool_calls, 1, "the call reached the tool");
    assert!(rows(&r.core, "tools.missed").is_empty());
    let used = rows(&r.core, "tools.used");
    assert_eq!(used.len(), 1, "{used:?}");
    assert_eq!(used[0]["mechanism"], "find", "{:?}", used[0]);
}

/// theseus-3msy0: the AWS tools are 6,869 tokens whole, more than any place can add, so a detour that
/// named `aws_whoami` loaded nothing and Haiku never reached it. The tools are six families by use
/// now: the message loads the one that holds the tool, and the request's tools stay within the cli's
/// budget, as the rule for a named family counts them (`want::extra`).
///
/// theseus-xlri5: back at the cli. At the cli a detour's request is about 7,300 tokens before any
/// named family (theseus-3bs4e), so the AWS family it names passes the budget: it is admitted past it
/// by its own size, under the ceiling, and the result's `raised` names it, `by: named`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget() {
    let jev = FakeJev::start().unwrap();
    // A quick detour: a trivial one carries `tools_find` alone (theseus-d9je7).
    mode(&jev, "quick", 0.95);
    let (r, haiku) = rig_detouring_to(&jev, "aws_whoami", true);
    let res = turn(
        &r.core,
        None,
        "Call the aws_whoami tool once and tell me in one line which account you are on.",
        None,
    )
    .await;
    let route = res.route.as_ref().unwrap();
    assert_eq!(
        (res.profile.as_str(), route.reason.as_str()),
        ("haiku", "verdict")
    );
    let first = &haiku.requests()[0];
    let names = offered(first);
    assert!(names.contains(&"aws_whoami".to_string()), "{names:?}");
    assert!(
        !names
            .iter()
            .any(|n| n.starts_with("aws_s3_") || n == "aws_hands_run"),
        "only the family that holds the tool loads: {names:?}"
    );
    let tokens = crate::tools_defer::tools_bytes(&first.tools) * 10 / 24;
    let budget = crate::first_request::CLI_TASK_TOKENS;
    assert!(
        tokens <= budget,
        "the request's tools are {tokens} tokens, over {budget}"
    );
    assert_eq!(res.tool_calls, 1, "the call reached the tool");
    assert!(rows(&r.core, "tools.missed").is_empty());
    let raised = res
        .raised
        .as_ref()
        .expect("the AWS family raised the request");
    let by: Vec<(&str, &str)> = raised
        .families
        .iter()
        .map(|f| (f.family.as_str(), f.by.as_str()))
        .collect();
    assert_eq!(by, [("aws", "named")], "{raised:?}");
}
